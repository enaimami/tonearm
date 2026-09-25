//! The wire layer: line-based JSON-RPC 2.0 (the protocol's §wire format).
//!
//! stdout is **only** protocol lines. Every log record goes either from here as
//! a `log` notification or through stderr — the core forwards stderr into
//! `tracing`, so neither is lost.

use std::io::Write;

use serde::Deserialize;

/// JSON-RPC's "method not found" code. It is the answer for optional methods.
pub const CODE_METHOD_NOT_FOUND: i64 = -32601;
/// The application error range: the process is alive, the call was refused.
pub const CODE_PLUGIN_ERROR: i64 = -32000;

/// A line coming from the core.
#[derive(Debug, Deserialize)]
pub struct Incoming {
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

/// An error that drops the call but does not kill the process (K5: if the
/// plugin crashes the core does not fall over — we still prefer not to crash;
/// returning an error is enough).
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

/// A shortcut for `Err(PluginError)`. The texts are in English (D-036, D-073).
pub fn err<T>(message: impl Into<String>) -> Result<T> {
    Err(PluginError::new(message))
}

/// Turns an `anyhow`/`std` error into a message, **keeping the chain of
/// causes.** Showing only the outermost sentence throws away half of the
/// diagnosis (K9).
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
        // If the protocol pipe is closed there is nothing to do, but we do not
        // stay silent: stderr flows into the core's `tracing`.
        eprintln!("could not write a protocol line: {error}");
    }
}

/// A successful answer. Without an `id` (a notification) no answer is sent.
pub fn reply(id: Option<u64>, result: serde_json::Value) {
    let Some(id) = id else { return };
    send(&serde_json::json!({"jsonrpc": "2.0", "id": id, "result": result}));
}

/// An error answer.
pub fn fail(id: Option<u64>, code: i64, message: &str) {
    let Some(id) = id else {
        eprintln!("cannot return an error to a request without an id: {message}");
        return;
    };
    send(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "message": message},
    }));
}

/// A `log` notification: the channel the core shows the user.
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
        let error = anyhow::anyhow!("root cause")
            .context("middle layer")
            .context("outer layer");
        let text = chain_text(&error);
        assert!(text.contains("outer layer"), "{text}");
        assert!(text.contains("root cause"), "{text}");
        assert!(!text.contains('\n'), "the wire format is line-based: {text}");
    }

    #[test]
    fn a_repeated_cause_is_not_printed_twice() {
        let error = anyhow::anyhow!("same").context("same");
        assert_eq!(chain_text(&error), "same");
    }
}
