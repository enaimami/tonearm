//! Çapa tahmininin doğruluk kümesi (PLAN §3.2).
//!
//! **Neden ayrı bir fixture dosyası, doğrudan Rust testi değil:** pozisyon
//! webview'de çekirdeğe sorulmadan tahmin ediliyor, yani formülün ikinci bir
//! kopyası JS'te yaşayacak. İki kopya zamanla kayar ve kayma kimsenin fark
//! etmediği yerde başlar — ilerleme çubuğu birkaç yüz milisaniye yalan söyler,
//! kimse şikâyet etmez, sonra Faz 4'te **aynı formül oda senkronunu sürer.**
//!
//! `fixtures/anchor/position_cases.json` iki tarafın da okuduğu tek doğruluk
//! kaynağı. Bu dosya Rust tarafını bağlar; GUI paketi yazıldığında JS tarafı
//! aynı dosyayı okuyacak.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use tune_core::playback::PlaybackAnchor;

#[derive(serde::Deserialize)]
struct Case {
    ad: String,
    capa: PlaybackAnchor,
    now: jiff::Timestamp,
    beklenen_ms: u64,
}

#[derive(serde::Deserialize)]
struct Cases {
    vakalar: Vec<Case>,
}

fn cases() -> Cases {
    let path = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/anchor/position_cases.json"
    ));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("{} okunamadı: {err}", path.display()));
    serde_json::from_str(&text).expect("doğruluk kümesi ayrıştırılmalı")
}

#[test]
fn every_case_in_the_shared_truth_set_matches_the_core_formula() {
    let cases = cases();
    assert!(
        cases.vakalar.len() >= 12,
        "doğruluk kümesi küçülmüş: {} vaka",
        cases.vakalar.len()
    );

    let mut failures = Vec::new();
    for case in &cases.vakalar {
        let got = case.capa.position_at(case.now);
        if got != case.beklenen_ms {
            failures.push(format!(
                "  {}: beklenen {} ms, çıkan {} ms",
                case.ad, case.beklenen_ms, got
            ));
        }
    }

    // Hepsi tek seferde raporlanıyor: ilk uyuşmazlıkta durmak, formül
    // değiştiğinde kaç vakanın etkilendiğini gizlerdi.
    assert!(
        failures.is_empty(),
        "{} / {} vaka uymadı:\n{}",
        failures.len(),
        cases.vakalar.len(),
        failures.join("\n")
    );
}

/// Doğruluk kümesi yalnızca kolay yolu kapsıyorsa kilit değildir.
#[test]
fn the_truth_set_covers_the_cases_that_actually_break_a_reimplementation() {
    let cases = cases();
    let all: String = cases
        .vakalar
        .iter()
        .map(|case| case.ad.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    for needle in [
        "duraklat",   // ilerlememeli
        "bekliyor",   // Buffering de ilerlememeli — kolayca atlanan durum
        "rate 0",     // çalıyor görünüp ilerlemeyen hâl
        "1.001",      // Faz 4 sürüklenme düzeltmesi
        "geriye",     // saat geri atlarsa
        "aşmaz",      // süreye kırpma
        "bilinmiyor", // süre yoksa kırpma yok
    ] {
        assert!(
            all.contains(needle),
            "doğruluk kümesinde '{needle}' vakası yok:\n{all}"
        );
    }
}
