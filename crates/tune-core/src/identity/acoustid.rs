//! AcoustID — K6 zincirinin **4. ve son halkası**.
//!
//! İlk üç halka metne bakar ve üçü de etiketin doğru olduğunu varsayar.
//! `track01.mp3` adlı, etiketsiz bir dosyada üçü de çaresizdir: ISRC yok,
//! arayacak sanatçı/başlık yok, bulanık eşleştirilecek metin yok. Bu halka
//! **sesin kendisine** bakar — [`fingerprint`](super::fingerprint) dosyadan
//! bir Chromaprint parmak izi çıkarır, burası onu AcoustID'ye sorar ve
//! karşılığında MusicBrainz kayıt kimlikleri alır.
//!
//! Sıra tesadüf değil: parmak izi en pahalı halka (dosyanın tamamı çözülür)
//! ve **dosya elde yokken hiç çalışamaz**. İçe aktarılan bir dinleme
//! geçmişinin dosyası olmadığı için o kayıtlar bu halkaya hiç ulaşmaz —
//! zincirin buraya kadar gelmesi bir istisnadır, kural değil.
//!
//! ## İstemci anahtarı (D-046)
//!
//! AcoustID her sorguda bir uygulama anahtarı ister. İki kaynağı var ve sıra
//! şu: önce sır deposu (`identity:acoustid` / `api_key`), yoksa derlemeye
//! gömülü varsayılan. Kullanıcının koyduğu anahtar **her zaman** kazanır —
//! gömülü anahtar iptal edilirse ya da kotası dolarsa kimse kilitlenmesin.
//!
//! İkisi de yoksa halka çalışmaz ve bu **söylenir**: "AcoustID anahtarı yok"
//! ile "AcoustID eşleşme bulamadı" bambaşka iki tanıdır ve ikincisi gibi
//! görünen bir birincisi, kusuru dosyada arattırır (K9).
//!
//! ## Sınırlar
//!
//! - **Saniyede üç istek.** AcoustID'nin açıkladığı ortalama; aşan istemci
//!   `429` alır. [`crate::net::RateLimiter`] çağrılar arasında uyur.
//! - **POST, GET değil.** Base64'lenmiş parmak izi binlerce karakter tutar;
//!   URL'ye koymak onu aracıların kesme sınırına teslim ederdi.

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;

use super::fingerprint::Fingerprint;
use super::{Candidate, FingerprintCandidate, FingerprintLookup, LookupFuture};
use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};
use crate::ids::Mbid;
use crate::net::{HttpClient, HttpHeader, HttpRequest, RateLimiter, encode_query, parse_json};

/// Genel AcoustID sunucusu.
pub const DEFAULT_BASE_URL: &str = "https://api.acoustid.org/v2";

/// Anahtarın arandığı sır ad alanı (D-042 deposu).
pub const SECRET_NAMESPACE: &str = "identity:acoustid";

/// Sır deposundaki anahtarın adı.
pub const SECRET_KEY: &str = "api_key";

/// Derlemeye gömülü varsayılan istemci anahtarı.
///
/// **Boş bırakılmıştır ve bu kasıtlı.** Anahtar `acoustid.org/new-application`
/// adresinden bu proje adına kaydedilmeli ve buraya yazılmalıdır; uydurulmuş
/// bir dize koymak, ilk canlı çağrıda "geçersiz anahtar" olarak dönerdi ve
/// kusuru anahtarın kendisinde değil parmak izinde arattırırdı.
///
/// Boş kaldığı sürece 4. halka yalnızca kullanıcının kendi anahtarıyla
/// çalışır ve anahtarsız çağrı [`missing_key_err`] ile reddedilir.
const EMBEDDED_API_KEY: &str = "";

/// İki istek arasındaki en kısa süre.
///
/// AcoustID ortalama saniyede üç isteğe izin veriyor. 340 ms, saat farkı için
/// pay bırakıyor — sınırı yalayan bir istemci `429` yiyip yeniden denemeye
/// başlar, bu da toplamda daha yavaştır (MusicBrainz'de öğrenildi).
const MIN_INTERVAL: Duration = Duration::from_millis(340);

/// `429` (hız sınırı) sonrası kaç kez yeniden denenir.
const RATE_LIMIT_RETRIES: u32 = 2;

/// Bu skorun altındaki AcoustID eşleşmeleri hiç aday sayılmaz.
///
/// AcoustID kendi güvenini 0–1 arasında veriyor ve zayıf eşleşmeleri de
/// listeliyor. 0.5'in altı pratikte "aynı parça olabilir de olmayabilir de"
/// demek; onu zincire aday olarak sokmak, kimliği bir tahmine bağlamak olur.
const MIN_ACOUSTID_SCORE: f64 = 0.5;

/// AcoustID'ye bağlanan parmak izi kaynağı.
///
/// HTTP'ye doğrudan değil [`HttpClient`] üstünden gidiyor (D-020): testler
/// sahte istemci verir, mobil kendi yığınını verir.
pub struct AcoustIdLookup {
    http: Arc<dyn HttpClient>,
    base_url: String,
    api_key: String,
    user_agent: String,
    limiter: RateLimiter,
}

impl std::fmt::Debug for AcoustIdLookup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Anahtar **yazılmıyor**: `Debug` çıktısı log'a ve tanı raporuna
        // düşebilir, sırlar oraya girmez (D-042).
        f.debug_struct("AcoustIdLookup")
            .field("base_url", &self.base_url)
            .field("api_key_set", &!self.api_key.is_empty())
            .finish_non_exhaustive()
    }
}

impl AcoustIdLookup {
    /// Gömülü varsayılan anahtarla genel sunucuya bağlanan kaynak.
    ///
    /// Gömülü anahtar boşsa çağrılar [`missing_key_err`] döndürür — nesne
    /// yine de kurulur, çünkü [`Self::with_api_key`] onu düzeltebilir.
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

    /// Kullanıcının kendi anahtarı. Boş dize **yok sayılır**.
    ///
    /// Boşu yok saymak, "anahtar ayarlamayı denedim ama boş bıraktım"
    /// durumunda gömülü anahtara düşmeyi sağlıyor; boşu geçerli saymak
    /// çağrıyı sunucuya kadar götürüp orada reddettirirdi.
    #[must_use]
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        let key = api_key.into();
        if !key.trim().is_empty() {
            self.api_key = key.trim().to_owned();
        }
        self
    }

    /// Başka bir sunucu (test sunucusu ya da kendi kopyanız).
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_owned();
        self
    }

    /// Kendi `User-Agent`'ınız.
    #[must_use]
    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }

    /// İstekler arasındaki en kısa süre. Yalnızca testler için.
    #[must_use]
    pub fn with_min_interval(mut self, interval: Duration) -> Self {
        self.limiter = RateLimiter::new(interval);
        self
    }

    /// Bir parmak izini sorgular.
    async fn lookup(&self, fingerprint: &Fingerprint) -> Result<Vec<FingerprintCandidate>> {
        if self.api_key.is_empty() {
            return Err(missing_key_err());
        }

        let url = format!("{}/lookup", self.base_url);
        // `meta=recordings`: kimlik zinciri kayıt (recording) düzeyinde
        // çalışıyor. Yayın (release) bilgisi istemiyoruz — istemek yanıtı
        // katlar ve zincirin kullanmadığı veriyi taşır.
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

            // `429` = kota aşıldı. Sunucu "yavaşla" diyor, "hayır" demiyor.
            if response.status == 429 && attempt < RATE_LIMIT_RETRIES {
                attempt += 1;
                tracing::warn!(
                    url,
                    deneme = attempt,
                    "AcoustID hız sınırı: bekleyip yeniden denenecek"
                );
                std::thread::sleep(MIN_INTERVAL);
                continue;
            }
            // Gövde **durum kodundan önce** okunuyor ve bu, canlı koşumun
            // öğrettiği şey (D-046): AcoustID geçersiz anahtara `400` dönüyor
            // ve asıl tanı (`invalid API key`) gövdede. Önce
            // `error_for_status` çağırmak onu `NETWORK_REQUEST` diye
            // raporluyordu — oysa sunucuya ulaşıldı, sunucu okudu ve **hayır
            // dedi**. D-023'ün `401`/`403` için kurduğu ayrımın aynısı: "ağa
            // çıkamadım" kullanıcıyı bağlantısını kontrol etmeye gönderir,
            // oysa yapması gereken şey anahtarını düzeltmek.
            if let Ok(payload) = serde_json::from_slice::<LookupResponse>(&response.body) {
                break payload;
            }
            // Ayrıştırılamayan gövde: durum kodu ne diyorsa o. Proxy hata
            // sayfası, kesilmiş yanıt, bakım ekranı — hiçbiri AcoustID'nin
            // kendi cevabı değil.
            response.error_for_status(&url)?;
            break parse_json::<LookupResponse>(&response, "acoustid lookup")?;
        };

        // AcoustID uygulama hatasını gövdede `status: error` ile söylüyor.
        // Bunu görmezden gelmek, geçersiz anahtarı "eşleşme yok" diye
        // raporlardı — K9'un tam olarak yasakladığı karışım.
        if payload.status != "ok" {
            let detail = payload
                .error
                .map_or_else(|| "sebep bildirilmedi".to_owned(), |err| err.message);
            return Err(Error::new(
                Stage::IdentityResolve,
                ErrorKind::InvalidInput {
                    detail: format!("AcoustID reddetti ({url}): {detail}"),
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
                    // Tanınmayan bir kimlik sessizce atılmaz: sayılabilir bir
                    // uyarı bırakır. Sessiz `unwrap_or_default` yasak.
                    tracing::warn!(
                        id = recording.id,
                        "AcoustID geçersiz bir MBID döndürdü, aday atlanıyor"
                    );
                    continue;
                };
                let Some(title) = recording.title else {
                    // Başlıksız kayıt skorlanamaz: bulanık karşılaştırmanın
                    // iki alanından biri eksik.
                    tracing::debug!(id = %mbid, "başlıksız AcoustID kaydı atlanıyor");
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
                        duration_ms: recording.duration.map(|secs| u64::from(secs) * 1000),
                        isrc: None,
                        disambiguation: None,
                    },
                    score: result.score,
                });
            }
        }

        // En güçlü eşleşme başta. Beraberlikte MBID sırası: keyfi ama
        // **sabit** — aynı dosya yarın başka bir kanonik kimlik alamaz
        // (D-045'in ikinci dersi).
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

/// Anahtarsız çağrının hatası.
///
/// Ayrı bir fonksiyon çünkü metni tam olarak bu: kullanıcı ne yapacağını
/// buradan öğreniyor.
fn missing_key_err() -> Error {
    Error::new(
        Stage::IdentityResolve,
        ErrorKind::InvalidInput {
            detail: format!(
                "AcoustID istemci anahtarı yok — bu derlemede gömülü anahtar boş. \
                 `tune secret set {SECRET_NAMESPACE} {SECRET_KEY} <anahtar>` ile \
                 kendi anahtarınızı koyun (acoustid.org/new-application)."
            ),
        },
    )
}

/// Bu derlemenin varsayılan `User-Agent`'ı.
fn default_user_agent() -> String {
    format!("tune/{}", env!("CARGO_PKG_VERSION"))
}

/// `POST /v2/lookup` yanıtı.
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
    /// AcoustID'nin kendi eşleşme güveni, 0–1.
    score: f64,
    #[serde(default)]
    recordings: Vec<RecordingRef>,
}

#[derive(Debug, Deserialize)]
struct RecordingRef {
    id: String,
    #[serde(default)]
    title: Option<String>,
    /// Saniye — AcoustID süreyi tam saniye veriyor.
    #[serde(default)]
    duration: Option<u32>,
    #[serde(default)]
    artists: Vec<ArtistRef>,
}

#[derive(Debug, Deserialize)]
struct ArtistRef {
    name: String,
}

/// Bu derlemenin varsayılan AcoustID kaynağı.
///
/// # Errors
/// `http-client` feature'ı kapalıysa.
#[cfg(feature = "http-client")]
pub fn default_acoustid_lookup() -> Result<Arc<dyn FingerprintLookup>> {
    Ok(Arc::new(AcoustIdLookup::new(
        crate::net::default_http_client()?,
    )))
}

/// Bu derlemenin varsayılan AcoustID kaynağı.
///
/// # Errors
/// Bu derlemede `http-client` kapalı olduğu için **her zaman** hata döner.
#[cfg(not(feature = "http-client"))]
pub fn default_acoustid_lookup() -> Result<Arc<dyn FingerprintLookup>> {
    Err(Error::new(
        Stage::IdentityResolve,
        ErrorKind::Unsupported {
            provider: "acoustid".to_owned(),
            what: "parmak izi sorgusu (`http-client` feature'ı kapalı derleme)".to_owned(),
            capabilities: "NONE".to_owned(),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::fake::FakeHttp;

    const CREEP_MATCH: &str = r#"{
      "status": "ok",
      "results": [
        {
          "id": "9ff43b6a-4f16-427c-93c2-92307ca505e0",
          "score": 0.97,
          "recordings": [
            {
              "id": "b1a9c0e9-d987-4042-ae91-78d6a3267d69",
              "title": "Creep",
              "duration": 238,
              "artists": [{ "id": "a74b1b7f-71a5-4011-9441-d0b5e4122711", "name": "Radiohead" }]
            }
          ]
        }
      ]
    }"#;

    fn sample() -> Fingerprint {
        Fingerprint {
            raw: vec![1, 2, 3, 4, 5, 6, 7, 8],
            duration_secs: 238,
        }
    }

    /// `pattern` yerine sabit bir yol: tek uç nokta var.
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
        let http = fake(CREEP_MATCH);
        let found = lookup(http).lookup(&sample()).await.expect("sorgu");

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].candidate.title, "Creep");
        assert_eq!(found[0].candidate.artist, "Radiohead");
        assert_eq!(found[0].candidate.duration_ms, Some(238_000));
        assert!((found[0].score - 0.97).abs() < f64::EPSILON);
    }

    /// Parmak izi gövdede gitmeli — URL'de değil.
    ///
    /// Kesilen bir URL "eşleşme yok" gibi görünür; bunu testle sabitliyoruz
    /// çünkü hata anında ayırt edilemez.
    #[tokio::test]
    async fn the_fingerprint_travels_in_the_body_not_the_url() {
        let http = fake(CREEP_MATCH);
        lookup(Arc::clone(&http))
            .lookup(&sample())
            .await
            .expect("sorgu");

        let request = http.last_request().expect("istek gitmeli");
        assert_eq!(request.method, crate::net::HttpMethod::Post);
        assert!(!request.url.contains("fingerprint"), "{}", request.url);
        let body = String::from_utf8_lossy(request.body.as_deref().unwrap_or_default()).to_string();
        assert!(body.contains("fingerprint="), "{body}");
        assert!(body.contains("duration=238"), "{body}");
    }

    /// Anahtar sorgu dizesine sızmamalı: URL'ler log'lanır.
    #[tokio::test]
    async fn the_api_key_never_appears_in_the_url() {
        let http = fake(CREEP_MATCH);
        lookup(Arc::clone(&http))
            .lookup(&sample())
            .await
            .expect("sorgu");

        let request = http.last_request().expect("istek");
        assert!(!request.url.contains("test-key"), "{}", request.url);
    }

    /// Kullanıcının anahtarı gömülü olanı geçersiz kılar.
    #[test]
    fn a_user_key_overrides_the_embedded_one_but_an_empty_one_does_not() {
        let http = fake(CREEP_MATCH);
        let set =
            AcoustIdLookup::new(Arc::clone(&http) as Arc<dyn HttpClient>).with_api_key("kullanici");
        assert_eq!(set.api_key, "kullanici");

        let blank = AcoustIdLookup::new(http as Arc<dyn HttpClient>).with_api_key("   ");
        assert_eq!(blank.api_key, EMBEDDED_API_KEY);
    }

    /// Anahtarsız çağrı ağa çıkmadan, ne yapılacağını söyleyerek durmalı.
    #[tokio::test]
    async fn a_missing_key_is_reported_before_any_request_goes_out() {
        let http = fake(CREEP_MATCH);
        let bare = AcoustIdLookup::new(Arc::clone(&http) as Arc<dyn HttpClient>);
        // Gömülü anahtar dolu bir dağıtımda bu test anlamsız olurdu; o zaman
        // atlanır ve sebebi yazılır.
        if !bare.api_key.is_empty() {
            eprintln!("atlanıyor: bu derlemede gömülü AcoustID anahtarı var");
            return;
        }

        let err = bare.lookup(&sample()).await.unwrap_err();
        let text = err.chain_text();
        assert!(text.contains("anahtar"), "{text}");
        assert!(text.contains("secret set"), "{text}");
        assert!(http.last_request().is_none(), "ağa çıkılmamalıydı");
    }

    /// Gövdede gelen uygulama hatası "eşleşme yok" sayılmamalı.
    #[tokio::test]
    async fn an_application_error_in_the_body_is_not_an_empty_result() {
        let http = fake(r#"{"status":"error","error":{"code":4,"message":"invalid API key"}}"#);
        let err = lookup(http).lookup(&sample()).await.unwrap_err();
        let text = err.chain_text();
        assert!(text.starts_with("ADIM: IDENTITY_RESOLVE"), "{text}");
        assert!(text.contains("invalid API key"), "{text}");
    }

    /// Geçersiz anahtar `400` ile geliyor — ve bu bir **ağ** hatası değil.
    ///
    /// Canlı koşumun bulduğu kusur (D-046): gövdeyi durum kodundan sonra
    /// okuyan ilk sürüm bunu `NETWORK_REQUEST` diye raporluyordu ve
    /// kullanıcıyı bağlantısını kontrol etmeye gönderiyordu. Sunucuya
    /// ulaşıldı; sunucu okudu ve hayır dedi (D-023 ayrımı).
    #[tokio::test]
    async fn a_rejected_key_arrives_as_400_and_is_still_an_identity_stage_error() {
        let http = Arc::new(FakeHttp::new().route_status(
            "/v2/lookup",
            400,
            r#"{"error": {"code": 4, "message": "invalid API key"}, "status": "error"}"#,
        ));
        let err = lookup(http).lookup(&sample()).await.unwrap_err();
        let text = err.chain_text();
        assert!(text.starts_with("ADIM: IDENTITY_RESOLVE"), "{text}");
        assert!(!text.contains("NETWORK_REQUEST"), "{text}");
        assert!(text.contains("invalid API key"), "{text}");
    }

    /// AcoustID'nin cevabı olmayan bir gövde durum koduna teslim edilir.
    ///
    /// Proxy hata sayfası, bakım ekranı, kesilmiş yanıt — hiçbiri servisin
    /// kendi reddi değil ve kimlik aşamasına yazılmamalı.
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

    /// Gerçekten eşleşme yoksa bu hata değil, boş listedir.
    #[tokio::test]
    async fn no_match_is_an_empty_list_not_an_error() {
        let http = fake(r#"{"status":"ok","results":[]}"#);
        let found = lookup(http).lookup(&sample()).await.expect("sorgu");
        assert!(found.is_empty());
    }

    /// Zayıf eşleşmeler aday sayılmaz.
    #[tokio::test]
    async fn a_weak_match_is_not_offered_as_a_candidate() {
        let weak = CREEP_MATCH.replace("0.97", "0.31");
        let http = fake(&weak);
        let found = lookup(http).lookup(&sample()).await.expect("sorgu");
        assert!(found.is_empty(), "{found:?}");
    }

    /// Geçersiz MBID taşıyan aday atlanır ama sorgu düşmez.
    #[tokio::test]
    async fn a_malformed_mbid_is_skipped_without_failing_the_lookup() {
        let broken = CREEP_MATCH.replace("b1a9c0e9-d987-4042-ae91-78d6a3267d69", "mbid-degil");
        let http = fake(&broken);
        let found = lookup(http).lookup(&sample()).await.expect("sorgu");
        assert!(found.is_empty());
    }

    /// `Debug` çıktısı anahtarı taşımamalı — tanı raporu kopyalanıp
    /// yapıştırılan bir metin (D-042).
    #[test]
    fn debug_output_does_not_leak_the_key() {
        let http = fake(CREEP_MATCH);
        let text = format!("{:?}", lookup(http));
        assert!(!text.contains("test-key"), "{text}");
    }
}
