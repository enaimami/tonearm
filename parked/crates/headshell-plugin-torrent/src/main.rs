//! The torrent plugin's thin shell: read a line from stdin, dispatch it, write
//! the answer.
//!
//! All the logic is in `lib.rs` and the modules under it. The split is the
//! Golden Rule as it applies to a plugin: nothing deleted from here should
//! take a capability with it — the integration tests call the same handlers
//! from the library.

use headshell_core::plugin::protocol::method;
use headshell_plugin_torrent::rpc::{CODE_METHOD_NOT_FOUND, CODE_PLUGIN_ERROR, Incoming};
use headshell_plugin_torrent::{App, dispatch, rpc};
use tokio::io::{AsyncBufReadExt, BufReader};

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    // stdout belongs to the protocol; every log line goes to stderr. The core
    // forwards stderr into `tracing`, so librqbit's diagnostics reach
    // `headshell diag`.
    let filter = tracing_subscriber::EnvFilter::try_from_env("HEADSHELL_TORRENT_LOG")
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
            // EOF: the core closed the pipe, we exit cleanly.
            Ok(None) => return,
            Err(error) => {
                eprintln!("could not read stdin: {error}");
                return;
            }
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(incoming) = serde_json::from_str::<Incoming>(line) else {
            eprintln!("skipped a line that could not be parsed");
            continue;
        };

        let method_name = incoming.method.clone().unwrap_or_default();
        if method_name == method::SHUTDOWN {
            return;
        }

        let params = incoming.params.clone().unwrap_or(serde_json::Value::Null);
        match dispatch(&mut app, &method_name, params).await {
            Ok(Some(result)) => rpc::reply(incoming.id, result),
            // An unrecognised method: the protocol reads this as "I do not support
            // this capability" and the core does not crash.
            Ok(None) => rpc::fail(
                incoming.id,
                CODE_METHOD_NOT_FOUND,
                &format!("no such method: {method_name}"),
            ),
            Err(error) => rpc::fail(incoming.id, CODE_PLUGIN_ERROR, &error.to_string()),
        }
    }
}
