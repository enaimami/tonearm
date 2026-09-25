//! The card's SVG drawing.
//!
//! D-012: the design is fixed, colours and sizes are named constants. Sizes
//! are a parameter through [`CardSize`]; the two ready-made sizes (square,
//! vertical) are at the module root. When the theme system (Phase 3) arrives,
//! `palette`/`metrics` move into a token set — the external contract is not
//! opened up.
//!
//! Text width is an estimate (a rough character measure); names that overflow
//! are cut. The goal is shareable quality, not pixel-perfect accuracy.

use super::CardSize;
use super::data::SleeveData;

/// Colours. A dark background, a single accent colour.
mod palette {
    pub const BG: &str = "#0f1218";
    pub const TEXT: &str = "#f4f6fa";
    pub const DIM: &str = "#8b94a3";
    pub const ACCENT: &str = "#ffb454";
    pub const BAR_TRACK: &str = "#262c36";
}

/// Sizes — relative to a 1080 design width. Since the square and the vertical
/// card have the same width the font sizes are fixed; only the vertical
/// margins and the section limits change with the size.
mod metrics {
    pub const MARGIN: f64 = 72.0;
    pub const TITLE: f64 = 34.0;
    pub const HERO: f64 = 96.0;
    pub const SUB: f64 = 34.0;
    pub const SECTION: f64 = 28.0;
    pub const VALUE: f64 = 44.0;
    pub const COUNT: f64 = 32.0;
    pub const DISCOVERY: f64 = 38.0;
    pub const BAR_LABEL: f64 = 22.0;
    pub const FOOTER: f64 = 28.0;
    /// The average character factor for estimating text width.
    pub const CHAR_W: f64 = 0.60;
}

/// The smallest breathing room between sections. The fitting loop guarantees
/// it.
const MIN_GAP: f64 = 22.0;

/// English month names — no dependency; the card's language is the project's
/// language.
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Draws the card as SVG.
#[must_use]
pub fn render(data: &SleeveData, size: CardSize) -> String {
    let w = f64::from(size.width);
    let h = f64::from(size.height);
    let content_w = w - 2.0 * metrics::MARGIN;

    // The vertical card has more room. Since a square card has `h == w`, the
    // comparison must be **strict** — `>=` counted the square card as tall too
    // and made its content overflow.
    let tall = h > w;
    let max_disc = if tall { 5 } else { 3 };
    let bar_area_wanted = if tall { 260.0 } else { 120.0 };

    let show_tops =
        data.top_artist.is_some() || data.top_track.is_some() || data.top_album.is_some();
    let show_bars = data.by_year.len() >= 2;

    // Heights are measured from the baseline; if the descender room below a
    // block's last line is not counted, the blocks above look glued together.
    let desc = |font: f64| font * 0.3;
    let h_header = metrics::TITLE + 18.0;
    let h_hero = if data.plays == 0 {
        metrics::HERO + desc(metrics::HERO)
    } else {
        metrics::HERO + 16.0 + metrics::SUB + desc(metrics::SUB)
    };
    let h_tops = show_tops
        .then_some(metrics::SECTION + 10.0 + 3.0 * (metrics::VALUE + 18.0) + desc(metrics::VALUE));
    let h_footer = metrics::FOOTER + 12.0;
    // The footer is pinned to the bottom of the card; the flow ends above it.
    let available = h - 2.0 * metrics::MARGIN - h_footer;

    // **Shrink** the number of discovery rows and the bar height until it fits.
    // Overflow silently produced visual breakage; here we measure and shrink.
    let fixed = h_header + h_hero + h_tops.unwrap_or(0.0);
    let (disc_len, bar_area) = {
        let mut disc = data.discoveries.len().min(max_disc);
        let mut bars = bar_area_wanted;
        loop {
            let d = if disc > 0 {
                metrics::SECTION
                    + 10.0
                    + disc as f64 * (metrics::DISCOVERY + 12.0)
                    + desc(metrics::DISCOVERY)
            } else {
                0.0
            };
            let b = if show_bars {
                metrics::SECTION + 10.0 + bars + 10.0 + metrics::BAR_LABEL
            } else {
                0.0
            };
            let sections_n =
                2 + usize::from(h_tops.is_some()) + usize::from(disc > 0) + usize::from(show_bars);
            // There must be at least this much breathing room; otherwise the sections
            // touch.
            let min_gaps = MIN_GAP * (sections_n.saturating_sub(1)) as f64;
            if fixed + d + b + min_gaps <= available {
                break (disc, bars);
            }
            if disc > 0 {
                disc -= 1;
            } else if bars > 70.0 {
                bars -= 20.0;
            } else {
                break (disc, bars);
            }
        }
    };

    let show_disc = disc_len > 0;
    let h_disc = show_disc.then_some(
        metrics::SECTION
            + 10.0
            + disc_len as f64 * (metrics::DISCOVERY + 12.0)
            + desc(metrics::DISCOVERY),
    );
    let h_bars =
        show_bars.then_some(metrics::SECTION + 10.0 + bar_area + 10.0 + metrics::BAR_LABEL);

    let sections: [Option<f64>; 5] = [Some(h_header), Some(h_hero), h_tops, h_disc, h_bars];
    let content: f64 = sections.iter().filter_map(|s| *s).sum();
    let gap_count = sections.iter().filter(|s| s.is_some()).count();
    let gaps = gap_count.saturating_sub(1).max(1);
    // The vertical card has the same number of sections but far more room; if
    // the upper bound stayed at the square card's, the bottom third of the story
    // card was left empty.
    let max_gap = if tall { 190.0 } else { 90.0 };
    let gap = ((available - content) / gaps as f64).clamp(MIN_GAP, max_gap);

    let mut svg = Svg::new(w, h);
    // The content starts at the top edge; since the extra space is spread over
    // the sections we do not centre separately (centring risked colliding with
    // the footer).
    let mut y = metrics::MARGIN;
    let step = |y: &mut f64, height: f64| {
        *y += height + gap;
    };

    // — Header: the brand on the left, the period on the right.
    svg.text(
        metrics::MARGIN,
        y + metrics::TITLE,
        metrics::TITLE,
        true,
        palette::ACCENT,
        "headshell",
    );
    let period = period_label(data.year);
    svg.text_end(
        w - metrics::MARGIN,
        y + metrics::TITLE,
        metrics::TITLE,
        false,
        palette::DIM,
        &period,
    );
    step(&mut y, h_header);

    // — The hero number.
    if data.plays == 0 {
        svg.text(
            metrics::MARGIN,
            y + metrics::HERO,
            metrics::HERO,
            false,
            palette::DIM,
            "no listens yet",
        );
        step(&mut y, h_hero);
    } else {
        let (value, unit) = hero_value(data.total_ms_played);
        svg.text(
            metrics::MARGIN,
            y + metrics::HERO,
            metrics::HERO,
            true,
            palette::TEXT,
            &value,
        );
        let num_w = value.chars().count() as f64 * metrics::CHAR_W * metrics::HERO;
        svg.text(
            metrics::MARGIN + num_w + 20.0,
            y + metrics::HERO,
            metrics::SUB + 6.0,
            false,
            palette::ACCENT,
            unit,
        );
        svg.text(
            metrics::MARGIN,
            y + metrics::HERO + 16.0 + metrics::SUB,
            metrics::SUB,
            false,
            palette::DIM,
            &format!(
                "{} plays · {} tracks · {} artists",
                fmt_thousands(data.plays),
                fmt_thousands(data.unique_tracks),
                fmt_thousands(data.unique_artists)
            ),
        );
        step(&mut y, h_hero);
    }

    // — The most listened.
    if let Some(height) = h_tops {
        svg.section_header(y, "MOST PLAYED");
        let mut row_y = y + metrics::SECTION + 10.0;
        let rows: [(&str, Option<String>, Option<usize>); 3] = [
            (
                "artist",
                data.top_artist.as_ref().map(|a| a.artist.clone()),
                data.top_artist.as_ref().map(|a| a.plays),
            ),
            (
                "track",
                data.top_track
                    .as_ref()
                    .map(|t| format!("{} — {}", t.title, t.artist)),
                data.top_track.as_ref().map(|t| t.plays),
            ),
            (
                "album",
                data.top_album
                    .as_ref()
                    .map(|a| format!("{} — {}", a.album, a.artist)),
                data.top_album.as_ref().map(|a| a.plays),
            ),
        ];
        for (label, name, plays) in rows {
            let Some(name) = name else {
                continue;
            };
            let dy = row_y + metrics::VALUE;
            // The kind label is small and faint in front of the name.
            svg.text(
                metrics::MARGIN,
                dy,
                metrics::COUNT,
                false,
                palette::DIM,
                label,
            );
            let label_w = label.chars().count() as f64 * metrics::CHAR_W * metrics::COUNT + 24.0;
            svg.text(
                metrics::MARGIN + label_w,
                dy,
                metrics::VALUE,
                true,
                palette::TEXT,
                &truncate(
                    &name,
                    max_chars(content_w - label_w - 180.0, metrics::VALUE),
                ),
            );
            if let Some(plays) = plays {
                svg.text_end(
                    w - metrics::MARGIN,
                    dy,
                    metrics::COUNT,
                    false,
                    palette::DIM,
                    &format!("{} plays", fmt_thousands(plays)),
                );
            }
            row_y += metrics::VALUE + 18.0;
        }
        step(&mut y, height);
    }

    // — Discoveries.
    if let Some(height) = h_disc {
        let header = match data.year {
            Some(_) => "DISCOVERED THIS YEAR",
            None => "FIRST LISTENED",
        };
        svg.section_header(y, header);
        let mut row_y = y + metrics::SECTION + 10.0;
        for discovery in data.discoveries.iter().take(disc_len) {
            let dy = row_y + metrics::DISCOVERY;
            svg.text(
                metrics::MARGIN,
                dy,
                metrics::DISCOVERY,
                true,
                palette::ACCENT,
                &discovery.first_year.to_string(),
            );
            let year_w = 4.0 * metrics::CHAR_W * metrics::DISCOVERY + 24.0;
            svg.text(
                metrics::MARGIN + year_w,
                dy,
                metrics::DISCOVERY,
                false,
                palette::TEXT,
                &truncate(
                    &discovery.artist,
                    max_chars(content_w - year_w, metrics::DISCOVERY),
                ),
            );
            svg.text_end(
                w - metrics::MARGIN,
                dy,
                metrics::COUNT,
                false,
                palette::DIM,
                &format!("{} plays", fmt_thousands(discovery.plays_in_scope)),
            );
            row_y += metrics::DISCOVERY + 12.0;
        }
        step(&mut y, height);
    }

    // — The timeline by year.
    if let Some(height) = h_bars {
        svg.section_header(y, "BY YEAR");
        let top = y + metrics::SECTION + 10.0;
        let max_plays = data.by_year.iter().map(|s| s.plays).max().unwrap_or(1);
        let n = data.by_year.len();
        let bar_gap = 10.0;
        let bar_w = ((content_w - bar_gap * (n as f64 - 1.0)) / n as f64).max(6.0);
        for (index, stat) in data.by_year.iter().enumerate() {
            let x = metrics::MARGIN + index as f64 * (bar_w + bar_gap);
            let filled = (bar_area * stat.plays as f64 / max_plays.max(1) as f64).max(6.0);
            svg.rect(x, top + bar_area - filled, bar_w, filled, palette::ACCENT);
            svg.rect(x, top + bar_area, bar_w, 2.0, palette::BAR_TRACK);
            // With many years, thin out the labels: at most ~12 labels.
            let label_every = n.div_ceil(12);
            if index % label_every == 0 || index == n - 1 {
                svg.text_middle(
                    x + bar_w / 2.0,
                    top + bar_area + 10.0 + metrics::BAR_LABEL,
                    metrics::BAR_LABEL,
                    false,
                    palette::DIM,
                    &stat.year.to_string(),
                );
            }
        }
        step(&mut y, height);
    }

    // — Footer: the archive's age. Here we make visible what the provider's
    //   12-month memory cannot do (D-004).
    let footer = footer_text(data);
    svg.text(
        metrics::MARGIN,
        h - metrics::MARGIN,
        metrics::FOOTER,
        false,
        palette::DIM,
        &footer,
    );

    svg.finish()
}

/// The period label.
fn period_label(year: Option<i16>) -> String {
    year.map_or_else(|| "all time".to_owned(), |y| y.to_string())
}

/// The hero number: hours; below 1 hour it drops to minutes.
fn hero_value(total_ms: u64) -> (String, &'static str) {
    let hours = total_ms / 3_600_000;
    if hours > 0 {
        (
            fmt_thousands(usize::try_from(hours).unwrap_or(0)),
            "hours listened",
        )
    } else {
        (
            fmt_thousands(usize::try_from(total_ms / 60_000).unwrap_or(0)),
            "minutes listened",
        )
    }
}

/// The footer text: "since 12 March 2019 (8 years) · headshell".
fn footer_text(data: &SleeveData) -> String {
    let span = match (data.archive_start_year, data.archive_end_year) {
        (Some(start), Some(end)) => {
            let years = end.saturating_sub(start).saturating_add(1);
            let years_label = if years > 1 {
                format!(" ({years} years)")
            } else {
                String::new()
            };
            let date = data.first_listen.as_ref().map_or_else(
                || start.to_string(),
                |ts| {
                    let zoned = ts.to_zoned(jiff::tz::TimeZone::UTC);
                    format!(
                        "{} {} {}",
                        zoned.day(),
                        MONTHS[(zoned.month() as usize).saturating_sub(1).min(11)],
                        zoned.year()
                    )
                },
            );
            format!("since {date}{years_label}")
        }
        _ => "headshell".to_owned(),
    };
    if span == "headshell" {
        span
    } else {
        format!("{span} · headshell")
    }
}

/// A number with thousands separators (English: a comma).
fn fmt_thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// The approximate number of characters that fit in the given pixel width.
fn max_chars(width_px: f64, font_size: f64) -> usize {
    (width_px / (metrics::CHAR_W * font_size)).max(4.0) as usize
}

/// Cuts on a Unicode character boundary; adds an ellipsis in place of the
/// cut.
fn truncate(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_owned();
    }
    let kept: String = input.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// XML text escaping: `& < > " '`.
fn xml_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

/// A simple SVG accumulator.
struct Svg {
    out: String,
    w: f64,
    h: f64,
}

impl Svg {
    fn new(w: f64, h: f64) -> Self {
        let mut out = String::new();
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        out.push_str(&format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">\n"
        ));
        out.push_str(&format!(
            "<rect x=\"0\" y=\"0\" width=\"{w}\" height=\"{h}\" fill=\"{}\"/>\n",
            palette::BG
        ));
        Self { out, w, h }
    }

    fn text(&mut self, x: f64, baseline: f64, size: f64, bold: bool, fill: &str, content: &str) {
        let weight = if bold { "700" } else { "400" };
        self.out.push_str(&format!(
            "<text x=\"{x:.1}\" y=\"{baseline:.1}\" font-size=\"{size:.0}\" font-weight=\"{weight}\" fill=\"{fill}\" font-family=\"sans-serif\">{}</text>\n",
            xml_escape(content)
        ));
    }

    fn text_end(
        &mut self,
        x: f64,
        baseline: f64,
        size: f64,
        bold: bool,
        fill: &str,
        content: &str,
    ) {
        let weight = if bold { "700" } else { "400" };
        self.out.push_str(&format!(
            "<text x=\"{x:.1}\" y=\"{baseline:.1}\" font-size=\"{size:.0}\" font-weight=\"{weight}\" fill=\"{fill}\" text-anchor=\"end\" font-family=\"sans-serif\">{}</text>\n",
            xml_escape(content)
        ));
    }

    fn text_middle(
        &mut self,
        x: f64,
        baseline: f64,
        size: f64,
        bold: bool,
        fill: &str,
        content: &str,
    ) {
        let weight = if bold { "700" } else { "400" };
        self.out.push_str(&format!(
            "<text x=\"{x:.1}\" y=\"{baseline:.1}\" font-size=\"{size:.0}\" font-weight=\"{weight}\" fill=\"{fill}\" text-anchor=\"middle\" font-family=\"sans-serif\">{}</text>\n",
            xml_escape(content)
        ));
    }

    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64, fill: &str) {
        self.out.push_str(&format!(
            "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{w:.1}\" height=\"{h:.1}\" fill=\"{fill}\"/>\n"
        ));
    }

    fn section_header(&mut self, y: f64, title: &str) {
        self.text(
            metrics::MARGIN,
            y + metrics::SECTION,
            metrics::SECTION,
            true,
            palette::DIM,
            title,
        );
    }

    fn finish(self) -> String {
        let Self { mut out, w, h } = self;
        out.push_str("</svg>\n");
        // The size attributes are already on the root; this marker is not only for
        // readability but for quick verification while debugging.
        let _ = (w, h);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::super::CardSize;
    use super::super::data::card_data;
    use super::*;
    use crate::model::{ExportKind, Listen, ListenSource, TrackRef};
    use crate::stats::{self, StatsQuery};

    fn listen(artist: &str, title: &str, ts: &str, ms: u64) -> Listen {
        Listen {
            track: TrackRef::new(artist, title).with_album(Some(format!("{title} album"))),
            played_at: ts.parse().expect("the test timestamp is valid"),
            ms_played: ms,
            source: ListenSource::Import {
                export: ExportKind::SpotifyExtended,
            },
            canonical_id: None,
        }
    }

    fn archive() -> Vec<Listen> {
        let mut listens = vec![
            listen("Radiohead", "Creep", "2019-06-01T10:00:00Z", 200_000),
            listen("Radiohead", "Karma Police", "2019-08-01T10:00:00Z", 260_000),
        ];
        // A few years, so the timeline fills up.
        for year in 2020..=2026 {
            listens.push(listen(
                "Portishead",
                "Roads",
                &format!("{year}-02-01T10:00:00Z"),
                300_000,
            ));
        }
        listens.push(listen("Sault", "9", "2026-03-01T10:00:00Z", 210_000));
        listens
    }

    fn full_data() -> SleeveData {
        let report = stats::compute(&archive(), StatsQuery::default());
        card_data(&report, &archive())
    }

    #[test]
    fn card_contains_the_story_numbers() {
        let svg = render(&full_data(), CardSize::square());
        assert!(svg.contains("listened"), "{svg}");
        assert!(svg.contains("MOST PLAYED"), "{svg}");
        assert!(svg.contains("Radiohead"), "{svg}");
        assert!(svg.contains("BY YEAR"), "{svg}");
        assert!(svg.contains("all time"), "{svg}");
        // The footer tells how old the archive is.
        assert!(svg.contains("since ") && svg.contains(" 2019 ("), "{svg}");
    }

    #[test]
    fn year_card_changes_period_and_discovery_header() {
        let report = stats::compute(
            &archive(),
            StatsQuery {
                year: Some(2026),
                ..StatsQuery::default()
            },
        );
        let data = card_data(&report, &archive());
        let svg = render(&data, CardSize::square());
        assert!(svg.contains("2026"), "{svg}");
        assert!(svg.contains("DISCOVERED THIS YEAR"), "{svg}");
        // A single-year scope → no bar section.
        assert!(!svg.contains("BY YEAR"), "{svg}");
    }

    #[test]
    fn special_characters_are_escaped() {
        // Make the artist **the most listened**: so its name is guaranteed to be
        // printed on the card. (The discovery list can shrink to fit; the test does
        // not rely on it.)
        let mut listens = archive();
        for day in 1..=9 {
            listens.push(listen(
                "AC&DC <Tribute>",
                "\"Back\" in 'Black'",
                &format!("2026-04-0{day}T10:00:00Z"),
                300_000,
            ));
        }
        let report = stats::compute(&listens, StatsQuery::default());
        let data = card_data(&report, &listens);
        let svg = render(&data, CardSize::square());
        assert!(svg.contains("AC&amp;DC"), "{svg}");
        assert!(svg.contains("&lt;Tribute&gt;"), "{svg}");
        assert!(!svg.contains("<Tribute>"), "{svg}");
        assert!(svg.contains("&quot;Back&quot;"), "{svg}");
    }

    #[test]
    fn empty_library_renders_a_valid_card() {
        let report = stats::compute(&[], StatsQuery::default());
        let data = card_data(&report, &[]);
        let svg = render(&data, CardSize::story());
        assert!(svg.contains("no listens yet"), "{svg}");
        assert!(svg.contains("</svg>"), "{svg}");
    }

    #[test]
    fn story_and_square_have_the_declared_dimensions() {
        let data = full_data();
        for (size, w, h) in [
            (CardSize::square(), 1080, 1080),
            (CardSize::story(), 1080, 1920),
        ] {
            let svg = render(&data, size);
            assert!(
                svg.contains(&format!("width=\"{w}\" height=\"{h}\"")),
                "{svg}"
            );
        }
    }

    /// The largest of all the `y` coordinates in the SVG — the overflow measure.
    fn lowest_drawn_y(svg: &str) -> f64 {
        let mut lowest = 0.0f64;
        for line in svg.lines() {
            // <text y="..."> is the baseline; <rect y="..."> + height is the bottom
            // edge.
            let y = line
                .split("y=\"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .and_then(|v| v.parse::<f64>().ok());
            let Some(y) = y else { continue };
            let bottom = if line.starts_with("<rect") {
                let height = line
                    .split("height=\"")
                    .nth(1)
                    .and_then(|rest| rest.split('"').next())
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(0.0);
                y + height
            } else {
                y
            };
            lowest = lowest.max(bottom);
        }
        lowest
    }

    #[test]
    fn content_never_spills_past_the_card() {
        // If the card overflows, the "shareable without explanation" criterion
        // fails: the bar section was riding over the footer. The fitting loop
        // prevents this.
        for size in [CardSize::square(), CardSize::story()] {
            let svg = render(&full_data(), size);
            let lowest = lowest_drawn_y(&svg);
            assert!(
                lowest <= f64::from(size.height),
                "on the {}×{} card the content overflows down to {lowest:.0}px",
                size.width,
                size.height
            );
        }
    }

    #[test]
    fn a_long_archive_still_fits_the_square_card() {
        // The tightest case: many years + many discoveries + a square card.
        let mut listens = Vec::new();
        for year in 2008..=2026 {
            for index in 0..3 {
                listens.push(listen(
                    &format!("Artist {year}-{index}"),
                    "Track",
                    &format!("{year}-0{}-01T10:00:00Z", index + 1),
                    200_000,
                ));
            }
        }
        let report = stats::compute(&listens, StatsQuery::default());
        let data = card_data(&report, &listens);
        let svg = render(&data, CardSize::square());
        assert!(
            lowest_drawn_y(&svg) <= 1080.0,
            "a 19-year archive must fit on a square card:\n{svg}"
        );
    }

    #[test]
    fn thousands_separator_is_a_comma() {
        assert_eq!(fmt_thousands(0), "0");
        assert_eq!(fmt_thousands(999), "999");
        assert_eq!(fmt_thousands(1_000), "1,000");
        assert_eq!(fmt_thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn hero_value_switches_to_minutes_below_an_hour() {
        assert_eq!(hero_value(0), ("0".to_owned(), "minutes listened"));
        assert_eq!(
            hero_value(59 * 60_000),
            ("59".to_owned(), "minutes listened")
        );
        assert_eq!(hero_value(3_600_000), ("1".to_owned(), "hours listened"));
    }

    #[test]
    fn truncation_respects_char_boundaries() {
        assert_eq!(truncate("Şebnem Ferah", 40), "Şebnem Ferah");
        assert_eq!(truncate("Şebnem Ferah", 7), "Şebnem…");
    }
}
