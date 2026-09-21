//! YouTube Music eklentisi, **gerçek YouTube Music'e karşı** (Faz 2 §2.5, D-048).
//!
//! `plugin_soundcloud.rs`'in kardeşi ve aynı yordamı izliyor: repodaki eklenti
//! veri dizinine kurulup canlı serviste yürütülüyor. Sınanan şey protokol
//! değil (onu `plugin_process.rs` sabit kataloglu bir fixture'la sınıyor),
//! **eklentinin kendisi**: InnerTube araması, yt-dlp'nin çözdüğü adres ve
//! sesin gerçekten çalması.
//!
//! **Bu testler varsayılan koşuma dahildir** (D-043) ve iki başarısızlığı ayrı
//! tutuyor (K9):
//!
//! - **Ulaşamamak** başarısızlık değil: ağ yoksa ya da `python3` yoksa test
//!   kendini atlar ve sebebini `stderr`'e yazar.
//! - **Ulaşıp beklenmeyeni almak** düşer.
//!
//! `yt-dlp` artık bir ön koşul değil: motor onu manifestteki sabitlenmiş
//! sürümden indiriyor (D-055). Ağ varken indirememek **atlama sebebi değil,
//! düşme sebebidir** — beyan edilen adres ölmüşse (yetim) bunu sessizce
//! geçmek, bozuk bir manifesti yeşil göstermek olurdu.
//!
//! Kırmızı yandığında ilk soru: **son commit'e mi baksam, yoksa
//! `yt-dlp -J "https://music.youtube.com/watch?v=..."` mi çeksem?** İkincisi
//! çalışıyorsa ve testler hâlâ kırmızıysa kusur bizdedir. yt-dlp eskiyse
//! kusur da bizde: sürümü manifest sabitliyor, kullanıcı değil.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use tonearm_core::config::Config;
use tonearm_core::ids::{ProviderId, ProviderTrackId};
use tonearm_core::plugin::PluginProvider;
use tonearm_core::plugin::manifest::{PluginManifest, Requirement};
use tonearm_core::plugin::runtime::Engine;
use tonearm_core::provider::{AudioSource, Capabilities, Provider};
use tonearm_core::secrets::{Secrets, plugin_namespace};

/// Eklentinin repodaki kaynağı (fixture değil — kullanıcıya dağıtılan dosya).
fn plugin_source_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/ytmusic")
}

fn command_runs(program: &str, args: &[&str]) -> bool {
    std::process::Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Motorun kuracağı yt-dlp'yi bir kez indirip testler arasında paylaşır.
///
/// Eklenti artık yt-dlp'yi kendi aramıyor (D-055); onu motor kuruyor. Test de
/// **aynı yoldan** geçiyor — sınanan şey tam olarak kullanıcının yaşadığı
/// akış. İndirme her test için tekrarlanmasın diye sonuç sabit bir dizinde
/// tutuluyor; motor eseri karmasıyla adlandırdığı için bu önbellek bayatlayamaz.
fn cached_ytdlp(requirement: &Requirement) -> Result<PathBuf, String> {
    let cache = std::env::temp_dir().join("tonearm-test-runtime");
    let cached = cache.join(requirement.file_name());
    if cached.exists() {
        return Ok(cached);
    }

    let engine = Engine::new(&Config::with_data_dir(
        std::env::temp_dir().join("tonearm-test-runtime-home"),
    ));
    let http = tonearm_core::net::default_http_client().map_err(|err| err.chain_text())?;
    match engine.install(&http, requirement) {
        Ok(outcome) if outcome.is_ready() => {}
        Ok(outcome) => return Err(outcome.describe()),
        Err(err) => return Err(err.chain_text()),
    }

    std::fs::create_dir_all(&cache).map_err(|err| err.to_string())?;
    std::fs::copy(engine.artifact_path(requirement), &cached).map_err(|err| err.to_string())?;
    Ok(cached)
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
    if !command_runs("python3", &["--version"]) {
        eprintln!("{test}: python3 yok — atlanıyor (bu bir başarısızlık değil)");
        return false;
    }
    // Motor eseri HTTP ile indiriyor; bu derlemede istemci yoksa test
    // koşulamaz. "Koşamadım" ile "koştu ve düştü" ayrı tanılar (K9) —
    // ve atlanan test geçmiş sayılmaz (D-043).
    if let Err(err) = tonearm_core::net::default_http_client() {
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

fn temp_config(name: &str) -> Config {
    let dir = std::env::temp_dir().join(format!(
        "tonearm-ytmusic-{}-{}-{name}",
        std::process::id(),
        jiff::Timestamp::now().as_nanosecond()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    Config::with_data_dir(dir)
}

/// CI ortam değişkenindeki YouTube çerezlerini sır deposuna yazar.
///
/// YouTube veri merkezi adreslerine bot duvarı çıkarıyor ("Sign in to
/// confirm you're not a bot") ve çerezsiz hiçbir akış çözülemiyor. Çerez
/// **sır deposundan** geçiyor, çıplak bir ortam değişkeninden değil:
/// kullanıcının `tonearm secret set plugin:ytmusic cookies` ile yaptığı
/// yolun aynısı sınansın (D-042).
///
/// Dönüş: çerez verildi mi. Verilmediyse bot duvarı ölçülebilir bir şey
/// değildir ve test kendini atlar.
fn store_cookies(config: &Config) -> bool {
    let Ok(raw) = std::env::var("TONEARM_TEST_YTMUSIC_COOKIES") else {
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

/// Eklentiyi repodan veri dizinine kurar — kullanıcının yaptığı şeyin aynısı.
fn install(config: &Config) -> PluginProvider {
    let dir = config.plugins_dir().join("ytmusic");
    std::fs::create_dir_all(&dir).unwrap();
    for file in ["main.py", "plugin.json"] {
        std::fs::copy(plugin_source_dir().join(file), dir.join(file)).unwrap();
    }

    let manifest = PluginManifest::load(&dir).unwrap();

    // Motorun işi: eklentinin beyan ettiği eserleri kur. Test bunu
    // kullanıcının `tonearm plugin install` ile yaptığının aynısı olarak
    // yapıyor, sonra sağlayıcıyı kuruyor — el sıkışmaya giden yol haritası
    // ancak eser yerindeyse doluyor.
    for requirement in &manifest.requires {
        let cached = match cached_ytdlp(requirement) {
            Ok(path) => path,
            Err(reason) => panic!("yt-dlp kurulamadı: {reason}"),
        };
        let engine = Engine::new(config);
        std::fs::create_dir_all(engine.runtime_dir()).unwrap();
        std::fs::copy(&cached, engine.artifact_path(requirement)).unwrap();
    }

    let secrets = Secrets::load(&config.secrets_path()).unwrap();
    PluginProvider::from_manifest(config, &manifest, &dir, &secrets).unwrap()
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
    let provider = install(&config);

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
    let provider = install(&config);

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
    let provider = install(&config);

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
                 Çerez vermek için: TONEARM_TEST_YTMUSIC_COOKIES\n  {report}"
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
    let provider = install(&config);

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
    let provider = install(&config);

    let mut registry = tonearm_core::provider::ProviderRegistry::new();
    registry.register(std::sync::Arc::new(provider));

    let hits = registry
        .get(&ProviderId::new("ytmusic"))
        .unwrap()
        .search("nujabes aruarian dance", 1)
        .await
        .unwrap();
    assert!(!hits.is_empty(), "canlı arama boş döndü");
    let item = tonearm_core::playback::QueueItem {
        id: hits[0].id.clone(),
        track: hits[0].track.clone(),
    };

    let mut player = tonearm_core::playback::Player::new(registry);
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
