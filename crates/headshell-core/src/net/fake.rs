//! Testler için sahte HTTP istemcisi.
//!
//! Konvansiyon: "ağa dokunan her şey trait arkasında olsun ki testler sahte
//! kullanabilsin". Bu, o cümlenin karşılığı — sağlayıcı mantığı (URL kurma,
//! kimlik, JSON ayrıştırma, hata çevirme) ağ olmadan sınanır.
//!
//! Taşıma katmanının kendisi bununla sınanmaz; onun için gerçek istemciyi
//! gerçek bir sokete bağlayan entegrasyon testi var (D-022).

use std::sync::Mutex;

use super::{HttpClient, HttpFuture, HttpRequest, HttpResponse};

/// URL'sinde `pattern` geçen isteğe hazır yanıt döndüren istemci.
pub(crate) struct FakeHttp {
    routes: Vec<Route>,
    seen: Mutex<Vec<HttpRequest>>,
}

pub(crate) struct Route {
    pattern: String,
    status: u16,
    headers: Vec<super::HttpHeader>,
    body: Vec<u8>,
}

impl FakeHttp {
    pub(crate) fn new() -> Self {
        Self {
            routes: Vec::new(),
            seen: Mutex::new(Vec::new()),
        }
    }

    /// URL'sinde `pattern` geçen isteğe `body` ile 200 döner.
    pub(crate) fn route(mut self, pattern: &str, body: &str) -> Self {
        self.routes.push(Route {
            pattern: pattern.to_owned(),
            status: 200,
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
        });
        self
    }

    /// Durum kodu verilen yanıt.
    pub(crate) fn route_status(mut self, pattern: &str, status: u16, body: &str) -> Self {
        self.routes.push(Route {
            pattern: pattern.to_owned(),
            status,
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
        });
        self
    }

    /// `location`'a yönlendiren yanıt (eklenti motorunun yönlendirme
    /// denetimi bununla sınanıyor, D-069).
    ///
    /// Bugün tek çağıranı `plugin-engine` feature'ının arkasındaki
    /// testler; feature kapalıyken çağrılmıyor.
    #[allow(dead_code)]
    pub(crate) fn route_redirect(mut self, pattern: &str, status: u16, location: &str) -> Self {
        self.routes.push(Route {
            pattern: pattern.to_owned(),
            status,
            headers: vec![super::HttpHeader::new("Location", location)],
            body: Vec::new(),
        });
        self
    }

    /// Şimdiye kadar görülen istekler (sırayla).
    pub(crate) fn requests(&self) -> Vec<HttpRequest> {
        self.seen
            .lock()
            .map(|seen| seen.clone())
            .unwrap_or_default()
    }

    /// Son istek — gövdesiyle birlikte.
    ///
    /// `last_url` yetmediği yer için: bir POST'un asıl yükü gövdededir ve
    /// "parmak izi URL'de mi gövdede mi gitti" ancak buradan görülür.
    ///
    /// Bugün tek çağıranı `identity::acoustid` testleri ve o modül
    /// `fingerprint` feature'ının arkasında; feature kapalıyken bu metot
    /// çağrılmıyor. Ölü değil **koşullu** — feature adını buraya yazmak,
    /// genel bir test yardımcısını tek bir özelliğe bağlardı.
    #[allow(dead_code)]
    pub(crate) fn last_request(&self) -> Option<HttpRequest> {
        self.requests().last().cloned()
    }

    /// Son isteğin URL'si.
    pub(crate) fn last_url(&self) -> String {
        self.requests()
            .last()
            .map(|req| req.url.clone())
            .unwrap_or_default()
    }
}

impl HttpClient for FakeHttp {
    fn send<'a>(&'a self, request: &'a HttpRequest) -> HttpFuture<'a> {
        Box::pin(async move {
            if let Ok(mut seen) = self.seen.lock() {
                seen.push(request.clone());
            }
            let hit = self
                .routes
                .iter()
                .find(|route| request.url.contains(&route.pattern));
            match hit {
                Some(route) => Ok(HttpResponse {
                    status: route.status,
                    headers: route.headers.clone(),
                    body: route.body.clone(),
                }),
                // Eşleşmeyen istek sessizce boş dönmez: test yanlış URL
                // kurulduğunu görmeli.
                None => Err(super::network_err(
                    &request.url,
                    "sahte istemcide bu URL için yol tanımlı değil",
                )),
            }
        })
    }
}
