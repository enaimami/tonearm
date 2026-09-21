//! Tema yükleyici (PLAN §3.3, D-037/D-038).
//!
//! **Neden burada, `headshell-core`'da değil.** Altın Kural'ın testi "bunu GUI de
//! isteyecek mi" değil, "çekirdek bunu sunabilir mi". Tema bir CSS custom
//! property mekanizması; mobil bağlamalar (K7) CSS kullanmayacak, CLI'nin
//! `--json` çıktısının teması yok. Yani bu bir çekirdek kavramı değil,
//! webview'e özgü bir kabuk işi — ve kabukta kalması gereken tek şey de bu
//! tür şeyler.
//!
//! ## Sözleşme
//!
//! Bir tema iki dosyalı bir dizin:
//!
//! ```text
//! <data_dir>/themes/<id>/
//!   theme.json   # { "name": ..., "author": ..., "api": 1 }
//!   theme.css    # yalnızca :root { --headshell-*: ...; }
//! ```
//!
//! Dizin adı **kimliktir**; manifestteki `name` yalnızca gösterim içindir.
//!
//! ## Üç kural, üçü de K9
//!
//! 1. **`api` uyuşmazlığı sessizce yok sayılmaz.** Tema reddedilir ve hangi
//!    sürümü beklediği yazılır. "Temam görünmüyor" bir tanı sorusu olmamalı.
//! 2. **`:root` dışına taşan tema reddedilmez, işaretlenir** (D-038).
//!    Garanti her zaman yalnızca `:root`'taki token'lar için geçerli;
//!    genişletilmiş tema yüklenir ama listede öyle etiketlenir.
//! 3. **Kayıp seçim yutulmaz.** Seçili tema diskten silinmişse varsayılana
//!    dönülür, ama dönüldüğü söylenir ([`ActiveTheme::problem`]).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Bu yapının desteklediği tema sözleşmesi sürümü.
///
/// Token adları ya da vaat edilen sınıf adları kaldırıldığında/anlamı
/// değiştiğinde artar. Yeni token eklemek artırmaz — eski temalar çalışmaya
/// devam eder, yalnızca yeni token'ın varsayılanını alırlar.
pub const THEME_API: u32 = 1;

/// Yerleşik referans temalar (PLAN §3.4).
///
/// Derleme zamanında gömülüyorlar ama **ayrıcalıkları yok**: diskteki bir
/// tema gibi aynı doğrulamadan geçiyorlar. Ayrıcalıklı olsalardı sözleşmenin
/// yeterli olduğunu kanıtlamazlardı, yalnızca kendi kendini kanıtlarlardı.
const BUILTIN: &[Builtin] = &[
    Builtin {
        id: "daylight",
        manifest: include_str!("../themes/daylight/theme.json"),
        css: include_str!("../themes/daylight/theme.css"),
    },
    Builtin {
        id: "contrast",
        manifest: include_str!("../themes/contrast/theme.json"),
        css: include_str!("../themes/contrast/theme.css"),
    },
];

struct Builtin {
    id: &'static str,
    manifest: &'static str,
    css: &'static str,
}

/// `theme.json`'ın şekli. Bilinmeyen alanlar hata değil: ileri bir sürümün
/// eklediği alan yüzünden tema düşmesin.
#[derive(Debug, Deserialize)]
struct Manifest {
    name: String,
    #[serde(default)]
    author: Option<String>,
    api: u32,
}

/// Yüklenebilir bir tema — listede gösterilen hâli.
#[derive(Debug, Clone, Serialize)]
pub struct ThemeInfo {
    /// Dizin adı. Seçim bununla saklanıyor.
    pub id: String,
    pub name: String,
    pub author: Option<String>,
    /// `:root` dışına taşıyor mu (D-038). `true` ise garanti yok.
    pub extended: bool,
    /// Uygulamayla gelen mi, kullanıcının koyduğu mu.
    pub builtin: bool,
}

/// Reddedilen tema ve **sebebi**. Sessiz atlama yok (K9).
#[derive(Debug, Clone, Serialize)]
pub struct RejectedTheme {
    pub id: String,
    pub reason: String,
}

/// `themes_list` cevabı.
#[derive(Debug, Clone, Serialize)]
pub struct ThemeList {
    /// Kullanıcının tema koyacağı dizin. Yoksa da gösteriliyor — "nereye
    /// koyayım" sorusu arayüzde cevaplanmalı.
    pub dir: PathBuf,
    /// Bu yapının desteklediği sözleşme sürümü.
    pub api: u32,
    /// Seçili tema; `None` ise varsayılan (gömülü `style.css`).
    pub active: Option<String>,
    pub themes: Vec<ThemeInfo>,
    pub rejected: Vec<RejectedTheme>,
}

/// Uygulanacak tema: webview'in `<style>` etiketine koyacağı CSS.
#[derive(Debug, Clone, Serialize, Default)]
pub struct ActiveTheme {
    pub id: Option<String>,
    pub name: Option<String>,
    /// Boş dize = varsayılan tema (hiçbir token geçersiz kılınmıyor).
    pub css: String,
    pub extended: bool,
    /// Seçim uygulanamadıysa sebebi. Varsayılana **sessizce** dönülmüyor.
    pub problem: Option<String>,
}

/// Seçimin saklandığı dosyanın şekli.
#[derive(Debug, Default, Serialize, Deserialize)]
struct UiSettings {
    #[serde(default)]
    theme: Option<String>,
}

/// Tema dizini + seçim dosyası. Durum tutmuyor: her çağrı diski okur.
///
/// Bilerek: tema yazarı dosyayı düzenleyip listeyi yenilediğinde değişikliği
/// görmeli. Belleğe alınmış bir liste, "neden değişmiyor" sorusunu doğururdu.
pub struct ThemeStore {
    dir: PathBuf,
    settings: PathBuf,
}

impl ThemeStore {
    /// Veri dizinine göre kurar.
    #[must_use]
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join("themes"),
            settings: data_dir.join("ui.json"),
        }
    }

    /// Yüklenebilir ve reddedilen temaların tamamı.
    ///
    /// # Errors
    /// Tema dizini var ama okunamıyorsa. Dizin **yoksa** hata değil: henüz
    /// tema koymamış olmak bir arıza değil.
    pub fn list(&self) -> Result<ThemeList, String> {
        let mut themes = Vec::new();
        let mut rejected = Vec::new();

        for entry in BUILTIN {
            match load_builtin(entry) {
                Ok((info, _)) => themes.push(info),
                Err(reason) => rejected.push(RejectedTheme {
                    id: entry.id.to_owned(),
                    reason,
                }),
            }
        }

        for id in self.disk_ids()? {
            if BUILTIN.iter().any(|entry| entry.id == id) {
                // Sessizce gölgelemek yerine söylüyoruz: kullanıcı hangi
                // dosyanın kazandığını tahmin etmek zorunda kalmasın.
                rejected.push(RejectedTheme {
                    id: id.clone(),
                    reason: format!(
                        "yerleşik bir temayla aynı ad ({id}); dizini başka bir adla yeniden adlandırın"
                    ),
                });
                continue;
            }
            match self.load_from_disk(&id) {
                Ok((info, _)) => themes.push(info),
                Err(reason) => rejected.push(RejectedTheme { id, reason }),
            }
        }

        themes.sort_by(|a, b| a.name.cmp(&b.name));
        rejected.sort_by(|a, b| a.id.cmp(&b.id));

        Ok(ThemeList {
            dir: self.dir.clone(),
            api: THEME_API,
            active: self.selection()?,
            themes,
            rejected,
        })
    }

    /// Kayıtlı seçimi yükler. Açılışta bir kez çağrılıyor.
    ///
    /// # Errors
    /// Seçim dosyası okunamıyorsa (bozuk JSON dahil) — o durumda hangi
    /// dosyanın bozuk olduğu söylenmeli, varsayılana kaçılmamalı.
    pub fn active(&self) -> Result<ActiveTheme, String> {
        let Some(id) = self.selection()? else {
            return Ok(ActiveTheme::default());
        };
        match self.load(&id) {
            Ok(theme) => Ok(theme),
            // Tema silinmiş ya da bozulmuş olabilir: varsayılana dönüyoruz
            // ama **sebebiyle**. Sessiz dönüş, kullanıcının temasının neden
            // kaybolduğunu hiç öğrenememesi olurdu.
            Err(reason) => Ok(ActiveTheme {
                problem: Some(format!("seçili tema \"{id}\" uygulanamadı: {reason}")),
                ..ActiveTheme::default()
            }),
        }
    }

    /// Temayı seçer ve seçimi kalıcı yazar. `None` = varsayılana dön.
    ///
    /// Sıra kasıtlı: **önce doğrula, sonra yaz.** Yüklenemeyen bir temayı
    /// kaydetmek, uygulamayı bir dahaki açılışta bozuk bir seçimle başlatırdı.
    ///
    /// # Errors
    /// Tema yüklenemezse ya da seçim dosyası yazılamazsa.
    pub fn select(&self, id: Option<String>) -> Result<ActiveTheme, String> {
        let theme = match &id {
            Some(id) => self.load(id)?,
            None => ActiveTheme::default(),
        };
        self.write_selection(id.as_deref())?;
        Ok(theme)
    }

    /// Tek bir temayı yükler (yerleşik ya da diskten).
    fn load(&self, id: &str) -> Result<ActiveTheme, String> {
        let (info, css) = match BUILTIN.iter().find(|entry| entry.id == id) {
            Some(entry) => load_builtin(entry)?,
            None => self.load_from_disk(id)?,
        };
        Ok(ActiveTheme {
            id: Some(info.id),
            name: Some(info.name),
            css,
            extended: info.extended,
            problem: None,
        })
    }

    fn load_from_disk(&self, id: &str) -> Result<(ThemeInfo, String), String> {
        let dir = self.dir.join(id);
        let manifest = read_file(&dir.join("theme.json"))?;
        let css = read_file(&dir.join("theme.css"))?;
        parse(id, &manifest, &css, false)
    }

    /// Tema dizinindeki alt dizin adları. Dizin yoksa boş liste.
    fn disk_ids(&self) -> Result<Vec<String>, String> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(format!("{} okunamadı: {err}", self.dir.display())),
        };

        let mut ids = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|err| format!("{} okunamadı: {err}", self.dir.display()))?;
            if !entry.path().is_dir() {
                continue;
            }
            match entry.file_name().into_string() {
                Ok(name) => ids.push(name),
                // UTF-8 olmayan dizin adı: yutmuyoruz, ama kimliği yazamadığımız
                // için de listeye koyamıyoruz — hata dizinin tamamını düşürür.
                Err(name) => {
                    return Err(format!(
                        "{} içinde UTF-8 olmayan bir dizin adı var: {name:?}",
                        self.dir.display()
                    ));
                }
            }
        }
        ids.sort();
        Ok(ids)
    }

    fn selection(&self) -> Result<Option<String>, String> {
        let text = match fs::read_to_string(&self.settings) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(format!("{} okunamadı: {err}", self.settings.display())),
        };
        let settings: UiSettings = serde_json::from_str(&text)
            .map_err(|err| format!("{} bozuk: {err}", self.settings.display()))?;
        Ok(settings.theme)
    }

    fn write_selection(&self, id: Option<&str>) -> Result<(), String> {
        if let Some(parent) = self.settings.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("{} oluşturulamadı: {err}", parent.display()))?;
        }
        let settings = UiSettings {
            theme: id.map(str::to_owned),
        };
        let text = serde_json::to_string_pretty(&settings)
            .map_err(|err| format!("tema seçimi yazıya çevrilemedi: {err}"))?;
        fs::write(&self.settings, text)
            .map_err(|err| format!("{} yazılamadı: {err}", self.settings.display()))
    }
}

fn load_builtin(entry: &Builtin) -> Result<(ThemeInfo, String), String> {
    parse(entry.id, entry.manifest, entry.css, true)
}

fn read_file(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| format!("{} okunamadı: {err}", path.display()))
}

/// Manifest + CSS → tema. Doğrulamanın tamamı burada.
fn parse(
    id: &str,
    manifest: &str,
    css: &str,
    builtin: bool,
) -> Result<(ThemeInfo, String), String> {
    let manifest: Manifest =
        serde_json::from_str(manifest).map_err(|err| format!("theme.json bozuk: {err}"))?;

    if manifest.api != THEME_API {
        // K9: reddediyoruz ama **hangi sürüm** sorusunu cevapsız bırakmıyoruz.
        return Err(format!(
            "tema sürüm {} istiyor, bu yapı sürüm {THEME_API} sunuyor",
            manifest.api
        ));
    }

    Ok((
        ThemeInfo {
            id: id.to_owned(),
            name: manifest.name,
            author: manifest.author,
            extended: !targets_only_root(css),
            builtin,
        },
        css.to_owned(),
    ))
}

/// CSS yalnızca `:root` bloğuna mı yazıyor?
///
/// D-038: "CSS ayrıştırmadan, yalnızca `:root { ... }` bloğu dışında başka
/// bir seçici var mı diye bakan basit bir kontrol yeter." Burada yapılan tam
/// olarak o — süslü parantez derinliği sayılıyor, en dış seviyedeki her
/// seçici `:root` olmalı. `@media` gibi bir sarmalayıcı da "genişletilmiş"
/// sayılıyor: token'ları koşullu değiştirmek de garantinin dışında.
///
/// Yanlış pozitif ihtimali kabul edilmiş bir takas: bir dizede geçen `{`
/// karakteri sayımı bozabilir. Bedeli yanlış bir **etiket**, kırılan bir tema
/// değil — reddetmiyoruz (D-038), yalnızca işaretliyoruz.
fn targets_only_root(css: &str) -> bool {
    let css = strip_comments(css);
    let mut depth = 0_u32;
    let mut selector = String::new();

    for ch in css.chars() {
        match ch {
            '{' => {
                if depth == 0 && selector.split_whitespace().collect::<String>() != ":root" {
                    return false;
                }
                depth += 1;
                selector.clear();
            }
            '}' => {
                depth = depth.saturating_sub(1);
                selector.clear();
            }
            _ if depth == 0 => selector.push(ch),
            _ => {}
        }
    }

    // Blok dışında kalan sarkıntı: `@import url(...)` gibi noktalı virgülle
    // biten at-kuralları da sözleşmenin dışında.
    selector.trim().is_empty()
}

/// `/* ... */` yorumlarını çıkarır. Yorumdaki bir `{` sayımı bozmasın diye.
fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            // Kapanmamış yorum: geri kalanın tamamı yorumdur.
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    // Testlerde `expect` serbest (CLAUDE.md); üretim yolunda değil.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn store(dir: &Path) -> ThemeStore {
        ThemeStore::new(dir)
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("headshell-theme-test-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("geçici dizin");
        dir
    }

    fn write_theme(root: &Path, id: &str, manifest: &str, css: &str) {
        let dir = root.join("themes").join(id);
        fs::create_dir_all(&dir).expect("tema dizini");
        fs::write(dir.join("theme.json"), manifest).expect("manifest");
        fs::write(dir.join("theme.css"), css).expect("css");
    }

    // ————————————————— kapsam taraması (D-038)

    #[test]
    fn a_root_only_theme_is_not_marked_extended() {
        assert!(targets_only_root(":root { --headshell-bg: #fff; }"));
        assert!(targets_only_root(
            "/* yorum { } */\n:root {\n  --headshell-bg: #fff;\n}\n"
        ));
    }

    #[test]
    fn a_theme_that_targets_a_class_is_marked_extended() {
        assert!(!targets_only_root(
            ":root { --headshell-bg: #fff; }\n.topbar { border: 0; }"
        ));
    }

    #[test]
    fn a_media_query_counts_as_extended() {
        // Token'ları koşullu değiştirmek de garantinin dışında: sözleşme
        // "bu token'lar şu değerlerdedir" diyor, "duruma göre değişir" demiyor.
        assert!(!targets_only_root(
            "@media (prefers-color-scheme: light) { :root { --headshell-bg: #fff; } }"
        ));
    }

    #[test]
    fn a_dangling_at_rule_counts_as_extended() {
        assert!(!targets_only_root(
            "@import url(baska.css);\n:root { --headshell-bg: #fff; }"
        ));
    }

    // ————————————————— manifest doğrulama

    #[test]
    fn a_theme_from_a_future_api_is_rejected_with_both_versions() {
        let manifest = format!(r#"{{"name":"Gelecek","api":{}}}"#, THEME_API + 1);
        let err = parse("future", &manifest, ":root {}", false).expect_err("reddedilmeli");
        // Hangi sürümü istediği **ve** neyi sunduğumuz yazmalı (K9).
        assert!(err.contains(&(THEME_API + 1).to_string()), "{err}");
        assert!(err.contains(&THEME_API.to_string()), "{err}");
    }

    #[test]
    fn a_broken_manifest_is_rejected_not_ignored() {
        let err = parse("broken", "{ bu json değil", ":root {}", false).expect_err("reddedilmeli");
        assert!(err.contains("theme.json"), "{err}");
    }

    #[test]
    fn an_unknown_manifest_field_does_not_break_a_theme() {
        // İleri bir sürümün eklediği alan yüzünden tema düşmemeli.
        let manifest = r#"{"name":"Örnek","api":1,"homepage":"https://ornek"}"#;
        let (info, _) = parse("example", manifest, ":root {}", false).expect("yüklenmeli");
        assert_eq!(info.name, "Örnek");
    }

    // ————————————————— referans temalar (PLAN §3.4)

    #[test]
    fn both_reference_themes_load_and_stay_inside_the_contract() {
        assert_eq!(BUILTIN.len(), 2, "§3.4 en az iki referans teması istiyor");
        for entry in BUILTIN {
            let (info, css) = load_builtin(entry).expect("referans tema yüklenmeli");
            assert_eq!(info.id, entry.id);
            assert!(info.builtin);
            // Referans tema genişletilmiş olamaz: sözleşmenin **yettiğini**
            // kanıtlaması gerekiyor, aştığını değil.
            assert!(!info.extended, "{} :root dışına taşıyor", entry.id);
            assert!(
                css.contains("--headshell-bg"),
                "{} token yazmıyor",
                entry.id
            );
        }
    }

    #[test]
    fn the_two_reference_themes_differ_on_more_than_color() {
        // "İki tema" ölçütü iki paletle karşılanmaz; token setinin renk
        // dışındaki ekseni de sınanmalı (yarıçap, süre).
        let contrast = BUILTIN
            .iter()
            .find(|entry| entry.id == "contrast")
            .expect("contrast teması");
        assert!(contrast.css.contains("--headshell-radius-sm: 0"));
        assert!(contrast.css.contains("--headshell-duration: 0ms"));
    }

    // ————————————————— dizin taraması ve seçim

    #[test]
    fn an_absent_theme_directory_is_not_an_error() {
        let root = temp_dir("no-dir");
        let list = store(&root).list().expect("liste");
        assert_eq!(list.active, None);
        // Yerleşikler yine de görünüyor.
        assert_eq!(list.themes.len(), BUILTIN.len());
        assert!(list.rejected.is_empty());
    }

    #[test]
    fn a_rejected_theme_appears_in_the_list_with_its_reason() {
        let root = temp_dir("rejected");
        write_theme(
            &root,
            "future",
            r#"{"name":"Gelecek","api":99}"#,
            ":root {}",
        );
        let list = store(&root).list().expect("liste");
        assert!(!list.themes.iter().any(|t| t.id == "future"));
        let rejected = list
            .rejected
            .iter()
            .find(|t| t.id == "future")
            .expect("sebebiyle listelenmeli");
        assert!(rejected.reason.contains("99"), "{}", rejected.reason);
    }

    #[test]
    fn a_disk_theme_that_shadows_a_builtin_is_rejected_by_name() {
        let root = temp_dir("shadow");
        write_theme(&root, "daylight", r#"{"name":"Sahte","api":1}"#, ":root {}");
        let list = store(&root).list().expect("liste");
        // Yerleşik olan hâlâ yüklenebilir; gölgeleyen sessizce kazanmıyor.
        assert_eq!(list.themes.iter().filter(|t| t.id == "daylight").count(), 1);
        assert!(list.themes.iter().any(|t| t.id == "daylight" && t.builtin));
        assert!(list.rejected.iter().any(|t| t.id == "daylight"));
    }

    #[test]
    fn an_extended_theme_loads_but_is_flagged() {
        // D-038: ne reddet ne serbest — işaretle.
        let root = temp_dir("extended");
        write_theme(
            &root,
            "wide",
            r#"{"name":"Geniş","api":1}"#,
            ":root { --headshell-bg: #000; }\n.topbar { border: 0; }",
        );
        let list = store(&root).list().expect("liste");
        let info = list
            .themes
            .iter()
            .find(|t| t.id == "wide")
            .expect("yüklenmeli");
        assert!(info.extended);
    }

    #[test]
    fn selecting_a_theme_survives_a_restart() {
        let root = temp_dir("select");
        store(&root)
            .select(Some("daylight".to_owned()))
            .expect("seçilmeli");
        // Yeni bir depo = yeni bir açılış: seçim diskten okunuyor.
        let active = store(&root).active().expect("okunmalı");
        assert_eq!(active.id.as_deref(), Some("daylight"));
        assert!(active.css.contains("--headshell-bg"));
        assert_eq!(active.problem, None);
    }

    #[test]
    fn selecting_none_returns_to_the_default_theme() {
        let root = temp_dir("default");
        let store = store(&root);
        store
            .select(Some("contrast".to_owned()))
            .expect("seçilmeli");
        let active = store.select(None).expect("varsayılana dönmeli");
        assert_eq!(active.id, None);
        assert!(active.css.is_empty());
        assert_eq!(store.list().expect("liste").active, None);
    }

    #[test]
    fn a_theme_that_cannot_load_is_never_written_as_the_selection() {
        let root = temp_dir("no-write");
        let store = store(&root);
        store
            .select(Some("daylight".to_owned()))
            .expect("seçilmeli");
        store.select(Some("yok".to_owned())).expect_err("düşmeli");
        // Önce doğrula, sonra yaz: başarısız seçim eskisini bozmadı.
        assert_eq!(
            store.active().expect("okunmalı").id.as_deref(),
            Some("daylight")
        );
    }

    #[test]
    fn a_selection_pointing_at_a_missing_theme_falls_back_out_loud() {
        let root = temp_dir("missing");
        write_theme(&root, "gone", r#"{"name":"Giden","api":1}"#, ":root {}");
        let store = store(&root);
        store.select(Some("gone".to_owned())).expect("seçilmeli");
        fs::remove_dir_all(root.join("themes").join("gone")).expect("silinmeli");

        let active = store.active().expect("varsayılana dönmeli");
        assert_eq!(active.id, None);
        // Sessiz dönüş olsaydı kullanıcı temasının neden kaybolduğunu
        // hiç öğrenemezdi (K9).
        let problem = active.problem.expect("sebebi söylenmeli");
        assert!(problem.contains("gone"), "{problem}");
    }

    #[test]
    fn a_broken_settings_file_is_reported_not_swallowed() {
        let root = temp_dir("broken-settings");
        fs::write(root.join("ui.json"), "{ bozuk").expect("yazılmalı");
        let err = store(&root).active().expect_err("düşmeli");
        assert!(err.contains("ui.json"), "{err}");
    }
}
