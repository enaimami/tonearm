//! HTTP taşıma sınırı (D-020).
//!
//! Çekirdek ağa **doğrudan** bağlanmaz: [`HttpClient`] bir trait'tir, somut
//! istemci `http-client` feature'ı arkasındadır. Bunun üç sonucu var:
//!
//! 1. Sağlayıcı mantığı (Subsonic, Jellyfin) ağ olmadan test edilebilir —
//!    testler sahte bir istemci verir.
//! 2. Mobil bağlamalar kendi HTTP yığınlarını `Arc<dyn HttpClient>` olarak
//!    verebilir; TLS ağacını ikinci kez taşımazlar (K7).
//! 3. Feature içindeki crate seçimi geri alınabilir bir ayrıntıya döner.
//!
//! ## `uniffi` kısıtı (K7)
//!
//! Trait `dyn` uyumlu: kutulanmış future döndürür, generic/lifetime/closure
//! taşımaz. Gövde `Vec<u8>`, başlıklar isim/değer listesi — hepsi `uniffi`'nin
//! ifade edebildiği tipler.

#[cfg(feature = "http-client")]
mod ureq_client;

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};

#[cfg(feature = "http-client")]
pub use ureq_client::UreqClient;

/// Tek bir HTTP başlığı.
///
/// `HashMap` değil liste: `uniffi` için daha basit ve sıra korunur.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpHeader {
    pub name: String,
    pub value: String,
}

impl HttpHeader {
    #[must_use]
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// Desteklenen yöntemler.
///
/// Kasıtlı olarak dar: uzak sağlayıcılar için `GET` ve `POST` yetiyor.
/// Genişletmek gerekirse eklenir; bugün ihtiyaç olmayan yöntemi taşımak
/// yabancı taraf uygulamalarına (mobil) boşuna yük olur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
}

impl HttpMethod {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

impl std::fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Gönderilecek istek.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<HttpHeader>,
    /// Gövde (POST için). `None` gövdesiz demek.
    pub body: Option<Vec<u8>>,
}

impl HttpRequest {
    #[must_use]
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Get,
            url: url.into(),
            headers: Vec::new(),
            body: None,
        }
    }

    #[must_use]
    pub fn post_json(url: impl Into<String>, body: impl Into<Vec<u8>>) -> Self {
        Self {
            method: HttpMethod::Post,
            url: url.into(),
            headers: vec![HttpHeader::new("Content-Type", "application/json")],
            body: Some(body.into()),
        }
    }

    /// Form kodlu POST (`application/x-www-form-urlencoded`).
    ///
    /// GET'in yetmediği yer için: bir Chromaprint parmak izi base64'e
    /// çevrildiğinde binlerce karakter tutar ve pek çok sunucu/aracı URL'yi
    /// 8 KB civarında keser. Kesilen bir URL "eşleşme yok" gibi görünür —
    /// yani tanısı en zor başarısızlık türü (K9). Gövdede böyle bir sınır yok.
    #[must_use]
    pub fn post_form(url: impl Into<String>, body: impl Into<Vec<u8>>) -> Self {
        Self {
            method: HttpMethod::Post,
            url: url.into(),
            headers: vec![HttpHeader::new(
                "Content-Type",
                "application/x-www-form-urlencoded",
            )],
            body: Some(body.into()),
        }
    }

    #[must_use]
    pub fn with_headers(mut self, headers: Vec<HttpHeader>) -> Self {
        self.headers.extend(headers);
        self
    }
}

/// Dönen yanıt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<HttpHeader>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    #[must_use]
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// Bir başlığı (büyük/küçük harf duyarsız) okur.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }

    /// Gövdeyi metin olarak verir (UTF-8 değilse kayıpsız değil, tanı içindir).
    #[must_use]
    pub fn text_lossy(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// 2xx değilse hata döndürür.
    ///
    /// Gövdenin başı hata mesajına giriyor: "500 döndü" demek bir tanı
    /// değil, sunucunun ne dediği tanıdır (K9).
    ///
    /// # Errors
    /// Durum kodu 2xx dışındaysa.
    pub fn error_for_status(&self, url: &str) -> Result<()> {
        if self.is_success() {
            return Ok(());
        }
        Err(Error::new(
            stage_for_status(self.status),
            ErrorKind::HttpStatus {
                url: url.to_owned(),
                status: self.status,
                detail: clip(&self.text_lossy(), DETAIL_LIMIT),
            },
        ))
    }
}

/// Hata ayrıntısına alınan gövde uzunluğu (bayt).
const DETAIL_LIMIT: usize = 200;

/// Çağrılar arasında en az `interval` geçmesini sağlayan kısıtlayıcı.
///
/// Kotası olan her servis için: MusicBrainz saniyede bir istek, AcoustID
/// saniyede üç. Servis başına bir örnek tutulur — sınır ortak değil, her
/// servisin kendi kotası kendi sayacıyla ölçülür.
///
/// ## Neden `thread::sleep`, `async` bir uykuya rağmen
///
/// Çekirdek çalışma zamanı kurmaz (K7 / konvansiyon: "çalışma zamanını çağıran
/// seçsin") ve bu yüzden `tokio::time::sleep` çağıramaz — bağımlılık olarak
/// `tokio` çekirdekte yok. Altımızdaki HTTP istemcisi (`ureq`) zaten senkron:
/// her istek çağıran iş parçacığını bloklar. Kısıtlayıcının aynı iş parçacığını
/// bloklaması bu yüzden yeni bir kısıt getirmiyor, var olanla tutarlı.
#[derive(Debug)]
pub(crate) struct RateLimiter {
    interval: Duration,
    last: Mutex<Option<Instant>>,
}

impl RateLimiter {
    pub(crate) fn new(interval: Duration) -> Self {
        Self {
            interval,
            last: Mutex::new(None),
        }
    }

    /// Sıra gelene kadar bekler ve çıkarken damgayı günceller.
    ///
    /// Kilit uyku boyunca **tutulmaz**: iki iş parçacığı aynı anda girerse
    /// ikisi de bekler, ama biri diğerinin uykusunu uzatmaz.
    pub(crate) fn acquire(&self) {
        let wait = {
            // Kilit zehirlenmişse (başka bir iş parçacığı panikledi) kısıtlamayı
            // düşürmüyoruz: `unwrap` yerine "bilmiyorum, tam aralık bekle".
            let Ok(mut last) = self.last.lock() else {
                std::thread::sleep(self.interval);
                return;
            };
            let now = Instant::now();
            let wait = last
                .map(|prev| self.interval.saturating_sub(now.duration_since(prev)))
                .unwrap_or_default();
            // Damgayı şimdiden ileri al: sıradaki çağıran bizim uyumamızı da
            // hesaba katsın, yoksa ikisi birlikte uyanır.
            *last = Some(now + wait);
            wait
        };
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
    }
}

/// Metni en fazla `limit` bayta kırpar — **karakter sınırında**.
///
/// `String::truncate` sınır ortasına düşerse panikler; sunucunun Türkçe
/// (ya da herhangi bir çok baytlı) hata mesajı `headshell`'u düşürebilirdi.
/// K8: çekirdekte panik yok.
pub(crate) fn clip(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut end = limit;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// Bir HTTP durum kodunun hangi aşamaya ait olduğu (D-023).
///
/// `401`/`403` **taşıma hatası değildir**: bağlantı kuruldu, istek gitti,
/// sunucu okudu ve reddetti. Bunu `NETWORK_REQUEST` diye raporlamak
/// kullanıcıyı ağını kontrol etmeye gönderir; oysa yapması gereken şey
/// kimliğini düzeltmek. K9'un "ulaşamadım ≠ hayır dedi" ayrımı burada da
/// geçerli. Geri kalan kodlar (404, 5xx, 429…) taşıma katmanında kalıyor:
/// onlarda hangi katmanın hata verdiği gövdeden okunmadan bilinemez.
pub(crate) fn stage_for_status(status: u16) -> Stage {
    match status {
        401 | 403 => Stage::ProviderCall,
        _ => Stage::NetworkRequest,
    }
}

/// Bir HTTP çağrısının dönüşü.
///
/// `async fn` yerine kutulanmış future: trait'in `dyn` uyumlu olması gerekiyor
/// (K7 / D-006 ile aynı gerekçe).
pub type HttpFuture<'a> = Pin<Box<dyn Future<Output = Result<HttpResponse>> + Send + 'a>>;

/// Ağa çıkabilen her şey.
///
/// Yanıt gövdesi **tamamen belleğe alınır**: bu trait üstveri çağrıları
/// (arama, ping, parça bilgisi) içindir. Ses baytları buradan geçmez —
/// akış [`crate::playback`] tarafında ilerlemeli okunur, yoksa 40 MB'lık
/// bir FLAC çalmaya başlamadan önce tamamen inmek zorunda kalırdı.
pub trait HttpClient: Send + Sync {
    fn send<'a>(&'a self, request: &'a HttpRequest) -> HttpFuture<'a>;
}

/// Bu derlemenin varsayılan HTTP istemcisi.
///
/// # Errors
/// `http-client` feature'ı kapalıysa: çağıran kendi istemcisini vermeli.
/// Sessizce "ağ yok" demek yerine hangi derleme kararının bunu yaptığını
/// söylüyoruz.
#[cfg(feature = "http-client")]
pub fn default_http_client() -> Result<Arc<dyn HttpClient>> {
    Ok(Arc::new(UreqClient::new()))
}

/// Bu derlemenin varsayılan HTTP istemcisi.
///
/// # Errors
/// Bu derlemede `http-client` kapalı olduğu için **her zaman** hata döner.
#[cfg(not(feature = "http-client"))]
pub fn default_http_client() -> Result<Arc<dyn HttpClient>> {
    Err(Error::new(
        Stage::NetworkRequest,
        ErrorKind::Unsupported {
            provider: "net".to_owned(),
            what: "HTTP istemcisi (`http-client` feature'ı kapalı derleme)".to_owned(),
            capabilities: "NONE".to_owned(),
        },
    ))
}

/// Yanıt gövdesini JSON olarak çözer.
///
/// `pub(crate)`: generic bir imza dışa açılırsa K7 kırılır. Çağıranlar
/// tiplenmiş sağlayıcı yüzeyini görür, bu yardımcıyı değil.
pub(crate) fn parse_json<T: serde::de::DeserializeOwned>(
    response: &HttpResponse,
    what: &str,
) -> Result<T> {
    serde_json::from_slice(&response.body).map_err(|source| {
        Error::new(
            Stage::ProviderCall,
            ErrorKind::Json {
                entry: what.to_owned(),
                source,
            },
        )
    })
}

/// Ağ katmanı hatası üretir (bağlantı kurulamadı, zaman aşımı, TLS…).
///
/// Yalnızca gerçekten sokete dokunan derlemelerde var: somut istemci
/// (`http-client`) ve testlerdeki sahte istemci. Varsayılan feature'larla
/// derlenen çekirdekte çağıranı yok — orada durması ölü kod uyarısıydı.
#[cfg(any(feature = "http-client", test))]
pub(crate) fn network_err(url: &str, detail: impl std::fmt::Display) -> Error {
    Error::new(
        Stage::NetworkRequest,
        ErrorKind::Network {
            url: url.to_owned(),
            detail: detail.to_string(),
        },
    )
}

/// Bir sorgu parametresini yüzde kodlar.
///
/// Kendi yazıyoruz: tek kullanım için bir kodlama crate'i eklemek ağacı
/// büyütür. Kural RFC 3986'nın `unreserved` kümesi — geri kalan her bayt
/// `%XX` olur, böylece boşluklu ve Türkçe karakterli aramalar bozulmaz.
pub(crate) fn encode_query(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            _ => {
                out.push('%');
                out.push(HEX[usize::from(byte >> 4)] as char);
                out.push(HEX[usize::from(byte & 0x0f)] as char);
            }
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod fake;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_encoding_survives_spaces_and_turkish() {
        assert_eq!(encode_query("Ezhel - Geceler"), "Ezhel%20-%20Geceler");
        assert_eq!(encode_query("Şebnem"), "%C5%9Eebnem");
        assert_eq!(encode_query("a~b_c.d-e"), "a~b_c.d-e");
    }

    #[test]
    fn non_2xx_becomes_an_error_carrying_the_body() {
        let response = HttpResponse {
            status: 500,
            headers: Vec::new(),
            body: b"nope".to_vec(),
        };
        let err = response
            .error_for_status("http://ev/rest/ping")
            .unwrap_err();
        let text = err.chain_text();
        assert!(text.starts_with("ADIM: NETWORK_REQUEST"), "{text}");
        assert!(text.contains("500"), "{text}");
        assert!(text.contains("nope"), "sunucunun dediği görünmeli: {text}");
    }

    /// D-023: reddedilmek ağ hatası değildir — aşama onu söylemeli.
    #[test]
    fn a_rejected_request_is_reported_at_the_provider_stage_not_the_network() {
        for status in [401, 403] {
            let response = HttpResponse {
                status,
                headers: Vec::new(),
                body: b"Access token is invalid or expired.".to_vec(),
            };
            let text = response
                .error_for_status("http://ev/Users/Me")
                .unwrap_err()
                .chain_text();
            assert!(
                text.starts_with("ADIM: PROVIDER_CALL"),
                "{status} kimlik reddi: {text}"
            );
            assert!(text.contains(&status.to_string()), "{text}");
        }

        // Geri kalanı taşıma katmanında kalıyor: hangi katmanın hata verdiği
        // gövde okunmadan bilinemez.
        for status in [404, 429, 500, 502] {
            let response = HttpResponse {
                status,
                headers: Vec::new(),
                body: Vec::new(),
            };
            let text = response
                .error_for_status("http://ev/rest/ping")
                .unwrap_err()
                .chain_text();
            assert!(
                text.starts_with("ADIM: NETWORK_REQUEST"),
                "{status}: {text}"
            );
        }
    }

    /// Sunucunun çok baytlı hata mesajı `headshell`'u **düşürmemeli** (K8).
    #[test]
    fn a_long_multibyte_body_is_clipped_without_panicking() {
        // "ğ" 2 bayt: 200 baytlık sınır karakterin ortasına denk geliyor.
        let response = HttpResponse {
            status: 500,
            headers: Vec::new(),
            body: "ğ".repeat(300).into_bytes(),
        };
        let text = response
            .error_for_status("http://ev/rest/ping")
            .unwrap_err()
            .chain_text();
        assert!(text.contains('ğ'), "{text}");

        assert_eq!(clip("kısa", 200), "kısa");
        assert_eq!(clip(&"ğ".repeat(300), 200).len(), 200);
        // Tek karakter geri gidilmeli, sınır ortada kalmamalı.
        assert_eq!(clip(&"ğ".repeat(300), 201).len(), 200);
    }

    #[test]
    fn headers_are_read_case_insensitively() {
        let response = HttpResponse {
            status: 200,
            headers: vec![HttpHeader::new("Content-Length", "42")],
            body: Vec::new(),
        };
        assert_eq!(response.header("content-length"), Some("42"));
        assert_eq!(response.header("CONTENT-LENGTH"), Some("42"));
        assert_eq!(response.header("etag"), None);
    }

    /// Kısıtlayıcı gerçekten bekletiyor mu — süre ölçülerek.
    #[test]
    fn the_rate_limiter_actually_spaces_calls_apart() {
        let limiter = RateLimiter::new(Duration::from_millis(40));
        let start = Instant::now();
        limiter.acquire(); // ilki beklemez
        limiter.acquire();
        limiter.acquire();
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(80),
            "iki aralık beklenmeliydi, {elapsed:?} geçti"
        );
    }
}
