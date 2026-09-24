//! Eklenti sözleşmesi, **gerçek motorla ve depodaki fikstürle** (D-069).
//!
//! Birim testleri (`src/plugin/tests.rs`) motorun her kapısını test içinde
//! yazılmış küçük betiklerle sınıyor. Burada sınanan şey başka: dışarıdan
//! görülen yüzey — keşif, onay, sağlayıcı kaydı — ve `fixtures/plugins/echo`
//! dosyasının, bir eklenti yazarının yazacağı biçimde, uçtan uca çalışması.
//!
//! Faz 2'nin "bitti sayılır" ölçütünün ikisi de bu dosyada: **Rust olmayan
//! bir eklenti çalışıyor** ve **çekirdek sürüm uyumsuzluğunda çökmeden
//! reddediyor**. api 1'den farkı: hiçbir test "python3 yok" diye atlamıyor,
//! çünkü eklenti çalıştırmak için dışarıda hiçbir şey gerekmiyor.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::path::{Path, PathBuf};
use std::time::Duration;

use headshell_core::config::Config;
use headshell_core::ids::{ProviderId, ProviderTrackId};
use headshell_core::plugin::consent::ConsentStore;
use headshell_core::plugin::manifest::{Permissions, PluginManifest};
use headshell_core::plugin::{PluginProvider, discover, load};
use headshell_core::provider::{AudioSource, Capabilities, Provider};
use headshell_core::secrets::Secrets;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/plugins/echo")
}

fn temp_config(name: &str) -> support::TestConfig {
    support::TestConfig::new(&format!("plugin-script-{name}"))
}

/// Fikstür eklentisini veri dizinine kurar — kullanıcının yapacağı gibi:
/// dizini kopyalayarak.
fn install_echo(config: &Config) -> PathBuf {
    let dir = config.plugins_dir().join("echo");
    std::fs::create_dir_all(&dir).unwrap();
    for file in ["plugin.json", "main.js"] {
        std::fs::copy(fixture_dir().join(file), dir.join(file)).unwrap();
    }
    dir
}

fn approve(config: &Config, name: &str) {
    let dir = config.plugins_dir().join(name);
    let manifest = PluginManifest::load(&dir).unwrap();
    let path = config.plugin_consent_path();
    let mut store = ConsentStore::load(&path).unwrap();
    store.approve(name, &manifest.permissions, jiff::Timestamp::now());
    store.save(&path).unwrap();
}

fn provider_for(config: &Config, dir: &Path) -> PluginProvider {
    let manifest = PluginManifest::load(dir).unwrap();
    let secrets = Secrets::load(&config.secrets_path()).unwrap();
    PluginProvider::from_manifest(config, &manifest, dir, &secrets).unwrap()
}

#[tokio::test]
async fn a_script_plugin_answers_health_search_and_resolve() {
    let config = temp_config("mutlu");
    let dir = install_echo(&config);

    let mut secrets = Secrets::default();
    secrets.set("plugin:echo", "token", "gizli-anahtar");
    secrets.save(&config.secrets_path()).unwrap();

    let provider = provider_for(&config, &dir);
    assert!(
        provider
            .info()
            .capabilities
            .contains(Capabilities::SEARCH | Capabilities::STREAM)
    );

    let health = provider.health().await.unwrap();
    assert!(health.reachable, "{:?}", health.detail);
    assert_eq!(health.track_count, Some(2));
    // Eklenti sırrı gördü ama **değerini** raporlamadı.
    assert_eq!(health.detail.as_deref(), Some("sır var"));

    let tracks = provider.search("EZHEL", 10).await.unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].id.provider, ProviderId::new("echo"));
    assert_eq!(tracks[0].track.title, "Geceler");
    assert!(tracks[0].track.isrc.is_some(), "geçerli ISRC korunmalı");

    let tracks = provider.search("sezen", 10).await.unwrap();
    assert!(
        tracks[0].track.isrc.is_none(),
        "biçimsiz ISRC kabul edilmemeli"
    );

    let id = ProviderTrackId::new(ProviderId::new("echo"), "track-1");
    match provider.resolve_source(&id).await.unwrap() {
        Some(AudioSource::HttpStream { url, headers }) => {
            assert!(url.ends_with("track-1.mp3"), "{url}");
            assert_eq!(headers[0].value, "gizli-anahtar");
        }
        other => panic!("beklenmeyen kaynak: {other:?}"),
    }

    let missing = ProviderTrackId::new(ProviderId::new("echo"), "yok");
    assert!(
        provider.resolve_source(&missing).await.unwrap().is_none(),
        "`null` bir cevaptır, hata değil"
    );
}

/// api 1'in Python eklentisi **çökmeden** reddedilir ve kullanıcıya ne
/// yapacağı söylenir — Faz 2 ölçütünün ikinci yarısı.
#[tokio::test]
async fn an_api1_plugin_is_refused_with_a_reason_and_the_core_keeps_going() {
    let config = temp_config("eski");
    install_echo(&config);
    let old = config.plugins_dir().join("eski");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(
        old.join("plugin.json"),
        r#"{"name":"eski","display_name":"Eski","api":1,"exec":["python3","./main.py"]}"#,
    )
    .unwrap();
    approve(&config, "echo");

    let (entries, summary) = discover(&config).unwrap();
    assert_eq!(summary.incompatible, 1);
    let eski = entries.iter().find(|entry| entry.name == "eski").unwrap();
    assert!(
        eski.status_text().contains("api 1"),
        "{}",
        eski.status_text()
    );

    let (providers, _) = load(&config).unwrap();
    assert_eq!(
        providers.len(),
        1,
        "eski eklenti yüklenmemeli, echo yüklenmeli"
    );
    assert!(providers[0].health().await.unwrap().reachable);
}

#[tokio::test]
async fn an_unapproved_plugin_is_not_loaded_but_is_visible() {
    let config = temp_config("onay");
    install_echo(&config);

    let (providers, summary) = load(&config).unwrap();
    assert!(providers.is_empty(), "onaysız eklenti yüklenmemeli");
    assert_eq!(summary.discovered, 1);
    assert_eq!(summary.awaiting_approval, 1);

    approve(&config, "echo");
    let (providers, summary) = load(&config).unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(summary.ready, 1);

    let registry = headshell_core::provider::registry_with_http(&config, None).unwrap();
    assert!(registry.get(&ProviderId::new("echo")).is_some());
}

#[tokio::test]
async fn a_plugin_asking_for_more_permissions_stops_loading_until_reapproved() {
    let config = temp_config("izin");
    let dir = install_echo(&config);
    approve(&config, "echo");
    assert_eq!(load(&config).unwrap().0.len(), 1);

    // Eklenti güncellendi ve yeni bir ana bilgisayar istiyor.
    let mut manifest = PluginManifest::load(&dir).unwrap();
    manifest.permissions = Permissions {
        net: vec!["ornek.gecersiz".to_owned(), "*.yeni.gecersiz".to_owned()],
    };
    std::fs::write(
        dir.join("plugin.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let (providers, summary) = load(&config).unwrap();
    assert!(providers.is_empty());
    assert_eq!(summary.awaiting_approval, 1);

    approve(&config, "echo");
    assert_eq!(load(&config).unwrap().0.len(), 1);
}

#[tokio::test]
async fn two_plugins_do_not_see_each_others_secrets() {
    let config = temp_config("sir");
    let dir = install_echo(&config);

    let mut secrets = Secrets::default();
    secrets.set("plugin:baska", "token", "komsunun-anahtari");
    secrets.save(&config.secrets_path()).unwrap();

    let provider = provider_for(&config, &dir);
    assert_eq!(
        provider.health().await.unwrap().detail.as_deref(),
        Some("sır yok")
    );
    let id = ProviderTrackId::new(ProviderId::new("echo"), "track-1");
    match provider.resolve_source(&id).await.unwrap() {
        Some(AudioSource::HttpStream { headers, .. }) => assert_eq!(headers[0].value, ""),
        other => panic!("beklenmeyen kaynak: {other:?}"),
    }
}

/// Takılan bir eklenti çekirdeği takmaz; süresi dolunca hata döner.
#[tokio::test]
async fn a_hanging_plugin_times_out_instead_of_freezing_the_core() {
    let config = temp_config("asili");
    let dir = install_echo(&config);
    std::fs::write(
        dir.join("main.js"),
        "export function health() { for (;;) {} }\n\
         export function search() { return []; }\n\
         export function resolve_source() { return null; }\n",
    )
    .unwrap();

    let provider = provider_for(&config, &dir)
        .with_timeouts(Duration::from_secs(2), Duration::from_millis(500));
    let started = std::time::Instant::now();
    let health = provider.health().await.unwrap();
    assert!(!health.reachable);
    let detail = health.detail.unwrap_or_default();
    assert!(detail.contains("cevap vermedi"), "{detail}");
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "zaman aşımı işe yaramadı: {:?}",
        started.elapsed()
    );
}
