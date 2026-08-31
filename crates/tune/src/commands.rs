//! IPC komutları (D-033).
//!
//! **Sözleşme = çekirdeğin yüzeyi + serde.** Buradaki hiçbir komut yeni bir
//! veri şekli uydurmuyor; her biri var olan bir çekirdek tipini döndürüyor ve
//! `serde` ile geçiyor. CLI'nin `--json` çıktısıyla **aynı** veri.
//!
//! Komut listesi CLI'nin alt komutlarıyla birebir örtüşüyor, çünkü ikisi de
//! aynı çekirdeğin kabuğu. Burada yapılan tek şey: argümanı çekirdeğe
//! iletmek, sonucu geri vermek.
//!
//! Her komut gövdesi çekirdek iş parçacığında koşuyor ([`crate::state`]),
//! o yüzden hepsi `state.calistir(...)` ile sarılı. Bu bir katman değil, bir
//! adres: işin nerede yapılacağını söylüyor.
//!
//! **Uzun komutlar** (`import`, `resolve`, `provider_scan`, `provider_test`,
//! `server_add`, `play`) sırayı tutar ve o sırada oynatma kumandaları bekler.
//! Bu görünmez kalmasın diye [`BUSY_EVENT`] gönderiyorlar: arayüz hangi işin
//! sürdüğünü yazar (K9).

use std::future::Future;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use tune_core::ids::ProviderId;
use tune_core::model::PlayRule;
use tune_core::playback::{PlaybackAnchor, QueueView, RepeatMode};
use tune_core::provider::remote::{self, NewServer, ServerKind};
use tune_core::session::{
    self, ImportReport, PlayOptions, ProviderListReport, ProviderTestReport, ResolveReport,
    ScanReport, SearchReport, ServerAddReport, ServerListReport, ServerRemoveReport, StatsResponse,
    WrappedResponse,
};
use tune_core::stats::StatsQuery;
use tune_core::wrapped::CardPreset;

use crate::state::{AppState, CommandResult};

/// Uzun bir işin başladığını/bittiğini webview'e bildirir.
///
/// Olay adı `tune://busy`, yükü işin adı (bitişte `null`). Zamanlayıcıyla
/// değil durum değişiminde gidiyor — D-033'ün kuralı burada da geçerli.
pub const BUSY_EVENT: &str = "tune://busy";

/// Uzun süren bir çekirdek çağrısını meşguliyet olayıyla sarar.
async fn busy<T, F>(app: &AppHandle, ne: &str, is: F) -> tune_core::Result<T>
where
    F: Future<Output = tune_core::Result<T>>,
{
    let _ = app.emit(BUSY_EVENT, Some(ne));
    let sonuc = is.await;
    // Hata yolunda da kapanmalı: yoksa arayüz sonsuza kadar meşgul görünürdü.
    let _ = app.emit(BUSY_EVENT, None::<&str>);
    sonuc
}

// ————————————————————————————————————— Kütüphane

#[tauri::command]
pub async fn search(
    state: State<'_, AppState>,
    query: String,
    limit: usize,
    min_ms: Option<u64>,
) -> CommandResult<SearchReport> {
    state
        .calistir(move |core| {
            Box::pin(async move {
                let rule = min_ms.map_or_else(PlayRule::default, PlayRule::new);
                core.live.session().search(&query, limit, rule)
            })
        })
        .await
}

#[tauri::command]
pub async fn stats(
    state: State<'_, AppState>,
    year: Option<i16>,
    top: usize,
    min_ms: Option<u64>,
) -> CommandResult<StatsResponse> {
    state
        .calistir(move |core| {
            Box::pin(async move {
                let query = StatsQuery {
                    year,
                    top,
                    min_ms_played: min_ms.unwrap_or(tune_core::stats::DEFAULT_MIN_MS_PLAYED),
                };
                core.live.session().stats(query)
            })
        })
        .await
}

/// Wrapped kartı. `out` verilirse dosyaya da yazılır (uzantı biçimi belirler).
#[tauri::command]
pub async fn wrapped(
    state: State<'_, AppState>,
    year: Option<i16>,
    story: bool,
    out: Option<String>,
) -> CommandResult<WrappedResponse> {
    state
        .calistir(move |core| {
            Box::pin(async move {
                let query = StatsQuery {
                    year,
                    top: 10,
                    min_ms_played: tune_core::stats::DEFAULT_MIN_MS_PLAYED,
                };
                let size = if story {
                    CardPreset::Story.size()
                } else {
                    CardPreset::Square.size()
                };
                let path = out.map(std::path::PathBuf::from);
                core.live.session().wrapped(query, size, path.as_deref())
            })
        })
        .await
}

// ————————————————————————————————————— İçe aktarma ve kimlik

#[tauri::command]
pub async fn import(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<ImportReport> {
    state
        .calistir(move |core| {
            Box::pin(async move {
                let path = std::path::PathBuf::from(path);
                busy(
                    &app,
                    "içe aktarılıyor",
                    core.live
                        .session_mut()
                        .import_archive(&path, session::default_lookup()),
                )
                .await
            })
        })
        .await
}

#[tauri::command]
pub async fn resolve(
    app: AppHandle,
    state: State<'_, AppState>,
    query: String,
) -> CommandResult<ResolveReport> {
    state
        .calistir(move |core| {
            Box::pin(async move {
                busy(
                    &app,
                    "kimlik çözümleniyor",
                    core.live
                        .session_mut()
                        .resolve_track(&query, session::default_lookup()),
                )
                .await
            })
        })
        .await
}

// ————————————————————————————————————— Sağlayıcılar

#[tauri::command]
pub async fn providers(state: State<'_, AppState>) -> CommandResult<ProviderListReport> {
    state
        .calistir(move |core| {
            Box::pin(async move { core.live.session().providers(&core.registry) })
        })
        .await
}

#[tauri::command]
pub async fn provider_test(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> CommandResult<ProviderTestReport> {
    state
        .calistir(move |core| {
            Box::pin(async move {
                let id = ProviderId::new(name);
                busy(
                    &app,
                    "sağlayıcı sınanıyor",
                    core.live.session().test_provider(&core.registry, &id),
                )
                .await
            })
        })
        .await
}

#[tauri::command]
pub async fn provider_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    if_stale: bool,
) -> CommandResult<ScanReport> {
    state
        .calistir(move |core| {
            Box::pin(async move {
                // Kayıt `Arc` taşıyor; klon ucuz ve `core`'u ikiye bölmekten
                // (bir yanı `&mut Session`, öbürü `&ProviderRegistry`)
                // okunaklı.
                let registry = core.registry.clone();
                let session = core.live.session_mut();
                let is = async {
                    if if_stale {
                        session.scan_providers_if_stale(&registry).await
                    } else {
                        session.scan_providers(&registry).await
                    }
                };
                busy(&app, "kütüphane taranıyor", is).await
            })
        })
        .await
}

#[tauri::command]
pub async fn servers_list(state: State<'_, AppState>) -> CommandResult<ServerListReport> {
    state
        .calistir(move |core| Box::pin(async move { core.live.session().list_servers() }))
        .await
}

/// Uzak sunucu ekler.
///
/// Parola IPC'den geliyor — CLI'de olduğu gibi komut satırına yazılmıyor,
/// `ps` çıktısına ya da kabuk geçmişine düşmüyor. Kalıcı saklama yine
/// çekirdeğin işi (`0600` izinli `servers.json`); keyring borcu Faz 2'de
/// (D-021).
#[tauri::command]
#[expect(
    clippy::too_many_arguments,
    reason = "CLI'nin `provider add` argümanlarıyla birebir; bir 'istek' tipi uydurmak D-033'ün reddettiği çevirmen katmanı olurdu"
)]
pub async fn server_add(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: String,
    url: String,
    user: String,
    password: Option<String>,
    api_key: Option<String>,
    name: Option<String>,
    verify: bool,
) -> CommandResult<ServerAddReport> {
    state
        .calistir(move |core| {
            Box::pin(async move {
                let kind = ServerKind::parse(&kind)?;
                // Ad önerisi çekirdekten: CLI de aynısını gösteriyor.
                let id = name.map_or_else(|| remote::suggest_id(&url, kind), ProviderId::new);
                let spec = NewServer {
                    id,
                    kind,
                    url,
                    username: user,
                    password: if api_key.is_some() { None } else { password },
                    api_key,
                    verify,
                };
                let is = async {
                    let http = tune_core::net::default_http_client()?;
                    core.live.session_mut().add_server(spec, http).await
                };
                let report = busy(&app, "sunucu doğrulanıyor", is).await?;
                // Yeni sunucu hemen çalınabilir olsun: kayıt yenilenmezse
                // uygulama kapanana kadar görünmezdi.
                core.refresh_registry()?;
                Ok(report)
            })
        })
        .await
}

#[tauri::command]
pub async fn server_remove(
    state: State<'_, AppState>,
    name: String,
) -> CommandResult<ServerRemoveReport> {
    state
        .calistir(move |core| {
            Box::pin(async move {
                let report = core.live.session().remove_server(&ProviderId::new(name))?;
                core.refresh_registry()?;
                Ok(report)
            })
        })
        .await
}

// ————————————————————————————————————— Oynatma

/// Arar, kuyruğa alır ve çalmaya başlar. **Beklemez.**
///
/// `Session::play` bloklayan yol (CLI'nin `tune play`'i); GUI onu değil
/// `player_from_search`'ü kullanıyor ve döngüyü çekirdek iş parçacığı sürüyor.
#[tauri::command]
pub async fn play(
    app: AppHandle,
    state: State<'_, AppState>,
    query: String,
    all: bool,
    shuffle: bool,
) -> CommandResult<QueueView> {
    state
        .calistir(move |core| {
            Box::pin(async move {
                let options = PlayOptions {
                    all,
                    shuffle,
                    ..PlayOptions::new(&query)
                };
                let is = core
                    .live
                    .session()
                    .player_from_search(&core.registry, options);
                let player = busy(&app, "parça aranıyor", is).await?;
                // Eski oynatıcının biriken dinlemeleri **alınıyor**, atılmıyor.
                core.live.replace_player(player);
                Ok(core.live.player().queue().view())
            })
        })
        .await
}

#[tauri::command]
pub async fn toggle_pause(state: State<'_, AppState>) -> CommandResult<PlaybackAnchor> {
    state
        .calistir(move |core| {
            Box::pin(async move {
                // "Duraklat mı sürdür mü" kararı `Player::toggle_pause` içinde.
                core.live.player().toggle_pause();
                Ok(core.live.anchor())
            })
        })
        .await
}

#[tauri::command]
pub async fn stop(state: State<'_, AppState>) -> CommandResult<PlaybackAnchor> {
    state
        .calistir(move |core| {
            Box::pin(async move {
                core.live.player_mut().stop();
                Ok(core.live.anchor())
            })
        })
        .await
}

#[tauri::command]
pub async fn next(state: State<'_, AppState>) -> CommandResult<QueueView> {
    state
        .calistir(move |core| {
            Box::pin(async move {
                core.live.player_mut().next().await?;
                Ok(core.live.player().queue().view())
            })
        })
        .await
}

#[tauri::command]
pub async fn previous(state: State<'_, AppState>) -> CommandResult<QueueView> {
    state
        .calistir(move |core| {
            Box::pin(async move {
                core.live.player_mut().previous().await?;
                Ok(core.live.player().queue().view())
            })
        })
        .await
}

#[tauri::command]
pub async fn jump_to(state: State<'_, AppState>, index: usize) -> CommandResult<QueueView> {
    state
        .calistir(move |core| {
            Box::pin(async move {
                core.live.player_mut().jump_to(index).await?;
                Ok(core.live.player().queue().view())
            })
        })
        .await
}

#[tauri::command]
pub async fn set_shuffle(state: State<'_, AppState>, on: bool) -> CommandResult<QueueView> {
    state
        .calistir(move |core| {
            Box::pin(async move {
                core.live.player_mut().queue_mut().set_shuffle(on);
                Ok(core.live.player().queue().view())
            })
        })
        .await
}

/// Tekrar kipini ayarlar. Kip adı çekirdeğin kendi yazımıyla gelir
/// (`off` | `all` | `one`) — kabukta ikinci bir eşleme tablosu tutulmuyor.
#[tauri::command]
pub async fn set_repeat(state: State<'_, AppState>, mode: RepeatMode) -> CommandResult<QueueView> {
    state
        .calistir(move |core| {
            Box::pin(async move {
                core.live.player_mut().queue_mut().set_repeat(mode);
                Ok(core.live.player().queue().view())
            })
        })
        .await
}

// ————————————————————————————————————— Durum ve tanılama

/// Şu anki çapa. Webview pozisyonu **bundan tahmin eder**, sormaz (D-015).
#[tauri::command]
pub async fn anchor(state: State<'_, AppState>) -> CommandResult<PlaybackAnchor> {
    state
        .calistir(move |core| Box::pin(async move { Ok(core.live.anchor()) }))
        .await
}

#[tauri::command]
pub async fn queue(state: State<'_, AppState>) -> CommandResult<QueueView> {
    state
        .calistir(move |core| Box::pin(async move { Ok(core.live.player().queue().view()) }))
        .await
}

#[tauri::command]
pub async fn diag(
    state: State<'_, AppState>,
) -> CommandResult<Option<tune_core::diag::DiagReport>> {
    state
        .calistir(move |core| Box::pin(async move { core.live.session().last_diag() }))
        .await
}

/// Pencere açılırken bir kez sorulan sabitler.
///
/// Yeni bir "durum" tipi değil, tanı bilgisi: hata mesajının yanında "hangi
/// kütüphaneye baktım" sorusu cevapsız kalmasın diye (K9).
#[derive(Debug, Clone, Serialize)]
pub struct Environment {
    pub data_dir: std::path::PathBuf,
    pub database: std::path::PathBuf,
    pub music_dirs: Vec<std::path::PathBuf>,
    pub version: String,
}

#[tauri::command]
pub async fn environment(state: State<'_, AppState>) -> CommandResult<Environment> {
    state
        .calistir(move |core| {
            Box::pin(async move {
                let config = core.live.session().config();
                Ok(Environment {
                    data_dir: config.data_dir().to_path_buf(),
                    database: config.database_path(),
                    music_dirs: config.music_dirs(),
                    version: env!("CARGO_PKG_VERSION").to_owned(),
                })
            })
        })
        .await
}
