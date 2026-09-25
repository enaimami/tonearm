//! `headshell` — çekirdeğin masaüstü kabuğu (PLAN Faz 3).
//!
//! **Altın Kural:** burada iş mantığı yok. Bu paket yalnızca pencereyi açar,
//! IPC komutlarını çekirdeğe iletir, tik döngüsünü sürer ve olayları
//! webview'e geçirir. Bir özelliği buradan silsen çekirdek onu hâlâ sunar —
//! CLI'nin `--json` çıktısı bunun kanıtı: GUI ile **aynı** veriyi alıyor.
//!
//! **D-030:** bağımlılık tek yönlü. Bu paket `headshell-cli`'yi hiç görmez; biri
//! diğerinden bir şey isterse o şey çekirdeğe aittir.

// Windows'ta konsol penceresi açılmasın; hata ayıklama derlemesinde kalsın
// ki tanı satırları görünür olsun.
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod commands;
mod core_thread;
mod env;
mod state;
mod theme;

use std::process::ExitCode;

use headshell_core::config::Config;
use tokio::sync::mpsc;

use crate::state::{AppState, Core};
use crate::theme::ThemeStore;

fn main() -> ExitCode {
    // İlk iş: ortam düzeltmesi. GDK `GDK_BACKEND`'i `gtk_init` sırasında,
    // WebKit `WEBKIT_DISABLE_DMABUF_RENDERER`'ı web süreci doğarken okur —
    // ikisi de Tauri kurulumundan sonra, yani buradan sonrası geç kalır.
    env::fixup();
    init_tracing();

    // Bağlam tek kez üretiliyor: arayüz dosyalarını ikiliye gömüyor ve iki
    // yolda da (uygulama ya da hata penceresi) aynısı kullanılıyor.
    let context = tauri::generate_context!();

    // Kütüphane pencereden **önce** açılıyor: veri dizini yoksa ya da
    // veritabanı bozuksa kullanıcı boş bir pencereye değil, aşamasını
    // söyleyen bir hataya baksın.
    let (core, themes) = match open_core() {
        Ok(opened) => opened,
        Err(text) => {
            report(&text);
            show_startup_error(context, &text);
            return ExitCode::FAILURE;
        }
    };

    match run(core, themes, context) {
        Ok(()) => ExitCode::SUCCESS,
        Err(text) => {
            // Pencere hiç açılamadıysa gösterilecek bir yüzey yok; hata
            // aşamasıyla birlikte `stderr`'e gider (K9).
            report(&text);
            ExitCode::FAILURE
        }
    }
}

fn report(text: &str) {
    eprintln!("{text}");
    eprintln!("\nayrıntı için: headshell diag");
}

fn open_core() -> Result<(Core, ThemeStore), String> {
    let config = Config::discover().map_err(|err| err.chain_text())?;
    // Tema deposu çekirdeğe gitmiyor, yalnızca veri dizinini biliyor (§3.3).
    let themes = ThemeStore::new(config.data_dir());
    let core = Core::open(config).map_err(|err| err.chain_text())?;
    Ok((core, themes))
}

/// Açılış başarısız olduysa hatayı bir pencerede gösterir (D-070).
///
/// Windows'un sürüm derlemesi konsolsuz (`windows_subsystem`, dosyanın
/// başında): `stderr`'e yazılan hata orada **hiçbir yere** gitmiyor ve
/// uygulamaya çift tıklayan kullanıcı hiçbir şey görmüyordu. K9 bunu
/// yasaklıyor — her başarısızlık hangi aşamada olduğunu söyler. Pencere her
/// platformda açılıyor: tek metin, tek davranış.
///
/// Metin sayfaya adresin `#` kısmıyla gidiyor, IPC'yle değil: çekirdek yok,
/// komutlar yok, ve sayfanın CSP'si (`script-src 'self'`) satır içi betiğe
/// izin vermiyor.
fn show_startup_error(mut context: tauri::Context, text: &str) {
    // Ana pencere (`index.html`) açılmasın: arkasında çekirdek yok, arayüz
    // boş ve donuk kalırdı.
    context.config_mut().app.windows.clear();
    let url = format!("startup-error.html#{}", encode_fragment(text));

    let built = tauri::Builder::default()
        .setup(move |app| {
            tauri::WebviewWindowBuilder::new(
                app,
                "startup-error",
                tauri::WebviewUrl::App(url.into()),
            )
            .title("headshell açılamadı")
            .inner_size(760.0, 460.0)
            .build()?;
            Ok(())
        })
        .build(context);
    match built {
        // Pencere kapanınca döner; süreç yine başarısızlık koduyla çıkar.
        Ok(app) => {
            let _ = app.run_return(|_, _| {});
        }
        Err(err) => eprintln!("ADIM: STARTUP_ERROR — hata penceresi de açılamadı: {err}"),
    }
}

/// Metni adresin parça (`#…`) kısmına yazılabilir hâle getirir.
///
/// Ayrılmamış karakterler (RFC 3986) dışındaki her bayt `%XX` olur. Elle
/// yazılıyor, çünkü URL ayrıştırıcıları satır sonlarını sessizce siler —
/// çok satırlı bir hata zinciri tek satıra düşerdi. Sayfa
/// `decodeURIComponent` ile geri çeviriyor.
fn encode_fragment(text: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    out
}

fn run(core: Core, themes: ThemeStore, context: tauri::Context) -> Result<(), String> {
    let (jobs_tx, jobs_rx) = mpsc::unbounded_channel();

    tauri::Builder::default()
        // Yalnızca yol seçtiriyor. Dosyayı okuyan/yazan taraf çekirdek —
        // `capabilities/default.json` bu yüzden `fs` izni vermiyor.
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new(jobs_tx, themes))
        .setup(move |app| {
            // Çekirdek kendi iş parçacığına burada taşınıyor: `AppHandle`
            // ancak kurulumda var, olaylar da oradan gidiyor.
            core_thread::spawn(core, jobs_rx, app.handle().clone())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Kütüphane
            commands::search,
            commands::stats,
            commands::sleeve,
            commands::sleeve_svg,
            // İçe aktarma ve kimlik
            commands::import,
            commands::resolve,
            // Sağlayıcı
            commands::providers,
            commands::provider_test,
            commands::provider_scan,
            commands::servers_list,
            commands::server_add,
            commands::server_remove,
            // Oynatma
            commands::play,
            commands::toggle_pause,
            commands::stop,
            commands::next,
            commands::previous,
            commands::jump_to,
            commands::set_shuffle,
            commands::set_repeat,
            // Eklenti ve sır (Faz 2 yüzeyi)
            commands::plugins,
            commands::plugin_approve,
            commands::plugin_disable,
            commands::plugin_enable,
            commands::plugin_forget,
            commands::plugin_install,
            commands::plugin_catalog,
            commands::plugin_update,
            commands::plugin_remove,
            commands::secrets,
            commands::secret_set,
            commands::secret_remove,
            // Tema
            commands::themes_list,
            commands::theme_active,
            commands::theme_select,
            // Durum ve tanılama
            commands::anchor,
            commands::queue,
            commands::diag,
            commands::diag_text,
            commands::environment,
        ])
        .run(context)
        .map_err(|err| format!("ADIM: PLAYBACK_OUTPUT\n  pencere açılamadı: {err}"))

    // `run` döndüğünde `AppState` düşer, kanal kapanır ve çekirdek iş
    // parçacığı döngüden çıkıp kalan dinlemeleri yazar (`core_thread::run`
    // sonundaki `shutdown`).
}

/// Log `stderr`'e; `println!` yalnızca kullanıcıya dönük çıktı içindi ve
/// bir GUI'de öyle bir çıktı yok.
fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new("headshell_core=warn,headshell=warn")
    });
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::encode_fragment;

    /// Kodlama, sayfanın `decodeURIComponent`'iyle geri dönmeli — satır
    /// sonları, Türkçe harfler ve `#`/`%` dahil. Sayfanın yaptığı çözme
    /// burada gerçek bir JS motorunda yapılıyor (QuickJS, D-070).
    #[test]
    fn the_error_text_survives_the_trip_through_the_url_fragment() {
        let text = "ADIM: CONFIG_LOAD\n  → veri dizini — %LOCALAPPDATA% yok; #1 çğıöşü İ";
        let encoded = encode_fragment(text);
        assert!(!encoded.contains('\n') && !encoded.contains('#') && !encoded.contains(' '));

        let runtime = rquickjs::Runtime::new().unwrap();
        let context = rquickjs::Context::full(&runtime).unwrap();
        let decoded: String = context.with(|ctx| {
            let decode: rquickjs::Function = ctx.globals().get("decodeURIComponent").unwrap();
            decode.call((encoded.as_str(),)).unwrap()
        });
        assert_eq!(decoded, text);
    }
}
