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
            body: body.as_bytes().to_vec(),
        });
        self
    }

    /// Durum kodu verilen yanıt.
    pub(crate) fn route_status(mut self, pattern: &str, status: u16, body: &str) -> Self {
        self.routes.push(Route {
            pattern: pattern.to_owned(),
            status,
            body: body.as_bytes().to_vec(),
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
                    headers: Vec::new(),
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
