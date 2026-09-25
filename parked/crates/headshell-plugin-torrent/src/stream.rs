//! A small HTTP/1.1 server serving sequential streams on `127.0.0.1` (D-047
//! S2).
//!
//! ## Why it exists
//!
//! `resolve_source` has to return a ready `AudioSource`, while a torrent is an
//! acquisition job that takes minutes. `librqbit`'s `FileStream` sets piece
//! priority by the read position — so playback can start the moment the start
//! of the file is in hand. In api 1, the only way to hand that stream to the
//! player **without changing the protocol** is to give an address.
//!
//! ## Not a K3 violation
//!
//! Nothing is relayed: the user's own machine fetches the data, and the server
//! is on the same machine, bound only to the local interface. A *design* that
//! streams audio from a server is what K3 forbids; this is a pipe inside the
//! process wearing HTTP clothes.
//!
//! ## Why there is a token
//!
//! Another process on the same machine could read what was downloaded by
//! trying `127.0.0.1:<port>/<infohash>/0`. The path starts with a random token
//! that lives as long as the process; the token is not written to disk.

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

use crate::engine::Engine;
use crate::rpc::{PluginError, Result, chain_text};

/// The upper limit for request headers. A request over it is refused unread.
const MAX_HEADER_BYTES: usize = 8 * 1024;
/// The copy buffer. Being smaller than a piece is no problem; `FileStream`
/// blocks while waiting for a missing piece, and we wait for it.
const COPY_BUFFER: usize = 64 * 1024;

pub struct StreamServer {
    addr: SocketAddr,
    token: String,
}

impl StreamServer {
    /// Starts the server and returns **after** it starts listening —
    /// `resolve_source` cannot answer before it has the address.
    pub async fn spawn(engine: Arc<Engine>) -> Result<Arc<Self>> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.map_err(|error| {
            PluginError::new(format!("could not bind the local stream server: {error}"))
        })?;
        let addr = listener
            .local_addr()
            .map_err(|error| PluginError::new(format!("could not read the local stream address: {error}")))?;

        let token = format!(
            "{:016x}{:016x}",
            rand::random::<u64>(),
            rand::random::<u64>()
        );
        let server = Arc::new(Self { addr, token });

        let accept_server = Arc::clone(&server);
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, _peer)) => {
                        let engine = Arc::clone(&engine);
                        let server = Arc::clone(&accept_server);
                        tokio::spawn(async move {
                            if let Err(error) = server.serve(socket, engine).await {
                                // The player closing the connection is normal; still, we
                                // write it to stderr, because half of the "it doesn't play"
                                // diagnosis is here.
                                eprintln!("a stream request dropped: {error}");
                            }
                        });
                    }
                    Err(error) => {
                        eprintln!("the stream server could not accept a connection: {error}");
                        return;
                    }
                }
            }
        });

        Ok(server)
    }

    /// A file's playable address.
    pub fn url_for(&self, infohash: &str, file_index: usize) -> String {
        format!(
            "http://{}/{}/{infohash}/{file_index}",
            self.addr, self.token
        )
    }

    async fn serve(&self, socket: TcpStream, engine: Arc<Engine>) -> Result<()> {
        let mut socket = BufReader::new(socket);
        let request = match read_request(&mut socket).await? {
            Some(request) => request,
            None => return Ok(()),
        };

        let Some(route) = self.route(&request.path) else {
            // If the token is wrong we say "not found", not "forbidden":
            // even confirming that it exists is information.
            return respond_status(&mut socket, 404, "not found").await;
        };

        let (source_url, from_catalog) = engine.source_for(&route.infohash).await;
        if !from_catalog {
            eprintln!(
                "{}: not in the catalog, trying with a bare magnet (DHT only)",
                route.infohash
            );
        }
        let handle = match engine.handle(&route.infohash, &source_url).await {
            Ok(handle) => handle,
            Err(error) => {
                eprintln!("{}: {error}", route.infohash);
                return respond_status(&mut socket, 503, "the torrent is not ready").await;
            }
        };

        let files = Engine::audio_files(&handle)?;
        let Some(file) = files.iter().find(|file| file.index == route.file_index) else {
            return respond_status(&mut socket, 404, "no such file").await;
        };
        let total = file.len;
        let content_type = content_type_for(&file.file_name);

        let range = match request.range.as_deref().map(|raw| parse_range(raw, total)) {
            Some(Ok(range)) => Some(range),
            Some(Err(())) => {
                // 416: the requested range is outside the file. Silently sending
                // from the start takes the player to the wrong position.
                let head = format!(
                    "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{total}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                return write_all(&mut socket, head.as_bytes()).await;
            }
            None => None,
        };

        let (start, end) = range.unwrap_or((0, total.saturating_sub(1)));
        let length = end.saturating_sub(start).saturating_add(1);

        let mut head = String::new();
        if range.is_some() {
            head.push_str("HTTP/1.1 206 Partial Content\r\n");
            head.push_str(&format!("Content-Range: bytes {start}-{end}/{total}\r\n"));
        } else {
            head.push_str("HTTP/1.1 200 OK\r\n");
        }
        head.push_str(&format!("Content-Type: {content_type}\r\n"));
        head.push_str(&format!("Content-Length: {length}\r\n"));
        head.push_str("Accept-Ranges: bytes\r\n");
        head.push_str("Connection: close\r\n\r\n");
        write_all(&mut socket, head.as_bytes()).await?;

        if request.head_only {
            return Ok(());
        }

        let mut stream = handle
            .clone()
            .stream(route.file_index)
            .await
            .map_err(|error| PluginError::new(format!("could not open the stream: {}", chain_text(&error))))?;
        if start > 0 {
            stream
                .seek(std::io::SeekFrom::Start(start))
                .await
                .map_err(|error| PluginError::new(format!("could not seek in the stream: {error}")))?;
        }

        let mut remaining = length;
        let mut buffer = vec![0_u8; COPY_BUFFER];
        while remaining > 0 {
            let want = usize::try_from(remaining.min(COPY_BUFFER as u64)).unwrap_or(COPY_BUFFER);
            let read = stream
                .read(&mut buffer[..want])
                .await
                .map_err(|error| PluginError::new(format!("could not read the stream: {error}")))?;
            if read == 0 {
                // The file ended shorter than expected: swallowing this would
                // make the player say "the track ended" and hide the reason.
                return Err(PluginError::new(format!(
                    "the stream ended {remaining} bytes short ({})",
                    route.infohash
                )));
            }
            write_all(&mut socket, &buffer[..read]).await?;
            remaining -= read as u64;
        }
        socket
            .get_mut()
            .flush()
            .await
            .map_err(|error| PluginError::new(format!("could not flush the stream: {error}")))
    }

    fn route(&self, path: &str) -> Option<Route> {
        let rest = path.strip_prefix('/')?;
        let (token, rest) = rest.split_once('/')?;
        // No constant-time comparison needed: the token is new in every
        // process and the attacker is local anyway; still, we accept
        // nothing but equality.
        if token != self.token {
            return None;
        }
        let (infohash, index) = rest.split_once('/')?;
        let infohash = infohash.to_ascii_lowercase();
        if !crate::torznab::is_infohash(&infohash) {
            return None;
        }
        Some(Route {
            infohash,
            file_index: index.split('?').next()?.parse().ok()?,
        })
    }
}

struct Route {
    infohash: String,
    file_index: usize,
}

struct HttpRequest {
    path: String,
    range: Option<String>,
    head_only: bool,
}

async fn read_request(socket: &mut BufReader<TcpStream>) -> Result<Option<HttpRequest>> {
    let mut line = String::new();
    let mut consumed = 0_usize;
    let read = socket
        .read_line(&mut line)
        .await
        .map_err(|error| PluginError::new(format!("could not read the request line: {error}")))?;
    if read == 0 {
        return Ok(None);
    }
    consumed += read;

    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_ascii_uppercase();
    let path = parts.next().unwrap_or_default().to_owned();
    if method != "GET" && method != "HEAD" {
        return Ok(Some(HttpRequest {
            path: String::new(),
            range: None,
            head_only: true,
        }));
    }

    let mut range = None;
    loop {
        let mut header = String::new();
        let read = socket
            .read_line(&mut header)
            .await
            .map_err(|error| PluginError::new(format!("could not read a header: {error}")))?;
        if read == 0 {
            break;
        }
        consumed += read;
        if consumed > MAX_HEADER_BYTES {
            return Err(PluginError::new("the request headers are too long"));
        }
        let trimmed = header.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("range") {
                range = Some(value.trim().to_owned());
            }
        }
    }

    Ok(Some(HttpRequest {
        path,
        range,
        head_only: method == "HEAD",
    }))
}

async fn respond_status(socket: &mut BufReader<TcpStream>, code: u16, reason: &str) -> Result<()> {
    let body = reason.as_bytes();
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    write_all(socket, head.as_bytes()).await?;
    write_all(socket, body).await
}

async fn write_all(socket: &mut BufReader<TcpStream>, bytes: &[u8]) -> Result<()> {
    socket
        .get_mut()
        .write_all(bytes)
        .await
        .map_err(|error| PluginError::new(format!("could not write the response: {error}")))
}

/// Parses `bytes=start-end`. `Err(())` = the range is outside the file (416).
///
/// Multiple ranges (`bytes=0-1,5-6`) are not supported: players do not use
/// them, and supporting them halfway is worse than not supporting them.
fn parse_range(raw: &str, total: u64) -> std::result::Result<(u64, u64), ()> {
    let spec = raw.trim().strip_prefix("bytes=").ok_or(())?;
    if spec.contains(',') {
        return Err(());
    }
    let (start, end) = spec.split_once('-').ok_or(())?;
    let (start, end) = (start.trim(), end.trim());

    if start.is_empty() {
        // `bytes=-500`: the last 500 bytes.
        let suffix: u64 = end.parse().map_err(|_| ())?;
        if suffix == 0 || total == 0 {
            return Err(());
        }
        return Ok((total.saturating_sub(suffix), total - 1));
    }

    let start: u64 = start.parse().map_err(|_| ())?;
    if start >= total {
        return Err(());
    }
    let end = if end.is_empty() {
        total - 1
    } else {
        end.parse::<u64>().map_err(|_| ())?.min(total - 1)
    };
    if end < start {
        Err(())
    } else {
        Ok((start, end))
    }
}

fn content_type_for(file_name: &str) -> &'static str {
    let extension = file_name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "flac" => "audio/flac",
        "mp3" => "audio/mpeg",
        "ogg" | "opus" => "audio/ogg",
        "m4a" | "aac" | "alac" => "audio/mp4",
        "wav" => "audio/wav",
        "aiff" | "aif" => "audio/aiff",
        "wv" => "audio/x-wavpack",
        "ape" => "audio/x-ape",
        // An unknown extension: instead of lying we say "a heap of bytes".
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_range_is_inclusive_on_both_ends() {
        assert_eq!(parse_range("bytes=0-99", 1000), Ok((0, 99)));
        assert_eq!(parse_range("bytes=500-", 1000), Ok((500, 999)));
    }

    #[test]
    fn a_suffix_range_counts_from_the_end() {
        assert_eq!(parse_range("bytes=-100", 1000), Ok((900, 999)));
    }

    #[test]
    fn an_end_past_the_file_is_clamped_not_rejected() {
        assert_eq!(parse_range("bytes=900-99999", 1000), Ok((900, 999)));
    }

    #[test]
    fn a_start_past_the_file_is_a_416_not_a_silent_restart() {
        assert_eq!(parse_range("bytes=1000-", 1000), Err(()));
        assert_eq!(parse_range("bytes=5000-6000", 1000), Err(()));
    }

    #[test]
    fn a_multi_range_request_is_refused_rather_than_half_answered() {
        assert_eq!(parse_range("bytes=0-1,5-6", 1000), Err(()));
    }

    #[test]
    fn a_malformed_range_is_refused() {
        assert_eq!(parse_range("octets=0-1", 1000), Err(()));
        assert_eq!(parse_range("bytes=abc-def", 1000), Err(()));
    }

    #[test]
    fn known_audio_extensions_get_a_real_content_type_and_others_do_not_lie() {
        assert_eq!(content_type_for("a.FLAC"), "audio/flac");
        assert_eq!(content_type_for("a.mp3"), "audio/mpeg");
        assert_eq!(content_type_for("a.whoknows"), "application/octet-stream");
        assert_eq!(content_type_for("no-extension"), "application/octet-stream");
    }

    #[test]
    fn a_url_carries_the_token_and_only_binds_locally() {
        let server = StreamServer {
            addr: "127.0.0.1:1234".parse().unwrap(),
            token: "abc".to_owned(),
        };
        let hash = "b".repeat(40);
        let url = server.url_for(&hash, 3);
        assert_eq!(url, format!("http://127.0.0.1:1234/abc/{hash}/3"));
        assert!(server.addr.ip().is_loopback());
    }

    #[test]
    fn a_wrong_token_does_not_route() {
        let server = StreamServer {
            addr: "127.0.0.1:1".parse().unwrap(),
            token: "right".to_owned(),
        };
        let hash = "c".repeat(40);
        assert!(server.route(&format!("/wrong/{hash}/0")).is_none());
        assert!(server.route(&format!("/right/{hash}/0")).is_some());
    }

    #[test]
    fn a_path_that_is_not_an_infohash_does_not_route() {
        let server = StreamServer {
            addr: "127.0.0.1:1".parse().unwrap(),
            token: "t".to_owned(),
        };
        assert!(server.route("/t/short/0").is_none());
        assert!(server.route("/t/../../etc/passwd/0").is_none());
    }

    #[test]
    fn a_query_string_after_the_index_is_ignored_not_fatal() {
        let server = StreamServer {
            addr: "127.0.0.1:1".parse().unwrap(),
            token: "t".to_owned(),
        };
        let hash = "d".repeat(40);
        let route = server.route(&format!("/t/{hash}/2?x=1")).unwrap();
        assert_eq!(route.file_index, 2);
    }
}
