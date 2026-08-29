//! Çekirdeğin dış yüzeyi.
//!
//! CLI, GUI ve mobil bağlamalar **yalnızca** buradaki yöntemleri çağırır:
//! her komut tek bir çağrıdır, tanı kaydı ve kalıcılık burada halledilir.
//! Bir yeteneği CLI'den silsen çekirdek onu hâlâ sunar — Altın Kural'ın testi.

use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::diag::{DiagReport, Recorder, Stage};
use crate::error::{Error, ErrorKind, Result};
use crate::identity::{MetadataLookup, OfflineLookup, Resolution, ResolveSummary, Resolver};
use crate::import::{self, ImportSummary};
use crate::library::{ListenStore, SearchHit, SqliteLibrary, WriteSummary};
use crate::model::{PlayRule, TrackRef};
use crate::stats::{self, StatsQuery, StatsReport};
use crate::wrapped::{self, CardSize, WrappedData};

/// Bir içe aktarma komutunun tam sonucu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportReport {
    pub import: ImportSummary,
    pub identity: ResolveSummary,
    pub write: WriteSummary,
    pub diag: DiagReport,
}

/// Tek parça çözümlemesinin sonucu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolveReport {
    pub query: String,
    pub artist: String,
    pub title: String,
    pub resolution: Resolution,
    pub diag: DiagReport,
}

/// Arama sonucu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchReport {
    pub query: String,
    pub hits: Vec<SearchHit>,
    pub diag: DiagReport,
}

/// İstatistik sonucu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatsResponse {
    pub report: StatsReport,
    pub diag: DiagReport,
}

/// Wrapped kartı sonucu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WrappedResponse {
    pub data: WrappedData,
    pub size: CardSize,
    /// Dosya yazıldıysa biçim, yol ve bayt sayısı.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub written: Option<WrittenCard>,
    pub diag: DiagReport,
}

/// Yazılan kart dosyasının bilgisi.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WrittenCard {
    pub path: std::path::PathBuf,
    pub bytes: u64,
    pub kind: wrapped::CardFileKind,
}

/// Açık bir kütüphane üzerinde çalışan oturum.
pub struct Session {
    config: Config,
    library: SqliteLibrary,
}

impl Session {
    /// Yapılandırmadaki kütüphaneyi açar (yoksa oluşturur).
    ///
    /// # Errors
    /// Veri dizini oluşturulamazsa ya da veritabanı açılamazsa.
    pub fn open(config: Config) -> Result<Self> {
        config.ensure_data_dir()?;
        let library = SqliteLibrary::open(config.database_path())?;
        Ok(Self { config, library })
    }

    /// Kullanılan yapılandırma.
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Bir export arşivini içe aktarır, kimlikleri çözer ve kütüphaneye yazar.
    ///
    /// Üstveri kaynağı çağıran tarafından verilir; Faz 0'da
    /// [`OfflineLookup`] ile çağrılır, ağ geldiğinde imza değişmez.
    /// Generic değil `Arc<dyn _>` — K7 (`uniffi` generic ifade edemez).
    ///
    /// # Errors
    /// Arşiv okunamaz/tanınmazsa, çözümleme kaynağı hata verirse ya da
    /// yazma başarısız olursa. Hata hangi aşamada olduğunu taşır.
    pub async fn import_archive(
        &mut self,
        path: &Path,
        lookup: Arc<dyn MetadataLookup>,
    ) -> Result<ImportReport> {
        let mut rec = Recorder::start(
            format!("import {}", path.display()),
            Some(self.config.data_dir().to_path_buf()),
        );
        let result = self.import_inner(path, lookup, &mut rec).await;
        self.finish(rec, result, |(import, identity, write), diag| {
            ImportReport {
                import,
                identity,
                write,
                diag,
            }
        })
    }

    async fn import_inner(
        &mut self,
        path: &Path,
        lookup: Arc<dyn MetadataLookup>,
        rec: &mut Recorder,
    ) -> Result<(ImportSummary, ResolveSummary, WriteSummary)> {
        let mut archive = open_archive(path)?;
        let outcome = import::import(archive.as_mut())?;
        outcome.summary.record_into(rec);

        let tracks: Vec<TrackRef> = outcome
            .listens
            .iter()
            .map(|listen| listen.track.clone())
            .collect();
        let resolver = Resolver::new(lookup);
        let (resolutions, identity) = resolver.resolve_all(&tracks).await?;
        identity.record_into(rec);

        let mut listens = outcome.listens;
        for (listen, resolution) in listens.iter_mut().zip(&resolutions) {
            listen.canonical_id = Some(resolution.canonical_id.clone());
        }

        let write = self.library.insert_listens(&listens)?;
        write.record_into(rec);

        // Çözümleme sonuçlarını parça satırlarına da yaz ki `search` ve
        // sonraki çalıştırmalar hangi yöntemin çözdüğünü bilsin.
        let mut seen = std::collections::HashSet::new();
        for (listen, resolution) in listens.iter().zip(&resolutions) {
            let key =
                crate::identity::normalize::track_key(&listen.track.artist, &listen.track.title);
            if seen.insert(key.clone()) {
                self.library.set_resolution(&key, resolution)?;
            }
        }

        Ok((outcome.summary, identity, write))
    }

    /// Tek bir `"Sanatçı - Başlık"` sorgusunu kimlik zincirinden geçirir.
    ///
    /// # Errors
    /// Sorgu biçimsizse ya da üstveri kaynağı hata verirse.
    pub async fn resolve_track(
        &self,
        query: &str,
        lookup: Arc<dyn MetadataLookup>,
    ) -> Result<ResolveReport> {
        let mut rec = Recorder::start(
            format!("resolve {query:?}"),
            Some(self.config.data_dir().to_path_buf()),
        );
        let result = async {
            let track = TrackRef::parse_query(query)?;
            let resolution = Resolver::new(lookup).resolve(&track).await?;
            rec.set(
                "identity.confidence_pct",
                (resolution.confidence * 100.0) as i64,
            );
            rec.note(format!("yöntem: {}", resolution.method));
            Ok((track, resolution))
        }
        .await;

        let query = query.to_owned();
        self.finish(rec, result, move |(track, resolution), diag| {
            ResolveReport {
                query,
                artist: track.artist,
                title: track.title,
                resolution,
                diag,
            }
        })
    }

    /// Kütüphanede tam metin arama.
    ///
    /// Gösterilen çalma sayısı `rule`'u geçen dinlemelerdir — `stats` ile
    /// birebir aynı hesap (D-008). Ham olay sayısı yalnızca tanı kaydına
    /// (`search.listen_events`) yazılır.
    ///
    /// # Errors
    /// Sorgu boşsa ya da veritabanı hatasında.
    pub fn search(&self, query: &str, limit: usize, rule: PlayRule) -> Result<SearchReport> {
        let mut rec = Recorder::start(
            format!("library search {query:?}"),
            Some(self.config.data_dir().to_path_buf()),
        );
        let n = |v: usize| i64::try_from(v).unwrap_or(i64::MAX);
        let result = self.library.search(query, limit, rule).map(|outcome| {
            rec.set("search.hits", n(outcome.hits.len()));
            rec.set(
                "search.play_count",
                n(outcome.hits.iter().map(|hit| hit.play_count).sum()),
            );
            rec.set("search.listen_events", n(outcome.listen_events));
            outcome.hits
        });
        let query = query.to_owned();
        self.finish(rec, result, move |hits, diag| SearchReport {
            query,
            hits,
            diag,
        })
    }

    /// Kütüphanedeki dinlemelerden istatistik üretir.
    ///
    /// # Errors
    /// Kütüphane okunamazsa.
    pub fn stats(&self, query: StatsQuery) -> Result<StatsResponse> {
        let mut rec = Recorder::start(
            "stats".to_owned(),
            Some(self.config.data_dir().to_path_buf()),
        );
        let result = self.library.all_listens().map(|listens| {
            let report = stats::compute(&listens, query);
            report.record_into(&mut rec);
            report
        });
        self.finish(rec, result, |report, diag| StatsResponse { report, diag })
    }

    /// Paylaşılabilir Wrapped kartı üretir ve isteğe bağlı olarak dosyaya yazar.
    ///
    /// `out` verilirse uzantıya göre SVG veya PNG yazar; verilmezse yalnızca
    /// veri döner (JSON çıktısı veya başka bir tüketici için).
    ///
    /// # Errors
    /// Kütüphane okunamazsa, rasterizasyon veya dosya yazma başarısız olursa.
    pub fn wrapped(
        &self,
        query: StatsQuery,
        size: CardSize,
        out: Option<&Path>,
    ) -> Result<WrappedResponse> {
        let mut rec = Recorder::start(
            format!(
                "wrapped{}",
                query.year.map_or(String::new(), |y| format!(" {y}"))
            ),
            Some(self.config.data_dir().to_path_buf()),
        );

        let result = self.library.all_listens().and_then(|listens| {
            let report = stats::compute(&listens, query);
            report.record_into(&mut rec);
            let data = wrapped::card_data(&report, &listens);
            rec.set(
                "wrapped.hours",
                i64::try_from(data.plays).unwrap_or(i64::MAX),
            );
            rec.set(
                "wrapped.discoveries",
                i64::try_from(data.discoveries.len()).unwrap_or(i64::MAX),
            );
            rec.set(
                "wrapped.timeline_years",
                i64::try_from(data.by_year.len()).unwrap_or(i64::MAX),
            );

            let written = match out {
                Some(path) => {
                    let (kind, bytes) = wrapped::write_card(&data, size, path)?;
                    rec.set(
                        "wrapped.bytes_written",
                        i64::try_from(bytes).unwrap_or(i64::MAX),
                    );
                    rec.note(format!("çıktı: {}", path.display()));
                    Some(WrittenCard {
                        path: path.to_owned(),
                        bytes,
                        kind,
                    })
                }
                None => None,
            };

            Ok((data, written))
        });

        self.finish(rec, result, |(data, written), diag| WrappedResponse {
            data,
            size,
            written,
            diag,
        })
    }

    /// Son çalıştırmanın tanı raporu.
    ///
    /// # Errors
    /// Rapor dosyası bozuksa.
    pub fn last_diag(&self) -> Result<Option<DiagReport>> {
        crate::diag::load_last_run(&self.config.last_run_path())
    }

    /// Komutu kapatır: tanı raporunu diske yazar, sonucu paketler.
    ///
    /// Rapor hem başarıda hem başarısızlıkta yazılır — `tune diag` en çok
    /// bir şey patladığında lazım olur.
    fn finish<T, R>(
        &self,
        rec: Recorder,
        result: Result<T>,
        wrap: impl FnOnce(T, DiagReport) -> R,
    ) -> Result<R> {
        let report = match &result {
            Ok(_) => rec.finish(Ok(())),
            Err(err) => rec.finish(Err(err)),
        };
        if let Err(write_err) = crate::diag::save_last_run(&self.config.last_run_path(), &report) {
            // Tanı yazılamadıysa asıl hatayı gizleme; yalnızca logla.
            tracing::warn!(error = %write_err.chain_text(), "tanı raporu yazılamadı");
        }
        result.map(|value| wrap(value, report))
    }
}

/// Yola göre zip mi dizin mi olduğuna karar verir.
fn open_archive(path: &Path) -> Result<Box<dyn import::ExportArchive>> {
    let meta = std::fs::metadata(path)
        .map_err(|source| crate::error::io_err(Stage::ImportRead, path, source))?;
    if meta.is_dir() {
        Ok(Box::new(import::DirArchive::open(path)?))
    } else if meta.is_file() {
        Ok(Box::new(import::ZipArchive::open(path)?))
    } else {
        Err(Error::new(
            Stage::ImportRead,
            ErrorKind::InvalidInput {
                detail: format!("{} ne dosya ne dizin", path.display()),
            },
        ))
    }
}

/// Faz 0'ın varsayılan üstveri kaynağı: ağ yok.
///
/// Dönüş tipi somut değil `Arc<dyn MetadataLookup>` (D-006): çağıranlar
/// — CLI, GUI, mobil — kaynağı değiştirdiğimizde imza görmeden geçsin.
#[must_use]
pub fn default_lookup() -> Arc<dyn MetadataLookup> {
    Arc::new(OfflineLookup)
}
