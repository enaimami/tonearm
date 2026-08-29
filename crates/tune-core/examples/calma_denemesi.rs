//! Elle çalıştırılan çalma denemesi — ses gerçekten çıkıyor mu?
//!
//! CI'da koşmaz (ses aygıtı gerektirir). Kullanım:
//!
//! ```bash
//! cargo run -p tune-core --features audio --example calma_denemesi -- <dosya>
//! ```

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/audio/etiketli.flac"
        )
        .to_owned()
    });
    let path = std::path::Path::new(&path);

    match tune_core::playback::AudioEngine::play_file(path) {
        Ok(engine) => {
            println!("dosya : {}", path.display());
            println!("süre  : {:?} ms", engine.duration_ms());
            for _ in 0..30 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                println!(
                    "durum={} pozisyon={}ms bitti={}",
                    engine.state(),
                    engine.position_ms(),
                    engine.finished()
                );
                if engine.finished() {
                    break;
                }
            }
            if let Some(err) = engine.take_error() {
                println!("HATA: {err}");
            }
        }
        Err(err) => println!("açılamadı:\n{}", err.chain_text()),
    }
}
