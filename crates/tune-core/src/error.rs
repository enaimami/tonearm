//! Tiplenmiş çekirdek hataları.
//!
//! Her hata bir [`Stage`] taşır — "nerede bozuldu?" sorusunun cevabı
//! hatanın kendisinde durur, log'da aranmaz.

use std::path::PathBuf;

use crate::diag::Stage;

/// Çekirdeğin döndürdüğü tek hata tipi.
#[derive(Debug, thiserror::Error)]
#[error("ADIM: {stage}")]
pub struct Error {
    stage: Stage,
    #[source]
    kind: ErrorKind,
}

impl Error {
    #[must_use]
    pub fn new(stage: Stage, kind: ErrorKind) -> Self {
        Self { stage, kind }
    }

    /// Hatanın oluştuğu aşama.
    #[must_use]
    pub fn stage(&self) -> Stage {
        self.stage
    }

    /// Hatanın türü.
    #[must_use]
    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }

    /// `ADIM: X` + neden zinciri, tek satırlarda. CLI ve GUI aynısını gösterir.
    #[must_use]
    pub fn chain_text(&self) -> String {
        let mut out = format!("ADIM: {}", self.stage);
        let mut current: Option<&dyn std::error::Error> = Some(&self.kind);
        while let Some(err) = current {
            out.push_str("\n  → ");
            out.push_str(&err.to_string());
            current = err.source();
        }
        out
    }
}

/// Ne bozuldu.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ErrorKind {
    #[error("dosya işlemi başarısız: {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("arşiv okunamadı: {path}")]
    Archive {
        path: PathBuf,
        #[source]
        source: zip::result::ZipError,
    },

    #[error("JSON ayrıştırılamadı: {entry}")]
    Json {
        /// Arşiv içi yol ya da dosya adı.
        entry: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("veritabanı hatası")]
    Database {
        #[source]
        source: rusqlite::Error,
    },

    #[error("tanınmayan export biçimi: {detail}")]
    UnsupportedExport { detail: String },

    #[error("{entry} içindeki {index}. kayıt bozuk: {detail}")]
    MalformedRecord {
        entry: String,
        index: usize,
        detail: String,
    },

    #[error("bulunamadı: {what}")]
    NotFound { what: String },

    #[error("geçersiz girdi: {detail}")]
    InvalidInput { detail: String },

    #[error("kart üretilemedi: {detail}")]
    CardRender { detail: String },
}

/// Çekirdek sonuç tipi.
pub type Result<T> = std::result::Result<T, Error>;

/// `io::Error`'ı yolu ve aşamasıyla birlikte sarar.
pub(crate) fn io_err(stage: Stage, path: impl Into<PathBuf>, source: std::io::Error) -> Error {
    Error::new(
        stage,
        ErrorKind::Io {
            path: path.into(),
            source,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_text_starts_with_stage_and_lists_causes() {
        let err = io_err(
            Stage::ImportRead,
            "/yok/dosya.zip",
            std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"),
        );
        let text = err.chain_text();
        assert!(text.starts_with("ADIM: IMPORT_READ"), "{text}");
        assert!(text.contains("/yok/dosya.zip"), "{text}");
        assert!(text.contains("no such file"), "{text}");
        assert_eq!(err.stage(), Stage::ImportRead);
    }
}
