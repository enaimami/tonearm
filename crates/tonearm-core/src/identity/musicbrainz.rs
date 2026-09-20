//! MusicBrainz üstveri kaynağı — K6 zincirinin 2. ve 3. halkasını besler.
//!
//! Bu dosyaya kadar `MetadataLookup`'ın tek gerçek uygulaması yoktu:
//! [`OfflineLookup`](super::OfflineLookup) her zaman "bulamadım" diyordu ve
//! zincir ISRC'si olmayan her kaydı `LocalKey`'e düşürüyordu. Yani ISRC →
//! **MBID** → **bulanık** sırasının ortadaki iki halkası ölçülebilir bir iş
//! yapmıyordu (D-045).
//!
//! ## Sınırlar burada, çünkü MusicBrainz'in kuralları var
//!
//! - **`User-Agent` zorunlu.** Uygulamayı tanıtmayan istekler `403` alır.
//!   Varsayılan bir değer üretiyoruz ama [`MusicBrainzLookup::with_user_agent`]
//!   ile değiştirilebilir; dağıtan kişi kendi iletişim adresini koymalı.
//! - **Saniyede bir istek.** Anonim istemcilerin ortalama hızı budur; aşınca
//!   `503` gelir. Kısıtlayıcı [`crate::net::RateLimiter`] içinde ve **çağrılar
//!   arasında uyur**; gerekçesi orada yazılı.

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;

use super::{Candidate, LookupFuture, MetadataLookup};
use crate::error::Result;
use crate::ids::{Isrc, Mbid};
use crate::net::{HttpClient, HttpHeader, HttpRequest, RateLimiter, encode_query, parse_json};

/// Genel MusicBrainz sunucusu.
pub const DEFAULT_BASE_URL: &str = "https://musicbrainz.org/ws/2";

/// İki istek arasındaki en kısa süre.
///
/// MusicBrainz'in anonim sınırı saniyede bir istek. 1100 ms, saat farkı ve
/// ağ dalgalanması için pay bırakıyor — sınırı yalayan bir istemci `503`
/// yiyip yeniden denemeye başlar, bu da toplamda daha yavaştır.
const MIN_INTERVAL: Duration = Duration::from_millis(1100);

/// Arama başına istenecek aday sayısı.
///
/// Skorlamayı [`super::fuzzy`] yapıyor; buradan dönen liste onun girdisi.
/// Fazlası ağı ve ayrıştırmayı büyütür, azı doğru adayı listeden düşürür.
const SEARCH_LIMIT: usize = 25;

/// `503` (hız sınırı) sonrası kaç kez yeniden denenir.
///
/// Sayılı: sonsuz yeniden deneme, kotayı aşan bir istemciyi sessiz kılardı —
/// eklenti yeniden başlatmalarının sayılı olmasıyla aynı gerekçe (§2.1).
const RATE_LIMIT_RETRIES: u32 = 2;

/// MusicBrainz'e bağlanan üstveri kaynağı.
///
/// HTTP'ye doğrudan değil [`HttpClient`] üstünden gidiyor (D-020): testler
/// sahte istemci verir, mobil kendi yığınını verir.
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
    /// Genel sunucuya bağlanan kaynak.
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

    /// Başka bir sunucu (kendi MusicBrainz kopyanız, ya da test sunucusu).
    ///
    /// Sondaki `/` atılır: `{base}/recording` kurarken çift eğik çizgi
    /// oluşmasın.
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        let url = base_url.into();
        self.base_url = url.trim_end_matches('/').to_owned();
        self
    }

    /// Kendi `User-Agent`'ınız.
    ///
    /// MusicBrainz uygulamayı ve **ulaşılabilir bir adresi** görmek ister;
    /// biçim: `uygulama/sürüm ( iletişim )`. Bunu ayarlamayan bir dağıtım
    /// varsayılanla gider ve sınırlama riskini paylaşır.
    #[must_use]
    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }

    /// İstekler arasındaki en kısa süre.
    ///
    /// Yalnızca kendi kopyasına bağlananlar ve testler için; genel sunucuda
    /// varsayılanın altına inmek kotayı ihlal eder.
    #[must_use]
    pub fn with_min_interval(mut self, interval: Duration) -> Self {
        self.limiter = RateLimiter::new(interval);
        self
    }

    /// Kaç aday istensin.
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

    /// Hız sınırına uyarak GET yapar; `404` **hata değil** `None`.
    ///
    /// "Bulamadım" ile "konuşamadım" ayrımı burada başlıyor (K9): ilki
    /// zincirin bir sonraki halkasına geçmek demek, ikincisi durup raporlamak.
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
            // 503 = kota aşıldı. Sunucu "yavaşla" diyor, "hayır" demiyor.
            if response.status == 503 && attempt < RATE_LIMIT_RETRIES {
                attempt += 1;
                tracing::warn!(
                    url,
                    deneme = attempt,
                    "MusicBrainz hız sınırı: bekleyip yeniden denenecek"
                );
                std::thread::sleep(MIN_INTERVAL);
                continue;
            }
            response.error_for_status(url)?;
            return parse_json::<T>(&response, what).map(Some);
        }
    }
}

/// Bu derlemenin varsayılan `User-Agent`'ı.
///
/// Adres yer tutucu (`README`'deki gibi); gerçek bir dağıtımda
/// [`MusicBrainzLookup::with_user_agent`] ile değiştirilmeli.
fn default_user_agent() -> String {
    format!(
        "tonearm/{} ( https://github.com/enaimami/tonearm )",
        env!("CARGO_PKG_VERSION")
    )
}

impl MetadataLookup for MusicBrainzLookup {
    fn recording_by_isrc<'a>(&'a self, isrc: &'a Isrc) -> LookupFuture<'a, Option<Candidate>> {
        Box::pin(async move {
            // `inc=artist-credits`: kayıt adı tek başına aday yapmaya yetmez,
            // bulanık skorlama sanatçıyı da ister.
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
                // Bir ISRC birden çok kayda bağlanabilir (aynı parçanın ayrı
                // yayınlardaki kayıtları). İlkini alıyoruz ama sayı kayda
                // geçiyor: sessizce seçim yapmak, sonradan "neden bu MBID"
                // sorusunu cevapsız bırakır.
                tracing::debug!(
                    isrc = isrc.as_str(),
                    adet = candidates.len(),
                    "ISRC birden çok kayda bağlı; ilki alındı"
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
                // Boş sorgu MusicBrainz'de `400` demek. İstek göndermeden
                // "aday yok" diyoruz; zincir bulanık halkaya boş listeyle iner.
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

/// Ham kayıtları adaylara çevirir; çevrilemeyeni **sayar ve raporlar**.
///
/// `filter_map` ile sessizce düşürmek "sessiz `unwrap_or_default()` yasak"
/// kuralının ta kendisi olurdu: MBID'si bozuk gelen bir kayıt, doğruluk
/// oranını sebepsiz düşürür ve kimse fark etmez.
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
            kaynak = source,
            atlanan = skipped,
            "MusicBrainz yanıtında geçersiz MBID taşıyan kayıtlar atlandı"
        );
    }
    out
}

/// Lucene sorgusu kurar: `artist:"..." AND recording:"..."`.
///
/// Alanlardan biri boşsa o alan düşer; ikisi de boşsa sorgu boş döner ve
/// çağıran istek göndermez.
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

/// Lucene'in özel karakterlerini kaçırır.
///
/// Kaçırılmazsa `AC/DC` ya da `Where Is My Mind?` gibi adlar sorguyu bozar ve
/// MusicBrainz `400` döndürür — yani en çok ihtiyaç duyulan yerde, tuhaf
/// adlarda, çözümleme çöker.
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

// --- Tel biçimi (MusicBrainz ws/2, `fmt=json`) ---------------------------

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
    /// Milisaniye. MusicBrainz süreyi bilmiyorsa `null` gelir — bu bir bilgi,
    /// sıfır değil: [`super::fuzzy`] süreyi bilmediğinde farklı davranır.
    #[serde(default)]
    length: Option<u64>,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<MbArtistCredit>,
    /// Kaydı adaşlarından ayıran not: `"live, 1994-05-27: Astoria, London, UK"`.
    ///
    /// Canlı kayıtların **tek** işareti bu alan — başlık düpedüz `Creep`
    /// kalıyor. Boş dize de geliyor (`""`), o yüzden `filter` ile eleniyor:
    /// "not yok" ile "notu boş" aynı şey, ama `Some("")` skorlamaya boş bir
    /// bağlam sokardı.
    #[serde(default)]
    disambiguation: Option<String>,
    #[serde(default)]
    isrcs: Vec<String>,
}

impl MbRecording {
    /// `artist-credit` listesini tek dizeye çevirir.
    ///
    /// MusicBrainz çoklu sanatçıyı parça parça verir ve aradaki bağlacı
    /// (`joinphrase`) ayrı taşır: `[{name:"Jay-Z", joinphrase:" & "},
    /// {name:"Kanye West"}]` → `Jay-Z & Kanye West`. Yalnızca ilkini almak
    /// düetlerin yarısını kaybettirirdi.
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

/// Bu derlemenin varsayılan üstveri kaynağı.
///
/// # Errors
/// `http-client` feature'ı kapalıysa: çağıran kendi istemcisini verip
/// [`MusicBrainzLookup::new`] çağırmalı.
#[cfg(feature = "http-client")]
pub fn default_musicbrainz_lookup() -> Result<Arc<dyn MetadataLookup>> {
    Ok(Arc::new(MusicBrainzLookup::new(
        crate::net::default_http_client()?,
    )))
}

/// Bu derlemenin varsayılan üstveri kaynağı.
///
/// # Errors
/// Bu derlemede `http-client` kapalı olduğu için **her zaman** hata döner.
/// Sessizce çevrimdışı kaynağa düşmüyoruz: kullanıcı zincirin neden
/// `LocalKey`'de bittiğini bilmeli (K9).
#[cfg(not(feature = "http-client"))]
pub fn default_musicbrainz_lookup() -> Result<Arc<dyn MetadataLookup>> {
    Err(crate::error::Error::new(
        crate::diag::Stage::IdentityResolve,
        crate::error::ErrorKind::Unsupported {
            provider: "musicbrainz".to_owned(),
            what: "üstveri araması (`http-client` feature'ı kapalı derleme)".to_owned(),
            capabilities: "NONE".to_owned(),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::fake::FakeHttp;

    /// Testlerde hız kısıtı 0: sınanan şey kısıtın kendisi değil.
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

    /// K9: "bulamadım" hata değil — zincir bir sonraki halkaya geçmeli.
    #[tokio::test]
    async fn an_unknown_isrc_is_absence_not_failure() {
        let http = FakeHttp::new().route_status("/isrc/", 404, r#"{"error":"Not Found"}"#);
        let mb = lookup(http);
        let isrc = Isrc::parse("GBAYE9200001").unwrap();
        assert_eq!(mb.recording_by_isrc(&isrc).await.unwrap(), None);
    }

    /// ...ama "konuşamadım" hatadır ve aşamasını söyler.
    #[tokio::test]
    async fn a_server_error_is_reported_not_swallowed() {
        let http = FakeHttp::new().route_status("/recording?query=", 500, "bozuk");
        let mb = lookup(http);
        let err = mb
            .search_recordings("Radiohead", "Creep")
            .await
            .unwrap_err();
        let text = err.chain_text();
        assert!(text.starts_with("ADIM: NETWORK_REQUEST"), "{text}");
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
            .expect("User-Agent gönderilmeli: MusicBrainz aksi halde 403 döner");
        assert!(agent.value.starts_with("tonearm/"), "{}", agent.value);
        assert!(
            agent.value.contains('('),
            "iletişim adresi: {}",
            agent.value
        );
    }

    /// Tuhaf adlar sorguyu bozmamalı — kaçırılmazsa MusicBrainz `400` döner.
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

    /// Bozuk MBID sessizce düşmez: sayılır ve raporlanır.
    #[test]
    fn invalid_mbids_are_counted_not_silently_dropped() {
        let recordings: Vec<MbRecording> = serde_json::from_str(
            r#"[
                { "id": "not-a-uuid", "title": "Bozuk" },
                { "id": "b1a9c0e9-d987-4042-ae91-78d6a3267d69", "title": "Creep" }
            ]"#,
        )
        .unwrap();
        let candidates = collect_candidates(recordings, "test");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].title, "Creep");
    }

    /// Süre bilinmiyorsa `None` kalmalı — sıfır olmamalı (skorlama ayrımı).
    #[test]
    fn a_missing_length_stays_unknown_rather_than_zero() {
        let recordings: Vec<MbRecording> = serde_json::from_str(
            r#"[{ "id": "b1a9c0e9-d987-4042-ae91-78d6a3267d69", "title": "Creep" }]"#,
        )
        .unwrap();
        let candidates = collect_candidates(recordings, "test");
        assert_eq!(candidates[0].duration_ms, None);
    }

    /// Hız sınırı `503` "hayır" değil "yavaşla" demek: bir kez daha denenir.
    #[tokio::test]
    async fn a_rate_limited_response_is_retried() {
        // Sahte istemci ilk eşleşen yolu döndürüyor; iki farklı yol tanımlayıp
        // sırayla dönmesini sağlayamıyoruz, o yüzden burada sınanan şey
        // yeniden denemenin *sayılı* olduğu: 503 kalıcıysa hata dönmeli.
        let http = Arc::new(FakeHttp::new().route_status("/recording?query=", 503, "yavaşla"));
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
            "ilk deneme + {RATE_LIMIT_RETRIES} yeniden deneme"
        );
    }
}
