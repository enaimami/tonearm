//! Çekirdeği barındıran iş parçacığı: tik döngüsü + iş kuyruğu (D-032, D-033).
//!
//! **Kabuk yalnızca döngüyü sürer.** İlerletme, dinleme yazma ve rapor üretme
//! `LiveSession::tick()` içinde — TUI ile aynı çağrı, aynı dans ikinci kez
//! yazılmıyor (Altın Kural).
//!
//! Komutlar ve tik **aynı** iş parçacığında sırayla koşuyor. Kilit yok, yarış
//! yok: kanal zaten sıralıyor. Bunun neden böyle olduğu [`crate::state`]
//! başında yazılı.
//!
//! ## Olaylar neden zamanlayıcıyla gitmiyor
//!
//! D-028 köprünün saniyede ~10.000 olay taşıdığını ölçtü; yani bu bir
//! performans önlemi değil. Sebep şu: webview pozisyonu **çapadan tahmin
//! ediyor** (D-015), o yüzden "hâlâ çalıyor" mesajının taşıdığı bilgi sıfır.
//! Yalnızca tahminin bilemeyeceği şeyler gönderiliyor — parça değişti,
//! dinleme yazıldı, depo hata verdi, kuyruk bitti, **ya da çapanın tahmine
//! girdi olan kısmı değişti** (durum, hız, süre, parça kimliği).
//!
//! Son madde D-033'ün ilk listesinde yoktu ve arayüzü sessizce dondurdu:
//! ses `Buffering` başlayıp `Playing`'e geçiyor, `Buffering` ilerlemediği
//! için ilerleme çubuğu 0:00'da kalıyordu. Ayrıntı [`dikkate_deger`].

use std::time::Duration;

use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

use tune_core::playback::TickReport;

use crate::state::{CommandError, Core, Job};

/// Tur aralığı. Ses hattından bağımsız: yalnızca "parça bitti mi" sorusu.
/// TUI ile aynı değer — iki kabuk aynı ritimde ilerlesin.
const TICK: Duration = Duration::from_millis(200);

/// Bir turda kayda değer bir şey olduğunda giden olay. Yükü `TickReport`.
pub const TICK_EVENT: &str = "tune://tick";

/// `tick()` hata döndürdüğünde giden olay. Yükü `CommandError`.
///
/// Oturum **düşürülmüyor**: hata bir parçaya ait, oturum bütününe değil.
/// Ama yutulmuyor da (K9).
pub const ERROR_EVENT: &str = "tune://error";

/// Çekirdeği kendi iş parçacığında başlatır.
///
/// # Errors
/// İş parçacığı ya da çalışma zamanı kurulamazsa.
pub fn baslat(
    core: Core,
    jobs: mpsc::UnboundedReceiver<Job>,
    app: AppHandle,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("tune-core".to_owned())
        .spawn(move || calis(core, jobs, &app))
}

fn calis(mut core: Core, mut jobs: mpsc::UnboundedReceiver<Job>, app: &AppHandle) {
    // Tek iş parçacıklı çalışma zamanı: çekirdek `Send` olmayan future'lar
    // üretiyor ve zaten hepsi burada koşacak. Çalışma zamanını **kabuk**
    // seçiyor (konvansiyon) — CLI de aynı seçimi yapıyor.
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
    {
        Ok(rt) => rt,
        Err(source) => {
            eprintln!("ADIM: CONFIG_LOAD\n  çekirdek çalışma zamanı kurulamadı: {source}");
            return;
        }
    };

    rt.block_on(async move {
        let mut zamanlayici = tokio::time::interval(TICK);
        // Gecikmiş turlar birikip peş peşe boşalmasın: geç kalındıysa
        // atlanır. `tick()` bir sayaç değil, "bir şey oldu mu" sorusu.
        zamanlayici.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        // Son gönderilen çapanın tahmin edilemeyen kısmı. Karşılaştırma
        // yapılacak ki aynı şey iki kez gönderilmesin.
        let mut son = Onemli::of(&core.live.anchor());

        loop {
            tokio::select! {
                is = jobs.recv() => match is {
                    Some(is) => is(&mut core).await,
                    // Gönderen ucun hepsi düştü: uygulama kapanıyor.
                    None => break,
                },
                _ = zamanlayici.tick() => tur(&mut core, app, &mut son).await,
            }
        }

        // Kapanış: `tick` her turda yazıyor (D-032), yani burada yazılacak
        // bir şey kalması beklenmiyor — kalırsa depo o an hata veriyordu ve
        // bu son deneme. Pencere gittiği için gösterilecek yüzey yok, ama
        // kayıp sessiz de olmamalı.
        match core.live.shutdown() {
            Ok(summary) if summary.inserted > 0 => {
                eprintln!("kapanışta yazılan dinleme: {}", summary.inserted);
            }
            Ok(_) => {}
            Err(err) => {
                eprintln!("{}", err.chain_text());
                eprintln!("elde kalan dinleme: {}", core.live.pending_listens());
            }
        }
    });
}

/// Çapanın **tahmin edilemeyen** kısmı.
///
/// Webview pozisyonu `position_ms + (now - wall_time) × rate` ile yürütüyor.
/// Bu alanlar o formülün girdisi ya da bağlamı: değiştiklerinde tahmin
/// yanlışa döner, değişmediklerinde göndermenin taşıdığı bilgi sıfırdır.
///
/// `position_ms` bilerek **yok**: tahminin işi zaten o.
#[derive(Debug, Clone, PartialEq)]
struct Onemli {
    track: Option<tune_core::ids::CanonicalId>,
    state: tune_core::playback::PlayState,
    rate: f64,
    duration_ms: Option<u64>,
}

impl Onemli {
    fn of(anchor: &tune_core::playback::PlaybackAnchor) -> Self {
        Self {
            track: anchor.track.clone(),
            state: anchor.state,
            rate: anchor.rate,
            duration_ms: anchor.duration_ms,
        }
    }
}

/// Bu tur webview'e gönderilmeli mi?
///
/// Ayrı bir fonksiyon çünkü asıl karar burada ve sınanabilir olmalı: yanlış
/// bir "hayır" arayüzü **sessizce** dondurur. Nitekim ilk hâli tam bunu yaptı
/// — `Buffering → Playing` geçişi listede yoktu, tahmin `Buffering`'de
/// ilerlemediği için ilerleme çubuğu 0:00'da kaldı ve hiçbir hata görünmedi.
fn dikkate_deger(son: &Onemli, report: &TickReport) -> bool {
    report.track_changed
        || report.listens_recorded > 0
        || report.store_error.is_some()
        || report.finished
        || Onemli::of(&report.anchor) != *son
}

/// Bir tur: çekirdeği ilerlet, yalnızca kayda değer olanı bildir.
async fn tur(core: &mut Core, app: &AppHandle, son: &mut Onemli) {
    match core.live.tick().await {
        Ok(report) => {
            if dikkate_deger(son, &report) {
                *son = Onemli::of(&report.anchor);
                let _ = app.emit(TICK_EVENT, &report);
            }
        }
        Err(err) => {
            let _ = app.emit(ERROR_EVENT, CommandError::from(err));
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use tune_core::playback::{PlayState, PlaybackAnchor};

    fn anchor(state: PlayState, position_ms: u64, duration_ms: Option<u64>) -> PlaybackAnchor {
        PlaybackAnchor {
            position_ms,
            rate: if state == PlayState::Playing {
                1.0
            } else {
                0.0
            },
            state,
            duration_ms,
            // Zaman damgası için `stopped()`: bu paket `jiff`e doğrudan
            // bağlı değil ve olmasına da gerek yok.
            ..PlaybackAnchor::stopped()
        }
    }

    fn report(anchor: PlaybackAnchor) -> TickReport {
        TickReport {
            anchor,
            track_changed: false,
            listens_recorded: 0,
            listens_pending: 0,
            store_error: None,
            finished: false,
        }
    }

    /// Kaydırma yalnızca zaman geçmesinden ibaretse gönderilmez: webview
    /// pozisyonu zaten çapadan yürütüyor.
    #[test]
    fn a_tick_that_only_advanced_time_is_not_worth_sending() {
        let son = Onemli::of(&anchor(PlayState::Playing, 1_000, Some(240_000)));
        let sonra = report(anchor(PlayState::Playing, 30_000, Some(240_000)));

        assert!(!dikkate_deger(&son, &sonra));
    }

    /// **Bu testin sebebi gerçek bir hata.** İlk sürümde yalnızca parça
    /// değişimi/dinleme/hata/bitiş gönderiliyordu. Çalma başladığında motor
    /// önce `Buffering` diyor, sonra tampon dolunca `Playing`'e geçiyor —
    /// ama o geçiş gönderilmediği için webview elindeki `Buffering` çapasıyla
    /// kalıyordu ve `Buffering` ilerlemediği için çubuk 0:00'da donuyordu.
    /// Hiçbir hata görünmüyordu; sadece "çalmıyor" gibi duruyordu.
    #[test]
    fn the_buffering_to_playing_transition_must_be_sent() {
        let son = Onemli::of(&anchor(PlayState::Buffering, 0, Some(10_000)));
        let sonra = report(anchor(PlayState::Playing, 0, Some(10_000)));

        assert!(dikkate_deger(&son, &sonra));
    }

    /// Süre kaptan geç okunabiliyor: `None` → `Some` de bir değişimdir,
    /// yoksa ilerleme çubuğu oranını hiç öğrenemez.
    #[test]
    fn learning_the_duration_later_is_a_change() {
        let son = Onemli::of(&anchor(PlayState::Playing, 0, None));
        let sonra = report(anchor(PlayState::Playing, 5_000, Some(240_000)));

        assert!(dikkate_deger(&son, &sonra));
    }

    #[test]
    fn a_recorded_listen_is_always_worth_sending() {
        let son = Onemli::of(&anchor(PlayState::Playing, 0, Some(240_000)));
        let mut sonra = report(anchor(PlayState::Playing, 1_000, Some(240_000)));
        sonra.listens_recorded = 1;

        assert!(dikkate_deger(&son, &sonra));
    }

    #[test]
    fn a_store_error_is_never_swallowed() {
        let son = Onemli::of(&anchor(PlayState::Playing, 0, Some(240_000)));
        let mut sonra = report(anchor(PlayState::Playing, 1_000, Some(240_000)));
        sonra.store_error = Some("ADIM: LIBRARY_WRITE\n  disk dolu".to_owned());

        assert!(dikkate_deger(&son, &sonra));
    }
}
