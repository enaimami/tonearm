//! Eklenti protokolü, **gerçek bir alt süreçle** (Faz 2 §2.1).
//!
//! Birim testleri betiklenmiş bir taşımayla protokolün mantığını sınıyor;
//! burada sınanan şey başka: `fixtures/plugins/echo` Python'da yazılmış
//! gerçek bir eklenti ve gerçekten çalıştırılıyor. Faz 2'nin "bitti sayılır"
//! ölçütünün ikisi de bu dosyada: **Rust olmayan bir eklenti çalışıyor** ve
//! **çekirdek sürüm uyumsuzluğunda çökmeden reddediyor**.
//!
//! `python3` yoksa testler kendini atlar — sessizce değil, sebebini yazarak.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tonearm_core::config::Config;
use tonearm_core::ids::{ProviderId, ProviderTrackId};
use tonearm_core::plugin::consent::ConsentStore;
use tonearm_core::plugin::manifest::{Permissions, PluginManifest};
use tonearm_core::plugin::{PluginProvider, load};
use tonearm_core::provider::{AudioSource, Capabilities, Provider};
use tonearm_core::secrets::Secrets;

/// `python3` var mı. Yoksa test atlanır (CI'da yorumlayıcı olmayabilir).
fn python_available() -> bool {
    std::process::Command::new("python3")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/plugins/echo")
}

fn temp_config(name: &str) -> Config {
    let dir = std::env::temp_dir().join(format!(
        "tonearm-plugin-process-{}-{}-{name}",
        std::process::id(),
        jiff::Timestamp::now().as_nanosecond()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    Config::with_data_dir(dir)
}

/// Fixture eklentisini veri dizinine kurar; `args` `exec`'e eklenir.
fn install_echo(config: &Config, args: &[&str]) -> PathBuf {
    let dir = config.plugins_dir().join("echo");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(fixture_dir().join("main.py"), dir.join("main.py")).unwrap();

    let mut exec = vec!["python3".to_owned(), "./main.py".to_owned()];
    exec.extend(args.iter().map(|arg| (*arg).to_owned()));
    let manifest = serde_json::json!({
        "name": "echo",
        "display_name": "Echo (sınama)",
        "version": "0.1.0",
        "api": 1,
        "exec": exec,
        "capabilities": ["search", "stream"],
        "permissions": {"net": ["ornek.gecersiz"]}
    });
    std::fs::write(
        dir.join("plugin.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
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
async fn a_python_plugin_answers_search_health_and_resolve() {
    if !python_available() {
        eprintln!("python3 yok — eklenti testi atlanıyor (bu bir başarısızlık değil)");
        return;
    }
    let config = temp_config("mutlu");
    // `--noise`: eklenti stdout'una JSON olmayan bir satır yazıyor.
    // El sıkışma yine de yürümeli — bir `print()` sağlayıcıyı düşürmemeli.
    let dir = install_echo(&config, &["--noise"]);

    let mut secrets = Secrets::default();
    secrets.set("plugin:echo", "token", "gizli-anahtar");
    secrets.save(&config.secrets_path()).unwrap();

    let provider = provider_for(&config, &dir);

    // Yetenekler el sıkışmadan önce manifestten biliniyor.
    assert!(
        provider
            .info()
            .capabilities
            .contains(Capabilities::SEARCH | Capabilities::STREAM)
    );

    let health = provider.health().await.unwrap();
    assert!(health.reachable);
    assert_eq!(health.track_count, Some(2));
    // Eklenti sırrını gördü ama **değerini** raporlamadı.
    assert_eq!(health.detail.as_deref(), Some("sır var"));

    let tracks = provider.search("ezhel", 10).await.unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].id.provider, ProviderId::new("echo"));
    assert_eq!(tracks[0].track.title, "Geceler");
    assert!(tracks[0].track.isrc.is_some(), "geçerli ISRC korunmalı");

    // Biçimsiz ISRC gönderen parça: alan düşürülmeli, parça değil.
    let tracks = provider.search("sezen", 10).await.unwrap();
    assert_eq!(tracks.len(), 1);
    assert!(
        tracks[0].track.isrc.is_none(),
        "biçimsiz ISRC kabul edilmemeli"
    );

    let id = ProviderTrackId::new(ProviderId::new("echo"), "track-1");
    let source = provider.resolve_source(&id).await.unwrap();
    match source {
        Some(AudioSource::HttpStream { url, headers }) => {
            assert!(url.ends_with("track-1.mp3"), "{url}");
            // Sır eklentiye ulaşmış: kendi ad alanından okuyup kullanabildi.
            assert_eq!(headers[0].value, "gizli-anahtar");
        }
        other => panic!("beklenmeyen kaynak: {other:?}"),
    }

    // Bilinmeyen parça "yok" cevabı, hata değil.
    let missing = ProviderTrackId::new(ProviderId::new("echo"), "yok-boyle-bir-sey");
    assert!(provider.resolve_source(&missing).await.unwrap().is_none());
}

#[tokio::test]
async fn a_plugin_that_speaks_another_protocol_version_is_refused_not_fatal() {
    if !python_available() {
        eprintln!("python3 yok — sürüm testi atlanıyor (bu bir başarısızlık değil)");
        return;
    }
    let config = temp_config("surum");
    // Manifest api 1 diyor, el sıkışma 99 diyor: yalan manifest en kötü hâl,
    // çünkü keşif onu yakalayamaz — el sıkışma yakalamalı.
    let dir = install_echo(&config, &["--api", "99"]);
    let provider = provider_for(&config, &dir);

    let err = provider.search("ezhel", 10).await.unwrap_err();
    assert!(
        matches!(
            err.kind(),
            tonearm_core::ErrorKind::PluginIncompatible {
                plugin_api: 99,
                host_api: 1,
                ..
            }
        ),
        "{}",
        err.chain_text()
    );
    assert!(err.chain_text().starts_with("ADIM: PLUGIN_HANDSHAKE"));

    // Çekirdek ayakta: sağlık sorusu hâlâ cevaplanıyor, çökme yok.
    let health = provider.health().await.unwrap();
    assert!(!health.reachable);
}

#[tokio::test]
async fn a_plugin_that_dies_mid_call_is_isolated_and_restarted() {
    if !python_available() {
        eprintln!("python3 yok — çökme testi atlanıyor (bu bir başarısızlık değil)");
        return;
    }
    let config = temp_config("cokme");
    let dir = install_echo(&config, &["--crash-on", "search"]);
    let provider = provider_for(&config, &dir);

    // Sağlık çalışıyor: süreç ayakta.
    assert!(provider.health().await.unwrap().reachable);

    // Arama süreci öldürüyor.
    let err = provider.search("ezhel", 10).await.unwrap_err();
    assert!(
        matches!(err.kind(), tonearm_core::ErrorKind::PluginCrashed { .. }),
        "{}",
        err.chain_text()
    );

    // Bir sonraki çağrı süreci yeniden başlatıyor — çekirdek düşmedi.
    let health = provider.health().await.unwrap();
    assert!(health.reachable, "çöken eklenti yeniden başlatılmalı");
}

#[tokio::test]
async fn an_unapproved_plugin_is_not_loaded_but_is_visible() {
    if !python_available() {
        eprintln!("python3 yok — onay testi atlanıyor (bu bir başarısızlık değil)");
        return;
    }
    let config = temp_config("onay");
    install_echo(&config, &[]);

    let (providers, summary) = load(&config).unwrap();
    assert!(providers.is_empty(), "onaysız eklenti yüklenmemeli");
    assert_eq!(summary.discovered, 1);
    assert_eq!(summary.awaiting_approval, 1);

    approve(&config, "echo");
    let (providers, summary) = load(&config).unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(summary.ready, 1);

    // Kayıt defterine de giriyor: `provider list` onu görmeli.
    let registry = tonearm_core::provider::registry_with_http(&config, None).unwrap();
    assert!(registry.get(&ProviderId::new("echo")).is_some());
}

#[tokio::test]
async fn a_hanging_plugin_times_out_instead_of_freezing_the_core() {
    if !python_available() {
        eprintln!("python3 yok — zaman aşımı testi atlanıyor (bu bir başarısızlık değil)");
        return;
    }
    let config = temp_config("asili");
    // El sıkışmada asılıyor: bu yolun zaman aşımı en kısası (5 sn), test
    // sonsuza kadar beklemesin.
    let dir = install_echo(&config, &["--hang-on", "handshake"]);
    let provider = provider_for(&config, &dir);

    let started = std::time::Instant::now();
    let err = provider.search("ezhel", 10).await.unwrap_err();
    let elapsed = started.elapsed();

    match err.kind() {
        tonearm_core::ErrorKind::PluginTimeout { method, .. } => assert_eq!(method, "handshake"),
        other => panic!("beklenmeyen hata: {other:?}"),
    }
    assert!(
        elapsed < std::time::Duration::from_secs(15),
        "zaman aşımı işe yaramadı: {elapsed:?}"
    );
}

#[tokio::test]
async fn a_plugin_asking_for_more_permissions_stops_loading_until_reapproved() {
    if !python_available() {
        eprintln!("python3 yok — izin testi atlanıyor (bu bir başarısızlık değil)");
        return;
    }
    let config = temp_config("izin");
    let dir = install_echo(&config, &[]);
    approve(&config, "echo");
    assert_eq!(load(&config).unwrap().0.len(), 1);

    // Eklenti güncellendi ve yeni bir ana bilgisayar istiyor.
    let mut manifest = PluginManifest::load(&dir).unwrap();
    manifest.permissions = Permissions {
        net: vec!["ornek.gecersiz".to_owned(), "yeni.gecersiz".to_owned()],
        fs: vec!["/ev/muzik".to_owned()],
    };
    std::fs::write(
        dir.join("plugin.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let (providers, summary) = load(&config).unwrap();
    assert!(
        providers.is_empty(),
        "izin büyüdüyse eklenti yeniden onaya kadar yüklenmemeli"
    );
    assert_eq!(summary.awaiting_approval, 1);

    approve(&config, "echo");
    assert_eq!(load(&config).unwrap().0.len(), 1);
}

#[tokio::test]
async fn two_plugins_do_not_see_each_others_secrets() {
    if !python_available() {
        eprintln!("python3 yok — sır testi atlanıyor (bu bir başarısızlık değil)");
        return;
    }
    let config = temp_config("sir");
    let dir = install_echo(&config, &[]);

    let mut secrets = Secrets::default();
    secrets.set("plugin:baska", "token", "komsunun-anahtari");
    secrets.save(&config.secrets_path()).unwrap();

    let provider = Arc::new(provider_for(&config, &dir));
    let id = ProviderTrackId::new(ProviderId::new("echo"), "track-1");
    let source = provider.resolve_source(&id).await.unwrap();
    match source {
        Some(AudioSource::HttpStream { headers, .. }) => {
            assert_eq!(
                headers[0].value, "",
                "eklenti başka bir eklentinin sırrını görmemeli"
            );
        }
        other => panic!("beklenmeyen kaynak: {other:?}"),
    }
}
