//! Kimlik çözümlemesinin doğruluk ölçümü.
//!
//! `fixtures/identity/cases.json` etiketli doğruluk kümesidir. Bu testin
//! bastığı oran projenin en önemli metriğidir: eşleştirmeye dokunan her
//! değişiklikten sonra buradaki sayıya bak.
//!
//! Ağa çıkılmaz — katalog dosyanın içindedir.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use serde::Deserialize;
use tonearm_core::identity::{Candidate, ResolveMethod, Resolver, StaticLookup};
use tonearm_core::ids::{CanonicalId, Isrc, Mbid};
use tonearm_core::model::TrackRef;

/// Kabul edilen en düşük doğruluk. Oran yükseldikçe bu da yükselir; asla
/// düşürülerek "test geçsin" yapılmaz — düşüş bir gerilemedir (D-009).
const ACCURACY_FLOOR: f64 = 0.97;

#[derive(Debug, Deserialize)]
struct Dataset {
    catalog: Vec<CatalogEntry>,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct CatalogEntry {
    mbid: String,
    artist: String,
    title: String,
    duration_ms: Option<u64>,
    isrc: Option<String>,
    /// MusicBrainz'in ayırt edici notu. Gerçek katalogda canlı kayıtların
    /// **tek** işareti budur; başlık düz kalır (D-045).
    #[serde(default)]
    disambiguation: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Case {
    /// Vaka sınıfı (`live`, `cover`, `turkish`, ...). Oran sınıf bazında da basılır.
    class: String,
    note: String,
    artist: String,
    title: String,
    duration_ms: Option<u64>,
    #[serde(default)]
    isrc: Option<String>,
    /// `None` ise: hiçbir otoriteye bağlanmamalı (yerel anahtara düşmeli).
    expect_mbid: Option<String>,
    /// Verilmişse zincirin bu halkasının çözmesi beklenir.
    #[serde(default)]
    expect_method: Option<String>,
}

impl Case {
    fn expectation(&self) -> String {
        match (&self.expect_mbid, &self.expect_method) {
            (Some(mbid), _) => mbid.clone(),
            (None, Some(method)) => format!("otorite: {method}"),
            (None, None) => "eşleşme yok".to_owned(),
        }
    }

    /// Çözümleme bu vakanın etiketiyle uyuşuyor mu?
    fn is_satisfied_by(&self, resolution: &tonearm_core::identity::Resolution) -> bool {
        if let Some(method) = &self.expect_method {
            if resolution.method.as_str() != method {
                return false;
            }
        }
        match &self.expect_mbid {
            Some(mbid) => {
                let want = CanonicalId::from_mbid(
                    &Mbid::parse(mbid).expect("beklenen mbid geçerli olmalı"),
                );
                resolution.canonical_id == want
            }
            // Yöntem beklentisi verilmişse onu zaten kontrol ettik; verilmemişse
            // "hiçbir otoriteye bağlanmamalı" demektir.
            None if self.expect_method.is_some() => true,
            None => resolution.method == ResolveMethod::LocalKey,
        }
    }
}

fn load() -> Dataset {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/identity/cases.json"
    );
    let body = std::fs::read(path).unwrap_or_else(|err| panic!("{path} okunamadı: {err}"));
    serde_json::from_slice(&body).expect("cases.json geçerli olmalı")
}

#[tokio::test]
async fn identity_accuracy_over_labelled_cases() {
    let dataset = load();
    let lookup = StaticLookup::new(
        dataset
            .catalog
            .iter()
            .map(|entry| Candidate {
                mbid: Mbid::parse(&entry.mbid).expect("katalogdaki mbid geçerli olmalı"),
                artist: entry.artist.clone(),
                title: entry.title.clone(),
                duration_ms: entry.duration_ms,
                isrc: entry.isrc.as_deref().and_then(Isrc::parse),
                disambiguation: entry.disambiguation.clone(),
            })
            .collect(),
    );
    let resolver = Resolver::new(std::sync::Arc::new(lookup));

    let mut correct = 0usize;
    let mut failures = Vec::new();
    // Sınıf bazlı kırılım: toplam oran bir sınıftaki çöküşü gizleyebilir.
    let mut by_class: BTreeMap<&str, (usize, usize)> = BTreeMap::new();

    for case in &dataset.cases {
        let track = TrackRef::new(&case.artist, &case.title)
            .with_duration_ms(case.duration_ms)
            .with_isrc(case.isrc.as_deref().and_then(Isrc::parse));

        let resolution = resolver
            .resolve(&track)
            .await
            .expect("çözümleme hata vermemeli");

        let entry = by_class.entry(case.class.as_str()).or_default();
        entry.1 += 1;
        if case.is_satisfied_by(&resolution) {
            correct += 1;
            entry.0 += 1;
        } else {
            failures.push(format!(
                "  [{}/{}] {} - {} → {} ({}, güven {:.2}), beklenen: {}",
                case.class,
                case.note,
                case.artist,
                case.title,
                resolution.canonical_id,
                resolution.method,
                resolution.confidence,
                case.expectation(),
            ));
        }
    }

    let total = dataset.cases.len();
    let negatives = dataset
        .cases
        .iter()
        .filter(|case| case.expect_mbid.is_none() && case.expect_method.is_none())
        .count();

    // D-009: kolay bir kümede %100 ölçüm yapılmadığı anlamına gelir. Kümenin
    // kendisi de bir sözleşmedir; küçülürse metrik anlamsızlaşır.
    assert!(
        total >= 60,
        "doğruluk kümesi en az 60 vaka içermeli, {total} var"
    );
    assert!(
        negatives >= 15,
        "en az 15 negatif vaka gerekli, {negatives} var"
    );

    #[expect(clippy::cast_precision_loss, reason = "oran gösterimi")]
    let accuracy = correct as f64 / total as f64;
    println!(
        "\nKİMLİK DOĞRULUĞU: {correct}/{total} = %{:.1}  ({negatives} negatif vaka)",
        accuracy * 100.0
    );
    println!("sınıf bazında:");
    for (class, (ok, seen)) in &by_class {
        let mark = if ok == seen { " " } else { "!" };
        println!("  {mark} {class:<16} {ok:>2}/{seen:<2}");
    }
    if !failures.is_empty() {
        println!("başarısız vakalar:\n{}", failures.join("\n"));
    }

    // Eşik bilinçli olarak mevcut orandan biraz aşağıda: gerileme yakalanır,
    // ufak dalgalanma testi kırmaz. Oran yükseldikçe eşiği de yükselt.
    assert!(
        accuracy >= ACCURACY_FLOOR,
        "doğruluk %{:.1}'e düştü (eşik %{:.1})\n{}",
        accuracy * 100.0,
        ACCURACY_FLOOR * 100.0,
        failures.join("\n")
    );
}
