//! Gerçek süreçle protokol sınaması: ikili gerçekten konuşuyor mu?
//!
//! §2.1'in `plugin_process.rs`'i bunu Python eklentisi için yapıyordu. Burada
//! sınanan aynı şey ama **Rust** bir eklenti için: satır çerçeveleme, el
//! sıkışma, tanınmayan metodun `-32601` olması, ve hataların süreci
//! öldürmemesi.
//!
//! Ağa çıkmıyor: Torznab yapılandırılmadığı için arama zaten indekse gitmeden
//! hata veriyor — ve tam olarak bu sınanıyor.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

/// Tek bir cevap için beklenecek en uzun süre. Bu testlerdeki çağrıların
/// hiçbiri ağa çıkmıyor; bunu aşmak "eklenti takıldı" demektir.
const REPLY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

struct Plugin {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Plugin {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_tonearm-plugin-torrent"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("eklenti ikilisi çalıştırılmalı");
        let stdin = child.stdin.take().expect("stdin borusu");
        let stdout = BufReader::new(child.stdout.take().expect("stdout borusu"));
        Self {
            child,
            stdin,
            stdout,
        }
    }

    fn send(&mut self, line: &serde_json::Value) {
        writeln!(self.stdin, "{line}").expect("istek yazılmalı");
        self.stdin.flush().expect("istek gönderilmeli");
    }

    /// Bir cevap okur; `log` bildirimlerini atlar (kimlikleri yok).
    fn recv(&mut self) -> serde_json::Value {
        let deadline = std::time::Instant::now() + REPLY_TIMEOUT;
        loop {
            assert!(
                std::time::Instant::now() < deadline,
                "eklenti {REPLY_TIMEOUT:?} içinde cevap vermedi"
            );
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).expect("stdout okunmalı");
            assert!(read > 0, "eklenti cevap vermeden kapandı");
            let value: serde_json::Value =
                serde_json::from_str(line.trim()).expect("stdout satırı JSON olmalı");
            if value.get("id").is_some() {
                return value;
            }
            // `log` bildirimi: kimliksiz, cevap değil.
            assert_eq!(
                value["method"], "log",
                "kimliksiz satır yalnızca log olmalı"
            );
        }
    }

    fn call(&mut self, id: u64, method: &str, params: serde_json::Value) -> serde_json::Value {
        self.send(&serde_json::json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params
        }));
        self.recv()
    }

    fn handshake(&mut self, secrets: serde_json::Value) -> serde_json::Value {
        let data_dir = std::env::temp_dir().join(format!(
            "tonearm-torrent-proc-{}-{}",
            std::process::id(),
            id_suffix()
        ));
        std::fs::create_dir_all(&data_dir).expect("veri dizini");
        self.call(
            1,
            "handshake",
            serde_json::json!({
                "api": 1,
                "host": {"name": "tonearm", "version": "test"},
                "data_dir": data_dir.display().to_string(),
                "secrets": secrets,
                "permissions": {"net": [], "fs": []},
            }),
        )
    }

    fn shutdown(mut self) {
        self.send(&serde_json::json!({"jsonrpc": "2.0", "method": "shutdown", "params": {}}));
        let status = self.child.wait().expect("süreç beklenmeli");
        assert!(status.success(), "eklenti düzgün kapanmalı: {status}");
    }
}

fn id_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
}

#[test]
fn the_binary_speaks_the_protocol_end_to_end() {
    let mut plugin = Plugin::start();

    let result = plugin.handshake(serde_json::json!({}));
    let handshake = &result["result"];
    assert_eq!(handshake["api"], 1);
    assert_eq!(handshake["name"], "torrent");
    assert_eq!(handshake["display_name"], "Torrent");
    assert_eq!(
        handshake["capabilities"],
        serde_json::json!(["search", "stream"])
    );

    // Tanınmayan metot: çökme değil, `-32601`.
    let unknown = plugin.call(2, "teleport", serde_json::json!({}));
    assert_eq!(unknown["error"]["code"], -32601);

    // Süreç hâlâ ayakta: bir sonraki çağrı cevaplanıyor.
    let alive = plugin.call(3, "health", serde_json::json!({}));
    assert!(
        alive["result"].is_object(),
        "hatadan sonra süreç yaşamalı: {alive}"
    );

    plugin.shutdown();
}

#[test]
fn an_unconfigured_search_says_it_did_not_look_rather_than_finding_nothing() {
    let mut plugin = Plugin::start();
    plugin.handshake(serde_json::json!({}));

    let response = plugin.call(
        2,
        "search",
        serde_json::json!({"query": "radiohead", "limit": 5}),
    );
    let message = response["error"]["message"]
        .as_str()
        .unwrap_or_else(|| panic!("boş sonuç değil hata bekleniyordu: {response}"));
    assert!(message.contains("yapılandırılmamış"), "{message}");
    assert!(
        message.contains("tonearm secret set plugin:torrent torznab_url"),
        "kullanıcıya ne yazacağı söylenmeli: {message}"
    );

    plugin.shutdown();
}

#[test]
fn health_separates_search_from_the_torrent_engine() {
    let mut plugin = Plugin::start();
    plugin.handshake(serde_json::json!({}));

    let response = plugin.call(2, "health", serde_json::json!({}));
    let detail = response["result"]["detail"]
        .as_str()
        .unwrap_or_else(|| panic!("detay bekleniyordu: {response}"));
    // İkisi ayrı satırda raporlanmalı: arama çalışmıyor olabilir ama elde
    // infohash olan bir parça yine çalar (K9).
    assert!(detail.contains("arama"), "{detail}");
    assert!(detail.contains("torrent motoru"), "{detail}");
    assert_eq!(
        response["result"]["track_count"],
        serde_json::Value::Null,
        "torrent'in kataloğu yok; sayı uydurulmamalı"
    );

    plugin.shutdown();
}

#[test]
fn a_bad_id_is_refused_without_starting_a_download() {
    let mut plugin = Plugin::start();
    plugin.handshake(serde_json::json!({}));

    let response = plugin.call(2, "resolve_source", serde_json::json!({"id": "merhaba"}));
    assert!(
        response["error"]["message"]
            .as_str()
            .is_some_and(|text| text.contains("infohash değil")),
        "{response}"
    );

    plugin.shutdown();
}

#[test]
fn a_malformed_line_is_skipped_instead_of_killing_the_process() {
    let mut plugin = Plugin::start();
    plugin.handshake(serde_json::json!({}));

    writeln!(plugin.stdin, "bu json değil").expect("bozuk satır yazılmalı");
    plugin.stdin.flush().expect("gönderilmeli");

    let alive = plugin.call(2, "health", serde_json::json!({}));
    assert!(alive["result"].is_object(), "{alive}");

    plugin.shutdown();
}
