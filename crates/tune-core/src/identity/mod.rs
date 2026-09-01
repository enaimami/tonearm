//! Kanonik kimlik çözümlemesi.
//!
//! **Değişmez kural #5 — zincir bozulmaz:**
//! ISRC → MusicBrainz ID → bulanık eşleşme (sanatçı+başlık+süre) → AcoustID
//! parmak izi. Her adım bir güven skoru döndürür ve hangi adımın çözdüğü
//! kayıtta durur; "çözüldü" demek yetmez, *nasıl* çözüldüğü ölçülebilir olmalı.

pub mod fuzzy;
pub mod musicbrainz;
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
    /// En yüksek skoru **kaç aday paylaştı**. `1` = tekil kazanan.
    ///
    /// Gerçek bir katalogda beraberlik istisna değil kural: `Radiohead —
    /// Creep` araması onlarca birebir aynı sanatçı ve başlığı döndürüyor ve
    /// süre bilinmediğinde hepsi aynı skoru alıyor. Bu sayı olmadan çıktı
    /// "%100 güven" diyordu — oysa yapılan iş 25 eşdeğer aday arasından
    /// **belirlenimci ama keyfi** bir seçimdi (D-045). K9: seçimin dayandığı
    /// kanıtın zayıflığı ölçülebilir olmalı.
    ///
    /// Aday listesi kullanılmayan halkalarda (ISRC, `LocalKey`) `1`.
    #[serde(default = "one")]
    pub tied_candidates: usize,
}

/// `serde` varsayılanı: alanı olmayan eski kayıtlar tekil sayılır.
const fn one() -> usize {
    1
}

/// İki skorun "aynı" sayılacağı pay.
///
/// Kayan noktada tam eşitlik aramak, aynı hesabı farklı sırayla yapan iki
/// adayı ayrı gösterirdi. Beraberliği kaçırmak onu uydurmaktan daha kötü:
/// kaçırılan beraberlik "%100 güven" diye raporlanır.
const SCORE_EPSILON: f64 = 1e-9;

/// Beraberlikte güvenin **kesin olarak** altında kalacağı eşiğe pay.
const AMBIGUITY_MARGIN: f64 = 0.01;

/// En yüksek skoru kaç aday paylaşıyor.
fn count_tied(scored: &[(Candidate, f64)], top: f64) -> usize {
    scored
        .iter()
        .take_while(|(_, score)| (top - score).abs() <= SCORE_EPSILON)
        .count()
        .max(1)
}

/// Üstveri kaynağından dönen aday kayıt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub mbid: Mbid,
    pub artist: String,
    pub title: String,
    pub duration_ms: Option<u64>,
    pub isrc: Option<Isrc>,
    /// Kaydı adaşlarından ayıran not — MusicBrainz'in `disambiguation` alanı.
    ///
    /// Başlıkta olmayan ama kimliği belirleyen bilgi buradan gelir:
    /// `"live, 1994-05-27: Astoria, London, UK"`. Katalogdaki üç `Creep`'in
    /// üçünün de başlığı `Creep`'tir; hangisinin canlı kayıt olduğu **yalnızca**
    /// bu alanda yazar (D-045). [`fuzzy::similarity`] onu `context_b` olarak
    /// alır.
    #[serde(default)]
    pub disambiguation: Option<String>,
}

/// Skorları eşit adaylar arasında hangisinin daha iyi bir kanonik seçim
/// olduğu. Büyük olan kazanır.
///
/// İki ölçüt, ikisi de "hangisi bu şarkının **varsayılan** kaydı" sorusunu
/// cevaplıyor:
/// - **Ayırt edici notu olmayan** kayıt varsayılandır: MusicBrainz notu
///   yalnızca bir kaydı adaşlarından ayırmak gerektiğinde yazar. Notu olan
///   kayıt tanım gereği bir istisnadır (canlı, remiks, farklı bir gece).
/// - **Süresi bilinen** kayıt, bilinmeyene tercih edilir: daha çok bilgi
///   taşıyan kayıt daha çok işlenmiş, dolayısıyla daha çok güvenilir kayıttır.
fn tiebreak_rank(candidate: &Candidate) -> u8 {
    u8::from(candidate.disambiguation.is_none()) * 2 + u8::from(candidate.duration_ms.is_some())
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
                    tied_candidates: 1,
                });
            }
            // Kaynak ISRC'yi tanımadı ama ISRC'nin kendisi geçerli bir otorite.
            return Ok(Resolution {
                canonical_id: CanonicalId::from_isrc(isrc),
                method: ResolveMethod::Isrc,
                confidence: 0.95,
                matched: None,
                tied_candidates: 1,
            });
        }

        // 2. ve 3. Halka: üstveri araması + skorlama.
        let candidates = self
            .lookup
            .search_recordings(&track.artist, &track.title)
            .await?;
        let mut scored: Vec<(Candidate, f64)> = candidates
            .into_iter()
            .map(|candidate| {
                let score = fuzzy::similarity(
                    &track.artist,
                    &track.title,
                    track.duration_ms,
                    &candidate.artist,
                    &candidate.title,
                    candidate.duration_ms,
                    candidate.disambiguation.as_deref(),
                );
                (candidate, score)
            })
            .collect();
        // Skor eşitliğini **belirlenimci** biçimde kır. Gerçek bir katalogda
        // eşitlik istisna değil kural: `Radiohead — Creep` araması 190'dan
        // fazla kayıt döndürüyor, onlarcası birebir aynı sanatçı ve başlığı
        // taşıyor ve süre bilinmediğinde hepsi aynı skoru alıyor. `max_by` bu
        // durumda MusicBrainz'in gönderdiği sıraya teslim oluyordu ve o sıra
        // sabit değil — aynı sorgu iki koşumda iki farklı MBID verdi (D-045).
        // Kimlik katmanı için bu kabul edilemez: aynı parça yarın başka bir
        // kanonik kimlik alamaz.
        scored.sort_by(|(left_candidate, left), (right_candidate, right)| {
            right
                .total_cmp(left)
                .then_with(|| tiebreak_rank(right_candidate).cmp(&tiebreak_rank(left_candidate)))
                // Son çare: MBID sırası. Keyfi ama **sabit** — sabit olması
                // keyfi olmamasından daha önemli.
                .then_with(|| {
                    left_candidate
                        .mbid
                        .as_str()
                        .cmp(right_candidate.mbid.as_str())
                })
        });
        let tied = scored
            .first()
            .map_or(1, |(_, top)| count_tied(&scored, *top));
        let best = scored.into_iter().next();

        if let Some((candidate, score)) = best {
            if score >= self.config.min_fuzzy_confidence {
                // Beraberlik varsa "tam isabet" denmez. Skor aynı kalıyor —
                // metin ve süre gerçekten uyuyor — ama seçim eşdeğerler
                // arasından yapıldı ve `Mbid` bunu iddia edemez.
                let method = if tied == 1 && score >= self.config.exact_match_confidence {
                    ResolveMethod::Mbid
                } else {
                    ResolveMethod::Fuzzy
                };
                let confidence = if tied > 1 {
                    score.min(self.config.exact_match_confidence - AMBIGUITY_MARGIN)
                } else {
                    score
                };
                return Ok(Resolution {
                    canonical_id: CanonicalId::from_mbid(&candidate.mbid),
                    method,
                    confidence,
                    matched: Some(candidate),
                    tied_candidates: tied,
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
            tied_candidates: 1,
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
            disambiguation: None,
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
        // Üstveri kasten çöp: ISRC varsa zincir ona bakmadan bağlanmalı.
        let track = TrackRef::new("misspelled artist", "wrong title")
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

    /// Eşdeğer adaylar arasından seçim "tam isabet" sayılmaz (D-045).
    #[tokio::test]
    async fn a_tie_is_reported_and_never_claims_an_exact_match() {
        // Aynı sanatçı, aynı başlık, üç ayrı kayıt — gerçek MusicBrainz'de
        // `Radiohead — Creep` tam olarak bunu döndürüyor.
        let lookup = StaticLookup::new(vec![
            candidate(
                "cccccccc-1111-4042-ae91-78d6a3267d01",
                "Nirvana",
                "Lithium",
                None,
            ),
            candidate(
                "aaaaaaaa-1111-4042-ae91-78d6a3267d02",
                "Nirvana",
                "Lithium",
                None,
            ),
            candidate(
                "bbbbbbbb-1111-4042-ae91-78d6a3267d03",
                "Nirvana",
                "Lithium",
                None,
            ),
        ]);
        let resolver = Resolver::new(Arc::new(lookup));
        let track = TrackRef::new("Nirvana", "Lithium");

        let res = resolver.resolve(&track).await.unwrap();
        assert_eq!(res.tied_candidates, 3, "üç aday da aynı skoru almalı");
        assert_eq!(
            res.method,
            ResolveMethod::Fuzzy,
            "beraberlikte `Mbid` (tam isabet) iddia edilemez"
        );
        assert!(
            res.confidence < 0.98,
            "güven tam isabet eşiğinin altında kalmalı: {}",
            res.confidence
        );
        // Belirlenimci: MBID sırası son çare olarak kullanılıyor.
        assert_eq!(
            res.canonical_id,
            CanonicalId::from_mbid(
                &Mbid::parse("aaaaaaaa-1111-4042-ae91-78d6a3267d02").expect("test mbid")
            )
        );
    }

    /// Sunucunun sırası değişse bile aynı kimlik çıkmalı.
    #[tokio::test]
    async fn candidate_order_does_not_change_the_chosen_id() {
        let entries = [
            candidate(
                "cccccccc-1111-4042-ae91-78d6a3267d01",
                "Nirvana",
                "Lithium",
                None,
            ),
            candidate(
                "aaaaaaaa-1111-4042-ae91-78d6a3267d02",
                "Nirvana",
                "Lithium",
                None,
            ),
            candidate(
                "bbbbbbbb-1111-4042-ae91-78d6a3267d03",
                "Nirvana",
                "Lithium",
                None,
            ),
        ];
        let track = TrackRef::new("Nirvana", "Lithium");

        let forward = Resolver::new(Arc::new(StaticLookup::new(entries.to_vec())))
            .resolve(&track)
            .await
            .unwrap();
        let mut reversed = entries.to_vec();
        reversed.reverse();
        let backward = Resolver::new(Arc::new(StaticLookup::new(reversed)))
            .resolve(&track)
            .await
            .unwrap();

        assert_eq!(
            forward.canonical_id, backward.canonical_id,
            "seçim kaynağın gönderdiği sıraya bağlı kalmış"
        );
    }

    /// Notu olmayan kayıt varsayılandır; süre bilgisi ikinci ölçüt.
    #[tokio::test]
    async fn the_plain_recording_wins_over_the_annotated_one() {
        let live = Candidate {
            disambiguation: Some("live, 1994-05-27: Astoria, London, UK".to_owned()),
            // MBID kasten alfabetik olarak önce: not olmasaydı bu kazanırdı.
            ..candidate(
                "00000000-1111-4042-ae91-78d6a3267d01",
                "Nirvana",
                "Lithium",
                None,
            )
        };
        let plain = candidate(
            "ffffffff-1111-4042-ae91-78d6a3267d02",
            "Nirvana",
            "Lithium",
            None,
        );
        let resolver = Resolver::new(Arc::new(StaticLookup::new(vec![live, plain])));

        let res = resolver
            .resolve(&TrackRef::new("Nirvana", "Lithium"))
            .await
            .unwrap();
        let matched = res.matched.expect("aday dönmeli");
        assert_eq!(
            matched.disambiguation, None,
            "notu olan kayıt seçilmemeliydi"
        );
    }

    #[tokio::test]
    async fn summary_counts_every_method() {
        let resolver = Resolver::new(Arc::new(catalog()));
        let tracks = vec![
            TrackRef::new("Radiohead", "Creep").with_duration_ms(Some(238_000)),
            TrackRef::new("Radiohead", "Creep").with_duration_ms(Some(238_000)),
            TrackRef::new("Unknown", "Track"),
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
