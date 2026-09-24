//! SoundCloud referans eklentisi, **gerçek SoundCloud'a karşı** (Faz 2 §2.2).
//!
//! `plugin_script.rs` sözleşmeyi sabit kataloglu bir fixture'la sınıyor; burada
//! sınanan şey başka: `plugins/soundcloud` gerçek bir servise bağlanıyor ve
//! zincirin tamamı — client_id keşfi, arama, akış adresi çözümü — canlı olarak
//! yürüyor. D-069'dan beri eklenti gömülü QuickJS'te koşuyor: ağa yalnızca
//! manifestin izin verdiği ana bilgisayarlardan çıkabiliyor, ve bu testler o
//! iznin gerçek servis için **yeterli** olduğunun da kanıtı.
//!
//! **Bu testler varsayılan koşuma dahildir** (D-043). Bilinçli bir seçim ve
//! bir bedeli var: SoundCloud düştüğünde ya da web yüzeyini değiştirdiğinde
//! paket kırmızı yanar, oysa kodumuzda hiçbir şey bozulmamıştır. Karşılığında
//! eklentinin bozulduğu gün *o gün* öğreniliyor, aylar sonra değil.
//!
//! Kırmızı yandığında ilk soru şu olmalı: **son commit'e mi baksam, yoksa
//! `curl https://soundcloud.com/` mu çeksem?** İkincisi çalışıyorsa ve testler
//! hâlâ kırmızıysa kusur bizdedir.
//!
//! İki başarısızlık ayrı tutuluyor (K9):
//! - **Ulaşamamak** bir test başarısızlığı değil: ağ yoksa test kendini atlar
//!   ve sebebini `stderr`'e yazar (ses aygıtı testlerinin yordamı).
//! - **Ulaşıp beklenmeyeni almak** düşer. "Ulaşamadım" ile "hayır dedi"
//!   farklı tanılardır ve farklı çözümleri vardır.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::path::{Path, PathBuf};

use headshell_core::config::Config;
use headshell_core::ids::{ProviderId, ProviderTrackId};
use headshell_core::plugin::PluginProvider;
use headshell_core::plugin::manifest::PluginManifest;
use headshell_core::provider::{AudioSource, Capabilities, Provider};
use headshell_core::secrets::Secrets;

/// Eklentinin repodaki kaynağı (fixture değil — kullanıcıya dağıtılan dosya).
fn plugin_source_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/soundcloud")
}

/// SoundCloud'a TCP ile ulaşılabiliyor mu.
///
/// Yalnızca DNS + bağlantı sınanıyor; HTTP'ye hiç girilmiyor. Amaç "ağ var mı"
/// sorusunu cevaplamak, servisin sağlığını ölçmek değil — sağlığı ölçmek
/// testlerin kendi işi.
fn soundcloud_reachable() -> bool {
    use std::net::ToSocketAddrs;
    let Ok(mut addrs) = ("soundcloud.com", 443).to_socket_addrs() else {
        return false;
    };
    addrs.any(|addr| {
        std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(5)).is_ok()
    })
}

/// Testin koşulup koşulamayacağını söyler; koşulamıyorsa sebebini yazar.
fn prerequisites_met(test: &str) -> bool {
    if !soundcloud_reachable() {
        eprintln!("{test}: soundcloud.com:443'e ulaşılamadı — atlanıyor (ağ yok sayılıyor)");
        return false;
    }
    true
}

fn temp_config(name: &str) -> support::TestConfig {
    support::TestConfig::new(&format!("soundcloud-{name}"))
}

/// Eklentiyi repodan veri dizinine kurar — kullanıcının yaptığı şeyin aynısı.
fn install(config: &Config) -> PluginProvider {
    let dir = config.plugins_dir().join("soundcloud");
    std::fs::create_dir_all(&dir).unwrap();
    for file in ["main.js", "plugin.json"] {
        std::fs::copy(plugin_source_dir().join(file), dir.join(file)).unwrap();
    }

    let manifest = PluginManifest::load(&dir).unwrap();
    let secrets = Secrets::load(&config.secrets_path()).unwrap();
    PluginProvider::from_manifest(config, &manifest, &dir, &secrets).unwrap()
}

/// Sırsız kurulumda eklenti client_id'yi kendisi keşfedip arama yapabilmeli.
///
/// D-043'ün "yoksa keşfet" kolu budur ve kurulum sürtünmesini sıfırlayan
/// şey de bu: kullanıcı hiçbir anahtar vermeden `headshell play` diyebiliyor.
#[tokio::test]
async fn a_plugin_with_no_secret_discovers_a_client_id_and_searches() {
    if !prerequisites_met("keşif+arama") {
        return;
    }
    let config = temp_config("kesif");
    let provider = install(&config);

    assert!(
        provider
            .info()
            .capabilities
            .contains(Capabilities::SEARCH | Capabilities::STREAM)
    );

    let hits = provider.search("nujabes", 5).await.unwrap();
    assert!(!hits.is_empty(), "canlı arama boş döndü");

    for hit in &hits {
        assert!(
            !hit.track.title.trim().is_empty(),
            "başlıksız parça: {hit:?}"
        );
        assert!(
            !hit.track.artist.trim().is_empty(),
            "sanatçısız parça: {hit:?}"
        );
        // Sağlayıcı adını çekirdek ekliyor; eklenti çıplak id gönderiyor.
        assert_eq!(
            hit.id.provider.as_str(),
            "soundcloud",
            "kimlik yanlış ad alanında: {}",
            hit.id
        );
    }
}

/// Sağlık cevabı, client_id'nin hangi kaynaktan geldiğini söylemeli.
///
/// D-043 üç kaynağa izin veriyor (sır → önbellek → keşif); hangisinin
/// kullanıldığı görünmezse yanlış anahtarla çalışan bir kurulum sessizce
/// doğru görünür.
#[tokio::test]
async fn health_reports_which_client_id_source_was_used() {
    if !prerequisites_met("sağlık") {
        return;
    }
    let config = temp_config("saglik");
    let provider = install(&config);

    let health = provider.health().await.unwrap();
    assert!(
        health.reachable,
        "SoundCloud'a ulaşıldı ama sağlık cevabı olumsuz: {:?}",
        health.detail
    );
    let detail = health.detail.unwrap_or_default();
    assert!(
        detail.contains("client_id kaynağı"),
        "sağlık cevabı client_id kaynağını söylemiyor: {detail}"
    );
    // Katalog boyutu bilinmiyor; sıfır değil **bilinmiyor** denmeli.
    assert_eq!(health.track_count, None);
}

/// Aramadan çıkan bir parça gerçekten çalınabilir bir adrese çözülmeli.
///
/// Zincirin son halkası: kullanıcı için "çalıyor mu" sorusunun cevabı budur.
#[tokio::test]
async fn a_search_hit_resolves_to_a_playable_http_stream() {
    if !prerequisites_met("kaynak çözümü") {
        return;
    }
    let config = temp_config("cozum");
    let provider = install(&config);

    let hits = provider.search("lofi", 10).await.unwrap();
    assert!(!hits.is_empty(), "canlı arama boş döndü");

    // Parçaların ~%1'i yalnızca HLS sunuyor ve o durumda eklenti bilerek hata
    // döndürüyor. Testin işi o %1'i kovalamak değil; ilk çözülen yeter.
    let mut resolved = None;
    let mut refusals = Vec::new();
    for hit in hits.iter().take(5) {
        match provider.resolve_source(&hit.id).await {
            Ok(Some(source)) => {
                resolved = Some(source);
                break;
            }
            Ok(None) => refusals.push(format!("{}: çalınamaz", hit.id)),
            // `chain_text`: `Display` yalnızca `ADIM: X` basıyor (D-061'in dersi).
            Err(err) => refusals.push(format!("{}: {}", hit.id, err.chain_text())),
        }
    }

    let source = resolved
        .unwrap_or_else(|| panic!("beş adayın hiçbiri çözülmedi:\n  {}", refusals.join("\n  ")));

    match source {
        AudioSource::HttpStream { url, headers } => {
            assert!(
                url.starts_with("https://"),
                "akış adresi https değil: {url}"
            );
            // K3: adres istemcinin kendi çekeceği bir kaynak. SoundCloud
            // imzayı sorgu dizesinde taşıyor, başlık göndermiyoruz.
            assert!(headers.is_empty(), "beklenmeyen başlık: {headers:?}");
        }
        other => panic!("http akışı bekleniyordu, gelen: {other:?}"),
    }
}

/// Olmayan bir parça **hata değil**, "yok" cevabı vermeli.
///
/// Bu ayrım eklentinin ilk canlı sürüşünde kırıldı: `api_get` 404'ü genel bir
/// hataya çeviriyordu ve `resolve_source`'un "yok" kolu hiç çalışmıyordu.
/// Sahte bir sunucu bunu gösteremezdi — 404'ü biz üretiyor olurduk (K9).
#[tokio::test]
async fn a_missing_track_is_an_answer_not_an_error() {
    if !prerequisites_met("olmayan parça") {
        return;
    }
    let config = temp_config("yok");
    let provider = install(&config);

    let id = ProviderTrackId::new(ProviderId::new("soundcloud"), "999999999999");
    let source = provider
        .resolve_source(&id)
        .await
        .expect("olmayan parça hata değil, cevap olmalı");
    assert_eq!(source, None);
}

/// §2.2'nin asıl kanıtı: SoundCloud'dan gelen ses **gerçekten çalıyor**.
///
/// Arama ve adres çözümü kâğıt üstünde doğru olabilir; kullanıcı için soru
/// "ses çıkıyor mu". Ses aygıtı olmayan ortamda test kendini atlar.
#[tokio::test]
async fn a_soundcloud_track_actually_plays() {
    if !prerequisites_met("çalma") {
        return;
    }
    let config = temp_config("calma");
    let provider = install(&config);

    let mut registry = headshell_core::provider::ProviderRegistry::new();
    registry.register(std::sync::Arc::new(provider));

    let hits = registry
        .get(&ProviderId::new("soundcloud"))
        .unwrap()
        .search("nujabes aruarian dance", 1)
        .await
        .unwrap();
    let item = headshell_core::playback::QueueItem {
        id: hits[0].id.clone(),
        track: hits[0].track.clone(),
    };

    let mut player = headshell_core::playback::Player::new(registry);
    if let Err(err) = player.play_items(vec![item]).await {
        eprintln!("çalma: ses hattı açılamadı ({err}) — atlanıyor");
        return;
    }

    // Sesin gerçekten ilerlediğini ölç: pozisyon artmalı.
    let mut positions = Vec::new();
    for _ in 0..12 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        player.tick().await.unwrap();
        positions.push((player.state(), player.anchor().position_ms));
        if player.anchor().position_ms > 1500 {
            break;
        }
    }

    let best = positions.iter().map(|(_, ms)| *ms).max().unwrap_or(0);
    assert!(
        best > 1000,
        "ses ilerlemedi; ölçülen durum/pozisyon dizisi: {positions:?}"
    );
}
