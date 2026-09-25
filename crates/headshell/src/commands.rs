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
//! o yüzden hepsi `state.run_on_core(...)` ile sarılı. Bu bir katman değil, bir
//! adres: işin nerede yapılacağını söylüyor.
//!
//! **Uzun komutlar** (`import`, `resolve`, `provider_scan`, `provider_test`,
//! `server_add`, `play`) sırayı tutar ve o sırada oynatma kumandaları bekler.
//! Bu görünmez kalmasın diye [`BUSY_EVENT`] gönderiyorlar: arayüz hangi işin
//! sürdüğünü yazar (K9).

use std::future::Future;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use headshell_core::ids::ProviderId;
use headshell_core::model::PlayRule;
use headshell_core::playback::{PlaybackAnchor, QueueView, RepeatMode};
use headshell_core::provider::remote::{self, NewServer, ServerKind};
use headshell_core::session::{
    self, ImportReport, PlayOptions, PluginCatalogReport, PluginConsentReport, PluginInstallReport,
    PluginListReport, PluginRemoveReport, PluginUpdateReport, ProviderListReport,
    ProviderTestReport, ResolveReport, ScanReport, SearchReport, SecretListReport,
    SecretWriteReport, ServerAddReport, ServerListReport, ServerRemoveReport, SleeveResponse,
    StatsResponse,
};
use headshell_core::sleeve::CardPreset;
use headshell_core::stats::StatsQuery;

use crate::state::{AppState, CommandError, CommandResult};
use crate::theme::{ActiveTheme, ThemeList};

/// Uzun bir işin başladığını/bittiğini webview'e bildirir.
///
/// Olay adı `headshell://busy`, yükü işin adı (bitişte `null`). Zamanlayıcıyla
/// değil durum değişiminde gidiyor — D-033'ün kuralı burada da geçerli.
pub const BUSY_EVENT: &str = "headshell://busy";

/// Uzun süren bir çekirdek çağrısını meşguliyet olayıyla sarar.
async fn busy<T, F>(app: &AppHandle, what: &str, task: F) -> headshell_core::Result<T>
where
    F: Future<Output = headshell_core::Result<T>>,
{
    let _ = app.emit(BUSY_EVENT, Some(what));
    let result = task.await;
    // Hata yolunda da kapanmalı: yoksa arayüz sonsuza kadar meşgul görünürdü.
    let _ = app.emit(BUSY_EVENT, None::<&str>);
    result
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
        .run_on_core(move |core| {
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
        .run_on_core(move |core| {
            Box::pin(async move {
                let query = StatsQuery {
                    year,
                    top,
                    min_ms_played: min_ms.unwrap_or(headshell_core::stats::DEFAULT_MIN_MS_PLAYED),
                };
                core.live.session().stats(query)
            })
        })
        .await
}

/// Sleeve kartı. `out` verilirse dosyaya da yazılır (uzantı biçimi belirler).
#[tauri::command]
pub async fn sleeve(
    state: State<'_, AppState>,
    year: Option<i16>,
    story: bool,
    out: Option<String>,
) -> CommandResult<SleeveResponse> {
    state
        .run_on_core(move |core| {
            Box::pin(async move {
                let query = StatsQuery {
                    year,
                    top: 10,
                    min_ms_played: headshell_core::stats::DEFAULT_MIN_MS_PLAYED,
                };
                let size = if story {
                    CardPreset::Story.size()
                } else {
                    CardPreset::Square.size()
                };
                let path = out.map(std::path::PathBuf::from);
                core.live.session().sleeve(query, size, path.as_deref())
            })
        })
        .await
}

/// Sleeve kartının **önizlemesi** — dosya yazmaz.
///
/// `sleeve` ile aynı hesabı çalıştırıp çekirdeğin kendi çizicisini
/// (`sleeve::render_svg`) döndürüyor. Kartı burada çizmek K1 ihlali olurdu:
/// aynı kartın ikinci bir çizimi JS'te yaşar ve sessizce kayardı.
#[tauri::command]
pub async fn sleeve_svg(
    app: AppHandle,
    state: State<'_, AppState>,
    year: Option<i16>,
    story: bool,
) -> CommandResult<String> {
    state
        .run_on_core(move |core| {
            Box::pin(async move {
                let query = StatsQuery {
                    year,
                    top: 10,
                    min_ms_played: headshell_core::stats::DEFAULT_MIN_MS_PLAYED,
                };
                let size = if story {
                    CardPreset::Story.size()
                } else {
                    CardPreset::Square.size()
                };
                busy(&app, "kart hazırlanıyor", async {
                    let response = core.live.session().sleeve(query, size, None)?;
                    Ok(headshell_core::sleeve::render_svg(
                        &response.data,
                        response.size,
                    ))
                })
                .await
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
        .run_on_core(move |core| {
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
        .run_on_core(move |core| {
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
        .run_on_core(move |core| {
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
        .run_on_core(move |core| {
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
        .run_on_core(move |core| {
            Box::pin(async move {
                // Kayıt `Arc` taşıyor; klon ucuz ve `core`'u ikiye bölmekten
                // (bir yanı `&mut Session`, öbürü `&ProviderRegistry`)
                // okunaklı.
                let registry = core.registry.clone();
                let session = core.live.session_mut();
                let task = async {
                    if if_stale {
                        session.scan_providers_if_stale(&registry).await
                    } else {
                        session.scan_providers(&registry).await
                    }
                };
                busy(&app, "kütüphane taranıyor", task).await
            })
        })
        .await
}

#[tauri::command]
pub async fn servers_list(state: State<'_, AppState>) -> CommandResult<ServerListReport> {
    state
        .run_on_core(move |core| Box::pin(async move { core.live.session().list_servers() }))
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
        .run_on_core(move |core| {
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
                let task = async {
                    let http = headshell_core::net::default_http_client()?;
                    core.live.session_mut().add_server(spec, http).await
                };
                let report = busy(&app, "sunucu doğrulanıyor", task).await?;
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
        .run_on_core(move |core| {
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
/// `Session::play` bloklayan yol (CLI'nin `headshell play`'i); GUI onu değil
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
        .run_on_core(move |core| {
            Box::pin(async move {
                let options = PlayOptions {
                    all,
                    shuffle,
                    ..PlayOptions::new(query)
                };
                let task = core
                    .live
                    .session()
                    .player_from_search(&core.registry, options);
                let player = busy(&app, "parça aranıyor", task).await?;
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
        .run_on_core(move |core| {
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
        .run_on_core(move |core| {
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
        .run_on_core(move |core| {
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
        .run_on_core(move |core| {
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
        .run_on_core(move |core| {
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
        .run_on_core(move |core| {
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
        .run_on_core(move |core| {
            Box::pin(async move {
                core.live.player_mut().queue_mut().set_repeat(mode);
                Ok(core.live.player().queue().view())
            })
        })
        .await
}

// ————————————————————————————————————— Tema (§3.3)
//
// Bu üç komut çekirdek iş parçacığına **girmiyor**: tema bir CSS mekanizması,
// çekirdeğin taşıdığı bir kavram değil (bkz. `crate::theme`). Bu yüzden uzun
// bir `import` sürerken de cevap veriyorlar.
//
// Aşama `CONFIG_LOAD`: veri dizininden bir yapılandırma okumak bu — çekirdeğe
// yalnızca GUI'nin ihtiyacı olan bir `THEME_LOAD` aşaması eklemek, kabuğa ait
// bir kavramı çekirdeğin tanı sözlüğüne sızdırmak olurdu.

fn theme_error(text: String) -> CommandError {
    CommandError::new(headshell_core::diag::Stage::ConfigLoad, &text)
}

/// Yüklenebilir temalar + **reddedilenler ve sebepleri** (K9).
#[tauri::command]
pub async fn themes_list(state: State<'_, AppState>) -> CommandResult<ThemeList> {
    state.themes().list().map_err(theme_error)
}

/// Kayıtlı seçimi yükler. Açılışta bir kez çağrılıyor.
#[tauri::command]
pub async fn theme_active(state: State<'_, AppState>) -> CommandResult<ActiveTheme> {
    state.themes().active().map_err(theme_error)
}

/// Temayı seçer ve kalıcı yazar. `id` yoksa varsayılana döner.
#[tauri::command]
pub async fn theme_select(
    state: State<'_, AppState>,
    id: Option<String>,
) -> CommandResult<ActiveTheme> {
    state.themes().select(id).map_err(theme_error)
}

// ————————————————————————————————————— Eklentiler (Faz 2)
//
// Eklenti yüzeyi GUI'ye Faz 3'ten **sonra** açıldı: arayüz yazıldığında Faz 2
// henüz ertelenmişti (D-027) ve kabuk onun yeteneklerinden habersiz kaldı.
// Komutlar CLI'nin `headshell plugin ...` alt komutlarıyla birebir aynı çekirdek
// çağrılarını yapıyor — ikisi de aynı çekirdeğin kabuğu.
//
// İzinlerin ne kadarının zorlandığı her listede `permissions_enforced`
// alanıyla yazıyor (D-040 → D-069). Arayüz bunu gizlemez: olmayan bir
// korumaya güven verilmez, var olanın sınırı da söylenir.
//
// Katalog (D-071) yalnızca kullanıcı isteyince okunur: panel açılınca değil,
// "kataloğu getir"e basınca. Ağa çıkan her komut meşguliyet olayı gönderir.
//
// Eklentinin durumunu değiştiren her komut sağlayıcı kaydını **yeniler**.
// Kayıt açılışta kuruluyor ve bunu yapan yalnızca sunucu komutlarıydı:
// onaylanan bir eklenti uygulama yeniden açılana kadar çalınamıyor,
// kaldırılan bir eklenti aramada görünmeye devam ediyordu (D-071'de
// bulundu). Çalan parça etkilenmez — oynatıcı kendi kopyasını tutuyor.

#[tauri::command]
pub async fn plugins(state: State<'_, AppState>) -> CommandResult<PluginListReport> {
    state
        .run_on_core(move |core| Box::pin(async move { core.live.session().plugins() }))
        .await
}

/// Eklentinin beyan ettiği izinleri onaylar (D-040).
#[tauri::command]
pub async fn plugin_approve(
    state: State<'_, AppState>,
    name: String,
) -> CommandResult<PluginConsentReport> {
    state
        .run_on_core(move |core| {
            Box::pin(async move {
                let report = core.live.session().approve_plugin(&name)?;
                core.refresh_registry()?;
                Ok(report)
            })
        })
        .await
}

#[tauri::command]
pub async fn plugin_disable(
    state: State<'_, AppState>,
    name: String,
) -> CommandResult<PluginConsentReport> {
    state
        .run_on_core(move |core| {
            Box::pin(async move {
                let report = core.live.session().disable_plugin(&name)?;
                core.refresh_registry()?;
                Ok(report)
            })
        })
        .await
}

#[tauri::command]
pub async fn plugin_enable(
    state: State<'_, AppState>,
    name: String,
) -> CommandResult<PluginConsentReport> {
    state
        .run_on_core(move |core| {
            Box::pin(async move {
                let report = core.live.session().enable_plugin(&name)?;
                core.refresh_registry()?;
                Ok(report)
            })
        })
        .await
}

#[tauri::command]
pub async fn plugin_forget(
    state: State<'_, AppState>,
    name: String,
) -> CommandResult<PluginConsentReport> {
    state
        .run_on_core(move |core| {
            Box::pin(async move {
                let report = core.live.session().forget_plugin(&name)?;
                core.refresh_registry()?;
                Ok(report)
            })
        })
        .await
}

/// Kataloğu okur ve bu makineye karşı gösterir (D-071). **Ağa çıkar.**
#[tauri::command]
pub async fn plugin_catalog(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<PluginCatalogReport> {
    state
        .run_on_core(move |core| {
            Box::pin(async move {
                let http = headshell_core::net::default_http_client()?;
                busy(
                    &app,
                    "katalog okunuyor",
                    core.live.session().plugin_catalog(http),
                )
                .await
            })
        })
        .await
}

/// Eklentiyi kurar: diskte yoksa katalogdan indirir, sonra araçlarını
/// (D-055, D-071). **Ağa çıkar** — bu yüzden meşguliyet olayı gönderiyor.
#[tauri::command]
pub async fn plugin_install(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> CommandResult<PluginInstallReport> {
    state
        .run_on_core(move |core| {
            Box::pin(async move {
                let http = headshell_core::net::default_http_client()?;
                let report = busy(
                    &app,
                    format!("{name} kuruluyor").as_str(),
                    core.live.session().install_plugin(&name, http),
                )
                .await?;
                core.refresh_registry()?;
                Ok(report)
            })
        })
        .await
}

/// Katalogdan kurulmuş eklentileri günceller; `name` yoksa hepsini (D-071).
#[tauri::command]
pub async fn plugin_update(
    app: AppHandle,
    state: State<'_, AppState>,
    name: Option<String>,
) -> CommandResult<PluginUpdateReport> {
    state
        .run_on_core(move |core| {
            Box::pin(async move {
                let http = headshell_core::net::default_http_client()?;
                let what = match &name {
                    Some(name) => format!("{name} güncelleniyor"),
                    None => "eklentiler güncelleniyor".to_owned(),
                };
                let report = busy(
                    &app,
                    &what,
                    core.live.session().update_plugins(name.as_deref(), http),
                )
                .await?;
                core.refresh_registry()?;
                Ok(report)
            })
        })
        .await
}

/// Eklentiyi kaldırır ve onayını unutur (D-071).
#[tauri::command]
pub async fn plugin_remove(
    state: State<'_, AppState>,
    name: String,
) -> CommandResult<PluginRemoveReport> {
    state
        .run_on_core(move |core| {
            Box::pin(async move {
                let report = core.live.session().remove_plugin(&name)?;
                core.refresh_registry()?;
                Ok(report)
            })
        })
        .await
}

// ————————————————————————————————————— Sırlar (D-042)
//
// Liste **anahtar adlarını** taşır, değerleri değil. Bir sırrı okuyan tek
// taraf onu kullanan eklentidir; arayüz yalnızca yazar ve siler.

#[tauri::command]
pub async fn secrets(state: State<'_, AppState>) -> CommandResult<SecretListReport> {
    state
        .run_on_core(move |core| Box::pin(async move { core.live.session().secrets() }))
        .await
}

#[tauri::command]
pub async fn secret_set(
    state: State<'_, AppState>,
    namespace: String,
    key: String,
    value: String,
) -> CommandResult<SecretWriteReport> {
    state
        .run_on_core(move |core| {
            Box::pin(async move { core.live.session().set_secret(&namespace, &key, &value) })
        })
        .await
}

#[tauri::command]
pub async fn secret_remove(
    state: State<'_, AppState>,
    namespace: String,
    key: String,
) -> CommandResult<SecretWriteReport> {
    state
        .run_on_core(move |core| {
            Box::pin(async move { core.live.session().remove_secret(&namespace, &key) })
        })
        .await
}

// ————————————————————————————————————— Durum ve tanılama

/// Şu anki çapa. Webview pozisyonu **bundan tahmin eder**, sormaz (D-015).
#[tauri::command]
pub async fn anchor(state: State<'_, AppState>) -> CommandResult<PlaybackAnchor> {
    state
        .run_on_core(move |core| Box::pin(async move { Ok(core.live.anchor()) }))
        .await
}

#[tauri::command]
pub async fn queue(state: State<'_, AppState>) -> CommandResult<QueueView> {
    state
        .run_on_core(move |core| Box::pin(async move { Ok(core.live.player().queue().view()) }))
        .await
}

#[tauri::command]
pub async fn diag(
    state: State<'_, AppState>,
) -> CommandResult<Option<headshell_core::diag::DiagReport>> {
    state
        .run_on_core(move |core| Box::pin(async move { core.live.session().last_diag() }))
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
        .run_on_core(move |core| {
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
