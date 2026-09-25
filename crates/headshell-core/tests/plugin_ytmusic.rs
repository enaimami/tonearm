//! YouTube Music eklentisi, **gerçek YouTube Music'e karşı** (Faz 2 §2.5, D-048).
//!
//! `plugin_soundcloud.rs`'in kardeşi ve aynı yordamı izliyor: eklenti canlı
//! katalogdan (`headshell/plugins`, D-071) veri dizinine kurulup canlı
//! serviste yürütülüyor. Sınanan şey protokol
//! değil (onu `plugin_process.rs` sabit kataloglu bir fixture'la sınıyor),
//! **eklentinin kendisi**: InnerTube araması, yt-dlp'nin çözdüğü adres ve
//! sesin gerçekten çalması.
//!
//! **Bu testler varsayılan koşuma dahildir** (D-043) ve iki başarısızlığı ayrı
//! tutuyor (K9):
//!
//! - **Ulaşamamak** başarısızlık değil: ağ yoksa test kendini atlar ve
//!   sebebini `stderr`'e yazar.
//! - **Ulaşıp beklenmeyeni almak** düşer.
//!
//! Ne `python3` ne `yt-dlp` bir ön koşul: eklenti gömülü QuickJS'te koşuyor
//! ve motor yt-dlp'nin **bu platformun** kendi kendine yeten ikilisini
//! manifestteki sabitlenmiş sürümden indiriyor (D-069). Ağ varken indirememek
//! **atlama sebebi değil, düşme sebebidir** — beyan edilen adres ölmüşse
//! (yetim) bunu sessizce geçmek, bozuk bir manifesti yeşil göstermek olurdu.
//!
//! Kırmızı yandığında ilk soru: **son commit'e mi baksam, yoksa
//! `yt-dlp -J "https://music.youtube.com/watch?v=..."` mi çeksem?** İkincisi
//! çalışıyorsa ve testler hâlâ kırmızıysa kusur bizdedir. yt-dlp eskiyse
//! kusur da bizde: sürümü manifest sabitliyor, kullanıcı değil.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::path::PathBuf;

use headshell_core::config::Config;
use headshell_core::ids::{ProviderId, ProviderTrackId};
use headshell_core::plugin::PluginProvider;
use headshell_core::plugin::artifact::{ArtifactStore, default_artifact_source};
use headshell_core::plugin::manifest::{PluginManifest, Requirement};
use headshell_core::provider::{AudioSource, Capabilities, Provider};
use headshell_core::secrets::{Secrets, plugin_namespace};

/// yt-dlp'nin bu platform ikilisi: koşum başına en çok bir kez iner.
///
/// Eklenti yt-dlp'yi kendi aramıyor (D-055); onu motor kuruyor. Test de
/// **aynı yoldan** geçiyor. İndirme ~40 MB, o yüzden sonuç projenin
/// `target/tmp`'sinde tutuluyor (D-070) — ama körü körüne değil:
///
/// - Önbellekteki dosyanın karması her koşumda yeniden doğrulanıyor.
/// - İndirme adresinin **hâlâ yaşadığı** her koşumda soruluyor (gövdesi
///   okunmadan). Önbellek bunu atlasaydı adres öldüğünde (yetim, D-055) bu
///   makine yeşil kalır, temiz bir makine kırmızı yanardı — sonuç makineye
///   bağlı olurdu.
fn shared_ytdlp(requirement: &Requirement) -> Result<PathBuf, String> {
    static SHARED: std::sync::OnceLock<Result<PathBuf, String>> = std::sync::OnceLock::new();
    SHARED.get_or_init(|| fetch_ytdlp(requirement)).clone()
}

fn fetch_ytdlp(requirement: &Requirement) -> Result<PathBuf, String> {
    let cache = support::root().join("ytdlp-cache");
    let store = ArtifactStore::new(&Config::with_data_dir(&cache));
    let platform = store.platform().to_owned();
    let Some(asset) = requirement.asset_for(&platform) else {
        return Err(format!("{platform} için yayın beyan edilmemiş"));
    };
    let source = default_artifact_source().map_err(|err| err.chain_text())?;

    // Adres yaşıyor mu: yalnızca durum kodu, gövde okunmadan.
    match source.open(&asset.url) {
        Ok(response) if response.status == 404 || response.status == 410 => {
            return Err(format!(
                "YETİM — {} {} dedi; önbellekteki kopya bunu gizlemeyecek",
                asset.url, response.status
            ));
        }
        Ok(_) => {}
        // Ulaşılamadıysa hüküm yok: önbellek varsa onunla devam, yoksa
        // aşağıdaki kurulum zaten ne olduğunu söyleyecek.
        Err(err) => eprintln!(
            "uyarı: {} yoklanamadı ({}), önbellekle devam",
            asset.url,
            err.chain_text().replace('\n', " ")
        ),
    }

    // `install` karma tutuyorsa ağa hiç çıkmıyor, tutmuyorsa yeniden iniyor.
    match store.install(source.as_ref(), requirement) {
        Ok(outcome) if outcome.is_ready() => Ok(store.artifact_path(requirement)),
        Ok(outcome) => Err(outcome.describe()),
        Err(err) => Err(err.chain_text()),
    }
}

/// YouTube Music'e TCP ile ulaşılabiliyor mu.
///
/// Yalnızca DNS + bağlantı; HTTP'ye hiç girilmiyor. Amaç "ağ var mı"
/// sorusunu cevaplamak, servisin sağlığını ölçmek değil.
fn ytmusic_reachable() -> bool {
    use std::net::ToSocketAddrs;
    let Ok(mut addrs) = ("music.youtube.com", 443).to_socket_addrs() else {
        return false;
    };
    addrs.any(|addr| {
        std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(5)).is_ok()
    })
}

/// Testin koşulup koşulamayacağını söyler; koşulamıyorsa sebebini yazar.
fn prerequisites_met(test: &str) -> bool {
    // Motor eseri HTTP ile indiriyor; bu derlemede istemci yoksa test
    // koşulamaz. "Koşamadım" ile "koştu ve düştü" ayrı tanılar (K9) —
    // ve atlanan test geçmiş sayılmaz (D-043).
    if let Err(err) = default_artifact_source() {
        eprintln!(
            "{test}: {} — atlanıyor (bu derleme yt-dlp'yi indiremez)",
            err.chain_text().replace('\n', " ")
        );
        return false;
    }
    if !ytmusic_reachable() {
        eprintln!("{test}: music.youtube.com:443'e ulaşılamadı — atlanıyor (ağ yok sayılıyor)");
        return false;
    }
    true
}

fn temp_config(name: &str) -> support::TestConfig {
    support::TestConfig::new(&format!("ytmusic-{name}"))
}

/// CI ortam değişkenindeki YouTube çerezlerini sır deposuna yazar.
///
/// YouTube veri merkezi adreslerine bot duvarı çıkarıyor ("Sign in to
/// confirm you're not a bot") ve çerezsiz hiçbir akış çözülemiyor. Çerez
/// **sır deposundan** geçiyor, çıplak bir ortam değişkeninden değil:
/// kullanıcının `headshell secret set plugin:ytmusic cookies` ile yaptığı
/// yolun aynısı sınansın (D-042).
///
/// Dönüş: çerez verildi mi. Verilmediyse bot duvarı ölçülebilir bir şey
/// değildir ve test kendini atlar.
fn store_cookies(config: &Config) -> bool {
    let Ok(raw) = std::env::var("HEADSHELL_TEST_YTMUSIC_COOKIES") else {
        return false;
    };
    if raw.trim().is_empty() {
        return false;
    }
    let mut secrets = Secrets::load(&config.secrets_path()).unwrap();
    secrets.set(&plugin_namespace("ytmusic"), "cookies", &raw);
    secrets.save(&config.secrets_path()).unwrap();
    true
}

/// Eklentiyi canlı katalogdan kurar — kullanıcının yaptığı şeyin aynısı
/// (D-071). Kataloğa ulaşılamıyorsa `None`: test atlanır ve sebebi yazılır.
async fn install(config: &Config, test: &str) -> Option<PluginProvider> {
    let dir = match support::install_from_catalog(config, "ytmusic").await {
        Ok(dir) => dir,
        Err(reason) => {
            eprintln!("{test}: {reason} — atlanıyor (ağ yok sayılıyor)");
            return None;
        }
    };

    let manifest = PluginManifest::load(&dir).unwrap();

    // Motorun işi: eklentinin beyan ettiği eserleri kur. Test bunu
    // kullanıcının `headshell plugin install` ile yaptığının aynısı olarak
    // yapıyor (paylaşılan önbellekten), sonra sağlayıcıyı kuruyor.
    for requirement in &manifest.requires {
        let shared = match shared_ytdlp(requirement) {
            Ok(path) => path,
            Err(reason) => panic!("yt-dlp kurulamadı: {reason}"),
        };
        let store = ArtifactStore::new(config);
        std::fs::create_dir_all(store.runtime_dir()).unwrap();
        // Sabit bağ, kopya değil: her test 40 MB'ı yeniden yazmasın. Bağ
        // kurulamazsa (başka dosya sistemi) kopyaya düşülür; `copy`
        // çalıştırma bitini de taşıyor.
        let target = store.artifact_path(requirement);
        if std::fs::hard_link(&shared, &target).is_err() {
            std::fs::copy(&shared, &target).unwrap();
        }
    }

    let secrets = Secrets::load(&config.secrets_path()).unwrap();
    Some(PluginProvider::from_manifest(config, &manifest, &dir, &secrets).unwrap())
}

/// Arama, K6'nın bulanık eşleşme halkasının istediği alanları vermeli.
///
/// Bu testin var oluş sebebi bir ölçüm: yt-dlp'nin kendi araması yalnızca
/// `title` + `id` veriyordu, sanatçı ve süre yoktu (D-048). InnerTube'a
/// geçilmesinin tek gerekçesi bu ve regresyonu burada yakalanır.
#[tokio::test]
async fn a_search_carries_artist_and_duration_not_just_a_title() {
    if !prerequisites_met("arama") {
        return;
    }
    let config = temp_config("arama");
    let Some(provider) = install(&config, "arama").await else {
        return;
    };

    assert!(
        provider
            .info()
            .capabilities
            .contains(Capabilities::SEARCH | Capabilities::STREAM)
    );

    let hits = provider.search("nujabes aruarian dance", 5).await.unwrap();
    assert!(!hits.is_empty(), "canlı arama boş döndü");

    let mut with_duration = 0;
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
            "ytmusic",
            "kimlik yanlış ad alanında: {}",
            hit.id
        );
        if hit.track.duration_ms.unwrap_or(0) > 0 {
            with_duration += 1;
        }
    }
    assert_eq!(
        with_duration,
        hits.len(),
        "süresi olmayan parça var; bulanık eşleşme halkası bunu kullanamaz: {hits:?}"
    );
}

/// Sonuçlar YouTube'un verdiği **alaka sırasında** kalmalı: tam eşleşen bir
/// sorguda ilk sonuç aranan parça olmalı.
///
/// İlk canlı koşum bunu kırık buldu: InnerTube ağacında satırları toplayan
/// gezinme yığın tabanlıydı ve sırayı tersine çeviriyordu. `limit=5` en iyi
/// eşleşmeyi değil rastgele beş satırı veriyordu — aranan parça listede hiç
/// yoktu. Birim testi bunu göremezdi: gelen beş satırın hepsi geçerli
/// parçaydı, yalnızca yanlış beş taneydi.
///
/// **Neden iki çağrının sırasını karşılaştırmıyoruz:** ölçüldü, YouTube aynı
/// sorguya aynı sırayı vermiyor. Beş koşumda ilk sonuç 5/5 aynı çıktı, 2. ve
/// 3. sıralar oynadı. Test yalnızca kararlı olan şeyi iddia ediyor —
/// D-045'in MusicBrainz'de öğrendiği ders (koşumlar arası kararsız bir
/// servise kararlılık yazdırmak, kendi kodunu değil servisi sınamaktır).
#[tokio::test]
async fn the_best_match_comes_first_not_somewhere_in_the_list() {
    if !prerequisites_met("sıra") {
        return;
    }
    let config = temp_config("sira");
    let Some(provider) = install(&config, "sıra").await else {
        return;
    };

    let hits = provider.search("nujabes aruarian dance", 5).await.unwrap();
    assert!(!hits.is_empty(), "canlı arama boş döndü");

    let first = &hits[0].track;
    let artist = first.artist.to_lowercase();
    let title = first.title.to_lowercase();
    assert!(
        artist.contains("nujabes") && title.contains("aruarian"),
        "ilk sonuç aranan parça değil — sıra bozulmuş olabilir. \
         Gelen liste: {:?}",
        hits.iter()
            .map(|hit| format!("{} - {}", hit.track.artist, hit.track.title))
            .collect::<Vec<_>>()
    );
}

/// Çözülen kaynak, **kısıtlamayı kaldıran** `Range` başlığını taşımalı.
///
/// Ölçüldü (D-048): aynı adres düz GET'te 32 KB/s, `Range: bytes=0-` ile
/// 8 MB/s veriyor — 250 kat. Başlık düşerse hiçbir şey "bozulmaz", ses
/// yalnızca 2× gerçek zamanda iner ve ilk dalgalanmada kesilir. Sessizce
/// kötüleşen bir kusurun tek bekçisi bu assert.
#[tokio::test]
async fn a_search_hit_resolves_to_a_stream_with_the_unthrottling_range_header() {
    if !prerequisites_met("kaynak çözümü") {
        return;
    }
    let config = temp_config("cozum");
    let cookies_given = store_cookies(&config);
    let Some(provider) = install(&config, "kaynak çözümü").await else {
        return;
    };

    let hits = provider.search("nujabes aruarian dance", 5).await.unwrap();
    assert!(!hits.is_empty(), "canlı arama boş döndü");

    let mut resolved = None;
    let mut refusals = Vec::new();
    for hit in hits.iter().take(3) {
        match provider.resolve_source(&hit.id).await {
            Ok(Some(source)) => {
                resolved = Some(source);
                break;
            }
            Ok(None) => refusals.push(format!("{}: çalınamaz", hit.id)),
            // `chain_text`, düz `{err}` değil: `Error`'ın `Display`'i yalnızca
            // `ADIM: X` basıyor ve bu test CI'da tam olarak öyle düşmüştü —
            // üç aday, üç aşama adı, sıfır sebep. Tanıyı yutan bir hata
            // mesajı K9'un yasakladığı şeydir.
            Err(err) => refusals.push(format!("{}: {}", hit.id, err.chain_text())),
        }
    }

    let Some(source) = resolved else {
        let report = refusals.join("\n  ");

        // YouTube veri merkezi adreslerine bot duvarı çıkarıyor. Çerez
        // verilmişse duvarı aşmak **bizim işimiz** ve aşamamak düşme
        // sebebidir. Çerez verilmemişse ölçülebilir bir şey yok: servis
        // bakmamıza izin vermedi, ürün hakkında hiçbir şey söylemedi.
        // Bu, D-043'ün "ulaşamadım" tarafıdır (D-061).
        //
        // Eşleşme **dar**: yalnızca bot duvarının kendi imzası. Başka her
        // ret hâlâ düşürür, yoksa gerçek bir regresyon buraya saklanırdı.
        if !cookies_given && report.contains("not a bot") {
            eprintln!(
                "kaynak çözümü: YouTube bot duvarı ve çerez verilmemiş — \
                 atlanıyor (bu bir başarısızlık değil).\n  \
                 Çerez vermek için: HEADSHELL_TEST_YTMUSIC_COOKIES\n  {report}"
            );
            return;
        }
        panic!("üç adayın hiçbiri çözülmedi:\n  {report}");
    };

    match source {
        AudioSource::HttpStream { url, headers } => {
            assert!(
                url.starts_with("https://"),
                "akış adresi https değil: {url}"
            );
            let range = headers
                .iter()
                .find(|header| header.name.eq_ignore_ascii_case("range"))
                .unwrap_or_else(|| panic!("Range başlığı yok; akış kısıtlanır: {headers:?}"));
            assert_eq!(range.value, "bytes=0-");
        }
        other => panic!("http akışı bekleniyordu, gelen: {other:?}"),
    }
}

/// Olmayan bir parça, yt-dlp'nin **kendi cümlesiyle** hata vermeli.
///
/// SoundCloud eklentisinde bunun karşılığı "yok cevabıdır, hata değil" idi;
/// burada bilerek farklı: yt-dlp "This video is unavailable", "Private video"
/// ve "Sign in to confirm you're not a bot" arasındaki farkı biliyor ve o
/// ayrım kullanıcıya ulaşmalı. `Ok(None)`'a düzleştirmek üç tanıyı bire
/// indirirdi (K9, D-048).
#[tokio::test]
async fn a_missing_track_fails_with_the_tools_own_words() {
    if !prerequisites_met("olmayan parça") {
        return;
    }
    let config = temp_config("yok");
    let Some(provider) = install(&config, "olmayan parça").await else {
        return;
    };

    let id = ProviderTrackId::new(ProviderId::new("ytmusic"), "zzzzzzzzzzz");
    let err = provider
        .resolve_source(&id)
        .await
        .expect_err("olmayan parça için hata bekleniyordu");

    // `Display` yalnızca aşamayı yazar (`ADIM: PROVIDER_CALL`); sebep zinciri
    // `chain_text()`'te ve CLI ile GUI'nin kullanıcıya gösterdiği de o.
    let message = err.chain_text();
    assert!(
        message.contains("yt-dlp"),
        "hata yt-dlp'nin mesajını taşımıyor: {message}"
    );
    assert!(
        message.contains("ADIM: PROVIDER_CALL"),
        "hata hangi aşamada olduğunu söylemiyor: {message}"
    );
}

/// §2.5'in asıl kanıtı: YouTube Music'ten gelen ses **gerçekten çalıyor**.
///
/// Buradaki tuzak ölçülmüştü: yt-dlp'nin `bestaudio` seçimi opus/webm verir
/// ve çekirdeğin symphonia'sında ne o kodek ne o kap var. Eklenti m4a'ya
/// sabitli; bu test o sabitin doğru olduğunu söyleyen tek yer.
#[tokio::test]
async fn a_ytmusic_track_actually_plays() {
    if !prerequisites_met("çalma") {
        return;
    }
    let config = temp_config("calma");
    let Some(provider) = install(&config, "çalma").await else {
        return;
    };

    let mut registry = headshell_core::provider::ProviderRegistry::new();
    registry.register(std::sync::Arc::new(provider));

    let hits = registry
        .get(&ProviderId::new("ytmusic"))
        .unwrap()
        .search("nujabes aruarian dance", 1)
        .await
        .unwrap();
    assert!(!hits.is_empty(), "canlı arama boş döndü");
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
