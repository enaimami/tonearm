//! A protocol test with the real process: does the binary really talk?
//!
//! §2.1's `plugin_process.rs` did this for the Python plugin. What is tested
//! here is the same thing, but for a **Rust** plugin: line framing, the
//! handshake, an unrecognised method being `-32601`, and errors not killing
//! the process.
//!
//! It does not go online: since Torznab is not configured, search fails
//! before going to the indexer anyway — and that is exactly what is tested.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

/// The longest to wait for a single answer. None of the calls in these tests
/// go online; going over this means "the plugin hung".
const REPLY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

struct Plugin {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Plugin {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_headshell-plugin-torrent"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the plugin binary must run");
        let stdin = child.stdin.take().expect("stdin pipe");
        let stdout = BufReader::new(child.stdout.take().expect("stdout pipe"));
        Self {
            child,
            stdin,
            stdout,
        }
    }

    fn send(&mut self, line: &serde_json::Value) {
        writeln!(self.stdin, "{line}").expect("the request must be written");
        self.stdin.flush().expect("the request must be sent");
    }

    /// Reads an answer; skips `log` notifications (they have no id).
    fn recv(&mut self) -> serde_json::Value {
        let deadline = std::time::Instant::now() + REPLY_TIMEOUT;
        loop {
            assert!(
                std::time::Instant::now() < deadline,
                "the plugin did not answer within {REPLY_TIMEOUT:?}"
            );
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).expect("stdout must be readable");
            assert!(read > 0, "the plugin closed without answering");
            let value: serde_json::Value =
                serde_json::from_str(line.trim()).expect("a stdout line must be JSON");
            if value.get("id").is_some() {
                return value;
            }
            // A `log` notification: no id, not an answer.
            assert_eq!(
                value["method"], "log",
                "a line without an id must only be a log"
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
            "headshell-torrent-proc-{}-{}",
            std::process::id(),
            id_suffix()
        ));
        std::fs::create_dir_all(&data_dir).expect("data directory");
        self.call(
            1,
            "handshake",
            serde_json::json!({
                "api": 1,
                "host": {"name": "headshell", "version": "test"},
                "data_dir": data_dir.display().to_string(),
                "secrets": secrets,
                "permissions": {"net": [], "fs": []},
            }),
        )
    }

    fn shutdown(mut self) {
        self.send(&serde_json::json!({"jsonrpc": "2.0", "method": "shutdown", "params": {}}));
        let status = self.child.wait().expect("the process must be waited for");
        assert!(status.success(), "the plugin must close cleanly: {status}");
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

    // An unrecognised method: not a crash, `-32601`.
    let unknown = plugin.call(2, "teleport", serde_json::json!({}));
    assert_eq!(unknown["error"]["code"], -32601);

    // The process is still up: the next call is answered.
    let alive = plugin.call(3, "health", serde_json::json!({}));
    assert!(
        alive["result"].is_object(),
        "the process must live on after an error: {alive}"
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
        .unwrap_or_else(|| panic!("an error was expected, not an empty result: {response}"));
    assert!(message.contains("not configured"), "{message}");
    assert!(
        message.contains("headshell secret set plugin:torrent torznab_url"),
        "the user must be told what to write: {message}"
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
        .unwrap_or_else(|| panic!("a detail was expected: {response}"));
    // The two must be reported on separate lines: search may not work, but a
    // track whose infohash is at hand still plays (K9).
    assert!(detail.contains("search"), "{detail}");
    assert!(detail.contains("torrent engine"), "{detail}");
    assert_eq!(
        response["result"]["track_count"],
        serde_json::Value::Null,
        "a torrent has no catalog; no number must be made up"
    );

    plugin.shutdown();
}

#[test]
fn a_bad_id_is_refused_without_starting_a_download() {
    let mut plugin = Plugin::start();
    plugin.handshake(serde_json::json!({}));

    let response = plugin.call(2, "resolve_source", serde_json::json!({"id": "hello"}));
    assert!(
        response["error"]["message"]
            .as_str()
            .is_some_and(|text| text.contains("not an infohash")),
        "{response}"
    );

    plugin.shutdown();
}

#[test]
fn a_malformed_line_is_skipped_instead_of_killing_the_process() {
    let mut plugin = Plugin::start();
    plugin.handshake(serde_json::json!({}));

    writeln!(plugin.stdin, "this is not json").expect("the broken line must be written");
    plugin.stdin.flush().expect("must be sent");

    let alive = plugin.call(2, "health", serde_json::json!({}));
    assert!(alive["result"].is_object(), "{alive}");

    plugin.shutdown();
}
