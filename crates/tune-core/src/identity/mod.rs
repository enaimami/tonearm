//! Kanonik kimlik çözümlemesi.
//!
//! **Değişmez kural #5 — zincir bozulmaz:**
//! ISRC → MusicBrainz ID → bulanık eşleşme (sanatçı+başlık+süre) → AcoustID
//! parmak izi. Her adım bir güven skoru döndürür ve hangi adımın çözdüğü
//! kayıtta durur; "çözüldü" demek yetmez, *nasıl* çözüldüğü ölçülebilir olmalı.

/// AcoustID sorgusu — yalnızca `fingerprint` derlemelerinde.
///
/// Modülün kendisi parmak izini AcoustID'nin beklediği biçime sıkıştırmak
/// zorunda ve o sıkıştırıcı `rusty-chromaprint` içinde. Feature kapalıyken
/// modül **yok**; zincirin 4. halkasını çağıran kod ise
/// [`Resolver::resolve_file`] üstünden geçtiği için derlemeye devam eder ve
/// halkanın neden yok olduğunu [`fingerprint::fingerprint_file`] söyler (K9).
#[cfg(feature = "fingerprint")]
pub mod acoustid;
pub mod fingerprint;
pub mod fuzzy;
pub mod musicbrainz;
pub mod normalize;

use std::future::Future;
use std::path::Path;
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

/// Skorlanmış bir aday ve onu ayırt eden bütün kanıt.
///
/// Skorun yanında `gap` taşınıyor çünkü [`fuzzy::similarity`] süre farkını
/// **bantlara** bölüyor: 0 sn ile 2.9 sn aynı bandın içinde, ikisi de "uyuyor".
/// Bant kararı için doğru, eşitliği kırmak için değil — o bilgi kayboluyordu.
struct Scored {
    candidate: Candidate,
    score: f64,
    /// Sorgunun süresiyle adayın süresi arasındaki fark. Biri bilinmiyorsa
    /// [`u64::MAX`]: "yakınlık iddiasında bulunamıyorum", en sona düşer.
    gap: u64,
}

/// Sorgunun süresine uzaklık; bilinmiyorsa en kötü değer.
///
/// Bilinmeyen süreyi 0 saymak "birebir uyuyor" demek olurdu — bilinmeyeni
/// uyuşma saymanın bu modüldeki üçüncü tekrarı, ve her seferinde yanlış
/// eşleşme üretti.
fn duration_gap(query_ms: Option<u64>, candidate_ms: Option<u64>) -> u64 {
    match (query_ms, candidate_ms) {
        (Some(query), Some(candidate)) => query.abs_diff(candidate),
        _ => u64::MAX,
    }
}

/// Kazananla **gerçekten ayırt edilemeyen** kaç aday var.
///
/// Liste sıralamanın bütün ölçütlerine göre sıralı olduğu için ayırt
/// edilemeyenler bir önek oluşturuyor: aynı skor, aynı [`tiebreak_rank`], aynı
/// süre farkı. Üçünde de eşit olan iki adayı ayıran tek şey MBID sırası kalır
/// — ve o bir kanıt değil, sadece sabit bir seçim.
///
/// Yalnızca skora bakmak yetmiyordu (D-046): canlı katalogda `Radiohead —
/// Creep` araması 9–10 adayı aynı skorda bırakıyor ve `Şebnem Ferah — Sil
/// Baştan` üç adayı — süreleri 309, 313, 315 sn — aynı skorda. İlkinde ortada
/// gerçekten kanıt yok; ikincisinde kanıt var ve bantların altında kalmıştı.
fn count_tied(scored: &[Scored]) -> usize {
    let Some(top) = scored.first() else {
        return 1;
    };
    let top_rank = tiebreak_rank(&top.candidate);
    scored
        .iter()
        .take_while(|entry| {
            (top.score - entry.score).abs() <= SCORE_EPSILON
                && tiebreak_rank(&entry.candidate) == top_rank
                && entry.gap == top.gap
        })
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

/// Bir parmak izi eşleşmesi: aday kayıt + servisin kendi güveni.
#[derive(Debug, Clone, PartialEq)]
pub struct FingerprintCandidate {
    pub candidate: Candidate,
    /// AcoustID'nin **parmak izi örtüşme** skoru, 0–1.
    ///
    /// Metin benzerliği değil: [`fuzzy::similarity`] sanatçı ve başlığı
    /// karşılaştırır, bu sayı ise sesin kendisini. İkisini aynı ölçekte
    /// görüp karıştırmamak için ayrı bir alanda duruyor — etiketi bozuk bir
    /// dosyada metin skoru sıfıra yakınken bu skor 0.99 olabilir, ve doğru
    /// olan odur.
    pub score: f64,
}

/// Ses parmak izinden kayıt arayan kaynak (AcoustID).
///
/// [`MetadataLookup`]'tan ayrı bir trait çünkü girdisi bambaşka: o metin
/// alır, bu ses alır. Aynı trait'e sıkıştırmak, ağa hiç çıkmayan
/// [`OfflineLookup`]'a anlamsız bir metot eklemek olurdu.
pub trait FingerprintLookup: Send + Sync {
    /// Parmak izine karşılık gelen kayıtlar — en güçlü eşleşme başta.
    ///
    /// Boş liste **hata değil**: "bu ses veritabanında yok" geçerli bir
    /// cevaptır ve zincirin bir sonraki adımına (yerel anahtar) geçmek
    /// demektir. Hata yalnızca soruyu **soramadığımız** hâldir (K9).
    fn recordings_by_fingerprint<'a>(
        &'a self,
        fingerprint: &'a fingerprint::Fingerprint,
    ) -> LookupFuture<'a, Vec<FingerprintCandidate>>;
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
    /// Zincirin 4. halkası. `None` = bu çözümleyici parmak izine bakmıyor.
    ///
    /// Opsiyonel çünkü halkanın çalışması iki şeye birden bağlı: elde bir
    /// **dosya** olmasına ve bir AcoustID kaynağının verilmiş olmasına. İçe
    /// aktarılan geçmiş kayıtlarının dosyası yok; onlar için bu alanı
    /// doldurmak boşuna bir bağımlılık taşımak olurdu.
    fingerprint_lookup: Option<Arc<dyn FingerprintLookup>>,
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
            fingerprint_lookup: None,
            config: ResolveConfig::default(),
        }
    }

    #[must_use]
    pub fn with_config(mut self, config: ResolveConfig) -> Self {
        self.config = config;
        self
    }

    /// Zincirin 4. halkasını bağlar.
    ///
    /// Yalnızca [`Self::resolve_file`] tarafından kullanılır — [`Self::resolve`]
    /// bir dosya görmediği için bu kaynağa hiç dokunmaz.
    #[must_use]
    pub fn with_fingerprint_lookup(mut self, lookup: Arc<dyn FingerprintLookup>) -> Self {
        self.fingerprint_lookup = Some(lookup);
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
        let mut scored: Vec<Scored> = candidates
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
                let gap = duration_gap(track.duration_ms, candidate.duration_ms);
                Scored {
                    candidate,
                    score,
                    gap,
                }
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
        scored.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| tiebreak_rank(&right.candidate).cmp(&tiebreak_rank(&left.candidate)))
                // Süreye yakınlık: skorun bantları içinde yutulan kanıt
                // (D-046). Küçük fark kazanır.
                .then_with(|| left.gap.cmp(&right.gap))
                // Son çare: MBID sırası. Keyfi ama **sabit** — sabit olması
                // keyfi olmamasından daha önemli.
                .then_with(|| {
                    left.candidate
                        .mbid
                        .as_str()
                        .cmp(right.candidate.mbid.as_str())
                })
        });
        let tied = count_tied(&scored);
        let best = scored.into_iter().next();

        if let Some(Scored {
            candidate, score, ..
        }) = best
        {
            if score >= self.config.min_fuzzy_confidence {
                if tied > 1 {
                    // **Beraberlikte otorite iddia edilmiyor** (D-046).
                    //
                    // D-045 buradan bir MBID döndürüyor, yalnızca güveni
                    // kırpıyor ve kaç adayın berabere olduğunu söylüyordu.
                    // Canlı koşum bunun yetmediğini gösterdi: MusicBrainz
                    // aramayı birden çok indeks kopyasından sunuyor ve aynı
                    // sorgu art arda iki kez **hiç kesişmeyen** 25'er aday
                    // döndürebiliyor. Belirlenimci sıralama bu kümenin
                    // *içinde* çalışıyor ama küme her seferinde başka, yani
                    // seçilen MBID hangi kopyanın cevapladığına bağlı
                    // kalıyordu. Kimlik katmanında bu kabul edilemez.
                    //
                    // Ölçülen: `Radiohead — Creep` (süresiz) her koşumda 9–10
                    // adayı aynı skorda **ve** aynı rütbede bırakıyor. Ortada
                    // ayırt edici kanıt yok; olmayan kanıttan kimlik üretmek
                    // yerine yerel anahtara düşüyoruz. Kaybedilen şey
                    // `authoritative_ratio`, kazanılan şey kimliğin **aynı
                    // kalması** — ikincisi olmadan birincisi anlamsız.
                    tracing::debug!(
                        track = track.display_name(),
                        score,
                        berabere = tied,
                        "eşdeğer adaylar ayırt edilemedi; otorite iddia edilmiyor"
                    );
                    return Ok(self.ambiguous(track, tied));
                }
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
                    tied_candidates: 1,
                });
            }
            tracing::debug!(
                track = track.display_name(),
                score,
                esik = self.config.min_fuzzy_confidence,
                "en iyi aday eşiği geçemedi"
            );
        }

        // 4. Halka (AcoustID) buradan çağrılmıyor ve çağrılamaz: parmak izi
        // sesin kendisine bakar, `TrackRef`'in elinde ses yok. O halka
        // [`Self::resolve_file`] üstünden, dosya varken çalışır.
        Ok(self.ambiguous(track, 1))
    }

    /// Zincirin otoriteye bağlanamadığı hâl: yerel anahtar.
    ///
    /// `tied` 1'den büyükse sebep "hiç aday yok" değil "adaylar ayırt
    /// edilemedi" — ikisi aynı kimliği üretiyor ama **aynı tanı değil** ve
    /// rapor bunu ayırabilmeli (K9).
    fn ambiguous(&self, track: &TrackRef, tied: usize) -> Resolution {
        let _ = self;
        Resolution {
            canonical_id: CanonicalId::from_local_key(&normalize::track_key(
                &track.artist,
                &track.title,
            )),
            method: ResolveMethod::LocalKey,
            confidence: 0.2,
            matched: None,
            tied_candidates: tied,
        }
    }

    /// Bir **ses dosyasını** zincirin dördünden birden geçirir.
    ///
    /// [`Self::resolve`]'dan farkı tek şey: elde dosya var. Bu, iki şey
    /// kazandırıyor — üstveri dosyanın kendi etiketlerinden okunuyor
    /// (export'un verdiğine mahkûm değiliz) ve metin halkaları sonuçsuz
    /// kalırsa **ses** sorulabiliyor.
    ///
    /// Sıra bozulmuyor (K6): önce etiketlerden ISRC/MBID/bulanık, ancak üçü
    /// de `LocalKey`'e düşerse parmak izi. Parmak izi en son çünkü en pahalı
    /// olanı — dosyanın tamamı çözülür — ve ilk üçü çalıştığında ona gerek
    /// yoktur.
    ///
    /// # Errors
    /// Dosya açılamaz/okunamazsa, ya da parmak izi kaynağına **soru
    /// sorulamazsa**. Parmak izinin *üretilememesi* (dosya çok kısa, paketler
    /// bozuk) hata değil: sebebi loglanır ve zincir yerel anahtarla biter —
    /// metin halkalarının bulduğu şeyi bir ses kusuru yüzünden kaybetmeyiz.
    pub async fn resolve_file(&self, path: &Path) -> Result<Resolution> {
        let (track, from_tags) = crate::provider::local::read_track(path)?;
        if !from_tags {
            // Etiket yok: metin halkalarının girdisi dosya adından türetildi.
            // Zincirin buraya kadar gelip parmak izine düşmesi **beklenen**
            // durum, sürpriz değil.
            tracing::debug!(dosya = %path.display(), "etiket yok, üstveri dosya adından");
        }

        let text = self.resolve(&track).await?;
        if text.method != ResolveMethod::LocalKey {
            return Ok(text);
        }

        let Some(lookup) = self.fingerprint_lookup.as_ref() else {
            tracing::debug!(
                dosya = %path.display(),
                "zincir yerel anahtara düştü ve parmak izi kaynağı bağlı değil"
            );
            return Ok(text);
        };

        let print = match fingerprint::fingerprint_file(path) {
            Ok(print) => print,
            Err(err) => {
                // Sessizce yutmuyoruz: hangi dosyanın neden parmak izi
                // veremediği görünür kalmalı, yoksa "AcoustID hiç eşleşme
                // bulmuyor" diye yanlış yerde aranır (K9).
                tracing::warn!(
                    dosya = %path.display(),
                    hata = %err.chain_text(),
                    "parmak izi üretilemedi, zincir yerel anahtarla bitiyor"
                );
                return Ok(text);
            }
        };

        // Buradaki hata **propagate ediliyor** ve bu bilinçli bir ayrım:
        // parmak izini üretememek dosyanın bir özelliğidir, AcoustID'ye
        // soramamak ise bir yapılandırma ya da ağ kusurudur. İkincisini
        // yutmak, anahtarı ayarlanmamış bir kurulumu "hiçbir şey eşleşmiyor"
        // diye raporlardı.
        let matches = lookup.recordings_by_fingerprint(&print).await?;
        let Some(best) = matches.first() else {
            tracing::debug!(dosya = %path.display(), "parmak izi tanınmadı");
            return Ok(text);
        };

        let tied = matches
            .iter()
            .take_while(|found| (best.score - found.score).abs() <= SCORE_EPSILON)
            .count()
            .max(1);
        // Metin tarafındaki kuralın aynısı (D-045): eşdeğerler arasından
        // yapılan seçim tam isabet diye raporlanamaz.
        let confidence = if tied > 1 {
            best.score
                .min(self.config.exact_match_confidence - AMBIGUITY_MARGIN)
        } else {
            best.score
        };

        Ok(Resolution {
            canonical_id: CanonicalId::from_mbid(&best.candidate.mbid),
            method: ResolveMethod::Fingerprint,
            confidence,
            matched: Some(best.candidate.clone()),
            tied_candidates: tied,
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

    /// Eşdeğer adaylar arasından **seçim yapılmaz** (D-046).
    ///
    /// D-045 buradan bir MBID döndürüyor, yalnızca güveni kırpıyordu. Canlı
    /// koşum bunun yetmediğini gösterdi: MusicBrainz aramayı birden çok indeks
    /// kopyasından sunuyor ve aynı sorgu art arda **hiç kesişmeyen** iki aday
    /// kümesi döndürebiliyor. Küme içinde belirlenimci olmak, küme değiştiğinde
    /// kimliği koruyamıyor. Ayırt edici kanıt yoksa otorite iddia edilmiyor.
    #[tokio::test]
    async fn equivalent_candidates_yield_no_authority_at_all() {
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
        assert_eq!(res.tied_candidates, 3, "üç aday da ayırt edilemez olmalı");
        assert_eq!(
            res.method,
            ResolveMethod::LocalKey,
            "ayırt edici kanıt yokken otorite iddia edilemez"
        );
        assert!(
            res.matched.is_none(),
            "seçilmeyen bir aday eşleşme diye raporlanamaz"
        );
        assert_eq!(
            res.canonical_id.kind(),
            crate::ids::CanonicalKind::Local,
            "kimlik yerel anahtardan gelmeli: {}",
            res.canonical_id
        );
    }

    /// Yerel anahtara düşmenin **iki ayrı sebebi** ayırt edilebilmeli (K9).
    ///
    /// "Hiç aday yok" ile "adaylar ayırt edilemedi" aynı kimliği üretiyor ama
    /// aynı tanı değil: ilki daha iyi üstveriyle çözülür, ikincisi daha iyi
    /// **ayırt edici** bilgiyle (süre, ISRC, parmak izi).
    #[tokio::test]
    async fn an_ambiguous_fallback_is_distinguishable_from_an_empty_one() {
        let empty = Resolver::new(Arc::new(OfflineLookup))
            .resolve(&TrackRef::new("Radiohead", "Creep"))
            .await
            .unwrap();
        assert_eq!(empty.method, ResolveMethod::LocalKey);
        assert_eq!(empty.tied_candidates, 1, "aday hiç yoktu");

        let ambiguous = Resolver::new(Arc::new(StaticLookup::new(vec![
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
        ])))
        .resolve(&TrackRef::new("Nirvana", "Lithium"))
        .await
        .unwrap();
        assert_eq!(ambiguous.method, ResolveMethod::LocalKey);
        assert_eq!(ambiguous.tied_candidates, 2, "iki aday ayırt edilemedi");
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

    /// Zincirin 4. halkası — yalnızca parmak izi üretilebilen derlemelerde.
    #[cfg(feature = "fingerprint")]
    mod chain_with_audio {
        use super::*;
        use std::sync::atomic::{AtomicUsize, Ordering};

        fn fixture(name: &str) -> std::path::PathBuf {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/audio")
                .join(name)
        }

        /// Sabit cevap veren parmak izi kaynağı; kaç kez sorulduğunu sayar.
        struct FakeFingerprints {
            answer: Vec<FingerprintCandidate>,
            calls: AtomicUsize,
        }

        impl FakeFingerprints {
            fn new(answer: Vec<FingerprintCandidate>) -> Self {
                Self {
                    answer,
                    calls: AtomicUsize::new(0),
                }
            }
        }

        impl FingerprintLookup for FakeFingerprints {
            fn recordings_by_fingerprint<'a>(
                &'a self,
                _fingerprint: &'a fingerprint::Fingerprint,
            ) -> LookupFuture<'a, Vec<FingerprintCandidate>> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Box::pin(std::future::ready(Ok(self.answer.clone())))
            }
        }

        fn found(mbid: &str, score: f64) -> FingerprintCandidate {
            FingerprintCandidate {
                candidate: candidate(mbid, "Radiohead", "Creep", Some(238_000)),
                score,
            }
        }

        /// Metin halkaları boşa çıkınca ses sorulur ve kimlik oradan gelir.
        #[tokio::test]
        async fn the_fingerprint_link_answers_when_the_text_links_cannot() {
            let prints = Arc::new(FakeFingerprints::new(vec![found(
                "b1a9c0e9-d987-4042-ae91-78d6a3267d69",
                0.99,
            )]));
            let resolver = Resolver::new(Arc::new(catalog()))
                .with_fingerprint_lookup(Arc::clone(&prints) as Arc<dyn FingerprintLookup>);

            // Fixture'ın etiketi yok ve adı katalogda hiçbir şeye uymuyor:
            // üç metin halkası da çaresiz.
            let res = resolver
                .resolve_file(&fixture("fingerprint_sample.flac"))
                .await
                .expect("çözümleme");

            assert_eq!(res.method, ResolveMethod::Fingerprint);
            assert!((res.confidence - 0.99).abs() < 1e-9);
            assert_eq!(prints.calls.load(Ordering::SeqCst), 1);
        }

        /// Sıra bozulmuyor: etiketler cevabı verdiyse sese hiç sorulmaz.
        ///
        /// Parmak izi en pahalı halka (dosyanın tamamı çözülür); onu gereksiz
        /// yere çalıştırmak sessiz bir maliyet olurdu.
        #[tokio::test]
        async fn a_tagged_file_never_reaches_the_fingerprint_link() {
            let prints = Arc::new(FakeFingerprints::new(vec![found(
                "b1a9c0e9-d987-4042-ae91-78d6a3267d69",
                0.99,
            )]));
            let catalog = StaticLookup::new(vec![candidate(
                "d3c9e2a1-1111-4042-ae91-78d6a3267d71",
                "Test Artist",
                "Sine 440 ünïcode",
                None,
            )]);
            let resolver = Resolver::new(Arc::new(catalog))
                .with_fingerprint_lookup(Arc::clone(&prints) as Arc<dyn FingerprintLookup>);

            let res = resolver
                .resolve_file(&fixture("tagged.flac"))
                .await
                .expect("çözümleme");

            assert_ne!(res.method, ResolveMethod::Fingerprint);
            assert_eq!(
                prints.calls.load(Ordering::SeqCst),
                0,
                "sese sorulmamalıydı"
            );
        }

        /// Parmak izi **üretilemezse** metin tarafının bulduğu kaybolmaz.
        ///
        /// 2 saniyelik fixture eşiğin altında: zincir yerel anahtarla biter,
        /// hata döndürmez.
        #[tokio::test]
        async fn a_file_that_cannot_be_fingerprinted_still_returns_a_local_key() {
            let prints = Arc::new(FakeFingerprints::new(vec![found(
                "b1a9c0e9-d987-4042-ae91-78d6a3267d69",
                0.99,
            )]));
            let resolver = Resolver::new(Arc::new(catalog()))
                .with_fingerprint_lookup(Arc::clone(&prints) as Arc<dyn FingerprintLookup>);

            let res = resolver
                .resolve_file(&fixture("Test Artist - Mp3 Track.mp3"))
                .await
                .expect("kısa dosya zinciri düşürmemeli");

            assert_eq!(res.method, ResolveMethod::LocalKey);
            assert_eq!(
                prints.calls.load(Ordering::SeqCst),
                0,
                "parmak izi yokken servise sorulmamalı"
            );
        }

        /// Ses tanınmadıysa bu hata değil: zincir yerel anahtarla biter.
        #[tokio::test]
        async fn an_unrecognised_fingerprint_is_absence_not_failure() {
            let prints = Arc::new(FakeFingerprints::new(Vec::new()));
            let resolver = Resolver::new(Arc::new(catalog()))
                .with_fingerprint_lookup(prints as Arc<dyn FingerprintLookup>);

            let res = resolver
                .resolve_file(&fixture("fingerprint_sample.flac"))
                .await
                .expect("çözümleme");

            assert_eq!(res.method, ResolveMethod::LocalKey);
        }

        /// Parmak izi tarafında da beraberlik "tam isabet" diye raporlanamaz
        /// (D-045'in üçüncü dersi, bu kez ses tarafında).
        #[tokio::test]
        async fn a_tie_on_the_audio_side_is_reported_and_capped() {
            let prints = Arc::new(FakeFingerprints::new(vec![
                found("b1a9c0e9-d987-4042-ae91-78d6a3267d69", 0.99),
                found("c2b8d1f0-1234-4042-ae91-78d6a3267d70", 0.99),
            ]));
            let resolver = Resolver::new(Arc::new(catalog()))
                .with_fingerprint_lookup(prints as Arc<dyn FingerprintLookup>);

            let res = resolver
                .resolve_file(&fixture("fingerprint_sample.flac"))
                .await
                .expect("çözümleme");

            assert_eq!(res.tied_candidates, 2);
            assert!(
                res.confidence < ResolveConfig::default().exact_match_confidence,
                "{}",
                res.confidence
            );
        }

        /// Kaynak bağlı değilse zincir üç halkayla biter — çökmez.
        #[tokio::test]
        async fn without_a_fingerprint_source_the_chain_simply_ends_early() {
            let resolver = Resolver::new(Arc::new(catalog()));
            let res = resolver
                .resolve_file(&fixture("fingerprint_sample.flac"))
                .await
                .expect("çözümleme");
            assert_eq!(res.method, ResolveMethod::LocalKey);
        }

        /// Servise **sorulamaması** yutulmaz: yapılandırma kusuru görünmeli.
        #[tokio::test]
        async fn a_lookup_failure_is_propagated_not_swallowed() {
            struct Broken;
            impl FingerprintLookup for Broken {
                fn recordings_by_fingerprint<'a>(
                    &'a self,
                    _fingerprint: &'a fingerprint::Fingerprint,
                ) -> LookupFuture<'a, Vec<FingerprintCandidate>> {
                    Box::pin(std::future::ready(Err(crate::error::Error::new(
                        crate::diag::Stage::IdentityResolve,
                        crate::error::ErrorKind::InvalidInput {
                            detail: "anahtar yok".to_owned(),
                        },
                    ))))
                }
            }

            let resolver = Resolver::new(Arc::new(catalog()))
                .with_fingerprint_lookup(Arc::new(Broken) as Arc<dyn FingerprintLookup>);
            let err = resolver
                .resolve_file(&fixture("fingerprint_sample.flac"))
                .await
                .unwrap_err();
            assert!(
                err.chain_text().contains("anahtar yok"),
                "{}",
                err.chain_text()
            );
        }
    }
}
