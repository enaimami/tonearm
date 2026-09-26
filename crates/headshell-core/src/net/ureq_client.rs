//! The concrete client of the `http-client` feature: `ureq` + rustls (D-020).
//!
//! **This file is the only place that touches `ureq`.** If the crate changes,
//! this changes; providers see the [`super::HttpClient`] trait and are not
//! affected.
//!
//! ## A note on blocking
//!
//! `ureq` is a blocking API. [`UreqClient::send`] blocks inside an async
//! signature — since the core does not choose a runtime (it does not set up
//! `#[tokio::main]`) we cannot call `spawn_blocking`. For the CLI this is not
//! a problem: the command is waiting for the answer anyway. A caller that
//! wants a non-blocking transport (the GUI's event loop, mobile) supplies its
//! own `HttpClient`; the trait boundary is what makes this trade reversible.

use std::io::Read;
use std::time::Duration;

use super::{HttpClient, HttpFuture, HttpHeader, HttpMethod, HttpRequest, HttpResponse};
use crate::error::Result;

/// The cap for metadata responses. Audio bytes do not go through here (see
/// [`UreqClient::open_stream`]); a hostile server must not fill memory.
const MAX_BODY_BYTES: u64 = 32 * 1024 * 1024;

/// The general timeout: connecting + reading. If the server hangs, the
/// command must not hang with it.
const TIMEOUT: Duration = Duration::from_secs(30);

/// The time allowed for plugin requests — it must stay below the call budget
/// (20 s).
const PLUGIN_TIMEOUT: Duration = Duration::from_secs(15);

/// Time limits for requests whose body takes long: artifact downloads and
/// audio streams.
///
/// The general timeout is no use here, because it covers the body too: a
/// 40 MB binary or FLAC easily takes more than 30 seconds on a slow
/// connection.
///
/// **There is no total limit**, on purpose: in ureq 3.4 an expired deadline is
/// not an error but becomes a 1 s read timeout, so no total limit ever cuts a
/// body that keeps flowing (measured, D-069 addendum). The protection against
/// stalling is the silence limit; the protection against size is the caller's
/// cap (artifacts 128 MB, streams 256 MB). A slow but flowing download is not
/// cut off — which is also what is right for the user.
#[derive(Debug, Clone, Copy)]
struct LongBody {
    /// The limit for resolving the name, connecting, sending the request and
    /// waiting for the response headers: a server that never answers does not
    /// leave us stuck for longer than this.
    handshake: Duration,
    /// The longest silence between two reads of the body.
    idle: Duration,
}

/// The shared time limits of artifact downloads and audio streams.
const LONG_BODY: LongBody = LongBody {
    handshake: TIMEOUT,
    idle: TIMEOUT,
};

/// A `ureq`-based HTTP client.
pub struct UreqClient {
    agent: ureq::Agent,
}

impl std::fmt::Debug for UreqClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UreqClient")
    }
}

impl Default for UreqClient {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqClient {
    #[must_use]
    pub fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            // We want the status code as data, not as an error: 401 and "could not
            // connect" are different diagnoses (K9).
            .http_status_as_error(false)
            .user_agent(concat!("headshell/", env!("CARGO_PKG_VERSION")))
            .build();
        Self {
            agent: ureq::Agent::new_with_config(config),
        }
    }

    /// The client given to plugins: it **does not follow redirects** (D-069).
    ///
    /// The permission check looks at the request address; if the client followed
    /// redirects itself, an allowed address could carry the plugin somewhere it
    /// is not allowed. A 3xx comes back as it is, and the engine follows each
    /// step and asks again.
    ///
    /// The time limit is shorter too: a plugin call's budget is 20 s and a single
    /// request must not eat all of it — a slow request should come back to the
    /// plugin as an error, rather than the call itself timing out.
    #[must_use]
    pub fn without_redirects() -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(PLUGIN_TIMEOUT))
            .http_status_as_error(false)
            .max_redirects(0)
            .user_agent(concat!("headshell/", env!("CARGO_PKG_VERSION")))
            .build();
        Self {
            agent: ureq::Agent::new_with_config(config),
        }
    }

    /// The client for the engine's artifact downloads (D-069).
    #[must_use]
    pub fn for_downloads() -> Self {
        Self::long_body(LONG_BODY)
    }

    /// The client for audio streams (with [`Self::open_stream`]).
    ///
    /// [`Self::new`] used to be used, and its 30 s general timeout covered the
    /// body too: a track that did not download completely in 30 s (FLAC from a
    /// distant server, anything over a slow connection) was cut off halfway.
    #[must_use]
    pub fn for_streams() -> Self {
        Self::long_body(LONG_BODY)
    }

    /// A client without a general timeout, limited stage by stage.
    ///
    /// Waiting for the headers is limited with `timeout_send_request`, **not**
    /// with `timeout_recv_response`. ureq 3.4 checks a stage's time limit in the
    /// following stage too, and counts it from the moment that stage *ended*:
    /// `recv_response` limited the body as well, counting from the moment the
    /// headers arrived. In the download client it was 30 s, and every artifact
    /// that did not download within 30 s was cut off with "timeout: receive
    /// response" (D-069 addendum). The same rule carries `send_request` over to
    /// the header wait and leaves it there: the only predecessor of the body is
    /// `recv_response`. `recv_body` is counted again on every read, so it is a
    /// silence limit, not a total. If the rule changes with ureq, the tests below
    /// fail.
    fn long_body(limits: LongBody) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_resolve(Some(limits.handshake))
            .timeout_connect(Some(limits.handshake))
            .timeout_send_request(Some(limits.handshake))
            .timeout_recv_body(Some(limits.idle))
            .http_status_as_error(false)
            .user_agent(concat!("headshell/", env!("CARGO_PKG_VERSION")))
            .build();
        Self {
            agent: ureq::Agent::new_with_config(config),
        }
    }

    fn send_blocking(&self, request: &HttpRequest) -> Result<HttpResponse> {
        let mut response = match request.method {
            HttpMethod::Get => {
                let mut builder = self.agent.get(&request.url);
                for header in &request.headers {
                    builder = builder.header(&header.name, &header.value);
                }
                builder.call()
            }
            HttpMethod::Post => {
                let mut builder = self.agent.post(&request.url);
                for header in &request.headers {
                    builder = builder.header(&header.name, &header.value);
                }
                match &request.body {
                    Some(body) => builder.send(&body[..]),
                    None => builder.send_empty(),
                }
            }
        }
        .map_err(|err| super::network_err(&request.url, err))?;

        let status = response.status().as_u16();
        let headers = collect_headers(response.headers());
        let body = response
            .body_mut()
            .with_config()
            .limit(MAX_BODY_BYTES)
            .read_to_vec()
            .map_err(|err| super::network_err(&request.url, err))?;

        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }

    /// Opens a **progressive** reader for an audio stream.
    ///
    /// Separate from [`HttpClient::send`], because that one takes the body into
    /// memory whole; downloading a 40 MB FLAC before starting to play it is the
    /// opposite of "gapless playback". It returns a pair: the known total length
    /// (if any) and the reader.
    ///
    /// # Errors
    /// If no connection can be made or the server returns a code outside 2xx.
    pub fn open_stream(
        &self,
        url: &str,
        headers: &[HttpHeader],
    ) -> Result<(Option<u64>, Box<dyn Read + Send>)> {
        let mut builder = self.agent.get(url);
        for header in headers {
            builder = builder.header(&header.name, &header.value);
        }
        // A stream address carries its key in the query string (Subsonic's
        // token, a service's signature): errors name it without.
        let shown = super::without_query(url);
        let response = builder
            .call()
            .map_err(|err| super::network_err(shown, err.to_string().replace(url, shown)))?;

        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            // Producing an error without reading the body leaves "what did it say?"
            // unanswered; we take a short piece of it.
            let mut response = response;
            let detail = response
                .body_mut()
                .with_config()
                .limit(1024)
                .read_to_string()
                .unwrap_or_else(|err| format!("(could not read the body: {err})"));
            // The stage is chosen by the same rule as in `error_for_status` (D-023):
            // getting a 401 while opening a stream is a credential problem too, not a
            // network problem.
            return Err(crate::Error::new(
                super::stage_for_status(status),
                crate::ErrorKind::HttpStatus {
                    url: shown.to_owned(),
                    status,
                    detail,
                },
            ));
        }

        let length = response
            .headers()
            .get("content-length")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());

        Ok((length, Box::new(response.into_body().into_reader())))
    }
}

impl crate::plugin::artifact::ArtifactSource for UreqClient {
    fn open(&self, url: &str) -> Result<crate::plugin::artifact::ArtifactResponse> {
        let response = self
            .agent
            .get(url)
            .call()
            .map_err(|err| super::network_err(url, err))?;
        let status = response.status().as_u16();
        let length = response
            .headers()
            .get("content-length")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        Ok(crate::plugin::artifact::ArtifactResponse {
            status,
            length,
            body: Box::new(response.into_body().into_reader()),
        })
    }
}

impl HttpClient for UreqClient {
    fn send<'a>(&'a self, request: &'a HttpRequest) -> HttpFuture<'a> {
        // The blocking is deliberate and documented (module header).
        Box::pin(std::future::ready(self.send_blocking(request)))
    }
}

fn collect_headers(map: &ureq::http::HeaderMap) -> Vec<HttpHeader> {
    map.iter()
        .map(|(name, value)| {
            HttpHeader::new(
                name.as_str(),
                // We **do not drop** a header with invalid UTF-8: its presence is
                // information too. The lossy form is for diagnostics.
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! The time limits are tested against a real local server: which limit ureq
    //! counts at which stage could not be read from its documentation; it was
    //! found by measuring.

    use std::io::{BufRead as _, BufReader, Read as _, Write as _};
    use std::net::{TcpListener, TcpStream};
    use std::time::{Duration, Instant};

    use super::{LongBody, UreqClient};
    use crate::plugin::artifact::ArtifactSource as _;

    const LIMITS: LongBody = LongBody {
        handshake: Duration::from_millis(500),
        idle: Duration::from_secs(2),
    };

    /// A local server for a single connection: reads the request's headers and
    /// hands the connection to `serve`.
    fn serve_once(serve: impl FnOnce(TcpStream) + Send + 'static) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let mut request = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            // The headers end with an empty line.
            while request.read_line(&mut line).unwrap_or(0) > 2 {
                line.clear();
            }
            serve(stream);
        });
        format!("http://{addr}/artifact")
    }

    fn send_headers(stream: &mut TcpStream, length: usize) {
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        stream.flush().unwrap();
    }

    /// D-069 addendum: the body used to be cut off 30 s after the headers.
    #[test]
    fn a_body_slower_than_the_handshake_budget_still_arrives_whole() {
        // 10 × 150 ms: the body downloads over three times the header wait limit.
        let url = serve_once(|mut stream| {
            send_headers(&mut stream, 10 * 1024);
            for _ in 0..10 {
                std::thread::sleep(Duration::from_millis(150));
                if stream.write_all(&[7; 1024]).is_err() {
                    return;
                }
            }
        });
        let mut body = UreqClient::long_body(LIMITS).open(&url).unwrap().body;
        let mut bytes = Vec::new();
        body.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes.len(), 10 * 1024);
    }

    /// A stream refused: the error says where, not with which key.
    #[test]
    fn a_refused_stream_is_named_without_its_query_string() {
        let url = serve_once(|mut stream| {
            let _ = write!(
                stream,
                "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
        });
        let url = format!("{url}?u=enai&t=26719a1196d2a940&s=c19b2d&id=a1");
        let Err(err) = UreqClient::for_streams().open_stream(&url, &[]) else {
            panic!("a 401 must be an error");
        };
        let text = err.chain_text();
        assert!(text.contains("401") && text.contains("/artifact"), "{text}");
        assert!(
            !text.contains("26719a") && !text.contains("c19b2d"),
            "the key must not reach the error: {text}"
        );
    }

    #[test]
    fn a_server_that_never_answers_does_not_hold_the_request() {
        let url = serve_once(|stream| {
            std::thread::sleep(Duration::from_secs(4));
            drop(stream);
        });
        let started = Instant::now();
        let Err(err) = UreqClient::long_body(LIMITS).open(&url) else {
            panic!("a server that does not answer must be an error");
        };
        let elapsed = started.elapsed();
        assert!(err.chain_text().contains("timeout"), "{}", err.chain_text());
        assert!(elapsed < Duration::from_millis(2500), "{elapsed:?}");
    }

    #[test]
    fn a_body_that_goes_silent_fails_after_the_idle_budget() {
        let limits = LongBody {
            idle: Duration::from_millis(300),
            ..LIMITS
        };
        let url = serve_once(|mut stream| {
            send_headers(&mut stream, 4096);
            let _ = stream.write_all(&[1; 1024]);
            std::thread::sleep(Duration::from_secs(4));
        });
        let started = Instant::now();
        let mut body = UreqClient::long_body(limits).open(&url).unwrap().body;
        let mut bytes = Vec::new();
        let err = body.read_to_end(&mut bytes).unwrap_err();
        let elapsed = started.elapsed();
        assert_eq!(
            bytes.len(),
            1024,
            "the part that arrived must have been read"
        );
        assert!(err.to_string().contains("timeout"), "{err}");
        assert!(elapsed < Duration::from_millis(2500), "{elapsed:?}");
    }
}
