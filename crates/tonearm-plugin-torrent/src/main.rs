//! Torrent eklentisinin ince kabuğu: stdin'den satır oku, gönder, cevabı yaz.
//!
//! Bütün mantık `lib.rs` ve altındaki modüllerde. Ayrım Altın Kural'ın
//! eklentiye düşen hâli: buradan silinen hiçbir şey yeteneği götürmemeli —
//! entegrasyon testleri de aynı işleyicileri kütüphaneden çağırıyor.

use tokio::io::{AsyncBufReadExt, BufReader};
use tonearm_core::plugin::protocol::method;
use tonearm_plugin_torrent::rpc::{CODE_METHOD_NOT_FOUND, CODE_PLUGIN_ERROR, Incoming};
use tonearm_plugin_torrent::{App, dispatch, rpc};

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    // stdout protokole ait; her günlük satırı stderr'e. Çekirdek stderr'i
    // `tracing`'e aktarıyor, yani librqbit'in teşhisi `tonearm diag`'a ulaşır.
    let filter = tracing_subscriber::EnvFilter::try_from_env("TONEARM_TORRENT_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let mut app = App::new();
    let mut lines = BufReader::new(tokio::io::stdin()).lines();

    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            // EOF: çekirdek boruyu kapattı, düzgün çıkıyoruz.
            Ok(None) => return,
            Err(error) => {
                eprintln!("stdin okunamadı: {error}");
                return;
            }
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(incoming) = serde_json::from_str::<Incoming>(line) else {
            eprintln!("ayrıştırılamayan satır atlandı");
            continue;
        };

        let method_name = incoming.method.clone().unwrap_or_default();
        if method_name == method::SHUTDOWN {
            return;
        }

        let params = incoming.params.clone().unwrap_or(serde_json::Value::Null);
        match dispatch(&mut app, &method_name, params).await {
            Ok(Some(result)) => rpc::reply(incoming.id, result),
            // Tanınmayan metot: protokol bunu "bu yeteneği desteklemiyorum"
            // diye okur ve çekirdek çökmez.
            Ok(None) => rpc::fail(
                incoming.id,
                CODE_METHOD_NOT_FOUND,
                &format!("metot yok: {method_name}"),
            ),
            Err(error) => rpc::fail(incoming.id, CODE_PLUGIN_ERROR, &error.to_string()),
        }
    }
}
