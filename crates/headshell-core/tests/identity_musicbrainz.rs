//! Kimlik zinciri, **gerçek MusicBrainz'e karşı** (Faz 2 §2.3).
//!
//! `identity_accuracy.rs` skorlamayı sentetik bir katalogla ölçüyor: orada
//! sınanan şey mantık. Burada sınanan şey başka — sorgunun gerçekten kurulup
//! gittiği, gelen yanıtın çözüldüğü ve zincirin `LocalKey`'e düşmeden
//! **otoriteye** bağlandığı. Bu iki testin ikisi de gerekli: biri kuralları
//! ölçer, öteki kuralların doğru veriye uygulandığını.
//!
//! **Bu testler varsayılan koşuma dahildir** (D-043'ün SoundCloud için verdiği
//! kararın aynısı). İki başarısızlık ayrı tutuluyor (K9):
//! - **Ulaşamamak** başarısızlık değil: ağ yoksa test kendini atlar ve sebebini
//!   `stderr`'e yazar. Bu, baştaki TCP yoklamasıyla **sınırlı değil** —
//!   testin ortasında gelen zaman aşımı ya da tükenmiş `503` yeniden
//!   denemesi de atlanır ([`skip_if_unreachable`], D-046). Yoğun bir koşumda
//!   kotayı sunucunun o anki yüküyle de paylaşıyoruz ve bu ayrım olmadan
//!   yeşil bir kapı ağ havasına bağlı kalırdı.
//! - **Ulaşıp beklenmeyeni almak** düşer. `400`, `404`, ayrıştırma hatası ve
//!   beklenmeyen gövde bu tarafta: onlar bizim kusurumuz olabilir.
//!
//! Kırmızı yandığında ilk soru: `curl -A 'x/1 ( y )'
//! 'https://musicbrainz.org/ws/2/recording?query=recording:%22Creep%22&fmt=json'`
//! ne diyor? Çalışıyorsa kusur bizde.
//!
//! **Hız sınırı.** MusicBrainz anonim istemciye saniyede bir istek veriyor.
//! Testler tek bir [`MusicBrainzLookup`] paylaşıyor ki paralel koşumda da aynı
//! kısıtlayıcıdan geçsinler — ayrı örnekler kotayı test sayısıyla çarpardı.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::{Arc, OnceLock};

use headshell_core::identity::musicbrainz::MusicBrainzLookup;
use headshell_core::identity::{MetadataLookup, ResolveMethod, Resolver};
use headshell_core::ids::{CanonicalKind, Isrc};
use headshell_core::model::TrackRef;

/// Gerçek bir ISRC ve bağlandığı kayıt (2026-09-01'de MusicBrainz'den ölçüldü).
///
/// Türkçe seçildi çünkü tek taşla iki kuş: ISRC halkası **ve** sorgunun
/// yüzde kodlamasının çok baytlı karakterlerde bozulmadığı.
const KNOWN_ISRC: &str = "TR0441211603";
const KNOWN_ISRC_TITLE: &str = "Sil Baştan";

/// MusicBrainz'e TCP ile ulaşılabiliyor mu.
///
/// Yalnızca DNS + bağlantı; HTTP'ye girilmiyor. Amaç "ağ var mı" sorusunu
/// cevaplamak, servisin sağlığını ölçmek değil — sağlığı ölçmek testlerin işi.
fn musicbrainz_reachable() -> bool {
    use std::net::ToSocketAddrs;
    let Ok(mut addrs) = ("musicbrainz.org", 443).to_socket_addrs() else {
        return false;
    };
    addrs.any(|addr| {
        std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(5)).is_ok()
    })
}

/// Bütün testlerin paylaştığı kaynak — tek kısıtlayıcı, tek kota.
///
/// `User-Agent` testlere özel: MusicBrainz'in günlüğünde kotayı kimin
/// harcadığı görünsün ve bir test koşumu gerçek dağıtımın kimliğine
/// yazılmasın.
fn shared_lookup() -> Option<Arc<dyn MetadataLookup>> {
    static LOOKUP: OnceLock<Option<Arc<dyn MetadataLookup>>> = OnceLock::new();
    LOOKUP
        .get_or_init(|| {
            let http = headshell_core::net::default_http_client().ok()?;
            let lookup = MusicBrainzLookup::new(http)
                .with_user_agent("headshell-tests/0.0.1 ( https://github.com/enaimami/headshell )");
            Some(Arc::new(lookup) as Arc<dyn MetadataLookup>)
        })
        .clone()
}

/// Ağa **ulaşamamak** başarısızlık değildir — sonucu atlamaya çevirir.
///
/// Baştaki TCP yoklaması "ağ var mı" sorusunu cevaplıyor ama testin ortasında
/// gelen zaman aşımını ya da tükenmiş `503` yeniden denemesini göremez.
/// MusicBrainz anonim istemciye saniyede bir istek veriyor ve yoğun bir
/// koşumda sınırı kendi kotamızla değil sunucunun o anki yüküyle de
/// paylaşıyoruz. K9 / D-043'ün çizgisi burada da geçerli: **ulaşamamak
/// atlanır, ulaşıp beklenmeyeni almak düşer.** Bu ayrım olmadan yeşil bir
/// kapı ağ havasına bağlı kalırdı.
///
/// Yalnızca taşıma katmanı hataları atlanıyor. Ayrıştırma hatası, `400`, `404`
/// ya da beklenmeyen bir gövde **düşer** — onlar bizim kusurumuz olabilir.
fn skip_if_unreachable<T>(test: &str, result: headshell_core::Result<T>) -> Option<T> {
    use headshell_core::error::ErrorKind;
    match result {
        Ok(value) => Some(value),
        Err(err) => {
            let unreachable = matches!(
                err.kind(),
                ErrorKind::Network { .. } | ErrorKind::HttpStatus { status: 503, .. }
            );
            assert!(
                unreachable,
                "{test}: beklenmeyen hata (ulaşılamamak değil):\n{}",
                err.chain_text()
            );
            eprintln!(
                "{test}: MusicBrainz'e ulaşılamadı — atlanıyor (ağ ya da kota):\n{}",
                err.chain_text()
            );
            None
        }
    }
}

/// Testin koşulup koşulamayacağını söyler; koşulamıyorsa **sebebini yazar**.
fn lookup_or_skip(test: &str) -> Option<Arc<dyn MetadataLookup>> {
    let Some(lookup) = shared_lookup() else {
        eprintln!(
            "{test}: `http-client` feature'ı kapalı derleme — atlanıyor \
             (bu bir başarısızlık değil; `cargo test --workspace` ile açılır)"
        );
        return None;
    };
    if !musicbrainz_reachable() {
        eprintln!("{test}: musicbrainz.org:443'e ulaşılamadı — atlanıyor (ağ yok sayılıyor)");
        return None;
    }
    Some(lookup)
}

/// Zincirin ortası artık gerçekten çalışıyor mu (D-045'in asıl sorusu).
#[tokio::test]
async fn the_chain_reaches_an_authority_instead_of_falling_to_a_local_key() {
    let Some(lookup) = lookup_or_skip("the_chain_reaches_an_authority") else {
        return;
    };
    let resolver = Resolver::new(lookup);
    let track = TrackRef::new("Şebnem Ferah", "Sil Baştan").with_duration_ms(Some(309_000));

    let Some(resolution) = skip_if_unreachable(
        "the_chain_reaches_an_authority",
        resolver.resolve(&track).await,
    ) else {
        return;
    };
    eprintln!(
        "çözüldü: {} ({}, güven {:.2})",
        resolution.canonical_id, resolution.method, resolution.confidence
    );

    assert_ne!(
        resolution.method,
        ResolveMethod::LocalKey,
        "`OfflineLookup` ile aynı sonucu verdiysek MusicBrainz hiç devreye girmemiş demektir"
    );
    assert_eq!(resolution.canonical_id.kind(), CanonicalKind::Mbid);
    let matched = resolution
        .matched
        .expect("otoriteli eşleşmenin adayı olmalı");
    assert_eq!(matched.title, "Sil Baştan");
}

/// Birinci halka: ISRC doğrudan kayda götürmeli.
#[tokio::test]
async fn a_real_isrc_resolves_through_the_first_link() {
    let Some(lookup) = lookup_or_skip("a_real_isrc_resolves") else {
        return;
    };
    let isrc = Isrc::parse(KNOWN_ISRC).expect("sabit ISRC geçerli biçimde");
    let Some(found) = skip_if_unreachable(
        "a_real_isrc_resolves",
        lookup.recording_by_isrc(&isrc).await,
    ) else {
        return;
    };

    let candidate = found.unwrap_or_else(|| {
        panic!("{KNOWN_ISRC} MusicBrainz'de bulunamadı — kayıt birleştirilmiş olabilir")
    });
    eprintln!(
        "ISRC {KNOWN_ISRC} → {} ({})",
        candidate.mbid, candidate.title
    );
    assert_eq!(candidate.title, KNOWN_ISRC_TITLE);
    assert_eq!(candidate.artist, "Şebnem Ferah");
}

/// Lucene'in özel karakterleri: kaçırılmazsa MusicBrainz `400` döner.
///
/// Sahte istemciyle sınanan şey sorgunun *şekli*; burada sınanan şey
/// MusicBrainz'in o şekli kabul ettiği. İkisi farklı iddialar.
#[tokio::test]
async fn a_slash_in_the_artist_name_does_not_break_the_query() {
    let Some(lookup) = lookup_or_skip("a_slash_in_the_artist_name") else {
        return;
    };
    // `400` **düşer**: Lucene kaçırması bizim işimiz. Yalnızca taşıma
    // katmanı hataları atlanıyor.
    let Some(found) = skip_if_unreachable(
        "a_slash_in_the_artist_name",
        lookup.search_recordings("AC/DC", "Back in Black").await,
    ) else {
        return;
    };

    assert!(!found.is_empty(), "AC/DC için aday dönmeliydi");
    assert!(
        found.iter().any(|c| c.artist.contains("AC/DC")),
        "dönen adaylar: {:?}",
        found.iter().map(|c| &c.artist).collect::<Vec<_>>()
    );
}

/// D-045'in düzeltmesi gerçek katalogda tutuyor mu.
///
/// MusicBrainz'de "Radiohead — Creep" araması 190'dan fazla kayıt döndürüyor ve
/// ilk sayfanın çoğu **canlı kayıt**; hiçbirinin başlığında "live" yazmıyor,
/// hepsinin ayırt edici notunda yazıyor. Düzeltmeden önce ölçülen davranış: 1994
/// Astoria kaydı 1.00 güvenle "tam isabet" sayılıyordu.
///
/// Süre veriliyor (stüdyo kaydı 3:58) çünkü D-046'dan sonra ayrımı yapan iki
/// kanıt var ve ikisi de gerçek katalogda sınanmalı: ayırt edici not **ve**
/// süreye yakınlık.
#[tokio::test]
async fn a_live_take_does_not_win_over_the_studio_take() {
    let Some(lookup) = lookup_or_skip("a_live_take_does_not_win") else {
        return;
    };
    let resolver = Resolver::new(lookup);
    let track = TrackRef::new("Radiohead", "Creep").with_duration_ms(Some(238_000));

    let Some(resolution) =
        skip_if_unreachable("a_live_take_does_not_win", resolver.resolve(&track).await)
    else {
        return;
    };
    // Değişmez şu: **canlı kayıt kazanamaz.** Hiç kazanan çıkmaması da bu
    // değişmezi bozmuyor ve gerçek katalogda olabiliyor — MusicBrainz aramayı
    // birden çok indeks kopyasından sunuyor ve 238 sn'lik stüdyo kaydı her
    // kopyanın ilk 25'inde bulunmuyor. O zaman D-046'nın kuralı devreye girip
    // otorite iddia etmiyor, ki doğrusu da bu. Testi "her zaman bir aday
    // dönmeli" diye yazmak, ölçtüğü şeyi ağın o anki kopyasına bağlardı.
    let Some(matched) = resolution.matched else {
        assert_eq!(
            resolution.method,
            ResolveMethod::LocalKey,
            "aday yoksa yöntem yerel anahtar olmalı"
        );
        eprintln!(
            "seçilen: yok — {} aday ayırt edilemedi, otorite iddia edilmedi",
            resolution.tied_candidates
        );
        return;
    };
    eprintln!(
        "seçilen: {} — {:?} ({}, güven {:.2})",
        matched.mbid, matched.disambiguation, resolution.method, resolution.confidence
    );

    let note = matched.disambiguation.unwrap_or_default().to_lowercase();
    assert!(
        !note.contains("live"),
        "canlı kayıt stüdyo kaydının önüne geçti: {note:?}"
    );
}

/// Süresiz belirsiz bir sorgu **hiç** otorite almamalı (D-046).
///
/// `Radiohead — Creep`, süre verilmeden, gerçek katalogda 9–10 adayı aynı
/// skorda ve aynı rütbede bırakıyor. Aralarından seçim yapan her kural
/// keyfidir ve MusicBrainz aramayı birden çok indeks kopyasından sunduğu için
/// **koşumlar arası da kararsızdır**. Doğru cevap: kimlik uydurma.
///
/// Bu testin kardeşi olan `a_live_take_does_not_win_over_the_studio_take`
/// aynı sorguyu süreyle soruyor ve orada otorite bekleniyor — ikisi birlikte
/// kuralın hangi kanıtla açıldığını gösteriyor.
#[tokio::test]
async fn an_ambiguous_query_without_duration_claims_no_authority() {
    let Some(lookup) = lookup_or_skip("an_ambiguous_query_without_duration") else {
        return;
    };
    let resolver = Resolver::new(lookup);
    let track = TrackRef::new("Radiohead", "Creep");

    let Some(resolution) = skip_if_unreachable(
        "an_ambiguous_query_without_duration",
        resolver.resolve(&track).await,
    ) else {
        return;
    };
    eprintln!(
        "süresiz sorgu: {} ({}, berabere {})",
        resolution.canonical_id, resolution.method, resolution.tied_candidates
    );

    assert_eq!(
        resolution.method,
        ResolveMethod::LocalKey,
        "ayırt edici kanıt yokken otorite iddia edildi"
    );
    assert!(
        resolution.tied_candidates > 1,
        "sebep 'aday yok' değil 'ayırt edilemedi' olmalı: {}",
        resolution.tied_candidates
    );
}

/// Aynı sorgu her koşumda aynı kanonik kimliği vermeli.
///
/// Bu test D-045'in canlı koşumunda **düştü ve düzeltmeyi doğurdu**: MusicBrainz
/// 190'dan fazla `Creep` kaydı döndürüyor, onlarcası birebir aynı sanatçı ve
/// başlığı taşıyor, süre bilinmediğinde hepsi aynı skoru alıyor. Seçim
/// sunucunun gönderdiği sıraya kalmıştı ve o sıra sabit değil — iki koşum iki
/// farklı MBID verdi. Kimlik katmanında bu, aynı parçanın yarın başka bir
/// kimlik alması demekti.
///
/// İki çağrı bilerek **ayrı** yapılıyor: [`Resolver`] tek çağrı içinde
/// önbellek kullanıyor, oysa sınanan şey iki ayrı ağ yanıtının aynı sonuca
/// varması.
#[tokio::test]
async fn the_same_query_always_yields_the_same_canonical_id() {
    let Some(lookup) = lookup_or_skip("the_same_query_always_yields_the_same_id") else {
        return;
    };
    let resolver = Resolver::new(lookup);
    let track = TrackRef::new("Radiohead", "Creep");

    let name = "the_same_query_always_yields_the_same_id";
    let (Some(first), Some(second)) = (
        skip_if_unreachable(name, resolver.resolve(&track).await),
        skip_if_unreachable(name, resolver.resolve(&track).await),
    ) else {
        return;
    };

    assert_eq!(
        first.canonical_id, second.canonical_id,
        "aynı sorgu iki farklı kimlik verdi: eşit skorlu adaylar arasındaki \
         seçim sunucunun sırasına bağlı kalmış demektir"
    );
}

/// Otorite uydurulmaz: karşılığı olmayan bir parça `LocalKey`'de kalmalı.
#[tokio::test]
async fn nonsense_never_invents_an_authority() {
    let Some(lookup) = lookup_or_skip("nonsense_never_invents_an_authority") else {
        return;
    };
    let resolver = Resolver::new(lookup);
    let track = TrackRef::new("Zzqx Vlorbnak Ensemble", "Hgggrmphl Suite No. 41");

    let Some(resolution) = skip_if_unreachable(
        "nonsense_never_invents_an_authority",
        resolver.resolve(&track).await,
    ) else {
        return;
    };
    assert_eq!(
        resolution.method,
        ResolveMethod::LocalKey,
        "eşleşmeyen parça otoriteye bağlanmamalı: {} (güven {:.2})",
        resolution.canonical_id,
        resolution.confidence
    );
}
