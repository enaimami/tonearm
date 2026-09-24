//! Yayım ve dosya adlarından sanatçı/başlık çıkarma.
//!
//! Bu bir **tahmin katmanıdır** ve öyle olduğunu saklamıyor. Torrent
//! adlarının şeması yok; buradan çıkan sanatçı/başlık kanonik kimlik değil,
//! kimlik zincirine (K6) verilecek girdidir. Zincir bunu MusicBrainz'e
//! sorar ve gerekirse parmak iziyle düzeltir — yani burada yanlış tahmin
//! etmek, zincirin düzeltemeyeceği bir hata değil.
//!
//! Emin olamadığımızda **uydurmuyoruz**: sanatçı boş kalır. Boş bir alan
//! "bilmiyorum" demektir; yanlış bir sanatçı adı ise zinciri yanlış kayda
//! bağlar (D-046'nın süre kusurunun aynısı).

/// Adlarda geçen ve müzikle ilgisi olmayan etiketler. Yalnızca **sondaki**
/// parantez/köşeli parantez grupları için kullanılıyor, adın ortasındaki bir
/// kelimeyi silmiyoruz — "Blur - 13" gibi adlar bozulmasın.
const NOISE: &[&str] = &[
    "flac",
    "mp3",
    "aac",
    "alac",
    "ape",
    "wav",
    "ogg",
    "opus",
    "wv",
    "dsd",
    "vinyl",
    "web",
    "cd",
    "cdrip",
    "webrip",
    "24bit",
    "16bit",
    "lossless",
    "remaster",
    "remastered",
    "reissue",
    "deluxe",
    "kbps",
    "khz",
    "v0",
    "v2",
    "320",
    "192",
    "256",
];

/// Ses dosyası uzantıları. Bir torrent'in içindeki hangi dosyaların parça
/// olduğunu bu belirliyor.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "flac", "mp3", "ogg", "opus", "m4a", "aac", "wav", "wv", "ape", "alac", "aiff", "aif", "mpc",
    "dsf",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedName {
    /// Boş dize "bilmiyorum" demektir — uydurulmuş bir ad değil.
    pub artist: String,
    pub title: String,
    pub year: Option<u16>,
}

/// Dosya yolunun uzantısı ses uzantısı mı.
pub fn is_audio(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .is_some_and(|ext| AUDIO_EXTENSIONS.contains(&ext.as_str()))
}

/// Bir yayım adını sanatçı + albüm + yıl'a ayırır.
pub fn parse_release_name(raw: &str) -> ParsedName {
    let raw = raw.trim();
    // Boşluksuz ad = "scene" yazımı. Yalnızca o durumda sondaki çıplak
    // kelimeleri (FLAC, WEB…) atıyoruz: boşluklu bir adda "Web" gerçekten
    // albüm adının parçası olabilir.
    let scene_style = !raw.contains(' ');
    let text = normalize_separators(raw);

    // **Sıra önemli.** Önce sanatçı ayrılıyor: "Van Halen - 1984"te yılı önce
    // almak başlığı boşaltıp sanatçıyı da kaybettiriyordu.
    let (artist, mut title) = split_artist(&text).unwrap_or((String::new(), text));

    if scene_style {
        strip_trailing_bare_noise(&mut title);
    }
    strip_trailing_noise(&mut title);
    let year = take_year(&mut title);
    strip_trailing_noise(&mut title);

    ParsedName {
        artist,
        title: if title.is_empty() {
            raw.to_owned()
        } else {
            title
        },
        year,
    }
}

/// Torrent içindeki bir dosya adından parça başlığı ve sıra numarası.
///
/// Dönüş `(başlık, parça no)`. Başlık hiçbir zaman boş dönmez: hiçbir şey
/// ayıklanamazsa uzantısız dosya adının kendisidir.
pub fn parse_file_name(file_name: &str) -> (String, Option<u32>) {
    let stem = file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem);
    let mut text = normalize_separators(stem).trim().to_owned();

    // Baştaki "01", "01 -", "1." gibi sıra numaraları.
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    let mut track_no = None;
    if !digits.is_empty() && digits.len() <= 3 {
        if let Ok(parsed) = digits.parse::<u32>() {
            let rest = text[digits.len()..].trim_start();
            let rest = rest
                .strip_prefix('-')
                .or_else(|| rest.strip_prefix('.'))
                .or_else(|| rest.strip_prefix(')'))
                .unwrap_or(rest)
                .trim_start();
            // "13" tek başına bir başlık olabilir; yalnızca arkasında bir şey
            // varsa sıra numarası sayıyoruz.
            if !rest.is_empty() {
                track_no = Some(parsed);
                text = rest.to_owned();
            }
        }
    }

    let title = if text.trim().is_empty() {
        stem.trim().to_owned()
    } else {
        text.trim().to_owned()
    };
    (title, track_no)
}

/// Nokta/alt çizgi ile yazılmış adları boşluklu hâle getirir.
///
/// Yalnızca adda hiç boşluk yoksa: "Artist.Name-Album.2007" böyle yazılır ama
/// "Godspeed You! Black Emperor - F♯A♯∞" içindeki noktaya dokunmamalıyız.
fn normalize_separators(raw: &str) -> String {
    if raw.contains(' ') {
        return raw.to_owned();
    }
    raw.replace(['.', '_'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Sondaki `(1997)` / `[1997]` / ` 1997` yılını alır ve metinden siler.
fn take_year(text: &mut String) -> Option<u16> {
    let mut found = None;
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(start) = rest.find(['(', '[']) {
        let open = &rest[start..start + 1];
        let close = if open == "(" { ')' } else { ']' };
        let Some(end) = rest[start..].find(close) else {
            break;
        };
        let inner = &rest[start + 1..start + end];
        if let Some(year) = as_year(inner) {
            found = Some(year);
            out.push_str(&rest[..start]);
            rest = &rest[start + end + 1..];
            continue;
        }
        out.push_str(&rest[..start + end + 1]);
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    *text = out.split_whitespace().collect::<Vec<_>>().join(" ");

    if found.is_none() {
        // Parantezsiz sondaki yıl: "Artist - Album 1997".
        if let Some((head, tail)) = text.rsplit_once(' ') {
            if let Some(year) = as_year(tail) {
                found = Some(year);
                *text = head.trim().to_owned();
            }
        }
    }
    found
}

fn as_year(candidate: &str) -> Option<u16> {
    let candidate = candidate.trim();
    if candidate.len() != 4 || !candidate.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let year: u16 = candidate.parse().ok()?;
    (1900..=2100).contains(&year).then_some(year)
}

/// Sondaki `[FLAC]`, `(WEB)`, `-GRUP` gibi kalıntıları temizler.
fn strip_trailing_noise(text: &mut String) {
    loop {
        let trimmed = text.trim_end();
        let Some(start) = trimmed.rfind(['(', '[']) else {
            break;
        };
        let close = if trimmed[start..].starts_with('(') {
            ')'
        } else {
            ']'
        };
        if !trimmed.ends_with(close) {
            break;
        }
        let inner = trimmed[start + 1..trimmed.len() - 1].to_ascii_lowercase();
        let words: Vec<&str> = inner
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|word| !word.is_empty())
            .collect();
        // Boş bir grup (`( )`) gürültü **değil**: Sigur Rós'un albümü öyle.
        // "Hepsi gürültü" iddiası, en az bir kelime varsa anlamlı.
        if words.is_empty() || !words.iter().all(|word| NOISE.contains(word)) {
            break;
        }
        *text = trimmed[..start].trim_end().to_owned();
    }
    *text = text.trim().to_owned();
}

/// Scene yazımında sondaki çıplak gürültü kelimeleri: "… 1994 FLAC" → "… 1994".
fn strip_trailing_bare_noise(text: &mut String) {
    loop {
        let Some((head, tail)) = text.trim_end().rsplit_once(' ') else {
            return;
        };
        if !NOISE.contains(&tail.to_ascii_lowercase().as_str()) {
            return;
        }
        *text = head.trim_end().to_owned();
    }
}

/// "Sanatçı - Albüm" ayrımı. Ayıraç yoksa `None` — sanatçı **uydurulmaz**.
fn split_artist(text: &str) -> Option<(String, String)> {
    for separator in [" - ", " – ", " — "] {
        if let Some((artist, title)) = text.split_once(separator) {
            let artist = artist.trim();
            let title = title.trim();
            if !artist.is_empty() && !title.is_empty() {
                return Some((artist.to_owned(), title.to_owned()));
            }
        }
    }
    // Boşluksuz scene adı: "Artist-Album-2007-GRUP" normalize sonrası zaten
    // boşluklu; kalan tek tireli hâl için son bir deneme.
    let (artist, title) = text.split_once('-')?;
    let (artist, title) = (artist.trim(), title.trim());
    (!artist.is_empty() && !title.is_empty() && artist.contains(' '))
        .then(|| (artist.to_owned(), title.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_conventional_release_name_splits_into_artist_album_and_year() {
        let parsed = parse_release_name("Radiohead - OK Computer (1997) [FLAC]");
        assert_eq!(parsed.artist, "Radiohead");
        assert_eq!(parsed.title, "OK Computer");
        assert_eq!(parsed.year, Some(1997));
    }

    #[test]
    fn a_scene_style_name_with_dots_is_normalized_first() {
        let parsed = parse_release_name("Portishead.Dummy.1994.FLAC");
        assert_eq!(parsed.artist, "");
        assert!(parsed.title.starts_with("Portishead Dummy"), "{parsed:?}");
        assert_eq!(parsed.year, Some(1994));
    }

    #[test]
    fn an_unparseable_name_leaves_the_artist_empty_instead_of_guessing() {
        let parsed = parse_release_name("VA Turkish Psych Compilation");
        assert_eq!(parsed.artist, "", "bilinmeyen sanatçı uydurulmamalı");
        assert_eq!(parsed.title, "VA Turkish Psych Compilation");
    }

    #[test]
    fn an_album_that_is_only_a_number_survives_the_year_stripper() {
        let parsed = parse_release_name("Blur - 13");
        assert_eq!(parsed.artist, "Blur");
        assert_eq!(parsed.title, "13");
        assert_eq!(parsed.year, None, "13 bir yıl değil");
    }

    #[test]
    fn a_year_that_is_actually_the_album_title_is_still_taken_but_the_title_survives() {
        // "1984" bir albüm adı olabilir; yılı alsak bile başlık boş kalmamalı.
        let parsed = parse_release_name("Van Halen - 1984");
        assert_eq!(parsed.artist, "Van Halen");
        assert!(
            !parsed.title.is_empty(),
            "başlık boş bırakılmamalı: {parsed:?}"
        );
    }

    #[test]
    fn quality_tags_are_stripped_only_from_the_end() {
        let parsed = parse_release_name("Miles Davis - Kind of Blue [24bit] [Vinyl]");
        assert_eq!(parsed.title, "Kind of Blue");
    }

    #[test]
    fn a_parenthesis_that_belongs_to_the_title_is_kept() {
        let parsed = parse_release_name("Sigur Rós - ( )");
        assert_eq!(parsed.artist, "Sigur Rós");
        assert!(parsed.title.contains('('), "{parsed:?}");
    }

    #[test]
    fn a_dot_inside_a_spaced_title_is_not_touched() {
        let parsed = parse_release_name("Godspeed You! Black Emperor - F. A. Infinity");
        assert_eq!(parsed.artist, "Godspeed You! Black Emperor");
        assert_eq!(parsed.title, "F. A. Infinity");
    }

    #[test]
    fn a_track_file_name_yields_a_title_and_a_number() {
        assert_eq!(
            parse_file_name("01 - Airbag.flac"),
            ("Airbag".to_owned(), Some(1))
        );
        assert_eq!(
            parse_file_name("07.Karma.Police.mp3"),
            ("Karma Police".to_owned(), Some(7))
        );
    }

    #[test]
    fn a_numeric_title_is_not_eaten_by_the_track_number_rule() {
        assert_eq!(parse_file_name("13.flac"), ("13".to_owned(), None));
    }

    #[test]
    fn a_file_name_with_nothing_to_strip_is_returned_as_is() {
        assert_eq!(
            parse_file_name("Untitled Track.opus"),
            ("Untitled Track".to_owned(), None)
        );
    }

    #[test]
    fn audio_files_are_recognized_by_extension_case_insensitively() {
        assert!(is_audio(std::path::Path::new("a/b/Song.FLAC")));
        assert!(is_audio(std::path::Path::new("Song.opus")));
        assert!(!is_audio(std::path::Path::new("cover.jpg")));
        assert!(!is_audio(std::path::Path::new("readme")));
    }
}
