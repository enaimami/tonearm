//! Arayüzün kendi içindeki bağları ve §3.3 sınıf sözleşmesi.
//!
//! Webview'de tip denetimi yok: `$("playQuery")` yazım hatasıyla `null`
//! döner, `null.addEventListener` açılışta patlar ve **pencere boş kalır**.
//! Derleyici bunu görmez, `cargo test` de görmezdi — bu dosya görüyor.
//!
//! Üç bağ kilitleniyor:
//!
//! 1. `app.js`'in aradığı her `id` `index.html`'de tanımlı.
//! 2. Kenar çubuğundaki her `data-panel` bir `panel-*` bölümüne denk geliyor
//!    ve `app.js`'teki `PANELS` listesiyle aynı kümede — kısayol
//!    (`Ctrl`+`1`…`8`) yanlış panele gitmesin.
//! 3. D-037/2'nin **sınıf sözleşmesi**: temaların hedeflediği sınıf adları
//!    hâlâ ulaşılabilir — bir seçici olarak ya da işaretlemede. README
//!    "habersiz yeniden adlandırılmazlar" diyor; "habersiz" kelimesini bu
//!    test tutuyor.
//!
//! Ayrıca ölü token denetimi: `:root`'ta ilan edilen her `--headshell-*` token'ının
//! en az bir kullanımı olmalı. Kullanılmayan bir token, tema yazarına
//! tutulmayan bir söz verir.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

const APP_JS: &str = include_str!("../ui/app.js");
const INDEX_HTML: &str = include_str!("../ui/index.html");
const STYLE_CSS: &str = include_str!("../ui/style.css");
const STARTUP_ERROR_HTML: &str = include_str!("../ui/startup-error.html");
const STARTUP_ERROR_JS: &str = include_str!("../ui/startup-error.js");

/// `api: 1`'in taahhüt ettiği sınıf adları.
///
/// README bu listeyi "`.topbar`, `.player`, `.queue`, `.toast`, …" diye
/// örnekliyordu; üç nokta test edilemez, bu liste edilir. Bir adı buradan
/// çıkarmak `THEME_API`'yi artırmayı gerektirir (bkz. `src/theme.rs`).
const CONTRACT_CLASSES: &[&str] = &[
    "topbar",
    "brand",
    "play-form",
    "busy",
    "sidebar",
    "nav",
    "content",
    "panel",
    "queue",
    "index",
    "current",
    "empty",
    "hint",
    "row",
    "grid",
    "check",
    "table",
    "num",
    "summary",
    "big",
    "label",
    "ranking",
    "two-col",
    "block",
    "themes",
    "theme",
    "name",
    "author",
    "tag",
    "warn",
    "rejected",
    "id",
    "dropzone",
    "over",
    "player",
    "controls",
    "on",
    "now",
    "title",
    "state",
    "bar",
    "fill",
    "time",
    "toasts",
    "toast",
    "stage",
    "info",
];

/// `haystack` içinde `needle`'dan sonra gelen her eşleşmenin kapanışa kadarki
/// parçasını toplar. Küçük bir tarayıcı: bu iş için bağımlılık eklemeye
/// değmez.
fn collect_between(haystack: &str, open: &str, close: char) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = haystack;
    while let Some(start) = rest.find(open) {
        rest = &rest[start + open.len()..];
        match rest.find(close) {
            Some(end) => {
                found.push(rest[..end].to_owned());
                rest = &rest[end..];
            }
            None => break,
        }
    }
    found
}

#[test]
fn every_id_the_script_looks_up_exists_in_the_page() {
    let wanted = collect_between(APP_JS, "$(\"", '"');
    assert!(wanted.len() > 30, "id taraması boş kaldı: {}", wanted.len());

    let mut missing: Vec<String> = wanted
        .into_iter()
        .filter(|id| !INDEX_HTML.contains(&format!("id=\"{id}\"")))
        .collect();
    missing.sort();
    missing.dedup();

    assert!(
        missing.is_empty(),
        "app.js var olmayan id arıyor (açılışta pencere boş kalır): {missing:?}"
    );
}

#[test]
fn the_sidebar_the_sections_and_the_shortcut_list_name_the_same_panels() {
    let mut from_sidebar = collect_between(INDEX_HTML, "data-panel=\"", '"');
    from_sidebar.sort();

    let list = APP_JS
        .split_once("const PANELS = [")
        .expect("app.js içinde PANELS listesi yok")
        .1
        .split_once(']')
        .expect("PANELS listesi kapanmıyor")
        .0;
    let mut from_script: Vec<String> = collect_between(list, "\"", '"')
        .into_iter()
        .filter(|item| !item.trim().is_empty() && item != ", ")
        .collect();
    from_script.sort();

    assert_eq!(
        from_sidebar, from_script,
        "kenar çubuğu ile Ctrl+sayı kısayolunun panel listesi ayrışmış"
    );

    for panel in &from_sidebar {
        assert!(
            INDEX_HTML.contains(&format!("id=\"panel-{panel}\"")),
            "`{panel}` sekmesinin bölümü yok — sekme boş bir içeriğe götürür"
        );
    }
}

#[test]
fn the_class_names_themes_target_are_still_reachable() {
    // Ölçüt "CSS'te bir kuralı var mı" **değil**: `.panel` hiçbir kural
    // taşımıyor ama DOM'da duruyor ve bir tema onu hedefleyebiliyor. Sözleşme
    // sınıfın *ulaşılabilir* olmasıdır — ya bir seçici olarak, ya işaretlemede.
    let markup_classes = class_attribute_words(INDEX_HTML);
    let mut missing = Vec::new();
    for class in CONTRACT_CLASSES {
        let reachable =
            defines_class(STYLE_CSS, class) || markup_classes.iter().any(|w| w == class);
        if !reachable {
            missing.push(*class);
        }
    }
    assert!(
        missing.is_empty(),
        "sözleşmedeki sınıf artık ne seçici ne işaretleme — temalar bunları hedefliyor (D-037/2): {missing:?}"
    );
}

/// `index.html`'deki bütün `class="…"` niteliklerinin kelimeleri.
fn class_attribute_words(html: &str) -> Vec<String> {
    collect_between(html, "class=\"", '"')
        .iter()
        .flat_map(|value| {
            value
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// `.foo` seçicisi var mı. `.foobar` yanlış eşleşmesin diye sınıf adından
/// sonraki karakter bir ad karakteri olmamalı.
fn defines_class(css: &str, class: &str) -> bool {
    let needle = format!(".{class}");
    let mut rest = css;
    while let Some(at) = rest.find(&needle) {
        let after = &rest[at + needle.len()..];
        let next = after.chars().next();
        if !matches!(next, Some(c) if c.is_alphanumeric() || c == '-' || c == '_') {
            return true;
        }
        rest = after;
    }
    false
}

/// `hidden` özniteliği bir sınıf kuralına yenilmemeli.
///
/// Bu test bir hatadan sonra yazıldı: `.sheet { display: flex }` UA
/// stylesheet'in `[hidden] { display: none }` kuralını eziyordu ve kısayol
/// penceresi her açılışta açık geliyordu. İşaretleme doğruydu, JavaScript
/// doğruydu, yalnızca özgüllük yanlıştı — davranış testi de görmezdi.
#[test]
fn the_hidden_attribute_outranks_layout_rules() {
    // Satır başındaki eşleşme aranıyor: bu dosyanın kendi yorumu da
    // `[hidden] { display: none }` metnini içeriyor ve testin ilk hâli tam
    // ona takılmıştı.
    let rule = STYLE_CSS
        .split_once("\n[hidden] {")
        .expect(
            "`[hidden]` kuralı yok: `display` tanımlayan bir sınıf `hidden` öğeyi ekranda bırakır",
        )
        .1
        .split_once('}')
        .expect("`[hidden]` kuralı kapanmıyor")
        .0;
    assert!(
        rule.contains("display: none") && rule.contains("!important"),
        "`[hidden]` kuralı var ama sınıfları yenmiyor: {rule:?}"
    );
}

#[test]
fn no_declared_token_is_dead() {
    let root = STYLE_CSS
        .split_once(":root {")
        .expect("style.css içinde :root bloğu yok")
        .1
        .split_once("\n}")
        .expect(":root bloğu kapanmıyor")
        .0;

    let declared: Vec<&str> = root
        .lines()
        .filter_map(|line| line.trim().strip_prefix("--"))
        .filter_map(|line| line.split(':').next())
        .map(str::trim)
        .collect();
    assert!(declared.len() >= 14, "token seti küçüldü: {declared:?}");

    for token in declared {
        let uses = STYLE_CSS.matches(&format!("var(--{token})")).count();
        assert!(
            uses > 0,
            "`--{token}` ilan edilmiş ama hiç kullanılmıyor — tema yazarına tutulmayan bir söz"
        );
    }
}

/// Açılış hatası penceresi (D-070) kendi başına bir sayfa: betiğin yazdığı
/// öğe orada olmalı, yoksa kullanıcı yine **boş** bir pencere görür — bu
/// sayfanın var oluş sebebinin tam tersi.
#[test]
fn the_startup_error_page_has_the_element_its_script_writes_to() {
    assert!(
        STARTUP_ERROR_HTML.contains("src=\"startup-error.js\""),
        "sayfa betiğini yüklemiyor"
    );
    let ids = collect_between(STARTUP_ERROR_JS, "getElementById(\"", '"');
    assert!(!ids.is_empty(), "betik hiçbir öğeye yazmıyor");
    for id in ids {
        assert!(
            STARTUP_ERROR_HTML.contains(&format!("id=\"{id}\"")),
            "startup-error.js `{id}` arıyor, sayfada yok"
        );
    }
    // Metin işaretleme olarak yorumlanmamalı: hata zinciri kullanıcı verisi
    // (dosya yolu, sunucu mesajı) taşıyabilir.
    let code_uses_inner_html = STARTUP_ERROR_JS
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .any(|line| line.contains("innerHTML"));
    assert!(
        !code_uses_inner_html,
        "hata metni `innerHTML` ile yazılıyor"
    );
}
