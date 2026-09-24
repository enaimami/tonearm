//! Taşıma katmanı testi: **gerçek** `UreqClient` ↔ **gerçek** soket (D-022).
//!
//! Birim testleri sahte bir `HttpClient` kullanıyor; onlar sağlayıcı
//! mantığını (URL kurma, JSON ayrıştırma, hata çevirme) sınıyor ama
//! `ureq`'in kendisine, gerçek bir sokete ya da HTTP çerçevelemesine hiç
//! dokunmuyor. Buradaki sunucu `std::net` ile elle yazıldı — yeni bağımlılık
//! yok — ve istekleri kaydediyor, böylece **telde ne gittiği** doğrulanıyor.
//!
//! **Bunun sınamadığı şey** (D-022, açıkça kayıtlı): gerçek bir Navidrome ya
//! da Jellyfin kurulumunun tuhaflıkları — yönlendirme, transcode, tarih
//! biçimleri, sürüm farkları. Sahte sunucu protokolün *bizim anladığımız*
//! hâlini kilitler, doğru anladığımızı kanıtlamaz.
//!
//! `http-client` feature'ı olmadan bu dosya boş derlenir.

#![cfg(feature = "http-client")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use headshell_core::ids::{ProviderId, ProviderTrackId};
use headshell_core::net::{HttpClient, UreqClient};
use headshell_core::provider::AudioSource;
use headshell_core::provider::remote::{self, NewServer, RemoteServer, ServerKind, StoredAuth};

// ————————————————————————————————————————————————————————————————
// Sahte sunucu
// ————————————————————————————————————————————————————————————————

/// Sunucunun gördüğü bir istek.
#[derive(Debug, Clone)]
struct Req {
    method: String,
    path: String,
    query: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Req {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// Döndürülecek yanıt.
struct Resp {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

impl Resp {
    fn json(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            content_type: "application/json",
            body: body.into().into_bytes(),
        }
    }

    /// Yalnızca ses gövdesi sunan testler için — onlar da `audio` arkasında.
    #[cfg(feature = "audio")]
    fn bytes(content_type: &'static str, body: Vec<u8>) -> Self {
        Self {
            status: 200,
            content_type,
            body,
        }
    }

    fn status(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            content_type: "text/plain; charset=utf-8",
            body: body.into().into_bytes(),
        }
    }
}

/// Rastgele bir porta bağlanan, isteklerini kaydeden minik HTTP sunucusu.
///
/// Bağlantı başına tek istek (`Connection: close`) — havuzlama ve
/// keep-alive'ı taklit etmeye çalışmıyoruz; sınadığımız şey istemcinin
/// gerçek bir sokete doğru baytları yazıp yanıtı doğru çözmesi.
struct FakeServer {
    addr: SocketAddr,
    seen: Arc<Mutex<Vec<Req>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl FakeServer {
    fn start<H>(handler: H) -> Self
    where
        H: Fn(&Req) -> Resp + Send + Sync + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("dinleyici bağlanmalı");
        let addr = listener.local_addr().expect("adres okunmalı");
        // Bloklamayan accept: durdurma bayrağı görülmeden iş parçacığı
        // sonsuza kadar asılı kalmasın.
        listener
            .set_nonblocking(true)
            .expect("bloklamayan kip ayarlanmalı");

        let seen = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let seen = Arc::clone(&seen);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let _ = stream.set_nonblocking(false);
                            serve_once(stream, &handler, &seen);
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(_) => break,
                    }
                }
            })
        };

        Self {
            addr,
            seen,
            stop,
            thread: Some(thread),
        }
    }

    fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn seen(&self) -> Vec<Req> {
        self.seen.lock().map(|log| log.clone()).unwrap_or_default()
    }

    /// Yolu eşleşen ilk istek.
    fn request_to(&self, path: &str) -> Option<Req> {
        self.seen().into_iter().find(|req| req.path == path)
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve_once<H>(stream: TcpStream, handler: &H, seen: &Mutex<Vec<Req>>)
where
    H: Fn(&Req) -> Resp,
{
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

    let Ok(peer) = stream.try_clone() else { return };
    let mut reader = BufReader::new(peer);
    let Some(request) = read_request(&mut reader) else {
        return;
    };
    if let Ok(mut log) = seen.lock() {
        log.push(request.clone());
    }

    let response = handler(&request);
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        reason(response.status),
        response.content_type,
        response.body.len()
    );
    let mut out = stream;
    let _ = out.write_all(head.as_bytes());
    let _ = out.write_all(&response.body);
    let _ = out.flush();
}

fn read_request(reader: &mut BufReader<TcpStream>) -> Option<Req> {
    let mut line = String::new();
    if reader.read_line(&mut line).ok()? == 0 {
        return None;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_owned();
    let target = parts.next()?.to_owned();

    let mut headers = Vec::new();
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).ok()? == 0 {
            break;
        }
        let trimmed = header.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            headers.push((name.trim().to_owned(), value.trim().to_owned()));
        }
    }

    let length = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    if length > 0 {
        reader.read_exact(&mut body).ok()?;
    }

    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path.to_owned(), query.to_owned()),
        None => (target.clone(), String::new()),
    };
    Some(Req {
        method,
        path,
        query,
        headers,
        body,
    })
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "Error",
    }
}

fn client() -> Arc<dyn HttpClient> {
    Arc::new(UreqClient::new())
}

/// Ses fixture'ının yolu. Çağıranların hepsi `audio` arkasında: feature
/// kapalıyken bu yardımcı ölü kod uyarısı oluyordu.
#[cfg(feature = "audio")]
fn fixture(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/audio"))
        .join(name)
}

// ————————————————————————————————————————————————————————————————
// Subsonic
// ————————————————————————————————————————————————————————————————

const SUBSONIC_PING: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1",
    "type":"navidrome","serverVersion":"0.53.3"}}"#;
const SUBSONIC_SCAN: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1",
    "scanStatus":{"scanning":false,"count":8123}}}"#;
const SUBSONIC_SEARCH: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1",
    "searchResult3":{"song":[
    {"id":"a1","title":"Sine Track","artist":"Test Artist","album":"Fixture",
     "duration":3,"isrc":["TRABC2400001"]},
    {"title":"kimliksiz"}]}}}"#;

fn subsonic_routes(req: &Req) -> Resp {
    match req.path.as_str() {
        "/rest/ping" => Resp::json(SUBSONIC_PING),
        "/rest/getScanStatus" => Resp::json(SUBSONIC_SCAN),
        "/rest/search3" => Resp::json(SUBSONIC_SEARCH),
        _ => Resp::status(404, "bu yol yok"),
    }
}

fn subsonic_spec(url: &str) -> NewServer {
    NewServer {
        id: ProviderId::new("ev"),
        kind: ServerKind::Subsonic,
        url: url.to_owned(),
        username: "enai".to_owned(),
        password: Some("susam".to_owned()),
        api_key: None,
        verify: true,
    }
}

/// Kayıt gerçek bir sokete bağlanır ve **parola tele hiç çıkmaz** (D-021).
#[tokio::test]
async fn registering_a_subsonic_server_verifies_over_a_real_socket_without_sending_the_password() {
    let server = FakeServer::start(subsonic_routes);
    let (stored, notes) = remote::prepare_server(&subsonic_spec(&server.url()), client())
        .await
        .expect("kayıt doğrulanmalı");

    assert_eq!(stored.kind, ServerKind::Subsonic);
    match &stored.auth {
        StoredAuth::SubsonicToken { salt, token } => {
            assert_eq!(token.len(), 32, "md5 onaltılık 32 karakter: {token}");
            assert!(!salt.is_empty());
        }
        other => panic!("Subsonic token bekleniyordu: {other:?}"),
    }
    assert!(
        notes.iter().all(|note| !note.contains("zayıf")),
        "bu makinede /dev/urandom okunabilmeli: {notes:?}"
    );

    // Asıl iddia: telde ne gitti?
    let ping = server.request_to("/rest/ping").expect("ping atılmalı");
    assert_eq!(ping.method, "GET");
    assert!(ping.query.contains("u=enai"), "{}", ping.query);
    assert!(ping.query.contains("&t="), "{}", ping.query);
    assert!(ping.query.contains("&s="), "{}", ping.query);
    assert!(
        !ping.query.contains("p="),
        "parola sorgu dizesine düşmemeli: {}",
        ping.query
    );
    assert!(
        !server
            .seen()
            .iter()
            .any(|req| req.query.contains("susam") || req.body_text().contains("susam")),
        "parola hiçbir istekte geçmemeli"
    );

    // Doğrulama sağlığı da okumuş olmalı.
    assert!(server.request_to("/rest/getScanStatus").is_some());
}

/// Kayıtlı sunucudan gerçek soket üzerinden arama ve akış çözümlemesi.
#[tokio::test]
async fn a_registered_subsonic_server_searches_and_resolves_a_stream_url() {
    let server = FakeServer::start(subsonic_routes);
    let (stored, _) = remote::prepare_server(&subsonic_spec(&server.url()), client())
        .await
        .expect("kayıt");
    let provider = remote::provider_for(&stored, client());

    let hits = provider.search("sine", 10).await.expect("arama");
    assert_eq!(hits.len(), 1, "kimliksiz satır atlanmalı");
    assert_eq!(hits[0].track.title, "Sine Track");
    assert_eq!(hits[0].track.duration_ms, Some(3_000));
    assert_eq!(hits[0].id.id, "a1");

    let source = provider
        .resolve_source(&hits[0].id)
        .await
        .expect("çözümleme")
        .expect("kaynak");
    match source {
        AudioSource::HttpStream { url, headers } => {
            assert!(url.contains("/rest/stream?"), "{url}");
            assert!(url.contains("id=a1"), "{url}");
            assert!(headers.is_empty(), "Subsonic kimliği sorgu dizesinde");
        }
        other => panic!("HTTP akışı bekleniyordu: {other:?}"),
    }
}

/// Subsonic hatayı **HTTP 200 ile** gönderir; taşıma başarılı olsa da komut
/// başarısız olmalı. Bu tuzağı gerçek bir istemciyle de kilitliyoruz.
#[tokio::test]
async fn a_subsonic_failure_arrives_with_http_200_and_still_fails() {
    let server = FakeServer::start(|_| {
        Resp::json(
            r#"{"subsonic-response":{"status":"failed","version":"1.16.1",
                "error":{"code":40,"message":"Wrong username or password."}}}"#,
        )
    });

    let err = remote::prepare_server(&subsonic_spec(&server.url()), client())
        .await
        .expect_err("yanlış parola kaydı geçmemeli");
    let text = err.chain_text();
    assert!(text.contains("doğrulanamadı"), "{text}");
    assert!(text.contains("Wrong username or password"), "{text}");
    assert!(text.contains("40"), "hata kodu görünmeli: {text}");
    // Sunucuya **ulaşıldı**, kimlik reddedildi. Dıştaki cümle "erişilemedi"
    // derse kullanıcıyı ağ hatası aramaya gönderir; gerçek Navidrome'a karşı
    // görülen kusur buydu.
    assert!(
        !text.contains("erişilemedi"),
        "reddedilmek erişilememek değildir: {text}"
    );
    assert!(
        text.contains("PROVIDER_CALL") && !text.contains("NETWORK_REQUEST"),
        "aşama uygulama katmanını göstermeli: {text}"
    );
}

/// Kapalı bir port bir **sağlık cevabıdır**, komutun çökmesi değil — ve
/// sebebini söyler.
#[tokio::test]
async fn a_closed_port_is_a_health_answer_with_a_reason() {
    // Bağla, adresi al, kapat: artık kimsenin dinlemediği bir port.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bağlanmalı");
    let addr = listener.local_addr().expect("adres");
    drop(listener);

    let stored = RemoteServer {
        id: ProviderId::new("kapali"),
        kind: ServerKind::Subsonic,
        url: format!("http://{addr}"),
        username: "enai".to_owned(),
        auth: StoredAuth::SubsonicToken {
            salt: "abc".to_owned(),
            token: "0".repeat(32),
        },
        user_id: None,
    };
    let health = remote::provider_for(&stored, client())
        .health()
        .await
        .expect("sağlık sorgusu hata döndürmemeli");

    assert!(!health.reachable);
    assert_eq!(health.track_count, None, "bilinmiyor sıfır değildir");
    let detail = health.detail.unwrap_or_default();
    assert!(
        detail.contains("NETWORK_REQUEST"),
        "taşıma katmanı aşaması görünmeli: {detail}"
    );
}

// ————————————————————————————————————————————————————————————————
// Jellyfin
// ————————————————————————————————————————————————————————————————

const JELLYFIN_TOKEN: &str = "erisim-anahtari";
const JELLYFIN_USER: &str = "u42";

fn jellyfin_routes(req: &Req) -> Resp {
    let authorized = req
        .header("Authorization")
        .is_some_and(|value| value.contains(&format!("Token=\"{JELLYFIN_TOKEN}\"")));

    if req.path == "/Users/AuthenticateByName" {
        if req.method != "POST" || !req.body_text().contains("susam") {
            return Resp::status(401, "kimlik reddedildi");
        }
        return Resp::json(format!(
            r#"{{"AccessToken":"{JELLYFIN_TOKEN}","User":{{"Id":"{JELLYFIN_USER}","Name":"enai"}}}}"#
        ));
    }
    if req.path == "/System/Info/Public" {
        return Resp::json(r#"{"ServerName":"Ev Jellyfin","Version":"10.9.11"}"#);
    }
    if !authorized {
        return Resp::status(401, "Access token is invalid or expired.");
    }
    if req.path == "/Users/Me" {
        return Resp::json(format!(r#"{{"Id":"{JELLYFIN_USER}","Name":"enai"}}"#));
    }
    if req.path == format!("/Users/{JELLYFIN_USER}/Items") {
        return Resp::json(
            r#"{"Items":[{"Id":"i1","Name":"Sine Track","Album":"Fixture",
                "AlbumArtist":"Test Artist","RunTimeTicks":30000000}],
                "TotalRecordCount":4211}"#,
        );
    }
    Resp::status(404, "bu yol yok")
}

/// Jellyfin'de parola bir kez anahtara çevrilir ve **anahtar saklanır**;
/// sonraki istekler kimliği başlıkta taşır (D-021).
#[tokio::test]
async fn jellyfin_exchanges_the_password_for_a_token_over_a_real_socket() {
    let server = FakeServer::start(jellyfin_routes);
    let spec = NewServer {
        id: ProviderId::new("jf"),
        kind: ServerKind::Jellyfin,
        url: server.url(),
        username: "enai".to_owned(),
        password: Some("susam".to_owned()),
        api_key: None,
        verify: true,
    };

    let (stored, notes) = remote::prepare_server(&spec, client())
        .await
        .expect("kayıt doğrulanmalı");

    match &stored.auth {
        StoredAuth::ApiKey { key } => assert_eq!(key, JELLYFIN_TOKEN),
        other => panic!("erişim anahtarı bekleniyordu: {other:?}"),
    }
    assert_eq!(
        stored.user_id.as_deref(),
        Some(JELLYFIN_USER),
        "kullanıcı kimliği kayıt anında öğrenilmeli"
    );
    assert!(
        notes.iter().any(|note| note.contains("parola saklanmıyor")),
        "kullanıcıya ne saklandığı söylenmeli: {notes:?}"
    );

    // Parola yalnızca giriş isteğinin gövdesinde geçti; kayda girmedi.
    let stored_json = serde_json::to_string(&stored).expect("kayıt seri hâle gelmeli");
    assert!(!stored_json.contains("susam"), "{stored_json}");

    let auth_call = server
        .request_to("/Users/AuthenticateByName")
        .expect("giriş isteği");
    assert_eq!(auth_call.method, "POST");
    assert_eq!(
        auth_call.header("Content-Type"),
        Some("application/json"),
        "gövde JSON olarak ilan edilmeli"
    );

    // Sonraki istekler: anahtar başlıkta, URL'de değil.
    let items = server
        .request_to(&format!("/Users/{JELLYFIN_USER}/Items"))
        .expect("parça sayısı isteği");
    assert!(
        !items.query.contains(JELLYFIN_TOKEN),
        "anahtar URL'ye sızmamalı: {}",
        items.query
    );
    assert!(
        items
            .header("Authorization")
            .is_some_and(|value| value.contains(JELLYFIN_TOKEN)),
        "kimlik başlıkta gitmeli: {:?}",
        items.header("Authorization")
    );
}

/// Ayakta ama anahtarı reddeden sunucu "erişilebilir" değildir — kullanıcının
/// yapacağı iş farklı.
#[tokio::test]
async fn a_jellyfin_server_that_rejects_the_key_is_not_reachable() {
    let server = FakeServer::start(jellyfin_routes);
    let stored = RemoteServer {
        id: ProviderId::new("jf"),
        kind: ServerKind::Jellyfin,
        url: server.url(),
        username: "enai".to_owned(),
        auth: StoredAuth::ApiKey {
            key: "wrong-key".to_owned(),
        },
        user_id: None,
    };

    let health = remote::provider_for(&stored, client())
        .health()
        .await
        .expect("sağlık sorgusu hata döndürmemeli");
    assert!(!health.reachable, "{health:?}");
    let detail = health.detail.unwrap_or_default();
    assert!(
        detail.contains("Ev Jellyfin"),
        "ayakta olduğu görünmeli: {detail}"
    );
    assert!(detail.contains("401"), "reddedildiği görünmeli: {detail}");
}

// ————————————————————————————————————————————————————————————————
// Akış (`audio` + `http-client`)
// ————————————————————————————————————————————————————————————————

/// Uzak akış gerçekten iniyor ve geriye arama yapılabiliyor.
///
/// Ses aygıtı **gerekmiyor**: burada sınanan şey kaynak katmanı, çıkış değil.
/// symphonia kabı tanırken geriye arıyor; `is_seekable` yalnızca sunucu
/// uzunluk bildirdiğinde doğru olduğu için `Content-Length` de dolaylı
/// olarak sınanıyor.
#[cfg(feature = "audio")]
#[test]
fn an_http_stream_downloads_completely_and_seeks_backwards() {
    use headshell_core::playback::http_source::HttpMediaSource;
    use std::io::{Seek, SeekFrom};

    let flac = std::fs::read(fixture("tagged.flac")).expect("fixture okunmalı");
    let served = flac.clone();
    let server = FakeServer::start(move |req| match req.path.as_str() {
        "/ses.flac" => Resp::bytes("audio/flac", served.clone()),
        _ => Resp::status(404, "bu yol yok"),
    });

    let mut source =
        HttpMediaSource::open(&format!("{}/ses.flac", server.url()), &[]).expect("akış açılmalı");

    let mut downloaded = Vec::new();
    source
        .read_to_end(&mut downloaded)
        .expect("baytlar okunmalı");
    assert_eq!(
        downloaded, flac,
        "inen baytlar dosyanın birebir aynısı olmalı"
    );

    source.seek(SeekFrom::Start(0)).expect("başa dönülmeli");
    let mut magic = [0u8; 4];
    source.read_exact(&mut magic).expect("başlık okunmalı");
    assert_eq!(&magic, b"fLaC", "geriye arama aynı baytları vermeli");

    source.seek(SeekFrom::End(-1)).expect("sondan aranmalı");
    let mut tail = [0u8; 1];
    source.read_exact(&mut tail).expect("son bayt okunmalı");
    assert_eq!(tail[0], *flac.last().expect("dosya boş değil"));
}

/// Sunucu 404 dönerse çalmaya **başlamadan** düşülmeli: "çalıyor ama ses yok"
/// durumundan iyidir.
#[cfg(feature = "audio")]
#[test]
fn a_missing_stream_fails_before_playback_starts() {
    use headshell_core::playback::http_source::HttpMediaSource;

    let server = FakeServer::start(|_| Resp::status(404, "böyle bir parça yok"));
    let err = HttpMediaSource::open(&format!("{}/yok.flac", server.url()), &[])
        .expect_err("404 sessizce boş akışa dönmemeli");

    let text = err.chain_text();
    assert!(text.starts_with("ADIM: NETWORK_REQUEST"), "{text}");
    assert!(text.contains("404"), "{text}");
    assert!(
        text.contains("böyle bir parça yok"),
        "sunucunun dediği görünmeli: {text}"
    );
}

/// Faz 1'in bitti ölçütünün uzak yarısı: HTTP üzerinden gelen ses gerçekten
/// çözülüp aygıta veriliyor.
///
/// Ses aygıtı olmayan ortamda kendini atlar — susturmak değil, koşulun
/// sağlanmadığını söyleyip geçmek (`playback_local.rs` ile aynı yordam).
#[cfg(feature = "audio")]
#[test]
fn an_http_stream_really_decodes_and_plays() {
    use cpal::traits::HostTrait;
    use headshell_core::playback::{AudioEngine, PlayState};

    if cpal::default_host().default_output_device().is_none() {
        eprintln!("ses çıkışı yok — uzak çalma testi atlanıyor (bu bir başarısızlık değil)");
        return;
    }

    let flac = std::fs::read(fixture("tagged.flac")).expect("fixture okunmalı");
    let server = FakeServer::start(move |req| match req.path.as_str() {
        // Uzantısız adres kasıtlı: Subsonic `/rest/stream?id=...` veriyor,
        // yani kap içerikten tanınmalı.
        "/rest/stream" => Resp::bytes("audio/flac", flac.clone()),
        _ => Resp::status(404, "bu yol yok"),
    });

    let engine = match AudioEngine::play_http(&format!("{}/rest/stream?id=a1", server.url()), &[]) {
        Ok(engine) => engine,
        Err(err) => {
            // Aygıt var göründü ama açılamadı (kilitli, izin yok…).
            eprintln!(
                "ses aygıtı açılamadı, test atlanıyor:\n{}",
                err.chain_text()
            );
            return;
        }
    };

    // Süre kaptan okundu demek: uzantısız adreste bile kap içerikten tanındı.
    let duration = engine.duration_ms().expect("süre kaptan okunmalı");
    assert!(
        (900..=1100).contains(&duration),
        "1 sn'lik fixture beklenirken {duration}ms"
    );

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while engine.position_ms() == 0 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        engine.position_ms() > 0,
        "uzak akışta tek kare bile çalınmadı (durum: {})",
        engine.state()
    );

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !engine.finished() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        engine.finished(),
        "parça bitmedi (durum: {})",
        engine.state()
    );
    assert_eq!(engine.state(), PlayState::Stopped);
    assert_eq!(engine.take_error(), None, "akış hatasız bitmeliydi");
}

// ————————————————————————————————————————————————————————————————
// Kayıt dosyası ↔ sağlayıcı kaydı
// ————————————————————————————————————————————————————————————————

/// Diske yazılan kayıt geri okunduğunda çalışan bir sağlayıcı veriyor:
/// `headshell provider add` ile `headshell provider test` arasındaki köprü.
#[tokio::test]
async fn a_saved_server_comes_back_as_a_working_provider() {
    let server = FakeServer::start(subsonic_routes);
    let (stored, _) = remote::prepare_server(&subsonic_spec(&server.url()), client())
        .await
        .expect("kayıt");

    let dir = support::TempDir::new("remote");
    let path = dir.join("servers.json");
    remote::save_servers(&path, &[stored]).expect("yazılmalı");

    let loaded = remote::load_servers(&path).expect("okunmalı");
    assert_eq!(loaded.len(), 1);
    let provider = remote::provider_for(&loaded[0], client());
    assert_eq!(provider.info().id, ProviderId::new("ev"));

    let health = provider.health().await.expect("sağlık");
    assert!(health.reachable, "{health:?}");
    assert_eq!(health.track_count, Some(8123));

    // Başka sağlayıcının kimliği sessizce kabul edilmemeli.
    let foreign = ProviderTrackId::new(ProviderId::new("baska"), "a1");
    assert!(provider.resolve_source(&foreign).await.is_err());

    std::fs::remove_dir_all(&dir).ok();
}
