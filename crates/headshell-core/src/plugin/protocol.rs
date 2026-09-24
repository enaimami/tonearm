//! Eklenti sözleşmesinin **veri biçimi** (api 2, D-069).
//!
//! api 1 bir tel protokolüydü: satır bazlı JSON-RPC, alt süreç, el sıkışma.
//! api 2'de tel yok — eklenti çekirdeğin içindeki QuickJS'te koşar ve
//! sözleşme **dışa aktarılan fonksiyonlardır**. Değerler JS ile Rust arasında
//! JSON olarak geçer; bu dosya o JSON'un Rust tarafındaki şekli.
//!
//! ## api 2'nin fonksiyonları
//!
//! | Fonksiyon | Zorunlu | Dönüş | Karşılığı |
//! |---|---|---|---|
//! | `health()` | evet | [`HealthResult`] | [`crate::provider::Provider::health`] |
//! | `search(query, limit)` | `search` yeteneği varsa | [`WireTrack`] dizisi | [`crate::provider::Provider::search`] |
//! | `resolve_source(id)` | `stream` yeteneği varsa | [`AudioSource`] ya da `null` | [`crate::provider::Provider::resolve_source`] |
//!
//! Fonksiyon adları bilerek api 1'in metot adlarıyla aynı (`resolve_source`,
//! `resolveSource` değil): belge, sağlayıcı trait'i ve eklenti aynı adı
//! kullanınca "hangi isim hangisine karşılık" tablosu gerekmiyor.
//!
//! ## Sürümleme
//!
//! [`PLUGIN_API`] tek bir tam sayı ve manifestte yazar; **eşit değilse
//! eklenti yüklenmez** ve kullanıcı iki sayıyı da görür
//! ([`crate::ErrorKind::PluginIncompatible`]). Kural D-039'un aynısı:
//! **eklemek sürümü artırmaz, kaldırmak/anlamını değiştirmek artırır.** api
//! 1'den 2'ye geçiş ikincisiydi: `exec` kalktı, eklentinin nerede koştuğu
//! değişti.

use crate::ids::{Isrc, ProviderId, ProviderTrackId};
use crate::model::TrackRef;
use crate::provider::{AudioSource, Capabilities};

use serde::{Deserialize, Serialize};

/// Çekirdeğin konuştuğu eklenti sözleşmesinin sürümü.
pub const PLUGIN_API: u32 = 2;

/// Eklentinin dışa aktardığı fonksiyonların adları. Tanımlayıcılar
/// İngilizce (D-036).
pub mod export {
    pub const HEALTH: &str = "health";
    pub const SEARCH: &str = "search";
    pub const RESOLVE_SOURCE: &str = "resolve_source";
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

/// `health()` dönüşü.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResult {
    pub reachable: bool,
    #[serde(default)]
    pub track_count: Option<usize>,
    #[serde(default)]
    pub detail: Option<String>,
}

/// Eklentinin döndürdüğü parça.
///
/// [`crate::provider::ProviderTrack`]'in serde gösterimi değil, kasten daha
/// yalın bir biçim: `id` çıplak bir dize, sağlayıcı adı **yok**. Sağlayıcı
/// adını çekirdek ekler — eklenti başka bir sağlayıcının ad alanında kimlik
/// uyduramasın diye.
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

/// `resolve_source(id)` dönüşü: kaynak ya da `null`.
///
/// `null` hata değil, "bu parça çalınamaz" cevabıdır. Eklentinin **yerel
/// dosya** döndürmesi ise kabul edilmez: dosya sistemine erişimi yok, ve
/// bir yol döndürebilseydi çekirdeğe istediği dosyayı açtırabilirdi
/// ([`check_source`]).
pub type SourceResult = Option<AudioSource>;

/// Eklentinin döndürdüğü kaynağın izinlere uyup uymadığı.
///
/// Akış adresini çekirdek çeker (K3), ama adresi eklenti seçer: denetim
/// olmasa bir eklenti beyan etmediği bir ana bilgisayara çekirdeği
/// kendisi yerine gönderebilirdi. Kural istekle aynı — beyan edilmemiş
/// adrese gidilmez (D-069).
///
/// # Errors
/// Kaynak yerel dosyaysa ya da adresi izinlerin dışındaysa, nedeniyle.
pub fn check_source(
    source: &AudioSource,
    permissions: &super::manifest::Permissions,
) -> std::result::Result<(), String> {
    match source {
        AudioSource::HttpStream { url, .. } => permissions.check_url(url),
        AudioSource::LocalFile { .. } => Err(
            "eklenti yerel dosya kaynağı döndürdü; eklentilerin dosya sistemine erişimi yok \
             (api 2)"
                .to_owned(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::manifest::Permissions;

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
    fn an_audio_source_round_trips_through_the_json_shape() {
        let json = r#"{"kind":"http_stream","url":"https://x/y","headers":[]}"#;
        let result: SourceResult = serde_json::from_str(json).unwrap();
        match result {
            Some(AudioSource::HttpStream { url, headers }) => {
                assert_eq!(url, "https://x/y");
                assert!(headers.is_empty());
            }
            other => panic!("beklenmeyen kaynak: {other:?}"),
        }
        let none: SourceResult = serde_json::from_str("null").unwrap();
        assert!(none.is_none(), "`null` bir cevaptır, hata değil");
    }

    #[test]
    fn a_source_outside_the_permissions_or_on_disk_is_refused() {
        let permissions = Permissions {
            net: vec!["*.googlevideo.com".to_owned()],
        };
        let allowed = AudioSource::HttpStream {
            url: "https://rr1---sn-x.googlevideo.com/videoplayback".to_owned(),
            headers: Vec::new(),
        };
        assert!(check_source(&allowed, &permissions).is_ok());

        let elsewhere = AudioSource::HttpStream {
            url: "http://192.168.1.1/admin".to_owned(),
            headers: Vec::new(),
        };
        assert!(check_source(&elsewhere, &permissions).is_err());

        let local = AudioSource::LocalFile {
            path: "/etc/passwd".into(),
        };
        let err = check_source(&local, &permissions).unwrap_err();
        assert!(err.contains("yerel dosya"), "{err}");
    }
}
