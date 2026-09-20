//! Subsonic / OpenSubsonic istemcisi (D-019).
//!
//! Navidrome, Airsonic, Gonic, LMS ve Subsonic eklentili Jellyfin bu API'yi
//! konuşur. Kimlik doğrulama Subsonic'in kendi salt/token yolu (D-021):
//! her istek `u`, `t`, `s` taşır; parola tel üzerinden hiç geçmez.
//!
//! **Subsonic hataları HTTP 200 ile gelir** — gövdedeki `status: "failed"`
//! okunmazsa "her şey yolunda" sanılır. Bu yüzden zarf her yanıtta denetlenir
//! ve hata [`crate::ErrorKind::RemoteApi`] olur (taşıma hatasından ayrı).

use std::sync::Arc;

use serde::Deserialize;

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};
use crate::ids::ProviderTrackId;
use crate::model::TrackRef;
use crate::net::{self, HttpClient, HttpRequest};
use crate::provider::{
    AudioSource, Capabilities, Provider, ProviderFuture, ProviderHealth, ProviderInfo,
    ProviderTrack,
};

use super::{RemoteServer, StoredAuth};

/// Konuştuğumuz protokol sürümü. 1.16.1 = Subsonic 6.1; `search3` ve
/// `getScanStatus` bu sürümde var.
const API_VERSION: &str = "1.16.1";

/// Sunucuların log'unda göreceği istemci adı.
const CLIENT_NAME: &str = "tonearm";

/// Subsonic sağlayıcısı.
pub struct SubsonicProvider {
    server: RemoteServer,
    http: Arc<dyn HttpClient>,
}

impl std::fmt::Debug for SubsonicProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubsonicProvider")
            .field("id", &self.server.id)
            .field("url", &self.server.url)
            .finish()
    }
}

impl SubsonicProvider {
    #[must_use]
    pub fn new(server: RemoteServer, http: Arc<dyn HttpClient>) -> Self {
        Self { server, http }
    }

    /// Kimlik parametreleriyle birlikte bir uç nokta URL'si kurar.
    ///
    /// Kimlik sorgu dizesinde: Subsonic'in tanımladığı yol bu. Bu yüzden
    /// **`http://` üzerinden kullanmak token'ı ağa açar** — `normalize_url`
    /// şema tahmininde bulunmamasının sebebi de bu.
    pub(crate) fn endpoint(&self, name: &str, params: &[(&str, &str)]) -> String {
        let mut url = format!(
            "{}/rest/{}?u={}&v={API_VERSION}&c={CLIENT_NAME}&f=json",
            self.server.url,
            name,
            net::encode_query(&self.server.username),
        );
        match &self.server.auth {
            StoredAuth::SubsonicToken { salt, token } => {
                url.push_str(&format!(
                    "&t={}&s={}",
                    net::encode_query(token),
                    net::encode_query(salt)
                ));
            }
            // Elle düzenlenmiş bir kayıtta olabilir: OpenSubsonic'in
            // `apiKey` yolu. Sessizce kimliksiz istek atmaktan iyidir.
            StoredAuth::ApiKey { key } => {
                url.push_str(&format!("&apiKey={}", net::encode_query(key)));
            }
        }
        for (key, value) in params {
            url.push_str(&format!("&{key}={}", net::encode_query(value)));
        }
        url
    }

    /// Uç noktayı çağırır ve zarfı doğrular.
    async fn call(&self, name: &str, params: &[(&str, &str)]) -> Result<SubsonicBody> {
        let url = self.endpoint(name, params);
        let request = HttpRequest::get(&url);
        let response = self.http.send(&request).await?;
        response.error_for_status(&url)?;

        let envelope: Envelope = net::parse_json(&response, &format!("subsonic {name}"))?;
        let body = envelope.response;
        if body.status.as_deref() == Some("failed") {
            let error = body.error.unwrap_or(ApiError {
                code: -1,
                message: None,
            });
            return Err(Error::new(
                Stage::ProviderCall,
                ErrorKind::RemoteApi {
                    server: self.server.id.to_string(),
                    endpoint: name.to_owned(),
                    code: error.code,
                    message: error
                        .message
                        .unwrap_or_else(|| "sunucu bir mesaj vermedi".to_owned()),
                },
            ));
        }
        Ok(body)
    }

    /// Bir parçanın akış URL'si.
    #[must_use]
    pub fn stream_url(&self, song_id: &str) -> String {
        self.endpoint("stream", &[("id", song_id)])
    }
}

impl Provider for SubsonicProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: self.server.id.clone(),
            display_name: self.server.display_name(),
            // CONTROL yok: Subsonic uzaktaki bir oynatıcıyı kumanda etmez,
            // ses baytlarını bize verir.
            capabilities: Capabilities::SEARCH | Capabilities::BROWSE | Capabilities::STREAM,
        }
    }

    fn health<'a>(&'a self) -> ProviderFuture<'a, ProviderHealth> {
        Box::pin(async move {
            let ping = match self.call("ping", &[]).await {
                Ok(body) => body,
                Err(err) => {
                    // Ulaşılamamak bir sağlık **cevabıdır**, komutun hatası
                    // değil: `provider test` çıktısı sebebi göstermeli.
                    return Ok(ProviderHealth {
                        id: self.server.id.clone(),
                        reachable: false,
                        track_count: None,
                        detail: Some(err.chain_text().replace('\n', " ")),
                    });
                }
            };

            let mut detail = match (&ping.server_type, &ping.server_version, &ping.version) {
                (Some(kind), Some(version), _) => format!("{kind} {version}"),
                (_, _, Some(api)) => format!("Subsonic API {api}"),
                _ => "Subsonic".to_owned(),
            };

            // Parça sayısı isteğe bağlı bir uç noktadan geliyor; yoksa
            // "bilmiyorum" (None) diyoruz, sıfır değil.
            let track_count = match self.call("getScanStatus", &[]).await {
                Ok(body) => body.scan_status.and_then(|status| status.count),
                Err(err) => {
                    detail.push_str(&format!(
                        " — parça sayısı okunamadı ({})",
                        err.chain_text().replace('\n', " ")
                    ));
                    None
                }
            };

            Ok(ProviderHealth {
                id: self.server.id.clone(),
                reachable: true,
                track_count: track_count.map(|count| count as usize),
                detail: Some(detail),
            })
        })
    }

    fn search<'a>(
        &'a self,
        query: &'a str,
        limit: usize,
    ) -> ProviderFuture<'a, Vec<ProviderTrack>> {
        Box::pin(async move {
            if query.trim().is_empty() {
                return Ok(Vec::new());
            }
            let limit_text = limit.to_string();
            let body = self
                .call(
                    "search3",
                    &[
                        ("query", query),
                        ("songCount", &limit_text),
                        ("artistCount", "0"),
                        ("albumCount", "0"),
                    ],
                )
                .await?;

            let songs = body
                .search_result3
                .map(|result| result.song)
                .unwrap_or_default();

            let mut hits = Vec::with_capacity(songs.len());
            let mut skipped = 0usize;
            for song in songs {
                match song.into_track(&self.server) {
                    Some(track) => hits.push(track),
                    None => skipped += 1,
                }
            }
            if skipped > 0 {
                // Sessizce yutmuyoruz: sayı log'a düşüyor (K9).
                tracing::warn!(
                    provider = %self.server.id,
                    skipped,
                    "başlığı ya da kimliği olmayan parçalar atlandı"
                );
            }
            Ok(hits)
        })
    }

    fn resolve_source<'a>(
        &'a self,
        id: &'a ProviderTrackId,
    ) -> ProviderFuture<'a, Option<AudioSource>> {
        Box::pin(async move {
            if id.provider != self.server.id {
                return Err(Error::new(
                    Stage::PlaybackResolve,
                    ErrorKind::InvalidInput {
                        detail: format!("{id} bu sağlayıcıya ait değil ({})", self.server.id),
                    },
                ));
            }
            // Varlık kontrolü için ayrı bir `getSong` çağrısı atmıyoruz:
            // olmayan bir kimlik akış açılırken net bir HTTP hatası verir
            // (ADIM: NETWORK_REQUEST) ve her çalmadan önce bir tur ağ
            // gecikmesi eklemeye değmez.
            Ok(Some(AudioSource::HttpStream {
                url: self.stream_url(&id.id),
                // Kimlik sorgu dizesinde; ek başlık gerekmiyor.
                headers: Vec::new(),
            }))
        })
    }

    // `scan_catalog` uygulanmadı (varsayılan `None`): Subsonic'te bütün
    // kataloğu ucuza döken bir uç nokta yok — sanatçı → albüm → parça diye
    // yüzlerce istek gerekirdi. Uzak arama sunucuya doğrudan gidiyor.
}

/// `{"subsonic-response": {...}}` zarfı.
#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(rename = "subsonic-response")]
    response: SubsonicBody,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubsonicBody {
    status: Option<String>,
    version: Option<String>,
    /// OpenSubsonic: `"navidrome"`, `"gonic"`…
    #[serde(rename = "type")]
    server_type: Option<String>,
    server_version: Option<String>,
    error: Option<ApiError>,
    search_result3: Option<SearchResult3>,
    scan_status: Option<ScanStatus>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    code: i64,
    message: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct SearchResult3 {
    #[serde(default)]
    song: Vec<Song>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScanStatus {
    count: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Song {
    id: Option<String>,
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    /// Saniye cinsinden (Subsonic böyle veriyor).
    duration: Option<u64>,
    /// OpenSubsonic eklentisi; varsa kimlik zincirinin ilk halkası (K6).
    isrc: Option<Vec<String>>,
}

impl Song {
    /// Kimliği ya da başlığı olmayan satır `None` döner — çağıran sayar.
    fn into_track(self, server: &RemoteServer) -> Option<ProviderTrack> {
        let id = self.id?;
        let title = self.title?;
        let isrc = self
            .isrc
            .as_ref()
            .and_then(|list| list.first())
            .and_then(|raw| crate::ids::Isrc::parse(raw));

        Some(ProviderTrack {
            id: ProviderTrackId::new(server.id.clone(), id),
            track: TrackRef::new(
                self.artist
                    .unwrap_or_else(|| "Bilinmeyen sanatçı".to_owned()),
                title,
            )
            .with_album(self.album)
            .with_duration_ms(self.duration.map(|secs| secs.saturating_mul(1000)))
            .with_isrc(isrc),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ProviderId;
    use crate::net::fake::FakeHttp;

    fn server() -> RemoteServer {
        RemoteServer {
            id: ProviderId::new("ev"),
            kind: super::super::ServerKind::Subsonic,
            url: "https://muzik.ev".to_owned(),
            username: "enai".to_owned(),
            auth: StoredAuth::SubsonicToken {
                salt: "c19b2d".to_owned(),
                token: "26719a1196d2a940705a59634eb18eab".to_owned(),
            },
            user_id: None,
        }
    }

    const SEARCH_JSON: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1",
        "type":"navidrome","serverVersion":"0.53.3","searchResult3":{"song":[
        {"id":"a1","title":"Geceler","artist":"Ezhel","album":"Müptezhel","duration":215,
         "isrc":["TRA123456789"]},
        {"id":"a2","title":"Felaket","artist":"Ezhel","duration":180},
        {"title":"kimliksiz"}]}}}"#;

    #[test]
    fn the_url_carries_credentials_and_encodes_the_query() {
        let provider = SubsonicProvider::new(server(), Arc::new(FakeHttp::new()));
        let url = provider.endpoint("search3", &[("query", "Ezhel Geceler")]);
        assert!(url.starts_with("https://muzik.ev/rest/search3?"), "{url}");
        assert!(url.contains("u=enai"), "{url}");
        assert!(url.contains("&t=26719a1196d2a940705a59634eb18eab"), "{url}");
        assert!(url.contains("&s=c19b2d"), "{url}");
        assert!(url.contains("&f=json"), "{url}");
        assert!(url.contains("query=Ezhel%20Geceler"), "{url}");
        // Parola hiçbir yerde geçmemeli.
        assert!(!url.contains("p="), "{url}");
    }

    #[tokio::test]
    async fn search_maps_songs_and_skips_broken_rows() {
        let http = Arc::new(FakeHttp::new().route("/rest/search3", SEARCH_JSON));
        let provider = SubsonicProvider::new(server(), Arc::clone(&http) as Arc<dyn HttpClient>);

        let hits = provider.search("Ezhel", 10).await.unwrap();
        assert_eq!(hits.len(), 2, "başlıksız satır atlanmalı");
        assert_eq!(hits[0].track.title, "Geceler");
        assert_eq!(hits[0].track.artist, "Ezhel");
        assert_eq!(hits[0].track.album.as_deref(), Some("Müptezhel"));
        assert_eq!(hits[0].track.duration_ms, Some(215_000));
        assert_eq!(
            hits[0].track.isrc.as_ref().map(|i| i.as_str()),
            Some("TRA123456789"),
            "ISRC varsa kimlik zincirinin ilk halkası (K6)"
        );
        assert_eq!(hits[0].id.provider.as_str(), "ev");
        assert_eq!(hits[0].id.id, "a1");
        assert!(
            http.last_url().contains("songCount=10"),
            "{}",
            http.last_url()
        );
    }

    #[tokio::test]
    async fn a_failed_envelope_becomes_a_remote_api_error_not_success() {
        let body = r#"{"subsonic-response":{"status":"failed","version":"1.16.1",
            "error":{"code":40,"message":"Wrong username or password"}}}"#;
        let http = Arc::new(FakeHttp::new().route("/rest/", body));
        let provider = SubsonicProvider::new(server(), http);

        let err = provider.search("Ezhel", 10).await.unwrap_err();
        let text = err.chain_text();
        assert!(text.starts_with("ADIM: PROVIDER_CALL"), "{text}");
        assert!(text.contains("Wrong username or password"), "{text}");
        assert!(text.contains("40"), "{text}");
    }

    #[tokio::test]
    async fn health_reports_the_server_build_and_track_count() {
        let ping = r#"{"subsonic-response":{"status":"ok","version":"1.16.1",
            "type":"navidrome","serverVersion":"0.53.3"}}"#;
        let scan = r#"{"subsonic-response":{"status":"ok","version":"1.16.1",
            "scanStatus":{"scanning":false,"count":8123}}}"#;
        let http = Arc::new(
            FakeHttp::new()
                .route("/rest/getScanStatus", scan)
                .route("/rest/ping", ping),
        );
        let provider = SubsonicProvider::new(server(), http);

        let health = provider.health().await.unwrap();
        assert!(health.reachable);
        assert_eq!(health.track_count, Some(8123));
        assert!(
            health
                .detail
                .as_deref()
                .unwrap_or_default()
                .contains("navidrome"),
            "{health:?}"
        );
    }

    #[tokio::test]
    async fn an_unreachable_server_is_a_health_answer_not_a_command_failure() {
        // Sahte istemcide hiç yol yok: her istek taşıma hatası.
        let http = Arc::new(FakeHttp::new());
        let provider = SubsonicProvider::new(server(), http);

        let health = provider.health().await.unwrap();
        assert!(!health.reachable);
        assert!(health.detail.is_some(), "sebep gösterilmeli");
        assert_eq!(health.track_count, None, "bilinmiyor sıfır değildir");
    }

    #[tokio::test]
    async fn resolve_source_builds_a_stream_url_for_this_server_only() {
        let http = Arc::new(FakeHttp::new());
        let provider = SubsonicProvider::new(server(), http);

        let id = ProviderTrackId::new(ProviderId::new("ev"), "a1");
        let source = provider.resolve_source(&id).await.unwrap().unwrap();
        match source {
            AudioSource::HttpStream { url, headers } => {
                assert!(url.contains("/rest/stream?"), "{url}");
                assert!(url.contains("id=a1"), "{url}");
                assert!(headers.is_empty());
            }
            other => panic!("HTTP akışı bekleniyordu: {other:?}"),
        }

        let foreign = ProviderTrackId::new(ProviderId::new("baska"), "a1");
        assert!(
            provider.resolve_source(&foreign).await.is_err(),
            "başka sağlayıcının kimliği sessizce kabul edilmemeli"
        );
    }
}
