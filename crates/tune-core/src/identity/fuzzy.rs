//! Bulanık eşleşme skoru — kimlik zincirinin üçüncü halkası.
//!
//! Girdi normalize edilmiş sanatçı+başlık+süre; çıktı 0.0–1.0 arası bir güven.
//! Skorun ağırlıkları burada tek yerde durur ki doğruluk kümesi üzerinde
//! ayarlanabilsin.

use crate::identity::normalize::{normalize_artist, normalize_text, variant_markers};

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

/// İki parça tanımı arasındaki benzerlik.
///
/// `duration_*` bilinmiyorsa süre bileşeni devreye girmez — bilinmeyen süreyi
/// uyuşma saymak yanlış eşleşme üretir.
///
/// Metin benzerliğinin **üstünde** iki ayrık kural var; ikisi de "başlık
/// benziyor ama bu aynı kayıt değil" durumunu yakalar:
/// - sanatçı tabanı ([`ARTIST_MIN_SIMILARITY`]) — cover/tribute/karaoke,
/// - varyant uyuşmazlığı — canlı/remix/akustik kayıtlar.
#[must_use]
pub fn similarity(
    artist_a: &str,
    title_a: &str,
    duration_a_ms: Option<u64>,
    artist_b: &str,
    title_b: &str,
    duration_b_ms: Option<u64>,
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
    if variant_markers(&norm_a) != variant_markers(&norm_b) {
        scored *= VARIANT_MISMATCH_PENALTY;
    }
    scored.clamp(0.0, 1.0)
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
        );
        let mismatched = similarity(
            "Daft Punk",
            "Aerodynamic",
            Some(212_000),
            "Daft Punk",
            "Aerodynamic",
            Some(420_000),
        );
        assert!(mismatched < same, "{mismatched} < {same} olmalı");
    }

    #[test]
    fn live_recording_does_not_match_the_studio_take() {
        // D-009: canlı kayıt ayrı bir kayıttır; süre bilinmese bile birleşmemeli.
        let studio = similarity("Radiohead", "Creep", None, "Radiohead", "Creep", None);
        let live = similarity(
            "Radiohead",
            "Creep (Live at Glastonbury)",
            None,
            "Radiohead",
            "Creep",
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
        );
        assert!(score < 0.7, "skor {score}");
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
        );
        let agreeing = similarity(
            "Daft Punk",
            "Aerodynamic",
            Some(212_000),
            "Daft Punk",
            "Aerodynamik",
            Some(212_500),
        );
        assert!(unknown < agreeing, "{unknown} < {agreeing} olmalı");
    }
}
