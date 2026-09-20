//! Çekirdeğin sahibi olan iş parçacığı ve ona iş gönderme yolu.
//!
//! **Altın Kural:** burada iş mantığı yok. Bu dosya bir kanal, bir kurucu ve
//! bir hata zarfından ibaret.
//!
//! ## Neden kilit değil de kendi iş parçacığı
//!
//! Ölçülen kısıt: `Session` **`Send` ama `Sync` değil** — SQLite bağlantısı
//! `RefCell` taşıyor — ve `import_archive`'ın döndürdüğü future `Send`
//! değil (`Box<dyn ExportArchive>` iş parçacıkları arası geçmiyor). Tauri
//! ise her async komutun future'ının `Send` olmasını istiyor.
//!
//! `Mutex<Core>` bunu çözmüyor: kilidi bir `.await` üzerinden taşımak
//! `Core: Sync` ister ve `Core` `Sync` değil. Çözüm çekirdeği tek bir iş
//! parçacığına yerleştirmek: **hiçbir çekirdek tipi iş parçacığı sınırını
//! geçmiyor**, yalnızca iş kapanışları ve seri hâle getirilebilir sonuçlar
//! geçiyor.
//!
//! Yan faydası: tik döngüsü de aynı iş parçacığında yaşıyor, yani komutlarla
//! `tick()` arasında kilit yarışı yok — sıraya kanal koyuyor.
//!
//! **Bedeli görünür:** uzun bir `import` sürerken oynatma kumandaları da
//! sırada bekler. Sessiz kalmasın diye uzun komutlar `tonearm://busy` olayı
//! gönderiyor (K9).

use std::future::Future;
use std::pin::Pin;

use serde::Serialize;
use tokio::sync::{mpsc, oneshot};

use tonearm_core::config::Config;
use tonearm_core::diag::Stage;
use tonearm_core::playback::{LiveSession, Player};
use tonearm_core::provider::ProviderRegistry;
use tonearm_core::session::Session;

use crate::theme::ThemeStore;

/// Çekirdek iş parçacığının sahip olduğu her şey.
pub struct Core {
    pub live: LiveSession,
    /// Sağlayıcı kaydı. Sunucu eklenip silindiğinde [`Core::refresh_registry`]
    /// ile yenileniyor: yoksa yeni sunucu uygulama kapanana kadar görünmezdi.
    pub registry: ProviderRegistry,
}

impl Core {
    /// Kütüphaneyi açar, sağlayıcıları kurar ve **boş** bir oynatıcıyla başlar.
    ///
    /// Boş oynatıcı bir yer tutucu değil: `LiveSession` hep var olsun ki
    /// komutlar "oturum açık mı" diye sormak zorunda kalmasın. Çalan bir şey
    /// yokken `anchor` zaten `Stopped` döner.
    ///
    /// # Errors
    /// Veri dizini açılamazsa, veritabanı kurulamazsa ya da kayıtlı sunucu
    /// dosyası bozuksa.
    pub fn open(config: Config) -> tonearm_core::Result<Self> {
        let session = Session::open(config)?;
        let registry = tonearm_core::provider::default_registry(session.config())?;
        let player = Player::new(registry.clone());
        Ok(Self {
            live: LiveSession::new(session, player),
            registry,
        })
    }

    /// Sunucu listesi değiştikten sonra kaydı yeniden kurar.
    ///
    /// # Errors
    /// Kayıtlı sunucu dosyası okunamazsa.
    pub fn refresh_registry(&mut self) -> tonearm_core::Result<()> {
        self.registry = tonearm_core::provider::default_registry(self.live.session().config())?;
        Ok(())
    }
}

/// Çekirdek iş parçacığına gönderilen bir iş.
///
/// Kapanış `&mut Core` ödünç alıyor ve kendi sonucunu kendi `oneshot`'ına
/// yazıyor. Böylece komut başına bir enum varyantı yazmaya gerek kalmıyor —
/// 23 varyantlık bir mesaj tipi, D-033'ün reddettiği çevirmen katmanının
/// başka bir kılığı olurdu.
pub type Job = Box<
    dyn for<'a> FnOnce(&'a mut Core) -> Pin<Box<dyn Future<Output = ()> + 'a>> + Send + 'static,
>;

/// Tauri'nin yönettiği durum: çekirdeğe giden kanalın ucu.
pub struct AppState {
    jobs: mpsc::UnboundedSender<Job>,
    /// Tema deposu (§3.3). Çekirdek iş parçacığına **girmiyor**: tema bir
    /// çekirdek kavramı değil (bkz. [`crate::theme`]) ve uzun bir `import`
    /// sürerken arayüzün temasını değiştirememek için bir sebep yok.
    themes: ThemeStore,
}

impl AppState {
    #[must_use]
    pub const fn new(jobs: mpsc::UnboundedSender<Job>, themes: ThemeStore) -> Self {
        Self { jobs, themes }
    }

    #[must_use]
    pub const fn themes(&self) -> &ThemeStore {
        &self.themes
    }

    /// Bir işi çekirdek iş parçacığında çalıştırır ve sonucunu bekler.
    ///
    /// # Errors
    /// Çekirdek hata döndürürse, iş parçacığı düşmüşse ya da iş cevap
    /// vermeden bitmişse.
    pub async fn run_on_core<T, F>(&self, task: F) -> CommandResult<T>
    where
        T: Send + 'static,
        F: for<'a> FnOnce(
                &'a mut Core,
            )
                -> Pin<Box<dyn Future<Output = tonearm_core::Result<T>> + 'a>>
            + Send
            + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let job: Job = Box::new(move |core| {
            Box::pin(async move {
                let result = task(core).await;
                // Alıcı gitmişse komut iptal edilmiş demektir; iş yine de
                // yapıldı ve çekirdeğin durumu tutarlı.
                let _ = tx.send(result);
            })
        });
        self.jobs.send(job).map_err(|_| core_thread_gone())?;
        rx.await
            .map_err(|_| core_thread_gone())?
            .map_err(CommandError::from)
    }
}

/// Çekirdek iş parçacığı kaybolduysa. Sessizce boş sonuç dönmüyoruz (K9).
fn core_thread_gone() -> CommandError {
    CommandError::new(Stage::ConfigLoad, "çekirdek iş parçacığı yanıt vermiyor")
}

/// Webview'e giden hata.
///
/// D-033 **veri** için ayrı bir IPC tipi yasakladı; bu bir veri tipi değil,
/// hata zarfı. `tonearm_core::Error` seri hâle getirilemiyor (kaynak zinciri
/// `dyn Error` taşıyor), ama kaybedilen bir şey yok: aşama ve tam zincir
/// metni geçiyor — CLI'nin `stderr`'e bastığının aynısı.
#[derive(Debug, Clone, Serialize)]
pub struct CommandError {
    /// `IDENTITY_RESOLVE` gibi sabit aşama adı. Arayüz buna göre yönlendirir.
    pub stage: Stage,
    /// `ADIM: ...` ile başlayan, kopyalanıp yapıştırılabilir tam zincir.
    pub chain: String,
}

impl CommandError {
    /// Çekirdekten gelmeyen bir hata için zarf üretir — biçim çekirdeğinkiyle
    /// aynı olsun diye tek yerden (`ADIM: <aşama>` + girintili sebep).
    #[must_use]
    pub fn new(stage: Stage, text: &str) -> Self {
        Self {
            stage,
            chain: format!("ADIM: {stage}\n  {text}"),
        }
    }
}

impl From<tonearm_core::Error> for CommandError {
    fn from(err: tonearm_core::Error) -> Self {
        Self {
            stage: err.stage(),
            chain: err.chain_text(),
        }
    }
}

/// Komutların dönüş tipi.
pub type CommandResult<T> = std::result::Result<T, CommandError>;
