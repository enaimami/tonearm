//! Veri dizini ve yapılandırma.
//!
//! Yol çözümlemesi çekirdekte: CLI, GUI ve mobil aynı dizini bulmalı.

use std::path::{Path, PathBuf};

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};

/// Veri dizinini elle vermek için ortam değişkeni (testler ve taşınabilir kurulum).
pub const DATA_DIR_ENV: &str = "TUNE_DATA_DIR";

/// Müzik dizinlerini elle vermek için ortam değişkeni (`:` ile ayrılmış).
pub const MUSIC_DIRS_ENV: &str = "TUNE_MUSIC_DIRS";

/// Çekirdeğin çalışması için gereken yollar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    data_dir: PathBuf,
}

impl Config {
    /// Verilen dizini kullanır.
    #[must_use]
    pub fn with_data_dir(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    /// Veri dizinini ortamdan bulur.
    ///
    /// Sıra: `TUNE_DATA_DIR` → `XDG_DATA_HOME/tune` → `HOME/.local/share/tune`.
    ///
    /// # Errors
    /// Hiçbiri bulunamazsa — sessizce geçici dizine düşmek, kullanıcının
    /// geçmişini fark ettirmeden kaybetmek demektir.
    pub fn discover() -> Result<Self> {
        if let Some(dir) = non_empty_env(DATA_DIR_ENV) {
            return Ok(Self::with_data_dir(dir));
        }
        if let Some(dir) = non_empty_env("XDG_DATA_HOME") {
            return Ok(Self::with_data_dir(PathBuf::from(dir).join("tune")));
        }
        if let Some(home) = non_empty_env("HOME") {
            return Ok(Self::with_data_dir(
                PathBuf::from(home).join(".local/share/tune"),
            ));
        }
        Err(Error::new(
            Stage::ConfigLoad,
            ErrorKind::NotFound {
                what: format!(
                    "veri dizini — {DATA_DIR_ENV}, XDG_DATA_HOME ya da HOME değişkenlerinden hiçbiri tanımlı değil"
                ),
            },
        ))
    }

    /// Veri dizini.
    #[must_use]
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Kütüphane veritabanı.
    #[must_use]
    pub fn database_path(&self) -> PathBuf {
        self.data_dir.join("library.db")
    }

    /// Kayıtlı uzak sunucular ve kimlik bilgileri (D-021).
    ///
    /// Veritabanından ayrı bir dosya: kimlik bilgisi kütüphane verisi
    /// değildir ve aynı dosyada durması yedekleme/paylaşma davranışlarını
    /// karıştırır. Unix'te `0600` yazılır.
    #[must_use]
    pub fn servers_path(&self) -> PathBuf {
        self.data_dir.join("servers.json")
    }

    /// Ad alanlı sır deposu (D-042). Unix'te `0600` yazılır.
    ///
    /// `servers.json`'dan ayrı ve bu bilinçli: orada duran şey bir **sunucu
    /// kaydı** (adres + tür + kullanıcı), sır o kaydın bir alanı. Buradaki
    /// ise sırrın kendisi, sahibi ad alanıyla belirtilmiş.
    #[must_use]
    pub fn secrets_path(&self) -> PathBuf {
        self.data_dir.join("secrets.json")
    }

    /// Eklentilerin bulunduğu dizin: `<data_dir>/plugins/<ad>/plugin.json`.
    #[must_use]
    pub fn plugins_dir(&self) -> PathBuf {
        self.data_dir.join("plugins")
    }

    /// Eklenti izin onayları defteri (D-040).
    #[must_use]
    pub fn plugin_consent_path(&self) -> PathBuf {
        self.data_dir.join("plugins.json")
    }

    /// Bir eklentinin yazabileceği kendi dizini.
    ///
    /// Eklentinin gördüğü tek yazılabilir yol bu — izin beyanı zorlanmasa da
    /// (D-040) çekirdeğin kendi eliyle verdiği şey daraltılmış olur.
    #[must_use]
    pub fn plugin_state_dir(&self, plugin: &str) -> PathBuf {
        self.plugins_dir().join(plugin).join("state")
    }

    /// Son çalıştırmanın tanı raporu.
    #[must_use]
    pub fn last_run_path(&self) -> PathBuf {
        self.data_dir.join("last-run.json")
    }

    /// Yerel sağlayıcının tarayacağı müzik dizinleri.
    ///
    /// Sıra: `TUNE_MUSIC_DIRS` (`:` ile ayrılmış) → `XDG_MUSIC_DIR` →
    /// `HOME/Müzik` → `HOME/Music`. Hiçbiri yoksa boş liste döner —
    /// uydurma bir yol seçmek, kullanıcının müziğini "bulamadım" yerine
    /// "yanlış yerde aradım" hatasına çevirir.
    #[must_use]
    pub fn music_dirs(&self) -> Vec<PathBuf> {
        if let Some(raw) = non_empty_env(MUSIC_DIRS_ENV) {
            return raw
                .split(':')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(PathBuf::from)
                .collect();
        }
        if let Some(dir) = non_empty_env("XDG_MUSIC_DIR") {
            return vec![PathBuf::from(dir)];
        }
        if let Some(home) = non_empty_env("HOME") {
            let home = PathBuf::from(home);
            // Türkçe ve İngilizce yerelin varsayılan adları.
            return [home.join("Müzik"), home.join("Music")]
                .into_iter()
                .filter(|dir| dir.is_dir())
                .collect();
        }
        Vec::new()
    }

    /// Veri dizinini oluşturur.
    ///
    /// # Errors
    /// Dizin oluşturulamazsa.
    pub fn ensure_data_dir(&self) -> Result<()> {
        std::fs::create_dir_all(&self.data_dir)
            .map_err(|source| crate::error::io_err(Stage::ConfigLoad, &self.data_dir, source))
    }
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_hang_off_the_data_dir() {
        let config = Config::with_data_dir("/veri/tune");
        assert_eq!(
            config.database_path(),
            PathBuf::from("/veri/tune/library.db")
        );
        assert_eq!(
            config.last_run_path(),
            PathBuf::from("/veri/tune/last-run.json")
        );
        assert_eq!(
            config.servers_path(),
            PathBuf::from("/veri/tune/servers.json")
        );
    }
}
