//! Uçtan uca: gerçek bir torrent'ten gerçek bir HTTP akışı (D-047).
//!
//! ## Neden peer yok
//!
//! Test bir torrent **üretiyor** ve verisini indirme dizinine önceden
//! koyuyor; `librqbit` başlangıçta karma doğrulaması yapıp torrent'i tamam
//! sayıyor. Böylece sınanan şey **bizim** kodumuz oluyor — üstveriden dosya
//! listesi çıkarma, akış açma, `Range` yanıtlama — `librqbit`'in peer bulma
//! yeteneği değil. Peer'a bağlı bir test, ağın hâline göre bazen geçen bir
//! test olurdu ve D-043'ün ayırmak istediği iki başarısızlığı karıştırırdı.
//!
//! Sınanmayan şey açıkça şudur: peer'lardan **indirme**. O `librqbit`'in
//! kendi test kümesinin işi; bizim yazdığımız satır değil.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use headshell_plugin_torrent::engine::Engine;
use headshell_plugin_torrent::stream::StreamServer;
use librqbit::CreateTorrentOptions;
use librqbit::spawn_utils::BlockingSpawner;

/// Depodaki ses fixture'ı (D-046'da üretildi; sentetik, telifli kayıt değil).
const FIXTURE: &str = "../../fixtures/audio/fingerprint_sample.flac";

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "headshell-torrent-it-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    std::fs::create_dir_all(&dir).expect("geçici dizin");
    dir
}

/// Bir torrent üretir, verisini indirme dizinine yerleştirir ve motoru döndürür.
async fn seeded_engine(name: &str) -> (Arc<Engine>, String, Vec<u8>, PathBuf) {
    let root = scratch(name);
    let data_dir = root.join("state");
    std::fs::create_dir_all(&data_dir).expect("durum dizini");

    let content = std::fs::read(Path::new(FIXTURE)).expect("fixture okunmalı");
    let staging = root.join("staging");
    std::fs::create_dir_all(&staging).expect("hazırlık dizini");
    let staged_file = staging.join("sample.flac");
    std::fs::write(&staged_file, &content).expect("fixture kopyalanmalı");

    let created = librqbit::create_torrent(
        &staged_file,
        CreateTorrentOptions {
            name: None,
            trackers: Vec::new(),
            piece_length: Some(16 * 1024),
        },
        &BlockingSpawner::new(4),
    )
    .await
    .expect("torrent üretilmeli");

    let infohash = created.info_hash().as_string();
    let torrent_path = root.join("sample.torrent");
    std::fs::write(&torrent_path, created.as_bytes().expect("torrent baytları"))
        .expect("torrent dosyası yazılmalı");

    // Motorun bakacağı yere veriyi önceden koy: `<indirme>/<infohash>/sample.flac`.
    let engine = Arc::new(Engine::new(data_dir).await.expect("motor açılmalı"));
    let output = engine.download_dir().join(&infohash);
    std::fs::create_dir_all(&output).expect("çıktı dizini");
    std::fs::write(output.join("sample.flac"), &content).expect("veri yerleştirilmeli");

    // Gerçek akışın aynısı: aramada görülen kaynak kataloğa yazılır, çalma
    // onu oradan okur. Yazmazsak akış sunucusu çıplak bir magnet üretir ve
    // peer arar — bu testte hiç peer yok.
    engine
        .remember(vec![(
            infohash.clone(),
            headshell_plugin_torrent::engine::CatalogEntry {
                title: "Test Yayımı".to_owned(),
                source_url: torrent_path.display().to_string(),
                indexer: None,
            },
        )])
        .await;

    (engine, infohash, content, torrent_path)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torrents_audio_files_are_listed_from_its_metadata() {
    let (engine, infohash, content, torrent_path) = seeded_engine("liste").await;
    let handle = engine
        .handle(&infohash, &torrent_path.display().to_string())
        .await
        .expect("torrent hazır olmalı");

    let files = Engine::audio_files(&handle).expect("dosyalar listelenmeli");
    assert_eq!(files.len(), 1, "tek ses dosyası bekleniyor: {files:?}");
    assert_eq!(files[0].index, 0);
    assert_eq!(files[0].file_name, "sample.flac");
    assert_eq!(
        files[0].len,
        content.len() as u64,
        "üstverideki uzunluk gerçek dosyayla uyuşmalı"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_local_stream_serves_the_whole_file_byte_for_byte() {
    let (engine, infohash, content, torrent_path) = seeded_engine("tam").await;
    engine
        .handle(&infohash, &torrent_path.display().to_string())
        .await
        .expect("torrent hazır olmalı");

    let server = StreamServer::spawn(Arc::clone(&engine))
        .await
        .expect("akış sunucusu");
    let url = server.url_for(&infohash, 0);
    assert!(
        url.starts_with("http://127.0.0.1:"),
        "yalnızca yerel: {url}"
    );

    let response = reqwest::get(&url).await.expect("akış istenebilmeli");
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("audio/flac")
    );
    assert_eq!(
        response
            .headers()
            .get("accept-ranges")
            .and_then(|value| value.to_str().ok()),
        Some("bytes"),
        "oynatıcı ileri sarabilmeli"
    );
    let body = response.bytes().await.expect("gövde").to_vec();
    assert_eq!(body.len(), content.len());
    assert_eq!(body, content, "akan baytlar dosyanın kendisi olmalı");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_range_request_returns_exactly_that_slice() {
    let (engine, infohash, content, torrent_path) = seeded_engine("aralik").await;
    engine
        .handle(&infohash, &torrent_path.display().to_string())
        .await
        .expect("torrent hazır olmalı");
    let server = StreamServer::spawn(Arc::clone(&engine))
        .await
        .expect("akış sunucusu");

    let client = reqwest::Client::new();
    let response = client
        .get(server.url_for(&infohash, 0))
        // Parça sınırının ötesine geçen bir dilim: `FileStream`'de konumlanma
        // gerçekten çalışıyor mu, yoksa yalnızca baştan mı okuyor?
        .header("Range", "bytes=20000-20099")
        .send()
        .await
        .expect("aralık istenebilmeli");

    assert_eq!(response.status().as_u16(), 206);
    assert_eq!(
        response
            .headers()
            .get("content-range")
            .and_then(|value| value.to_str().ok()),
        Some(format!("bytes 20000-20099/{}", content.len()).as_str())
    );
    let body = response.bytes().await.expect("gövde").to_vec();
    assert_eq!(body.len(), 100);
    assert_eq!(
        body,
        content[20000..20100],
        "dilim dosyanın tam o yerinden gelmeli"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_range_past_the_end_is_416_not_a_silent_restart_from_zero() {
    let (engine, infohash, content, torrent_path) = seeded_engine("416").await;
    engine
        .handle(&infohash, &torrent_path.display().to_string())
        .await
        .expect("torrent hazır olmalı");
    let server = StreamServer::spawn(Arc::clone(&engine))
        .await
        .expect("akış sunucusu");

    let response = reqwest::Client::new()
        .get(server.url_for(&infohash, 0))
        .header("Range", format!("bytes={}-", content.len() + 1000))
        .send()
        .await
        .expect("istek gönderilebilmeli");
    assert_eq!(
        response.status().as_u16(),
        416,
        "dosya dışını sessizce baştan göndermek oynatıcıyı yanlış konuma götürür"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_token_is_not_decoration_a_wrong_one_is_refused() {
    let (engine, infohash, _content, torrent_path) = seeded_engine("jeton").await;
    engine
        .handle(&infohash, &torrent_path.display().to_string())
        .await
        .expect("torrent hazır olmalı");
    let server = StreamServer::spawn(Arc::clone(&engine))
        .await
        .expect("akış sunucusu");

    let real = server.url_for(&infohash, 0);
    // Jetonu bozuyoruz; adresin geri kalanı doğru.
    let forged = real.replacen(
        real.split('/').nth(3).expect("jeton parçası"),
        "0000000000000000000000000000000",
        1,
    );
    let response = reqwest::get(&forged).await.expect("istek gönderilebilmeli");
    assert_eq!(
        response.status().as_u16(),
        404,
        "jeton olmadan indirilenler okunabilmemeli"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_index_that_does_not_exist_is_404_not_a_hang() {
    let (engine, infohash, _content, torrent_path) = seeded_engine("yok").await;
    engine
        .handle(&infohash, &torrent_path.display().to_string())
        .await
        .expect("torrent hazır olmalı");
    let server = StreamServer::spawn(Arc::clone(&engine))
        .await
        .expect("akış sunucusu");

    let response = reqwest::get(server.url_for(&infohash, 99))
        .await
        .expect("istek gönderilebilmeli");
    assert_eq!(response.status().as_u16(), 404);
}

/// Peer'ı olmayan bir kaynak **bütçe içinde** pes etmeli.
///
/// Bu testin sebebi bir kusur: bütçe önce yalnızca `wait_until_initialized`'ı
/// sarıyordu, oysa bir magnet'te üstveriyi çözen `add_torrent`'ın kendisi.
/// Soğuk bir magnet eklentiyi süresizce bekletiyor, çekirdek 20 sn'de "eklenti
/// takıldı" diyor ve kullanıcı sebebi hiç öğrenemiyordu. Kusuru yalnızca
/// gerçek koşum gösterdi — sahte bir oturum "hemen döndü" derdi.
#[tokio::test(flavor = "multi_thread")]
async fn a_source_with_no_peers_gives_up_within_the_budget_and_says_why() {
    let root = scratch("peersiz");
    let data_dir = root.join("state");
    std::fs::create_dir_all(&data_dir).expect("durum dizini");
    let engine = Engine::new(data_dir).await.expect("motor açılmalı");

    // Hiçbir yerde olmayan bir infohash: katalogda yok, tracker yok, DHT'de yok.
    let nowhere = "f".repeat(40);
    let (source, from_catalog) = engine.source_for(&nowhere).await;
    assert!(!from_catalog, "katalog boş olmalı");

    let started = std::time::Instant::now();
    let outcome = engine.handle(&nowhere, &source).await;
    let elapsed = started.elapsed();
    let Err(error) = outcome else {
        panic!("hiçbir yerde olmayan bir torrent için başarı dönmemeli");
    };

    assert!(
        elapsed < headshell_core::plugin::client::CALL_TIMEOUT,
        "çekirdeğin zaman aşımından önce cevap vermeliydik: {elapsed:?}"
    );
    let text = error.to_string();
    assert!(
        text.contains("peer bulunamamış"),
        "sebep söylenmeli: {text}"
    );
    assert!(
        text.contains("tekrar deneyin"),
        "ne yapılacağı söylenmeli: {text}"
    );
}
