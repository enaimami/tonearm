//! `http-client` feature'ının somut istemcisi: `ureq` + rustls (D-020).
//!
//! **Bu dosya `ureq`'e dokunan tek yerdir.** Crate değişirse burası değişir;
//! sağlayıcılar [`super::HttpClient`] trait'ini gördüğü için etkilenmez.
//!
//! ## Bloklama uyarısı
//!
//! `ureq` bloklayan bir API. [`UreqClient::send`] async imzanın içinde
//! bloklar — çekirdek bir çalışma zamanı seçmediği için (`#[tokio::main]`
//! kurmaz) `spawn_blocking` çağıramıyoruz. CLI için bu sorun değil: komut
//! zaten cevabı bekliyor. Bloklamayan bir taşıma isteyen çağıran (GUI'nin
//! olay döngüsü, mobil) kendi `HttpClient`'ını verir; trait sınırı bu takası
//! geri alınabilir kılan şeydir.

use std::io::Read;
use std::time::Duration;

use super::{HttpClient, HttpFuture, HttpHeader, HttpMethod, HttpRequest, HttpResponse};
use crate::error::Result;

/// Üstveri yanıtları için tavan. Ses baytları buradan geçmez (bkz.
/// [`UreqClient::open_stream`]); düşmanca bir sunucu belleği doldurmasın.
const MAX_BODY_BYTES: u64 = 32 * 1024 * 1024;

/// Genel zaman aşımı: bağlantı + okuma. Sunucu asılı kalırsa komut da asılı
/// kalmasın.
const TIMEOUT: Duration = Duration::from_secs(30);

/// Eklenti isteklerinin süresi — çağrı bütçesinin (20 sn) altında kalmalı.
const PLUGIN_TIMEOUT: Duration = Duration::from_secs(15);

/// Bir eserin gövdesini okumanın üst sınırı. 40 MB ~45 KB/s'de iner.
const DOWNLOAD_BODY_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// `ureq` tabanlı HTTP istemcisi.
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
            // Durum kodunu hata değil veri olarak istiyoruz: 401 ile
            // "bağlanamadım" farklı tanılardır (K9).
            .http_status_as_error(false)
            .user_agent(concat!("headshell/", env!("CARGO_PKG_VERSION")))
            .build();
        Self {
            agent: ureq::Agent::new_with_config(config),
        }
    }

    /// Eklentilere verilen istemci: **yönlendirme izlemez** (D-069).
    ///
    /// İzin denetimi istek adresine bakıyor; istemci yönlendirmeyi kendisi
    /// izleseydi izinli bir adres eklentiyi izinsiz bir yere taşıyabilirdi.
    /// 3xx olduğu gibi döner, her adımı motor izler ve yeniden sorar.
    ///
    /// Süre de daha kısa: bir eklenti çağrısının bütçesi 20 sn ve tek bir
    /// istek bunun hepsini yememeli — yavaş bir istek eklentiye bir hata
    /// olarak dönsün, çağrının kendisi zaman aşımına uğramasın.
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

    /// Motorun eser indirmesi için istemci (D-069).
    ///
    /// Genel zaman aşımı **yok**: 40 MB'lık bir ikili yavaş bir bağlantıda
    /// 30 saniyeyi rahatça geçer ve genel süre gövdeyi okumayı da kapsıyor.
    /// Yerine her aşamanın kendi süresi var — bağlanamayan ya da hiç cevap
    /// vermeyen bir sunucu yine takılı bırakmaz.
    #[must_use]
    pub fn for_downloads() -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_connect(Some(TIMEOUT))
            .timeout_recv_response(Some(TIMEOUT))
            .timeout_recv_body(Some(DOWNLOAD_BODY_TIMEOUT))
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

    /// Ses akışı için **ilerlemeli** okuyucu açar.
    ///
    /// [`HttpClient::send`]'den ayrı, çünkü o gövdeyi tamamen belleğe alır;
    /// 40 MB'lık bir FLAC'ı çalmaya başlamadan önce indirmek "kesintisiz
    /// çalma"nın tersidir. Dönen ikili: bilinen toplam uzunluk (varsa) ve
    /// okuyucu.
    ///
    /// # Errors
    /// Bağlantı kurulamazsa ya da sunucu 2xx dışında bir kod dönerse.
    pub fn open_stream(
        &self,
        url: &str,
        headers: &[HttpHeader],
    ) -> Result<(Option<u64>, Box<dyn Read + Send>)> {
        let mut builder = self.agent.get(url);
        for header in headers {
            builder = builder.header(&header.name, &header.value);
        }
        let response = builder.call().map_err(|err| super::network_err(url, err))?;

        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            // Gövdeyi okumadan hata üretmek "ne dedi?" sorusunu cevapsız
            // bırakır; kısa bir parçasını alıyoruz.
            let mut response = response;
            let detail = response
                .body_mut()
                .with_config()
                .limit(1024)
                .read_to_string()
                .unwrap_or_else(|err| format!("(gövde okunamadı: {err})"));
            // Aşama seçimi `error_for_status` ile aynı kuraldan (D-023):
            // akış açarken 401 almak da bir kimlik sorunudur, ağ sorunu değil.
            return Err(crate::Error::new(
                super::stage_for_status(status),
                crate::ErrorKind::HttpStatus {
                    url: url.to_owned(),
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
        // Bloklama kasıtlı ve dokümante (modül başlığı).
        Box::pin(std::future::ready(self.send_blocking(request)))
    }
}

fn collect_headers(map: &ureq::http::HeaderMap) -> Vec<HttpHeader> {
    map.iter()
        .map(|(name, value)| {
            HttpHeader::new(
                name.as_str(),
                // Geçersiz UTF-8 başlığını **düşürmüyoruz**: varlığı da bir
                // bilgidir. Kayıpsız olmayan gösterim tanı içindir.
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect()
}
