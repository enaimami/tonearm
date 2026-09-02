//! Aynı arama iki kez: MusicBrainz aynı aday kümesini mi döndürüyor?
//!
//! D-046'nın canlı koşumu `the_same_query_always_yields_the_same_canonical_id`
//! testini düşürdü. Bu sonda sebebi ayırıyor: skorlama mı kararsız, yoksa
//! **gelen aday kümesi** mi? İkisi bambaşka kusurlar ve ölçmeden ayrılmıyorlar.
//!
//! Ölçülen (2026-09-02): aynı sorgu art arda iki kez 25 aday döndürüyor ve bazı
//! koşumlarda **ortak aday sayısı sıfır** — MusicBrainz aramayı birden çok
//! indeks kopyasından sunuyor. Bir kopya içinde sıra sabit, kopyalar arasında
//! top-25 tamamen farklı.
//!
//! ```bash
//! cargo run -p tune-core --features http-client --example mb_stability_probe -- "Sanatçı" "Başlık"
//! ```

use tune_core::identity::{MetadataLookup as _, Resolver};
use tune_core::model::TrackRef;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let artist = args.next().unwrap_or_else(|| "Radiohead".to_owned());
    let title = args.next().unwrap_or_else(|| "Creep".to_owned());
    let duration_ms: Option<u64> = args.next().and_then(|raw| raw.parse().ok());

    let Ok(http) = tune_core::net::default_http_client() else {
        println!("`http-client` feature'ı kapalı derleme — sonda çalışamaz.");
        return;
    };
    let lookup = std::sync::Arc::new(
        tune_core::identity::musicbrainz::MusicBrainzLookup::new(http)
            .with_user_agent("tune-tests/0.0.1 ( https://github.com/kullanici-adi/tune )"),
    );
    let resolver = Resolver::new(lookup.clone());
    let track = TrackRef::new(&artist, &title).with_duration_ms(duration_ms);
    println!("sorgu: {artist} — {title} (süre {duration_ms:?})");

    let mut sets: Vec<Vec<String>> = Vec::new();
    for round in 1..=2 {
        match lookup.search_recordings(&artist, &title).await {
            Ok(found) => {
                println!("koşum {round}: {} aday", found.len());
                // En iyi beş adayın skoru ve süresi: eşitlik gerçek mi, yoksa
                // skorlama ayırt edebilecekken ayırmıyor mu?
                let mut scored: Vec<(f64, String, Option<u64>)> = found
                    .iter()
                    .map(|c| {
                        let score = tune_core::identity::fuzzy::similarity(
                            &artist,
                            &title,
                            duration_ms,
                            &c.artist,
                            &c.title,
                            c.duration_ms,
                            c.disambiguation.as_deref(),
                        );
                        (score, c.mbid.as_str().to_owned(), c.duration_ms)
                    })
                    .collect();
                scored.sort_by(|a, b| b.0.total_cmp(&a.0));
                for (score, mbid, dur) in scored.iter().take(5) {
                    println!("    {score:.4}  {mbid}  süre={dur:?}");
                }
                sets.push(
                    found
                        .iter()
                        .map(|candidate| candidate.mbid.as_str().to_owned())
                        .collect(),
                );
            }
            Err(err) => {
                println!("koşum {round} başarısız:\n{}", err.chain_text());
                return;
            }
        }

        // Zincirin bu küme üzerinde ne yaptığı: kimlik, yöntem, kaç aday
        // berabere. Asıl soru "aynı cevabı mı veriyor", ve beraberlik sayısı
        // reddetme kuralının hangi eşikte tutacağını söylüyor.
        match resolver.resolve(&track).await {
            Ok(res) => println!(
                "  → {} ({}, güven {:.3}, berabere {})",
                res.canonical_id, res.method, res.confidence, res.tied_candidates
            ),
            Err(err) => println!("  → çözümleme hatası: {}", err.chain_text()),
        }
    }

    let (first, second) = (&sets[0], &sets[1]);
    let overlap = first.iter().filter(|id| second.contains(id)).count();
    println!("aynı sıra mı : {}", first == second);
    println!("ortak aday   : {overlap}");
    println!("1. ilk üç    : {:?}", &first[..first.len().min(3)]);
    println!("2. ilk üç    : {:?}", &second[..second.len().min(3)]);
}
