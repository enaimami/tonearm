//! `tune` — çekirdeğin masaüstü kabuğu (PLAN Faz 3).
//!
//! **Altın Kural:** burada iş mantığı yok. Bu paket yalnızca pencereyi açar,
//! IPC komutlarını çekirdeğe iletir, tik döngüsünü sürer ve olayları
//! webview'e geçirir. Bir özelliği buradan silsen çekirdek onu hâlâ sunar —
//! CLI'nin `--json` çıktısı bunun kanıtı: GUI ile **aynı** veriyi alıyor.
//!
//! **D-030:** bağımlılık tek yönlü. Bu paket `tune-cli`'yi hiç görmez; biri
//! diğerinden bir şey isterse o şey çekirdeğe aittir.

// Windows'ta konsol penceresi açılmasın; hata ayıklama derlemesinde kalsın
// ki tanı satırları görünür olsun.
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod commands;
mod core_thread;
mod env;
mod state;

use std::process::ExitCode;

use tokio::sync::mpsc;
use tune_core::config::Config;

use crate::state::{AppState, Core};

fn main() -> ExitCode {
    // İlk iş: ortam düzeltmesi. GDK `GDK_BACKEND`'i `gtk_init` sırasında,
    // WebKit `WEBKIT_DISABLE_DMABUF_RENDERER`'ı web süreci doğarken okur —
    // ikisi de Tauri kurulumundan sonra, yani buradan sonrası geç kalır.
    env::duzelt();
    init_tracing();

    match calistir() {
        Ok(()) => ExitCode::SUCCESS,
        Err(text) => {
            // Pencere hiç açılamadıysa gösterilecek bir yüzey yok; hata
            // aşamasıyla birlikte `stderr`'e gider (K9).
            eprintln!("{text}");
            eprintln!("\nayrıntı için: tune diag");
            ExitCode::FAILURE
        }
    }
}

fn calistir() -> Result<(), String> {
    // Kütüphane pencereden **önce** açılıyor: veri dizini yoksa ya da
    // veritabanı bozuksa kullanıcı boş bir pencereye değil, aşamasını
    // söyleyen bir hataya baksın.
    let config = Config::discover().map_err(|err| err.chain_text())?;
    let core = Core::open(config).map_err(|err| err.chain_text())?;

    let (jobs_tx, jobs_rx) = mpsc::unbounded_channel();

    tauri::Builder::default()
        .manage(AppState::new(jobs_tx))
        .setup(move |app| {
            // Çekirdek kendi iş parçacığına burada taşınıyor: `AppHandle`
            // ancak kurulumda var, olaylar da oradan gidiyor.
            core_thread::baslat(core, jobs_rx, app.handle().clone())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Kütüphane
            commands::search,
            commands::stats,
            commands::wrapped,
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
            // Durum ve tanılama
            commands::anchor,
            commands::queue,
            commands::diag,
            commands::environment,
        ])
        .run(tauri::generate_context!())
        .map_err(|err| format!("ADIM: PLAYBACK_OUTPUT\n  pencere açılamadı: {err}"))

    // `run` döndüğünde `AppState` düşer, kanal kapanır ve çekirdek iş
    // parçacığı döngüden çıkıp kalan dinlemeleri yazar (`core_thread::calis`
    // sonundaki `shutdown`).
}

/// Log `stderr`'e; `println!` yalnızca kullanıcıya dönük çıktı içindi ve
/// bir GUI'de öyle bir çıktı yok.
fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("tune_core=warn,tune=warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}
