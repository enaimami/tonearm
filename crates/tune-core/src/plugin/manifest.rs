//! Eklenti manifesti (`plugin.json`) ve izin beyanı (D-040).
//!
//! Bir eklenti, içinde `plugin.json` olan bir dizindir; **dizin adı
//! kimliktir** (tema sisteminin kuralı, D-037). Manifestteki `name` dizin
//! adıyla uyuşmak zorunda: uyuşmazsa reddedilir, sessizce dizin adı
//! kullanılmaz — hangi adın kazandığı tahmin edilmemeli (K9).
//!
//! ## İzinler bir sözleşmedir, güvenlik duvarı değil (D-040)
//!
//! Eklenti alt süreç olarak **kullanıcının bütün yetkisiyle** çalışır.
//! Buradaki beyan, eklentinin ne yapacağını *söylemesidir*; çekirdek onu
//! zorlamaz ve bunu kullanıcıya da böyle söyler. Alanlar makine okunur
//! seçildi (ana bilgisayar adı, yol öneki) — sonradan bir Landlock/bwrap
//! kuralına çevrilebilsinler diye. O gün geldiğinde `api` artmaz: manifest
//! aynı kalır, değişen çekirdeğin onunla ne yaptığıdır.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result, io_err};

/// Manifest dosyasının adı.
pub const MANIFEST_FILE: &str = "plugin.json";

/// Bir eklentinin beyan ettiği izinler.
///
/// Boş küme "hiçbir şey istemiyorum" demektir ve geçerlidir — yerel dosya
/// okuyan bir eklenti bile `fs` beyan etmek zorunda.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Permissions {
    /// Erişilecek ana bilgisayar adları (`api.soundcloud.com`). `*` yok:
    /// "her yere çıkarım" diyen bir eklenti bunu tek tek yazmalı ya da
    /// kullanıcı onu reddetmeli.
    #[serde(default)]
    pub net: Vec<String>,
    /// Okunacak/yazılacak yol önekleri. Eklentinin **kendi** veri dizini
    /// buraya yazılmaz; onu çekirdek zaten veriyor.
    #[serde(default)]
    pub fs: Vec<String>,
}

impl Permissions {
    /// Hiçbir izin istemiyor mu.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.net.is_empty() && self.fs.is_empty()
    }

    /// Sıralanmış ve tekilleştirilmiş kopya. Karşılaştırma bunun üstünden
    /// yapılır ki manifestteki sıra değişince yeniden onay istenmesin.
    #[must_use]
    pub fn normalized(&self) -> Self {
        let tidy = |items: &[String]| {
            let mut out: Vec<String> = items
                .iter()
                .map(|item| item.trim().to_owned())
                .filter(|item| !item.is_empty())
                .collect();
            out.sort();
            out.dedup();
            out
        };
        Self {
            net: tidy(&self.net),
            fs: tidy(&self.fs),
        }
    }

    /// Bu küme `granted`'ın içinde mi kalıyor?
    ///
    /// Onay büyümeyi yakalamak için var: eklenti izinlerini **küçültürse**
    /// yeniden sorulmaz, büyütürse sorulur (D-040).
    #[must_use]
    pub fn is_covered_by(&self, granted: &Self) -> bool {
        let mine = self.normalized();
        let granted = granted.normalized();
        mine.net.iter().all(|item| granted.net.contains(item))
            && mine.fs.iter().all(|item| granted.fs.contains(item))
    }

    /// `granted`'da olmayan istekler — kullanıcıya "bunlar yeni" diye
    /// gösterilecek olan liste.
    #[must_use]
    pub fn beyond(&self, granted: &Self) -> Self {
        let mine = self.normalized();
        let granted = granted.normalized();
        Self {
            net: mine
                .net
                .into_iter()
                .filter(|item| !granted.net.contains(item))
                .collect(),
            fs: mine
                .fs
                .into_iter()
                .filter(|item| !granted.fs.contains(item))
                .collect(),
        }
    }

    /// İnsan okunur özet. `tune plugin list` ve `tune diag` bunu basar.
    #[must_use]
    pub fn describe(&self) -> String {
        let normalized = self.normalized();
        if normalized.is_empty() {
            return "izin istemiyor".to_owned();
        }
        let mut parts = Vec::new();
        if !normalized.net.is_empty() {
            parts.push(format!("ağ: {}", normalized.net.join(", ")));
        }
        if !normalized.fs.is_empty() {
            parts.push(format!("dosya: {}", normalized.fs.join(", ")));
        }
        parts.join(" | ")
    }
}

/// `plugin.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Dizin adıyla aynı olmak zorunda. Sağlayıcı kimliği bu.
    pub name: String,
    /// Kullanıcıya gösterilecek ad.
    pub display_name: String,
    /// Eklentinin kendi sürümü. Protokol sürümü değil.
    #[serde(default)]
    pub version: Option<String>,
    /// Konuştuğu protokol sürümü ([`super::protocol::PLUGIN_API`]).
    pub api: u32,
    /// Çalıştırılacak komut: ilk öğe program, gerisi argüman.
    ///
    /// İçinde `/` olan bir program adı eklenti dizinine göre çözülür
    /// (`./main.py`), olmayan `PATH`'ten aranır (`python3`).
    pub exec: Vec<String>,
    /// Yetenek adları (`search`, `browse`, `stream`, `control`).
    ///
    /// El sıkışmada da geliyor; **burada da olmasının sebebi tembellik**:
    /// `tune provider list` ve arama yönlendirmesi eklentiyi başlatmadan
    /// "bu ne yapabiliyor?" sorusunu cevaplayabilmeli. Çelişirlerse el
    /// sıkışma kazanır (çalışan kod beyandan doğrudur) ve fark bir uyarı
    /// olarak raporlanır (K9).
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub permissions: Permissions,
    /// İsteğe bağlı bir cümlelik açıklama.
    #[serde(default)]
    pub description: Option<String>,
}

impl PluginManifest {
    /// Bir eklenti dizininden okur ve doğrular.
    ///
    /// # Errors
    /// Dosya yoksa/okunamazsa, JSON bozuksa, ad dizin adıyla uyuşmuyorsa ya
    /// da `exec` boşsa.
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join(MANIFEST_FILE);
        let raw =
            std::fs::read_to_string(&path).map_err(|err| io_err(Stage::PluginLoad, &path, err))?;
        let manifest: Self = serde_json::from_str(&raw).map_err(|source| {
            Error::new(
                Stage::PluginLoad,
                ErrorKind::Json {
                    entry: path.display().to_string(),
                    source,
                },
            )
        })?;
        manifest.validate(dir, &path)?;
        Ok(manifest)
    }

    fn validate(&self, dir: &Path, path: &Path) -> Result<()> {
        let invalid = |detail: String| {
            Err(Error::new(
                Stage::PluginLoad,
                ErrorKind::PluginManifest {
                    path: path.to_path_buf(),
                    detail,
                },
            ))
        };

        let dir_name = dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if self.name.trim().is_empty() {
            return invalid("`name` boş".to_owned());
        }
        if self.name != dir_name {
            return invalid(format!(
                "`name` ({}) dizin adıyla ({dir_name}) uyuşmuyor — kimlik dizin adıdır",
                self.name
            ));
        }
        if self.display_name.trim().is_empty() {
            return invalid("`display_name` boş".to_owned());
        }
        if self.exec.is_empty() || self.exec[0].trim().is_empty() {
            return invalid("`exec` boş — çalıştırılacak bir komut yok".to_owned());
        }
        Ok(())
    }

    /// Çalıştırılacak programın yolu ve argümanları.
    ///
    /// Göreli program adları eklenti dizinine göre çözülür; böylece bir
    /// eklenti kendi yanındaki betiği `./main.py` diye gösterebilir ve
    /// çekirdek `PATH`'e bakmaz.
    #[must_use]
    pub fn resolve_exec(&self, dir: &Path) -> (PathBuf, Vec<String>) {
        let program = &self.exec[0];
        let path = if program.contains('/') || program.contains('\\') {
            dir.join(program)
        } else {
            PathBuf::from(program)
        };
        (path, self.exec[1..].to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!(
                "tune-plugin-manifest-{}-{}",
                std::process::id(),
                jiff::Timestamp::now().as_nanosecond()
            ))
            .join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_manifest(dir: &Path, json: &str) {
        std::fs::write(dir.join(MANIFEST_FILE), json).unwrap();
    }

    #[test]
    fn a_valid_manifest_loads_with_its_permissions() {
        let dir = temp_dir("soundcloud");
        write_manifest(
            &dir,
            r#"{
                "name": "soundcloud",
                "display_name": "SoundCloud",
                "version": "0.1.0",
                "api": 1,
                "exec": ["python3", "main.py"],
                "permissions": {"net": ["api.soundcloud.com"]}
            }"#,
        );
        let manifest = PluginManifest::load(&dir).unwrap();
        assert_eq!(manifest.name, "soundcloud");
        assert_eq!(manifest.api, 1);
        assert_eq!(manifest.permissions.net, vec!["api.soundcloud.com"]);
        assert!(manifest.permissions.fs.is_empty());
    }

    #[test]
    fn a_name_that_disagrees_with_the_directory_is_rejected_not_guessed() {
        let dir = temp_dir("soundcloud");
        write_manifest(
            &dir,
            r#"{"name":"baska","display_name":"X","api":1,"exec":["x"]}"#,
        );
        let err = PluginManifest::load(&dir).unwrap_err();
        assert_eq!(err.stage(), Stage::PluginLoad);
        assert!(
            err.chain_text().contains("dizin adıyla"),
            "{}",
            err.chain_text()
        );
    }

    #[test]
    fn an_empty_exec_is_rejected() {
        let dir = temp_dir("bos");
        write_manifest(
            &dir,
            r#"{"name":"bos","display_name":"Boş","api":1,"exec":[]}"#,
        );
        let err = PluginManifest::load(&dir).unwrap_err();
        assert!(err.chain_text().contains("exec"), "{}", err.chain_text());
    }

    #[test]
    fn a_relative_program_resolves_inside_the_plugin_dir_but_a_bare_name_does_not() {
        let dir = temp_dir("p");
        let manifest = PluginManifest {
            name: "p".to_owned(),
            display_name: "P".to_owned(),
            version: None,
            api: 1,
            exec: vec!["./main.py".to_owned(), "--json".to_owned()],
            capabilities: vec!["search".to_owned()],
            permissions: Permissions::default(),
            description: None,
        };
        let (program, args) = manifest.resolve_exec(&dir);
        assert_eq!(program, dir.join("./main.py"));
        assert_eq!(args, vec!["--json".to_owned()]);

        let bare = PluginManifest {
            exec: vec!["python3".to_owned()],
            ..manifest
        };
        let (program, args) = bare.resolve_exec(&dir);
        assert_eq!(program, PathBuf::from("python3"));
        assert!(args.is_empty());
    }

    #[test]
    fn shrinking_permissions_stays_covered_but_growing_them_does_not() {
        let granted = Permissions {
            net: vec!["a.example".to_owned(), "b.example".to_owned()],
            fs: vec!["/muzik".to_owned()],
        };
        let smaller = Permissions {
            net: vec!["a.example".to_owned()],
            fs: vec![],
        };
        assert!(smaller.is_covered_by(&granted));
        assert!(smaller.beyond(&granted).is_empty());

        let bigger = Permissions {
            net: vec!["a.example".to_owned(), "c.example".to_owned()],
            fs: vec!["/ev".to_owned()],
        };
        assert!(!bigger.is_covered_by(&granted));
        let extra = bigger.beyond(&granted);
        assert_eq!(extra.net, vec!["c.example".to_owned()]);
        assert_eq!(extra.fs, vec!["/ev".to_owned()]);
    }

    #[test]
    fn reordering_permissions_does_not_ask_the_user_again() {
        let granted = Permissions {
            net: vec!["b.example".to_owned(), "a.example".to_owned()],
            fs: vec![],
        };
        let same_other_order = Permissions {
            net: vec![
                "a.example".to_owned(),
                "b.example".to_owned(),
                " ".to_owned(),
            ],
            fs: vec![],
        };
        assert!(same_other_order.is_covered_by(&granted));
    }

    #[test]
    fn describe_says_what_is_asked_for_in_turkish() {
        let permissions = Permissions {
            net: vec!["api.soundcloud.com".to_owned()],
            fs: vec!["/muzik".to_owned()],
        };
        let text = permissions.describe();
        assert!(text.contains("ağ: api.soundcloud.com"), "{text}");
        assert!(text.contains("dosya: /muzik"), "{text}");
        assert_eq!(Permissions::default().describe(), "izin istemiyor");
    }
}
