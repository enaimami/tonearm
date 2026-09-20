//! Torznab istemcisi (D-047 S3b).
//!
//! Torznab, Newznab'dan türeyen ve Prowlarr/Jackett'ın konuştuğu standart
//! arama API'si: sorgu bir URL, cevap RSS. **Depoda hiçbir siteye özel
//! kazıyıcı yok** — hangi indekslerin sorgulanacağını kullanıcı kendi
//! Prowlarr/Jackett'ında seçer, biz tek bir ayrıştırıcı taşırız.
//!
//! Cevabın üç ayrı "boş"u var ve üçü ayrı tanıdır (K9):
//! - `<error code=...>` — indeks bizi **reddetti** (anahtar yanlış vb.),
//! - sıfır `<item>` — indeks baktı, **bulamadı**,
//! - ağ hatası — indekse **ulaşamadık**.

use crate::rpc::{Result, err};

/// Torznab'ın ses kategorisi. Alt kategoriler (3010 MP3, 3040 FLAC…) bunun
/// içinde; üst kategoriyi sormak indeksin kendi eşlemesine güvenmek demektir.
pub const CATEGORY_AUDIO: &str = "3000";

/// Torznab'ın `torznab:attr` ad alanı. `roxmltree` yerel ada bakmamıza izin
/// veriyor ama önekin ne olduğu indeksten indekse değişiyor; ad alanına
/// bakmak öneki tahmin etmekten sağlam.
const TORZNAB_NS: &str = "http://torznab.com/schemas/2015/feed";

/// Aramadan dönen tek bir yayım (release). **Bir parça değil** — genelde bir
/// albüm ya da derleme. Parçaya inmek ikinci adım (bkz. `main::search`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Yayım adı, indeksin yazdığı gibi.
    pub title: String,
    /// 40 haneli hex infohash. Kimliğimiz bu.
    pub infohash: String,
    /// Tercih edilen kaynak: tracker listesini de taşıdığı için magnet.
    pub magnet: Option<String>,
    /// Magnet yoksa `.torrent` dosyasının adresi.
    pub torrent_url: Option<String>,
    pub size_bytes: Option<u64>,
    pub seeders: Option<u32>,
    /// Hangi indeksten geldi (Prowlarr bunu yazıyor).
    pub indexer: Option<String>,
}

impl Release {
    /// `librqbit`'e verilecek adres. Magnet varsa o, yoksa `.torrent` adresi.
    pub fn source_url(&self) -> Option<&str> {
        self.magnet
            .as_deref()
            .or(self.torrent_url.as_deref())
            .filter(|url| !url.is_empty())
    }
}

/// Bir aramanın sonucu ve **düşürülenlerin sayısı**. Sessizce kısaltılmış bir
/// liste, kullanıcıya indeksin az sonuç verdiğini düşündürür (K9).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchOutcome {
    pub releases: Vec<Release>,
    /// Infohash'i de magnet'i de olmayan, yani çalınamayacak kayıtlar.
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
            return err("Torznab adresi boş");
        }
        if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
            return err(format!(
                "Torznab adresi `http://` ya da `https://` ile başlamalı: {base_url}"
            ));
        }
        let http = reqwest::Client::builder()
            .user_agent(concat!("tonearm-torrent/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(12))
            .build()
            .map_err(|error| {
                crate::rpc::PluginError::new(format!("HTTP istemcisi kurulamadı: {error}"))
            })?;
        Ok(Self {
            base_url,
            api_key: api_key.into().trim().to_owned(),
            http,
        })
    }

    /// Adresi kurar. Anahtar sorgu dizesinde gider (Torznab'ın tanımı böyle),
    /// bu yüzden **hiçbir log satırında tam adres basılmaz** (D-042).
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

    /// Log'a ve hata metnine girecek olan adres: anahtar **yok**.
    fn redacted_base(&self) -> &str {
        &self.base_url
    }

    /// Indeksin ayakta olup olmadığını en ucuz şekilde sorar (`t=caps`).
    pub async fn caps(&self) -> Result<String> {
        let body = self.get(&self.url(&[("t", "caps")])).await?;
        let document = parse(&body)?;
        check_error(&document)?;
        let categories = document
            .descendants()
            .filter(|node| node.has_tag_name("category"))
            .count();
        Ok(format!(
            "Torznab {} — {categories} kategori bildiriyor",
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
                "Torznab'a ulaşılamadı ({}): {error}",
                self.redacted_base()
            ))
        })?;
        let status = response.status();
        let body = response.text().await.map_err(|error| {
            crate::rpc::PluginError::new(format!("Torznab cevabı okunamadı: {error}"))
        })?;
        if !status.is_success() {
            // Gövde önce ayrıştırılıyor: Torznab hataları 200 ile de gelebiliyor,
            // ama HTTP hatasıyla gelenin gövdesinde de açıklama olabilir.
            if let Ok(document) = parse(&body) {
                check_error(&document)?;
            }
            return err(format!(
                "Torznab HTTP {} döndürdü ({})",
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
            "Torznab XML olmayan bir cevap verdi: {error} (ilk 120 karakter: {})",
            body.chars().take(120).collect::<String>()
        ))
    })
}

/// `<error code=.. description=..>` varsa hataya çevirir.
///
/// Bu, "sonuç yok"tan **ayrı** bir tanıdır: indeks baktı ve bulamadı değil,
/// indeks bize bakmayı reddetti.
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
        .unwrap_or("açıklama yok");
    err(format!(
        "Torznab isteği reddetti (kod {code}): {description}"
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

/// `<torznab:attr name="..." value="..."/>` okur.
///
/// Ad alanı doğruysa onunla, değilse yerel adla eşleşiyor: bazı indeksler
/// `newznab` ad alanını kullanıyor ve alan adları aynı.
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

/// `magnet:?xt=urn:btih:<hash>` içinden infohash'i çıkarır.
///
/// Base32 (32 hane) biçimini **kabul etmiyoruz**: dönüştürmek mümkün ama
/// bugüne kadar ölçtüğümüz bir örneği yok ve yanlış dönüştürmek kimliği
/// sessizce bozar. Karşılaşırsak sayılıp düşer.
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
                 <torznab:attr name="indexer" value="ornek" />
               </item>"#
        ));
        let outcome = parse_search_response(&body).unwrap();
        assert_eq!(outcome.dropped_unidentifiable, 0);
        let release = &outcome.releases[0];
        assert_eq!(release.infohash, HASH);
        assert_eq!(release.seeders, Some(42));
        assert_eq!(release.size_bytes, Some(512_000_000));
        assert_eq!(release.indexer.as_deref(), Some("ornek"));
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
                 <title>Bir Yayım</title>
                 <link>magnet:?xt=urn:btih:{}&amp;tr=udp://x</link>
               </item>"#,
            HASH.to_ascii_uppercase()
        ));
        let outcome = parse_search_response(&body).unwrap();
        assert_eq!(
            outcome.releases[0].infohash, HASH,
            "hex küçük harfe indirilmeli"
        );
    }

    #[test]
    fn an_item_we_could_never_play_is_counted_not_silently_dropped() {
        let body = feed(
            r#"<item><title>Kimliksiz</title><link>https://ornek/sayfa</link></item>
               <item><title>Boş</title></item>"#,
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
        assert!(text.contains("reddetti"), "{text}");
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
        let error = parse_search_response("<!DOCTYPE html><html><body>Giriş yapın").unwrap_err();
        let text = error.to_string();
        assert!(text.contains("XML olmayan"), "{text}");
        assert!(
            text.contains("DOCTYPE"),
            "ilk karakterler gösterilmeli: {text}"
        );
    }

    #[test]
    fn the_api_key_never_appears_in_an_error_message() {
        let client = Torznab::new("https://indeks.ornek/api", "GIZLI-ANAHTAR").unwrap();
        assert!(!client.redacted_base().contains("GIZLI"));
        let url = client.url(&[("t", "search")]);
        assert!(
            url.contains("apikey=GIZLI-ANAHTAR"),
            "anahtar adreste olmalı: {url}"
        );
        assert!(
            !client.redacted_base().contains("apikey"),
            "ama log'a giden adreste olmamalı"
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
        let error = Torznab::new("indeks.ornek/api", "k").unwrap_err();
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
            "anahtar boşken parametre eklenmemeli: {url}"
        );
    }
}
