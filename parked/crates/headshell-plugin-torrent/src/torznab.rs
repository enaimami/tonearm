//! The Torznab client (D-047 S3b).
//!
//! Torznab is the standard search API derived from Newznab that Prowlarr and
//! Jackett speak: the query is a URL, the answer is RSS. **There is no
//! site-specific scraper in the repository** — the user picks which indexers
//! to query in their own Prowlarr/Jackett; we carry a single parser.
//!
//! The answer has three different "empties", and all three are separate
//! diagnoses (K9):
//! - `<error code=...>` — the indexer **refused** us (a wrong key etc.),
//! - zero `<item>`s — the indexer looked and **found nothing**,
//! - a network error — we **could not reach** the indexer.

use crate::rpc::{Result, err};

/// Torznab's audio category. The subcategories (3010 MP3, 3040 FLAC…) are
/// inside it; asking for the parent category means trusting the indexer's own
/// mapping.
pub const CATEGORY_AUDIO: &str = "3000";

/// Torznab's `torznab:attr` namespace. `roxmltree` lets us look at the local
/// name, but the prefix varies from indexer to indexer; looking at the
/// namespace is sturdier than guessing the prefix.
const TORZNAB_NS: &str = "http://torznab.com/schemas/2015/feed";

/// A single release returned by a search. **Not a track** — usually an album
/// or a compilation. Going down to a track is the second step (see
/// `main::search`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// The release name, as the indexer wrote it.
    pub title: String,
    /// The 40-digit hex infohash. This is our id.
    pub infohash: String,
    /// The preferred source: a magnet, since it also carries the tracker list.
    pub magnet: Option<String>,
    /// The address of the `.torrent` file if there is no magnet.
    pub torrent_url: Option<String>,
    pub size_bytes: Option<u64>,
    pub seeders: Option<u32>,
    /// Which indexer it came from (Prowlarr writes this).
    pub indexer: Option<String>,
}

impl Release {
    /// The address to give `librqbit`. The magnet if there is one, otherwise the
    /// `.torrent` address.
    pub fn source_url(&self) -> Option<&str> {
        self.magnet
            .as_deref()
            .or(self.torrent_url.as_deref())
            .filter(|url| !url.is_empty())
    }
}

/// A search's result and **the number of dropped entries**. A silently
/// shortened list makes the user think the indexer gave few results (K9).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchOutcome {
    pub releases: Vec<Release>,
    /// Records with neither an infohash nor a magnet, that is, ones that could not
    /// be played.
    pub dropped_unidentifiable: usize,
}

#[derive(Debug, Clone)]
pub struct Torznab {
    base_url: String,
    api_key: String,
    http: reqwest::Client,
}

impl Torznab {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Result<Self> {
        let base_url = base_url.into().trim().to_owned();
        if base_url.is_empty() {
            return err("the Torznab address is empty");
        }
        if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
            return err(format!(
                "the Torznab address must start with `http://` or `https://`: {base_url}"
            ));
        }
        let http = reqwest::Client::builder()
            .user_agent(concat!("headshell-torrent/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(12))
            .build()
            .map_err(|error| {
                crate::rpc::PluginError::new(format!("could not set up the HTTP client: {error}"))
            })?;
        Ok(Self {
            base_url,
            api_key: api_key.into().trim().to_owned(),
            http,
        })
    }

    /// Builds the address. The key goes in the query string (that is how Torznab
    /// defines it), which is why **the full address is printed in no log line**
    /// (D-042).
    fn url(&self, params: &[(&str, &str)]) -> String {
        let mut url = self.base_url.clone();
        url.push(if url.contains('?') { '&' } else { '?' });
        let mut pairs: Vec<String> = Vec::new();
        if !self.api_key.is_empty() {
            pairs.push(format!("apikey={}", encode(&self.api_key)));
        }
        for (key, value) in params {
            pairs.push(format!("{}={}", encode(key), encode(value)));
        }
        url.push_str(&pairs.join("&"));
        url
    }

    /// The address that goes into the log and the error text: **without** the
    /// key.
    fn redacted_base(&self) -> &str {
        &self.base_url
    }

    /// Asks the indexer whether it is up, in the cheapest way (`t=caps`).
    pub async fn caps(&self) -> Result<String> {
        let body = self.get(&self.url(&[("t", "caps")])).await?;
        let document = parse(&body)?;
        check_error(&document)?;
        let categories = document
            .descendants()
            .filter(|node| node.has_tag_name("category"))
            .count();
        Ok(format!(
            "Torznab {} — reports {categories} categories",
            self.redacted_base()
        ))
    }

    pub async fn search(&self, query: &str, limit: usize) -> Result<SearchOutcome> {
        let limit = limit.clamp(1, 200).to_string();
        let url = self.url(&[
            ("t", "search"),
            ("cat", CATEGORY_AUDIO),
            ("limit", &limit),
            ("q", query),
        ]);
        let body = self.get(&url).await?;
        parse_search_response(&body)
    }

    async fn get(&self, url: &str) -> Result<String> {
        let response = self.http.get(url).send().await.map_err(|error| {
            crate::rpc::PluginError::new(format!(
                "could not reach Torznab ({}): {error}",
                self.redacted_base()
            ))
        })?;
        let status = response.status();
        let body = response.text().await.map_err(|error| {
            crate::rpc::PluginError::new(format!("could not read the Torznab answer: {error}"))
        })?;
        if !status.is_success() {
            // The body is parsed first: Torznab errors can arrive with a 200 too,
            // and one arriving with an HTTP error may still carry an explanation in
            // its body.
            if let Ok(document) = parse(&body) {
                check_error(&document)?;
            }
            return err(format!(
                "Torznab returned HTTP {} ({})",
                status.as_u16(),
                self.redacted_base()
            ));
        }
        Ok(body)
    }
}

fn encode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

fn parse(body: &str) -> Result<roxmltree::Document<'_>> {
    roxmltree::Document::parse(body).map_err(|error| {
        crate::rpc::PluginError::new(format!(
            "Torznab gave an answer that is not XML: {error} (first 120 characters: {})",
            body.chars().take(120).collect::<String>()
        ))
    })
}

/// Turns an `<error code=.. description=..>` into an error if there is one.
///
/// This is a diagnosis **separate** from "no results": it is not that the
/// indexer looked and found nothing, it is that the indexer refused to look for
/// us.
fn check_error(document: &roxmltree::Document<'_>) -> Result<()> {
    let Some(node) = document
        .descendants()
        .find(|node| node.has_tag_name("error"))
    else {
        return Ok(());
    };
    let code = node.attribute("code").unwrap_or("?");
    let description = node
        .attribute("description")
        .or_else(|| node.attribute("message"))
        .unwrap_or("no description");
    err(format!(
        "Torznab refused the request (code {code}): {description}"
    ))
}

pub fn parse_search_response(body: &str) -> Result<SearchOutcome> {
    let document = parse(body)?;
    check_error(&document)?;

    let mut outcome = SearchOutcome::default();
    for item in document
        .descendants()
        .filter(|node| node.is_element() && node.has_tag_name("item"))
    {
        match release_from_item(&item) {
            Some(release) => outcome.releases.push(release),
            None => outcome.dropped_unidentifiable += 1,
        }
    }
    Ok(outcome)
}

fn release_from_item(item: &roxmltree::Node<'_, '_>) -> Option<Release> {
    let title = child_text(item, "title")?.trim().to_owned();
    if title.is_empty() {
        return None;
    }

    let magnet = attr(item, "magneturl")
        .or_else(|| enclosure_url(item).filter(|url| url.starts_with("magnet:")))
        .or_else(|| child_text(item, "link").filter(|url| url.starts_with("magnet:")));
    let torrent_url = enclosure_url(item)
        .filter(|url| !url.starts_with("magnet:"))
        .or_else(|| child_text(item, "link").filter(|url| !url.starts_with("magnet:")));

    let infohash = attr(item, "infohash")
        .map(|raw| raw.trim().to_ascii_lowercase())
        .filter(|hash| is_infohash(hash))
        .or_else(|| magnet.as_deref().and_then(infohash_from_magnet))?;

    Some(Release {
        title,
        infohash,
        magnet,
        torrent_url,
        size_bytes: attr(item, "size")
            .or_else(|| child_text(item, "size"))
            .and_then(|raw| raw.trim().parse().ok()),
        seeders: attr(item, "seeders").and_then(|raw| raw.trim().parse().ok()),
        indexer: attr(item, "indexer").or_else(|| child_text(item, "jackettindexer")),
    })
}

fn child_text(node: &roxmltree::Node<'_, '_>, name: &str) -> Option<String> {
    node.children()
        .find(|child| child.is_element() && child.has_tag_name(name))
        .and_then(|child| child.text())
        .map(str::to_owned)
}

fn enclosure_url(node: &roxmltree::Node<'_, '_>) -> Option<String> {
    node.children()
        .find(|child| child.is_element() && child.has_tag_name("enclosure"))
        .and_then(|child| child.attribute("url"))
        .map(str::to_owned)
}

/// Reads `<torznab:attr name="..." value="..."/>`.
///
/// It matches by the namespace if that is right, otherwise by the local name:
/// some indexers use the `newznab` namespace and the field names are the same.
fn attr(node: &roxmltree::Node<'_, '_>, name: &str) -> Option<String> {
    node.children()
        .filter(|child| child.is_element() && child.tag_name().name() == "attr")
        .filter(|child| {
            child
                .tag_name()
                .namespace()
                .is_none_or(|ns| ns == TORZNAB_NS || ns.contains("newznab"))
        })
        .find(|child| {
            child
                .attribute("name")
                .is_some_and(|found| found.eq_ignore_ascii_case(name))
        })
        .and_then(|child| child.attribute("value"))
        .map(str::to_owned)
        .filter(|value| !value.trim().is_empty())
}

pub fn is_infohash(candidate: &str) -> bool {
    candidate.len() == 40 && candidate.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Extracts the infohash from a `magnet:?xt=urn:btih:<hash>`.
///
/// We **do not accept** the base32 (32-digit) form: converting it is possible,
/// but we have not measured a single example of it so far, and a wrong
/// conversion silently corrupts the id. If we meet one, it is counted and
/// dropped.
pub fn infohash_from_magnet(magnet: &str) -> Option<String> {
    let marker = "urn:btih:";
    let start = magnet.to_ascii_lowercase().find(marker)? + marker.len();
    let hash: String = magnet[start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    is_infohash(&hash).then_some(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

    fn feed(items: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:torznab="http://torznab.com/schemas/2015/feed">
  <channel>{items}</channel>
</rss>"#
        )
    }

    #[test]
    fn a_normal_item_yields_a_release_with_its_infohash_and_magnet() {
        let body = feed(&format!(
            r#"<item>
                 <title>Radiohead - OK Computer (1997) [FLAC]</title>
                 <enclosure url="magnet:?xt=urn:btih:{HASH}&amp;dn=x" />
                 <torznab:attr name="infohash" value="{HASH}" />
                 <torznab:attr name="seeders" value="42" />
                 <torznab:attr name="size" value="512000000" />
                 <torznab:attr name="indexer" value="example" />
               </item>"#
        ));
        let outcome = parse_search_response(&body).unwrap();
        assert_eq!(outcome.dropped_unidentifiable, 0);
        let release = &outcome.releases[0];
        assert_eq!(release.infohash, HASH);
        assert_eq!(release.seeders, Some(42));
        assert_eq!(release.size_bytes, Some(512_000_000));
        assert_eq!(release.indexer.as_deref(), Some("example"));
        assert!(
            release
                .source_url()
                .is_some_and(|url| url.starts_with("magnet:"))
        );
    }

    #[test]
    fn an_item_without_an_infohash_attribute_still_works_if_the_magnet_has_one() {
        let body = feed(&format!(
            r#"<item>
                 <title>A Release</title>
                 <link>magnet:?xt=urn:btih:{}&amp;tr=udp://x</link>
               </item>"#,
            HASH.to_ascii_uppercase()
        ));
        let outcome = parse_search_response(&body).unwrap();
        assert_eq!(
            outcome.releases[0].infohash, HASH,
            "the hex must be lower-cased"
        );
    }

    #[test]
    fn an_item_we_could_never_play_is_counted_not_silently_dropped() {
        let body = feed(
            r#"<item><title>No id</title><link>https://example/page</link></item>
               <item><title>Empty</title></item>"#,
        );
        let outcome = parse_search_response(&body).unwrap();
        assert!(outcome.releases.is_empty());
        assert_eq!(outcome.dropped_unidentifiable, 2);
    }

    #[test]
    fn a_rejection_is_an_error_not_an_empty_result_set() {
        let body =
            r#"<?xml version="1.0"?><error code="100" description="Incorrect user credentials" />"#;
        let error = parse_search_response(body).unwrap_err();
        let text = error.to_string();
        assert!(text.contains("refused"), "{text}");
        assert!(text.contains("100"), "{text}");
        assert!(text.contains("Incorrect user credentials"), "{text}");
    }

    #[test]
    fn a_zero_item_feed_is_a_success_with_no_releases() {
        let outcome = parse_search_response(&feed("")).unwrap();
        assert!(outcome.releases.is_empty());
        assert_eq!(outcome.dropped_unidentifiable, 0);
    }

    #[test]
    fn html_instead_of_xml_says_what_arrived_instead_of_a_bare_parse_error() {
        let error = parse_search_response("<!DOCTYPE html><html><body>Please sign in").unwrap_err();
        let text = error.to_string();
        assert!(text.contains("not XML"), "{text}");
        assert!(
            text.contains("DOCTYPE"),
            "the first characters must be shown: {text}"
        );
    }

    #[test]
    fn the_api_key_never_appears_in_an_error_message() {
        let client = Torznab::new("https://indexer.example/api", "SECRET-KEY").unwrap();
        assert!(!client.redacted_base().contains("SECRET"));
        let url = client.url(&[("t", "search")]);
        assert!(
            url.contains("apikey=SECRET-KEY"),
            "the key must be in the address: {url}"
        );
        assert!(
            !client.redacted_base().contains("apikey"),
            "but not in the address that goes to the log"
        );
    }

    #[test]
    fn a_base_url_that_already_has_a_query_gets_an_ampersand_not_a_second_question_mark() {
        let client = Torznab::new("https://x/api?t=indexers", "k").unwrap();
        let url = client.url(&[("t", "search")]);
        assert_eq!(url.matches('?').count(), 1, "{url}");
        assert!(url.contains("&apikey=k"), "{url}");
    }

    #[test]
    fn a_url_without_a_scheme_is_refused_up_front() {
        let error = Torznab::new("indexer.example/api", "k").unwrap_err();
        assert!(error.to_string().contains("http"), "{error}");
    }

    #[test]
    fn a_base32_magnet_is_dropped_rather_than_converted_wrongly() {
        assert!(
            infohash_from_magnet("magnet:?xt=urn:btih:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA").is_none()
        );
    }

    #[test]
    fn a_space_in_the_query_is_encoded() {
        let client = Torznab::new("https://x/api", "").unwrap();
        let url = client.url(&[("q", "pink floyd")]);
        assert!(url.contains("q=pink+floyd"), "{url}");
        assert!(
            !url.contains("apikey="),
            "no parameter must be added while the key is empty: {url}"
        );
    }
}
