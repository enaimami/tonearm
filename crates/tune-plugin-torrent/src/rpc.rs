//! Tel katmanı: satır bazlı JSON-RPC 2.0 (protokol §tel biçimi).
//!
//! stdout **yalnızca** protokol satırlarıdır. Her günlük kaydı ya `log`
//! bildirimi olarak buradan, ya da stderr'den gider — çekirdek stderr'i
//! `tracing`'e aktarıyor, yani ikisi de kaybolmuyor.

use std::io::Write;

use serde::Deserialize;

/// JSON-RPC'nin "metot yok" kodu. Opsiyonel metotların cevabı budur.
pub const CODE_METHOD_NOT_FOUND: i64 = -32601;
/// Uygulama hatası aralığı: süreç sağ, çağrı reddedildi.
pub const CODE_PLUGIN_ERROR: i64 = -32000;

/// Çekirdekten gelen satır.
#[derive(Debug, Deserialize)]
pub struct Incoming {
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

/// Çağrıyı düşüren ama süreci öldürmeyen hata (K5: eklenti çökerse çekirdek
/// düşmez — biz de çökmemeyi tercih ediyoruz, hata döndürmek yeterli).
#[derive(Debug)]
pub struct PluginError(String);

impl PluginError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PluginError {}

pub type Result<T> = std::result::Result<T, PluginError>;

/// `Err(PluginError)` kısayolu. Metinler Türkçe (D-036).
pub fn err<T>(message: impl Into<String>) -> Result<T> {
    Err(PluginError::new(message))
}

/// Bir `anyhow`/`std` hatasını mesaja çevirir, **sebep zincirini koruyarak.**
/// Yalnızca en dıştaki cümleyi göstermek, tanının yarısını atmak demektir (K9).
pub fn chain_text(error: &anyhow::Error) -> String {
    let mut parts = Vec::new();
    for cause in error.chain() {
        let text = cause.to_string();
        if !parts.contains(&text) {
            parts.push(text);
        }
    }
    parts.join(": ")
}

fn send(value: &serde_json::Value) {
    let mut out = std::io::stdout().lock();
    let written = serde_json::to_writer(&mut out, value)
        .map_err(std::io::Error::from)
        .and_then(|()| out.write_all(b"\n"))
        .and_then(|()| out.flush());
    if let Err(error) = written {
        // Protokol borusu kapandıysa yapacak bir şey yok ama sessiz kalmıyoruz:
        // stderr çekirdeğin `tracing`'ine akıyor.
        eprintln!("protokol satırı yazılamadı: {error}");
    }
}

/// Başarılı cevap. `id` yoksa (bildirim) cevap gönderilmez.
pub fn reply(id: Option<u64>, result: serde_json::Value) {
    let Some(id) = id else { return };
    send(&serde_json::json!({"jsonrpc": "2.0", "id": id, "result": result}));
}

/// Hata cevabı.
pub fn fail(id: Option<u64>, code: i64, message: &str) {
    let Some(id) = id else {
        eprintln!("kimliksiz isteğe hata döndürülemez: {message}");
        return;
    };
    send(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "message": message},
    }));
}

/// `log` bildirimi: çekirdeğin kullanıcıya gösterdiği kanal.
pub fn log(level: &str, message: impl AsRef<str>) {
    send(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": "log",
        "params": {"level": level, "message": message.as_ref()},
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cause_chain_becomes_one_line_without_repeating_itself() {
        let error = anyhow::anyhow!("kök sebep")
            .context("orta katman")
            .context("dış katman");
        let text = chain_text(&error);
        assert!(text.contains("dış katman"), "{text}");
        assert!(text.contains("kök sebep"), "{text}");
        assert!(!text.contains('\n'), "tel biçimi satır bazlı: {text}");
    }

    #[test]
    fn a_repeated_cause_is_not_printed_twice() {
        let error = anyhow::anyhow!("aynı").context("aynı");
        assert_eq!(chain_text(&error), "aynı");
    }
}
