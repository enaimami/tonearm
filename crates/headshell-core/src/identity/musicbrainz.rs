//! The MusicBrainz metadata source — it feeds the 2nd and 3rd links of the K6
//! chain.
//!
//! Until this file, `MetadataLookup` had no real implementation:
//! [`OfflineLookup`](super::OfflineLookup) always said "not found" and the
//! chain dropped every record without an ISRC to `LocalKey`. So the two middle
//! links of the ISRC → **MBID** → **fuzzy** order did no measurable work
//! (D-045).
//!
//! ## The limits live here, because MusicBrainz has rules
//!
//! - **`User-Agent` is mandatory.** Requests that do not identify the
//!   application get a `403`. We produce a default value, but it can be
//!   changed with [`MusicBrainzLookup::with_user_agent`]; whoever distributes
//!   the app should put their own contact address there.
//! - **One request per second.** That is the average rate for anonymous
//!   clients; exceed it and a `503` comes back. The limiter is in
//!   [`crate::net::RateLimiter`] and **sleeps between calls**; the reasoning
//!   is written there.

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;

use super::{Candidate, LookupFuture, MetadataLookup};
use crate::error::Result;
use crate::ids::{Isrc, Mbid};
use crate::net::{HttpClient, HttpHeader, HttpRequest, RateLimiter, encode_query, parse_json};

/// The public MusicBrainz server.
pub const DEFAULT_BASE_URL: &str = "https://musicbrainz.org/ws/2";

/// The shortest time between two requests.
///
/// MusicBrainz's anonymous limit is one request per second. 1100 ms leaves
/// room for clock skew and network jitter — a client that grazes the limit
/// eats a `503` and starts retrying, which is slower overall.
const MIN_INTERVAL: Duration = Duration::from_millis(1100);

/// How many candidates to ask for per search.
///
/// [`super::fuzzy`] does the scoring; the list returned here is its input.
/// More grows the network traffic and the parsing, fewer drops the right
/// candidate off the list.
const SEARCH_LIMIT: usize = 25;

/// How many times to retry after a `503` (rate limit).
///
/// Counted: endless retries would make a client that exceeds its quota
/// silent — the same reasoning as plugin restarts being counted (§2.1).
const RATE_LIMIT_RETRIES: u32 = 2;

/// A metadata source that connects to MusicBrainz.
///
/// It goes through [`HttpClient`], not straight to HTTP (D-020): tests supply
/// a fake client, mobile supplies its own stack.
pub struct MusicBrainzLookup {
    http: Arc<dyn HttpClient>,
    base_url: String,
    user_agent: String,
    limiter: RateLimiter,
    search_limit: usize,
}

impl std::fmt::Debug for MusicBrainzLookup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MusicBrainzLookup")
            .field("base_url", &self.base_url)
            .field("user_agent", &self.user_agent)
            .field("search_limit", &self.search_limit)
            .finish_non_exhaustive()
    }
}

impl MusicBrainzLookup {
    /// A source connecting to the public server.
    #[must_use]
    pub fn new(http: Arc<dyn HttpClient>) -> Self {
        Self {
            http,
            base_url: DEFAULT_BASE_URL.to_owned(),
            user_agent: default_user_agent(),
            limiter: RateLimiter::new(MIN_INTERVAL),
            search_limit: SEARCH_LIMIT,
        }
    }

    /// Another server (your own copy of MusicBrainz, or a test server).
    ///
    /// A trailing `/` is dropped, so no double slash appears when building
    /// `{base}/recording`.
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        let url = base_url.into();
        self.base_url = url.trim_end_matches('/').to_owned();
        self
    }

    /// Your own `User-Agent`.
    ///
    /// MusicBrainz wants to see the application and **a reachable address**;
    /// format: `application/version ( contact )`. A distribution that does not
    /// set this goes with the default and shares the risk of being throttled.
    #[must_use]
    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }

    /// The shortest time between requests.
    ///
    /// Only for those connecting to their own copy, and for tests; going below
    /// the default on the public server breaks the quota.
    #[must_use]
    pub fn with_min_interval(mut self, interval: Duration) -> Self {
        self.limiter = RateLimiter::new(interval);
        self
    }

    /// How many candidates to ask for.
    #[must_use]
    pub fn with_search_limit(mut self, limit: usize) -> Self {
        self.search_limit = limit.clamp(1, 100);
        self
    }

    fn headers(&self) -> Vec<HttpHeader> {
        vec![
            HttpHeader::new("User-Agent", self.user_agent.clone()),
            HttpHeader::new("Accept", "application/json"),
        ]
    }

    /// Makes a GET, respecting the rate limit; `404` is **not an error** but
    /// `None`.
    ///
    /// The distinction between "not found" and "could not talk" starts here
    /// (K9): the first means moving on to the next link of the chain, the second
    /// means stopping and reporting.
    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        what: &str,
    ) -> Result<Option<T>> {
        let request = HttpRequest::get(url).with_headers(self.headers());
        let mut attempt = 0;
        loop {
            self.limiter.acquire();
            let response = self.http.send(&request).await?;

            if response.status == 404 {
                return Ok(None);
            }
            // 503 = quota exceeded. The server says "slow down", not "no".
            if response.status == 503 && attempt < RATE_LIMIT_RETRIES {
                attempt += 1;
                tracing::warn!(url, attempt, "MusicBrainz rate limit: waiting and retrying");
                std::thread::sleep(MIN_INTERVAL);
                continue;
            }
            response.error_for_status(url)?;
            return parse_json::<T>(&response, what).map(Some);
        }
    }
}

/// This build's default `User-Agent`.
///
/// The address is a placeholder (as in the `README`); a real distribution
/// should change it with [`MusicBrainzLookup::with_user_agent`].
fn default_user_agent() -> String {
    format!(
        "headshell/{} ( https://github.com/enaimami/headshell )",
        env!("CARGO_PKG_VERSION")
    )
}

impl MetadataLookup for MusicBrainzLookup {
    fn recording_by_isrc<'a>(&'a self, isrc: &'a Isrc) -> LookupFuture<'a, Option<Candidate>> {
        Box::pin(async move {
            // `inc=artist-credits`: the recording title alone is not enough to make a
            // candidate; fuzzy scoring wants the artist too.
            let url = format!(
                "{}/isrc/{}?fmt=json&inc=artist-credits+isrcs",
                self.base_url,
                encode_query(isrc.as_str())
            );
            let Some(payload) = self
                .get_json::<IsrcResponse>(&url, "musicbrainz isrc")
                .await?
            else {
                return Ok(None);
            };

            let mut candidates = collect_candidates(payload.recordings, "isrc");
            if candidates.len() > 1 {
                // An ISRC can be tied to more than one recording (the same track's
                // recordings on different releases). We take the first, but the count
                // goes on record: choosing silently would leave "why this MBID?"
                // unanswered later.
                tracing::debug!(
                    isrc = isrc.as_str(),
                    count = candidates.len(),
                    "the ISRC is tied to more than one recording; took the first"
                );
            }
            Ok((!candidates.is_empty()).then(|| candidates.swap_remove(0)))
        })
    }

    fn search_recordings<'a>(
        &'a self,
        artist: &'a str,
        title: &'a str,
    ) -> LookupFuture<'a, Vec<Candidate>> {
        Box::pin(async move {
            let query = build_query(artist, title);
            if query.is_empty() {
                // An empty query means `400` at MusicBrainz. We say "no candidates"
                // without sending a request; the chain goes down to the fuzzy link
                // with an empty list.
                return Ok(Vec::new());
            }
            let url = format!(
                "{}/recording?query={}&fmt=json&limit={}",
                self.base_url,
                encode_query(&query),
                self.search_limit
            );
            let Some(payload) = self
                .get_json::<SearchResponse>(&url, "musicbrainz recording search")
                .await?
            else {
                return Ok(Vec::new());
            };
            Ok(collect_candidates(payload.recordings, "search"))
        })
    }
}

/// Turns raw recordings into candidates; what cannot be turned **is counted
/// and reported**.
///
/// Dropping them silently with `filter_map` would be exactly what the "no
/// silent `unwrap_or_default()`" rule forbids: a recording arriving with a
/// broken MBID lowers the accuracy rate for no reason and nobody notices.
fn collect_candidates(recordings: Vec<MbRecording>, source: &str) -> Vec<Candidate> {
    let mut out = Vec::with_capacity(recordings.len());
    let mut skipped = 0usize;
    for recording in recordings {
        match Mbid::parse(&recording.id) {
            Some(mbid) => out.push(Candidate {
                mbid,
                artist: recording.artist_name(),
                title: recording.title,
                duration_ms: recording.length,
                isrc: recording.isrcs.iter().find_map(|raw| Isrc::parse(raw)),
                disambiguation: recording.disambiguation.filter(|note| !note.is_empty()),
            }),
            None => skipped += 1,
        }
    }
    if skipped > 0 {
        tracing::warn!(
            source,
            skipped,
            "skipped recordings with an invalid MBID in the MusicBrainz response"
        );
    }
    out
}

/// Builds a Lucene query: `artist:"..." AND recording:"..."`.
///
/// If one of the fields is empty that field is dropped; if both are empty the
/// query comes back empty and the caller sends no request.
fn build_query(artist: &str, title: &str) -> String {
    let mut parts = Vec::new();
    let artist = escape_lucene(artist);
    let title = escape_lucene(title);
    if !artist.is_empty() {
        parts.push(format!("artist:\"{artist}\""));
    }
    if !title.is_empty() {
        parts.push(format!("recording:\"{title}\""));
    }
    parts.join(" AND ")
}

/// Escapes Lucene's special characters.
///
/// Without escaping, names like `AC/DC` or `Where Is My Mind?` break the
/// query and MusicBrainz returns `400` — so resolution collapses exactly
/// where it is most needed, on odd names.
fn escape_lucene(value: &str) -> String {
    const SPECIAL: &[char] = &[
        '+', '-', '&', '|', '!', '(', ')', '{', '}', '[', ']', '^', '"', '~', '*', '?', ':', '\\',
        '/',
    ];
    let mut out = String::with_capacity(value.len());
    for ch in value.trim().chars() {
        if SPECIAL.contains(&ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

// --- Wire format (MusicBrainz ws/2, `fmt=json`) ---------------------------

#[derive(Debug, Deserialize)]
struct IsrcResponse {
    #[serde(default)]
    recordings: Vec<MbRecording>,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    recordings: Vec<MbRecording>,
}

#[derive(Debug, Deserialize)]
struct MbRecording {
    id: String,
    #[serde(default)]
    title: String,
    /// Milliseconds. If MusicBrainz does not know the duration it comes as
    /// `null` — that is information, not zero: [`super::fuzzy`] behaves
    /// differently when it does not know the duration.
    #[serde(default)]
    length: Option<u64>,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<MbArtistCredit>,
    /// The note that sets a recording apart from its namesakes: `"live,
    /// 1994-05-27: Astoria, London, UK"`.
    ///
    /// This field is the **only** marker of live recordings — the title stays
    /// plainly `Creep`. An empty string arrives too (`""`), so it is weeded out
    /// with `filter`: "no note" and "an empty note" are the same thing, but
    /// `Some("")` would push an empty context into scoring.
    #[serde(default)]
    disambiguation: Option<String>,
    #[serde(default)]
    isrcs: Vec<String>,
}

impl MbRecording {
    /// Turns the `artist-credit` list into a single string.
    ///
    /// MusicBrainz gives multiple artists piece by piece and carries the joining
    /// phrase (`joinphrase`) separately: `[{name:"Jay-Z", joinphrase:" & "},
    /// {name:"Kanye West"}]` → `Jay-Z & Kanye West`. Taking only the first would
    /// lose half of every duet.
    fn artist_name(&self) -> String {
        let mut out = String::new();
        for credit in &self.artist_credit {
            out.push_str(&credit.name);
            out.push_str(&credit.joinphrase);
        }
        out.trim().to_owned()
    }
}

#[derive(Debug, Deserialize)]
struct MbArtistCredit {
    #[serde(default)]
    name: String,
    #[serde(default)]
    joinphrase: String,
}

/// This build's default metadata source.
///
/// # Errors
/// If the `http-client` feature is off: the caller must supply its own
/// client and call [`MusicBrainzLookup::new`].
#[cfg(feature = "http-client")]
pub fn default_musicbrainz_lookup() -> Result<Arc<dyn MetadataLookup>> {
    Ok(Arc::new(MusicBrainzLookup::new(
        crate::net::default_http_client()?,
    )))
}

/// This build's default metadata source.
///
/// # Errors
/// In this build `http-client` is off, so it **always** returns an error. We
/// do not silently fall back to the offline source: the user should know why
/// the chain ended at `LocalKey` (K9).
#[cfg(not(feature = "http-client"))]
pub fn default_musicbrainz_lookup() -> Result<Arc<dyn MetadataLookup>> {
    Err(crate::error::Error::new(
        crate::diag::Stage::IdentityResolve,
        crate::error::ErrorKind::Unsupported {
            provider: "musicbrainz".to_owned(),
            what: "metadata lookup (a build with the `http-client` feature off)".to_owned(),
            capabilities: "NONE".to_owned(),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::fake::FakeHttp;

    /// In tests the rate limit is 0: the limiter itself is not what is tested.
    fn lookup(http: FakeHttp) -> MusicBrainzLookup {
        MusicBrainzLookup::new(Arc::new(http))
            .with_base_url("http://mb.test/ws/2")
            .with_min_interval(Duration::ZERO)
    }

    const CREEP_SEARCH: &str = r#"{
        "count": 1,
        "recordings": [{
            "id": "b1a9c0e9-d987-4042-ae91-78d6a3267d69",
            "title": "Creep",
            "length": 238000,
            "artist-credit": [{ "name": "Radiohead", "joinphrase": "" }],
            "isrcs": ["GBAYE9200001"]
        }]
    }"#;

    #[tokio::test]
    async fn a_search_becomes_a_scored_candidate() {
        let http = FakeHttp::new().route("/recording?query=", CREEP_SEARCH);
        let mb = lookup(http);
        let found = mb.search_recordings("Radiohead", "Creep").await.unwrap();

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].artist, "Radiohead");
        assert_eq!(found[0].title, "Creep");
        assert_eq!(found[0].duration_ms, Some(238_000));
        assert_eq!(
            found[0].isrc.as_ref().map(Isrc::as_str),
            Some("GBAYE9200001")
        );
    }

    #[tokio::test]
    async fn an_isrc_lookup_returns_the_first_recording() {
        let body = r#"{
            "isrc": "GBAYE9200001",
            "recordings": [
                { "id": "b1a9c0e9-d987-4042-ae91-78d6a3267d69", "title": "Creep",
                  "length": 238000, "artist-credit": [{ "name": "Radiohead" }] },
                { "id": "63f1a2c3-1111-4042-ae91-78d6a3267d01", "title": "Creep (live)",
                  "length": 251000, "artist-credit": [{ "name": "Radiohead" }] }
            ]
        }"#;
        let http = FakeHttp::new().route("/isrc/", body);
        let mb = lookup(http);
        let isrc = Isrc::parse("GBAYE9200001").unwrap();
        let found = mb.recording_by_isrc(&isrc).await.unwrap().unwrap();
        assert_eq!(found.mbid.as_str(), "b1a9c0e9-d987-4042-ae91-78d6a3267d69");
    }

    /// K9: "not found" is not an error — the chain must move on to the next link.
    #[tokio::test]
    async fn an_unknown_isrc_is_absence_not_failure() {
        let http = FakeHttp::new().route_status("/isrc/", 404, r#"{"error":"Not Found"}"#);
        let mb = lookup(http);
        let isrc = Isrc::parse("GBAYE9200001").unwrap();
        assert_eq!(mb.recording_by_isrc(&isrc).await.unwrap(), None);
    }

    /// ...but "could not talk" is an error, and it says its stage.
    #[tokio::test]
    async fn a_server_error_is_reported_not_swallowed() {
        let http = FakeHttp::new().route_status("/recording?query=", 500, "broken");
        let mb = lookup(http);
        let err = mb
            .search_recordings("Radiohead", "Creep")
            .await
            .unwrap_err();
        let text = err.chain_text();
        assert!(text.starts_with("STEP: NETWORK_REQUEST"), "{text}");
        assert!(text.contains("500"), "{text}");
    }

    #[tokio::test]
    async fn the_request_carries_a_user_agent_musicbrainz_accepts() {
        let http = Arc::new(FakeHttp::new().route("/recording?query=", CREEP_SEARCH));
        let mb = MusicBrainzLookup::new(http.clone())
            .with_base_url("http://mb.test/ws/2")
            .with_min_interval(Duration::ZERO);
        mb.search_recordings("Radiohead", "Creep").await.unwrap();

        let sent = http.requests();
        let agent = sent[0]
            .headers
            .iter()
            .find(|h| h.name == "User-Agent")
            .expect("a User-Agent must be sent: MusicBrainz returns 403 otherwise");
        assert!(agent.value.starts_with("headshell/"), "{}", agent.value);
        assert!(
            agent.value.contains('('),
            "contact address: {}",
            agent.value
        );
    }

    /// Odd names must not break the query — unescaped, MusicBrainz returns
    /// `400`.
    #[test]
    fn lucene_special_characters_are_escaped() {
        let query = build_query("AC/DC", "Where Is My Mind?");
        assert!(query.contains(r#"artist:"AC\/DC""#), "{query}");
        assert!(
            query.contains(r#"recording:"Where Is My Mind\?""#),
            "{query}"
        );
    }

    #[test]
    fn an_empty_query_is_not_sent_at_all() {
        assert_eq!(build_query("", ""), "");
        assert_eq!(build_query("  ", "  "), "");
        assert_eq!(build_query("", "Creep"), r#"recording:"Creep""#);
    }

    #[test]
    fn multi_artist_credits_are_joined_with_their_phrases() {
        let recording: MbRecording = serde_json::from_str(
            r#"{ "id": "b1a9c0e9-d987-4042-ae91-78d6a3267d69", "title": "Otis",
                 "artist-credit": [
                   { "name": "JAY-Z", "joinphrase": " & " },
                   { "name": "Kanye West" }
                 ] }"#,
        )
        .unwrap();
        assert_eq!(recording.artist_name(), "JAY-Z & Kanye West");
    }

    /// A broken MBID is not dropped silently: it is counted and reported.
    #[test]
    fn invalid_mbids_are_counted_not_silently_dropped() {
        let recordings: Vec<MbRecording> = serde_json::from_str(
            r#"[
                { "id": "not-a-uuid", "title": "Broken" },
                { "id": "b1a9c0e9-d987-4042-ae91-78d6a3267d69", "title": "Creep" }
            ]"#,
        )
        .unwrap();
        let candidates = collect_candidates(recordings, "test");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].title, "Creep");
    }

    /// If the duration is unknown it must stay `None` — not zero (the scoring
    /// distinction).
    #[test]
    fn a_missing_length_stays_unknown_rather_than_zero() {
        let recordings: Vec<MbRecording> = serde_json::from_str(
            r#"[{ "id": "b1a9c0e9-d987-4042-ae91-78d6a3267d69", "title": "Creep" }]"#,
        )
        .unwrap();
        let candidates = collect_candidates(recordings, "test");
        assert_eq!(candidates[0].duration_ms, None);
    }

    /// A rate-limit `503` means "slow down", not "no": it is tried once more.
    #[tokio::test]
    async fn a_rate_limited_response_is_retried() {
        // The fake client returns the first matching route; we cannot define
        // two different routes and have them returned in turn, so what is tested
        // here is that retrying is *counted*: if the 503 persists, an error must
        // come back.
        let http = Arc::new(FakeHttp::new().route_status("/recording?query=", 503, "slow down"));
        let mb = MusicBrainzLookup::new(http.clone())
            .with_base_url("http://mb.test/ws/2")
            .with_min_interval(Duration::ZERO);
        let err = mb
            .search_recordings("Radiohead", "Creep")
            .await
            .unwrap_err();

        assert!(err.chain_text().contains("503"), "{}", err.chain_text());
        assert_eq!(
            http.requests().len(),
            (RATE_LIMIT_RETRIES + 1) as usize,
            "first attempt + {RATE_LIMIT_RETRIES} retries"
        );
    }
}
