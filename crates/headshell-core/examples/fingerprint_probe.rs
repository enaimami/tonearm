//! A fingerprint probe run by hand — what comes out of a file?
//!
//! The tests verify that the fingerprint is **stable** but do not show what
//! it looks like. This probe prints the string that goes to AcoustID: when a
//! match does not hold, that is the only way to tell whether the problem is
//! in the fingerprint or in the query (K9). The string can also be sent to
//! the `acoustid.org` API by hand.
//!
//! Does not run in CI. Usage:
//!
//! ```bash
//! cargo run -p headshell-core --features fingerprint --example fingerprint_probe -- <file>
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

    match headshell_core::identity::fingerprint::fingerprint_file(path) {
        Ok(print) => {
            let encoded = print.to_acoustid_string();
            println!("file    : {}", path.display());
            println!("duration: {} s", print.duration_secs);
            println!("items   : {} sub-fingerprints", print.raw.len());
            println!("encoded : {} characters", encoded.len());
            println!("{encoded}");
        }
        Err(err) => println!("{}", err.chain_text()),
    }
}
