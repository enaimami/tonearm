//! Canlı Torznab sınaması (D-043'ün kuralıyla).
//!
//! Torznab'ın herkese açık bir örneği yok — kullanıcının kendi Prowlarr ya da
//! Jackett'ı gerekiyor. Bu yüzden "ulaşamamak" gibi burada bir de
//! "yapılandırılmamış" hâli var, ve **ikisi de başarısızlık değil**: test
//! kendini atlar ve sebebini `stderr`'e yazar. Ulaşıp beklenmeyeni alırsa
//! düşer.
//!
//! ```bash
//! TUNE_TORZNAB_URL=http://127.0.0.1:9696/1/api \
//! TUNE_TORZNAB_KEY=... cargo test -p tune-plugin-torrent --test torznab_live
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use tune_plugin_torrent::torznab::Torznab;

fn client_or_skip(test: &str) -> Option<Torznab> {
    let Ok(url) = std::env::var("TUNE_TORZNAB_URL") else {
        eprintln!(
            "{test}: atlandı — TUNE_TORZNAB_URL yok. \
             Canlı sınama kullanıcının kendi Prowlarr/Jackett'ını ister."
        );
        return None;
    };
    let key = std::env::var("TUNE_TORZNAB_KEY").unwrap_or_default();
    match Torznab::new(&url, &key) {
        Ok(client) => Some(client),
        Err(error) => {
            // Adresin biçimi yanlışsa bu bir yapılandırma hatasıdır ve
            // atlanacak bir şey değil: kullanıcı canlı sınama istedi.
            panic!("{test}: TUNE_TORZNAB_URL geçersiz: {error}");
        }
    }
}

/// Ağ hatası mı, yoksa indeksin verdiği bir cevap mı? İkisi ayrı tanı (K9).
fn is_unreachable(message: &str) -> bool {
    message.contains("ulaşılamadı") || message.contains("cevabı okunamadı")
}

#[tokio::test]
async fn the_indexer_answers_a_capabilities_request() {
    let Some(client) = client_or_skip("caps") else {
        return;
    };
    match client.caps().await {
        Ok(detail) => {
            assert!(detail.contains("kategori"), "{detail}");
        }
        Err(error) => {
            let message = error.to_string();
            assert!(
                is_unreachable(&message),
                "indeks cevap verdi ama beklenmeyen bir şey söyledi: {message}"
            );
            eprintln!("caps: atlandı — indekse ulaşılamadı: {message}");
        }
    }
}

#[tokio::test]
async fn a_real_search_returns_releases_that_carry_an_infohash() {
    let Some(client) = client_or_skip("search") else {
        return;
    };
    let query = std::env::var("TUNE_TORZNAB_QUERY").unwrap_or_else(|_| "radiohead".to_owned());

    let outcome = match client.search(&query, 20).await {
        Ok(outcome) => outcome,
        Err(error) => {
            let message = error.to_string();
            assert!(
                is_unreachable(&message),
                "indeks cevap verdi ama beklenmeyen bir şey söyledi: {message}"
            );
            eprintln!("search: atlandı — indekse ulaşılamadı: {message}");
            return;
        }
    };

    // Sıfır sonuç bir başarısızlık **değil**: kullanıcının indeksinde o
    // sorgunun karşılığı olmayabilir. Sınanan şey biçim, katalog değil.
    eprintln!(
        "search: {} yayım, {} kayıt kimliksiz olduğu için düştü",
        outcome.releases.len(),
        outcome.dropped_unidentifiable
    );

    for release in &outcome.releases {
        assert_eq!(
            release.infohash.len(),
            40,
            "infohash 40 hane olmalı: {release:?}"
        );
        assert!(
            release.source_url().is_some(),
            "çalınamayacak bir yayım listeye girmemeli: {release:?}"
        );
        assert!(!release.title.trim().is_empty(), "{release:?}");
    }
}

/// Anahtar yanlışken **boş sonuç değil hata** almalıyız.
///
/// Bu, D-046'nın AcoustID'de öğrendiği dersin torrent tarafındaki karşılığı:
/// reddedilmeyi "bulunamadı" diye okumak, kullanıcıya yanlış şeyi tamir
/// ettirir. Anahtar gerektirmeyen bir kurulumda atlanır.
#[tokio::test]
async fn a_rejected_key_is_an_error_not_an_empty_result() {
    let Ok(url) = std::env::var("TUNE_TORZNAB_URL") else {
        eprintln!("reddedilen anahtar: atlandı — TUNE_TORZNAB_URL yok");
        return;
    };
    if std::env::var("TUNE_TORZNAB_KEY").is_err() {
        eprintln!("reddedilen anahtar: atlandı — kurulum anahtar istemiyor");
        return;
    }
    let Ok(client) = Torznab::new(&url, "kesinlikle-yanlis-bir-anahtar") else {
        panic!("TUNE_TORZNAB_URL geçersiz");
    };

    match client.search("radiohead", 5).await {
        Ok(outcome) => panic!(
            "yanlış anahtar kabul edildi ve {} sonuç döndü — reddedilme sessizce \
             boş sonuca çevrilmiş olabilir",
            outcome.releases.len()
        ),
        Err(error) => {
            let message = error.to_string();
            if is_unreachable(&message) {
                eprintln!("reddedilen anahtar: atlandı — indekse ulaşılamadı: {message}");
                return;
            }
            assert!(
                message.contains("reddetti") || message.contains("HTTP"),
                "reddedilme açıkça söylenmeli: {message}"
            );
        }
    }
}
