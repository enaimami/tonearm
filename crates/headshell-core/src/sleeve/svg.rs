//! Kartın SVG çizimi.
//!
//! D-012: tasarım sabit, renkler ve ölçüler isimli sabitler. Ölçüler
//! [`CardSize`] ile parametredir; iki hazır ölçü (kare, dikey) modül
//! kökünde. Tema sistemi (Faz 3) geldiğinde `palette`/`metrics` token
//! setine taşınır — dış sözleşme açılmaz.
//!
//! Metin genişliği tahminidir (kaba karakter ölçüsü); taşan isimler
//! kırılır. Piksel noktası doğruluğu değil, paylaşılabilir kalite hedefi.

use super::CardSize;
use super::data::SleeveData;

/// Renkler. Koyu zemin, tek vurgu rengi.
mod palette {
    pub const BG: &str = "#0f1218";
    pub const TEXT: &str = "#f4f6fa";
    pub const DIM: &str = "#8b94a3";
    pub const ACCENT: &str = "#ffb454";
    pub const BAR_TRACK: &str = "#262c36";
}

/// Ölçüler — 1080 tasarım genişliği referansıyla. Kare ve dikey kart
/// aynı genişlikte olduğu için font boyutları sabittir; yalnızca dikey
/// paylar ve bölüm limitleri ölçüye göre değişir.
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
    /// Metin genişliği tahmini için ortalama karakter katsayısı.
    pub const CHAR_W: f64 = 0.60;
}

/// Bölümler arasındaki en küçük nefes payı. Sığdırma döngüsü bunu garanti eder.
const MIN_GAP: f64 = 22.0;

/// Türkçe ay adları — bağımlılık yok, kart dili projenin dili.
const MONTHS_TR: [&str; 12] = [
    "Ocak", "Şubat", "Mart", "Nisan", "Mayıs", "Haziran", "Temmuz", "Ağustos", "Eylül", "Ekim",
    "Kasım", "Aralık",
];

/// Kartı SVG olarak çizer.
#[must_use]
pub fn render(data: &SleeveData, size: CardSize) -> String {
    let w = f64::from(size.width);
    let h = f64::from(size.height);
    let content_w = w - 2.0 * metrics::MARGIN;

    // Dikey kartta daha çok yer var. Kare kart `h == w` olduğu için karşılaştırma
    // **kesin** olmalı — `>=` kare kartı da uzun sayıp içeriği taşırıyordu.
    let tall = h > w;
    let max_disc = if tall { 5 } else { 3 };
    let bar_area_wanted = if tall { 260.0 } else { 120.0 };

    let show_tops =
        data.top_artist.is_some() || data.top_track.is_some() || data.top_album.is_some();
    let show_bars = data.by_year.len() >= 2;

    // Yükseklikler baseline'a göre ölçülür; bir bloğun son satırının altında
    // kalan descender payı sayılmazsa üst bloklar bitişik görünür.
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
    // Alt bilgi kartın dibine sabitlenir; akış onun üstünde biter.
    let available = h - 2.0 * metrics::MARGIN - h_footer;

    // Keşif satırı sayısını ve bar yüksekliğini **sığana kadar** kıs.
    // Taşma sessizce görsel bozulma üretiyordu; burada ölçüp kısıyoruz.
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
            // En az bu kadar nefes payı olmalı; yoksa bölümler bitişir.
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
    // Dikey kartta bölüm sayısı aynı ama alan çok daha fazla; üst sınır
    // kare kart için kalırsa story kartının alt üçte biri boş kalıyordu.
    let max_gap = if tall { 190.0 } else { 90.0 };
    let gap = ((available - content) / gaps as f64).clamp(MIN_GAP, max_gap);

    let mut svg = Svg::new(w, h);
    // İçerik üst kenardan başlar; artan boşluk bölümlere dağıtıldığı için
    // ayrıca ortalamıyoruz (ortalamak alt bilgiyle çakışma riski doğuruyordu).
    let mut y = metrics::MARGIN;
    let step = |y: &mut f64, height: f64| {
        *y += height + gap;
    };

    // — Üst bilgi: marka solda, dönem sağda.
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

    // — Ana sayı.
    if data.plays == 0 {
        svg.text(
            metrics::MARGIN,
            y + metrics::HERO,
            metrics::HERO,
            false,
            palette::DIM,
            "henüz dinleme yok",
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
                "{} çalma · {} parça · {} sanatçı",
                fmt_thousands(data.plays),
                fmt_thousands(data.unique_tracks),
                fmt_thousands(data.unique_artists)
            ),
        );
        step(&mut y, h_hero);
    }

    // — En çok dinlenenler.
    if let Some(height) = h_tops {
        svg.section_header(y, "EN ÇOK DİNLEDİKLERİN");
        let mut row_y = y + metrics::SECTION + 10.0;
        let rows: [(&str, Option<String>, Option<usize>); 3] = [
            (
                "sanatçı",
                data.top_artist.as_ref().map(|a| a.artist.clone()),
                data.top_artist.as_ref().map(|a| a.plays),
            ),
            (
                "parça",
                data.top_track
                    .as_ref()
                    .map(|t| format!("{} — {}", t.title, t.artist)),
                data.top_track.as_ref().map(|t| t.plays),
            ),
            (
                "albüm",
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
            // Tür etiketi ismin önünde küçük ve soluk.
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
                    &format!("{} çalma", fmt_thousands(plays)),
                );
            }
            row_y += metrics::VALUE + 18.0;
        }
        step(&mut y, height);
    }

    // — Keşifler.
    if let Some(height) = h_disc {
        let header = match data.year {
            Some(_) => "BU YIL KEŞFETTİKLERİN",
            None => "İLK DİNLEDİĞİNDE",
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
                &format!("{} çalma", fmt_thousands(discovery.plays_in_scope)),
            );
            row_y += metrics::DISCOVERY + 12.0;
        }
        step(&mut y, height);
    }

    // — Yıllara göre zaman çizelgesi.
    if let Some(height) = h_bars {
        svg.section_header(y, "YILLARA GÖRE");
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
            // Çok yıl varsa etiket seyrekleştir: en fazla ~12 etiket.
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

    // — Alt bilgi: arşivin yaşı. Sağlayıcının 12 aylık hafızasının
    //   yapamadığını burada görünür kılıyoruz (D-004).
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

/// Dönem etiketi.
fn period_label(year: Option<i16>) -> String {
    year.map_or_else(|| "tüm zamanlar".to_owned(), |y| y.to_string())
}

/// Ana sayı: saat; 1 saatin altındakiler dakikaya iner.
fn hero_value(total_ms: u64) -> (String, &'static str) {
    let hours = total_ms / 3_600_000;
    if hours > 0 {
        (
            fmt_thousands(usize::try_from(hours).unwrap_or(0)),
            "saat dinleme",
        )
    } else {
        (
            fmt_thousands(usize::try_from(total_ms / 60_000).unwrap_or(0)),
            "dakika dinleme",
        )
    }
}

/// Alt bilgi metni: "12 Mart 2019'dan beri (8 yıl) · headshell".
fn footer_text(data: &SleeveData) -> String {
    let span = match (data.archive_start_year, data.archive_end_year) {
        (Some(start), Some(end)) => {
            let years = end.saturating_sub(start).saturating_add(1);
            let years_label = if years > 1 {
                format!(" ({years} yıl)")
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
                        MONTHS_TR[(zoned.month() as usize).saturating_sub(1).min(11)],
                        zoned.year()
                    )
                },
            );
            format!("{date}'dan beri{years_label}")
        }
        _ => "headshell".to_owned(),
    };
    if span == "headshell" {
        span
    } else {
        format!("{span} · headshell")
    }
}

/// Binlik ayraçlı sayı (Türkçe: nokta).
fn fmt_thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push('.');
        }
        out.push(ch);
    }
    out
}

/// Verilen piksel genişliğine sığan yaklaşık karakter sayısı.
fn max_chars(width_px: f64, font_size: f64) -> usize {
    (width_px / (metrics::CHAR_W * font_size)).max(4.0) as usize
}

/// Unicode karakter sınırıyla kırpar; kesme yerine üç nokta ekler.
fn truncate(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_owned();
    }
    let kept: String = input.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// XML metin kaçışı: `& < > " '`.
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

/// Basit SVG biriktirici.
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
        //-boyut öznitelikleri zaten kökte; bu işaretleyici yalnızca okunurluk için değil,
        //hata ayıklamada hızlı doğrulama içindir.
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
            track: TrackRef::new(artist, title).with_album(Some(format!("{title} albümü"))),
            played_at: ts.parse().expect("test zaman damgası geçerli"),
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
        // Birkaç yıl, zaman çizelgesi dolusun.
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
        assert!(svg.contains("dinleme"), "{svg}");
        assert!(svg.contains("EN ÇOK DİNLEDİKLERİN"), "{svg}");
        assert!(svg.contains("Radiohead"), "{svg}");
        assert!(svg.contains("YILLARA GÖRE"), "{svg}");
        assert!(svg.contains("tüm zamanlar"), "{svg}");
        // Kesme işareti XML'de &apos; olarak kaçılır.
        assert!(svg.contains("2019&apos;dan beri"), "{svg}");
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
        assert!(svg.contains("BU YIL KEŞFETTİKLERİN"), "{svg}");
        // Tek yıllık kapsam → bar bölümü yok.
        assert!(!svg.contains("YILLARA GÖRE"), "{svg}");
    }

    #[test]
    fn special_characters_are_escaped() {
        // Sanatçıyı **en çok dinlenen** yap: adın kartta basıldığı garanti
        // olsun. (Keşif listesi sığmaya göre kısalabilir; test ona bel bağlamaz.)
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
        assert!(svg.contains("henüz dinleme yok"), "{svg}");
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

    /// SVG'deki bütün `y` koordinatlarının en büyüğü — taşma ölçüsü.
    fn lowest_drawn_y(svg: &str) -> f64 {
        let mut lowest = 0.0f64;
        for line in svg.lines() {
            // <text y="..."> tabandır; <rect y="..."> + height alt kenardır.
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
        // Kart taşarsa "açıklamasız paylaşılabilir" ölçütü düşer: bar bölümü
        // alt bilginin üstüne biniyordu. Sığdırma döngüsü bunu engelliyor.
        for size in [CardSize::square(), CardSize::story()] {
            let svg = render(&full_data(), size);
            let lowest = lowest_drawn_y(&svg);
            assert!(
                lowest <= f64::from(size.height),
                "{}×{} kartında içerik {lowest:.0}px'e kadar taşıyor",
                size.width,
                size.height
            );
        }
    }

    #[test]
    fn a_long_archive_still_fits_the_square_card() {
        // En sıkışık durum: çok yıl + çok keşif + kare kart.
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
            "19 yıllık arşiv kare karta sığmalı:\n{svg}"
        );
    }

    #[test]
    fn thousands_separator_is_a_dot() {
        assert_eq!(fmt_thousands(0), "0");
        assert_eq!(fmt_thousands(999), "999");
        assert_eq!(fmt_thousands(1_000), "1.000");
        assert_eq!(fmt_thousands(1_234_567), "1.234.567");
    }

    #[test]
    fn hero_value_switches_to_minutes_below_an_hour() {
        assert_eq!(hero_value(0), ("0".to_owned(), "dakika dinleme"));
        assert_eq!(hero_value(59 * 60_000), ("59".to_owned(), "dakika dinleme"));
        assert_eq!(hero_value(3_600_000), ("1".to_owned(), "saat dinleme"));
    }

    #[test]
    fn truncation_respects_char_boundaries() {
        assert_eq!(truncate("Şebnem Ferah", 40), "Şebnem Ferah");
        assert_eq!(truncate("Şebnem Ferah", 7), "Şebnem…");
    }
}
