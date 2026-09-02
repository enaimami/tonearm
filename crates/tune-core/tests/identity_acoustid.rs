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
