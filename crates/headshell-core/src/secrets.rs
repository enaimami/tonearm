//! Ad alanlı sır deposu (D-042).
//!
//! Tek bir kavram: `<data_dir>/secrets.json`, unix'te `0600`, anahtarlar ad
//! alanına bölünmüş — `plugin:soundcloud`, `provider:navidrome`. Bir eklenti
//! el sıkışmada **yalnızca kendi ad alanını** görür.
//!
//! `keyring` bağımlılığı yok ve bu bilinçli (D-021 → D-042): yeni bir bağımlılık
//! ve başsız Linux'ta kırılgan. Okuma tek bir yerden geçtiği için arkasına
//! sonradan bir anahtarlık koymak bu dosyayı değiştirmekle sınırlı bir iş.
//!
//! **Değerler log'a ve `headshell diag`'a girmez.** Tanı raporu kopyalanıp
//! yapıştırılan bir metin (K9); içinde token taşıyamaz. Dışarı verilen tek
//! şey anahtar **adları** ve sayıları ([`Secrets::describe`]).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result, io_err};

/// Bir eklentinin ad alanı: `plugin:<ad>`.
#[must_use]
pub fn plugin_namespace(plugin: &str) -> String {
    format!("plugin:{plugin}")
}

/// Ad alanı → (anahtar → değer).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secrets {
    namespaces: BTreeMap<String, BTreeMap<String, String>>,
}

impl Secrets {
    /// Dosyadan okur. Dosya yoksa **boş depo** — bu bir hata değil, henüz sır
    /// yazılmamış demek. Bozuksa hata: sessizce boş dönmek, kullanıcının
    /// kimlik bilgisini "yok" sanıp yeniden sormak olurdu.
    ///
    /// # Errors
    /// Dosya okunamaz ya da JSON bozuksa.
    pub fn load(path: &Path) -> Result<Self> {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(io_err(Stage::ConfigLoad, path, err)),
        };
        serde_json::from_str(&raw).map_err(|source| {
            Error::new(
                Stage::ConfigLoad,
                ErrorKind::Json {
                    entry: path.display().to_string(),
                    source,
                },
            )
        })
    }

    /// Dosyaya yazar. Unix'te izinler `0600`.
    ///
    /// # Errors
    /// Dizin oluşturulamaz ya da dosya yazılamazsa.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| io_err(Stage::ConfigLoad, parent, err))?;
        }
        let text = serde_json::to_string_pretty(&self.namespaces).map_err(|source| {
            Error::new(
                Stage::ConfigLoad,
                ErrorKind::Json {
                    entry: path.display().to_string(),
                    source,
                },
            )
        })?;
        std::fs::write(path, text).map_err(|err| io_err(Stage::ConfigLoad, path, err))?;
        restrict_permissions(path)
    }

    /// Bir ad alanının bütün sırları. Ad alanı yoksa boş harita.
    #[must_use]
    pub fn namespace(&self, namespace: &str) -> BTreeMap<String, String> {
        self.namespaces.get(namespace).cloned().unwrap_or_default()
    }

    /// Tek bir sır yazar.
    pub fn set(&mut self, namespace: &str, key: impl Into<String>, value: impl Into<String>) {
        self.namespaces
            .entry(namespace.to_owned())
            .or_default()
            .insert(key.into(), value.into());
    }

    /// Tek bir sırrı siler. Ad alanı boşalırsa o da silinir.
    ///
    /// Dönüş: gerçekten bir şey silindi mi.
    pub fn remove(&mut self, namespace: &str, key: &str) -> bool {
        let Some(entries) = self.namespaces.get_mut(namespace) else {
            return false;
        };
        let removed = entries.remove(key).is_some();
        if entries.is_empty() {
            self.namespaces.remove(namespace);
        }
        removed
    }

    /// Bir ad alanının tamamını siler. Dönüş: ad alanı var mıydı.
    pub fn remove_namespace(&mut self, namespace: &str) -> bool {
        self.namespaces.remove(namespace).is_some()
    }

    /// Tanı ve `--json` için güvenli özet: ad alanı → **anahtar adları**.
    ///
    /// Değerler bilerek yok (K9 raporu kopyalanabilir olmalı).
    #[must_use]
    pub fn describe(&self) -> BTreeMap<String, Vec<String>> {
        self.namespaces
            .iter()
            .map(|(ns, entries)| (ns.clone(), entries.keys().cloned().collect()))
            .collect()
    }
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|err| io_err(Stage::ConfigLoad, path, err))
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<()> {
    // Windows'ta karşılığı ACL; oraya geldiğimizde yazılacak. Sessiz
    // geçmiyoruz: çağıran bir şey yapılmadığını bilsin.
    tracing::warn!("bu platformda sır dosyası izinleri kısıtlanmadı");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "headshell-secrets-{}-{}",
            std::process::id(),
            jiff::Timestamp::now().as_nanosecond()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn secrets_survive_a_round_trip_and_stay_private() {
        let path = temp_path("secrets.json");
        let mut secrets = Secrets::default();
        secrets.set(&plugin_namespace("soundcloud"), "client_id", "abc123");
        secrets.set("provider:navidrome", "token", "xyz");
        secrets.save(&path).unwrap();

        let back = Secrets::load(&path).unwrap();
        assert_eq!(back, secrets);
        assert_eq!(
            back.namespace("plugin:soundcloud").get("client_id"),
            Some(&"abc123".to_owned())
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "sır dosyası yalnızca sahibine okunur");
        }
    }

    #[test]
    fn a_plugin_sees_only_its_own_namespace() {
        let mut secrets = Secrets::default();
        secrets.set("plugin:soundcloud", "client_id", "abc");
        secrets.set("plugin:other", "client_id", "gizli");

        let mine = secrets.namespace("plugin:soundcloud");
        assert_eq!(mine.len(), 1);
        assert_eq!(mine.get("client_id"), Some(&"abc".to_owned()));
        assert!(secrets.namespace("plugin:yok").is_empty());
    }

    #[test]
    fn describe_lists_key_names_but_never_values() {
        let mut secrets = Secrets::default();
        secrets.set("plugin:soundcloud", "client_id", "cok-gizli-deger");
        let described = secrets.describe();
        let text = serde_json::to_string(&described).unwrap();
        assert!(text.contains("client_id"), "{text}");
        assert!(
            !text.contains("cok-gizli-deger"),
            "özet değerleri sızdırmamalı: {text}"
        );
    }

    #[test]
    fn removing_the_last_key_drops_the_namespace() {
        let mut secrets = Secrets::default();
        secrets.set("plugin:a", "k", "v");
        assert!(secrets.remove("plugin:a", "k"));
        assert!(secrets.describe().is_empty());
        assert!(
            !secrets.remove("plugin:a", "k"),
            "ikinci silme yalan söylememeli"
        );
    }

    #[test]
    fn a_missing_file_is_empty_but_a_broken_one_is_an_error() {
        let path = temp_path("secrets.json");
        assert_eq!(Secrets::load(&path).unwrap(), Secrets::default());

        std::fs::write(&path, "{ bozuk").unwrap();
        let err = Secrets::load(&path).unwrap_err();
        assert_eq!(err.stage(), Stage::ConfigLoad);
    }
}
