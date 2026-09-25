//! PNG rasterisation (D-011: the optional `render-png` feature).
//!
//! The core always produces SVG; this module is only compiled when the
//! feature is on.

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};

use super::CardSize;

/// Rasterises SVG as PNG.
///
/// # Errors
/// If SVG parsing, pixmap creation or PNG encoding fails.
#[allow(clippy::field_reassign_with_default)]
pub fn render(svg: &str, size: CardSize) -> Result<Vec<u8>> {
    // Load the system fonts and bind a concrete family for sans-serif.
    let mut fontdb = resvg::usvg::fontdb::Database::new();
    fontdb.load_system_fonts();

    // Pick a font family: use the first one found on the system, in order of
    // preference.
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
                detail: format!("could not parse the SVG: {e}"),
            },
        )
    })?;

    let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width, size.height).ok_or_else(|| {
        Error::new(
            Stage::SleeveRender,
            ErrorKind::CardRender {
                detail: format!("could not create a {}×{} pixmap", size.width, size.height),
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
                detail: format!("could not encode the PNG: {e}"),
            },
        )
    })
}

/// Finds a sans-serif font family present on the system.
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
    // Last resort: use the first family of any face.
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
        let png = render(&svg, size).expect("could not produce the png");
        // The PNG header check.
        assert_eq!(&png[0..8], b"\x89PNG\r\n\x1a\n");
    }
}
