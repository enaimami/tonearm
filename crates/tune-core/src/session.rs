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
use crate::ids::ProviderId;
use crate::import::{self, ImportSummary};
use crate::library::{
    CatalogStore, CatalogTrack, CatalogWriteSummary, ListenStore, SearchHit, SqliteLibrary,
    WriteSummary,
};
use crate::model::{Listen, PlayRule, TrackRef};
use crate::playback::QueueItem;
use crate::provider::{ProviderHealth, ProviderInfo, ProviderRegistry, ScanSummary};
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

/// Kayıtlı sağlayıcıların listesi.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderListReport {
    pub providers: Vec<ProviderInfo>,
    pub diag: DiagReport,
}

/// Bir sağlayıcının sınama sonucu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderTestReport {
    pub info: ProviderInfo,
    pub health: ProviderHealth,
    pub diag: DiagReport,
}

/// Tarama sonucu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScanReport {
    /// Taranan kök dizinler. Boşsa kullanıcı `TUNE_MUSIC_DIRS` vermemiş.
    pub dirs: Vec<std::path::PathBuf>,
    pub summary: ScanSummary,
    /// Kalıcı kataloğa ne yazıldığı: eklenen, güncellenen, düşen satırlar.
    pub write: CatalogWriteSummary,
    pub diag: DiagReport,
}

/// Çalma seçenekleri.
///
/// Ayrı bir struct: `uniffi` için de tek bir record olarak geçer ve yeni
/// seçenek eklemek çağıranların imzasını kırmaz.
#[derive(Debug, Clone, Copy)]
pub struct PlayOptions<'a> {
    /// Aranacak metin.
    pub query: &'a str,
    /// Eşleşen tüm parçalar kuyruğa alınsın mı (false: yalnızca ilki).
    pub all: bool,
    /// Kuyruk karıştırılsın mı.
    pub shuffle: bool,
    /// Çalmadan yalnızca kuyruğu göster.
    pub dry_run: bool,
    /// Aramadan en fazla kaç sonuç alınacağı.
    pub limit: usize,
}

impl<'a> PlayOptions<'a> {
    /// Varsayılan seçeneklerle: ilk eşleşmeyi çal.
    #[must_use]
    pub fn new(query: &'a str) -> Self {
        Self {
            query,
            all: false,
            shuffle: false,
            dry_run: false,
            limit: 100,
        }
    }
}

/// Bir çalma komutunun sonucu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayReport {
    pub query: String,
    /// Kuyruğa alınan parçalar.
    pub queued: Vec<QueueItem>,
    /// Çalma sonunda üretilen dinleme kayıtları (§1.6).
    pub listens_recorded: usize,
    /// Gerçekten çalındı mı (`--dry-run` ile false).
    pub played: bool,
    pub diag: DiagReport,
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

    /// Sağlayıcıları listeler.
    ///
    /// # Errors
    /// Şu an hata üretmiyor; imza sağlayıcılar ağa taşındığında (Faz 2)
    /// değişmesin diye `Result`.
    pub fn providers(&self, registry: &ProviderRegistry) -> Result<ProviderListReport> {
        let mut rec = Recorder::start(
            "provider list".to_owned(),
            Some(self.config.data_dir().to_path_buf()),
        );
        rec.set(
            "provider.count",
            i64::try_from(registry.len()).unwrap_or(i64::MAX),
        );
        let result = Ok(registry.list());
        self.finish(rec, result, |providers, diag| ProviderListReport {
            providers,
            diag,
        })
    }

    /// Bir sağlayıcıyı sınar: ayakta mı, kaç parça görüyor.
    ///
    /// # Errors
    /// Sağlayıcı kayıtlı değilse ya da sağlık sorgusu hata verirse.
    pub async fn test_provider(
        &self,
        registry: &ProviderRegistry,
        id: &ProviderId,
    ) -> Result<ProviderTestReport> {
        let mut rec = Recorder::start(
            format!("provider test {id}"),
            Some(self.config.data_dir().to_path_buf()),
        );

        let result = async {
            let provider = registry.get(id).ok_or_else(|| {
                Error::new(
                    Stage::ProviderCall,
                    ErrorKind::NotFound {
                        what: format!(
                            "sağlayıcı: {id} (kayıtlı olanlar: {})",
                            registry
                                .list()
                                .iter()
                                .map(|info| info.id.to_string())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    },
                )
            })?;
            let info = provider.info();
            let health = provider.health().await?;
            Ok((info, health))
        }
        .await;

        if let Ok((_, health)) = &result {
            rec.set("provider.reachable", i64::from(health.reachable));
            if let Some(count) = health.track_count {
                rec.set(
                    "provider.track_count",
                    i64::try_from(count).unwrap_or(i64::MAX),
                );
            }
            if let Some(detail) = &health.detail {
                rec.note(detail.clone());
            }
        }

        self.finish(rec, result, |(info, health), diag| ProviderTestReport {
            info,
            health,
            diag,
        })
    }

    /// Yerel müzik dizinlerini tarar ve indeksi tazeler.
    ///
    /// # Errors
    /// Kök dizin okunamazsa. Tek tek dosya hataları hata değildir; özette
    /// sayılır (K9).
    pub async fn scan_providers(&mut self, registry: &ProviderRegistry) -> Result<ScanReport> {
        let mut rec = Recorder::start(
            "provider scan".to_owned(),
            Some(self.config.data_dir().to_path_buf()),
        );

        let result = Self::scan_inner(&mut self.library, registry).await;

        if let Ok((summary, write)) = &result {
            summary.record_into(&mut rec);
            let n = |v: usize| i64::try_from(v).unwrap_or(i64::MAX);
            rec.set("catalog.inserted", n(write.inserted));
            rec.set("catalog.updated", n(write.updated));
            rec.set("catalog.removed", n(write.removed));
            rec.set("catalog.unchanged", n(write.unchanged));
        }
        let dirs = self.config.music_dirs();
        self.finish(rec, result, move |(summary, write), diag| ScanReport {
            dirs,
            summary,
            write,
            diag,
        })
    }

    /// Taramayı yürütür ve sonucu **kalıcı kataloğa** yazar.
    ///
    /// Kütüphane ödünç alma çakışmasını önlemek için `&mut SqliteLibrary`
    /// ayrı parametre; `self` üzerinden çağrılamıyordu.
    async fn scan_inner(
        library: &mut SqliteLibrary,
        registry: &ProviderRegistry,
    ) -> Result<(ScanSummary, CatalogWriteSummary)> {
        let mut total = ScanSummary::default();
        let mut write_total = CatalogWriteSummary::default();
        let mut scanned = 0usize;

        for provider in registry.all() {
            let info = provider.info();
            // Damgalar: değişmemiş dosyanın etiketi yeniden okunmasın.
            let known = library.catalog_stamps(&info.id)?;

            let Some(scan) = provider.scan_catalog(&known).await? else {
                continue;
            };
            scanned += 1;
            total.files_seen += scan.summary.files_seen;
            total.audio_files += scan.summary.audio_files;
            total.indexed += scan.summary.indexed;
            total.tag_fallback += scan.summary.tag_fallback;
            total.failed += scan.summary.failed;
            total.unreadable_dirs += scan.summary.unreadable_dirs;
            total.unchanged += scan.summary.unchanged;

            // Değişmemiş satırların üstverisi taramadan gelmez; katalogdaki
            // hâlini koruyoruz. Aksi halde her tarama onları silerdi.
            let mut rows = Vec::with_capacity(scan.tracks.len());
            for entry in scan.tracks {
                match entry.track {
                    Some(track) => rows.push(CatalogTrack {
                        id: entry.id,
                        track,
                        from_tags: entry.from_tags,
                        mtime_ms: entry.mtime_ms,
                    }),
                    None => {
                        if let Some(existing) = library.catalog_get(&entry.id)? {
                            rows.push(existing);
                        }
                    }
                }
            }

            let write = library.replace_catalog(&info.id, &rows)?;
            write_total.inserted += write.inserted;
            write_total.updated += write.updated;
            write_total.removed += write.removed;
            write_total.unchanged += write.unchanged;
        }

        if scanned == 0 {
            return Err(Error::new(
                Stage::ProviderCall,
                ErrorKind::NotFound {
                    what: "taranabilir kataloğu olan sağlayıcı".to_owned(),
                },
            ));
        }
        Ok((total, write_total))
    }

    /// Aramayı çalınabilir bir kuyruğa çevirir.
    ///
    /// `all` false ise yalnızca ilk eşleşme alınır. Sonuç boşsa **hata**
    /// döner: "çaldım ama ses yok" durumundan iyidir.
    ///
    /// # Errors
    /// Sağlayıcı yoksa, arama hata verirse ya da hiç eşleşme yoksa.
    pub async fn queue_from_search(
        &self,
        registry: &ProviderRegistry,
        query: &str,
        all: bool,
        limit: usize,
    ) -> Result<Vec<QueueItem>> {
        // Önce **kalıcı katalog**: tarama bir kez yapılır, arama diske
        // gitmeden FTS ile cevaplanır. Sağlayıcıya sormak yalnızca katalog
        // boşsa gerekir (henüz taranmamış ya da uzak sağlayıcı).
        let mut items: Vec<QueueItem> = self
            .library
            .search_catalog(query, limit)?
            .into_iter()
            .filter(|hit| {
                // Katalogda duran ama artık çalınamayan sağlayıcıyı atla.
                registry.get(&hit.id.provider).is_some_and(|provider| {
                    provider
                        .info()
                        .capabilities
                        .contains(crate::provider::Capabilities::STREAM)
                })
            })
            .map(|hit| QueueItem {
                id: hit.id,
                track: hit.track,
            })
            .collect();

        if items.is_empty() {
            let streamers = registry.with_capability(crate::provider::Capabilities::STREAM);
            if streamers.is_empty() {
                return Err(Error::new(
                    Stage::PlaybackResolve,
                    ErrorKind::NotFound {
                        what: "ses akışı verebilen sağlayıcı".to_owned(),
                    },
                ));
            }
            for provider in streamers {
                let hits = provider.search(query, limit).await?;
                items.extend(hits.into_iter().map(|hit| QueueItem {
                    id: hit.id,
                    track: hit.track,
                }));
            }
        }

        if items.is_empty() {
            return Err(Error::new(
                Stage::PlaybackResolve,
                ErrorKind::NotFound {
                    what: format!(
                        "{query:?} ile eşleşen parça (indeks boşsa: `tune provider scan`)"
                    ),
                },
            ));
        }
        if !all {
            items.truncate(1);
        }
        Ok(items)
    }

    /// Arar, kuyruğa alır, çalar ve dinleme kayıtlarını yazar.
    ///
    /// Çalma bitene kadar bekler — CLI'nin bir döngü yazmasına gerek kalmasın
    /// diye akışın tamamı burada (Altın Kural). GUI ileride bunun yerine
    /// [`Session::queue_from_search`] + kendi `Player`'ıyla kendi döngüsünü
    /// kurar; ikisi de aynı çekirdek parçalarını kullanır.
    ///
    /// # Errors
    /// Eşleşme yoksa, sağlayıcı çalamıyorsa ya da ses hattı kurulamazsa.
    pub async fn play(
        &mut self,
        registry: &ProviderRegistry,
        options: PlayOptions<'_>,
    ) -> Result<PlayReport> {
        let mut rec = Recorder::start(
            format!("play {:?}", options.query),
            Some(self.config.data_dir().to_path_buf()),
        );

        let result = async {
            let items = self
                .queue_from_search(registry, options.query, options.all, options.limit)
                .await?;

            let mut player = crate::playback::Player::new(registry.clone());
            if options.shuffle {
                player.queue_mut().set_shuffle(true);
            }

            if options.dry_run {
                player.queue_mut().replace(items.clone());
                return Ok((items, Vec::new(), false));
            }

            player.play_items(items.clone()).await?;

            // Kuyruk bitene kadar sür. Yoklama aralığı çapadan bağımsız:
            // pozisyon tüketici tarafında hesaplanır (D-015), burada
            // yalnızca "parça bitti mi" sorulur.
            //
            // Uyku `std::thread::sleep`: çekirdek bir async çalışma zamanı
            // seçmez (PLAN konvansiyonu), `tokio::time` burada kullanılamaz.
            // Ses zaten kendi iş parçacığında çaldığı için bu bekleme sesi
            // kesmiyor; yalnızca bu çağrı bloklanıyor.
            loop {
                player.tick().await?;
                if player.state() == crate::playback::PlayState::Stopped {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            player.stop();

            let listens = player.take_listens();
            Ok((items, listens, true))
        }
        .await;

        let report = match result {
            Ok((items, listens, played)) => {
                // Scrobble'lar burada yazılır: import verisiyle aynı tabloya (§1.6).
                let written = self.record_listens(&listens)?;
                rec.set(
                    "play.queued",
                    i64::try_from(items.len()).unwrap_or(i64::MAX),
                );
                rec.set(
                    "play.listens_recorded",
                    i64::try_from(written.inserted).unwrap_or(i64::MAX),
                );
                Ok((items, written.inserted, played))
            }
            Err(err) => Err(err),
        };

        let query = options.query.to_owned();
        self.finish(
            rec,
            report,
            move |(queued, listens_recorded, played), diag| PlayReport {
                query,
                queued,
                listens_recorded,
                played,
                diag,
            },
        )
    }

    /// Bir arama sonucundan hazır bir [`crate::playback::Player`] kurar.
    ///
    /// `Session::play`'den farkı: **beklemez**. Çağıran kendi döngüsünü
    /// yürütür (TUI çizim döngüsü, GUI zamanlayıcısı), `tick()` çağırır ve
    /// biten dinlemeleri [`Session::record_listens`] ile yazar.
    ///
    /// # Errors
    /// Eşleşme yoksa ya da ilk parça çalınamazsa.
    pub async fn player_from_search(
        &self,
        registry: &ProviderRegistry,
        options: PlayOptions<'_>,
    ) -> Result<crate::playback::Player> {
        let items = self
            .queue_from_search(registry, options.query, options.all, options.limit)
            .await?;

        let mut player = crate::playback::Player::new(registry.clone());
        if options.shuffle {
            player.queue_mut().set_shuffle(true);
        }
        if options.dry_run {
            player.queue_mut().replace(items);
        } else {
            player.play_items(items).await?;
        }
        Ok(player)
    }

    /// Çalınan parçaların dinleme kayıtlarını kütüphaneye yazar (§1.6).
    ///
    /// Import verisiyle **aynı tabloya** yazılır: geçmiş ve bugün tek bir
    /// zaman çizelgesi olur.
    ///
    /// # Errors
    /// Yazma başarısız olursa.
    pub fn record_listens(&mut self, listens: &[Listen]) -> Result<WriteSummary> {
        if listens.is_empty() {
            return Ok(WriteSummary::default());
        }
        self.library.insert_listens(listens)
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
