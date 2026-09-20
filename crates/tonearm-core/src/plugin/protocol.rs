//! Eklenti protokolünün **tel biçimi**: JSON-RPC 2.0, satır bazlı.
//!
//! Çerçeveleme: her mesaj tek satır JSON + `\n`. Uzunluk başlığı yok —
//! eklenti herhangi bir dilde yazılabilmeli (K5) ve `print(json.dumps(x))`
//! her dilde tek satırdır. Bunun bedeli mesaj içinde çıplak `\n`
//! olamamasıdır; `serde_json` zaten kaçırıyor.
//!
//! ## Sürümleme
//!
//! [`PLUGIN_API`] tek bir tam sayı. El sıkışmada iki taraf da kendi
//! sürümünü söyler; **eşit değilse eklenti yüklenmez** ve kullanıcı iki
//! sayıyı da görür ([`crate::ErrorKind::PluginIncompatible`]).
//!
//! Kural, tema token'larının kuralıyla aynı (D-039): **eklemek sürümü
//! artırmaz, kaldırmak/anlamını değiştirmek artırır.** Yeni bir metot
//! eklendiğinde eski eklentiler onu bilmez, `-32601 MethodNotFound` döner ve
//! çekirdek "bu yeteneği desteklemiyor" diye okur — çökmez.
//!
//! ## api 1'in metotları
//!
//! | Metot | Zorunlu | Karşılığı |
//! |---|---|---|
//! | `handshake` | evet | sürüm + kimlik + yetenekler |
//! | `health` | evet | [`crate::provider::Provider::health`] |
//! | `search` | `SEARCH` bayrağı varsa | [`crate::provider::Provider::search`] |
//! | `resolve_source` | `STREAM` bayrağı varsa | [`crate::provider::Provider::resolve_source`] |
//! | `shutdown` | evet (bildirim) | süreci düzgün kapatma |
//!
//! `scan_catalog` ve `catalog_changed_since` **bilerek yok**: ikisi de
//! trait'te varsayılanı olan opsiyonel metotlar ve ilk referans eklentinin
//! (SoundCloud, D-041) taranacak yerel bir kataloğu yok. Tel biçimini
//! kullanmayan bir metotla açmak, ilk kullanıcısı çıktığında yanlış çıktığını
//! öğreneceğimiz bir tahmindir. Eklenmeleri `api`'yi artırmayacak.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::ids::{Isrc, ProviderId, ProviderTrackId};
use crate::model::TrackRef;
use crate::provider::{AudioSource, Capabilities};

/// Çekirdeğin konuştuğu protokol sürümü.
pub const PLUGIN_API: u32 = 1;

/// JSON-RPC'nin "metot yok" kodu. Opsiyonel metotların cevabı budur.
pub const CODE_METHOD_NOT_FOUND: i64 = -32601;

/// api 1'in metot adları. Tanımlayıcılar İngilizce (D-036).
pub mod method {
    pub const HANDSHAKE: &str = "handshake";
    pub const HEALTH: &str = "health";
    pub const SEARCH: &str = "search";
    pub const RESOLVE_SOURCE: &str = "resolve_source";
    pub const SHUTDOWN: &str = "shutdown";
}

/// Çekirdekten eklentiye giden istek.
#[derive(Debug, Clone, Serialize)]
pub struct Request {
    pub jsonrpc: &'static str,
    /// Bildirimlerde (`shutdown`) yok.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    pub method: String,
    pub params: serde_json::Value,
}

impl Request {
    /// Cevap beklenen istek.
    #[must_use]
    pub fn call(id: u64, method: &str, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id: Some(id),
            method: method.to_owned(),
            params,
        }
    }

    /// Cevap beklenmeyen bildirim.
    #[must_use]
    pub fn notify(method: &str, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id: None,
            method: method.to_owned(),
            params,
        }
    }
}

/// Eklentiden gelen satır: ya bir cevap ya da bir bildirim (`log`).
#[derive(Debug, Clone, Deserialize)]
pub struct Incoming {
    /// Cevaplarda dolu, bildirimlerde yok.
    #[serde(default)]
    pub id: Option<u64>,
    /// Bildirimlerde dolu.
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<RpcError>,
}

/// JSON-RPC hata nesnesi: süreç sağ, işi reddetti.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// `handshake` isteğinin gövdesi.
///
/// Eklentinin gördüğü **tek** dış dünya budur: kendi veri dizini, kendi
/// sırları (D-042) ve kullanıcının onayladığı izinler (D-040).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandshakeParams {
    pub api: u32,
    pub host: HostInfo,
    /// Eklentinin yazabileceği dizin: `<data_dir>/plugins/<ad>/state`.
    pub data_dir: String,
    /// **Yalnızca bu eklentinin** ad alanındaki sırlar.
    pub secrets: BTreeMap<String, String>,
    /// Kullanıcının onayladığı izinler — beyan edilenle aynı küme (D-040).
    pub permissions: super::manifest::Permissions,
    /// Motorun kurduğu eserlerin `ad → yol` haritası (D-055).
    ///
    /// Yalnızca **hazır** eserler burada: yolu olan bir eser kurulu ve
    /// karması doğrulanmış demektir. Eksik bir eser haritada hiç görünmez,
    /// boş dize olarak değil — eklenti "var mı?" diye bakarken boş bir yolu
    /// kazara çalıştırmaya kalkmasın.
    ///
    /// **`api`'yi kırmaz:** alan eklemek protokol sürümünü artırmaz (§2.1).
    /// Bu alanı okumayan eski bir eklenti bugüne kadar olduğu gibi çalışır.
    #[serde(default)]
    pub requirements: std::collections::BTreeMap<String, String>,
}

/// Çekirdeğin kendini tanıtması. Eklenti buna bakıp davranış değiştirebilir.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostInfo {
    pub name: String,
    pub version: String,
}

impl HostInfo {
    #[must_use]
    pub fn current() -> Self {
        Self {
            name: "tonearm".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        }
    }
}

/// `handshake` cevabı.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandshakeResult {
    pub api: u32,
    /// Eklentinin kendi adı. Manifestteki adla uyuşmalı.
    pub name: String,
    pub display_name: String,
    #[serde(default)]
    pub plugin_version: Option<String>,
    /// Yetenek adları: `search`, `browse`, `stream`, `control`.
    ///
    /// Bit maskesi değil ad listesi: eklenti yazarı `1 << 2`'yi bilmek
    /// zorunda kalmasın ve tanımadığımız bir ad geldiğinde onu **söyleyebilelim**.
    #[serde(default)]
    pub capabilities: Vec<String>,
}

/// Yetenek adlarını bit maskesine çevirir.
///
/// Dönüş: `(maske, tanınmayan adlar)`. Tanınmayanı yutmuyoruz — çağıran
/// bunu bir not olarak raporlar (K9). Boş liste hata değil: yeteneksiz bir
/// eklenti (yalnızca `health`) geçerlidir.
#[must_use]
pub fn parse_capabilities(names: &[String]) -> (Capabilities, Vec<String>) {
    let mut caps = Capabilities::NONE;
    let mut unknown = Vec::new();
    for name in names {
        match name.to_ascii_lowercase().as_str() {
            "search" => caps = caps | Capabilities::SEARCH,
            "browse" => caps = caps | Capabilities::BROWSE,
            "stream" => caps = caps | Capabilities::STREAM,
            "control" => caps = caps | Capabilities::CONTROL,
            _ => unknown.push(name.clone()),
        }
    }
    (caps, unknown)
}

/// `health` cevabı.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResult {
    pub reachable: bool,
    #[serde(default)]
    pub track_count: Option<usize>,
    #[serde(default)]
    pub detail: Option<String>,
}

/// `search` isteğinin gövdesi.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchParams {
    pub query: String,
    pub limit: usize,
}

/// `search` cevabı.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResult {
    #[serde(default)]
    pub tracks: Vec<WireTrack>,
}

/// Tel üstündeki parça.
///
/// [`crate::provider::ProviderTrack`]'in serde gösterimi değil, kasten daha
/// yalın bir biçim: `id` çıplak bir dize, sağlayıcı adı **yok**. Sağlayıcı
/// adını çekirdek ekler — eklenti başka bir sağlayıcının ad alanında kimlik
/// uyduramasın diye. Bu, izin modelinin (D-040) zorlanabilen tek parçası.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireTrack {
    pub id: String,
    pub artist: String,
    pub title: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// ISRC — biçimi tutmuyorsa **düşürülür ve sayılır**, sessizce kabul
    /// edilmez (K6: zincirin ilk halkası çürütülemez).
    #[serde(default)]
    pub isrc: Option<String>,
}

impl WireTrack {
    /// Çekirdek tiplerine çevirir. `dropped_isrc`: ISRC biçimsizdi mi.
    #[must_use]
    pub fn into_provider_track(
        self,
        provider: &ProviderId,
    ) -> (crate::provider::ProviderTrack, bool) {
        let raw_isrc = self.isrc;
        let isrc = raw_isrc.as_deref().and_then(Isrc::parse);
        let dropped_isrc = raw_isrc.is_some() && isrc.is_none();
        let id = ProviderTrackId::new(provider.clone(), self.id);
        let track = TrackRef {
            artist: self.artist,
            title: self.title,
            album: self.album,
            duration_ms: self.duration_ms,
            isrc,
            provider_track_id: Some(id.clone()),
        };
        (crate::provider::ProviderTrack { id, track }, dropped_isrc)
    }
}

/// `resolve_source` isteğinin gövdesi.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveSourceParams {
    pub id: String,
}

/// `resolve_source` cevabı. `source` yoksa parça çalınamıyor demektir —
/// hata değil, "yok" cevabı.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveSourceResult {
    #[serde(default)]
    pub source: Option<AudioSource>,
}

/// Eklentinin gönderdiği `log` bildirimi.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogParams {
    #[serde(default)]
    pub level: Option<String>,
    pub message: String,
}

/// Yolu tel üstünde taşınabilir bir dizeye çevirir.
///
/// Kayıpsız olmayan dönüşüm burada **bilinçli**: JSON metindir, UTF-8 olmayan
/// bir yol tel üstünde zaten temsil edilemez. Çağıran, yolun kendi ürettiği
/// veri dizini olduğunu bilir.
#[must_use]
pub fn path_to_wire(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_is_one_line_of_json_rpc() {
        let request = Request::call(7, method::HEALTH, serde_json::json!({}));
        let line = serde_json::to_string(&request).unwrap();
        assert!(line.contains("\"jsonrpc\":\"2.0\""), "{line}");
        assert!(line.contains("\"id\":7"), "{line}");
        assert!(!line.contains('\n'), "çerçeveleme satır bazlı: {line}");
    }

    #[test]
    fn a_notification_carries_no_id() {
        let request = Request::notify(method::SHUTDOWN, serde_json::json!({}));
        let line = serde_json::to_string(&request).unwrap();
        assert!(!line.contains("\"id\""), "{line}");
    }

    #[test]
    fn an_error_response_parses_into_code_and_message() {
        let line = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"yok"}}"#;
        let incoming: Incoming = serde_json::from_str(line).unwrap();
        let error = incoming.error.unwrap();
        assert_eq!(error.code, CODE_METHOD_NOT_FOUND);
        assert_eq!(error.message, "yok");
        assert!(incoming.result.is_none());
    }

    #[test]
    fn capability_names_become_a_mask_and_unknown_names_are_reported() {
        let names = vec![
            "search".to_owned(),
            "STREAM".to_owned(),
            "teleport".to_owned(),
        ];
        let (caps, unknown) = parse_capabilities(&names);
        assert!(caps.contains(Capabilities::SEARCH | Capabilities::STREAM));
        assert!(!caps.contains(Capabilities::CONTROL));
        assert_eq!(unknown, vec!["teleport".to_owned()]);
    }

    #[test]
    fn a_wire_track_cannot_claim_another_providers_namespace() {
        let wire = WireTrack {
            id: "12345".to_owned(),
            artist: "Sanatçı".to_owned(),
            title: "Parça".to_owned(),
            album: None,
            duration_ms: Some(1000),
            isrc: Some("USRC17607839".to_owned()),
        };
        let provider = ProviderId::new("soundcloud");
        let (track, dropped) = wire.into_provider_track(&provider);
        assert!(!dropped);
        assert_eq!(track.id.provider, provider);
        assert_eq!(track.id.id, "12345");
        assert!(track.track.isrc.is_some());
    }

    #[test]
    fn a_malformed_isrc_is_dropped_and_counted_not_accepted() {
        let wire = WireTrack {
            id: "1".to_owned(),
            artist: "A".to_owned(),
            title: "B".to_owned(),
            album: None,
            duration_ms: None,
            isrc: Some("uydurma".to_owned()),
        };
        let (track, dropped) = wire.into_provider_track(&ProviderId::new("p"));
        assert!(dropped, "biçimsiz ISRC sayılmalı");
        assert!(track.track.isrc.is_none(), "biçimsiz ISRC kabul edilmemeli");
    }

    #[test]
    fn an_audio_source_round_trips_through_the_wire_shape() {
        let json = r#"{"source":{"kind":"http_stream","url":"https://x/y","headers":[]}}"#;
        let result: ResolveSourceResult = serde_json::from_str(json).unwrap();
        match result.source {
            Some(AudioSource::HttpStream { url, headers }) => {
                assert_eq!(url, "https://x/y");
                assert!(headers.is_empty());
            }
            other => panic!("beklenmeyen kaynak: {other:?}"),
        }
    }

    #[test]
    fn a_missing_source_is_a_no_not_an_error() {
        let result: ResolveSourceResult = serde_json::from_str("{}").unwrap();
        assert!(result.source.is_none());
    }
}
