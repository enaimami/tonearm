//! Çekirdek veri modeli: dinleme olayı ve parça referansı.

use serde::{Deserialize, Serialize};

use crate::ids::{CanonicalId, Isrc, ProviderId, ProviderTrackId};

/// Bir parçanın kanonikleşmemiş hâli — export dosyasından okuduğumuz gibi.
///
/// Kimlik çözümlemesinin girdisi budur; çıktısı [`CanonicalId`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackRef {
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    /// Parçanın tam süresi (biliniyorsa). Bulanık eşleşmenin ayırt edici alanı.
    pub duration_ms: Option<u64>,
    /// Kimlik zincirinin ilk halkası. Export'ta varsa altın değerinde.
    pub isrc: Option<Isrc>,
    /// Export'un kendi kimliği (`spotify:track:...`). Kanonik değildir.
    pub provider_track_id: Option<ProviderTrackId>,
}

impl TrackRef {
    /// Yalnızca sanatçı+başlık bilinen en yalın hâl.
    #[must_use]
    pub fn new(artist: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            artist: artist.into(),
            title: title.into(),
            album: None,
            duration_ms: None,
            isrc: None,
            provider_track_id: None,
        }
    }

    #[must_use]
    pub fn with_album(mut self, album: Option<String>) -> Self {
        self.album = album;
        self
    }

    #[must_use]
    pub fn with_duration_ms(mut self, duration_ms: Option<u64>) -> Self {
        self.duration_ms = duration_ms;
        self
    }

    #[must_use]
    pub fn with_isrc(mut self, isrc: Option<Isrc>) -> Self {
        self.isrc = isrc;
        self
    }

    #[must_use]
    pub fn with_provider_track_id(mut self, id: Option<ProviderTrackId>) -> Self {
        self.provider_track_id = id;
        self
    }

    /// İnsan okunur tek satır: `Sanatçı - Başlık`.
    #[must_use]
    pub fn display_name(&self) -> String {
        format!("{} - {}", self.artist, self.title)
    }

    /// `"Radiohead - Creep"` biçimindeki tek satırlık sorguyu ayrıştırır.
    ///
    /// Ayırıcı ilk ` - ` dizisidir; sanatçı adında tire olabilir diye
    /// boşluklu biçim aranır. CLI bunu kendisi yapmaz — ayrıştırma veri
    /// dönüşümüdür ve çekirdeğe aittir.
    ///
    /// # Errors
    /// Ayırıcı yoksa ya da iki taraftan biri boşsa.
    pub fn parse_query(input: &str) -> crate::Result<Self> {
        let invalid = |detail: String| {
            crate::Error::new(
                crate::diag::Stage::IdentityResolve,
                crate::ErrorKind::InvalidInput { detail },
            )
        };
        let (artist, title) = input.split_once(" - ").ok_or_else(|| {
            invalid(format!(
                "{input:?} 'Sanatçı - Başlık' biçiminde değil (ayırıcı: boşluk-tire-boşluk)"
            ))
        })?;
        let (artist, title) = (artist.trim(), title.trim());
        if artist.is_empty() || title.is_empty() {
            return Err(invalid(format!(
                "{input:?} içinde sanatçı ya da başlık boş"
            )));
        }
        Ok(Self::new(artist, title))
    }
}

/// Tek bir dinleme olayı. Sözlükteki `listen`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listen {
    /// Ne dinlendi (ham hâliyle).
    pub track: TrackRef,
    /// Ne zaman başladı/bitti — export'a göre değişir, UTC'ye normalize edilir.
    pub played_at: jiff::Timestamp,
    /// Kaç milisaniye çalındı. Skip tespiti ve "gerçek dinleme" eşiği buna dayanır.
    pub ms_played: u64,
    /// Kayıt nereden geldi.
    pub source: ListenSource,
    /// Çözümlenmişse kanonik kimlik. Faz 0'da import sonrası doldurulur.
    pub canonical_id: Option<CanonicalId>,
}

impl Listen {
    /// Bu dinleme sayılan bir çalma mı?
    ///
    /// Kararı [`PlayRule`] verir — çekirdekte tek tanım (D-008).
    #[must_use]
    pub fn counts_as_play(&self, rule: PlayRule) -> bool {
        rule.counts(self.ms_played, self.track.duration_ms)
    }
}

/// Spotify'ın "dinlendi" saydığı eşik; sektör alışkanlığı olduğu için
/// varsayılan bu, ama gizli değil — [`PlayRule`] ile değiştirilebilir.
pub const DEFAULT_MIN_MS_PLAYED: u64 = 30_000;

/// Bir dinleme olayının **sayılan çalma** olup olmadığına karar veren kural.
///
/// D-008: bu kavram çekirdekte tek yerde tanımlıdır. `stats` eşiği uygulayıp,
/// `library search` ham olayları sayınca aynı fixture'da "18 çalma" ve
/// "9 çalma" çıkmıştı. Artık iki yüzey de bu kuralı çağırır.
///
/// İki sayı birbirine karıştırılmasın diye adları da ayrıldı:
/// - **`play_count`** — bu kuralı geçen, kullanıcıya gösterilen çalma sayısı.
/// - **`listen_events`** — ham olay sayısı; yalnızca `diag` ve hata ayıklamada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayRule {
    /// Bu sürenin altında kalan çalmalar atlama (skip) sayılır.
    pub min_ms_played: u64,
}

impl Default for PlayRule {
    fn default() -> Self {
        Self {
            min_ms_played: DEFAULT_MIN_MS_PLAYED,
        }
    }
}

impl PlayRule {
    #[must_use]
    pub const fn new(min_ms_played: u64) -> Self {
        Self { min_ms_played }
    }

    /// Scrobble konvansiyonu: eşiği geçmiş **ya da** parçanın en az yarısı.
    ///
    /// Yarım-parça kolu yalnızca süre biliniyorsa çalışır; bilinmeyen süreyi
    /// "yarısını dinledi" saymak sayıyı şişirir. Faz 0'daki Spotify export'u
    /// süre taşımıyor, dolayısıyla pratikte eşik kolu karar veriyor — kural
    /// yerel dosyalar geldiğinde (Faz 1) devreye girecek.
    #[must_use]
    pub const fn counts(self, ms_played: u64, duration_ms: Option<u64>) -> bool {
        if ms_played >= self.min_ms_played {
            return true;
        }
        match duration_ms {
            Some(duration) if duration > 0 => ms_played.saturating_mul(2) >= duration,
            _ => false,
        }
    }
}

/// Bir dinlemenin kaynağı.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ListenSource {
    /// Bir veri export dosyasından içe aktarıldı.
    Import { export: ExportKind },
    /// Bir sağlayıcı üzerinden `headshell` ile çalındı.
    Playback { provider: ProviderId },
}

/// Hangi sağlayıcının export biçimi.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportKind {
    /// Spotify "Extended streaming history" (`Streaming_History_Audio_*.json`).
    SpotifyExtended,
    /// Spotify hesap verisi (`StreamingHistory*.json`) — yalnızca son 1 yıl.
    SpotifyAccount,
    /// Apple Music gizlilik export'u.
    AppleMusic,
    /// Google Takeout / YouTube Music.
    GoogleTakeout,
}

impl ExportKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SpotifyExtended => "spotify_extended",
            Self::SpotifyAccount => "spotify_account",
            Self::AppleMusic => "apple_music",
            Self::GoogleTakeout => "google_takeout",
        }
    }
}

impl std::fmt::Display for ExportKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listen(ms_played: u64, duration_ms: Option<u64>) -> Listen {
        Listen {
            track: TrackRef::new("Radiohead", "Creep").with_duration_ms(duration_ms),
            played_at: jiff::Timestamp::UNIX_EPOCH,
            ms_played,
            source: ListenSource::Import {
                export: ExportKind::SpotifyExtended,
            },
            canonical_id: None,
        }
    }

    #[test]
    fn play_threshold_is_explicit() {
        let l = listen(25_000, None);
        assert!(!l.counts_as_play(PlayRule::new(30_000)));
        assert!(l.counts_as_play(PlayRule::new(20_000)));
    }

    #[test]
    fn half_a_short_track_counts_even_below_the_threshold() {
        let rule = PlayRule::default();
        // 40 sn'lik bir parçanın 25 sn'si: eşiğin altında ama yarısından fazla.
        assert!(rule.counts(25_000, Some(40_000)));
        // Aynı süre, uzun parça: yarısına ulaşmıyor, sayılmaz.
        assert!(!rule.counts(25_000, Some(240_000)));
        // Süre bilinmiyorsa yarım-parça kolu hiç çalışmaz.
        assert!(!rule.counts(25_000, None));
        // Sıfır süre bölme/şişirme üretmemeli.
        assert!(!rule.counts(1, Some(0)));
    }

    #[test]
    fn threshold_alone_is_enough_regardless_of_duration() {
        let rule = PlayRule::default();
        assert!(rule.counts(30_000, None));
        assert!(rule.counts(30_000, Some(600_000)));
    }

    #[test]
    fn parse_query_splits_on_the_first_spaced_dash() {
        let track = TrackRef::parse_query("Radiohead - Creep").unwrap();
        assert_eq!(track.artist, "Radiohead");
        assert_eq!(track.title, "Creep");

        let dashed = TrackRef::parse_query("Sault - 9 - Reprise").unwrap();
        assert_eq!(dashed.artist, "Sault");
        assert_eq!(dashed.title, "9 - Reprise");
    }

    #[test]
    fn parse_query_rejects_input_without_a_separator() {
        let err = TrackRef::parse_query("sadece başlık").unwrap_err();
        assert!(
            err.chain_text().contains("Sanatçı - Başlık"),
            "{}",
            err.chain_text()
        );
        assert!(TrackRef::parse_query(" - Creep").is_err());
        assert!(TrackRef::parse_query("Radiohead - ").is_err());
    }

    #[test]
    fn listen_source_round_trips_as_tagged_json() {
        let source = ListenSource::Import {
            export: ExportKind::SpotifyExtended,
        };
        let json = serde_json::to_string(&source).unwrap();
        assert_eq!(json, r#"{"kind":"import","export":"spotify_extended"}"#);
        let back: ListenSource = serde_json::from_str(&json).unwrap();
        assert_eq!(back, source);
    }
}
