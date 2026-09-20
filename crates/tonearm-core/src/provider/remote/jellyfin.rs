//! Jellyfin istemcisi (D-019).
//!
//! Subsonic'ten üç yerde ayrılıyor:
//!
//! 1. **Kimlik başlıkta**, sorgu dizesinde değil: `Authorization: MediaBrowser
//!    Token="..."`. Bu yüzden akış kaynağı `AudioSource::HttpStream`'in
//!    `headers` alanını kullanır — kaynak URL'si tek başına yetmez.
//! 2. **Hatalar HTTP durum koduyla** gelir; gövdede `status: failed` yok.
//! 3. Çoğu uç nokta **kullanıcı kimliği** ister (`/Users/{id}/Items`). Kayıt
//!    anında öğrenilir, öğrenilemezse ilk kullanımda tembel olarak sorulur.
//!
//! Parola saklanmaz (D-021): `AuthenticateByName` bir kez çağrılır ve
//! dönen erişim anahtarı yazılır.

use std::sync::{Arc, RwLock};

use serde::Deserialize;

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};
use crate::ids::{ProviderId, ProviderTrackId};
use crate::model::TrackRef;
use crate::net::{self, HttpClient, HttpHeader, HttpRequest};
use crate::provider::{
    AudioSource, Capabilities, Provider, ProviderFuture, ProviderHealth, ProviderInfo,
    ProviderTrack,
};

use super::{RemoteServer, ServerKind, StoredAuth};

/// Jellyfin'in sürüm/istemci tanıtımı. Sunucunun "aygıtlar" listesinde
/// bu adla görünürüz.
const CLIENT_NAME: &str = "tonearm";
const DEVICE_NAME: &str = "tonearm-core";

/// `RunTimeTicks` 100 nanosaniyelik birimlerde; milisaniye için bölen.
const TICKS_PER_MS: u64 = 10_000;

/// Jellyfin sağlayıcısı.
pub struct JellyfinProvider {
    server: RemoteServer,
    http: Arc<dyn HttpClient>,
    /// Kayıt anında bilinmiyorsa ilk kullanımda öğrenilir.
    user_id: RwLock<Option<String>>,
}

impl std::fmt::Debug for JellyfinProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JellyfinProvider")
            .field("id", &self.server.id)
            .field("url", &self.server.url)
            .finish()
    }
}

impl JellyfinProvider {
    #[must_use]
    pub fn new(server: RemoteServer, http: Arc<dyn HttpClient>) -> Self {
        let user_id = RwLock::new(server.user_id.clone());
        Self {
            server,
            http,
            user_id,
        }
    }

    /// Erişim anahtarı.
    fn token(&self) -> Result<&str> {
        match &self.server.auth {
            StoredAuth::ApiKey { key } => Ok(key),
            // Elle düzenlenmiş kayıt: Subsonic token'ı Jellyfin'de işe yaramaz.
            StoredAuth::SubsonicToken { .. } => Err(Error::new(
                Stage::ProviderCall,
                ErrorKind::InvalidInput {
                    detail: format!(
                        "{} kaydı Jellyfin ama Subsonic token'ı taşıyor; `tonearm provider add` ile yeniden kaydedin",
                        self.server.id
                    ),
                },
            )),
        }
    }

    /// Kimlik başlıkları.
    fn auth_headers(&self) -> Result<Vec<HttpHeader>> {
        Ok(auth_headers(Some(self.token()?)))
    }

    /// Kullanıcı kimliğini verir; bilinmiyorsa sunucuya sorar ve saklar.
    async fn user_id(&self) -> Result<String> {
        if let Ok(cached) = self.user_id.read()
            && let Some(id) = cached.as_ref()
        {
            return Ok(id.clone());
        }
        let id = fetch_user_id(&self.server, &*self.http).await?;
        if let Ok(mut slot) = self.user_id.write() {
            *slot = Some(id.clone());
        }
        Ok(id)
    }

    /// Kimlikli bir GET atar ve JSON'u çözer.
    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str, what: &str) -> Result<T> {
        let request = HttpRequest::get(url).with_headers(self.auth_headers()?);
        let response = self.http.send(&request).await?;
        response.error_for_status(url)?;
        net::parse_json(&response, what)
    }

    /// Bir parçanın akış URL'si. Kimlik **başlıkta** gider.
    #[must_use]
    pub fn stream_url(&self, item_id: &str) -> String {
        format!(
            "{}/Audio/{}/stream?static=true",
            self.server.url,
            net::encode_query(item_id)
        )
    }
}

impl Provider for JellyfinProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: self.server.id.clone(),
            display_name: self.server.display_name(),
            capabilities: Capabilities::SEARCH | Capabilities::BROWSE | Capabilities::STREAM,
        }
    }

    fn health<'a>(&'a self) -> ProviderFuture<'a, ProviderHealth> {
        Box::pin(async move {
            let unreachable = |detail: String| ProviderHealth {
                id: self.server.id.clone(),
                reachable: false,
                track_count: None,
                detail: Some(detail),
            };

            // Kimliksiz uç: sunucu ayakta mı?
            let public_url = format!("{}/System/Info/Public", self.server.url);
            let public: PublicInfo = match self.get_json(&public_url, "jellyfin public info").await
            {
                Ok(info) => info,
                Err(err) => return Ok(unreachable(err.chain_text().replace('\n', " "))),
            };
            let mut detail = format!(
                "{} {}",
                public.server_name.unwrap_or_else(|| "Jellyfin".to_owned()),
                public
                    .version
                    .unwrap_or_else(|| "(sürüm bildirmedi)".to_owned())
            );

            // Kimlikli uç: anahtar hâlâ geçerli mi? Ayakta ama reddeden bir
            // sunucu "erişilebilir" değildir — kullanıcının yapacağı iş farklı.
            let user_id = match self.user_id().await {
                Ok(id) => id,
                Err(err) => {
                    return Ok(unreachable(format!(
                        "{detail} — kimlik doğrulanamadı: {}",
                        err.chain_text().replace('\n', " ")
                    )));
                }
            };

            let count_url = format!(
                "{}/Users/{}/Items?IncludeItemTypes=Audio&Recursive=true&Limit=0",
                self.server.url,
                net::encode_query(&user_id)
            );
            let track_count = match self
                .get_json::<ItemsPage>(&count_url, "jellyfin count")
                .await
            {
                Ok(page) => page.total_record_count.map(|n| n as usize),
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
                track_count,
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
            let user_id = self.user_id().await?;
            let url = format!(
                "{}/Users/{}/Items?searchTerm={}&IncludeItemTypes=Audio&Recursive=true&Limit={}&Fields=RunTimeTicks",
                self.server.url,
                net::encode_query(&user_id),
                net::encode_query(query),
                limit
            );
            let page: ItemsPage = self.get_json(&url, "jellyfin search").await?;

            let items = match page.items {
                Some(items) => items,
                // Alan hiç yoksa sonuç yok demektir; sessiz bir kayıp değil.
                None => return Ok(Vec::new()),
            };

            let mut hits = Vec::with_capacity(items.len());
            let mut skipped = 0usize;
            for item in items {
                match item.into_track(&self.server.id) {
                    Some(track) => hits.push(track),
                    None => skipped += 1,
                }
            }
            if skipped > 0 {
                tracing::warn!(
                    provider = %self.server.id,
                    skipped,
                    "adı ya da kimliği olmayan öğeler atlandı"
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
            Ok(Some(AudioSource::HttpStream {
                url: self.stream_url(&id.id),
                // Kimlik URL'de değil başlıkta: akış adresi log'a ya da
                // ekrana düşerse anahtar sızmasın.
                headers: self.auth_headers()?,
            }))
        })
    }
}

/// `Authorization: MediaBrowser ...` başlığı.
///
/// Token yoksa (giriş isteği) alan atlanır — Jellyfin bunu böyle bekler.
fn auth_headers(token: Option<&str>) -> Vec<HttpHeader> {
    let version = env!("CARGO_PKG_VERSION");
    let mut value = format!(
        "MediaBrowser Client=\"{CLIENT_NAME}\", Device=\"{DEVICE_NAME}\", DeviceId=\"{CLIENT_NAME}\", Version=\"{version}\""
    );
    if let Some(token) = token {
        value.push_str(&format!(", Token=\"{token}\""));
    }
    vec![HttpHeader::new("Authorization", value)]
}

/// Kullanıcı adı + parolayı erişim anahtarına çevirir (D-021).
///
/// # Errors
/// Sunucuya ulaşılamazsa, kimlik reddedilirse ya da yanıt anahtar
/// içermiyorsa.
pub async fn authenticate(
    url: &str,
    username: &str,
    password: &str,
    id: ProviderId,
    http: &dyn HttpClient,
) -> Result<RemoteServer> {
    let endpoint = format!("{url}/Users/AuthenticateByName");
    let body = serde_json::to_vec(&serde_json::json!({
        "Username": username,
        "Pw": password,
    }))
    .map_err(|source| {
        Error::new(
            Stage::ProviderCall,
            ErrorKind::Json {
                entry: "jellyfin auth isteği".to_owned(),
                source,
            },
        )
    })?;

    let request = HttpRequest::post_json(&endpoint, body).with_headers(auth_headers(None));
    let response = http.send(&request).await?;
    response.error_for_status(&endpoint)?;

    let auth: AuthResult = net::parse_json(&response, "jellyfin auth yanıtı")?;
    let key = auth.access_token.ok_or_else(|| {
        Error::new(
            Stage::ProviderCall,
            ErrorKind::NotFound {
                what: "Jellyfin yanıtında AccessToken".to_owned(),
            },
        )
    })?;

    Ok(RemoteServer {
        id,
        kind: ServerKind::Jellyfin,
        url: url.to_owned(),
        username: username.to_owned(),
        auth: StoredAuth::ApiKey { key },
        user_id: auth.user.and_then(|user| user.id),
    })
}

/// Anahtarın sahibi olan kullanıcının kimliğini sorar.
///
/// # Errors
/// Anahtar geçersizse ya da sunucuya ulaşılamazsa.
pub async fn fetch_user_id(server: &RemoteServer, http: &dyn HttpClient) -> Result<String> {
    let token = match &server.auth {
        StoredAuth::ApiKey { key } => key.as_str(),
        StoredAuth::SubsonicToken { .. } => {
            return Err(Error::new(
                Stage::ProviderCall,
                ErrorKind::InvalidInput {
                    detail: format!("{} Jellyfin kaydı değil", server.id),
                },
            ));
        }
    };
    let url = format!("{}/Users/Me", server.url);
    let request = HttpRequest::get(&url).with_headers(auth_headers(Some(token)));
    let response = http.send(&request).await?;
    response.error_for_status(&url)?;

    let me: UserInfo = net::parse_json(&response, "jellyfin /Users/Me")?;
    me.id.ok_or_else(|| {
        Error::new(
            Stage::ProviderCall,
            ErrorKind::NotFound {
                what: "Jellyfin yanıtında kullanıcı kimliği (Id)".to_owned(),
            },
        )
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PublicInfo {
    server_name: Option<String>,
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AuthResult {
    access_token: Option<String>,
    user: Option<UserInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct UserInfo {
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ItemsPage {
    items: Option<Vec<Item>>,
    total_record_count: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Item {
    id: Option<String>,
    name: Option<String>,
    album: Option<String>,
    album_artist: Option<String>,
    #[serde(default)]
    artists: Vec<String>,
    run_time_ticks: Option<u64>,
}

impl Item {
    fn into_track(self, provider: &ProviderId) -> Option<ProviderTrack> {
        let id = self.id?;
        let title = self.name?;
        let artist = self
            .album_artist
            .or_else(|| self.artists.first().cloned())
            .unwrap_or_else(|| "Bilinmeyen sanatçı".to_owned());

        Some(ProviderTrack {
            id: ProviderTrackId::new(provider.clone(), id),
            track: TrackRef::new(artist, title)
                .with_album(self.album)
                .with_duration_ms(self.run_time_ticks.map(|ticks| ticks / TICKS_PER_MS)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::fake::FakeHttp;

    fn server() -> RemoteServer {
        RemoteServer {
            id: ProviderId::new("jf"),
            kind: ServerKind::Jellyfin,
            url: "https://jf.ev".to_owned(),
            username: "enai".to_owned(),
            auth: StoredAuth::ApiKey {
                key: "gizli-anahtar".to_owned(),
            },
            user_id: Some("u1".to_owned()),
        }
    }

    const SEARCH_JSON: &str = r#"{"Items":[
        {"Id":"i1","Name":"Geceler","Album":"Müptezhel","AlbumArtist":"Ezhel",
         "Artists":["Ezhel"],"RunTimeTicks":2150000000},
        {"Id":"i2","Name":"Felaket","Artists":["Ezhel"]},
        {"Name":"kimliksiz"}],"TotalRecordCount":3}"#;

    #[tokio::test]
    async fn search_maps_items_and_converts_ticks_to_ms() {
        let http = Arc::new(FakeHttp::new().route("/Users/u1/Items", SEARCH_JSON));
        let provider = JellyfinProvider::new(server(), Arc::clone(&http) as Arc<dyn HttpClient>);

        let hits = provider.search("Ezhel", 5).await.unwrap();
        assert_eq!(hits.len(), 2, "kimliksiz öğe atlanmalı");
        assert_eq!(hits[0].track.title, "Geceler");
        assert_eq!(hits[0].track.artist, "Ezhel");
        assert_eq!(hits[0].track.duration_ms, Some(215_000));
        assert_eq!(
            hits[1].track.artist, "Ezhel",
            "AlbumArtist yoksa Artists[0]"
        );

        let url = http.last_url();
        assert!(url.contains("searchTerm=Ezhel"), "{url}");
        assert!(url.contains("IncludeItemTypes=Audio"), "{url}");
        assert!(url.contains("Limit=5"), "{url}");
    }

    #[tokio::test]
    async fn credentials_travel_in_the_header_not_the_url() {
        let http = Arc::new(FakeHttp::new().route("/Users/u1/Items", SEARCH_JSON));
        let provider = JellyfinProvider::new(server(), Arc::clone(&http) as Arc<dyn HttpClient>);
        provider.search("Ezhel", 5).await.unwrap();

        let request = http.requests().pop().unwrap();
        assert!(
            !request.url.contains("gizli-anahtar"),
            "anahtar URL'de olmamalı: {}",
            request.url
        );
        let auth = request
            .headers
            .iter()
            .find(|h| h.name == "Authorization")
            .expect("Authorization başlığı");
        assert!(auth.value.contains("Token=\"gizli-anahtar\""), "{auth:?}");
        assert!(auth.value.contains("Client=\"tonearm\""), "{auth:?}");
    }

    #[tokio::test]
    async fn resolve_source_carries_the_auth_header() {
        let provider = JellyfinProvider::new(server(), Arc::new(FakeHttp::new()));
        let id = ProviderTrackId::new(ProviderId::new("jf"), "i1");
        let source = provider.resolve_source(&id).await.unwrap().unwrap();
        match source {
            AudioSource::HttpStream { url, headers } => {
                assert_eq!(url, "https://jf.ev/Audio/i1/stream?static=true");
                assert_eq!(headers.len(), 1);
                assert!(headers[0].value.contains("Token="), "{headers:?}");
            }
            other => panic!("HTTP akışı bekleniyordu: {other:?}"),
        }
    }

    #[tokio::test]
    async fn an_http_error_is_reported_with_its_status_and_body() {
        let http = Arc::new(FakeHttp::new().route_status(
            "/Users/u1/Items",
            401,
            "Access token is invalid or expired.",
        ));
        let provider = JellyfinProvider::new(server(), http);

        let err = provider.search("Ezhel", 5).await.unwrap_err();
        let text = err.chain_text();
        // D-023: sunucu ayaktaydı ve **reddetti**; bu bir ağ hatası değil.
        // Kullanıcının yapacağı iş anahtarını yenilemek, ağını kurcalamak değil.
        assert!(text.starts_with("ADIM: PROVIDER_CALL"), "{text}");
        assert!(text.contains("401"), "{text}");
        assert!(text.contains("expired"), "{text}");
    }

    #[tokio::test]
    async fn the_user_id_is_learned_once_when_missing() {
        let mut server = server();
        server.user_id = None;
        let http = Arc::new(
            FakeHttp::new()
                .route("/Users/Me", r#"{"Id":"u9","Name":"enai"}"#)
                .route("/Users/u9/Items", SEARCH_JSON),
        );
        let provider = JellyfinProvider::new(server, Arc::clone(&http) as Arc<dyn HttpClient>);

        provider.search("Ezhel", 5).await.unwrap();
        provider.search("Ezhel", 5).await.unwrap();

        let me_calls = http
            .requests()
            .iter()
            .filter(|req| req.url.ends_with("/Users/Me"))
            .count();
        assert_eq!(me_calls, 1, "kullanıcı kimliği her aramada sorulmamalı");
    }

    #[tokio::test]
    async fn authentication_stores_a_token_not_the_password() {
        let http = FakeHttp::new().route(
            "/Users/AuthenticateByName",
            r#"{"AccessToken":"tok123","User":{"Id":"u1","Name":"enai"}}"#,
        );
        let stored = authenticate(
            "https://jf.ev",
            "enai",
            "sesame",
            ProviderId::new("jf"),
            &http,
        )
        .await
        .unwrap();

        assert_eq!(stored.user_id.as_deref(), Some("u1"));
        match &stored.auth {
            StoredAuth::ApiKey { key } => assert_eq!(key, "tok123"),
            other => panic!("anahtar bekleniyordu: {other:?}"),
        }
        // Parola yalnızca istekte gitti, kayda girmedi.
        let stored_json = serde_json::to_string(&stored).unwrap();
        assert!(!stored_json.contains("sesame"), "{stored_json}");
    }
}
