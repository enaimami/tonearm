//! A playback probe run by hand — does sound really come out?
//!
//! Does not run in CI (it needs an audio device). Usage:
//!
//! ```bash
//! cargo run -p headshell-core --features audio --example playback_probe -- <file>
//! ```

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/audio/tagged.flac"
        )
        .to_owned()
    });
    let path = std::path::Path::new(&path);

    match headshell_core::playback::AudioEngine::play_file(path) {
        Ok(engine) => {
            println!("file    : {}", path.display());
            println!("duration: {:?} ms", engine.duration_ms());
            for _ in 0..30 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                println!(
                    "state={} position={}ms finished={}",
                    engine.state(),
                    engine.position_ms(),
                    engine.finished()
                );
                if engine.finished() {
                    break;
                }
            }
            if let Some(err) = engine.take_error() {
                println!("ERROR: {err}");
            }
        }
        Err(err) => println!("could not open:\n{}", err.chain_text()),
    }
}
