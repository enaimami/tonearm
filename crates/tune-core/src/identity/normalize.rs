//! Metin normalizasyonu — bulanık eşleşmenin girdisi.
//!
//! "Radiohead - Creep (Remastered 2011)" ile "radiohead - creep" aynı parçadır.
//! Buradaki her kural bir doğruluk kararıdır; değiştirirsen
//! `fixtures/identity/cases.json` üzerindeki oranı yeniden ölç.

/// Bir sanatçı/başlık dizgisini karşılaştırılabilir hâle getirir.
///
/// Sırayla: aksan katlama → küçük harf → parantezli/tireli ekler → `feat.` →
/// noktalama → boşluk sadeleştirme.
#[must_use]
pub fn normalize_text(input: &str) -> String {
    let folded = fold_diacritics(input);
    let lowered = folded.to_lowercase();
    let without_suffix = strip_edition_suffixes(&lowered);
    let without_feat = strip_featuring(&without_suffix);
    collapse(&strip_punctuation(&without_feat))
}

/// Sanatçı adını normalize eder; başlıktan farklı olarak `the` öneki atılır.
#[must_use]
pub fn normalize_artist(input: &str) -> String {
    let base = normalize_text(input);
    base.strip_prefix("the ")
        .map_or(base.clone(), ToOwned::to_owned)
}

/// Bir parçanın yerel gruplama anahtarı: `sanatçı\u{1}başlık`.
#[must_use]
pub fn track_key(artist: &str, title: &str) -> String {
    format!("{}\u{1}{}", normalize_artist(artist), normalize_text(title))
}

/// Türkçe ve yaygın Latin aksanlarını ASCII'ye katlar.
///
/// Kütüphane eklemek yerine elle: liste küçük ve tam olarak neyin katlandığı
/// görünür olsun istiyoruz.
fn fold_diacritics(input: &str) -> String {
    input
        .chars()
        .map(|c| match c {
            'ç' | 'Ç' => 'c',
            'ğ' | 'Ğ' => 'g',
            'ı' | 'I' => 'i',
            'İ' => 'i',
            'ö' | 'Ö' => 'o',
            'ş' | 'Ş' => 's',
            'ü' | 'Ü' => 'u',
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'Á' | 'À' | 'Â' | 'Ä' | 'Ã' | 'Å' => {
                'a'
            }
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'Ó' | 'Ò' | 'Ô' | 'Õ' => 'o',
            'ú' | 'ù' | 'û' | 'Ú' | 'Ù' | 'Û' => 'u',
            'ñ' | 'Ñ' => 'n',
            'ý' | 'ÿ' | 'Ý' => 'y',
            'ø' | 'Ø' => 'o',
            'æ' | 'Æ' => 'a',
            'ß' => 's',
            other => other,
        })
        .collect()
}

/// Aynı kaydın yeniden yayımını gösteren etiketler — atılırlar.
///
/// Bunlar kaydın kendisini değiştirmez: `Creep` ile `Creep (Remastered 2011)`
/// aynı performanstır.
const REISSUE_MARKERS: [&str; 13] = [
    "remaster",
    "remastered",
    "deluxe",
    "edition",
    "version",
    "mono",
    "stereo",
    "bonus track",
    "radio edit",
    "single version",
    "album version",
    "anniversary",
    "expanded",
];

/// Parçanın **başka bir kaydı** olduğunu söyleyen etiketler — atılmazlar.
///
/// D-009: canlı kayıt stüdyo kaydından, remix orijinalinden ayrı olmalı.
/// Bu etiketler eskiden [`REISSUE_MARKERS`] içindeydi ve `Creep (Live at
/// Glastonbury)` stüdyo kaydına %100 güvenle bağlanıyordu. Artık hem metinde
/// kalırlar hem de [`variant_markers`] üzerinden skoru düşürürler.
pub(crate) const VARIANT_MARKERS: [&str; 9] = [
    "live",
    "remix",
    "acoustic",
    "instrumental",
    "karaoke",
    "demo",
    "unplugged",
    "cover",
    "reprise",
];

/// Normalize edilmiş bir başlıkta geçen varyant etiketleri (sıralı, tekil).
///
/// İki tarafın kümesi farklıysa bu iki kayıt aynı performans değildir.
/// Karşılaştırma **kelime** bazında: `"live"`, `"delivered"` içinde geçtiği
/// için alt dizi araması yanlış pozitif üretir.
#[must_use]
pub fn variant_markers(normalized_title: &str) -> Vec<&'static str> {
    let mut found: Vec<&'static str> = VARIANT_MARKERS
        .iter()
        .copied()
        .filter(|marker| {
            marker.split_whitespace().all(|word| {
                normalized_title
                    .split_whitespace()
                    .any(|token| token == word)
            })
        })
        .collect();
    found.sort_unstable();
    found
}

/// Yeniden yayım eklerini atar: `(Remastered 2011)`, `- 2011 Remaster`,
/// `[Deluxe Edition]`.
///
/// Yalnızca [`REISSUE_MARKERS`] atılır. İçinde bir [`VARIANT_MARKERS`] geçen
/// ek **korunur** — `(Live Version)` hem "live" hem "version" içerir ve
/// atılırsa canlı kayıt stüdyo kaydıyla birleşir. Parçanın gerçek adının
/// parçası olan ekler de (`(Reprise)`) korunur; farklı parçaları birleştirmek
/// kaçırılan eşleşmeden daha kötüdür.
fn strip_edition_suffixes(input: &str) -> String {
    let droppable = |text: &str| {
        !VARIANT_MARKERS.iter().any(|m| text.contains(m))
            && REISSUE_MARKERS.iter().any(|m| text.contains(m))
    };

    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(open) = rest.find(['(', '[']) {
        let close_char = if rest.as_bytes()[open] == b'(' {
            ')'
        } else {
            ']'
        };
        let Some(close_rel) = rest[open..].find(close_char) else {
            break;
        };
        let close = open + close_rel;
        let inside = &rest[open + 1..close];
        out.push_str(&rest[..open]);
        if !droppable(inside) {
            out.push_str(&rest[open..=close]);
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);

    // " - Remastered 2011" biçimindeki tireli ekler.
    if let Some(dash) = out.rfind(" - ") {
        if droppable(&out[dash + 3..]) {
            out.truncate(dash);
        }
    }
    out
}

/// `feat. X`, `ft. X`, `featuring X` eklerini atar.
fn strip_featuring(input: &str) -> String {
    const MARKERS: [&str; 4] = ["feat.", "feat ", "ft.", "featuring "];
    let mut out = input.to_owned();
    for marker in MARKERS {
        if let Some(pos) = out.find(marker) {
            out.truncate(pos);
        }
    }
    out
}

/// Alfanümerik ve boşluk dışındaki her şeyi boşluğa çevirir.
fn strip_punctuation(input: &str) -> String {
    input
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect()
}

/// Ardışık boşlukları teke indirir ve kırpar.
fn collapse(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edition_suffixes_are_dropped() {
        assert_eq!(normalize_text("Creep (Remastered 2011)"), "creep");
        assert_eq!(
            normalize_text("Wish You Were Here - 2011 Remaster"),
            "wish you were here"
        );
        assert_eq!(
            normalize_text("Bohemian Rhapsody [Deluxe Edition]"),
            "bohemian rhapsody"
        );
    }

    #[test]
    fn meaningful_parentheses_survive() {
        assert_eq!(
            normalize_text("Karma Police (Reprise)"),
            "karma police reprise"
        );
    }

    #[test]
    fn variant_labels_are_kept_not_stripped() {
        // D-009: canlı/remix kaydı stüdyo kaydıyla birleşmemeli.
        assert_eq!(
            normalize_text("Creep (Live at Glastonbury)"),
            "creep live at glastonbury"
        );
        assert_eq!(
            normalize_text("Wish You Were Here - Live"),
            "wish you were here live"
        );
        // "Live Version" hem varyant hem yeniden yayım etiketi içerir;
        // varyant kazanır.
        assert_eq!(normalize_text("Numb (Live Version)"), "numb live version");
        assert_eq!(
            normalize_text("Aerodynamic (Slum Village Remix)"),
            "aerodynamic slum village remix"
        );
    }

    #[test]
    fn variant_markers_match_whole_words_only() {
        assert_eq!(variant_markers(&normalize_text("Creep (Live)")), ["live"]);
        assert_eq!(variant_markers(&normalize_text("Creep")), [] as [&str; 0]);
        // "live", "delivered" içinde geçer — alt dizi araması yanlış pozitif verir.
        assert_eq!(
            variant_markers(&normalize_text("Delivered")),
            [] as [&str; 0]
        );
        assert_eq!(
            variant_markers(&normalize_text("Numb (Live Acoustic)")),
            ["acoustic", "live"]
        );
    }

    #[test]
    fn featuring_is_dropped() {
        assert_eq!(normalize_text("Numb feat. Jay-Z"), "numb");
        assert_eq!(normalize_text("Otherside ft. Someone"), "otherside");
    }

    #[test]
    fn turkish_characters_fold_to_ascii() {
        assert_eq!(normalize_artist("Şebnem Ferah"), "sebnem ferah");
        assert_eq!(normalize_text("Gündoğdu"), "gundogdu");
        assert_eq!(normalize_text("Işıl Işıl"), "isil isil");
    }

    #[test]
    fn the_prefix_only_drops_for_artists() {
        assert_eq!(normalize_artist("The Beatles"), "beatles");
        assert_eq!(
            normalize_text("The Less I Know The Better"),
            "the less i know the better"
        );
    }

    #[test]
    fn track_key_joins_both_sides() {
        assert_eq!(
            track_key("The Beatles", "Yesterday"),
            "beatles\u{1}yesterday"
        );
        assert_eq!(
            track_key("Radiohead", "Creep (Remastered)"),
            track_key("radiohead", "creep")
        );
    }
}
