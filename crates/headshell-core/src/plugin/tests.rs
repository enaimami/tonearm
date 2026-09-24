//! Eklenti sınırının birim testleri.
//!
//! İki kısım: keşif ve onay her derlemede sınanıyor (motor gerekmiyor);
//! motorun kendisi `plugin-engine` açıkken **gerçek QuickJS'le** sınanıyor.
//! Betikler test içinde yazılıyor ve ağ sahte istemciden geçiyor — izin
//! denetimi, yönlendirme ve zaman aşımı ağa çıkmadan görülebiliyor.

use std::path::PathBuf;

use super::*;

fn temp_config(name: &str) -> crate::test_support::TestConfig {
    crate::test_support::TestConfig::new(&format!("plugin-{name}"))
}

fn write_plugin(config: &Config, name: &str, manifest_json: &str) -> PathBuf {
    let dir = config.plugins_dir().join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(MANIFEST_FILE), manifest_json).unwrap();
    dir
}

fn approve(config: &Config, name: &str, net: &[&str]) {
    let mut store = ConsentStore::load(&config.plugin_consent_path()).unwrap();
    store.approve(
        name,
        &Permissions {
            net: net.iter().map(|h| (*h).to_owned()).collect(),
        },
        jiff::Timestamp::now(),
    );
    store.save(&config.plugin_consent_path()).unwrap();
}

// --- keşif ----------------------------------------------------------------

#[test]
fn a_missing_plugins_dir_is_an_empty_list_not_an_error() {
    let config = temp_config("bos");
    let (entries, summary) = discover(&config).unwrap();
    assert!(entries.is_empty());
    assert_eq!(summary, PluginSummary::default());
}

#[test]
fn discovery_reports_each_plugins_reason_without_stopping() {
    let config = temp_config("karisik");
    write_plugin(
        &config,
        "iyi",
        r#"{"name":"iyi","display_name":"İyi","api":2,"main":"main.js",
            "permissions":{"net":["a.example"]}}"#,
    );
    write_plugin(
        &config,
        "eski",
        r#"{"name":"eski","display_name":"Eski","api":1,"exec":["python3","./main.py"]}"#,
    );
    write_plugin(&config, "bozuk", "{ bu json değil");

    let (entries, summary) = discover(&config).unwrap();
    assert_eq!(entries.len(), 3, "bozuk eklenti ötekileri gizlememeli");
    assert_eq!(summary.discovered, 3);
    assert_eq!(summary.incompatible, 1);
    assert_eq!(summary.broken, 1);
    assert_eq!(summary.awaiting_approval, 1, "onay bekleyen: iyi");
    assert_eq!(summary.ready, 0);

    let by_name = |name: &str| {
        entries
            .iter()
            .find(|entry| entry.name == name)
            .unwrap()
            .clone()
    };
    let eski = by_name("eski");
    assert_eq!(eski.api, Some(1));
    let problem = eski.problem.unwrap();
    assert!(problem.contains("api 1"), "{problem}");
    assert!(
        problem.contains("QuickJS"),
        "eski eklentiye ne yapılacağını söylemeli: {problem}"
    );
    assert!(by_name("bozuk").problem.is_some());
    assert!(
        !by_name("iyi").is_loadable(),
        "onaysız eklenti yüklenmemeli"
    );
}

#[test]
fn an_approved_plugin_becomes_loadable_and_load_starts_no_engine() {
    let config = temp_config("onayli");
    // `main.js` bilerek yok: yükleme motoru açsaydı bu dosyayı okumaya
    // kalkıp düşerdi.
    write_plugin(
        &config,
        "iyi",
        r#"{"name":"iyi","display_name":"İyi","api":2,"main":"main.js",
            "capabilities":["search"],"permissions":{"net":["a.example"]}}"#,
    );
    approve(&config, "iyi", &["a.example"]);

    let (entries, summary) = discover(&config).unwrap();
    assert_eq!(summary.ready, 1);
    assert!(entries[0].is_loadable());

    let (providers, summary) = load(&config).unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(summary.ready, 1);
    assert_eq!(providers[0].info().id, ProviderId::new("iyi"));
    assert!(
        providers[0]
            .info()
            .capabilities
            .contains(Capabilities::SEARCH)
    );
    assert!(config.plugin_state_dir("iyi").is_dir());
}

#[test]
fn a_plugin_that_grew_its_permissions_is_not_loaded_until_reapproved() {
    let config = temp_config("buyuyen");
    write_plugin(
        &config,
        "iyi",
        r#"{"name":"iyi","display_name":"İyi","api":2,"main":"main.js",
            "permissions":{"net":["a.example","yeni.example"]}}"#,
    );
    approve(&config, "iyi", &["a.example"]);

    let (entries, summary) = discover(&config).unwrap();
    assert_eq!(summary.awaiting_approval, 1);
    assert!(!entries[0].is_loadable());
    assert!(
        entries[0].status_text().contains("yeni.example"),
        "{}",
        entries[0].status_text()
    );
    assert!(load(&config).unwrap().0.is_empty());
}

/// Bu platform için yayını olmayan bir eser: eklenti yüklenmez ve durum
/// satırı **kurulum önermez** — kurulum bunu düzeltmez.
#[test]
fn a_tool_without_an_asset_for_this_platform_blocks_loading_without_a_false_hint() {
    let config = temp_config("platformsuz");
    let other = if artifact::current_platform() == "windows-x86" {
        "linux-x86_64"
    } else {
        "windows-x86"
    };
    write_plugin(
        &config,
        "arac",
        &format!(
            r#"{{"name":"arac","display_name":"Araç","api":2,"main":"main.js",
                "requires":[{{"name":"yt-dlp","version":"1","assets":{{"{other}":
                {{"url":"https://ornek.gecersiz/x","sha256":"{}"}}}}}}]}}"#,
            "a".repeat(64)
        ),
    );
    approve(&config, "arac", &[]);

    let (entries, summary) = discover(&config).unwrap();
    assert_eq!(summary.needs_install, 1);
    assert!(!entries[0].is_loadable());
    let text = entries[0].status_text();
    assert!(text.contains("bu platformda yok"), "{text}");
    assert!(!text.contains("plugin install"), "yanlış tavsiye: {text}");
}

#[test]
fn a_plugin_only_sees_its_own_secrets() {
    let config = temp_config("sirlar");
    let dir = write_plugin(
        &config,
        "iyi",
        r#"{"name":"iyi","display_name":"İyi","api":2,"main":"main.js","capabilities":["search"]}"#,
    );
    let mut secrets = Secrets::default();
    secrets.set("plugin:iyi", "client_id", "benim");
    secrets.set("plugin:baska", "client_id", "onun");

    let manifest = PluginManifest::load(&dir).unwrap();
    let provider = PluginProvider::from_manifest(&config, &manifest, &dir, &secrets).unwrap();
    assert_eq!(provider.spec.secrets.len(), 1);
    assert_eq!(
        provider.spec.secrets.get("client_id"),
        Some(&"benim".to_owned())
    );
}

fn offline_provider(config: &Config, name: &str, capabilities: &str) -> PluginProvider {
    let dir = write_plugin(
        config,
        name,
        &format!(
            r#"{{"name":"{name}","display_name":"X","api":2,"main":"main.js",
                "capabilities":{capabilities}}}"#
        ),
    );
    let manifest = PluginManifest::load(&dir).unwrap();
    PluginProvider::with_http(
        config,
        &manifest,
        &dir,
        &Secrets::default(),
        Err("sınama".to_owned()),
    )
    .unwrap()
}

#[tokio::test]
async fn a_capability_the_plugin_lacks_is_refused_before_the_engine_starts() {
    let config = temp_config("yeteneksiz");
    // `main.js` yok: motor açılsaydı "okunamadı" derdi, "desteklemiyor" değil.
    let provider = offline_provider(&config, "demo", r#"["search"]"#);
    let id = ProviderTrackId::new(ProviderId::new("demo"), "1");
    let err = provider.resolve_source(&id).await.unwrap_err();
    assert!(
        matches!(err.kind(), ErrorKind::Unsupported { .. }),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_track_id_from_another_provider_is_refused() {
    let config = temp_config("baskasi");
    let provider = offline_provider(&config, "demo", r#"["stream"]"#);
    let id = ProviderTrackId::new(ProviderId::new("baska"), "1");
    let err = provider.resolve_source(&id).await.unwrap_err();
    assert!(
        matches!(err.kind(), ErrorKind::InvalidInput { .. }),
        "{err:?}"
    );
}

#[test]
fn block_on_waits_for_a_future_that_is_not_ready_on_the_first_poll() {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    /// İlk yoklamada hazır değil, uyandırıcıyı başka bir iş parçacığından
    /// çağırıyor — meşgul bekleyen bir `block_on` burada da geçerdi ama
    /// uyandırıcıyı yok sayan bir `block_on` sonsuza kadar uyurdu.
    struct Later(bool);
    impl Future for Later {
        type Output = u8;
        fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<u8> {
            if self.0 {
                return Poll::Ready(7);
            }
            self.0 = true;
            let waker = context.waker().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(20));
                waker.wake();
            });
            Poll::Pending
        }
    }
    assert_eq!(block_on(Later(false)), 7);
}

// --- motor ----------------------------------------------------------------

#[cfg(feature = "plugin-engine")]
mod engine {
    use super::*;
    use crate::net::fake::FakeHttp;

    const CALL: Duration = Duration::from_secs(5);

    /// Betiği ve manifesti yazar, sağlayıcıyı sahte ağla kurar.
    struct Rig {
        config: crate::test_support::TestConfig,
        dir: PathBuf,
    }

    impl Rig {
        fn new(name: &str, manifest: &str, script: &str) -> Self {
            let config = temp_config(name);
            let dir = write_plugin(&config, "demo", manifest);
            std::fs::write(dir.join("main.js"), script).unwrap();
            Self { config, dir }
        }

        fn simple(name: &str, script: &str) -> Self {
            Self::new(
                name,
                r#"{"name":"demo","display_name":"Demo","api":2,"main":"main.js",
                    "capabilities":["search","stream"],
                    "permissions":{"net":["izinli.ornek","*.cdn.ornek"]}}"#,
                script,
            )
        }

        fn provider_with(&self, http: Arc<FakeHttp>, secrets: &Secrets) -> PluginProvider {
            let manifest = PluginManifest::load(&self.dir).unwrap();
            let http: Arc<dyn HttpClient> = http;
            PluginProvider::with_http(&self.config, &manifest, &self.dir, secrets, Ok(http))
                .unwrap()
                .with_timeouts(Duration::from_secs(2), CALL)
        }

        fn provider(&self) -> PluginProvider {
            self.provider_with(Arc::new(FakeHttp::new()), &Secrets::default())
        }
    }

    fn track_id(id: &str) -> ProviderTrackId {
        ProviderTrackId::new(ProviderId::new("demo"), id)
    }

    const BASIC: &str = r#"
        let calls = 0;
        export function health() {
            calls += 1;
            return { reachable: true, detail: `çağrı ${calls}` };
        }
        export function search(query, limit) {
            return [
                { id: "42", artist: "Ezhel", title: query, duration_ms: 1000, isrc: "TRA111700001" },
                { id: "43", artist: "B", title: "C", isrc: "uydurma" },
            ].slice(0, limit);
        }
        export function resolve_source(id) {
            if (id === "yok") return null;
            return { kind: "http_stream", url: `https://a.cdn.ornek/${id}.mp3`, headers: [] };
        }
    "#;

    #[tokio::test]
    async fn a_script_answers_health_search_and_resolve() {
        let rig = Rig::simple("mutlu", BASIC);
        let provider = rig.provider();

        let health = provider.health().await.unwrap();
        assert!(health.reachable, "{:?}", health.detail);
        assert_eq!(health.detail.as_deref(), Some("çağrı 1"));

        let tracks = provider.search("Geceler", 10).await.unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].id.provider, ProviderId::new("demo"));
        assert_eq!(tracks[0].track.title, "Geceler");
        assert!(tracks[0].track.isrc.is_some());
        assert!(tracks[1].track.isrc.is_none(), "biçimsiz ISRC düşmeli");

        match provider.resolve_source(&track_id("7")).await.unwrap() {
            Some(AudioSource::HttpStream { url, .. }) => {
                assert_eq!(url, "https://a.cdn.ornek/7.mp3");
            }
            other => panic!("beklenmeyen kaynak: {other:?}"),
        }
        assert!(
            provider
                .resolve_source(&track_id("yok"))
                .await
                .unwrap()
                .is_none(),
            "`null` bir cevaptır"
        );
    }

    #[tokio::test]
    async fn a_throw_is_a_rejection_with_a_location_and_does_not_restart_the_engine() {
        let rig = Rig::simple(
            "firlatan",
            r#"
            let calls = 0;
            export function health() { calls += 1; return { reachable: true, detail: String(calls) }; }
            export function search() {
                throw new Error("kota doldu");
            }
            export function resolve_source() { return null; }
            "#,
        );
        let provider = rig.provider();
        assert_eq!(
            provider.health().await.unwrap().detail.as_deref(),
            Some("1")
        );

        let err = provider.search("x", 1).await.unwrap_err();
        match err.kind() {
            ErrorKind::PluginThrew {
                message, location, ..
            } => {
                assert_eq!(message, "kota doldu");
                assert!(location.contains("main.js:"), "konum yok: {location}");
            }
            other => panic!("beklenmeyen hata: {other:?}"),
        }
        assert_eq!(err.stage(), Stage::ProviderCall);

        // Aynı motor: sayaç kaldığı yerden devam ediyor.
        assert_eq!(
            provider.health().await.unwrap().detail.as_deref(),
            Some("2")
        );
    }

    #[tokio::test]
    async fn an_async_export_is_awaited() {
        let rig = Rig::simple(
            "async",
            r#"
            export async function health() { await null; return { reachable: true }; }
            export async function search(q) { const x = await Promise.resolve(q); return [{ id: "1", artist: "a", title: x }]; }
            export async function resolve_source() { throw new Error("async ret"); }
            "#,
        );
        let provider = rig.provider();
        assert!(provider.health().await.unwrap().reachable);
        assert_eq!(
            provider.search("söz", 5).await.unwrap()[0].track.title,
            "söz"
        );
        let err = provider.resolve_source(&track_id("1")).await.unwrap_err();
        assert!(
            err.chain_text().contains("async ret"),
            "{}",
            err.chain_text()
        );
    }

    #[tokio::test]
    async fn a_hanging_call_times_out_and_the_next_call_gets_a_fresh_engine() {
        let rig = Rig::simple(
            "asili",
            r#"
            let calls = 0;
            export function health() { calls += 1; return { reachable: true, detail: String(calls) }; }
            export function search() { for (;;) {} }
            export function resolve_source() { try { for (;;) {} } catch (_) { return null; } }
            "#,
        );
        let provider = rig
            .provider()
            .with_timeouts(Duration::from_secs(2), Duration::from_millis(300));
        assert_eq!(
            provider.health().await.unwrap().detail.as_deref(),
            Some("1")
        );

        let started = std::time::Instant::now();
        let err = provider.search("x", 1).await.unwrap_err();
        assert!(
            matches!(err.kind(), ErrorKind::PluginTimeout { method, .. } if method == "search"),
            "{err:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );

        // `try/catch` kesmeyi yakalayamaz.
        let err = provider.resolve_source(&track_id("1")).await.unwrap_err();
        assert!(
            matches!(err.kind(), ErrorKind::PluginTimeout { .. }),
            "{err:?}"
        );

        // Yeni motor: sayaç sıfırdan.
        assert_eq!(
            provider.health().await.unwrap().detail.as_deref(),
            Some("1")
        );
    }

    #[tokio::test]
    async fn a_hang_while_loading_times_out_at_the_start_stage() {
        let rig = Rig::simple(
            "yuklemede-asili",
            "for (;;) {}\nexport function health() {}",
        );
        let provider = rig
            .provider()
            .with_timeouts(Duration::from_millis(300), CALL);
        let started = std::time::Instant::now();
        let err = provider.search("x", 1).await.unwrap_err();
        assert!(
            matches!(err.kind(), ErrorKind::PluginTimeout { method, .. } if method == "yükleme"),
            "{err:?}"
        );
        assert_eq!(err.stage(), Stage::PluginStart);
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[tokio::test]
    async fn a_missing_export_is_a_contract_breach_and_is_not_retried() {
        let rig = Rig::simple(
            "eksik",
            "export function health() { return { reachable: true }; }\n\
             export function search() { return []; }",
        );
        let provider = rig.provider();
        let err = provider.search("x", 1).await.unwrap_err();
        match err.kind() {
            ErrorKind::PluginContract { detail, .. } => {
                assert!(detail.contains("resolve_source"), "{detail}");
            }
            other => panic!("beklenmeyen hata: {other:?}"),
        }
        assert_eq!(err.stage(), Stage::PluginStart);

        // Tekrar denenmez: sözleşme ihlali yeniden başlatmayla düzelmez.
        let again = provider.search("x", 1).await.unwrap_err();
        assert!(matches!(again.kind(), ErrorKind::PluginCrashed { .. }));
        assert!(provider.state.lock().unwrap().starts == 1);
    }

    #[tokio::test]
    async fn a_script_that_keeps_failing_to_load_is_given_up_after_max_starts() {
        let rig = Rig::simple("hep-dusen", "throw new Error('açılışta patladı');");
        let provider = rig.provider();
        for _ in 0..MAX_STARTS {
            let err = provider.search("x", 1).await.unwrap_err();
            assert!(
                matches!(err.kind(), ErrorKind::PluginThrew { method, .. } if method == "yükleme"),
                "{err:?}"
            );
        }
        let err = provider.search("x", 1).await.unwrap_err();
        assert!(
            err.chain_text().contains("vazgeçildi"),
            "{}",
            err.chain_text()
        );
    }

    #[tokio::test]
    async fn a_wrong_return_shape_is_a_contract_breach_not_an_empty_answer() {
        // api 1'in `{ tracks: [...] }` biçimi: api 2 dizi bekliyor.
        let rig = Rig::simple(
            "bicim",
            r#"
            export function health() { return { reachable: true }; }
            export function search() { return { tracks: [] }; }
            export function resolve_source() { return { kind: "teleport" }; }
            "#,
        );
        let provider = rig.provider();
        let err = provider.search("x", 1).await.unwrap_err();
        assert!(
            matches!(err.kind(), ErrorKind::PluginContract { .. }),
            "{err:?}"
        );
        let err = provider.resolve_source(&track_id("1")).await.unwrap_err();
        assert!(
            matches!(err.kind(), ErrorKind::PluginContract { .. }),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn running_out_of_memory_is_an_error_not_a_crash() {
        let rig = Rig::simple(
            "bellek",
            r#"
            export function health() { return { reachable: true }; }
            export function search() { const all = []; for (;;) all.push(new Array(100000).fill(1)); }
            export function resolve_source() { return null; }
            "#,
        );
        let provider = rig.provider();
        let err = provider.search("x", 1).await.unwrap_err();
        assert!(
            err.chain_text().contains("out of memory"),
            "{}",
            err.chain_text()
        );
        assert!(
            provider.health().await.unwrap().reachable,
            "motor ayakta kalmalı"
        );
    }

    #[tokio::test]
    async fn the_network_is_refused_outside_the_declared_hosts() {
        let rig = Rig::simple(
            "izinsiz",
            r#"
            export function health() { return { reachable: true }; }
            export function search() { host.http.get("https://kotu.ornek/sızdır"); return []; }
            export function resolve_source() { return null; }
            "#,
        );
        let http = Arc::new(FakeHttp::new().route("kotu.ornek", "{}"));
        let provider = rig.provider_with(http.clone(), &Secrets::default());
        let err = provider.search("x", 1).await.unwrap_err();
        assert!(
            err.chain_text().contains("izin yok"),
            "{}",
            err.chain_text()
        );
        assert!(
            err.chain_text().contains("kotu.ornek"),
            "{}",
            err.chain_text()
        );
        assert!(http.requests().is_empty(), "izinsiz istek ağa çıktı");
    }

    #[tokio::test]
    async fn a_declared_host_is_reached_and_the_response_comes_back_whole() {
        let rig = Rig::simple(
            "izinli",
            r#"
            export function health() { return { reachable: true }; }
            export function search(q) {
                const res = host.http.post("https://izinli.ornek/ara", JSON.stringify({ q }),
                    { "Content-Type": "application/json" });
                const body = JSON.parse(res.body);
                return [{ id: String(res.status), artist: body.artist, title: res.url }];
            }
            export function resolve_source() { return null; }
            "#,
        );
        let http = Arc::new(FakeHttp::new().route("izinli.ornek/ara", r#"{"artist":"Ezhel"}"#));
        let provider = rig.provider_with(http.clone(), &Secrets::default());
        let tracks = provider.search("geceler", 1).await.unwrap();
        assert_eq!(tracks[0].id.id, "200");
        assert_eq!(tracks[0].track.artist, "Ezhel");
        let seen = http.requests();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].method, crate::net::HttpMethod::Post);
        assert_eq!(
            seen[0].body.as_deref(),
            Some(br#"{"q":"geceler"}"#.as_slice())
        );
    }

    /// İzinli bir adres izinsiz bir adrese yönlendirirse motor **ikinci
    /// adımda** durur: istemci yönlendirmeyi kendisi izleseydi bu görünmezdi.
    #[tokio::test]
    async fn a_redirect_to_an_undeclared_host_is_refused_at_the_hop() {
        let rig = Rig::simple(
            "yonlendirme",
            r#"
            export function health() { return { reachable: true }; }
            export function search(q) {
                const res = host.http.get(`https://izinli.ornek/${q}`);
                return [{ id: "1", artist: "a", title: res.body }];
            }
            export function resolve_source() { return null; }
            "#,
        );
        let http = Arc::new(
            FakeHttp::new()
                .route_redirect("izinli.ornek/kacis", 302, "https://kotu.ornek/x")
                .route_redirect("izinli.ornek/ic", 301, "/son")
                .route("izinli.ornek/son", "vardık")
                .route("kotu.ornek", "sızdı"),
        );
        let provider = rig.provider_with(http.clone(), &Secrets::default());

        let err = provider.search("kacis", 1).await.unwrap_err();
        assert!(
            err.chain_text().contains("kotu.ornek"),
            "{}",
            err.chain_text()
        );
        assert_eq!(http.requests().len(), 1, "izinsiz adrese istek gitti");

        let tracks = provider.search("ic", 1).await.unwrap();
        assert_eq!(tracks[0].track.title, "vardık");
    }

    #[tokio::test]
    async fn the_network_is_refused_while_the_module_loads() {
        let rig = Rig::simple(
            "yuklemede-ag",
            r#"
            host.http.get("https://izinli.ornek/");
            export function health() { return { reachable: true }; }
            "#,
        );
        let http = Arc::new(FakeHttp::new().route("izinli.ornek", ""));
        let provider = rig.provider_with(http.clone(), &Secrets::default());
        let err = provider.health().await.unwrap();
        let detail = err.detail.unwrap_or_default();
        assert!(!err.reachable);
        assert!(detail.contains("modül yüklenirken"), "{detail}");
        assert!(http.requests().is_empty());
    }

    #[tokio::test]
    async fn a_stream_the_plugin_may_not_reach_is_refused() {
        let rig = Rig::simple(
            "akis",
            r#"
            export function health() { return { reachable: true }; }
            export function search() { return []; }
            export function resolve_source(id) {
                if (id === "yerel") return { kind: "local_file", path: "/etc/passwd" };
                return { kind: "http_stream", url: "http://192.168.1.1/admin", headers: [] };
            }
            "#,
        );
        let provider = rig.provider();
        let err = provider
            .resolve_source(&track_id("uzak"))
            .await
            .unwrap_err();
        assert!(
            matches!(err.kind(), ErrorKind::PluginContract { .. }),
            "{err:?}"
        );
        assert!(
            err.chain_text().contains("192.168.1.1"),
            "{}",
            err.chain_text()
        );

        let err = provider
            .resolve_source(&track_id("yerel"))
            .await
            .unwrap_err();
        assert!(
            err.chain_text().contains("yerel dosya"),
            "{}",
            err.chain_text()
        );
    }

    #[tokio::test]
    async fn secrets_are_scoped_and_a_secret_file_is_private_and_removed() {
        let rig = Rig::simple(
            "sir",
            r#"
            export function health() {
                return { reachable: true, detail: `${host.secrets.get("token")}|${host.secrets.get("yok")}` };
            }
            export function search() { return []; }
            export function resolve_source() {
                return { kind: "http_stream", url: "https://a.cdn.ornek/x",
                         headers: [{ name: "X-Yol", value: host.secrets.file("cookies") }] };
            }
            "#,
        );
        let mut secrets = Secrets::default();
        secrets.set("plugin:demo", "token", "benim");
        secrets.set("plugin:demo", "cookies", "# Netscape HTTP Cookie File\n");
        secrets.set("plugin:baska", "token", "onun");
        let provider = rig.provider_with(Arc::new(FakeHttp::new()), &secrets);

        assert_eq!(
            provider.health().await.unwrap().detail.as_deref(),
            Some("benim|null")
        );

        let path = match provider.resolve_source(&track_id("1")).await.unwrap() {
            Some(AudioSource::HttpStream { headers, .. }) => PathBuf::from(&headers[0].value),
            other => panic!("beklenmeyen kaynak: {other:?}"),
        };
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# Netscape HTTP Cookie File\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "sır dosyası başkalarına açık");
        }

        drop(provider);
        assert!(!path.exists(), "motor kapandı ama sır dosyası kaldı");
    }

    #[tokio::test]
    async fn storage_survives_a_restart_and_a_corrupt_store_is_reported_not_reset() {
        let script = r#"
            export function health() {
                const seen = host.storage.get("sayac");
                host.storage.set("sayac", String(Number(seen ?? "0") + 1));
                return { reachable: true, detail: seen ?? "ilk" };
            }
            export function search() { host.storage.remove("sayac"); return []; }
            export function resolve_source() { return null; }
        "#;
        let rig = Rig::simple("depo", script);
        assert_eq!(
            rig.provider().health().await.unwrap().detail.as_deref(),
            Some("ilk")
        );
        assert_eq!(
            rig.provider().health().await.unwrap().detail.as_deref(),
            Some("1")
        );

        let provider = rig.provider();
        provider.search("x", 1).await.unwrap();
        assert_eq!(
            provider.health().await.unwrap().detail.as_deref(),
            Some("ilk")
        );

        std::fs::write(
            rig.config.plugin_state_dir("demo").join("storage.json"),
            "{bozuk",
        )
        .unwrap();
        let health = rig.provider().health().await.unwrap();
        assert!(!health.reachable);
        assert!(
            health
                .detail
                .as_deref()
                .unwrap_or_default()
                .contains("depo bozuk"),
            "{:?}",
            health.detail
        );
    }

    #[tokio::test]
    async fn console_goes_to_the_log_and_does_not_break_the_call() {
        let rig = Rig::simple(
            "konsol",
            r#"
            console.log("yükleniyor", { a: 1 }, undefined);
            export function health() { console.warn(new Error("uyarı")); return { reachable: true }; }
            export function search() { return []; }
            export function resolve_source() { return null; }
            "#,
        );
        assert!(rig.provider().health().await.unwrap().reachable);
    }

    #[tokio::test]
    async fn the_host_object_cannot_be_replaced_by_the_plugin() {
        let rig = Rig::simple(
            "donuk",
            r#"
            "use strict";
            export function health() {
                try { host.http = null; return { reachable: false, detail: "değişti" }; }
                catch (e) { return { reachable: true, detail: host.platform }; }
            }
            export function search() { return []; }
            export function resolve_source() { return null; }
            "#,
        );
        let health = rig.provider().health().await.unwrap();
        assert!(health.reachable, "{:?}", health.detail);
        assert_eq!(health.detail, Some(artifact::current_platform()));
    }

    #[cfg(unix)]
    mod tools {
        use super::*;
        use crate::plugin::artifact::sha256_hex;

        const TOOL: &str = "#!/bin/sh\necho \"arg:$1\"\necho \"hata\" >&2\nexit 3\n";
        const SLOW: &str = "#!/bin/sh\nsleep 5\n";

        fn manifest(tool_sha: &str, slow_sha: &str) -> String {
            let platform = artifact::current_platform();
            format!(
                r#"{{"name":"demo","display_name":"Demo","api":2,"main":"main.js",
                    "capabilities":["search"],
                    "requires":[
                      {{"name":"arac","version":"1","assets":{{"{platform}":
                        {{"url":"https://ornek.gecersiz/arac","sha256":"{tool_sha}"}}}}}},
                      {{"name":"yavas","version":"1","assets":{{"{platform}":
                        {{"url":"https://ornek.gecersiz/yavas","sha256":"{slow_sha}"}}}}}},
                      {{"name":"kurulmamis","version":"1","assets":{{"{platform}":
                        {{"url":"https://ornek.gecersiz/k","sha256":"{}"}}}}}}
                    ]}}"#,
                "c".repeat(64)
            )
        }

        fn install(rig: &Rig, name: &str, body: &str) {
            use std::os::unix::fs::PermissionsExt as _;
            let manifest = PluginManifest::load(&rig.dir).unwrap();
            let requirement = manifest
                .requires
                .iter()
                .find(|r| r.name == name)
                .unwrap()
                .clone();
            let store = ArtifactStore::new(&rig.config);
            std::fs::create_dir_all(store.runtime_dir()).unwrap();
            let path = store.artifact_path(&requirement);
            std::fs::write(&path, body).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        const SCRIPT: &str = r#"
            export function health() { return { reachable: true }; }
            export function search(q) {
                const r = host.tools.run(q, ["bir", "iki"], { timeoutMs: 400 });
                return [{ id: String(r.code), artist: r.stdout.trim(), title: r.stderr.trim() }];
            }
        "#;

        #[tokio::test]
        async fn a_declared_installed_tool_runs_and_its_output_comes_back() {
            let rig = Rig::new(
                "arac",
                &manifest(&sha256_hex(TOOL.as_bytes()), &sha256_hex(SLOW.as_bytes())),
                SCRIPT,
            );
            install(&rig, "arac", TOOL);
            let tracks = rig.provider().search("arac", 1).await.unwrap();
            assert_eq!(tracks[0].id.id, "3", "çıkış kodu");
            assert_eq!(tracks[0].track.artist, "arg:bir");
            assert_eq!(tracks[0].track.title, "hata");
        }

        #[tokio::test]
        async fn only_declared_and_installed_tools_run_and_the_reason_is_said() {
            let rig = Rig::new(
                "arac-yok",
                &manifest(&sha256_hex(TOOL.as_bytes()), &sha256_hex(SLOW.as_bytes())),
                SCRIPT,
            );
            let provider = rig.provider();

            let err = provider.search("/bin/sh", 1).await.unwrap_err();
            assert!(
                err.chain_text().contains("beyan edilmiş"),
                "{}",
                err.chain_text()
            );

            let err = provider.search("kurulmamis", 1).await.unwrap_err();
            assert!(
                err.chain_text().contains("headshell plugin install demo"),
                "{}",
                err.chain_text()
            );

            // Karması tutmayan bir dosya çalıştırılmaz.
            install(&rig, "arac", "#!/bin/sh\necho kurcalandi\n");
            let err = provider.search("arac", 1).await.unwrap_err();
            assert!(
                err.chain_text().contains("karma tutmuyor"),
                "{}",
                err.chain_text()
            );
        }

        #[tokio::test]
        async fn a_tool_that_overruns_its_budget_is_stopped() {
            let rig = Rig::new(
                "yavas",
                &manifest(&sha256_hex(TOOL.as_bytes()), &sha256_hex(SLOW.as_bytes())),
                SCRIPT,
            );
            install(&rig, "yavas", SLOW);
            let started = std::time::Instant::now();
            let err = rig.provider().search("yavas", 1).await.unwrap_err();
            assert!(err.chain_text().contains("bitmedi"), "{}", err.chain_text());
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "{:?}",
                started.elapsed()
            );
        }
    }

    /// Unix'teki araç testlerinin Windows karşılığı (D-070).
    ///
    /// Windows'ta kabuk betiği yok; "araç" olarak sistemin kendi `cmd.exe`'sinin
    /// bir kopyası kuruluyor, karması koşum anında hesaplanıyor. Motorun
    /// sınadığı şey aynı: yalnızca beyan edilmiş, kurulu ve karması tutan bir
    /// dosya çalışır; çıktı, çıkış kodu ve süre sınırı geri gelir.
    #[cfg(windows)]
    mod tools_windows {
        use super::*;
        use crate::plugin::artifact::sha256_hex;

        fn system_cmd() -> Vec<u8> {
            let root = std::env::var_os("SystemRoot").expect("SystemRoot tanımlı olmalı");
            std::fs::read(PathBuf::from(root).join("System32").join("cmd.exe"))
                .expect("cmd.exe okunmalı")
        }

        /// `/D`: kayıt defterindeki AutoRun komutları koşmasın — test
        /// makinenin ayarına bağlı kalmasın.
        const SCRIPT: &str = r#"
            export function health() { return { reachable: true }; }
            export function search(q) {
                const [tool, command] = q.split("|");
                const r = host.tools.run(tool, ["/D", "/C", command], { timeoutMs: 1500 });
                return [{ id: String(r.code), artist: r.stdout.trim(), title: r.stderr.trim() }];
            }
        "#;

        fn rig(name: &str) -> Rig {
            let body = system_cmd();
            let platform = artifact::current_platform();
            let rig = Rig::new(
                name,
                &format!(
                    r#"{{"name":"demo","display_name":"Demo","api":2,"main":"main.js",
                        "capabilities":["search"],
                        "requires":[
                          {{"name":"arac","version":"1","assets":{{"{platform}":
                            {{"url":"https://ornek.gecersiz/arac","sha256":"{}"}}}}}},
                          {{"name":"kurulmamis","version":"1","assets":{{"{platform}":
                            {{"url":"https://ornek.gecersiz/k","sha256":"{}"}}}}}}
                        ]}}"#,
                    sha256_hex(&body),
                    "c".repeat(64)
                ),
                SCRIPT,
            );
            install(&rig, "arac", &body);
            rig
        }

        fn install(rig: &Rig, name: &str, body: &[u8]) {
            let manifest = PluginManifest::load(&rig.dir).unwrap();
            let requirement = manifest
                .requires
                .iter()
                .find(|r| r.name == name)
                .unwrap()
                .clone();
            let store = ArtifactStore::new(&rig.config);
            std::fs::create_dir_all(store.runtime_dir()).unwrap();
            std::fs::write(store.artifact_path(&requirement), body).unwrap();
        }

        #[tokio::test]
        async fn a_declared_installed_tool_runs_and_its_output_comes_back() {
            let rig = rig("arac-win");
            let tracks = rig
                .provider()
                .search("arac|echo arg:bir& echo hata 1>&2& exit /b 3", 1)
                .await
                .unwrap();
            assert_eq!(tracks[0].id.id, "3", "çıkış kodu");
            assert_eq!(tracks[0].track.artist, "arg:bir");
            assert_eq!(tracks[0].track.title, "hata");
        }

        #[tokio::test]
        async fn only_declared_and_installed_tools_run_and_the_reason_is_said() {
            let rig = rig("arac-yok-win");
            let provider = rig.provider();

            let err = provider
                .search(r"C:\Windows\System32\cmd.exe|echo x", 1)
                .await
                .unwrap_err();
            assert!(
                err.chain_text().contains("beyan edilmiş"),
                "{}",
                err.chain_text()
            );

            let err = provider.search("kurulmamis|echo x", 1).await.unwrap_err();
            assert!(
                err.chain_text().contains("headshell plugin install demo"),
                "{}",
                err.chain_text()
            );

            // Karması tutmayan bir dosya çalıştırılmaz.
            install(&rig, "arac", b"MZ kurcalandi");
            let fresh = rig.provider();
            let err = fresh.search("arac|echo x", 1).await.unwrap_err();
            assert!(
                err.chain_text().contains("karma tutmuyor"),
                "{}",
                err.chain_text()
            );
        }

        #[tokio::test]
        async fn a_tool_that_overruns_its_budget_is_stopped() {
            let rig = rig("yavas-win");
            let started = std::time::Instant::now();
            // ~5 sn bekleyen bir komut; süre sınırı 1,5 sn.
            let err = rig
                .provider()
                .search("arac|ping -n 6 127.0.0.1 >nul", 1)
                .await
                .unwrap_err();
            assert!(err.chain_text().contains("bitmedi"), "{}", err.chain_text());
            assert!(
                started.elapsed() < Duration::from_secs(4),
                "{:?}",
                started.elapsed()
            );
        }
    }
}
