//! Birim testlerinin ortak yardımcıları. Yalnızca `cfg(test)` derlenir.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::config::Config;

/// Kendini silen geçici dizin.
///
/// Testler geçici dizinlerini **siler**, düşen bir test de: `Drop` panik
/// sırasında da koşar. Bir zamanlar silmiyorlardı ve bir geliştirme
/// makinesinin `/tmp`'sinde — orada bir tmpfs, yani bellek — 1.100 dizin,
/// 1,2 GB birikmişti (D-070).
///
/// Ad `headshell-<etiket>-<süreç>-<sıra>`. Sıra süreç içinde artan bir sayı,
/// saat değil: Windows'un saat çözünürlüğü iki paralel testi aynı ada
/// düşürebilirdi. Aynı adlı eski bir dizin (öldürülmüş bir koşumdan) varsa
/// **kullanılmaz**, bir sonraki sıraya geçilir — test başkasının artığını
/// kendi verisi sanmasın.
pub(crate) struct TempDir(PathBuf);

impl TempDir {
    pub(crate) fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir();
        loop {
            let dir = base.join(format!(
                "headshell-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&dir) {
                Ok(()) => return Self(dir),
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => panic!("geçici dizin açılamadı ({}): {err}", dir.display()),
            }
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl std::ops::Deref for TempDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Silinemezse (Windows'ta açık kalmış bir dosya) test düşürülmez;
        // ama sessiz de kalınmaz, geride ne kaldığı yazılır.
        if let Err(err) = std::fs::remove_dir_all(&self.0)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!(
                "uyarı: geçici dizin silinemedi ({}): {err}",
                self.0.display()
            );
        }
    }
}

/// Kendini silen bir veri dizini üstünde `Config`.
///
/// `Config`'e deref ediyor: `&Config` bekleyen her yere `&test_config`
/// verilebilir, ve değer düştüğünde dizin de gider.
pub(crate) struct TestConfig {
    _dir: TempDir,
    config: Config,
}

impl TestConfig {
    pub(crate) fn new(label: &str) -> Self {
        let dir = TempDir::new(label);
        let config = Config::with_data_dir(dir.path());
        Self { _dir: dir, config }
    }
}

impl std::ops::Deref for TestConfig {
    type Target = Config;

    fn deref(&self) -> &Config {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_temp_dir_is_unique_and_gone_after_drop() {
        let first = TempDir::new("destek");
        let second = TempDir::new("destek");
        assert_ne!(first.path(), second.path());
        std::fs::write(first.join("dosya"), "x").unwrap();
        let path = first.path().to_path_buf();
        drop(first);
        assert!(!path.exists(), "dizin silinmedi: {}", path.display());
    }
}
