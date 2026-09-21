//! Veri export dosyalarından içe aktarma.
//!
//! **Değişmez kural #1:** içe aktarma sağlayıcı API'sinden değil, kullanıcının
//! GDPR taşınabilirlik hakkıyla indirdiği export dosyalarından yapılır.

pub mod archive;
pub mod spotify;

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};
use crate::model::{ExportKind, Listen};

pub use archive::{DirArchive, ExportArchive, MemoryArchive, ZipArchive};

/// Bir kaydın neden `Listen`'e dönüşmediği.
///
/// Sessizce düşürmek yok — her atlanan kayıt bir nedene sayılır ve raporlanır.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// Podcast/sesli kitap — müzik değil.
    NotMusic,
    /// Parça adı yok.
    MissingTitle,
    /// Sanatçı adı yok.
    MissingArtist,
    /// Zaman damgası okunamadı.
    BadTimestamp,
    /// `ms_played` alanı yok ya da anlamsız.
    BadDuration,
}

impl SkipReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotMusic => "not_music",
            Self::MissingTitle => "missing_title",
            Self::MissingArtist => "missing_artist",
            Self::BadTimestamp => "bad_timestamp",
            Self::BadDuration => "bad_duration",
        }
    }
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// İçe aktarmanın özeti. Kısmi başarı üreten her işlem böyle bir özet döndürür.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportSummary {
    /// Kaynağın adı (zip yolu ya da dizin).
    pub source: String,
    /// Hangi export biçimi olarak ayrıştırıldı.
    pub export: ExportKind,
    /// Arşivde bu biçime ait kaç dosya bulundu.
    pub files_matched: usize,
    /// Bu dosyalarda toplam kaç ham kayıt vardı.
    pub records_total: usize,
    /// Kaçı `Listen`'e dönüştü.
    pub listens: usize,
    /// Atlananlar, nedene göre.
    pub skipped: BTreeMap<SkipReason, usize>,
    /// Kaçında export'un kendisi ISRC verdi (kimlik zincirinin ilk halkası).
    pub with_isrc: usize,
    /// Kaçında sağlayıcı parça kimliği vardı.
    pub with_provider_id: usize,
}

impl ImportSummary {
    /// Toplam atlanan kayıt sayısı.
    #[must_use]
    pub fn skipped_total(&self) -> usize {
        self.skipped.values().sum()
    }

    /// Sayaçları tanı kaydediciye aktarır.
    pub fn record_into(&self, recorder: &mut crate::diag::Recorder) {
        recorder.set("import.files_matched", as_i64(self.files_matched));
        recorder.set("import.records_total", as_i64(self.records_total));
        recorder.set("import.listens", as_i64(self.listens));
        recorder.set("import.with_isrc", as_i64(self.with_isrc));
        recorder.set("import.with_provider_id", as_i64(self.with_provider_id));
        for (reason, count) in &self.skipped {
            recorder.set(format!("import.skipped.{reason}"), as_i64(*count));
        }
    }
}

fn as_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// İçe aktarmanın çıktısı: dinlemeler + özet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOutcome {
    pub listens: Vec<Listen>,
    pub summary: ImportSummary,
}

/// Ayrıştırıcıların ortak arayüzü. Her export biçimi için bir tane.
pub(crate) trait ExportParser {
    /// Bu ayrıştırıcının ilgilendiği biçim.
    fn kind(&self) -> ExportKind;

    /// Arşivdeki hangi girdiler bu biçime ait.
    fn matching_entries(&self, entries: &[String]) -> Vec<String>;

    /// Tek bir girdiyi ayrıştırır ve toplayıcıya yazar.
    fn parse_entry(&self, entry: &str, body: &[u8], sink: &mut ParseSink) -> Result<()>;
}

/// Ayrıştırma sırasında dinlemeleri ve atlama nedenlerini toplar.
#[derive(Debug, Default)]
pub(crate) struct ParseSink {
    pub(crate) listens: Vec<Listen>,
    pub(crate) records_total: usize,
    pub(crate) skipped: BTreeMap<SkipReason, usize>,
}

impl ParseSink {
    pub(crate) fn accept(&mut self, listen: Listen) {
        self.records_total += 1;
        self.listens.push(listen);
    }

    pub(crate) fn skip(&mut self, reason: SkipReason) {
        self.records_total += 1;
        *self.skipped.entry(reason).or_insert(0) += 1;
    }
}

/// Arşivin hangi export biçimi olduğunu içindeki dosya adlarından belirler.
///
/// # Errors
/// Tanınan hiçbir biçim yoksa [`ErrorKind::UnsupportedExport`] — arşivde ne
/// bulunduğu hata metnine yazılır ki kullanıcı yanlış zip verdiğini görsün.
pub fn detect(archive: &dyn ExportArchive) -> Result<ExportKind> {
    let entries = archive.entry_names();
    for parser in parsers() {
        if !parser.matching_entries(&entries).is_empty() {
            return Ok(parser.kind());
        }
    }
    let sample: Vec<&str> = entries.iter().take(10).map(String::as_str).collect();
    Err(Error::new(
        Stage::ImportDetect,
        ErrorKind::UnsupportedExport {
            detail: format!(
                "{} içinde tanınan bir geçmiş dosyası yok ({} girdi). İlk girdiler: {}",
                archive.source_label(),
                entries.len(),
                if sample.is_empty() {
                    "(arşiv boş)".to_owned()
                } else {
                    sample.join(", ")
                }
            ),
        },
    ))
}

fn parsers() -> Vec<Box<dyn ExportParser>> {
    vec![
        Box::new(spotify::ExtendedParser),
        Box::new(spotify::AccountParser),
    ]
}

/// Bir arşivi içe aktarır.
///
/// Biçim otomatik belirlenir. Kanonik kimlik çözümlemesi burada yapılmaz —
/// o ayrı bir aşamadır ([`crate::identity`]), böylece hangi adımın ne ürettiği
/// ayrı ayrı raporlanabilir.
///
/// # Errors
/// Biçim tanınmazsa, arşiv okunamazsa ya da bir dosya hiç ayrıştırılamazsa.
pub fn import(archive: &mut dyn ExportArchive) -> Result<ImportOutcome> {
    let export = detect(&*archive)?;
    let entries = archive.entry_names();
    let parser = parsers()
        .into_iter()
        .find(|p| p.kind() == export)
        .ok_or_else(|| {
            Error::new(
                Stage::ImportDetect,
                ErrorKind::UnsupportedExport {
                    detail: format!("{export} için ayrıştırıcı yok"),
                },
            )
        })?;

    let matched = parser.matching_entries(&entries);
    let mut sink = ParseSink::default();
    for entry in &matched {
        let body = archive.read_entry(entry)?;
        tracing::debug!(entry, bytes = body.len(), "export girdisi ayrıştırılıyor");
        parser.parse_entry(entry, &body, &mut sink)?;
    }

    let with_isrc = sink
        .listens
        .iter()
        .filter(|l| l.track.isrc.is_some())
        .count();
    let with_provider_id = sink
        .listens
        .iter()
        .filter(|l| l.track.provider_track_id.is_some())
        .count();

    let summary = ImportSummary {
        source: archive.source_label(),
        export,
        files_matched: matched.len(),
        records_total: sink.records_total,
        listens: sink.listens.len(),
        skipped: sink.skipped,
        with_isrc,
        with_provider_id,
    };
    tracing::info!(
        records = summary.records_total,
        listens = summary.listens,
        skipped = summary.skipped_total(),
        "içe aktarma tamamlandı"
    );
    Ok(ImportOutcome {
        listens: sink.listens,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_reports_what_it_saw_when_unsupported() {
        let archive = MemoryArchive::new("boş.zip").with_entry("readme.txt", b"merhaba".to_vec());
        let err = detect(&archive).unwrap_err();
        assert_eq!(err.stage(), Stage::ImportDetect);
        let text = err.chain_text();
        assert!(text.contains("readme.txt"), "{text}");
    }
}
