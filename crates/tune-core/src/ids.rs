//! Tip güvenli kimlikler.
//!
//! Kural: kimlikler `String` değil. Bir `ProviderTrackId`'yi `CanonicalId`
//! bekleyen yere geçiremezsin — derleyici durdurur.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Sağlayıcıdan bağımsız parça kimliği.
///
/// Tercih sırası: MusicBrainz Recording ID → ISRC türevi → yerel türetilmiş.
/// Hangisi olduğu [`CanonicalId::kind`] ile okunur.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CanonicalId(String);

/// Bir [`CanonicalId`]'nin hangi otoriteden geldiği.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalKind {
    /// MusicBrainz Recording MBID — altın standart.
    Mbid,
    /// ISRC'den türetilmiş; MBID bulunana kadar geçerli.
    Isrc,
    /// Yerel olarak türetilmiş (normalize sanatçı+başlık hash'i). En zayıf hâli.
    Local,
}

impl CanonicalId {
    /// MusicBrainz Recording MBID'sinden kanonik kimlik.
    #[must_use]
    pub fn from_mbid(mbid: &Mbid) -> Self {
        Self(format!("mb:{}", mbid.as_str()))
    }

    /// ISRC'den kanonik kimlik. MBID bulunana kadarki ara durum.
    #[must_use]
    pub fn from_isrc(isrc: &Isrc) -> Self {
        Self(format!("isrc:{}", isrc.as_str()))
    }

    /// Yerel türetilmiş kimlik. `key` normalize edilmiş "sanatçı\u{1}başlık" olmalı.
    #[must_use]
    pub fn from_local_key(key: &str) -> Self {
        Self(format!("local:{}", fnv1a64_hex(key)))
    }

    /// Kimliğin hangi otoriteden geldiği.
    #[must_use]
    pub fn kind(&self) -> CanonicalKind {
        match self.0.split_once(':') {
            Some(("mb", _)) => CanonicalKind::Mbid,
            Some(("isrc", _)) => CanonicalKind::Isrc,
            _ => CanonicalKind::Local,
        }
    }

    /// Depolama/serileştirme için ham gösterim.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Depodan okunan ham gösterimi geri sarar.
    ///
    /// Yalnızca `tune`'un kendi yazdığı veriyi geri okurken kullanılmalı;
    /// dış girdi için `from_*` yapıcılarını kullan.
    #[must_use]
    pub fn from_stored(raw: String) -> Self {
        Self(raw)
    }
}

impl fmt::Display for CanonicalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Bir sağlayıcının kendi parça kimliği. Sağlayıcı adı olmadan anlamsızdır.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ProviderTrackId {
    pub provider: ProviderId,
    pub id: String,
}

impl ProviderTrackId {
    #[must_use]
    pub fn new(provider: ProviderId, id: impl Into<String>) -> Self {
        Self {
            provider,
            id: id.into(),
        }
    }
}

impl fmt::Display for ProviderTrackId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.provider, self.id)
    }
}

/// Bir sağlayıcının adı (`local`, `subsonic`, `soundcloud`, ...).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProviderId(String);

impl ProviderId {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// ISRC — 12 karakter: 2 ülke + 3 kayıt sahibi + 2 yıl + 5 tekil.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Isrc(String);

impl Isrc {
    /// Girdiyi normalize eder (tire/boşluk atılır, büyük harfe çevrilir) ve doğrular.
    ///
    /// Biçim tutmuyorsa `None` — sessizce kabul etmek kimlik zincirinin ilk
    /// halkasını çürütür.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let cleaned: String = raw
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_uppercase())
            .collect();
        if cleaned.len() != 12 {
            return None;
        }
        let b = cleaned.as_bytes();
        let shape_ok = b[0..2].iter().all(u8::is_ascii_alphabetic)
            && b[2..5].iter().all(u8::is_ascii_alphanumeric)
            && b[5..12].iter().all(u8::is_ascii_digit);
        shape_ok.then_some(Self(cleaned))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Isrc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// MusicBrainz kimliği (UUID biçimi).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Mbid(String);

impl Mbid {
    /// UUID biçimini doğrular (8-4-4-4-12 onaltılık).
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let lower = raw.trim().to_ascii_lowercase();
        let groups: Vec<&str> = lower.split('-').collect();
        let expected = [8usize, 4, 4, 4, 12];
        if groups.len() != expected.len() {
            return None;
        }
        let ok = groups
            .iter()
            .zip(expected)
            .all(|(g, len)| g.len() == len && g.bytes().all(|c| c.is_ascii_hexdigit()));
        ok.then_some(Self(lower))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Mbid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// FNV-1a 64 bit. Yerel kimlik türetmek için; kriptografik değil, sadece stabil.
fn fnv1a64_hex(input: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isrc_normalizes_and_validates() {
        assert_eq!(
            Isrc::parse("us-rc1-17-00001").unwrap().as_str(),
            "USRC11700001"
        );
        assert_eq!(
            Isrc::parse("usrc11700001").unwrap().as_str(),
            "USRC11700001"
        );
        assert!(
            Isrc::parse("USRC1170000").is_none(),
            "11 karakter reddedilmeli"
        );
        assert!(
            Isrc::parse("12RC11700001").is_none(),
            "ülke kodu harf olmalı"
        );
        assert!(
            Isrc::parse("USRC1170000X").is_none(),
            "son 7 hane rakam olmalı"
        );
    }

    #[test]
    fn mbid_validates_uuid_shape() {
        let ok = "b1a9c0e9-d987-4042-ae91-78d6a3267d69";
        assert_eq!(Mbid::parse(ok).unwrap().as_str(), ok);
        assert_eq!(
            Mbid::parse("B1A9C0E9-D987-4042-AE91-78D6A3267D69")
                .unwrap()
                .as_str(),
            ok
        );
        assert!(Mbid::parse("not-a-uuid").is_none());
        assert!(Mbid::parse("b1a9c0e9d9874042ae9178d6a3267d69").is_none());
    }

    #[test]
    fn canonical_id_reports_its_authority() {
        let mbid = Mbid::parse("b1a9c0e9-d987-4042-ae91-78d6a3267d69").unwrap();
        assert_eq!(CanonicalId::from_mbid(&mbid).kind(), CanonicalKind::Mbid);
        let isrc = Isrc::parse("USRC11700001").unwrap();
        assert_eq!(CanonicalId::from_isrc(&isrc).kind(), CanonicalKind::Isrc);
        assert_eq!(
            CanonicalId::from_local_key("a\u{1}b").kind(),
            CanonicalKind::Local
        );
    }

    #[test]
    fn local_key_is_stable_and_distinct() {
        let a = CanonicalId::from_local_key("radiohead\u{1}creep");
        assert_eq!(a, CanonicalId::from_local_key("radiohead\u{1}creep"));
        assert_ne!(a, CanonicalId::from_local_key("radiohead\u{1}creeq"));
    }
}
