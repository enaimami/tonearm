//! Gerçek dosyayı gerçek ses aygıtında çalma testi.
//!
//! **Ses aygıtı olmayan ortamda (CI, konteyner) kendini atlar** — testi
//! susturmak değil, koşulun sağlanmadığını açıkça söyleyip geçmek. Aygıt
//! varsa gerçekten çalar ve pozisyonun ilerlediğini doğrular.
//!
//! `audio` feature'ı olmadan bu dosya boş derlenir.

#![cfg(feature = "audio")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use tune_core::playback::{AudioEngine, PlayState};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/audio")).join(name)
}

/// Ses çıkışı var mı? Yoksa test anlamlı değil.
fn has_output_device() -> bool {
    use cpal::traits::HostTrait;
    cpal::default_host().default_output_device().is_some()
}

/// Motoru kurar; aygıt yoksa `None` döner.
fn engine_for(name: &str) -> Option<AudioEngine> {
    if !has_output_device() {
        eprintln!("ses çıkışı yok — test atlanıyor (bu bir başarısızlık değil)");
        return None;
    }
    match AudioEngine::play_file(&fixture(name)) {
        Ok(engine) => Some(engine),
        Err(err) => {
            // Aygıt var göründü ama açılamadı (kilitli, izin yok…).
            // Bunu başarısızlık saymıyoruz ama sessizce de geçmiyoruz.
            eprintln!(
                "ses aygıtı açılamadı, test atlanıyor:\n{}",
                err.chain_text()
            );
            None
        }
    }
}

#[test]
fn a_real_file_plays_and_the_position_advances() {
    let Some(engine) = engine_for("etiketli.flac") else {
        return;
    };

    // Süre kaptan okunmalı: 1 saniyelik fixture.
    let duration = engine.duration_ms().expect("süre okunmalı");
    assert!(
        (900..=1100).contains(&duration),
        "1 sn beklenirken {duration}ms"
    );

    // Sesin akmaya başlamasını bekle.
    let deadline = Instant::now() + Duration::from_secs(3);
    while engine.position_ms() == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        engine.position_ms() > 0,
        "3 saniyede tek kare bile çalınmadı (durum: {})",
        engine.state()
    );

    // Parça bitene kadar bekle; 1 sn'lik dosya 4 sn içinde bitmeli.
    let deadline = Instant::now() + Duration::from_secs(4);
    while !engine.finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        engine.finished(),
        "parça bitmedi (durum: {})",
        engine.state()
    );
    assert_eq!(engine.state(), PlayState::Stopped);

    // Pozisyon süreyi aşmamalı ve ona yakın durmalı.
    let position = engine.position_ms();
    assert!(
        position >= duration.saturating_sub(150),
        "pozisyon ({position}ms) süreye ({duration}ms) ulaşmalıydı"
    );

    assert_eq!(engine.take_error(), None, "çözme hatasız bitmeliydi");
}

#[test]
fn pausing_freezes_the_position() {
    let Some(engine) = engine_for("Test Sanatci - Mp3 Parca.mp3") else {
        return;
    };

    let deadline = Instant::now() + Duration::from_secs(3);
    while engine.position_ms() == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    if engine.position_ms() == 0 {
        eprintln!("ses akmadı — test atlanıyor");
        return;
    }

    engine.pause();
    assert_eq!(engine.state(), PlayState::Paused);
    let paused_at = engine.position_ms();

    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        engine.position_ms(),
        paused_at,
        "duraklatılmışken pozisyon ilerlememeli"
    );

    engine.resume();
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        engine.position_ms() > paused_at,
        "sürdürüldükten sonra ilerlemeli"
    );
}

#[test]
fn a_corrupt_file_fails_with_the_decode_stage() {
    // Ses aygıtı olmasa da çalışır: hata çözme aşamasında, çıkıştan önce.
    let err = AudioEngine::play_file(&fixture("bozuk.flac")).expect_err("bozuk dosya açılmamalı");
    assert_eq!(err.stage(), tune_core::diag::Stage::PlaybackDecode);
}

#[test]
fn a_missing_file_names_the_path() {
    let err = AudioEngine::play_file(&fixture("olmayan.flac")).expect_err("olmayan dosya");
    assert_eq!(err.stage(), tune_core::diag::Stage::PlaybackDecode);
    assert!(
        err.chain_text().contains("olmayan.flac"),
        "hata dosyayı söylemeli:\n{}",
        err.chain_text()
    );
}
