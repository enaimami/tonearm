//! Eklenti kataloğunun birim testleri (D-071).
//!
//! Katalog deposu testin içinde kuruluyor: geçici bir dizine eklentiler
//! yazılıyor, indeks [`build_index`] ile — yani bakımcının kullandığı yoldan
//! — üretiliyor ve sahte HTTP istemcisi indeksle dosyaları sunuyor. Ağa
//! çıkılmıyor; karma, manifest ve köken kaydı kuralları diskte sınanıyor.

use std::sync::Arc;

use super::*;
use crate::net::fake::FakeHttp;
use crate::plugin::consent::ConsentStatus;
use crate::test_support::{TempDir, TestConfig};

const INDEX: &str = "https://katalog.ornek/refs/heads/main/index.json";
const TEMPLATE: &str = "https://katalog.ornek/refs/tags/{name}-{version}/{name}/{path}";
const PLATFORM: &str = "linux-x86_64";

fn run<T>(future: impl std::future::Future<Output = T>) -> T {
    crate::plugin::block_on(future)
}

fn temp_config(label: &str) -> TestConfig {
    TestConfig::new(&format!("catalog-{label}"))
}

fn manifest_json(name: &str, version: &str, net: &[&str]) -> String {
    format!(
        r#"{{"name":"{name}","display_name":"{name} eklentisi","version":"{version}","api":2,"main":"main.js","capabilities":["search"],"permissions":{{"net":{}}}}}"#,
        serde_json::to_string(net).unwrap()
    )
}

fn script(tag: &str) -> String {
    format!(
        "export function health() {{ return {{ reachable: true, detail: \"{tag}\" }}; }}\nexport function search() {{ return []; }}\n"
    )
}

/// Testin içinde kurulan bir katalog deposu.
struct Source {
    dir: TempDir,
}

impl Source {
    fn new(label: &str) -> Self {
        Self {
            dir: TempDir::new(&format!("catalog-kaynak-{label}")),
        }
    }

    fn plugin(&self, name: &str, version: &str, net: &[&str], main: &str) -> &Self {
        let dir = self.dir.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("plugin.json"), manifest_json(name, version, net)).unwrap();
        std::fs::write(dir.join("main.js"), main).unwrap();
        self
    }

    fn build(&self) -> BuiltIndex {
        build_index(&self.dir, TEMPLATE).unwrap_or_else(|err| panic!("{}", err.chain_text()))
    }

    /// İndeksi ve bütün dosyaları sunan sahte istemci.
    fn http(&self) -> FakeHttp {
        self.http_with_index(&self.build().json)
    }

    fn http_with_index(&self, index: &str) -> FakeHttp {
        let built = self.build();
        let mut http = FakeHttp::new().route("index.json", index);
        for plugin in &built.plugins {
            for file in &plugin.files {
                let body =
                    std::fs::read_to_string(self.dir.join(&plugin.name).join(&file.path)).unwrap();
                let pattern = format!(
                    "{}-{}/{}/{}",
                    plugin.name, plugin.version, plugin.name, file.path
                );
                http = http.route(&pattern, &body);
            }
        }
        http
    }
}

fn catalog_of(source: &Source) -> Catalog {
    Catalog::parse(INDEX, source.build().json.as_bytes()).unwrap()
}

fn install_from(config: &Config, source: &Source, name: &str) -> CatalogFetch {
    let http = source.http();
    run(async {
        let catalog = fetch(&http, INDEX).await.unwrap();
        install(config, &http, &catalog, name).await
    })
    .unwrap_or_else(|err| panic!("{}", err.chain_text()))
}

fn state(config: &Config, source: &Source, name: &str) -> InstallState {
    let catalog = catalog_of(source);
    let survey = catalog.survey(config).unwrap();
    survey
        .plugins
        .into_iter()
        .find(|plugin| plugin.name == name)
        .map(|plugin| plugin.installed)
        .unwrap()
}

// --- adres --------------------------------------------------------------

#[test]
fn the_index_url_comes_from_the_environment_or_the_default() {
    assert_eq!(resolve_index_url(&|_| None), DEFAULT_INDEX_URL);
    let mirror =
        |key: &str| (key == INDEX_ENV).then(|| " https://ayna.ornek/index.json ".to_owned());
    assert_eq!(resolve_index_url(&mirror), "https://ayna.ornek/index.json");
    let blank = |key: &str| (key == INDEX_ENV).then(|| "   ".to_owned());
    assert_eq!(resolve_index_url(&blank), DEFAULT_INDEX_URL);
}

/// İndeks güvenin kökü: düz HTTP yalnızca bu makinenin kendisine.
#[test]
fn a_catalog_over_plain_http_is_refused_unless_it_is_this_machine() {
    let http = FakeHttp::new().route("index.json", r#"{"schema":1,"plugins":[]}"#);
    let err = run(fetch(&http, "http://katalog.ornek/index.json")).unwrap_err();
    assert_eq!(err.stage(), Stage::PluginCatalog);
    assert!(
        err.chain_text().contains("https://"),
        "{}",
        err.chain_text()
    );
    assert!(http.requests().is_empty(), "reddedilen adrese istek gitti");

    let local = run(fetch(&http, "http://127.0.0.1:8080/index.json")).unwrap();
    assert!(local.names().is_empty());
}

// --- indeks üretimi -----------------------------------------------------

#[test]
fn a_built_index_lists_every_plugin_with_version_pinned_urls_and_hashes() {
    let source = Source::new("uret");
    source
        .plugin("ytmusic", "0.4.0", &["music.youtube.com"], &script("yt"))
        .plugin("echo", "0.1.0", &["ornek.gecersiz"], &script("echo"));
    let built = source.build();

    let names: Vec<&str> = built.plugins.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["echo", "ytmusic"], "sıra belirlenimci olmalı");
    let echo = &built.plugins[0];
    assert_eq!(echo.version, "0.1.0");
    let paths: Vec<&str> = echo.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, vec!["plugin.json", "main.js"]);
    assert_eq!(
        echo.files[1].url,
        "https://katalog.ornek/refs/tags/echo-0.1.0/echo/main.js"
    );
    assert_eq!(echo.files[1].sha256, sha256_hex(script("echo").as_bytes()));

    assert!(built.json.ends_with('\n'));
    assert_eq!(
        source.build().json,
        built.json,
        "aynı dizin aynı metni üretmeli"
    );
    assert_eq!(
        read_url_template(&{
            let path = source.dir.join(INDEX_FILE);
            std::fs::write(&path, &built.json).unwrap();
            path
        })
        .unwrap()
        .as_deref(),
        Some(TEMPLATE)
    );
}

/// Bozuk eklentinin hepsi birden, tek tek sebebiyle söylenir.
#[test]
fn build_index_refuses_every_invalid_plugin_at_once() {
    let source = Source::new("bozuk");
    source.plugin("echo", "0.1.0", &[], &script("e"));
    let unversioned = source.dir.join("surumsuz");
    std::fs::create_dir_all(&unversioned).unwrap();
    std::fs::write(
        unversioned.join("plugin.json"),
        r#"{"name":"surumsuz","display_name":"S","api":2,"main":"main.js"}"#,
    )
    .unwrap();
    std::fs::write(unversioned.join("main.js"), "").unwrap();
    let upper = source.dir.join("Kotu");
    std::fs::create_dir_all(&upper).unwrap();
    std::fs::write(
        upper.join("plugin.json"),
        manifest_json("Kotu", "1.0.0", &[]),
    )
    .unwrap();
    std::fs::write(upper.join("main.js"), "").unwrap();

    let err = build_index(&source.dir, TEMPLATE).unwrap_err();
    let text = err.chain_text();
    assert!(text.contains("2 eklenti indekse giremedi"), "{text}");
    assert!(text.contains("surumsuz: `version` yok"), "{text}");
    assert!(text.contains("Kotu: katalog adı"), "{text}");
}

#[test]
fn build_index_skips_hidden_dirs_and_refuses_an_empty_catalog() {
    let source = Source::new("bos");
    let hidden = source.dir.join(".git");
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::write(hidden.join("plugin.json"), "{}").unwrap();
    let err = build_index(&source.dir, TEMPLATE).unwrap_err();
    assert!(
        err.chain_text().contains("dizinde eklenti yok"),
        "{}",
        err.chain_text()
    );
}

#[test]
fn a_template_must_pin_the_version_and_use_https() {
    let source = Source::new("sablon");
    source.plugin("echo", "0.1.0", &[], &script("e"));
    for (template, expected) in [
        ("https://k.ornek/main/{name}/{path}", "`{version}` yok"),
        ("http://k.ornek/{name}-{version}/{path}", "https://"),
    ] {
        let err = build_index(&source.dir, template).unwrap_err();
        assert!(
            err.chain_text().contains(expected),
            "{template}: {}",
            err.chain_text()
        );
    }
    assert!(build_index(&source.dir, "http://127.0.0.1:9/{name}/{version}/{path}").is_ok());
}

#[test]
fn index_differences_name_what_changed() {
    let source = Source::new("fark");
    source.plugin("echo", "0.1.0", &[], &script("e"));
    let before = source.build().json;
    source
        .plugin("echo", "0.2.0", &[], &script("e2"))
        .plugin("yeni", "1.0.0", &[], &script("y"));
    let after = source.build().json;
    let differences = index_differences(&before, &after);
    assert!(
        differences.contains(&"echo: girdisi değişti".to_owned()),
        "{differences:?}"
    );
    assert!(
        differences.contains(&"yeni: indekste yok".to_owned()),
        "{differences:?}"
    );
}

// --- indeks okuma --------------------------------------------------------

#[test]
fn a_catalog_reads_back_what_build_index_wrote() {
    let config = temp_config("oku");
    let source = Source::new("oku");
    source
        .plugin("echo", "0.1.0", &["ornek.gecersiz"], &script("e"))
        .plugin("ikinci", "2.0.0", &[], &script("i"));
    let catalog = catalog_of(&source);
    assert_eq!(catalog.names(), vec!["echo", "ikinci"]);

    let survey = catalog.survey(&config).unwrap();
    assert!(survey.delisted.is_empty());
    assert_eq!(
        survey.summary,
        CatalogSummary {
            listed: 2,
            installable: 2,
            installed: 0,
            updates: 0,
            problems: 0,
        }
    );
    let echo = &survey.plugins[0];
    assert_eq!(echo.installed, InstallState::NotInstalled);
    assert_eq!(echo.permissions.net, vec!["ornek.gecersiz".to_owned()]);
    assert_eq!(echo.display_name.as_deref(), Some("echo eklentisi"));
    assert!(echo.problem.is_none(), "{:?}", echo.problem);
}

#[test]
fn an_unknown_schema_is_refused_with_what_to_do() {
    let err = Catalog::parse(INDEX, br#"{"schema":2,"plugins":[]}"#).unwrap_err();
    assert!(
        err.chain_text().contains("güncelleyin"),
        "{}",
        err.chain_text()
    );
    let err = Catalog::parse(INDEX, br#"{"plugins":[]}"#).unwrap_err();
    assert!(
        err.chain_text().contains("`schema`"),
        "{}",
        err.chain_text()
    );
    let err = Catalog::parse(INDEX, b"<html>404</html>").unwrap_err();
    assert!(err.chain_text().contains("JSON"), "{}", err.chain_text());
}

/// Bozuk bir girdi ötekileri gizlemez; her biri kendi sebebini taşır.
#[test]
fn a_bad_entry_is_reported_without_hiding_the_others() {
    let source = Source::new("girdi");
    source.plugin("echo", "0.1.0", &[], &script("e"));
    let mut index: serde_json::Value = serde_json::from_str(&source.build().json).unwrap();
    let good = index["plugins"][0].clone();

    let mut newer = good.clone();
    newer["manifest"]["name"] = "yeni".into();
    newer["manifest"]["api"] = 3.into();

    let mut escaping = good.clone();
    escaping["manifest"]["name"] = "kacak".into();
    escaping["files"][1]["path"] = "../../main.js".into();

    let mut plain = good.clone();
    plain["manifest"]["name"] = "duz".into();
    plain["files"][1]["url"] = "http://kotu.ornek/main.js".into();

    let mut extra = good.clone();
    extra["manifest"]["name"] = "fazla".into();
    extra["files"].as_array_mut().unwrap().push(
        serde_json::json!({"path":"state/x.js","url":"https://k.ornek/x","sha256":"a".repeat(64)}),
    );

    let twin = good.clone();
    let mut other_twin = good.clone();
    other_twin["manifest"]["display_name"] = "ikiz".into();

    index["plugins"] = serde_json::json!([newer, escaping, plain, extra, twin, other_twin]);
    let catalog = Catalog::parse(INDEX, index.to_string().as_bytes()).unwrap();
    let config = temp_config("girdi");
    let survey = catalog.survey(&config).unwrap();
    assert_eq!(survey.summary.listed, 6);
    assert_eq!(survey.summary.problems, 6);

    let problem = |name: &str| {
        survey
            .plugins
            .iter()
            .find(|plugin| plugin.name == name)
            .and_then(|plugin| plugin.problem.clone())
            .unwrap_or_default()
    };
    assert!(problem("yeni").contains("api 3"), "{}", problem("yeni"));
    assert!(
        problem("kacak").contains("geçerli bir eklenti dosyası yolu değil"),
        "{}",
        problem("kacak")
    );
    assert!(problem("duz").contains("https://"), "{}", problem("duz"));
    assert!(
        problem("fazla").contains("motorun eklenti dizininde"),
        "{}",
        problem("fazla")
    );
    assert!(
        problem("echo").contains("birden çok kez"),
        "{}",
        problem("echo")
    );

    let err = run(install(&config, &FakeHttp::new(), &catalog, "echo")).unwrap_err();
    assert!(
        err.chain_text().contains("kurulamıyor"),
        "{}",
        err.chain_text()
    );
}

#[test]
fn a_name_not_in_the_catalog_lists_what_is_there_and_suggests_the_right_case() {
    let source = Source::new("ad");
    source.plugin("echo", "0.1.0", &[], &script("e"));
    let catalog = catalog_of(&source);
    let err = catalog.find("Echo").unwrap_err();
    let text = err.chain_text();
    assert!(text.contains("bunu mu demek istediniz: `echo`"), "{text}");
    assert!(text.contains("katalogdakiler: echo"), "{text}");
}

// --- kurulum ------------------------------------------------------------

#[test]
fn install_puts_the_plugin_in_place_with_its_origin_and_it_awaits_approval() {
    let config = temp_config("kur");
    let source = Source::new("kur");
    source.plugin("echo", "0.1.0", &["ornek.gecersiz"], &script("e"));

    let fetched = install_from(&config, &source, "echo");
    assert_eq!(fetched.version, "0.1.0");
    assert_eq!(fetched.index, INDEX);

    let dir = config.plugins_dir().join("echo");
    assert_eq!(
        std::fs::read_to_string(dir.join("main.js")).unwrap(),
        script("e")
    );
    let record = read_record(&dir).unwrap().unwrap();
    assert_eq!(record.version, "0.1.0");
    assert_eq!(record.index, INDEX);
    assert_eq!(record.files.len(), 2);

    // Katalogdan gelmek onay değildir (D-040).
    let (entries, _) = crate::plugin::discover(&config).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].consent, Some(ConsentStatus::NotAsked));
    assert!(entries[0].problem.is_none(), "{:?}", entries[0].problem);

    assert_eq!(
        state(&config, &source, "echo"),
        InstallState::Current {
            version: "0.1.0".to_owned()
        }
    );
    assert_no_staging_left(&config);
}

fn assert_no_staging_left(config: &Config) {
    let leftovers: Vec<String> = std::fs::read_dir(config.data_dir())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".kuruluyor"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "geçici kurulum dizini kaldı: {leftovers:?}"
    );
}

/// Bir dosyanın karması tutmazsa diske **hiçbir şey** yazılmaz.
#[test]
fn a_hash_mismatch_writes_nothing() {
    let config = temp_config("karma");
    let source = Source::new("karma");
    source.plugin("echo", "0.1.0", &[], &script("e"));
    let http = FakeHttp::new()
        .route("index.json", &source.build().json)
        .route("echo/plugin.json", &manifest_json("echo", "0.1.0", &[]))
        .route(
            "echo/main.js",
            "export function health() { /* değiştirilmiş */ }",
        );

    let err = run(async {
        let catalog = fetch(&http, INDEX).await.unwrap();
        install(&config, &http, &catalog, "echo").await
    })
    .unwrap_err();
    assert_eq!(err.stage(), Stage::PluginCatalog);
    assert!(
        err.chain_text().contains("karma tutmuyor"),
        "{}",
        err.chain_text()
    );
    assert!(
        err.chain_text().contains("hiçbir şey yazılmadı"),
        "{}",
        err.chain_text()
    );
    assert!(!config.plugins_dir().join("echo").exists());
}

/// Katalogda gösterilen izinler kurulanın izinleri olmalı.
#[test]
fn a_manifest_that_differs_from_the_index_is_refused() {
    let config = temp_config("manifest");
    let source = Source::new("manifest");
    source.plugin("echo", "0.1.0", &["ornek.gecersiz"], &script("e"));
    let mut index: serde_json::Value = serde_json::from_str(&source.build().json).unwrap();
    // İndeks daha az izin gösteriyor; inen dosya (karması doğru) daha fazlasını istiyor.
    index["plugins"][0]["manifest"]["permissions"]["net"] = serde_json::json!([]);
    let http = source.http_with_index(&index.to_string());

    let err = run(async {
        let catalog = fetch(&http, INDEX).await.unwrap();
        install(&config, &http, &catalog, "echo").await
    })
    .unwrap_err();
    assert!(
        err.chain_text().contains("indeksin gösterdiğinden farklı"),
        "{}",
        err.chain_text()
    );
    assert!(!config.plugins_dir().join("echo").exists());
}

/// Var olmayan bir sürümü gösteren indeks bakımcının kusurudur, ağın değil.
#[test]
fn a_missing_file_is_the_catalogs_fault_not_the_network() {
    let config = temp_config("yok");
    let source = Source::new("yok");
    source.plugin("echo", "0.1.0", &[], &script("e"));
    let http = FakeHttp::new()
        .route("index.json", &source.build().json)
        .route("echo/plugin.json", &manifest_json("echo", "0.1.0", &[]))
        .route_status("echo/main.js", 404, "Not Found");

    let err = run(async {
        let catalog = fetch(&http, INDEX).await.unwrap();
        install(&config, &http, &catalog, "echo").await
    })
    .unwrap_err();
    assert_eq!(err.stage(), Stage::PluginCatalog);
    assert!(
        err.chain_text().contains("dosya bulunamadı (HTTP 404"),
        "{}",
        err.chain_text()
    );
}

#[test]
fn installing_over_an_existing_directory_is_refused() {
    let config = temp_config("var");
    let source = Source::new("var");
    source.plugin("echo", "0.1.0", &[], &script("e"));
    let dir = config.plugins_dir().join("echo");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.js"), "// geliştiricinin kopyası").unwrap();

    let http = source.http();
    let err = run(async {
        let catalog = fetch(&http, INDEX).await.unwrap();
        install(&config, &http, &catalog, "echo").await
    })
    .unwrap_err();
    assert!(
        err.chain_text().contains("zaten kurulu"),
        "{}",
        err.chain_text()
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("main.js")).unwrap(),
        "// geliştiricinin kopyası"
    );
}

// --- güncelleme ---------------------------------------------------------

fn update_from(config: &Config, source: &Source, name: &str) -> Result<UpdateOutcome> {
    let http = source.http();
    run(async {
        let catalog = fetch(&http, INDEX).await?;
        update(config, &http, &catalog, name, PLATFORM).await
    })
}

#[test]
fn update_brings_a_catalog_plugin_to_the_new_version_and_keeps_its_state() {
    let config = temp_config("guncelle");
    let source = Source::new("guncelle");
    source.plugin("echo", "0.1.0", &["a.ornek"], &script("v1"));
    install_from(&config, &source, "echo");
    let state_file = config.plugin_state_dir("echo").join("storage.json");
    std::fs::create_dir_all(state_file.parent().unwrap()).unwrap();
    std::fs::write(&state_file, r#"{"client_id":"sakla"}"#).unwrap();

    source.plugin("echo", "0.2.0", &["a.ornek", "b.ornek"], &script("v2"));
    assert_eq!(
        state(&config, &source, "echo"),
        InstallState::UpdateAvailable {
            installed: "0.1.0".to_owned(),
            available: "0.2.0".to_owned()
        }
    );

    match update_from(&config, &source, "echo").unwrap() {
        UpdateOutcome::Updated {
            from,
            to,
            permissions_added,
            tools_changed,
        } => {
            assert_eq!((from.as_str(), to.as_str()), ("0.1.0", "0.2.0"));
            assert_eq!(permissions_added.net, vec!["b.ornek".to_owned()]);
            assert!(tools_changed.is_empty(), "{tools_changed:?}");
        }
        other => panic!("güncellenmedi: {other:?}"),
    }
    let dir = config.plugins_dir().join("echo");
    assert_eq!(
        std::fs::read_to_string(dir.join("main.js")).unwrap(),
        script("v2")
    );
    assert_eq!(
        std::fs::read_to_string(&state_file).unwrap(),
        r#"{"client_id":"sakla"}"#,
        "eklentinin deposu güncellemede silinmemeli"
    );
    assert_eq!(read_record(&dir).unwrap().unwrap().version, "0.2.0");
    assert_eq!(
        update_from(&config, &source, "echo").unwrap(),
        UpdateOutcome::Current {
            version: "0.2.0".to_owned()
        }
    );
    let leftovers: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".indiriliyor"))
        .collect();
    assert!(leftovers.is_empty(), "geçici dosya kaldı: {leftovers:?}");
}

/// Elle konmuş ya da elle değiştirilmiş eklentinin üstüne yazılmaz.
#[test]
fn update_leaves_a_manual_or_modified_plugin_alone() {
    let config = temp_config("dokunma");
    let source = Source::new("dokunma");
    source.plugin("elle", "0.1.0", &[], &script("elle")).plugin(
        "echo",
        "0.1.0",
        &[],
        &script("v1"),
    );

    let manual = config.plugins_dir().join("elle");
    std::fs::create_dir_all(&manual).unwrap();
    std::fs::write(manual.join("main.js"), "// çalışma kopyası").unwrap();
    assert_eq!(state(&config, &source, "elle"), InstallState::Manual);

    install_from(&config, &source, "echo");
    let main = config.plugins_dir().join("echo").join("main.js");
    std::fs::write(&main, "// elle düzeltildi").unwrap();
    source
        .plugin("elle", "0.2.0", &[], &script("elle2"))
        .plugin("echo", "0.2.0", &[], &script("v2"));
    assert_eq!(
        state(&config, &source, "echo"),
        InstallState::Modified {
            version: "0.1.0".to_owned(),
            files: vec!["main.js".to_owned()]
        }
    );

    for name in ["elle", "echo"] {
        match update_from(&config, &source, name).unwrap() {
            UpdateOutcome::Skipped { reason } => {
                assert!(reason.contains("üstüne yaz"), "{name}: {reason}");
            }
            other => panic!("{name} güncellenmemeliydi: {other:?}"),
        }
    }
    assert_eq!(
        std::fs::read_to_string(&main).unwrap(),
        "// elle düzeltildi"
    );
    assert_eq!(
        std::fs::read_to_string(manual.join("main.js")).unwrap(),
        "// çalışma kopyası"
    );
}

/// Yarıda kalmış bir güncellemenin bıraktığı yeni dosya yerel değişiklik
/// sayılmaz — yoksa kullanıcı onu yalnızca kaldırıp yeniden kurarak
/// düzeltebilirdi.
#[test]
fn a_half_finished_update_is_not_mistaken_for_a_local_edit() {
    let config = temp_config("yarim");
    let source = Source::new("yarim");
    source.plugin("echo", "0.1.0", &[], &script("v1"));
    install_from(&config, &source, "echo");
    source.plugin("echo", "0.2.0", &[], &script("v2"));
    std::fs::write(
        config.plugins_dir().join("echo").join("main.js"),
        script("v2"),
    )
    .unwrap();

    assert!(matches!(
        state(&config, &source, "echo"),
        InstallState::UpdateAvailable { .. }
    ));
    assert!(matches!(
        update_from(&config, &source, "echo").unwrap(),
        UpdateOutcome::Updated { .. }
    ));
}

/// Katalogdan çekilen bir eklenti sessizce "güncel" görünmemeli.
#[test]
fn a_plugin_delisted_from_its_catalog_is_reported() {
    let config = temp_config("cekildi");
    let source = Source::new("cekildi");
    source
        .plugin("echo", "0.1.0", &[], &script("e"))
        .plugin("kalan", "1.0.0", &[], &script("k"));
    install_from(&config, &source, "echo");
    std::fs::remove_dir_all(source.dir.join("echo")).unwrap();

    let catalog = catalog_of(&source);
    let survey = catalog.survey(&config).unwrap();
    assert_eq!(survey.delisted, vec!["echo".to_owned()]);
    assert_eq!(
        catalog.update_candidates(&config).unwrap(),
        vec!["echo".to_owned()]
    );
    match update_from(&config, &source, "echo").unwrap() {
        UpdateOutcome::Skipped { reason } => assert!(reason.contains("katalogda yok"), "{reason}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn updating_a_plugin_that_is_not_installed_says_how_to_install_it() {
    let config = temp_config("kurulu-degil");
    let source = Source::new("kurulu-degil");
    source.plugin("echo", "0.1.0", &[], &script("e"));
    let err = update_from(&config, &source, "echo").unwrap_err();
    assert!(
        err.chain_text().contains("headshell plugin install echo"),
        "{}",
        err.chain_text()
    );
}

/// Araç değişikliği onay istemez (kullanıcının kararı, D-071) ama söylenir.
#[test]
fn tool_changes_are_named_for_this_platform() {
    use crate::plugin::manifest::Asset;
    let tool = |version: &str, sha: char| Requirement {
        name: "yt-dlp".to_owned(),
        version: version.to_owned(),
        assets: [(
            PLATFORM.to_owned(),
            Asset {
                url: format!("https://ornek/{version}"),
                sha256: sha.to_string().repeat(64),
            },
        )]
        .into_iter()
        .collect(),
    };

    let bumped = tool_changes(&[tool("1", 'a')], &[tool("2", 'b')], PLATFORM);
    assert_eq!(bumped.len(), 1);
    assert_eq!(bumped[0].describe(), "yt-dlp 1 → 2");

    let rebuilt = tool_changes(&[tool("1", 'a')], &[tool("1", 'b')], PLATFORM);
    assert!(rebuilt[0].binary_changed);
    assert!(
        rebuilt[0].describe().contains("ikilisi değişti"),
        "{}",
        rebuilt[0].describe()
    );

    assert_eq!(
        tool_changes(&[], &[tool("1", 'a')], PLATFORM)[0].describe(),
        "yt-dlp 1 eklendi"
    );
    assert_eq!(
        tool_changes(&[tool("1", 'a')], &[], PLATFORM)[0].describe(),
        "yt-dlp 1 artık istenmiyor"
    );
    assert!(tool_changes(&[tool("1", 'a')], &[tool("1", 'a')], PLATFORM).is_empty());
    // Başka bir platformun ikilisi değişti: bu makine için değişiklik yok.
    assert!(tool_changes(&[tool("1", 'a')], &[tool("1", 'b')], "windows-x86_64").is_empty());
}

// --- kaldırma -----------------------------------------------------------

#[test]
fn remove_deletes_the_directory_with_its_state() {
    let config = temp_config("kaldir");
    let source = Source::new("kaldir");
    source.plugin("echo", "0.1.0", &[], &script("e"));
    install_from(&config, &source, "echo");
    std::fs::create_dir_all(config.plugin_state_dir("echo")).unwrap();

    let removed = remove(&config, "echo").unwrap();
    assert_eq!(removed.path, config.plugins_dir().join("echo"));
    assert!(removed.link_target.is_none());
    assert!(!removed.path.exists());
}

/// Kaldırma adı yola ekliyor: veri dizininin dışına uzanan ad reddedilir.
#[test]
fn remove_refuses_a_name_that_leaves_the_plugins_dir() {
    let config = temp_config("kacis");
    let outside = config.data_dir().join("kurban");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::create_dir_all(config.plugins_dir()).unwrap();

    let err = remove(&config, "../kurban").unwrap_err();
    assert!(
        matches!(err.kind(), ErrorKind::InvalidInput { .. }),
        "{err:?}"
    );
    assert!(outside.exists(), "eklenti dizininin dışı silindi");

    let err = remove(&config, "yok").unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::NotFound { .. }), "{err:?}");
}

/// Geliştiricinin çalışma kopyasına bağlantı: bağlantı gider, kopya kalır.
#[cfg(unix)]
#[test]
fn remove_takes_away_only_the_link_of_a_symlinked_plugin() {
    let config = temp_config("baglanti");
    let work = TempDir::new("catalog-calisma-kopyasi");
    std::fs::write(work.join("main.js"), "// kaynak").unwrap();
    std::fs::create_dir_all(config.plugins_dir()).unwrap();
    let link = config.plugins_dir().join("gelistirme");
    std::os::unix::fs::symlink(work.path(), &link).unwrap();

    let removed = remove(&config, "gelistirme").unwrap();
    assert_eq!(removed.link_target.as_deref(), Some(work.path()));
    assert!(std::fs::symlink_metadata(&link).is_err(), "bağlantı kalmış");
    assert_eq!(
        std::fs::read_to_string(work.join("main.js")).unwrap(),
        "// kaynak"
    );
}

#[test]
fn install_state_says_manual_for_a_plugin_placed_by_hand() {
    let config = temp_config("elle");
    let source = Source::new("elle");
    source.plugin("echo", "0.1.0", &[], &script("e"));
    let dir = config.plugins_dir().join("echo");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(ORIGIN_FILE), "{ bozuk").unwrap();
    match state(&config, &source, "echo") {
        InstallState::Unreadable { detail } => assert!(detail.contains("origin.json"), "{detail}"),
        other => panic!("{other:?}"),
    }
    std::fs::remove_file(dir.join(ORIGIN_FILE)).unwrap();
    assert_eq!(state(&config, &source, "echo"), InstallState::Manual);
}

#[test]
fn the_http_client_is_only_used_through_the_trait() {
    // `Arc<dyn HttpClient>` — Session'ın çağıranlardan aldığı biçim (K7).
    let source = Source::new("dyn");
    source.plugin("echo", "0.1.0", &[], &script("e"));
    let http: Arc<dyn HttpClient> = Arc::new(source.http());
    let catalog = run(fetch(http.as_ref(), INDEX)).unwrap();
    assert_eq!(catalog.names(), vec!["echo"]);
}
