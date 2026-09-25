//! Extracting artist/title from release and file names.
//!
//! This is a **guessing layer**, and it does not hide it. Torrent names have
//! no schema; the artist/title that comes out of here is not a canonical
//! identity but input for the identity chain (K6). The chain asks MusicBrainz
//! about it and corrects it with the fingerprint if needed — so a wrong guess
//! here is not an error the chain cannot fix.
//!
//! When we are not sure we **do not make things up**: the artist stays empty.
//! An empty field means "I don't know"; a wrong artist name ties the chain to
//! the wrong recording (the same as D-046's duration flaw).

/// Labels found in names that have nothing to do with the music. Only used for
/// the **trailing** parenthesis/bracket groups; we do not delete a word in the
/// middle of a name — so names like "Blur - 13" are not mangled.
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

/// Audio file extensions. These decide which files inside a torrent are
/// tracks.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "flac", "mp3", "ogg", "opus", "m4a", "aac", "wav", "wv", "ape", "alac", "aiff", "aif", "mpc",
    "dsf",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedName {
    /// An empty string means "I don't know" — not a made-up name.
    pub artist: String,
    pub title: String,
    pub year: Option<u16>,
}

/// Is the file path's extension an audio extension.
pub fn is_audio(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .is_some_and(|ext| AUDIO_EXTENSIONS.contains(&ext.as_str()))
}

/// Splits a release name into artist + album + year.
pub fn parse_release_name(raw: &str) -> ParsedName {
    let raw = raw.trim();
    // A name without spaces = "scene" spelling. Only then do we drop the bare
    // trailing words (FLAC, WEB…): in a name with spaces, "Web" may really be
    // part of the album name.
    let scene_style = !raw.contains(' ');
    let text = normalize_separators(raw);

    // **The order matters.** The artist is split off first: taking the year
    // first in "Van Halen - 1984" emptied the title and lost the artist too.
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

/// The track title and track number from a file name inside a torrent.
///
/// Returns `(title, track no)`. The title never comes back empty: if nothing
/// can be extracted it is the file name itself without its extension.
pub fn parse_file_name(file_name: &str) -> (String, Option<u32>) {
    let stem = file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem);
    let mut text = normalize_separators(stem).trim().to_owned();

    // Leading track numbers like "01", "01 -", "1.".
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
            // "13" alone can be a title; we only count it as a track number if
            // something follows it.
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

/// Turns names written with dots/underscores into names with spaces.
///
/// Only if the name has no spaces at all: "Artist.Name-Album.2007" is written
/// that way, but we must not touch the dot in "Godspeed You! Black Emperor -
/// F♯A♯∞".
fn normalize_separators(raw: &str) -> String {
    if raw.contains(' ') {
        return raw.to_owned();
    }
    raw.replace(['.', '_'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Takes a trailing `(1997)` / `[1997]` / ` 1997` year and deletes it from the
/// text.
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
        // A trailing year without parentheses: "Artist - Album 1997".
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

/// Cleans up trailing leftovers like `[FLAC]`, `(WEB)`, `-GROUP`.
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
        // An empty group (`( )`) is **not** noise: Sigur Rós's album is called that.
        // The claim "all of it is noise" only means something if there is at least
        // one word.
        if words.is_empty() || !words.iter().all(|word| NOISE.contains(word)) {
            break;
        }
        *text = trimmed[..start].trim_end().to_owned();
    }
    *text = text.trim().to_owned();
}

/// The bare trailing noise words of scene spelling: "… 1994 FLAC" → "… 1994".
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

/// The "Artist - Album" split. `None` without a separator — the artist **is
/// not made up**.
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
    // A scene name without spaces: "Artist-Album-2007-GROUP" already has spaces
    // after normalising; one last attempt for what is left with a single dash.
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
        assert_eq!(parsed.artist, "", "an unknown artist must not be made up");
        assert_eq!(parsed.title, "VA Turkish Psych Compilation");
    }

    #[test]
    fn an_album_that_is_only_a_number_survives_the_year_stripper() {
        let parsed = parse_release_name("Blur - 13");
        assert_eq!(parsed.artist, "Blur");
        assert_eq!(parsed.title, "13");
        assert_eq!(parsed.year, None, "13 is not a year");
    }

    #[test]
    fn a_year_that_is_actually_the_album_title_is_still_taken_but_the_title_survives() {
        // "1984" can be an album name; even if we take the year, the title must not
        // be left empty.
        let parsed = parse_release_name("Van Halen - 1984");
        assert_eq!(parsed.artist, "Van Halen");
        assert!(
            !parsed.title.is_empty(),
            "the title must not be left empty: {parsed:?}"
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
