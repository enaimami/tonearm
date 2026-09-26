//! A fake HTTP client for tests.
//!
//! The convention: "everything that touches the network should be behind a
//! trait so tests can use a fake". This is that sentence made real — provider
//! logic (building URLs, credentials, JSON parsing, error translation) is
//! tested without a network.
//!
//! The transport layer itself is not tested with this; for that there is an
//! integration test that connects the real client to a real socket (D-022).

use std::sync::Mutex;

use super::{HttpClient, HttpFuture, HttpRequest, HttpResponse};

/// A client that returns a canned response to requests whose URL contains
/// `pattern`.
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

    /// Returns 200 with `body` for requests whose URL contains `pattern`.
    pub(crate) fn route(mut self, pattern: &str, body: &str) -> Self {
        self.routes.push(Route {
            pattern: pattern.to_owned(),
            status: 200,
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
        });
        self
    }

    /// Returns 200 with a binary `body` and its `Content-Type` — an image
    /// (D-076).
    ///
    /// Only the cover tests call it; it is not called in a build without
    /// the features they need.
    #[allow(dead_code)]
    pub(crate) fn route_bytes(mut self, pattern: &str, content_type: &str, body: &[u8]) -> Self {
        self.routes.push(Route {
            pattern: pattern.to_owned(),
            status: 200,
            headers: vec![super::HttpHeader::new("Content-Type", content_type)],
            body: body.to_vec(),
        });
        self
    }

    /// A response with the given status code.
    pub(crate) fn route_status(mut self, pattern: &str, status: u16, body: &str) -> Self {
        self.routes.push(Route {
            pattern: pattern.to_owned(),
            status,
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
        });
        self
    }

    /// A response redirecting to `location` (the plugin engine's redirect check
    /// is tested with this, D-069).
    ///
    /// Today its only callers are the tests behind the `plugin-engine` feature;
    /// it is not called when the feature is off.
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

    /// The requests seen so far (in order).
    pub(crate) fn requests(&self) -> Vec<HttpRequest> {
        self.seen
            .lock()
            .map(|seen| seen.clone())
            .unwrap_or_default()
    }

    /// The last request — with its body.
    ///
    /// For where `last_url` is not enough: a POST's real payload is in the body,
    /// and "did the fingerprint go in the URL or in the body" can only be seen
    /// here.
    ///
    /// Today its only callers are the `identity::acoustid` tests, and that module
    /// is behind the `fingerprint` feature; when the feature is off this method is
    /// not called. It is not dead but **conditional** — writing the feature name
    /// here would tie a general test helper to a single feature.
    #[allow(dead_code)]
    pub(crate) fn last_request(&self) -> Option<HttpRequest> {
        self.requests().last().cloned()
    }

    /// The URL of the last request.
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
                // A request that matches nothing does not silently come back empty: the test
                // must see that a wrong URL was built.
                None => Err(super::network_err(
                    &request.url,
                    "no route is defined for this URL in the fake client",
                )),
            }
        })
    }
}
