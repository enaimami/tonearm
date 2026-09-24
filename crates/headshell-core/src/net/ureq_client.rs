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

/// Gövdesi uzun süren istekler için süreler: eser indirme ve ses akışı.
///
/// Genel süre burada işe yaramaz, çünkü gövdeyi de kapsar: 40 MB'lık bir
/// ikili ya da FLAC yavaş bir bağlantıda 30 saniyeyi rahatça geçer.
///
/// **Toplam süre yok**, bilerek: ureq 3.4'te süresi geçmiş bir son tarih
/// hata değil 1 sn'lik bir okuma süresi oluyor, yani sürekli akan bir gövdeyi
/// hiçbir toplam süre kesmiyor (ölçüldü, D-069 eki). Takılmaya karşı koruma
/// sessizlik sınırı; boyuta karşı koruma çağıranın tavanı (eser 128 MB,
/// akış 256 MB). Yavaş ama akan bir indirme kesilmez — kullanıcı için doğru
/// olan da bu.
#[derive(Debug, Clone, Copy)]
struct LongBody {
    /// Ad çözme, bağlanma, isteği gönderme ve yanıt başlıklarını bekleme
    /// sınırı: hiç cevap vermeyen bir sunucu bundan uzun takılı bırakmaz.
    handshake: Duration,
    /// Gövdede iki okuma arasındaki en uzun sessizlik.
    idle: Duration,
}

/// Eser indirme ve ses akışının ortak süreleri.
const LONG_BODY: LongBody = LongBody {
    handshake: TIMEOUT,
    idle: TIMEOUT,
};

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
    #[must_use]
    pub fn for_downloads() -> Self {
        Self::long_body(LONG_BODY)
    }

    /// Ses akışı için istemci ([`Self::open_stream`] ile).
    ///
    /// Eskiden [`Self::new`] kullanılıyordu ve 30 sn'lik genel süre gövdeyi
    /// de kapsıyordu: 30 sn'de tamamı inmeyen bir parça (uzak bir sunucudan
    /// FLAC, yavaş bir bağlantıdan herhangi bir şey) ortasında kesiliyordu.
    #[must_use]
    pub fn for_streams() -> Self {
        Self::long_body(LONG_BODY)
    }

    /// Genel süresi olmayan, aşama aşama sınırlanmış istemci.
    ///
    /// Başlık bekleme `timeout_send_request` ile sınırlanıyor,
    /// `timeout_recv_response` ile **değil**. ureq 3.4 bir aşamanın süresini
    /// sonraki aşamada da denetliyor ve o aşamanın *bittiği* andan sayıyor:
    /// `recv_response` gövdeyi de başlıkların geldiği andan itibaren
    /// sınırlıyordu. İndirme istemcisinde 30 sn'ydi ve 30 sn'de inmeyen her
    /// eser "timeout: receive response" ile kesildi (D-069 eki). Aynı kural
    /// `send_request`'i başlık beklemeye taşıyor ve orada bırakıyor: gövdenin
    /// öncülü yalnızca `recv_response`. `recv_body` ise her okumada yeniden
    /// sayıldığı için bir toplam değil, sessizlik sınırı. Kural ureq'le
    /// değişirse aşağıdaki testler düşer.
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

#[cfg(test)]
mod tests {
    //! Süreler gerçek bir yerel sunucuya karşı sınanıyor: ureq'in hangi
    //! süreyi hangi aşamada saydığı belgesinden okunamadı, ölçülerek bulundu.

    use std::io::{BufRead as _, BufReader, Read as _, Write as _};
    use std::net::{TcpListener, TcpStream};
    use std::time::{Duration, Instant};

    use super::{LongBody, UreqClient};
    use crate::plugin::artifact::ArtifactSource as _;

    const LIMITS: LongBody = LongBody {
        handshake: Duration::from_millis(500),
        idle: Duration::from_secs(2),
    };

    /// Tek bağlantılık yerel sunucu: isteğin başlıklarını okuyup bağlantıyı
    /// `serve`'e verir.
    fn serve_once(serve: impl FnOnce(TcpStream) + Send + 'static) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let mut request = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            // Başlıklar boş bir satırla biter.
            while request.read_line(&mut line).unwrap_or(0) > 2 {
                line.clear();
            }
            serve(stream);
        });
        format!("http://{addr}/eser")
    }

    fn send_headers(stream: &mut TcpStream, length: usize) {
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        stream.flush().unwrap();
    }

    /// D-069 eki: eskiden gövde başlıklardan 30 sn sonra kesiliyordu.
    #[test]
    fn a_body_slower_than_the_handshake_budget_still_arrives_whole() {
        // 10 × 150 ms: gövde, başlık bekleme süresinin üç katında iniyor.
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

    #[test]
    fn a_server_that_never_answers_does_not_hold_the_request() {
        let url = serve_once(|stream| {
            std::thread::sleep(Duration::from_secs(4));
            drop(stream);
        });
        let started = Instant::now();
        let Err(err) = UreqClient::long_body(LIMITS).open(&url) else {
            panic!("cevap vermeyen sunucu hata olmalı");
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
        assert_eq!(bytes.len(), 1024, "gelen kısım okunmuş olmalı");
        assert!(err.to_string().contains("timeout"), "{err}");
        assert!(elapsed < Duration::from_millis(2500), "{elapsed:?}");
    }
}
