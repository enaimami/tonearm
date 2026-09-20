//! Paylaşılabilir Wrapped kartı üretimi (Faz 0.5).
//!
//! Çekirdekte yaşar; CLI yalnızca sürer. GUI ve mobil aynı
//! üreticiyi çağırır — burada yazılan kod üç kez yazılmaz (Altın Kural).
//!
//! ## Katmanlar
//!
//! - [`data`] — ham dinlemelerden ve istatistiklerden kart verisi üretir.
//! - [`svg`] — veriyi SVG olarak çizer. Koşulsuz derlenir.
//! - [`png`] — *opsiyonel* PNG rasterizasyonu (`render-png` feature'ı).
//!
//! ## Kullanım
//!
//! ```ignore
//! let report = stats::compute(&listens, query);
//! let data = wrapped::card_data(&report, &listens);
//! let svg = wrapped::render_svg(&data, CardSize::square());
//!
//! #[cfg(feature = "render-png")]
//! let png = wrapped::render_png(&svg)?;
//! ```

mod data;
#[cfg(feature = "render-png")]
mod png;
mod svg;

pub use data::{Discovery, WrappedData, card_data};
#[cfg(feature = "render-png")]
pub use png::render as render_png;
pub use svg::render as render_svg;

use std::path::Path;

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};

/// Kartın boyutları. PLAN 0.5.2: ölçüler parametre, gömülü sabit değil.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CardSize {
    /// Piksel cinsinden genişlik.
    pub width: u32,
    /// Piksel cinsinden yükseklik.
    pub height: u32,
}

impl CardSize {
    /// Yeni boyut oluşturur.
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// Kare biçim (feed) — 1080×1080.
    #[must_use]
    pub const fn square() -> Self {
        Self {
            width: 1080,
            height: 1080,
        }
    }

    /// Dikey biçim (story) — 1080×1920.
    #[must_use]
    pub const fn story() -> Self {
        Self {
            width: 1080,
            height: 1920,
        }
    }
}

/// Hazır biçim seçenekleri (CLI argümanı için).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardPreset {
    Square,
    Story,
}

impl CardPreset {
    /// Seçeneğe karşılık gelen boyutu döndürür.
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

/// Yazılan kart dosyasının biçimi.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CardFileKind {
    Svg,
    Png,
}

/// Kartı dosyaya yazar.
///
/// Uzantıya göre biçim kararı verir: `.svg` → SVG (koşulsuz),
/// `.png` → PNG (`render-png` feature'ı açık olmalı).
///
/// # Errors
/// Biçim tanınamazsa, PNG istenip feature kapalıysa, rasterizasyon başarısız
/// olursa veya dosya yazılamazsa.
pub fn write_card(data: &WrappedData, size: CardSize, path: &Path) -> Result<(CardFileKind, u64)> {
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
                    Stage::WrappedRender,
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
                        Stage::WrappedRender,
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
                    Stage::WrappedRender,
                    ErrorKind::InvalidInput {
                        detail: "PNG çıktısı bu derlemede yok (render-png feature'ı kapalı); SVG kullanın".to_owned(),
                    },
                ))
            }
        }
        Some(ext) => Err(Error::new(
            Stage::WrappedRender,
            ErrorKind::InvalidInput {
                detail: format!("desteklenmeyen uzantı: .{ext} (svg veya png kullanın)"),
            },
        )),
        None => Err(Error::new(
            Stage::WrappedRender,
            ErrorKind::InvalidInput {
                detail: "çıktı dosyasının uzantısı yok (svg veya png kullanın)".to_owned(),
            },
        )),
    }
}
