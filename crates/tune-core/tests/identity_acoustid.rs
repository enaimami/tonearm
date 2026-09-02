//! Zincirin 4. halkası, **gerçek AcoustID'ye karşı** (Faz 2 §2.3 / D-046).
//!
//! Sahte istemciyle sınanan şey mantık: gövde nasıl kuruluyor, yanıt nasıl
//! çözülüyor, hangi hata hangi aşamaya yazılıyor. Burada sınanan şey başka —
//! ürettiğimiz parmak izi dizesini **AcoustID'nin gerçekten kabul ettiği**.
//! Bunu hiçbir sahte gösteremez: kodlama ya da sıkıştırma bir bit kaysa sahte
//! istemci yine "eşleşme yok" derdi, gerçek servis ise "geçersiz parmak izi".
//!
//! **Üç başarısızlık ayrı tutuluyor** (K9 / D-043):
//! - **Anahtar yok** → atlanır, sebebi yazılır. Yapılandırma eksiği, kusur değil.
//! - **Ulaşamamak** → atlanır, sebebi yazılır.
//! - **Ulaşıp beklenmeyeni almak** → düşer.
//!
//! Anahtarı olan bir kurulumda koşturmak için:
//! `tune secret set identity:acoustid api_key <anahtar>` — ya da doğrudan
//! `TUNE_ACOUSTID_KEY` ortam değişkeni.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg(all(feature = "fingerprint", feature = "http-client"))]

use std::path::PathBuf;
use std::sync::Arc;

use tune_core::identity::FingerprintLookup;
use tune_core::identity::acoustid::AcoustIdLookup;
use tune_core::identity::fingerprint::fingerprint_file;

/// Sentetik fixture — üretimi `fixtures/audio/README.md`'de yazılı.
///
/// AcoustID veritabanında karşılığı **yok ve olmamalı**: burada sınanan şey
/// bilinen bir parçayı tanımak değil, "eşleşme bulunamadı" ile "soru
/// sorulamadı" arasındaki farkın doğru raporlandığı.
fn sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/audio/fingerprint_sample.flac")
}

/// api.acoustid.org'a TCP ile ulaşılabiliyor mu.
fn acoustid_reachable() -> bool {
    use std::net::ToSocketAddrs;
    let Ok(mut addrs) = ("api.acoustid.org", 443).to_socket_addrs() else {
        return false;
    };
    addrs.any(|addr| {
        std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(5)).is_ok()
    })
}

/// Testin koşulup koşulamayacağını söyler; koşulamıyorsa **sebebini yazar**.
fn lookup_or_skip(test: &str) -> Option<AcoustIdLookup> {
    let key = std::env::var("TUNE_ACOUSTID_KEY").unwrap_or_default();
    let http = tune_core::net::default_http_client().ok()?;
    let lookup = AcoustIdLookup::new(http)
        .with_api_key(key)
        .with_user_agent("tune-tests/0.0.1 ( https://github.com/kullanici-adi/tune )");

    // Anahtarın varlığı `Debug` çıktısından okunuyor: değer hiçbir yerde
    // yazılmıyor (D-042).
    if !format!("{lookup:?}").contains("api_key_set: true") {
        eprintln!(
            "{test}: AcoustID anahtarı yok — atlanıyor (bu bir başarısızlık değil; \
             `TUNE_ACOUSTID_KEY` ya da `tune secret set identity:acoustid api_key`)"
        );
        return None;
    }
    if !acoustid_reachable() {
        eprintln!("{test}: api.acoustid.org:443'e ulaşılamadı — atlanıyor (ağ yok sayılıyor)");
        return None;
    }
    Some(lookup)
}

/// Ürettiğimiz parmak izini AcoustID ayrıştırabiliyor mu.
///
/// Asıl iddia bu: servis "geçersiz parmak izi" demiyorsa kodlama, sıkıştırma
/// ve base64 alfabesi doğru. Eşleşme bulunup bulunmaması ayrı bir soru —
/// sentetik bir sesin veritabanında olmaması **beklenen** sonuç.
#[tokio::test]
async fn acoustid_accepts_the_fingerprint_we_produce() {
    let Some(lookup) = lookup_or_skip("acoustid_accepts_the_fingerprint") else {
        return;
    };
    let print = fingerprint_file(&sample()).expect("fixture parmak izi verebilmeli");

    let found = lookup
        .recordings_by_fingerprint(&print)
        .await
        .expect("AcoustID parmak izini reddetmemeli");

    eprintln!(
        "AcoustID {} sn'lik parmak izini kabul etti, {} aday döndürdü",
        print.duration_secs,
        found.len()
    );
    // Sentetik ses: aday dönmemesi doğru sonuç. Dönerse de düşmüyoruz —
    // veritabanı büyüyor ve bir gün bir eşleşme çıkabilir; yanlış olan tek
    // şey servisin parmak izini **ayrıştıramaması** olurdu.
    for candidate in &found {
        assert!(
            (0.0..=1.0).contains(&candidate.score),
            "skor 0–1 dışında: {}",
            candidate.score
        );
    }
}

/// Birim testlerin dayandığı fixture hâlâ gerçeği anlatıyor mu.
///
/// `fixtures/identity/acoustid_lookup.json` bütün AcoustID birim testlerinin
/// girdisi. Donmuş bir dosya, ve donmuş her fixture zamanla yalana dönüşebilir:
/// servis şemasını değiştirir, fixture değişmez, birim testler yeşil kalır ve
/// **üretim düşer**. D-046'da tam olarak bu oldu — `"duration"` alanı elle
/// tam sayı yazılmıştı, servis ondalık gönderiyordu ve ilk gerçek eşleşme bir
/// JSON hatasıyla düşecekti.
///
/// Bu test o sınıfı kapatıyor: canlı yanıtı çekip fixture'la **alan alan tip**
/// karşılaştırması yapıyor. Değerler değişebilir (katalog yaşayan bir şey);
/// değişmemesi gereken şey şekil.
#[tokio::test]
async fn the_committed_fixture_still_matches_what_the_service_sends() {
    const FIXTURE: &str = include_str!("../../../fixtures/identity/acoustid_lookup.json");
    /// Fixture'ın alındığı AcoustID kaydı — `Şebnem Ferah — Sil Baştan`.
    const TRACK_ID: &str = "5e45e8ba-c6d9-4782-a20e-5e897f528d23";

    let Some(_lookup) = lookup_or_skip("the_committed_fixture_still_matches") else {
        return;
    };
    let key = std::env::var("TUNE_ACOUSTID_KEY").unwrap_or_default();

    let http = tune_core::net::default_http_client().expect("http-client açık");
    let url = format!(
        "https://api.acoustid.org/v2/lookup?client={key}&trackid={TRACK_ID}\
         &meta=recordings&format=json"
    );
    let response = match http.send(&tune_core::net::HttpRequest::get(&url)).await {
        Ok(response) => response,
        Err(err) => {
            eprintln!(
                "the_committed_fixture_still_matches: ulaşılamadı — atlanıyor\n{}",
                err.chain_text()
            );
            return;
        }
    };

    let live: serde_json::Value =
        serde_json::from_slice(&response.body).expect("canlı yanıt JSON olmalı");
    let frozen: serde_json::Value = serde_json::from_str(FIXTURE).expect("fixture JSON olmalı");

    let mut drift = Vec::new();
    compare_shape("", &frozen, &live, &mut drift);
    assert!(
        drift.is_empty(),
        "AcoustID şeması fixture'dan ayrıldı — birim testler artık gerçeği \
         ölçmüyor olabilir:\n  {}\ncanlı yanıt:\n{live:#}",
        drift.join("\n  ")
    );
    eprintln!("fixture şeması canlı yanıtla uyuşuyor ({TRACK_ID})");
}

/// İki JSON ağacını **tip düzeyinde** karşılaştırır; farkları `drift`'e yazar.
///
/// Değerleri değil tipleri karşılaştırıyor: bir kaydın süresi düzeltilebilir,
/// başlığı değişebilir — bunlar bizi ilgilendirmiyor. `309.0`'ın `309` olması
/// ya da bir alanın kaybolması ilgilendiriyor.
///
/// Yalnızca fixture'ın bildiği alanlar aranıyor: servisin **yeni** alan
/// eklemesi bizi bozmaz (`serde` bilmediğini yok sayar), eksiltmesi bozar.
fn compare_shape(
    path: &str,
    frozen: &serde_json::Value,
    live: &serde_json::Value,
    drift: &mut Vec<String>,
) {
    use serde_json::Value;
    let at = if path.is_empty() { "<kök>" } else { path };
    match (frozen, live) {
        (Value::Object(frozen_map), Value::Object(live_map)) => {
            for (key, frozen_value) in frozen_map {
                match live_map.get(key) {
                    Some(live_value) => {
                        compare_shape(&format!("{path}.{key}"), frozen_value, live_value, drift);
                    }
                    None => drift.push(format!("{at}.{key}: fixture'da var, canlı yanıtta yok")),
                }
            }
        }
        (Value::Array(frozen_items), Value::Array(live_items)) => {
            // İlk öğe temsilci: dizinin uzunluğu değil, öğelerinin şekli önemli.
            match (frozen_items.first(), live_items.first()) {
                (Some(frozen_first), Some(live_first)) => {
                    compare_shape(&format!("{path}[0]"), frozen_first, live_first, drift);
                }
                (Some(_), None) => drift.push(format!("{at}: fixture dolu, canlı yanıt boş")),
                _ => {}
            }
        }
        // Sayılarda tam sayı/ondalık ayrımı **önemli**: `serde` `309.0`'ı bir
        // tam sayı alanına çözemez ve D-046'yı doğuran kusur tam buydu.
        (Value::Number(frozen_num), Value::Number(live_num)) => {
            if frozen_num.is_f64() != live_num.is_f64() {
                drift.push(format!(
                    "{at}: sayı türü değişti (fixture {}, canlı {})",
                    if frozen_num.is_f64() {
                        "ondalık"
                    } else {
                        "tam sayı"
                    },
                    if live_num.is_f64() {
                        "ondalık"
                    } else {
                        "tam sayı"
                    },
                ));
            }
        }
        (Value::String(_), Value::String(_))
        | (Value::Bool(_), Value::Bool(_))
        | (Value::Null, Value::Null) => {}
        (frozen_other, live_other) => drift.push(format!(
            "{at}: tip değişti (fixture {}, canlı {})",
            kind_of(frozen_other),
            kind_of(live_other)
        )),
    }
}

fn kind_of(value: &serde_json::Value) -> &'static str {
    use serde_json::Value;
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "sayı",
        Value::String(_) => "metin",
        Value::Array(_) => "dizi",
        Value::Object(_) => "nesne",
    }
}

/// Yukarıdaki testin **kontrolü**: AcoustID her dizeyi kabul etmiyor.
///
/// `acoustid_accepts_the_fingerprint_we_produce` "servis reddetmedi" diyor.
/// Servis hiçbir şeyi reddetmeseydi o iddia boş olurdu ve boşluğu görünmezdi —
/// test yeşil kalırdı, sıkıştırıcımız bozulsa bile. Bu test o boşluğu kapatıyor:
/// bilerek bozulmuş bir dize gönderiyor ve **reddedilmesini** bekliyor.
///
/// İstek elle kuruluyor çünkü kendi istemcimizden bozuk bir dize geçirmek
/// mümkün değil: `Fingerprint::to_acoustid_string` her zaman yapısal olarak
/// geçerli bir çıktı üretir. Sınanan şey zaten bizim kodumuz değil, servisin
/// ayrımı yapıp yapmadığı (2026-09-02'de ölçüldü: `code 3, invalid fingerprint`).
#[tokio::test]
async fn acoustid_rejects_a_corrupted_fingerprint_so_the_positive_test_means_something() {
    let Some(lookup) = lookup_or_skip("acoustid_rejects_a_corrupted_fingerprint") else {
        return;
    };
    // Anahtarı `lookup`'tan okuyamıyoruz (bilerek gizli, D-042); ortamdan
    // alıyoruz — `lookup_or_skip` zaten dolu olduğunu doğruladı.
    let key = std::env::var("TUNE_ACOUSTID_KEY").unwrap_or_default();
    drop(lookup);

    let print = fingerprint_file(&sample()).expect("fixture parmak izi verebilmeli");
    let valid = print.to_acoustid_string();
    // Ortasından bir parça kesiliyor: alfabesi hâlâ geçerli, yapısı değil.
    let corrupted = format!("{}{}", &valid[..40], &valid[80..]);
    assert_ne!(corrupted, valid, "bozma gerçekten bir şey değiştirmeli");

    let http = tune_core::net::default_http_client().expect("http-client açık");
    // Gövde elle kuruluyor. Kaçırma gerekmiyor: anahtar ve parmak izi yalnızca
    // URL-güvenli alfabeden karakterler taşıyor.
    let body = format!(
        "client={key}&duration={}&fingerprint={corrupted}&meta=recordings",
        print.duration_secs
    );
    let request = tune_core::net::HttpRequest::post_form(
        "https://api.acoustid.org/v2/lookup",
        body.into_bytes(),
    );
    let response = match http.send(&request).await {
        Ok(response) => response,
        Err(err) => {
            eprintln!(
                "acoustid_rejects_a_corrupted_fingerprint: ulaşılamadı — atlanıyor\n{}",
                err.chain_text()
            );
            return;
        }
    };

    let text = response.text_lossy();
    eprintln!("bozuk parmak izine yanıt: {text}");
    assert!(
        text.contains("invalid fingerprint"),
        "servis bozuk parmak izini kabul etti; `kabul etti` testi artık hiçbir şey ölçmüyor: {text}"
    );
}

/// Geçersiz anahtar "eşleşme yok" diye raporlanmamalı.
///
/// Bu test **anahtar gerektirmiyor**: kasten bozuk bir anahtarla gidiyor ve
/// servisin reddini bekliyor. K9'un en çok tekrarlanan dersi burada ölçülüyor —
/// gerçek servis `200` gövdesinde hata döndürüyor ve onu yutan bir istemci
/// kullanıcıyı kusuru dosyasında aramaya gönderirdi.
#[tokio::test]
async fn a_rejected_key_is_an_error_not_an_empty_result() {
    if !acoustid_reachable() {
        eprintln!(
            "a_rejected_key_is_an_error: api.acoustid.org:443'e ulaşılamadı — \
             atlanıyor (ağ yok sayılıyor)"
        );
        return;
    }
    let Ok(http) = tune_core::net::default_http_client() else {
        return;
    };
    let lookup = AcoustIdLookup::new(Arc::clone(&http))
        .with_api_key("gecersiz-anahtar-test")
        .with_user_agent("tune-tests/0.0.1 ( https://github.com/kullanici-adi/tune )");
    let print = fingerprint_file(&sample()).expect("fixture parmak izi verebilmeli");

    let err = lookup
        .recordings_by_fingerprint(&print)
        .await
        .expect_err("geçersiz anahtar boş liste değil hata vermeli");
    let text = err.chain_text();
    eprintln!("{text}");
    assert!(text.starts_with("ADIM: IDENTITY_RESOLVE"), "{text}");
    assert!(text.to_lowercase().contains("api key"), "{text}");
}
