//! Kanonik kimlik çözümlemesi.
//!
//! **Değişmez kural #5 — zincir bozulmaz:**
//! ISRC → MusicBrainz ID → bulanık eşleşme (sanatçı+başlık+süre) → AcoustID
//! parmak izi. Her adım bir güven skoru döndürür ve hangi adımın çözdüğü
//! kayıtta durur; "çözüldü" demek yetmez, *nasıl* çözüldüğü ölçülebilir olmalı.

pub mod fuzzy;
pub mod normalize;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::ids::{CanonicalId, Isrc, Mbid};
use crate::model::TrackRef;

/// Zincirin hangi halkasının çözdüğü.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolveMethod {
    /// ISRC doğrudan bir kayda götürdü.
    Isrc,
    /// Üstveri araması tam (normalize) isabet verdi.
    Mbid,
    /// Bulanık eşleşme eşiği geçti.
    Fuzzy,
    /// Ses parmak izi (AcoustID) — Faz 2.
    Fingerprint,
    /// Zincir sonuçsuz kaldı; yerel anahtardan kimlik türetildi.
    ///
    /// Gruplama için yeterli, otorite olarak değersiz. Ağ geldiğinde bu
    /// kayıtlar yeniden çözümlenmeli.
    LocalKey,
}

impl ResolveMethod {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Isrc => "isrc",
            Self::Mbid => "mbid",
            Self::Fuzzy => "fuzzy",
            Self::Fingerprint => "fingerprint",
            Self::LocalKey => "local_key",
        }
    }
}

impl std::fmt::Display for ResolveMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Tek bir parçanın çözümleme sonucu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resolution {
    pub canonical_id: CanonicalId,
    pub method: ResolveMethod,
    /// 0.0–1.0. `LocalKey` için düşüktür: kimlik tutarlı ama otoritesiz.
    pub confidence: f64,
    /// Eşleşmenin hangi kayda gittiği (varsa).
    pub matched: Option<Candidate>,
}

/// Üstveri kaynağından dönen aday kayıt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub mbid: Mbid,
    pub artist: String,
    pub title: String,
    pub duration_ms: Option<u64>,
    pub isrc: Option<Isrc>,
}

/// Bir çözümleme turunun özeti. Projenin en önemli metriği buradan okunur.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveSummary {
    pub total: usize,
    pub by_isrc: usize,
    pub by_mbid: usize,
    pub by_fuzzy: usize,
    pub by_fingerprint: usize,
    pub by_local_key: usize,
}

impl ResolveSummary {
    fn record(&mut self, method: ResolveMethod) {
        self.total += 1;
        match method {
            ResolveMethod::Isrc => self.by_isrc += 1,
            ResolveMethod::Mbid => self.by_mbid += 1,
            ResolveMethod::Fuzzy => self.by_fuzzy += 1,
            ResolveMethod::Fingerprint => self.by_fingerprint += 1,
            ResolveMethod::LocalKey => self.by_local_key += 1,
        }
    }

    /// Otoriteli (MusicBrainz'e bağlanmış) çözümlemelerin oranı.
    ///
    /// Doğruluk kümesindeki hedef metrik budur — `LocalKey` sayılmaz.
    #[must_use]
    pub fn authoritative_ratio(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        let authoritative = self.by_isrc + self.by_mbid + self.by_fuzzy + self.by_fingerprint;
        #[expect(clippy::cast_precision_loss, reason = "oran gösterimi; kayıp önemsiz")]
        {
            authoritative as f64 / self.total as f64
        }
    }

    /// Sayaçları tanı kaydediciye aktarır.
    pub fn record_into(&self, recorder: &mut crate::diag::Recorder) {
        let n = |v: usize| i64::try_from(v).unwrap_or(i64::MAX);
        recorder.set("identity.total", n(self.total));
        recorder.set("identity.by_isrc", n(self.by_isrc));
        recorder.set("identity.by_mbid", n(self.by_mbid));
        recorder.set("identity.by_fuzzy", n(self.by_fuzzy));
        recorder.set("identity.by_fingerprint", n(self.by_fingerprint));
        recorder.set("identity.by_local_key", n(self.by_local_key));
    }
}

/// Bir üstveri çağrısının dönüşü.
///
/// `async fn` yerine kutulanmış future, çünkü trait'in `dyn` uyumlu olması
/// gerekiyor (K7 / D-006): `-> impl Future` taşıyan bir trait'ten
/// `Arc<dyn MetadataLookup>` üretilemez. Bu, `async-trait` makrosunun
/// ürettiğinin elle yazılmış hâli — bağımlılık ağacını büyütmemek için.
pub type LookupFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

/// Üstveri kaynağı (MusicBrainz, yerel katalog, test sahtesi).
///
/// Ağa çıkan her şey bu trait'in arkasında; testler ağa bağlanmaz.
/// İmzalar `async` çünkü gerçek uygulaması HTTP konuşacak — bugün senkron
/// yazıp yarın bütün çağıranları değiştirmemek için.
///
/// `Send + Sync`: `uniffi` bunu **callback interface** olarak modeller
/// (`#[uniffi::export(with_foreign)]`), yani Kotlin/Swift tarafında da
/// uygulanabilir; oradan gelen nesne iş parçacıkları arasında geçer.
pub trait MetadataLookup: Send + Sync {
    /// ISRC'den kayıt kimliği.
    fn recording_by_isrc<'a>(&'a self, isrc: &'a Isrc) -> LookupFuture<'a, Option<Candidate>>;

    /// Sanatçı+başlık ile aday arama.
    fn search_recordings<'a>(
        &'a self,
        artist: &'a str,
        title: &'a str,
    ) -> LookupFuture<'a, Vec<Candidate>>;
}

/// Ağa çıkmayan kaynak. Faz 0'ın varsayılanı: zincir yerel anahtara düşer.
#[derive(Debug, Clone, Copy, Default)]
pub struct OfflineLookup;

impl MetadataLookup for OfflineLookup {
    fn recording_by_isrc<'a>(&'a self, _isrc: &'a Isrc) -> LookupFuture<'a, Option<Candidate>> {
        Box::pin(std::future::ready(Ok(None)))
    }

    fn search_recordings<'a>(
        &'a self,
        _artist: &'a str,
        _title: &'a str,
    ) -> LookupFuture<'a, Vec<Candidate>> {
        Box::pin(std::future::ready(Ok(Vec::new())))
    }
}

/// Bellekteki sabit katalog. Testler ve doğruluk kümesi için.
#[derive(Debug, Clone, Default)]
pub struct StaticLookup {
    candidates: Vec<Candidate>,
}

impl StaticLookup {
    #[must_use]
    pub fn new(candidates: Vec<Candidate>) -> Self {
        Self { candidates }
    }
}

impl MetadataLookup for StaticLookup {
    fn recording_by_isrc<'a>(&'a self, isrc: &'a Isrc) -> LookupFuture<'a, Option<Candidate>> {
        Box::pin(async move {
            Ok(self
                .candidates
                .iter()
                .find(|c| c.isrc.as_ref() == Some(isrc))
                .cloned())
        })
    }

    fn search_recordings<'a>(
        &'a self,
        artist: &'a str,
        title: &'a str,
    ) -> LookupFuture<'a, Vec<Candidate>> {
        Box::pin(async move {
            let key_artist = normalize::normalize_artist(artist);
            let key_title = normalize::normalize_text(title);
            // Gerçek bir arama motoru gibi davran: ilk harflerden kabaca ele,
            // asıl kararı skorlama versin.
            Ok(self
                .candidates
                .iter()
                .filter(|c| {
                    normalize::normalize_artist(&c.artist) == key_artist
                        || normalize::normalize_text(&c.title) == key_title
                })
                .cloned()
                .collect())
        })
    }
}

/// Çözümleme eşikleri.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolveConfig {
    /// Bulanık eşleşmenin kabul eşiği. Altı `LocalKey`'e düşer.
    pub min_fuzzy_confidence: f64,
    /// Bu skorun üstündeki eşleşme "tam isabet" (`Mbid`) sayılır.
    pub exact_match_confidence: f64,
}

impl Default for ResolveConfig {
    fn default() -> Self {
        Self {
            min_fuzzy_confidence: 0.88,
            exact_match_confidence: 0.98,
        }
    }
}

/// Kimlik zincirini yürüten çözümleyici.
///
/// Üstveri kaynağı generic değil `Arc<dyn MetadataLookup>` (D-006): `uniffi`
/// generic parametre ifade edemez, `Arc<dyn Trait>` ise callback interface
/// olarak geçer.
#[derive(Clone)]
pub struct Resolver {
    lookup: Arc<dyn MetadataLookup>,
    config: ResolveConfig,
}

impl std::fmt::Debug for Resolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Kaynak yabancı dilde uygulanmış olabilir; `Debug` istemiyoruz.
        f.debug_struct("Resolver")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl Resolver {
    #[must_use]
    pub fn new(lookup: Arc<dyn MetadataLookup>) -> Self {
        Self {
            lookup,
            config: ResolveConfig::default(),
        }
    }

    #[must_use]
    pub fn with_config(mut self, config: ResolveConfig) -> Self {
        self.config = config;
        self
    }

    /// Tek bir parçayı zincirden geçirir.
    ///
    /// # Errors
    /// Üstveri kaynağı hata döndürürse. Kaynağın "bulamadım" demesi hata
    /// değildir — zincir bir sonraki halkaya geçer.
    pub async fn resolve(&self, track: &TrackRef) -> Result<Resolution> {
        // 1. Halka: ISRC.
        if let Some(isrc) = &track.isrc {
            if let Some(candidate) = self.lookup.recording_by_isrc(isrc).await? {
                return Ok(Resolution {
                    canonical_id: CanonicalId::from_mbid(&candidate.mbid),
                    method: ResolveMethod::Isrc,
                    confidence: 1.0,
                    matched: Some(candidate),
                });
            }
            // Kaynak ISRC'yi tanımadı ama ISRC'nin kendisi geçerli bir otorite.
            return Ok(Resolution {
                canonical_id: CanonicalId::from_isrc(isrc),
                method: ResolveMethod::Isrc,
                confidence: 0.95,
                matched: None,
            });
        }

        // 2. ve 3. Halka: üstveri araması + skorlama.
        let candidates = self
            .lookup
            .search_recordings(&track.artist, &track.title)
            .await?;
        let best = candidates
            .into_iter()
            .map(|candidate| {
                let score = fuzzy::similarity(
                    &track.artist,
                    &track.title,
                    track.duration_ms,
                    &candidate.artist,
                    &candidate.title,
                    candidate.duration_ms,
                );
                (candidate, score)
            })
            .max_by(|(_, a), (_, b)| a.total_cmp(b));

        if let Some((candidate, score)) = best {
            if score >= self.config.min_fuzzy_confidence {
                let method = if score >= self.config.exact_match_confidence {
                    ResolveMethod::Mbid
                } else {
                    ResolveMethod::Fuzzy
                };
                return Ok(Resolution {
                    canonical_id: CanonicalId::from_mbid(&candidate.mbid),
                    method,
                    confidence: score,
                    matched: Some(candidate),
                });
            }
            tracing::debug!(
                track = track.display_name(),
                score,
                esik = self.config.min_fuzzy_confidence,
                "en iyi aday eşiği geçemedi"
            );
        }

        // 4. Halka (AcoustID) Faz 2'de gelecek; dosya elimizde olmadan
        // parmak izi çıkaramayız. Şimdilik yerel anahtara düşüyoruz.
        Ok(Resolution {
            canonical_id: CanonicalId::from_local_key(&normalize::track_key(
                &track.artist,
                &track.title,
            )),
            method: ResolveMethod::LocalKey,
            confidence: 0.2,
            matched: None,
        })
    }

    /// Bir parça kümesini çözümler ve özet döndürür.
    ///
    /// Aynı parça birden çok kez geçebilir; tekrar eden çözümlemeler
    /// önbelleğe alınır.
    ///
    /// # Errors
    /// Üstveri kaynağı hata döndürürse.
    pub async fn resolve_all(
        &self,
        tracks: &[TrackRef],
    ) -> Result<(Vec<Resolution>, ResolveSummary)> {
        let mut cache: std::collections::HashMap<String, Resolution> =
            std::collections::HashMap::new();
        let mut out = Vec::with_capacity(tracks.len());
        let mut summary = ResolveSummary::default();

        for track in tracks {
            let key = normalize::track_key(&track.artist, &track.title);
            let resolution = if let Some(hit) = cache.get(&key) {
                hit.clone()
            } else {
                let resolved = self.resolve(track).await?;
                cache.insert(key, resolved.clone());
                resolved
            };
            summary.record(resolution.method);
            out.push(resolution);
        }
        Ok((out, summary))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(mbid: &str, artist: &str, title: &str, duration_ms: Option<u64>) -> Candidate {
        Candidate {
            mbid: Mbid::parse(mbid).expect("test mbid geçerli"),
            artist: artist.to_owned(),
            title: title.to_owned(),
            duration_ms,
            isrc: None,
        }
    }

    fn catalog() -> StaticLookup {
        StaticLookup::new(vec![
            Candidate {
                isrc: Isrc::parse("GBAYE9200001"),
                ..candidate(
                    "b1a9c0e9-d987-4042-ae91-78d6a3267d69",
                    "Radiohead",
                    "Creep",
                    Some(238_000),
                )
            },
            candidate(
                "c2b8d1f0-1234-4042-ae91-78d6a3267d70",
                "Portishead",
                "Roads",
                Some(303_000),
            ),
        ])
    }

    #[tokio::test]
    async fn isrc_wins_the_chain() {
        let resolver = Resolver::new(Arc::new(catalog()));
        let track = TrackRef::new("yanlış yazılmış sanatçı", "yanlış başlık")
            .with_isrc(Isrc::parse("GBAYE9200001"));
        let res = resolver.resolve(&track).await.unwrap();
        assert_eq!(res.method, ResolveMethod::Isrc);
        assert_eq!(res.confidence, 1.0);
        assert_eq!(res.canonical_id.kind(), crate::ids::CanonicalKind::Mbid);
    }

    #[tokio::test]
    async fn exact_metadata_match_is_reported_as_mbid() {
        let resolver = Resolver::new(Arc::new(catalog()));
        let track =
            TrackRef::new("Radiohead", "Creep (Remastered)").with_duration_ms(Some(238_400));
        let res = resolver.resolve(&track).await.unwrap();
        assert_eq!(res.method, ResolveMethod::Mbid);
        assert!(res.confidence >= 0.98, "{}", res.confidence);
    }

    #[tokio::test]
    async fn below_threshold_falls_back_to_local_key() {
        let resolver = Resolver::new(Arc::new(catalog()));
        let track = TrackRef::new("Radiohead", "Karma Police");
        let res = resolver.resolve(&track).await.unwrap();
        assert_eq!(res.method, ResolveMethod::LocalKey);
        assert_eq!(res.canonical_id.kind(), crate::ids::CanonicalKind::Local);
    }

    #[tokio::test]
    async fn offline_lookup_never_invents_authority() {
        let resolver = Resolver::new(Arc::new(OfflineLookup));
        let track = TrackRef::new("Radiohead", "Creep");
        let res = resolver.resolve(&track).await.unwrap();
        assert_eq!(res.method, ResolveMethod::LocalKey);
    }

    #[tokio::test]
    async fn summary_counts_every_method() {
        let resolver = Resolver::new(Arc::new(catalog()));
        let tracks = vec![
            TrackRef::new("Radiohead", "Creep").with_duration_ms(Some(238_000)),
            TrackRef::new("Radiohead", "Creep").with_duration_ms(Some(238_000)),
            TrackRef::new("Bilinmeyen", "Parça"),
        ];
        let (resolutions, summary) = resolver.resolve_all(&tracks).await.unwrap();
        assert_eq!(resolutions.len(), 3);
        assert_eq!(summary.total, 3);
        assert_eq!(
            summary.by_mbid, 2,
            "aynı parça önbellekten aynı sonucu almalı"
        );
        assert_eq!(summary.by_local_key, 1);
        assert!((summary.authoritative_ratio() - 2.0 / 3.0).abs() < 1e-9);
    }
}
