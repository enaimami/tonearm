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

use headshell_core::playback::{AudioEngine, PlayState};
use headshell_core::provider::AudioSource;

/// Yerel dosya kaynağı (kısayol).
fn local(path: &std::path::Path) -> AudioSource {
    AudioSource::LocalFile {
        path: path.to_path_buf(),
    }
}

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
    let Some(engine) = engine_for("tagged.flac") else {
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
    let Some(engine) = engine_for("Test Artist - Mp3 Track.mp3") else {
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

/// D-024'ün iddiası: iki parça arka arkaya çalarken **çıkış hiç durmuyor.**
///
/// Eski tasarımda her parça için yeni bir cpal akışı ve yeni bir çözücü
/// kuruluyordu; boşluk buydu. Artık akış açık kalıyor ve sıradaki parçanın
/// örnekleri bitenin arkasına ekleniyor.
#[test]
fn two_tracks_play_back_to_back_without_the_output_ever_stopping() {
    if !has_output_device() {
        eprintln!("ses çıkışı yok — gapless testi atlanıyor (bu bir başarısızlık değil)");
        return;
    }
    let engine = match AudioEngine::open() {
        Ok(engine) => engine,
        Err(err) => {
            eprintln!(
                "ses aygıtı açılamadı, test atlanıyor:\n{}",
                err.chain_text()
            );
            return;
        }
    };

    let first = local(&fixture("tagged.flac"));
    let second = local(&fixture("Test Artist - Mp3 Track.mp3"));

    let seq0 = engine.play_source(&first).expect("ilk parça açılmalı");
    // İkinci parça, birincisi **çalarken** sıraya giriyor: gapless'ın koşulu.
    let seq1 = engine.enqueue(&second);
    assert_ne!(seq0, seq1);

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut switched = false;
    let mut stopped_before_switch = false;
    while Instant::now() < deadline {
        if engine.current_seq() == Some(seq1) {
            switched = true;
            break;
        }
        // Geçiş duyulmadan önce çıkış **durmuş** görünmemeli.
        if engine.state() == PlayState::Stopped {
            stopped_before_switch = true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    assert!(
        switched,
        "ikinci parçaya geçilmeliydi (durum: {})",
        engine.state()
    );
    assert!(
        !stopped_before_switch,
        "geçişte çıkış durdu — gapless bozuk"
    );
    assert!(
        !engine.finished(),
        "sırada parça varken motor bitmiş sayılmamalı"
    );

    // Biten parçanın scrobble'ı kendi uzunluğunu görmeli, çıkışın toplamını değil.
    let played = engine.played_ms_of(seq0).expect("ilk dilim bilinmeli");
    assert!(
        (900..=1200).contains(&played),
        "1 sn'lik parça tam çalınmalıydı: {played}ms"
    );
    // Yeni parçanın pozisyonu **baştan** sayılmalı.
    assert!(
        engine.position_ms() < 900,
        "ikinci parça baştan başlamalı: {}ms",
        engine.position_ms()
    );
}

#[test]
fn a_corrupt_file_fails_with_the_decode_stage() {
    // Ses aygıtı olmasa da çalışır: hata çözme aşamasında, çıkıştan önce.
    let err = AudioEngine::play_file(&fixture("corrupt.flac")).expect_err("bozuk dosya açılmamalı");
    assert_eq!(err.stage(), headshell_core::diag::Stage::PlaybackDecode);
}

#[test]
fn a_missing_file_names_the_path() {
    let err = AudioEngine::play_file(&fixture("olmayan.flac")).expect_err("olmayan dosya");
    assert_eq!(err.stage(), headshell_core::diag::Stage::PlaybackDecode);
    assert!(
        err.chain_text().contains("olmayan.flac"),
        "hata dosyayı söylemeli:\n{}",
        err.chain_text()
    );
}
