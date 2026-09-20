//! JSON-RPC istemcisi: istek/cevap eşleme, zaman aşımı, hata haritalama.
//!
//! Taşımanın üstünde duran ince katman. Bildiği üç şey var:
//! 1. Cevaplar `id` ile eşlenir; başka bir `id` gelirse **atlanır ve
//!    söylenir**, ilk satır cevap sanılmaz.
//! 2. `log` bildirimi eklentinin konuşma hakkıdır; `tracing`'e aktarılır.
//! 3. JSON olmayan satır eklentiyi öldürmez. Bir `print()` hatası
//!    sağlayıcıyı düşürmemeli; satır uyarı olarak geçilir.

use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};

use super::protocol::{Incoming, LogParams, Request, method};
use super::transport::{PluginTransport, Received};

/// El sıkışmaya tanınan süre. Kısa: bir eklenti açılırken ağ çağrısı
/// yapmamalı, kendini tanıtmalı.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// Normal çağrılara tanınan süre. Ağ gerektiren bir arama bu kadar sürebilir.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(20);

/// Tek bir eklenti süreciyle konuşan istemci.
pub struct PluginClient {
    plugin: String,
    transport: Box<dyn PluginTransport>,
    next_id: u64,
}

impl std::fmt::Debug for PluginClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginClient")
            .field("plugin", &self.plugin)
            .field("next_id", &self.next_id)
            .finish()
    }
}

impl PluginClient {
    #[must_use]
    pub fn new(plugin: &str, transport: Box<dyn PluginTransport>) -> Self {
        Self {
            plugin: plugin.to_owned(),
            transport,
            next_id: 1,
        }
    }

    /// Ham cevabı döndürür.
    ///
    /// # Errors
    /// Süreç ölürse ([`ErrorKind::PluginCrashed`]), süre dolarsa
    /// ([`ErrorKind::PluginTimeout`]) ya da eklenti hata nesnesi dönerse
    /// ([`ErrorKind::PluginRpc`]).
    pub fn call_value(
        &mut self,
        stage: Stage,
        method: &str,
        params: serde_json::Value,
        timeout: Duration,
    ) -> Result<serde_json::Value> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        let request = Request::call(id, method, params);
        let line = serde_json::to_string(&request).map_err(|source| {
            Error::new(
                stage,
                ErrorKind::Json {
                    entry: format!("{}.{method} isteği", self.plugin),
                    source,
                },
            )
        })?;
        self.transport.send_line(&line)?;

        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(self.timeout(stage, method, timeout));
            }
            match self.transport.receive_line(remaining)? {
                Received::Timeout => return Err(self.timeout(stage, method, timeout)),
                Received::Closed => {
                    return Err(Error::new(
                        stage,
                        ErrorKind::PluginCrashed {
                            plugin: self.plugin.clone(),
                            detail: format!("{method} cevabı gelmeden süreç kapandı"),
                        },
                    ));
                }
                Received::Line(line) => {
                    let incoming: Incoming = match serde_json::from_str(&line) {
                        Ok(incoming) => incoming,
                        Err(err) => {
                            // Eklentinin stdout'una düşen bir `print()`.
                            // Öldürmüyoruz ama yutmuyoruz da (K9).
                            tracing::warn!(
                                plugin = %self.plugin,
                                error = %err,
                                line = %crate::net::clip(&line, 200),
                                "eklenti JSON olmayan bir satır yazdı, atlandı"
                            );
                            continue;
                        }
                    };

                    if incoming.id == Some(id) {
                        if let Some(error) = incoming.error {
                            return Err(Error::new(
                                stage,
                                ErrorKind::PluginRpc {
                                    plugin: self.plugin.clone(),
                                    method: method.to_owned(),
                                    code: error.code,
                                    message: error.message,
                                },
                            ));
                        }
                        return Ok(incoming.result.unwrap_or(serde_json::Value::Null));
                    }

                    self.handle_unmatched(&incoming, method);
                }
            }
        }
    }

    /// Cevabı beklenen tipe çevirir.
    ///
    /// # Errors
    /// [`Self::call_value`]'nun hataları + cevap beklenen biçimde değilse.
    pub fn call<T: DeserializeOwned>(
        &mut self,
        stage: Stage,
        method: &str,
        params: serde_json::Value,
        timeout: Duration,
    ) -> Result<T> {
        let value = self.call_value(stage, method, params, timeout)?;
        serde_json::from_value(value).map_err(|source| {
            Error::new(
                stage,
                ErrorKind::Json {
                    entry: format!("{}.{method} cevabı", self.plugin),
                    source,
                },
            )
        })
    }

    /// Cevap beklemeyen bildirim gönderir.
    ///
    /// # Errors
    /// Süreç ölmüşse.
    pub fn notify(&mut self, method: &str, params: serde_json::Value) -> Result<()> {
        let request = Request::notify(method, params);
        let line = serde_json::to_string(&request).map_err(|source| {
            Error::new(
                Stage::ProviderCall,
                ErrorKind::Json {
                    entry: format!("{}.{method} bildirimi", self.plugin),
                    source,
                },
            )
        })?;
        self.transport.send_line(&line)
    }

    /// `shutdown` bildirimi yollar ve süreci kapatır.
    ///
    /// Bildirim gönderilemezse (süreç zaten ölmüşse) sorun değil: kapatma
    /// yine de yapılır.
    pub fn shutdown(&mut self) {
        if let Err(err) = self.notify(method::SHUTDOWN, serde_json::json!({})) {
            tracing::debug!(
                plugin = %self.plugin,
                error = %err.chain_text().replace('\n', " "),
                "kapatma bildirimi gönderilemedi"
            );
        }
        self.transport.shutdown();
    }

    /// Bize ait olmayan satır: bildirim mi, başıboş cevap mı.
    fn handle_unmatched(&self, incoming: &Incoming, awaiting: &str) {
        match incoming.method.as_deref() {
            Some("log") => {
                let params = incoming
                    .params
                    .clone()
                    .and_then(|value| serde_json::from_value::<LogParams>(value).ok());
                match params {
                    Some(log) => {
                        let level = log.level.unwrap_or_else(|| "info".to_owned());
                        tracing::info!(plugin = %self.plugin, level = %level, "eklenti: {}", log.message);
                    }
                    None => tracing::warn!(
                        plugin = %self.plugin,
                        "eklenti bozuk bir log bildirimi gönderdi"
                    ),
                }
            }
            Some(other) => tracing::warn!(
                plugin = %self.plugin,
                method = %other,
                "eklenti tanınmayan bir bildirim gönderdi, atlandı"
            ),
            None => tracing::warn!(
                plugin = %self.plugin,
                awaiting = %awaiting,
                id = ?incoming.id,
                "eşleşmeyen cevap atlandı"
            ),
        }
    }

    fn timeout(&self, stage: Stage, method: &str, timeout: Duration) -> Error {
        Error::new(
            stage,
            ErrorKind::PluginTimeout {
                plugin: self.plugin.clone(),
                method: method.to_owned(),
                seconds: timeout.as_secs(),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::transport::ScriptedTransport;

    fn client(replies: Vec<Received>) -> PluginClient {
        PluginClient::new("test", Box::new(ScriptedTransport::new(replies)))
    }

    fn line(text: &str) -> Received {
        Received::Line(text.to_owned())
    }

    #[test]
    fn a_matching_response_is_returned() {
        let mut client = client(vec![line(
            r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#,
        )]);
        let value = client
            .call_value(
                Stage::ProviderCall,
                "health",
                serde_json::json!({}),
                Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!(value, serde_json::json!({"ok": true}));
    }

    #[test]
    fn a_stray_line_does_not_become_the_answer() {
        let mut client = client(vec![
            line("hazırım!"),
            line(r#"{"jsonrpc":"2.0","method":"log","params":{"message":"merhaba"}}"#),
            line(r#"{"jsonrpc":"2.0","id":99,"result":{"baska":true}}"#),
            line(r#"{"jsonrpc":"2.0","id":1,"result":{"dogru":true}}"#),
        ]);
        let value = client
            .call_value(
                Stage::ProviderCall,
                "health",
                serde_json::json!({}),
                Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!(
            value,
            serde_json::json!({"dogru": true}),
            "JSON olmayan satır, log bildirimi ve başka id'li cevap atlanmalı"
        );
    }

    #[test]
    fn an_error_object_becomes_a_typed_rejection_not_a_crash() {
        let mut client = client(vec![line(
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"metot yok"}}"#,
        )]);
        let err = client
            .call_value(
                Stage::ProviderCall,
                "search",
                serde_json::json!({}),
                Duration::from_secs(1),
            )
            .unwrap_err();
        match err.kind() {
            ErrorKind::PluginRpc { code, message, .. } => {
                assert_eq!(*code, -32601);
                assert_eq!(message, "metot yok");
            }
            other => panic!("beklenmeyen hata: {other:?}"),
        }
    }

    #[test]
    fn a_dead_process_is_a_crash_not_a_timeout() {
        let mut client = client(Vec::new());
        let err = client
            .call_value(
                Stage::ProviderCall,
                "health",
                serde_json::json!({}),
                Duration::from_secs(1),
            )
            .unwrap_err();
        assert!(
            matches!(err.kind(), ErrorKind::PluginCrashed { .. }),
            "{}",
            err.chain_text()
        );
    }

    #[test]
    fn a_hanging_plugin_is_a_timeout_naming_the_method() {
        let mut client = PluginClient::new(
            "test",
            Box::new(crate::plugin::transport::ScriptedTransport::hanging()),
        );
        let err = client
            .call_value(
                Stage::ProviderCall,
                "search",
                serde_json::json!({}),
                Duration::from_millis(50),
            )
            .unwrap_err();
        match err.kind() {
            ErrorKind::PluginTimeout { method, .. } => assert_eq!(method, "search"),
            other => panic!("beklenmeyen hata: {other:?}"),
        }
    }

    #[test]
    fn a_response_of_the_wrong_shape_is_a_json_error_naming_the_method() {
        let mut client = client(vec![line(r#"{"jsonrpc":"2.0","id":1,"result":42}"#)]);
        let err = client
            .call::<super::super::protocol::HealthResult>(
                Stage::ProviderCall,
                "health",
                serde_json::json!({}),
                Duration::from_secs(1),
            )
            .unwrap_err();
        assert!(
            err.chain_text().contains("health cevabı"),
            "{}",
            err.chain_text()
        );
    }

    #[test]
    fn each_call_uses_a_fresh_id() {
        let mut client = client(vec![
            line(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#),
            line(r#"{"jsonrpc":"2.0","id":2,"result":{}}"#),
        ]);
        for _ in 0..2 {
            client
                .call_value(
                    Stage::ProviderCall,
                    "health",
                    serde_json::json!({}),
                    Duration::from_secs(1),
                )
                .unwrap();
        }
        assert_eq!(client.next_id, 3);
    }
}
