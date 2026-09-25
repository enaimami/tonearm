//! Entegrasyon testlerinin ortak yardımcıları (D-070).
//!
//! Her entegrasyon testi dosyası ayrı bir crate ve bu modül her birine
//! `mod support;` ile ayrıca derleniyor. Bir dosyanın kullanmadığı yardımcıyı
//! derleyici o crate için ölü sayar; uyarı bu yüzden modül düzeyinde kapalı.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use headshell_core::config::Config;

/// Testlerin yazdığı kök: Cargo'nun `CARGO_TARGET_TMPDIR`'i (`target/tmp`).
///
/// İşletim sisteminin ortak geçici dizini değil. Testler dizinlerini
/// siliyor, ama öldürülmüş bir koşumun artığı bile makinenin `/tmp`'sine
/// değil projenin `target`'ına düşer ve `cargo clean` ile gider. Bir zamanlar
/// ortak `/tmp`'ye yazılıyordu ve bir geliştirme makinesinde 1,2 GB
/// birikmişti — o makinede `/tmp` bir tmpfs, yani bellekti.
#[must_use]
pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
}

/// Kendini silen geçici dizin — düşen bir testte de (`Drop` panikte koşar).
pub struct TempDir(PathBuf);

impl TempDir {
    #[must_use]
    pub fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let base = root();
        std::fs::create_dir_all(&base).expect("test kökü açılmalı");
        loop {
            let dir = base.join(format!(
                "{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&dir) {
                Ok(()) => return Self(dir),
                // Öldürülmüş bir koşumdan kalma aynı adlı dizin: kullanılmaz.
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => panic!("geçici dizin açılamadı ({}): {err}", dir.display()),
            }
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
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

/// Kendini silen bir veri dizini üstünde `Config`; `&Config` bekleyen her
/// yere verilebilir.
pub struct TestConfig {
    _dir: TempDir,
    config: Config,
}

impl TestConfig {
    #[must_use]
    pub fn new(label: &str) -> Self {
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

/// Bir eklentiyi **canlı katalogdan** veri dizinine kurar (D-071) —
/// kullanıcının `headshell plugin install <ad>` ile yaptığının aynısı.
///
/// Eklentiler bu depoda durmuyor, `headshell/plugins`'te yaşıyor; canlı
/// testler onları kullanıcının aldığı yoldan alıyor ve böylece katalog da
/// uçtan uca sınanıyor: indeks, sha256, manifest karşılaştırması, kurulum.
/// Adres `HEADSHELL_PLUGIN_INDEX`'ten ya da varsayılandan.
///
/// İki başarısızlık ayrı (K9, D-043):
/// - Kataloğa **ulaşılamadıysa** `Err(sebep)` döner; çağıran testi atlar.
/// - Ulaşılıp eklenti **kurulamadıysa** panikler: indeks bozuk, dosya yok ya
///   da karma tutmuyor. Bu kırmızı yanmalı — kullanıcı da kuramaz.
pub async fn install_from_catalog(config: &Config, name: &str) -> Result<PathBuf, String> {
    use headshell_core::diag::Stage;
    use headshell_core::plugin::catalog;

    let http = headshell_core::net::default_http_client().map_err(|err| err.chain_text())?;
    let index = config.plugin_index_url();
    let result = async {
        let catalog = catalog::fetch(http.as_ref(), &index).await?;
        catalog::install(config, http.as_ref(), &catalog, name).await
    }
    .await;
    match result {
        Ok(_) => Ok(config.plugins_dir().join(name)),
        Err(err) if err.stage() == Stage::NetworkRequest => Err(format!(
            "katalog ({index}) okunamadı: {}",
            err.chain_text().replace('\n', " ")
        )),
        Err(err) => panic!(
            "{name} katalogdan kurulamadı ({index}):\n{}",
            err.chain_text()
        ),
    }
}
