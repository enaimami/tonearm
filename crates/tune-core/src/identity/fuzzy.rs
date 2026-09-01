//! Bulanık eşleşme skoru — kimlik zincirinin üçüncü halkası.
//!
//! Girdi normalize edilmiş sanatçı+başlık+süre; çıktı 0.0–1.0 arası bir güven.
//! Skorun ağırlıkları burada tek yerde durur ki doğruluk kümesi üzerinde
//! ayarlanabilsin.

use crate::identity::normalize::{
    VARIANT_MARKERS, normalize_artist, normalize_text, variant_markers,
};

/// Başlık, sanatçıdan daha ayırt edicidir: aynı sanatçının 300 parçası olur.
const TITLE_WEIGHT: f64 = 0.6;
/// Sanatçının ağırlığı.
const ARTIST_WEIGHT: f64 = 0.4;
/// Bu farkın altındaki süreler "aynı" sayılır (farklı master'lar, sessizlik payı).
const DURATION_MATCH_MS: u64 = 3_000;
/// Bu farkın üstündeki süreler ciddi şüphe: radio edit / uzun versiyon.
const DURATION_MISMATCH_MS: u64 = 15_000;
/// Süre uyuşması skora eklenen pay.
const DURATION_BONUS: f64 = 0.08;
/// Süre çelişmesi skoru bu oranla çarpar.
const DURATION_PENALTY: f64 = 0.7;
/// Sanatçı benzerliği bunun altındaysa aynı kayıt olamaz.
///
/// Aynı başlığı taşıyan farklı sanatçılar çok yaygın (cover, tribute,
/// karaoke, adaş parça). Başlık ağırlığı tek başına 0.6 olduğu için
/// tamamen yabancı bir sanatçı bile eşiğe dayanabiliyordu (D-009).
const ARTIST_MIN_SIMILARITY: f64 = 0.7;
/// Sanatçı tabanının altında kalan skor bu oranla çarpar.
const ARTIST_MISMATCH_PENALTY: f64 = 0.5;
/// Varyant etiketleri uyuşmayan skor bu oranla çarpar.
///
/// `Creep` ile `Creep (Live)` aynı şarkıdır ama aynı **kayıt** değildir.
const VARIANT_MISMATCH_PENALTY: f64 = 0.5;
/// Bundan kısa kelimeler ayırt edici sayılmaz.
///
/// `at`, `in`, `de`, `ve` gibi bağlaçlar iki metinde de bulunmayabilir ve
/// bulunmamaları hiçbir şey söylemez. `nyc` (3) ayırt edicidir, `at` (2) değil.
const DETAIL_MIN_LEN: usize = 3;

/// İki parça tanımı arasındaki benzerlik.
///
/// `duration_*` bilinmiyorsa süre bileşeni devreye girmez — bilinmeyen süreyi
/// uyuşma saymak yanlış eşleşme üretir.
///
/// Metin benzerliğinin **üstünde** iki ayrık kural var; ikisi de "başlık
/// benziyor ama bu aynı kayıt değil" durumunu yakalar:
/// - sanatçı tabanı ([`ARTIST_MIN_SIMILARITY`]) — cover/tribute/karaoke,
/// - varyant uyuşmazlığı — canlı/remix/akustik kayıtlar.
///
/// # `context_b` — varyant bilgisi başlıkta olmayabilir
///
/// Asimetrik ve bilerek öyle: `a` kullanıcının kaydıdır, elinde ne varsa
/// başlıktadır. `b` bir üstveri kataloğundan gelir ve MusicBrainz canlı
/// kayıtları **başlıkta değil** `disambiguation` alanında işaretler —
/// katalogda üç ayrı `Creep`'in ikisi canlıdır ve üçünün de başlığı düpedüz
/// `Creep`'tir. Bu alan okunmadığında ölçülen sonuç şuydu (D-045, gerçek
/// yanıt üzerinde): 1994 Astoria kaydı, süresi stüdyo kaydına 12 sn yakın
/// olduğu için **1.00 güvenle "tam isabet"** sayılıyordu. Yani zincirin
/// en emin göründüğü yerde yanlış kayda bağlanıyordu.
#[must_use]
pub fn similarity(
    artist_a: &str,
    title_a: &str,
    duration_a_ms: Option<u64>,
    artist_b: &str,
    title_b: &str,
    duration_b_ms: Option<u64>,
    context_b: Option<&str>,
) -> f64 {
    let artist_sim = strsim::jaro_winkler(&normalize_artist(artist_a), &normalize_artist(artist_b));
    let (norm_a, norm_b) = (normalize_text(title_a), normalize_text(title_b));
    let title_sim = strsim::jaro_winkler(&norm_a, &norm_b);
    let base = ARTIST_WEIGHT.mul_add(artist_sim, TITLE_WEIGHT * title_sim);

    let mut scored = match (duration_a_ms, duration_b_ms) {
        (Some(a), Some(b)) => {
            let diff = a.abs_diff(b);
            if diff <= DURATION_MATCH_MS {
                base + DURATION_BONUS
            } else if diff >= DURATION_MISMATCH_MS {
                base * DURATION_PENALTY
            } else {
                base
            }
        }
        _ => base,
    };

    if artist_sim < ARTIST_MIN_SIMILARITY {
        scored *= ARTIST_MISMATCH_PENALTY;
    }
    let markers_a = variant_markers(&norm_a);
    let markers_b = merged_variant_markers(&norm_b, context_b);
    if markers_a != markers_b {
        scored *= VARIANT_MISMATCH_PENALTY;
    } else if !markers_a.is_empty() && unmatched_detail(&norm_a, &norm_b, context_b) {
        // İkisi de canlı kayıt, ama farklı geceler: `Creep (Live at
        // Glastonbury)` ile `Creep` + `live, 1994-05-27: Astoria, London, UK`
        // aynı işareti taşır, aynı performans değildir (D-010: kanonik kimlik
        // kayıt düzeyindedir). Bu ayrım katalogda gerçek canlı kayıtlar
        // olmadan görünmüyordu; onlar eklenince ortaya çıktı (D-045).
        scored *= VARIANT_MISMATCH_PENALTY;
    }
    scored.clamp(0.0, 1.0)
}

/// Kullanıcının verdiği ayırt edici bir ayrıntı adayda karşılık buluyor mu?
///
/// Kullanıcı `Live at Glastonbury` yazmışsa `glastonbury` bir iddiadır ve
/// adayın metninde (başlık **veya** ayırt edici not) geçmiyorsa aday o kayıt
/// değildir. Yalnızca `Live` yazmışsa hiçbir iddia yok — o zaman en iyi canlı
/// aday kabul edilir.
///
/// Tek yönlü ve bilerek öyle: adayın fazladan taşıdığı ayrıntı (`1994-05-27`)
/// kullanıcının onu bilmediği anlamına gelir, çelişki değil.
fn unmatched_detail(norm_a: &str, norm_b: &str, context_b: Option<&str>) -> bool {
    let haystack = match context_b {
        Some(context) => format!("{norm_b} {}", normalize_text(context)),
        None => norm_b.to_owned(),
    };
    let known: Vec<&str> = haystack.split_whitespace().collect();
    norm_a
        .split_whitespace()
        .filter(|word| word.chars().count() >= DETAIL_MIN_LEN)
        // Varyant etiketlerinin kendisi ayrıntı değil, sınıflandırma.
        .filter(|word| !VARIANT_MARKERS.contains(word))
        .any(|word| !known.contains(&word))
}

/// Adayın varyant işaretleri: başlığında geçenler **ve** bağlam alanındakiler.
///
/// Birleşim alınıyor, bağlam başlığın yerine geçmiyor: bir kayıt hem
/// `Creep (Acoustic)` başlığını hem `live, 2003` notunu taşıyabilir.
fn merged_variant_markers(normalized_title: &str, context: Option<&str>) -> Vec<&'static str> {
    let mut markers = variant_markers(normalized_title);
    if let Some(context) = context {
        for marker in variant_markers(&normalize_text(context)) {
            if !markers.contains(&marker) {
                markers.push(marker);
            }
        }
        markers.sort_unstable();
    }
    markers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_tracks_score_at_the_top() {
        let score = similarity(
            "Radiohead",
            "Creep",
            Some(238_000),
            "radiohead",
            "Creep (Remastered)",
            Some(238_500),
            None,
        );
        assert!(score > 0.99, "skor {score}");
    }

    #[test]
    fn different_tracks_by_same_artist_score_low() {
        let score = similarity(
            "Radiohead",
            "Creep",
            Some(238_000),
            "Radiohead",
            "Karma Police",
            Some(261_000),
            None,
        );
        assert!(score < 0.75, "skor {score}");
    }

    #[test]
    fn duration_mismatch_pulls_the_score_down() {
        let same = similarity(
            "Daft Punk",
            "Aerodynamic",
            None,
            "Daft Punk",
            "Aerodynamic",
            None,
            None,
        );
        let mismatched = similarity(
            "Daft Punk",
            "Aerodynamic",
            Some(212_000),
            "Daft Punk",
            "Aerodynamic",
            Some(420_000),
            None,
        );
        assert!(mismatched < same, "{mismatched} < {same} olmalı");
    }

    #[test]
    fn live_recording_does_not_match_the_studio_take() {
        // D-009: canlı kayıt ayrı bir kayıttır; süre bilinmese bile birleşmemeli.
        let studio = similarity("Radiohead", "Creep", None, "Radiohead", "Creep", None, None);
        let live = similarity(
            "Radiohead",
            "Creep (Live at Glastonbury)",
            None,
            "Radiohead",
            "Creep",
            None,
            None,
        );
        assert!(studio > 0.99, "stüdyo skoru {studio}");
        assert!(live < 0.7, "canlı skoru {live}");
    }

    #[test]
    fn two_live_takes_are_not_penalised_against_each_other() {
        // Ceza varyant *uyuşmazlığında*; iki canlı kayıt birbirine ceza almaz.
        let score = similarity(
            "Radiohead",
            "Creep (Live)",
            None,
            "Radiohead",
            "Creep - Live",
            None,
            None,
        );
        assert!(score > 0.88, "skor {score}");
    }

    #[test]
    fn a_different_artist_cannot_win_on_the_title_alone() {
        // D-009: cover/tribute/karaoke — başlık birebir, sanatçı yabancı.
        let score = similarity(
            "Karaoke Version",
            "Creep",
            Some(238_000),
            "Radiohead",
            "Creep",
            Some(238_000),
            None,
        );
        assert!(score < 0.7, "skor {score}");
    }

    /// D-045: gerçek katalogda canlı kaydın **tek** işareti ayırt edici nottur.
    #[test]
    fn a_live_take_marked_only_in_the_note_loses_to_the_studio_take() {
        // İki aday da başlığı düz `Creep`; fark yalnızca notta.
        let studio = similarity("Radiohead", "Creep", None, "Radiohead", "Creep", None, None);
        let live = similarity(
            "Radiohead",
            "Creep",
            None,
            "Radiohead",
            "Creep",
            None,
            Some("live, 1994-05-27: Astoria, London, UK"),
        );
        assert!(studio > 0.99, "stüdyo skoru {studio}");
        assert!(
            live < studio,
            "not okunmadığında ikisi de 1.00 alıyordu: {live} < {studio} olmalı"
        );
    }

    /// İki farklı canlı kayıt aynı kayıt değildir (D-010).
    #[test]
    fn two_different_live_nights_do_not_merge() {
        let score = similarity(
            "Radiohead",
            "Creep (Live at Glastonbury)",
            None,
            "Radiohead",
            "Creep",
            None,
            Some("live, 1994-05-27: Astoria, London, UK"),
        );
        assert!(score < 0.7, "farklı geceler birleşmemeli: {score}");
    }

    /// Ama kullanıcı yalnızca "Live" dediyse hiçbir iddiada bulunmamıştır.
    #[test]
    fn a_bare_live_query_accepts_the_best_live_candidate() {
        let score = similarity(
            "Radiohead",
            "Creep (Live)",
            None,
            "Radiohead",
            "Creep",
            None,
            Some("live, 1994-05-27: Astoria, London, UK"),
        );
        assert!(score > 0.88, "eşiği geçmeli: {score}");
    }

    /// Adayın fazladan bildiği ayrıntı çelişki değil: kullanıcı bilmiyordur.
    #[test]
    fn extra_detail_on_the_candidate_side_is_not_a_contradiction() {
        let score = similarity(
            "Pink Floyd",
            "Wish You Were Here - Live",
            None,
            "Pink Floyd",
            "Wish You Were Here",
            None,
            Some("live, 1994-10-20: Earls Court, London"),
        );
        assert!(score > 0.88, "skor {score}");
    }

    #[test]
    fn unknown_duration_is_not_treated_as_agreement() {
        // Tam isabet eden bir çift seçilirse skor zaten 1.0'a dayanır ve bonus
        // görünmez; bu yüzden başlığı kasten hafifçe kaydırıyoruz.
        let unknown = similarity(
            "Daft Punk",
            "Aerodynamic",
            None,
            "Daft Punk",
            "Aerodynamik",
            Some(212_000),
            None,
        );
        let agreeing = similarity(
            "Daft Punk",
            "Aerodynamic",
            Some(212_000),
            "Daft Punk",
            "Aerodynamik",
            Some(212_500),
            None,
        );
        assert!(unknown < agreeing, "{unknown} < {agreeing} olmalı");
    }
}
