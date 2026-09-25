//! AcoustID — the **4th and last link** of the K6 chain.
//!
//! The first three links look at text, and all three assume the tags are
//! right. On an untagged file named `track01.mp3` all three are helpless: no
//! ISRC, no artist/title to search for, no text to fuzzy-match. This link
//! looks at **the audio itself** — [`fingerprint`](super::fingerprint)
//! extracts a Chromaprint fingerprint from the file, this module asks
//! AcoustID about it and gets MusicBrainz recording ids in return.
//!
//! The order is no accident: the fingerprint is the most expensive link (the
//! whole file is decoded) and **cannot work at all without the file at
//! hand**. Since an imported listening history has no files, those records
//! never reach this link — the chain getting this far is the exception, not
//! the rule.
//!
//! ## The client key (D-046)
//!
//! AcoustID asks for an application key with every query. It has two
//! sources, in this order: first the secret store (`identity:acoustid` /
//! `api_key`), otherwise the default embedded in the build. The key the user
//! set **always** wins — so nobody is locked out if the embedded key is
//! revoked or its quota runs out.
//!
//! If neither exists the link does not run, and this **is said**: "no
//! AcoustID key" and "AcoustID found no match" are two entirely different
//! diagnoses, and the first looking like the second sends people hunting for
//! the fault in the file (K9).
//!
//! ## Limits
//!
//! - **Three requests per second.** AcoustID's published average; a client
//!   that exceeds it gets a `429`. [`crate::net::RateLimiter`] sleeps between
//!   calls.
//! - **POST, not GET.** A base64-encoded fingerprint takes thousands of
//!   characters; putting it in the URL would hand it over to the cutting
//!   limits of proxies.

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;

use super::fingerprint::Fingerprint;
use super::{Candidate, FingerprintCandidate, FingerprintLookup, LookupFuture};
use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};
use crate::ids::Mbid;
use crate::net::{HttpClient, HttpHeader, HttpRequest, RateLimiter, encode_query, parse_json};

/// The public AcoustID server.
pub const DEFAULT_BASE_URL: &str = "https://api.acoustid.org/v2";

/// The secret namespace the key is looked up in (the D-042 store).
pub const SECRET_NAMESPACE: &str = "identity:acoustid";

/// The name of the key in the secret store.
pub const SECRET_KEY: &str = "api_key";

/// The default client key embedded in the build.
///
/// **It is left empty, and on purpose.** The key has to be registered for this
/// project at `acoustid.org/new-application` and written here; putting in a
/// made-up string would come back as "invalid key" on the first live call
/// and send people hunting for the fault in the fingerprint instead of the
/// key.
///
/// As long as it stays empty the 4th link only works with the user's own key,
/// and a call without a key is refused with [`missing_key_err`].
const EMBEDDED_API_KEY: &str = "";

/// The shortest time between two requests.
///
/// AcoustID allows three requests per second on average. 340 ms leaves room
/// for clock skew — a client that grazes the limit eats a `429` and starts
/// retrying, which is slower overall (learned at MusicBrainz).
const MIN_INTERVAL: Duration = Duration::from_millis(340);

/// How many times to retry after a `429` (rate limit).
const RATE_LIMIT_RETRIES: u32 = 2;

/// AcoustID matches below this score do not count as candidates at all.
///
/// AcoustID gives its own confidence between 0 and 1 and lists weak matches
/// too. Below 0.5 in practice means "might be the same track, might not";
/// putting it into the chain as a candidate would tie the identity to a
/// guess.
const MIN_ACOUSTID_SCORE: f64 = 0.5;

/// A fingerprint source that connects to AcoustID.
///
/// It goes through [`HttpClient`], not straight to HTTP (D-020): tests supply
/// a fake client, mobile supplies its own stack.
pub struct AcoustIdLookup {
    http: Arc<dyn HttpClient>,
    base_url: String,
    api_key: String,
    user_agent: String,
    limiter: RateLimiter,
}

impl std::fmt::Debug for AcoustIdLookup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The key **is not printed**: `Debug` output can end up in the log
        // and the diagnostics report, and secrets do not go there (D-042).
        f.debug_struct("AcoustIdLookup")
            .field("base_url", &self.base_url)
            .field("api_key_set", &!self.api_key.is_empty())
            .finish_non_exhaustive()
    }
}

impl AcoustIdLookup {
    /// A source connecting to the public server with the embedded default key.
    ///
    /// If the embedded key is empty, calls return [`missing_key_err`] — the
    /// object is still built, because [`Self::with_api_key`] can fix it.
    #[must_use]
    pub fn new(http: Arc<dyn HttpClient>) -> Self {
        Self {
            http,
            base_url: DEFAULT_BASE_URL.to_owned(),
            api_key: EMBEDDED_API_KEY.to_owned(),
            user_agent: default_user_agent(),
            limiter: RateLimiter::new(MIN_INTERVAL),
        }
    }

    /// The user's own key. An empty string **is ignored**.
    ///
    /// Ignoring empty means that "I tried to set a key but left it empty" falls
    /// back to the embedded key; accepting empty as valid would carry the call
    /// all the way to the server and get it refused there.
    #[must_use]
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        let key = api_key.into();
        if !key.trim().is_empty() {
            self.api_key = key.trim().to_owned();
        }
        self
    }

    /// Another server (a test server or your own copy).
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_owned();
        self
    }

    /// Your own `User-Agent`.
    #[must_use]
    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }

    /// The shortest time between requests. For tests only.
    #[must_use]
    pub fn with_min_interval(mut self, interval: Duration) -> Self {
        self.limiter = RateLimiter::new(interval);
        self
    }

    /// Looks up a fingerprint.
    async fn lookup(&self, fingerprint: &Fingerprint) -> Result<Vec<FingerprintCandidate>> {
        if self.api_key.is_empty() {
            return Err(missing_key_err());
        }

        let url = format!("{}/lookup", self.base_url);
        // `meta=recordings`: the identity chain works at the recording
        // level. We do not ask for release information — asking multiplies
        // the response and carries data the chain does not use.
        let body = format!(
            "client={}&duration={}&fingerprint={}&meta=recordings",
            encode_query(&self.api_key),
            fingerprint.duration_secs,
            encode_query(&fingerprint.to_acoustid_string()),
        );
        let request = HttpRequest::post_form(&url, body).with_headers(vec![
            HttpHeader::new("User-Agent", self.user_agent.clone()),
            HttpHeader::new("Accept", "application/json"),
        ]);

        let mut attempt = 0;
        let payload: LookupResponse = loop {
            self.limiter.acquire();
            let response = self.http.send(&request).await?;

            // `429` = quota exceeded. The server says "slow down", not "no".
            if response.status == 429 && attempt < RATE_LIMIT_RETRIES {
                attempt += 1;
                tracing::warn!(url, attempt, "AcoustID rate limit: waiting and retrying");
                std::thread::sleep(MIN_INTERVAL);
                continue;
            }
            // The body is read **before the status code**, and that is what the
            // live run taught (D-046): AcoustID returns `400` for an invalid key,
            // and the real diagnosis (`invalid API key`) is in the body. Calling
            // `error_for_status` first reported it as `NETWORK_REQUEST` — when
            // the server was reached, read the request and **said no**. The same
            // distinction D-023 made for `401`/`403`: "I could not go online"
            // sends the user off to check their connection, when what they need
            // to do is fix their key.
            if let Ok(payload) = serde_json::from_slice::<LookupResponse>(&response.body) {
                break payload;
            }
            // A body that cannot be parsed: whatever the status code says. A
            // proxy error page, a truncated response, a maintenance screen —
            // none of them is AcoustID's own answer.
            response.error_for_status(&url)?;
            break parse_json::<LookupResponse>(&response, "acoustid lookup")?;
        };

        // AcoustID reports an application error in the body with
        // `status: error`. Ignoring it would report an invalid key as "no
        // match" — exactly the mix-up K9 forbids.
        if payload.status != "ok" {
            let detail = payload
                .error
                .map_or_else(|| "no reason given".to_owned(), |err| err.message);
            return Err(Error::new(
                Stage::IdentityResolve,
                ErrorKind::InvalidInput {
                    detail: format!("AcoustID refused ({url}): {detail}"),
                },
            ));
        }

        let mut out = Vec::new();
        for result in payload.results {
            if result.score < MIN_ACOUSTID_SCORE {
                continue;
            }
            for recording in result.recordings {
                let Some(mbid) = Mbid::parse(&recording.id) else {
                    // An unrecognised id is not silently thrown away: it leaves a
                    // countable warning. A silent `unwrap_or_default` is forbidden.
                    tracing::warn!(
                        id = recording.id,
                        "AcoustID returned an invalid MBID; skipping the candidate"
                    );
                    continue;
                };
                let Some(title) = recording.title else {
                    // A recording without a title cannot be scored: one of the two
                    // fields of the fuzzy comparison is missing.
                    tracing::debug!(id = %mbid, "skipping an AcoustID recording without a title");
                    continue;
                };
                let artist = recording
                    .artists
                    .into_iter()
                    .map(|artist| artist.name)
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push(FingerprintCandidate {
                    candidate: Candidate {
                        mbid,
                        artist,
                        title,
                        duration_ms: recording.duration.and_then(duration_secs_to_ms),
                        isrc: None,
                        disambiguation: None,
                    },
                    score: result.score,
                });
            }
        }

        // The strongest match first. On a tie, MBID order: arbitrary but
        // **stable** — the same file cannot get a different canonical
        // identity tomorrow (D-045's second lesson).
        out.sort_by(|left, right| {
            right.score.total_cmp(&left.score).then_with(|| {
                left.candidate
                    .mbid
                    .as_str()
                    .cmp(right.candidate.mbid.as_str())
            })
        });
        Ok(out)
    }
}

impl FingerprintLookup for AcoustIdLookup {
    fn recordings_by_fingerprint<'a>(
        &'a self,
        fingerprint: &'a Fingerprint,
    ) -> LookupFuture<'a, Vec<FingerprintCandidate>> {
        Box::pin(self.lookup(fingerprint))
    }
}

/// The error for a call without a key.
///
/// A separate function because its text is exactly this: it is where the
/// user learns what to do.
fn missing_key_err() -> Error {
    Error::new(
        Stage::IdentityResolve,
        ErrorKind::InvalidInput {
            detail: format!(
                "no AcoustID client key — the key embedded in this build is empty. \
                 Set your own key with `headshell secret set {SECRET_NAMESPACE} {SECRET_KEY} <key>` \
                 (acoustid.org/new-application)."
            ),
        },
    )
}

/// This build's default `User-Agent`.
fn default_user_agent() -> String {
    format!("headshell/{}", env!("CARGO_PKG_VERSION"))
}

/// Turns AcoustID's decimal seconds into milliseconds.
///
/// Meaningless values (negative, `NaN`, infinite, an absurd length) return
/// `None` — a made-up duration pulls fuzzy matching the wrong way, and since
/// the duration is now a tie-breaker (D-046) it could tie the identity to the
/// wrong recording too. Ignoring is better than guessing.
fn duration_secs_to_ms(secs: f64) -> Option<u64> {
    // 24 hours: a "recording" longer than this is either a data error or none
    // of our business. The upper bound also protects the `as` conversion from
    // overflowing.
    const MAX_SECS: f64 = 86_400.0;
    if !secs.is_finite() || secs <= 0.0 || secs > MAX_SECS {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the condition above guarantees it is finite, positive and under 24 hours"
    )]
    Some((secs * 1000.0).round() as u64)
}

/// The response of `POST /v2/lookup`.
#[derive(Debug, Deserialize)]
struct LookupResponse {
    status: String,
    #[serde(default)]
    error: Option<ApiError>,
    #[serde(default)]
    results: Vec<LookupResult>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct LookupResult {
    /// AcoustID's own match confidence, 0–1.
    score: f64,
    #[serde(default)]
    recordings: Vec<RecordingRef>,
}

#[derive(Debug, Deserialize)]
struct RecordingRef {
    id: String,
    #[serde(default)]
    title: Option<String>,
    /// Seconds — and it arrives **as a decimal** (`309.0`).
    ///
    /// It was written as `u32`, and the fake tests were green because they used
    /// an integer (`238`). The real service returns `"duration": 309.0`, and
    /// `serde_json` cannot decode a decimal into a `u32`: the first real match
    /// would have fallen over with a JSON error before it could be parsed (D-046
    /// addendum, a live measurement).
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    artists: Vec<ArtistRef>,
}

#[derive(Debug, Deserialize)]
struct ArtistRef {
    name: String,
}

/// This build's default AcoustID source.
///
/// # Errors
/// If the `http-client` feature is off.
#[cfg(feature = "http-client")]
pub fn default_acoustid_lookup() -> Result<Arc<dyn FingerprintLookup>> {
    Ok(Arc::new(AcoustIdLookup::new(
        crate::net::default_http_client()?,
    )))
}

/// This build's default AcoustID source.
///
/// # Errors
/// In this build `http-client` is off, so it **always** returns an error.
#[cfg(not(feature = "http-client"))]
pub fn default_acoustid_lookup() -> Result<Arc<dyn FingerprintLookup>> {
    Err(Error::new(
        Stage::IdentityResolve,
        ErrorKind::Unsupported {
            provider: "acoustid".to_owned(),
            what: "fingerprint lookup (a build with the `http-client` feature off)".to_owned(),
            capabilities: "NONE".to_owned(),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::fake::FakeHttp;

    /// A **real** AcoustID response — not written by hand, but measured.
    ///
    /// `fixtures/identity/acoustid_lookup.json` was taken from the live service
    /// (2026-09-02, `GET /v2/lookup?trackid=5e45e8ba-…&meta=recordings`).
    ///
    /// The previous version wrote the body here by hand and put an integer into
    /// the `"duration"` field. The service sends a decimal (`309.0`); since the
    /// field was `Option<u32>`, **the first real match would have fallen over
    /// with a JSON error** while every unit test stayed green. A schema is not
    /// made up, it is measured (D-046 addendum).
    ///
    /// The live test in `tests/identity_acoustid.rs` verifies that the fixture
    /// still tells the truth — the file is frozen; if the service changes, that
    /// test says so.
    const REAL_MATCH: &str = include_str!("../../../../fixtures/identity/acoustid_lookup.json");

    fn sample() -> Fingerprint {
        Fingerprint {
            raw: vec![1, 2, 3, 4, 5, 6, 7, 8],
            duration_secs: 309,
        }
    }

    /// A fixed path instead of a `pattern`: there is a single endpoint.
    fn fake(body: &str) -> Arc<FakeHttp> {
        Arc::new(FakeHttp::new().route("/v2/lookup", body))
    }

    fn lookup(http: Arc<FakeHttp>) -> AcoustIdLookup {
        AcoustIdLookup::new(http)
            .with_api_key("test-key")
            .with_base_url("https://acoustid.test/v2")
            .with_min_interval(Duration::ZERO)
    }

    #[tokio::test]
    async fn a_match_becomes_a_scored_candidate() {
        let http = fake(REAL_MATCH);
        let found = lookup(http).lookup(&sample()).await.expect("lookup");

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].candidate.title, "Sil Baştan");
        assert_eq!(found[0].candidate.artist, "Şebnem Ferah");
        // `309.0` comes as a decimal; it must convert exactly into milliseconds.
        assert_eq!(found[0].candidate.duration_ms, Some(309_000));
        assert!((found[0].score - 1.0).abs() < f64::EPSILON);
        assert_eq!(
            found[0].candidate.mbid.as_str(),
            "e0a22727-1fcf-4e3a-81a3-b65623b2c53e"
        );
    }

    /// The duration field is a **decimal**, and that is a measurement, not an
    /// assumption.
    ///
    /// A separate test because the loss is silent: while the field was
    /// `Option<u32>` the whole body could not be parsed, and the failure did not
    /// look like "no match" but like a JSON error — that is, the chain did not
    /// answer at all.
    #[tokio::test]
    async fn a_fractional_duration_is_parsed_not_rejected() {
        let fractional = REAL_MATCH.replace("\"duration\": 309.0", "\"duration\": 238.44");
        let http = fake(&fractional);
        let found = lookup(http).lookup(&sample()).await.expect("lookup");
        assert_eq!(found[0].candidate.duration_ms, Some(238_440));
    }

    /// A meaningless duration is not made up, it is dropped.
    #[test]
    fn a_nonsensical_duration_becomes_unknown_rather_than_a_wrong_number() {
        assert_eq!(duration_secs_to_ms(309.0), Some(309_000));
        assert_eq!(duration_secs_to_ms(0.0), None);
        assert_eq!(duration_secs_to_ms(-5.0), None);
        assert_eq!(duration_secs_to_ms(f64::NAN), None);
        assert_eq!(duration_secs_to_ms(f64::INFINITY), None);
        assert_eq!(duration_secs_to_ms(1e12), None);
    }

    /// The fingerprint must go in the body — not in the URL.
    ///
    /// A truncated URL looks like "no match"; we pin this with a test because
    /// at the moment of failure it cannot be told apart.
    #[tokio::test]
    async fn the_fingerprint_travels_in_the_body_not_the_url() {
        let http = fake(REAL_MATCH);
        lookup(Arc::clone(&http))
            .lookup(&sample())
            .await
            .expect("lookup");

        let request = http.last_request().expect("a request must go out");
        assert_eq!(request.method, crate::net::HttpMethod::Post);
        assert!(!request.url.contains("fingerprint"), "{}", request.url);
        let body = String::from_utf8_lossy(request.body.as_deref().unwrap_or_default()).to_string();
        assert!(body.contains("fingerprint="), "{body}");
        assert!(body.contains("duration=309"), "{body}");
    }

    /// The key must not leak into the query string: URLs get logged.
    #[tokio::test]
    async fn the_api_key_never_appears_in_the_url() {
        let http = fake(REAL_MATCH);
        lookup(Arc::clone(&http))
            .lookup(&sample())
            .await
            .expect("lookup");

        let request = http.last_request().expect("request");
        assert!(!request.url.contains("test-key"), "{}", request.url);
    }

    /// The user's key overrides the embedded one.
    #[test]
    fn a_user_key_overrides_the_embedded_one_but_an_empty_one_does_not() {
        let http = fake(REAL_MATCH);
        let set =
            AcoustIdLookup::new(Arc::clone(&http) as Arc<dyn HttpClient>).with_api_key("user");
        assert_eq!(set.api_key, "user");

        let blank = AcoustIdLookup::new(http as Arc<dyn HttpClient>).with_api_key("   ");
        assert_eq!(blank.api_key, EMBEDDED_API_KEY);
    }

    /// A call without a key must stop without going online, saying what to do.
    #[tokio::test]
    async fn a_missing_key_is_reported_before_any_request_goes_out() {
        let http = fake(REAL_MATCH);
        let bare = AcoustIdLookup::new(Arc::clone(&http) as Arc<dyn HttpClient>);
        // In a distribution with a filled-in embedded key this test would be
        // meaningless; then it is skipped and the reason is written.
        if !bare.api_key.is_empty() {
            eprintln!("skipped: this build has an embedded AcoustID key");
            return;
        }

        let err = bare.lookup(&sample()).await.unwrap_err();
        let text = err.chain_text();
        assert!(text.contains("client key"), "{text}");
        assert!(text.contains("secret set"), "{text}");
        assert!(
            http.last_request().is_none(),
            "it should not have gone online"
        );
    }

    /// An application error arriving in the body must not count as "no match".
    #[tokio::test]
    async fn an_application_error_in_the_body_is_not_an_empty_result() {
        let http = fake(r#"{"status":"error","error":{"code":4,"message":"invalid API key"}}"#);
        let err = lookup(http).lookup(&sample()).await.unwrap_err();
        let text = err.chain_text();
        assert!(text.starts_with("STEP: IDENTITY_RESOLVE"), "{text}");
        assert!(text.contains("invalid API key"), "{text}");
    }

    /// An invalid key comes with `400` — and that is not a **network** error.
    ///
    /// The flaw the live run found (D-046): the first version, reading the body
    /// after the status code, reported this as `NETWORK_REQUEST` and sent the
    /// user off to check their connection. The server was reached; it read the
    /// request and said no (the D-023 distinction).
    #[tokio::test]
    async fn a_rejected_key_arrives_as_400_and_is_still_an_identity_stage_error() {
        let http = Arc::new(FakeHttp::new().route_status(
            "/v2/lookup",
            400,
            r#"{"error": {"code": 4, "message": "invalid API key"}, "status": "error"}"#,
        ));
        let err = lookup(http).lookup(&sample()).await.unwrap_err();
        let text = err.chain_text();
        assert!(text.starts_with("STEP: IDENTITY_RESOLVE"), "{text}");
        assert!(!text.contains("NETWORK_REQUEST"), "{text}");
        assert!(text.contains("invalid API key"), "{text}");
    }

    /// A body that is not AcoustID's answer is handed over to the status code.
    ///
    /// A proxy error page, a maintenance screen, a truncated response — none of
    /// them is the service's own refusal, and none must be written to the
    /// identity stage.
    #[tokio::test]
    async fn a_body_that_is_not_acoustids_answer_falls_back_to_the_status_code() {
        let http = Arc::new(FakeHttp::new().route_status(
            "/v2/lookup",
            502,
            "<html><body>Bad Gateway</body></html>",
        ));
        let err = lookup(http).lookup(&sample()).await.unwrap_err();
        let text = err.chain_text();
        assert!(text.contains("NETWORK_REQUEST"), "{text}");
        assert!(text.contains("502"), "{text}");
    }

    /// If there really is no match, that is not an error but an empty list.
    #[tokio::test]
    async fn no_match_is_an_empty_list_not_an_error() {
        let http = fake(r#"{"status":"ok","results":[]}"#);
        let found = lookup(http).lookup(&sample()).await.expect("lookup");
        assert!(found.is_empty());
    }

    /// Weak matches do not count as candidates.
    #[tokio::test]
    async fn a_weak_match_is_not_offered_as_a_candidate() {
        let weak = REAL_MATCH.replace("\"score\": 1.0", "\"score\": 0.31");
        let http = fake(&weak);
        let found = lookup(http).lookup(&sample()).await.expect("lookup");
        assert!(found.is_empty(), "{found:?}");
    }

    /// A candidate with an invalid MBID is skipped, but the lookup does not fail.
    #[tokio::test]
    async fn a_malformed_mbid_is_skipped_without_failing_the_lookup() {
        let broken = REAL_MATCH.replace("e0a22727-1fcf-4e3a-81a3-b65623b2c53e", "not-an-mbid");
        let http = fake(&broken);
        let found = lookup(http).lookup(&sample()).await.expect("lookup");
        assert!(found.is_empty());
    }

    /// `Debug` output must not carry the key — the diagnostics report is text
    /// that gets copied and pasted (D-042).
    #[test]
    fn debug_output_does_not_leak_the_key() {
        let http = fake(REAL_MATCH);
        let text = format!("{:?}", lookup(http));
        assert!(!text.contains("test-key"), "{text}");
    }
}
