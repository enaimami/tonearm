//! JS'teki çapa formülünü doğruluk kümesine bağlar (D-033).
//!
//! Rust tarafını `tune-core/tests/anchor_parity.rs` zaten bağlıyor. Kayma
//! **iki taraf da aynı dosyayı okuduğunda** yakalanır; tek taraflı bir test
//! yalnızca kendi kopyasının kendisiyle tutarlı olduğunu söyler.
//!
//! ## `node` yoksa bu test neden atlanmıyor
//!
//! Atlanabilir yapmak kolaydı ve yanlış olurdu: D-032'de koşulu
//! sağlanamayan bir test yeşil yanıp hiçbir şey kanıtlamadığı için silindi.
//! Aynı hata burada da yapılabilirdi — `node` kurulu olmayan bir makinede
//! test sessizce geçer, formüller aylarca kayar, kimse fark etmez.
//! Bu yüzden `node` yoksa test **düşer** ve neden düştüğünü söyler.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;

fn betik_yolu() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("anchor_parity.mjs")
}

#[test]
fn the_javascript_copy_agrees_with_the_shared_truth_set() {
    let betik = betik_yolu();
    let cikti = Command::new("node").arg(&betik).output();

    let cikti = match cikti {
        Ok(cikti) => cikti,
        Err(source) => panic!(
            "ADIM: ANCHOR_PARITY — `node` çalıştırılamadı ({source}).\n\
             GUI'nin pozisyon formülü JS'te yaşıyor ve doğruluk kümesine \
             yalnızca bu testle bağlı.\n\
             Bu test atlanabilir olsaydı formüller sessizce kayardı (D-032).\n\
             Betik tek başına da koşar: node {}",
            betik.display()
        ),
    };

    assert!(
        cikti.status.success(),
        "JS kopyası çekirdekten kaydı:\n{}{}",
        String::from_utf8_lossy(&cikti.stdout),
        String::from_utf8_lossy(&cikti.stderr),
    );
}
