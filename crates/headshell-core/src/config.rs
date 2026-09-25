//! Veri dizini ve yapılandırma.
//!
//! Yol çözümlemesi çekirdekte: CLI, GUI ve mobil aynı dizini bulmalı.
//!
//! ## Platformlar (D-070)
//!
//! Veri dizini her işletim sisteminin **kendi** yerinde durur; ilk yazım
//! yalnızca `HOME`'a bakıyordu ve standart bir Windows `HOME` tanımlamadığı
//! için orada hiç açılmıyordu:
//!
//! | sistem | varsayılan |
//! |---|---|
//! | Linux, BSD ve öteki Unix'ler | `$XDG_DATA_HOME/headshell` → `~/.local/share/headshell` |
//! | macOS | `~/Library/Application Support/headshell` |
//! | Windows | `%LOCALAPPDATA%\headshell` |
//!
//! `HEADSHELL_DATA_DIR` her sistemde önce gelir; `XDG_DATA_HOME` açıkça
//! tanımlanmışsa macOS ve Windows'ta da uyulur — kullanıcı bilerek koymuştur.
//!
//! Ortam okuması saf fonksiyonlarda ([`resolve_data_dir`], [`resolve_music_dirs`]):
//! Rust 2024'te `set_var` `unsafe` ve workspace `unsafe_code = "forbid"`
//! diyor, yani Windows'un ve macOS'un dalları bu yolla her makinede sınanıyor.

use std::path::{Path, PathBuf};

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};

/// Veri dizinini elle vermek için ortam değişkeni (testler ve taşınabilir kurulum).
pub const DATA_DIR_ENV: &str = "HEADSHELL_DATA_DIR";

/// Müzik dizinlerini elle vermek için ortam değişkeni.
///
/// Liste `PATH` gibi yazılır: Unix'te `:`, Windows'ta `;` ile ayrılır —
/// `C:\Müzik` gibi bir yolun içindeki iki nokta ayırıcı sayılmasın diye.
pub const MUSIC_DIRS_ENV: &str = "HEADSHELL_MUSIC_DIRS";

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

    /// Veri dizinini ortamdan bulur — bu işletim sisteminin kuralıyla.
    ///
    /// # Errors
    /// Hiçbir aday bulunamazsa. Sessizce geçici dizine düşmek, kullanıcının
    /// geçmişini fark ettirmeden kaybetmek demektir; hata bu sistemde hangi
    /// değişkenlere bakıldığını söyler.
    pub fn discover() -> Result<Self> {
        match resolve_data_dir(std::env::consts::OS, &non_empty_env) {
            Some(dir) => Ok(Self::with_data_dir(dir)),
            None => Err(Error::new(
                Stage::ConfigLoad,
                ErrorKind::NotFound {
                    what: format!(
                        "veri dizini — bu sistemde bakılan değişkenlerin hiçbiri tanımlı değil: {}",
                        data_dir_sources(std::env::consts::OS).join(", ")
                    ),
                },
            )),
        }
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

    /// Eklenti kataloğunun adresi (D-071).
    ///
    /// `HEADSHELL_PLUGIN_INDEX` verilmişse o — bir çatal, bir ayna ya da
    /// sınama için yerel bir sunucu — yoksa `headshell/plugins` deposunun
    /// indeksi. Her çağrıda ortamdan okunur, `music_dirs` gibi.
    #[must_use]
    pub fn plugin_index_url(&self) -> String {
        crate::plugin::catalog::resolve_index_url(&non_empty_env)
    }

    /// Bir eklentinin durum dizini: `host.storage`'ın dosyası ve motorun
    /// kurduğu araçların çalışma dizini (D-069). Eklentinin kendisi dosya
    /// sistemine dokunamaz; bu dizini onun adına motor kullanır.
    #[must_use]
    pub fn plugin_state_dir(&self, plugin: &str) -> PathBuf {
        self.plugins_dir().join(plugin).join("state")
    }

    /// Eklenti motorunun kurduğu eserler (D-055): `<data_dir>/runtime`.
    ///
    /// `plugins_dir`'in **dışında** ve bilerek: bir eser tek bir eklentiye
    /// ait değil. İki eklenti aynı `yt-dlp` sürümünü isterse aynı dosyayı
    /// paylaşır, ve bir eklenti silindiğinde ötekinin çalışma zamanı
    /// gitmez.
    #[must_use]
    pub fn runtime_dir(&self) -> PathBuf {
        self.data_dir.join("runtime")
    }

    /// Son çalıştırmanın tanı raporu.
    #[must_use]
    pub fn last_run_path(&self) -> PathBuf {
        self.data_dir.join("last-run.json")
    }

    /// Yerel sağlayıcının tarayacağı müzik dizinleri.
    ///
    /// Sıra: `HEADSHELL_MUSIC_DIRS` → `XDG_MUSIC_DIR` → bu sistemin olağan
    /// müzik dizini (Windows'ta `%USERPROFILE%\Music`, macOS'ta `~/Music`,
    /// ötekilerde `~/Müzik` ve `~/Music`). Hiçbiri yoksa boş liste döner —
    /// uydurma bir yol seçmek, kullanıcının müziğini "bulamadım" yerine
    /// "yanlış yerde aradım" hatasına çevirir.
    #[must_use]
    pub fn music_dirs(&self) -> Vec<PathBuf> {
        resolve_music_dirs(std::env::consts::OS, &non_empty_env)
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

/// Bir işletim sisteminde veri dizininin nereden bulunacağı, sırasıyla —
/// hata mesajı ve belge bu listeyi kullanır.
fn data_dir_sources(os: &str) -> Vec<&'static str> {
    let default: &[&'static str] = match os {
        "windows" => &["LOCALAPPDATA", "USERPROFILE"],
        _ => &["HOME"],
    };
    [DATA_DIR_ENV, "XDG_DATA_HOME"]
        .into_iter()
        .chain(default.iter().copied())
        .collect()
}

/// Veri dizinini verilen ortamdan çözer. `os`, `std::env::consts::OS`'un
/// değeri (`"linux"`, `"macos"`, `"windows"`, `"freebsd"`…).
fn resolve_data_dir(os: &str, env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if let Some(dir) = env(DATA_DIR_ENV) {
        return Some(PathBuf::from(dir));
    }
    if let Some(dir) = env("XDG_DATA_HOME") {
        return Some(PathBuf::from(dir).join("headshell"));
    }
    match os {
        // `LOCALAPPDATA` her kullanıcı oturumunda tanımlı. Olmadığı ender
        // durumda (bazı hizmet hesapları) aynı yer profilden kurulur.
        "windows" => env("LOCALAPPDATA")
            .map(PathBuf::from)
            .or_else(|| {
                env("USERPROFILE").map(|home| PathBuf::from(home).join("AppData").join("Local"))
            })
            .map(|base| base.join("headshell")),
        "macos" => env("HOME").map(|home| {
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("headshell")
        }),
        _ => env("HOME").map(|home| {
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("headshell")
        }),
    }
}

/// Müzik dizinlerini verilen ortamdan çözer. Olağan dizinler yalnızca
/// **varsa** listeye girer; açıkça verilenler olduğu gibi girer — yoklarsa
/// bunu tarama söyler.
fn resolve_music_dirs(os: &str, env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    if let Some(raw) = env(MUSIC_DIRS_ENV) {
        // `split_paths` bu sistemin ayırıcısını kullanıyor: Windows'ta `;`,
        // ötekilerde `:`. Elle `:` ile bölmek `C:\Müzik`'i ikiye ayırırdı.
        return std::env::split_paths(&raw)
            .filter(|path| !path.as_os_str().is_empty())
            .collect();
    }
    if let Some(dir) = env("XDG_MUSIC_DIR") {
        return vec![PathBuf::from(dir)];
    }
    let candidates: Vec<PathBuf> = match os {
        "windows" => env("USERPROFILE")
            .map(|home| vec![PathBuf::from(home).join("Music")])
            .unwrap_or_default(),
        "macos" => env("HOME")
            .map(|home| vec![PathBuf::from(home).join("Music")])
            .unwrap_or_default(),
        // Türkçe ve İngilizce yerelin varsayılan adları.
        _ => env("HOME")
            .map(|home| {
                let home = PathBuf::from(home);
                vec![home.join("Müzik"), home.join("Music")]
            })
            .unwrap_or_default(),
    };
    candidates.into_iter().filter(|dir| dir.is_dir()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sahte ortam: yalnızca verilen değişkenler tanımlı.
    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        move |key: &str| {
            pairs
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
        }
    }

    /// Standart bir Windows `HOME` tanımlamaz; ilk yazım tam da bu yüzden
    /// orada hiç açılmıyordu (D-070).
    #[test]
    fn windows_uses_localappdata_and_does_not_need_home() {
        let env = env_of(&[
            ("LOCALAPPDATA", r"C:\Users\enai\AppData\Local"),
            ("USERPROFILE", r"C:\Users\enai"),
        ]);
        assert_eq!(
            resolve_data_dir("windows", &env),
            Some(PathBuf::from(r"C:\Users\enai\AppData\Local").join("headshell"))
        );

        let only_profile = env_of(&[("USERPROFILE", r"C:\Users\enai")]);
        assert_eq!(
            resolve_data_dir("windows", &only_profile),
            Some(
                PathBuf::from(r"C:\Users\enai")
                    .join("AppData")
                    .join("Local")
                    .join("headshell")
            )
        );

        // `HOME` Windows'ta aday bile değil: Git Bash onu tanımlar ve
        // aynı kullanıcının iki kabukta iki ayrı kütüphanesi olurdu.
        let git_bash = env_of(&[("HOME", "/c/Users/enai")]);
        assert_eq!(resolve_data_dir("windows", &git_bash), None);
    }

    #[test]
    fn macos_uses_application_support() {
        let env = env_of(&[("HOME", "/Users/enai")]);
        assert_eq!(
            resolve_data_dir("macos", &env),
            Some(PathBuf::from(
                "/Users/enai/Library/Application Support/headshell"
            ))
        );
    }

    #[test]
    fn linux_and_other_unixes_use_the_xdg_default() {
        let env = env_of(&[("HOME", "/home/enai")]);
        for os in ["linux", "freebsd", "netbsd", "openbsd"] {
            assert_eq!(
                resolve_data_dir(os, &env),
                Some(PathBuf::from("/home/enai/.local/share/headshell")),
                "{os}"
            );
        }
    }

    #[test]
    fn an_explicit_choice_wins_on_every_system() {
        for os in ["linux", "macos", "windows", "freebsd"] {
            let explicit = env_of(&[
                ("HEADSHELL_DATA_DIR", "/secilen"),
                ("XDG_DATA_HOME", "/xdg"),
                ("LOCALAPPDATA", "/yerel"),
                ("HOME", "/ev"),
            ]);
            assert_eq!(
                resolve_data_dir(os, &explicit),
                Some(PathBuf::from("/secilen")),
                "{os}"
            );
            let xdg = env_of(&[
                ("XDG_DATA_HOME", "/xdg"),
                ("LOCALAPPDATA", "/yerel"),
                ("HOME", "/ev"),
            ]);
            assert_eq!(
                resolve_data_dir(os, &xdg),
                Some(PathBuf::from("/xdg/headshell")),
                "{os}: açıkça tanımlanmış XDG_DATA_HOME uyulmalı"
            );
        }
    }

    #[test]
    fn the_error_names_what_this_system_looked_at() {
        assert_eq!(
            data_dir_sources("windows"),
            vec![
                "HEADSHELL_DATA_DIR",
                "XDG_DATA_HOME",
                "LOCALAPPDATA",
                "USERPROFILE"
            ]
        );
        assert_eq!(
            data_dir_sources("linux"),
            vec!["HEADSHELL_DATA_DIR", "XDG_DATA_HOME", "HOME"]
        );
        assert_eq!(resolve_data_dir("linux", &env_of(&[])), None);
    }

    /// Liste bu sistemin kendi ayırıcısıyla bölünür; Windows'ta sürücü
    /// harfinin iki noktası ayırıcı değildir.
    #[test]
    fn a_music_dir_list_is_split_with_this_systems_separator() {
        let separator = if cfg!(windows) { ';' } else { ':' };
        let first = if cfg!(windows) { r"C:\Muzik" } else { "/muzik" };
        let raw = format!("{first}{separator}{separator}/arsiv");
        let env = env_of(&[("HEADSHELL_MUSIC_DIRS", &raw)]);
        assert_eq!(
            resolve_music_dirs(std::env::consts::OS, &env),
            vec![PathBuf::from(first), PathBuf::from("/arsiv")]
        );
    }

    #[test]
    fn default_music_dirs_follow_the_system_and_must_exist() {
        let home = crate::test_support::TempDir::new("config-muzik");
        std::fs::create_dir_all(home.join("Music")).unwrap();
        let root = home.to_str().unwrap();

        for (os, key) in [
            ("windows", "USERPROFILE"),
            ("macos", "HOME"),
            ("linux", "HOME"),
        ] {
            let env = env_of(&[(key, root)]);
            assert_eq!(
                resolve_music_dirs(os, &env),
                vec![home.join("Music")],
                "{os}"
            );
        }
        // Var olmayan olağan dizin uydurulmaz.
        let empty = crate::test_support::TempDir::new("config-muzik-bos");
        let env = env_of(&[("HOME", empty.to_str().unwrap())]);
        assert!(resolve_music_dirs("linux", &env).is_empty());
    }

    #[test]
    fn paths_hang_off_the_data_dir() {
        let config = Config::with_data_dir("/veri/headshell");
        assert_eq!(
            config.database_path(),
            PathBuf::from("/veri/headshell/library.db")
        );
        assert_eq!(
            config.last_run_path(),
            PathBuf::from("/veri/headshell/last-run.json")
        );
        assert_eq!(
            config.servers_path(),
            PathBuf::from("/veri/headshell/servers.json")
        );
    }
}
