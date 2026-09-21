//! PNG rasterizasyonu (D-011: opsiyonel `render-png` feature).
//!
//! Çekirdek her zaman SVG üretir; bu modül yalnızca feature açıkken derlenir.

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};

use super::CardSize;

/// SVG'yi PNG olarak rasterize eder.
///
/// # Errors
/// SVG ayrıştırma, pixmap oluşturma veya PNG kodlama başarısız olursa.
#[allow(clippy::field_reassign_with_default)]
pub fn render(svg: &str, size: CardSize) -> Result<Vec<u8>> {
    // Sistem fontlarını yükle ve sans-serif için somut bir aile bağla.
    let mut fontdb = resvg::usvg::fontdb::Database::new();
    fontdb.load_system_fonts();

    // Font ailesi seç: tercih sırasıyla sistemde bulunan ilkini kullan.
    let sans = pick_sans(&fontdb);
    if let Some(ref family) = sans {
        fontdb.set_sans_serif_family(family);
    }
    let opts = {
        let mut o = resvg::usvg::Options::default();
        o.fontdb = std::sync::Arc::new(fontdb);
        o
    };

    let tree = resvg::usvg::Tree::from_str(svg, &opts).map_err(|e| {
        Error::new(
            Stage::SleeveRender,
            ErrorKind::CardRender {
                detail: format!("SVG ayrıştırılamadı: {e}"),
            },
        )
    })?;

    let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width, size.height).ok_or_else(|| {
        Error::new(
            Stage::SleeveRender,
            ErrorKind::CardRender {
                detail: format!("{}×{} pixmap oluşturulamadı", size.width, size.height),
            },
        )
    })?;

    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );

    pixmap.encode_png().map_err(|e| {
        Error::new(
            Stage::SleeveRender,
            ErrorKind::CardRender {
                detail: format!("PNG kodlanamadı: {e}"),
            },
        )
    })
}

/// Sistemde var olan bir sans-serif font ailesini bulur.
fn pick_sans(db: &resvg::usvg::fontdb::Database) -> Option<String> {
    let preferred = [
        "DejaVu Sans",
        "Noto Sans",
        "Liberation Sans",
        "Arial",
        "Helvetica",
        "Segoe UI",
        "Tahoma",
        "Verdana",
    ];
    for name in preferred {
        let q = resvg::usvg::fontdb::Query {
            families: &[resvg::usvg::fontdb::Family::Name(name)],
            weight: resvg::usvg::fontdb::Weight::NORMAL,
            style: resvg::usvg::fontdb::Style::Normal,
            stretch: resvg::usvg::fontdb::Stretch::Normal,
        };
        if db.query(&q).is_some() {
            return Some(name.to_owned());
        }
    }
    // Son çare: herhangi bir yüzeyin ilk ailesini kullan.
    db.faces()
        .next()
        .and_then(|face| face.families.first().map(|(s, _)| s.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ExportKind, Listen, ListenSource, TrackRef};
    use crate::sleeve::{CardSize, card_data};
    use crate::stats::{self, StatsQuery};

    fn listen(artist: &str, title: &str, ts: &str, ms: u64) -> Listen {
        Listen {
            track: TrackRef::new(artist, title),
            played_at: ts.parse().unwrap(),
            ms_played: ms,
            source: ListenSource::Import {
                export: ExportKind::SpotifyExtended,
            },
            canonical_id: None,
        }
    }

    #[test]
    fn png_round_trip() {
        let listens = vec![
            listen("Radiohead", "Creep", "2024-01-01T10:00:00Z", 200_000),
            listen("Portishead", "Roads", "2024-02-01T10:00:00Z", 300_000),
        ];
        let report = stats::compute(&listens, StatsQuery::default());
        let data = card_data(&report, &listens);
        let size = CardSize::square();

        let svg = crate::sleeve::render_svg(&data, size);
        let png = render(&svg, size).expect("png üretilemedi");
        // PNG başlık kontrolü.
        assert_eq!(&png[0..8], b"\x89PNG\r\n\x1a\n");
    }
}
