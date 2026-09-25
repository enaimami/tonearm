//! Producing the shareable Sleeve card (Phase 0.5).
//!
//! It lives in the core; the CLI only drives it. The GUI and mobile call the
//! same generator — code written here is not written three times (the Golden
//! Rule).
//!
//! ## Layers
//!
//! - [`data`] — produces the card data from raw listens and statistics.
//! - [`svg`] — draws the data as SVG. Always compiled.
//! - [`png`] — *optional* PNG rasterisation (the `render-png` feature).
//!
//! ## Usage
//!
//! ```ignore
//! let report = stats::compute(&listens, query);
//! let data = sleeve::card_data(&report, &listens);
//! let svg = sleeve::render_svg(&data, CardSize::square());
//!
//! #[cfg(feature = "render-png")]
//! let png = sleeve::render_png(&svg)?;
//! ```

mod data;
#[cfg(feature = "render-png")]
mod png;
mod svg;

pub use data::{Discovery, SleeveData, card_data};
#[cfg(feature = "render-png")]
pub use png::render as render_png;
pub use svg::render as render_svg;

use std::path::Path;

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};

/// The card's dimensions. PLAN 0.5.2: sizes are parameters, not embedded
/// constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CardSize {
    /// The width in pixels.
    pub width: u32,
    /// The height in pixels.
    pub height: u32,
}

impl CardSize {
    /// Creates a new size.
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// The square format (feed) — 1080×1080.
    #[must_use]
    pub const fn square() -> Self {
        Self {
            width: 1080,
            height: 1080,
        }
    }

    /// The vertical format (story) — 1080×1920.
    #[must_use]
    pub const fn story() -> Self {
        Self {
            width: 1080,
            height: 1920,
        }
    }
}

/// The ready-made format options (for the CLI argument).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardPreset {
    Square,
    Story,
}

impl CardPreset {
    /// Returns the size matching the option.
    #[must_use]
    pub fn size(self) -> CardSize {
        match self {
            Self::Square => CardSize::square(),
            Self::Story => CardSize::story(),
        }
    }
}

impl std::fmt::Display for CardFileKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Svg => write!(f, "svg"),
            Self::Png => write!(f, "png"),
        }
    }
}

impl std::fmt::Display for CardPreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Square => write!(f, "square"),
            Self::Story => write!(f, "story"),
        }
    }
}

/// The format of the card file written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CardFileKind {
    Svg,
    Png,
}

/// Writes the card to a file.
///
/// Decides the format from the extension: `.svg` → SVG (always), `.png` →
/// PNG (the `render-png` feature must be on).
///
/// # Errors
/// If the format is not recognised, PNG is asked for with the feature off,
/// rasterisation fails or the file cannot be written.
pub fn write_card(data: &SleeveData, size: CardSize, path: &Path) -> Result<(CardFileKind, u64)> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase());

    match ext.as_deref() {
        Some("svg") => {
            let svg = svg::render(data, size);
            let bytes = svg.len() as u64;
            std::fs::write(path, &svg).map_err(|e| {
                Error::new(
                    Stage::SleeveRender,
                    ErrorKind::Io {
                        path: path.to_owned(),
                        source: e,
                    },
                )
            })?;
            Ok((CardFileKind::Svg, bytes))
        }
        Some("png") => {
            #[cfg(feature = "render-png")]
            {
                let svg = svg::render(data, size);
                let bytes = png::render(&svg, size)?;
                std::fs::write(path, &bytes).map_err(|e| {
                    Error::new(
                        Stage::SleeveRender,
                        ErrorKind::Io {
                            path: path.to_owned(),
                            source: e,
                        },
                    )
                })?;
                Ok((CardFileKind::Png, bytes.len() as u64))
            }
            #[cfg(not(feature = "render-png"))]
            {
                Err(Error::new(
                    Stage::SleeveRender,
                    ErrorKind::InvalidInput {
                        detail: "PNG output is not in this build (the render-png feature is off); use SVG".to_owned(),
                    },
                ))
            }
        }
        Some(ext) => Err(Error::new(
            Stage::SleeveRender,
            ErrorKind::InvalidInput {
                detail: format!("unsupported extension: .{ext} (use svg or png)"),
            },
        )),
        None => Err(Error::new(
            Stage::SleeveRender,
            ErrorKind::InvalidInput {
                detail: "the output file has no extension (use svg or png)".to_owned(),
            },
        )),
    }
}
