//! Elle çalıştırılan parmak izi denemesi — dosyadan ne çıkıyor?
//!
//! Testler parmak izinin **kararlı** olduğunu doğruluyor ama nasıl göründüğünü
//! göstermiyor. Bu sonda, AcoustID'ye giden dizeyi ekrana basar: bir eşleşme
//! tutmadığında sorunun parmak izinde mi sorguda mı olduğu ancak böyle ayrılır
//! (K9). Dizeyi `acoustid.org` API'sine elle göndermek de mümkün.
//!
//! CI'da koşmaz. Kullanım:
//!
//! ```bash
//! cargo run -p tune-core --features fingerprint --example fingerprint_probe -- <dosya>
//! ```

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/audio/fingerprint_sample.flac"
        )
        .to_owned()
    });
    let path = std::path::Path::new(&path);

    match tune_core::identity::fingerprint::fingerprint_file(path) {
        Ok(print) => {
            let encoded = print.to_acoustid_string();
            println!("dosya   : {}", path.display());
            println!("süre    : {} sn", print.duration_secs);
            println!("öğe     : {} alt-parmak izi", print.raw.len());
            println!("kodlu   : {} karakter", encoded.len());
            println!("{encoded}");
        }
        Err(err) => println!("{}", err.chain_text()),
    }
}
