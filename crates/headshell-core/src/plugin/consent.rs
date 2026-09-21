//! Eklenti onay defteri: `<data_dir>/plugins.json` (D-040).
//!
//! Onaylanan izin kümesi **olduğu gibi** saklanır, özeti değil: kullanıcı
//! neye evet dediğini dosyayı açıp okuyabilmeli, ve "yeni ne isteniyor?"
//! sorusunun cevabı bir hash karşılaştırmasından değil kümeler farkından
//! gelmeli.
//!
//! Onay verilmemiş bir eklenti **yüklenmez ama görünür**: `headshell plugin list`
//! onu "onay bekliyor" diye gösterir. Sessizce atlanan bir eklenti,
//! kullanıcının kurduğunu sandığı ama çalışmayan bir eklentidir (K9).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result, io_err};

use super::manifest::Permissions;

/// Bir eklentinin onay kaydı.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginConsent {
    /// Kullanıcının evet dediği izin kümesi.
    pub granted: Permissions,
    pub granted_at: jiff::Timestamp,
    /// Kullanıcı sonradan kapattıysa `false`. Kayıt silinmez: kapatmak
    /// unutmak değildir, ve yeniden açarken aynı izinler sorulmaz.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

const fn default_true() -> bool {
    true
}

/// Bir eklentinin bu andaki onay durumu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ConsentStatus {
    /// Onaylı ve istenen izinler onayın içinde.
    Approved,
    /// Hiç sorulmamış.
    NotAsked,
    /// Onay var ama eklenti **daha fazlasını** istiyor. `extra` yeni istekler.
    NeedsApproval { extra: Permissions },
    /// Kullanıcı kapattı.
    Disabled,
}

impl ConsentStatus {
    /// Eklenti çalıştırılabilir mi.
    #[must_use]
    pub fn is_approved(&self) -> bool {
        matches!(self, Self::Approved)
    }

    /// Kullanıcıya gösterilecek tek satırlık sebep.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Approved => "onaylı".to_owned(),
            Self::NotAsked => "onay bekliyor — `headshell plugin approve <ad>`".to_owned(),
            Self::NeedsApproval { extra } => format!(
                "yeni izin istiyor ({}) — `headshell plugin approve <ad>`",
                extra.describe()
            ),
            Self::Disabled => "kapalı — `headshell plugin enable <ad>`".to_owned(),
        }
    }
}

/// Onay defteri.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConsentStore {
    plugins: BTreeMap<String, PluginConsent>,
}

impl ConsentStore {
    /// Dosyadan okur. Dosya yoksa boş defter — henüz hiçbir eklenti
    /// onaylanmamış demek. Bozuksa hata: boş defter dönmek, bütün onayları
    /// sessizce silmek olurdu.
    ///
    /// # Errors
    /// Dosya okunamaz ya da JSON bozuksa.
    pub fn load(path: &Path) -> Result<Self> {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(io_err(Stage::PluginLoad, path, err)),
        };
        serde_json::from_str(&raw).map_err(|source| {
            Error::new(
                Stage::PluginLoad,
                ErrorKind::Json {
                    entry: path.display().to_string(),
                    source,
                },
            )
        })
    }

    /// Dosyaya yazar.
    ///
    /// # Errors
    /// Dizin oluşturulamaz ya da dosya yazılamazsa.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| io_err(Stage::PluginLoad, parent, err))?;
        }
        let text = serde_json::to_string_pretty(&self.plugins).map_err(|source| {
            Error::new(
                Stage::PluginLoad,
                ErrorKind::Json {
                    entry: path.display().to_string(),
                    source,
                },
            )
        })?;
        std::fs::write(path, text).map_err(|err| io_err(Stage::PluginLoad, path, err))
    }

    /// Bir eklentinin istediği izinlere göre durumu.
    #[must_use]
    pub fn status(&self, name: &str, requested: &Permissions) -> ConsentStatus {
        let Some(record) = self.plugins.get(name) else {
            return ConsentStatus::NotAsked;
        };
        if !record.enabled {
            return ConsentStatus::Disabled;
        }
        if requested.is_covered_by(&record.granted) {
            ConsentStatus::Approved
        } else {
            ConsentStatus::NeedsApproval {
                extra: requested.beyond(&record.granted),
            }
        }
    }

    /// İzinleri onaylar (ve kapalıysa açar). Onaylanan küme **istenenin
    /// kendisidir**, birleşimi değil: eklenti izin bıraktıysa defter de
    /// bırakmalı.
    pub fn approve(&mut self, name: &str, requested: &Permissions, now: jiff::Timestamp) {
        self.plugins.insert(
            name.to_owned(),
            PluginConsent {
                granted: requested.normalized(),
                granted_at: now,
                enabled: true,
            },
        );
    }

    /// Eklentiyi kapatır. Onay kaydı **korunur**.
    ///
    /// Dönüş: kayıt var mıydı.
    pub fn disable(&mut self, name: &str) -> bool {
        match self.plugins.get_mut(name) {
            Some(record) => {
                record.enabled = false;
                true
            }
            None => false,
        }
    }

    /// Kapalı bir eklentiyi yeniden açar. Dönüş: kayıt var mıydı.
    pub fn enable(&mut self, name: &str) -> bool {
        match self.plugins.get_mut(name) {
            Some(record) => {
                record.enabled = true;
                true
            }
            None => false,
        }
    }

    /// Onayı tamamen unutur — bir sonraki çalıştırmada baştan sorulur.
    /// Dönüş: kayıt var mıydı.
    pub fn forget(&mut self, name: &str) -> bool {
        self.plugins.remove(name).is_some()
    }

    /// Kayıtlı onay (varsa).
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&PluginConsent> {
        self.plugins.get(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(hosts: &[&str]) -> Permissions {
        Permissions {
            net: hosts.iter().map(|h| (*h).to_owned()).collect(),
            fs: Vec::new(),
        }
    }

    fn now() -> jiff::Timestamp {
        jiff::Timestamp::now()
    }

    #[test]
    fn an_unknown_plugin_has_not_been_asked_about() {
        let store = ConsentStore::default();
        assert_eq!(
            store.status("soundcloud", &net(&["a.example"])),
            ConsentStatus::NotAsked
        );
    }

    #[test]
    fn approval_covers_the_same_and_smaller_sets_but_not_bigger_ones() {
        let mut store = ConsentStore::default();
        store.approve("p", &net(&["a.example", "b.example"]), now());

        assert!(
            store
                .status("p", &net(&["a.example", "b.example"]))
                .is_approved()
        );
        assert!(store.status("p", &net(&["a.example"])).is_approved());

        let status = store.status("p", &net(&["a.example", "c.example"]));
        match status {
            ConsentStatus::NeedsApproval { extra } => {
                assert_eq!(extra.net, vec!["c.example".to_owned()]);
            }
            other => panic!("büyüyen izin yeniden onay istemeli: {other:?}"),
        }
    }

    #[test]
    fn disabling_keeps_the_record_so_re_enabling_asks_nothing() {
        let mut store = ConsentStore::default();
        store.approve("p", &net(&["a.example"]), now());
        assert!(store.disable("p"));
        assert_eq!(
            store.status("p", &net(&["a.example"])),
            ConsentStatus::Disabled
        );
        assert!(store.enable("p"));
        assert!(store.status("p", &net(&["a.example"])).is_approved());
    }

    #[test]
    fn forgetting_sends_the_plugin_back_to_the_start() {
        let mut store = ConsentStore::default();
        store.approve("p", &net(&["a.example"]), now());
        assert!(store.forget("p"));
        assert_eq!(
            store.status("p", &net(&["a.example"])),
            ConsentStatus::NotAsked
        );
        assert!(!store.forget("p"));
    }

    #[test]
    fn the_ledger_survives_a_file_round_trip() {
        let dir = std::env::temp_dir().join(format!(
            "headshell-consent-{}-{}",
            std::process::id(),
            jiff::Timestamp::now().as_nanosecond()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plugins.json");

        assert_eq!(ConsentStore::load(&path).unwrap(), ConsentStore::default());

        let mut store = ConsentStore::default();
        store.approve("p", &net(&["b.example", "a.example"]), now());
        store.save(&path).unwrap();

        let back = ConsentStore::load(&path).unwrap();
        assert_eq!(back, store);
        // Onaylanan küme sıralı yazılmalı; sıra değişikliği yeniden onay
        // istemesin diye normalize ediliyor.
        assert_eq!(
            back.get("p").unwrap().granted.net,
            vec!["a.example".to_owned(), "b.example".to_owned()]
        );
    }

    #[test]
    fn a_broken_ledger_is_an_error_not_an_empty_one() {
        let dir = std::env::temp_dir().join(format!(
            "headshell-consent-broken-{}-{}",
            std::process::id(),
            jiff::Timestamp::now().as_nanosecond()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plugins.json");
        std::fs::write(&path, "{bozuk").unwrap();
        let err = ConsentStore::load(&path).unwrap_err();
        assert_eq!(err.stage(), Stage::PluginLoad);
    }
}
