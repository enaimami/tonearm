//! Yerel dosya sağlayıcı (PLAN §1.2, D-017).
//!
//! Bir dizini tarar, ses dosyalarının etiketlerini okur ve bellekte bir
//! indeks tutar. `SEARCH | BROWSE | STREAM` yeteneklerine sahiptir;
//! `CONTROL` yoktur — yerel disk uzaktan kumanda edilmez.
//!
//! Etiket okuma `audio` feature'ı gerektirir (symphonia). Feature kapalıyken
//! sağlayıcı yine derlenir ama dosya adından üstveri türetir — böylece
//! `provider list`/`search` yüzeyi ses hattı olmayan derlemelerde de çalışır.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::RwLock;

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};
use crate::identity::normalize;
#[cfg(feature = "audio")]
use crate::ids::Isrc;
use crate::ids::{ProviderId, ProviderTrackId};
use crate::model::TrackRef;

use super::{
    AudioSource, Capabilities, Provider, ProviderFuture, ProviderHealth, ProviderInfo,
    ProviderTrack,
};

/// Tanınan ses dosyası uzantıları.
///
/// Liste kasıtlı olarak dar: symphonia'nın açtığımız feature'larıyla
/// çözebildikleri. Tanımadığımız uzantıyı taramaya almak, "neden çalmıyor?"
/// sorusunu tarama anından çalma anına erteler.
pub const AUDIO_EXTENSIONS: &[&str] = &["flac", "mp3", "ogg", "oga", "m4a", "mp4", "aac", "wav"];

/// Taramanın sonucu — kaç dosya görüldü, kaçı alındı, kaçı neden atlandı.
///
/// K9: kısmi başarı üreten işlem özet döndürür. "12 parça bulundu" demek
/// yetmez; 300 dosyalık dizinde 288'inin neden atlandığı görünmeli.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScanSummary {
    /// Gezilen dosya sayısı (dizinler hariç).
    pub files_seen: usize,
    /// Ses uzantısı taşıyanlar.
    pub audio_files: usize,
    /// İndekse giren parça sayısı.
    pub indexed: usize,
    /// Etiketi okunamadığı için dosya adından türetilenler.
    pub tag_fallback: usize,
    /// Okunamayan/bozuk dosyalar.
    pub failed: usize,
    /// Erişilemeyen alt dizinler (izin vb.).
    pub unreadable_dirs: usize,
    /// Damgası değişmediği için yeniden okunmayan dosyalar.
    ///
    /// Taramanın hızlı olmasının sebebi bu sayıdır: ikinci taramada
    /// neredeyse her dosya buraya düşer.
    pub unchanged: usize,
}

impl ScanSummary {
    /// Sayaçları tanı kaydediciye aktarır.
    pub fn record_into(&self, recorder: &mut crate::diag::Recorder) {
        let n = |v: usize| i64::try_from(v).unwrap_or(i64::MAX);
        recorder.set("scan.files_seen", n(self.files_seen));
        recorder.set("scan.audio_files", n(self.audio_files));
        recorder.set("scan.indexed", n(self.indexed));
        recorder.set("scan.tag_fallback", n(self.tag_fallback));
        recorder.set("scan.failed", n(self.failed));
        recorder.set("scan.unreadable_dirs", n(self.unreadable_dirs));
        recorder.set("scan.unchanged", n(self.unchanged));
    }
}

/// İndekslenmiş bir yerel dosya.
#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexedFile {
    path: PathBuf,
    track: TrackRef,
}

/// Taramada görülen bir dosya — kalıcı kataloğa yazılmaya hazır.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedFile {
    pub path: PathBuf,
    /// Dosyanın son değişme zamanı (ms).
    pub mtime_ms: Option<i64>,
    /// Okunan üstveri. `None` ise dosya değişmemiş — çağıran katalogdaki
    /// satırı olduğu gibi bırakmalı, yeniden okumaya gerek yok.
    pub track: Option<TrackRef>,
    /// Üstveri etiketlerden mi geldi (dosya adından değil).
    pub from_tags: bool,
}

/// Yerel dosya sağlayıcı.
pub struct LocalProvider {
    id: ProviderId,
    roots: Vec<PathBuf>,
    index: RwLock<Vec<IndexedFile>>,
    last_scan: RwLock<ScanSummary>,
}

impl std::fmt::Debug for LocalProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalProvider")
            .field("roots", &self.roots)
            .field("indexed", &self.index.read().map(|i| i.len()).unwrap_or(0))
            .finish()
    }
}

impl LocalProvider {
    /// Verilen kök dizinleri tarayan bir sağlayıcı kurar (henüz taramaz).
    #[must_use]
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            id: ProviderId::new("local"),
            roots,
            index: RwLock::new(Vec::new()),
            last_scan: RwLock::new(ScanSummary::default()),
        }
    }

    /// Kök dizinleri yeniden tarar ve indeksi değiştirir.
    ///
    /// # Errors
    /// Hiçbir kök okunamazsa. Tek tek dosya hataları **hata değildir** —
    /// sayılır ve özette raporlanır (K9).
    pub fn rescan_now(&self) -> Result<ScanSummary> {
        let mut summary = ScanSummary::default();
        let mut found = Vec::new();

        for root in &self.roots {
            scan_dir(root, &mut found, &mut summary)?;
        }

        // Sıra deterministik olsun; aynı dizin iki çalıştırmada aynı sonucu versin.
        found.sort_by(|a, b| a.path.cmp(&b.path));
        summary.indexed = found.len();

        let mut index = self.index.write().map_err(|_| poisoned())?;
        *index = found;
        let mut last = self.last_scan.write().map_err(|_| poisoned())?;
        *last = summary.clone();
        Ok(summary)
    }

    /// Son taramanın özeti.
    ///
    /// # Errors
    /// İç kilit bozulmuşsa.
    pub fn last_scan(&self) -> Result<ScanSummary> {
        Ok(self.last_scan.read().map_err(|_| poisoned())?.clone())
    }

    /// Kök ağacındaki **en yeni dizin damgası** (D-025).
    ///
    /// Yalnızca dizinler geziliyor, dosyalar `stat` edilmiyor: soru "taramaya
    /// değer mi", "ne değişti" değil. Bir dosya eklendiğinde/silindiğinde
    /// bulunduğu dizinin mtime'ı değişir, bu yüzden ekleme ve silme buradan
    /// görünür.
    ///
    /// **Görünmeyen:** dosyanın yerinde düzenlenmesi (yeniden etiketleme).
    /// Dosya değişir ama dizin damgası değişmez. Bunu yakalamak her dosyayı
    /// `stat` etmek demekti — yani zaten artımlı taramanın kendisi. Kullanıcı
    /// etiketleri değiştirdiyse `tune provider scan` demeli; bunu bilmek,
    /// bilmiyormuş gibi "değişmedi" demekten iyidir (K9).
    ///
    /// Okunamayan dizin **sessizce atlanmıyor**: sayılıyor ve okunamayan bir
    /// dizin varsa "bilmiyorum" (`None`) dönülüyor — orada bir değişiklik
    /// olabilir ve "değişmedi" demek onu gizlerdi.
    ///
    /// # Errors
    /// Şu an hata üretmiyor; imza ileride değişmesin diye `Result`.
    pub fn newest_dir_mtime_ms(&self) -> Result<Option<i64>> {
        let mut newest: Option<i64> = None;
        let mut unreadable = 0usize;
        for root in &self.roots {
            walk_dir_stamps(root, &mut newest, &mut unreadable);
        }
        if unreadable > 0 {
            tracing::debug!(
                unreadable,
                "okunamayan dizin var; bayatlık sorusu cevaplanamadı"
            );
            return Ok(None);
        }
        Ok(newest)
    }

    /// Kök dizinleri tarar ve **kalıcı kataloğa yazılmaya hazır** satırlar üretir.
    ///
    /// `known` daha önce görülmüş `yol → mtime_ms` eşlemesi; damgası değişmemiş
    /// dosyanın etiketleri **yeniden okunmaz**. Taramanın pahalı kısmı budur:
    /// 10.000 dosyalık bir kütüphanede her seferinde hepsini çözmek dakikalar
    /// alır, damga karşılaştırması saniyeler.
    ///
    /// Değişmemiş dosyalar için üstveri döndürülmez; çağıran o satırları
    /// katalogda olduğu gibi bırakır. Bu yüzden dönüş `Option<TrackRef>`.
    ///
    /// # Errors
    /// Kök dizin okunamazsa. Tek tek dosya hataları özette sayılır (K9).
    pub fn scan_for_catalog(
        &self,
        known: &std::collections::HashMap<String, i64>,
    ) -> Result<(Vec<ScannedFile>, ScanSummary)> {
        let mut summary = ScanSummary::default();
        let mut out = Vec::new();

        for root in &self.roots {
            scan_dir_for_catalog(root, known, &mut out, &mut summary)?;
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        summary.indexed = out.len();

        let mut last = self.last_scan.write().map_err(|_| poisoned())?;
        *last = summary.clone();
        Ok((out, summary))
    }

    /// Bir dosya yolundan sağlayıcı kimliği üretir.
    fn track_id(&self, path: &Path) -> ProviderTrackId {
        ProviderTrackId::new(self.id.clone(), path.to_string_lossy().into_owned())
    }

    /// Yol, taranan köklerden birinin altında mı?
    ///
    /// `canonicalize` ile sembolik bağ ve `..` çözülür; aksi halde
    /// `~/Müzik/../../etc/passwd` gibi bir yol kontrolü atlatırdı.
    /// Yol çözülemiyorsa (dosya yok) **reddedilir** — şüpheliyi kabul etmek
    /// bir dosya okuma açığıdır.
    fn is_within_roots(&self, path: &Path) -> bool {
        let Ok(target) = path.canonicalize() else {
            return false;
        };
        self.roots.iter().any(|root| {
            root.canonicalize()
                .is_ok_and(|root| target.starts_with(&root))
        })
    }
}

fn poisoned() -> Error {
    Error::new(
        Stage::ProviderCall,
        ErrorKind::InvalidInput {
            detail: "yerel sağlayıcı indeksi bozuldu (kilit zehirlendi)".to_owned(),
        },
    )
}

/// Bir dizini özyinelemeli tarar.
///
/// Alt dizin okunamazsa **durmaz**: sayar ve devam eder. Tek bir izin hatası
/// yüzünden 10.000 dosyalık bir kütüphaneyi kaybetmek kabul edilemez.
/// Dizin ağacını gezip en yeni dizin damgasını bulur (yalnızca dizinler).
///
/// Hata **yutulmuyor**: okunamayan her dizin sayılıyor ve çağıran bunu
/// "bilmiyorum"a çeviriyor.
fn walk_dir_stamps(dir: &Path, newest: &mut Option<i64>, unreadable: &mut usize) {
    let stamp = std::fs::metadata(dir)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|since| i64::try_from(since.as_millis()).ok());
    match stamp {
        Some(ms) => {
            if newest.is_none_or(|current| ms > current) {
                *newest = Some(ms);
            }
        }
        None => *unreadable += 1,
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        *unreadable += 1;
        return;
    };
    for entry in entries.flatten() {
        // `file_type` sembolik bağı izlemiyor: bağ döngüsü taramayı asmasın.
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            walk_dir_stamps(&entry.path(), newest, unreadable);
        }
    }
}

fn scan_dir(dir: &Path, out: &mut Vec<IndexedFile>, summary: &mut ScanSummary) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(source) => {
            // Kök dizin okunamıyorsa bu gerçek bir hata: kullanıcı yanlış yol verdi.
            if out.is_empty() && summary.files_seen == 0 {
                return Err(crate::error::io_err(Stage::ProviderCall, dir, source));
            }
            summary.unreadable_dirs += 1;
            tracing::warn!(dizin = %dir.display(), hata = %source, "dizin okunamadı, atlanıyor");
            return Ok(());
        }
    };

    for entry in entries {
        let Ok(entry) = entry else {
            summary.failed += 1;
            continue;
        };
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            summary.failed += 1;
            continue;
        };

        if file_type.is_dir() {
            scan_dir(&path, out, summary)?;
            continue;
        }
        summary.files_seen += 1;

        if !has_audio_extension(&path) {
            continue;
        }
        summary.audio_files += 1;

        match read_track(&path) {
            Ok((track, from_tags)) => {
                if !from_tags {
                    summary.tag_fallback += 1;
                }
                out.push(IndexedFile { path, track });
            }
            Err(err) => {
                summary.failed += 1;
                tracing::warn!(
                    dosya = %path.display(),
                    hata = %err.chain_text(),
                    "dosya okunamadı, atlanıyor"
                );
            }
        }
    }
    Ok(())
}

/// Kalıcı katalog için tarar: değişmemiş dosyaların etiketlerini okumaz.
fn scan_dir_for_catalog(
    dir: &Path,
    known: &std::collections::HashMap<String, i64>,
    out: &mut Vec<ScannedFile>,
    summary: &mut ScanSummary,
) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(source) => {
            if out.is_empty() && summary.files_seen == 0 {
                return Err(crate::error::io_err(Stage::ProviderCall, dir, source));
            }
            summary.unreadable_dirs += 1;
            tracing::warn!(dizin = %dir.display(), hata = %source, "dizin okunamadı, atlanıyor");
            return Ok(());
        }
    };

    for entry in entries {
        let Ok(entry) = entry else {
            summary.failed += 1;
            continue;
        };
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            summary.failed += 1;
            continue;
        };

        if file_type.is_dir() {
            scan_dir_for_catalog(&path, known, out, summary)?;
            continue;
        }
        summary.files_seen += 1;

        if !has_audio_extension(&path) {
            continue;
        }
        summary.audio_files += 1;

        let mtime_ms = entry
            .metadata()
            .ok()
            .and_then(|meta| meta.modified().ok())
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|since| i64::try_from(since.as_millis()).ok());

        // Damga değişmediyse etiketi yeniden okuma — taramanın pahalı kısmı bu.
        let reference = path.to_string_lossy().into_owned();
        if let (Some(mtime), Some(previous)) = (mtime_ms, known.get(&reference)) {
            if mtime == *previous {
                summary.unchanged += 1;
                out.push(ScannedFile {
                    path,
                    mtime_ms,
                    track: None,
                    from_tags: false,
                });
                continue;
            }
        }

        match read_track(&path) {
            Ok((track, from_tags)) => {
                if !from_tags {
                    summary.tag_fallback += 1;
                }
                out.push(ScannedFile {
                    path,
                    mtime_ms,
                    track: Some(track),
                    from_tags,
                });
            }
            Err(err) => {
                summary.failed += 1;
                tracing::warn!(
                    dosya = %path.display(),
                    hata = %err.chain_text(),
                    "dosya okunamadı, atlanıyor"
                );
            }
        }
    }
    Ok(())
}

fn has_audio_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .is_some_and(|ext| AUDIO_EXTENSIONS.contains(&ext.as_str()))
}

/// Dosyadan parça üstverisi okur.
///
/// Dönüş: `(track, etiketlerden_mi)`. İkinci alan `false` ise üstveri dosya
/// adından türetildi — bu bir kayıp değil ama sayılması gereken bir düşüş (K9).
fn read_track(path: &Path) -> Result<(TrackRef, bool)> {
    #[cfg(feature = "audio")]
    {
        match tags::read(path) {
            Ok(Some(track)) => return Ok((track, true)),
            Ok(None) => {}
            Err(err) => return Err(err),
        }
    }
    Ok((track_from_filename(path), false))
}

/// Etiket yoksa dosya adından üstveri türetir.
///
/// `Sanatçı - Başlık.flac` biçimini tanır; tanımazsa başlık dosya adı,
/// sanatçı üst dizin olur. Uydurmuyoruz — ne bulduysak onu söylüyoruz.
fn track_from_filename(path: &Path) -> TrackRef {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("bilinmeyen");

    let parent_name = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|s| s.to_str());

    if let Some((artist, title)) = stem.split_once(" - ") {
        let artist = artist.trim();
        let title = title.trim();
        if !artist.is_empty() && !title.is_empty() {
            return TrackRef::new(artist, title).with_album(parent_name.map(str::to_owned));
        }
    }

    TrackRef::new(parent_name.unwrap_or("Bilinmeyen Sanatçı"), stem)
        .with_album(parent_name.map(str::to_owned))
}

/// Etiket okuma — yalnızca `audio` feature'ıyla.
#[cfg(feature = "audio")]
mod tags {
    use super::{Isrc, Path, Result, TrackRef};
    use crate::diag::Stage;
    use crate::error::{Error, ErrorKind};

    use symphonia::core::formats::probe::Hint;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::{MetadataOptions, StandardTag};

    /// Dosyanın etiketlerini okur.
    ///
    /// `Ok(None)`: dosya açıldı ama kullanılabilir etiket yok — çağıran
    /// dosya adına düşer. `Err`: dosya açılamadı/bozuk.
    pub(super) fn read(path: &Path) -> Result<Option<TrackRef>> {
        let file = std::fs::File::open(path)
            .map_err(|source| crate::error::io_err(Stage::PlaybackDecode, path, source))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        let mut reader = symphonia::default::get_probe()
            .probe(&hint, mss, Default::default(), MetadataOptions::default())
            .map_err(|source| {
                Error::new(
                    Stage::PlaybackDecode,
                    ErrorKind::Audio {
                        detail: format!("{} açılamadı: {source}", path.display()),
                    },
                )
            })?;

        // Süreyi kaptan al: bulanık eşleşmenin ayırt edici alanı (K6).
        let duration_ms = reader.tracks().iter().find_map(|track| {
            let time_base = track.time_base?;
            let duration = track.duration?;
            let millis = time_base.calc_duration(duration)?.as_millis();
            u64::try_from(millis).ok().filter(|ms| *ms > 0)
        });

        let mut artist = None;
        let mut album_artist = None;
        let mut title = None;
        let mut album = None;
        let mut isrc = None;

        let mut collect = |tags: &[symphonia::core::meta::Tag]| {
            for tag in tags {
                match tag.std.as_ref() {
                    Some(StandardTag::Artist(v)) if artist.is_none() => {
                        artist = Some(v.to_string());
                    }
                    Some(StandardTag::AlbumArtist(v)) if album_artist.is_none() => {
                        album_artist = Some(v.to_string());
                    }
                    Some(StandardTag::TrackTitle(v)) if title.is_none() => {
                        title = Some(v.to_string());
                    }
                    Some(StandardTag::Album(v)) if album.is_none() => {
                        album = Some(v.to_string());
                    }
                    Some(StandardTag::IdentIsrc(v)) if isrc.is_none() => {
                        isrc = Isrc::parse(v);
                    }
                    _ => {}
                }
            }
        };

        if let Some(revision) = reader.metadata().current() {
            collect(&revision.media.tags);
        }

        // Başlık ve sanatçıdan biri yoksa etiket kullanışsız sayılır.
        let (Some(title), Some(artist)) = (title, artist.or(album_artist)) else {
            return Ok(None);
        };

        Ok(Some(
            TrackRef::new(artist, title)
                .with_album(album)
                .with_duration_ms(duration_ms)
                .with_isrc(isrc),
        ))
    }
}

impl Provider for LocalProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: self.id.clone(),
            display_name: "Yerel dosyalar".to_owned(),
            // CONTROL yok: yerel disk uzaktan kumanda edilmez.
            capabilities: Capabilities::SEARCH | Capabilities::BROWSE | Capabilities::STREAM,
        }
    }

    fn health<'a>(&'a self) -> ProviderFuture<'a, ProviderHealth> {
        Box::pin(async move {
            let missing: Vec<String> = self
                .roots
                .iter()
                .filter(|root| !root.is_dir())
                .map(|root| root.display().to_string())
                .collect();

            let count = self.index.read().map_err(|_| poisoned())?.len();
            Ok(ProviderHealth {
                id: self.id.clone(),
                reachable: missing.is_empty(),
                track_count: Some(count),
                detail: if missing.is_empty() {
                    (count == 0).then(|| {
                        "indeks boş — `tune provider scan` çalıştırılmamış olabilir".to_owned()
                    })
                } else {
                    Some(format!("erişilemeyen dizin: {}", missing.join(", ")))
                },
            })
        })
    }

    /// Dizin damgalarına bakarak cevaplıyor (D-025) — tam tarama yapmadan.
    fn catalog_changed_since(&self, since_ms: i64) -> ProviderFuture<'_, Option<bool>> {
        Box::pin(async move { Ok(self.newest_dir_mtime_ms()?.map(|newest| newest > since_ms)) })
    }

    fn search<'a>(
        &'a self,
        query: &'a str,
        limit: usize,
    ) -> ProviderFuture<'a, Vec<ProviderTrack>> {
        Box::pin(async move {
            let needle = normalize::normalize_text(query);
            if needle.is_empty() {
                return Ok(Vec::new());
            }
            let index = self.index.read().map_err(|_| poisoned())?;
            let hits = index
                .iter()
                .filter(|file| {
                    let artist = normalize::normalize_text(&file.track.artist);
                    let title = normalize::normalize_text(&file.track.title);
                    let album = file
                        .track
                        .album
                        .as_deref()
                        .map(normalize::normalize_text)
                        .unwrap_or_default();
                    artist.contains(&needle) || title.contains(&needle) || album.contains(&needle)
                })
                .take(limit)
                .map(|file| ProviderTrack {
                    id: self.track_id(&file.path),
                    track: file.track.clone(),
                })
                .collect();
            Ok(hits)
        })
    }

    fn scan_catalog<'a>(
        &'a self,
        known: &'a std::collections::HashMap<String, i64>,
    ) -> ProviderFuture<'a, Option<super::CatalogScan>> {
        Box::pin(async move {
            let (files, summary) = self.scan_for_catalog(known)?;
            let tracks = files
                .into_iter()
                .map(|file| super::ScannedItem {
                    id: self.track_id(&file.path),
                    mtime_ms: file.mtime_ms,
                    track: file.track,
                    from_tags: file.from_tags,
                })
                .collect();
            Ok(Some(super::CatalogScan { tracks, summary }))
        })
    }

    fn resolve_source<'a>(
        &'a self,
        id: &'a ProviderTrackId,
    ) -> ProviderFuture<'a, Option<AudioSource>> {
        Box::pin(async move {
            if id.provider != self.id {
                return Ok(None);
            }
            let path = PathBuf::from(&id.id);
            // Yolun taranan köklerin **altında** olduğunu doğrula.
            //
            // Eskiden bellek indeksine bakılıyordu; indeks artık kalıcı
            // katalogda ve sağlayıcı onu görmüyor. Kök kontrolü aynı işi
            // görür ve daha sağlamdır: `resolve_source` rastgele dosya
            // okuma yüzeyi değil, `/etc/passwd` buradan çalınamaz.
            if !self.is_within_roots(&path) {
                return Ok(None);
            }
            if !path.is_file() {
                // İndekslendikten sonra silinmiş: sessiz None değil, açık hata.
                return Err(Error::new(
                    Stage::PlaybackResolve,
                    ErrorKind::NotFound {
                        what: format!("{} (indeksten beri silinmiş)", path.display()),
                    },
                ));
            }
            Ok(Some(AudioSource::LocalFile { path }))
        })
    }
}

/// Kayıt defterine eklenebilir hâle getirir.
impl LocalProvider {
    /// `Arc`'a sararak kayıt defterine hazırlar.
    #[must_use]
    pub fn shared(self) -> Arc<dyn Provider> {
        Arc::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("tune-local-{label}-{unique}"));
        std::fs::create_dir_all(&dir).expect("geçici dizin");
        dir
    }

    /// Gerçek ses fixture'larının dizini (`ffmpeg` üretimi sinüs tonları).
    fn audio_fixtures() -> PathBuf {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/audio"))
    }

    /// Bir fixture'ı geçici dizine kopyalar.
    fn copy_fixture(dir: &Path, name: &str, to: &str) {
        let target = dir.join(to);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).expect("alt dizin");
        }
        std::fs::copy(audio_fixtures().join(name), &target)
            .unwrap_or_else(|e| panic!("{name} kopyalanmalı: {e}"));
    }

    fn write(dir: &Path, name: &str, body: &[u8]) {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("alt dizin");
        }
        std::fs::write(&path, body).expect("dosya yazılmalı");
    }

    #[test]
    fn scan_counts_every_file_it_saw_and_skipped() {
        let dir = temp_dir("tarama");
        copy_fixture(&dir, "etiketli.flac", "etiketli.flac");
        copy_fixture(&dir, "Test Sanatci - Mp3 Parca.mp3", "Alt/Parca.mp3");
        write(&dir, "kapak.jpg", b"kapak");
        write(&dir, "notlar.txt", b"not");

        let provider = LocalProvider::new(vec![dir.clone()]);
        let summary = provider.rescan_now().expect("tarama");

        assert_eq!(summary.files_seen, 4);
        assert_eq!(summary.audio_files, 2, "yalnızca flac ve mp3");
        assert_eq!(summary.indexed, 2);
        assert_eq!(summary.failed, 0, "geçerli dosyalar başarısız olmamalı");
        // K9: sayılar birbirini tutmalı — kayıp dosya sessizce yutulmasın.
        assert_eq!(summary.indexed + summary.failed, summary.audio_files);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(feature = "audio")]
    #[test]
    fn a_corrupt_file_is_counted_not_swallowed() {
        // Bozuk dosya taramayı düşürmemeli ama görünmez de olmamalı (K9).
        // Yalnızca `audio` açıkken anlamlı: bozukluğu çözücü fark eder.
        let dir = temp_dir("bozuk");
        copy_fixture(&dir, "etiketli.flac", "saglam.flac");
        write(&dir, "bozuk.flac", b"bu bir ses dosyasi degil");

        let provider = LocalProvider::new(vec![dir.clone()]);
        let summary = provider.rescan_now().expect("tarama sürmeli");

        assert_eq!(summary.audio_files, 2);
        assert_eq!(summary.indexed, 1, "yalnızca sağlam dosya indekslenmeli");
        assert_eq!(summary.failed, 1, "bozuk dosya sayılmalı");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// `audio` kapalıyken üstveri dosya adından gelir; bu kipte tarama yine
    /// çalışmalı ve her dosya indekse girmeli (çözücü olmadığı için hiçbir
    /// dosya "bozuk" sayılmaz).
    #[cfg(not(feature = "audio"))]
    #[test]
    fn without_the_audio_feature_metadata_comes_from_filenames() {
        let dir = temp_dir("featuresiz");
        copy_fixture(&dir, "etiketli.flac", "Ad Soyad - Parça.flac");
        write(&dir, "bozuk.flac", b"bu bir ses dosyasi degil");

        let provider = LocalProvider::new(vec![dir.clone()]);
        let summary = provider.rescan_now().expect("tarama");

        assert_eq!(summary.audio_files, 2);
        assert_eq!(summary.indexed, 2, "çözücü yokken hiçbir dosya elenmez");
        assert_eq!(
            summary.tag_fallback, 2,
            "hepsi dosya adına düşmeli ve sayılmalı"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(feature = "audio")]
    #[test]
    fn tags_are_read_from_the_file_not_guessed_from_its_name() {
        let dir = temp_dir("etiket");
        // Dosya adı kasten yanıltıcı: etiketler kazanmalı.
        copy_fixture(&dir, "etiketli.flac", "yanlis-ad.flac");

        let provider = LocalProvider::new(vec![dir.clone()]);
        let summary = provider.rescan_now().expect("tarama");
        assert_eq!(summary.tag_fallback, 0, "etiket okunabilmeli");

        let index = provider.index.read().unwrap();
        let track = &index[0].track;
        assert_eq!(track.artist, "Test Sanatçı");
        assert_eq!(track.title, "Sinüs 440");
        assert_eq!(track.album.as_deref(), Some("Fixture Albümü"));
        // Süre kaptan okunmalı — bulanık eşleşmenin ayırt edici alanı (K6).
        let duration = track.duration_ms.expect("süre okunmalı");
        assert!(
            (900..=1100).contains(&duration),
            "1 saniyelik fixture, okunan: {duration}ms"
        );

        drop(index);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(feature = "audio")]
    #[test]
    fn an_untagged_file_falls_back_to_its_name_and_is_counted() {
        let dir = temp_dir("etiketsiz");
        copy_fixture(
            &dir,
            "Baska Sanatci - Ogg Parca.ogg",
            "Baska Sanatci - Ogg Parca.ogg",
        );

        let provider = LocalProvider::new(vec![dir.clone()]);
        let summary = provider.rescan_now().expect("tarama");
        assert_eq!(summary.indexed, 1);
        assert_eq!(
            summary.tag_fallback, 1,
            "etiketsiz dosya düşüş olarak sayılmalı"
        );

        let index = provider.index.read().unwrap();
        assert_eq!(index[0].track.artist, "Baska Sanatci");
        assert_eq!(index[0].track.title, "Ogg Parca");

        drop(index);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn filename_fallback_parses_artist_and_title() {
        let track = track_from_filename(Path::new("/muzik/Albüm/Radiohead - Creep.flac"));
        assert_eq!(track.artist, "Radiohead");
        assert_eq!(track.title, "Creep");
        assert_eq!(track.album.as_deref(), Some("Albüm"));
    }

    #[test]
    fn filename_fallback_uses_the_parent_dir_when_there_is_no_dash() {
        let track = track_from_filename(Path::new("/muzik/Portishead/Roads.mp3"));
        assert_eq!(track.artist, "Portishead", "üst dizin sanatçı sayılır");
        assert_eq!(track.title, "Roads");
    }

    #[test]
    fn filename_fallback_never_invents_an_empty_field() {
        // " - Creep.flac" gibi bozuk adlar boş sanatçı üretmemeli.
        let track = track_from_filename(Path::new("/muzik/ - Creep.flac"));
        assert!(!track.artist.is_empty());
        assert!(!track.title.is_empty());
    }

    #[test]
    fn capabilities_exclude_control() {
        let provider = LocalProvider::new(vec![]);
        let caps = provider.info().capabilities;
        assert!(caps.contains(Capabilities::STREAM));
        assert!(caps.contains(Capabilities::SEARCH));
        assert!(
            !caps.contains(Capabilities::CONTROL),
            "yerel disk uzaktan kumanda edilmez"
        );
    }

    #[cfg(feature = "audio")]
    #[tokio::test]
    async fn search_matches_tags_and_filename_derived_fields() {
        let dir = temp_dir("arama");
        copy_fixture(&dir, "etiketli.flac", "etiketli.flac");
        copy_fixture(
            &dir,
            "Baska Sanatci - Ogg Parca.ogg",
            "Baska Sanatci - Ogg Parca.ogg",
        );

        let provider = LocalProvider::new(vec![dir.clone()]);
        provider.rescan_now().expect("tarama");

        // Etiketten gelen sanatçı.
        let hits = provider.search("test sanatçı", 10).await.expect("arama");
        assert_eq!(hits.len(), 1, "{hits:?}");

        // Dosya adından gelen sanatçı (ogg'de etiket yok).
        let other = provider.search("baska", 10).await.expect("arama");
        assert_eq!(other.len(), 1, "{other:?}");

        let none = provider.search("bulunmayan", 10).await.expect("arama");
        assert!(none.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Arama, üstveri hangi yoldan gelirse gelsin dosya adı alanlarını bulmalı.
    /// Bu test iki kipte de aynı: ad `Ortak Sanatci - Ortak Parca`.
    #[tokio::test]
    async fn search_finds_filename_derived_tracks_in_both_modes() {
        let dir = temp_dir("arama-ortak");
        copy_fixture(
            &dir,
            "Baska Sanatci - Ogg Parca.ogg",
            "Ortak Sanatci - Ortak Parca.ogg",
        );

        let provider = LocalProvider::new(vec![dir.clone()]);
        provider.rescan_now().expect("tarama");

        let hits = provider.search("ortak sanatci", 10).await.expect("arama");
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert!(provider.search("yok", 10).await.unwrap().is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn search_respects_the_limit() {
        let dir = temp_dir("limit");
        for index in 0..4 {
            copy_fixture(
                &dir,
                "Baska Sanatci - Ogg Parca.ogg",
                &format!("Ortak Sanatci - Parca {index}.ogg"),
            );
        }
        let provider = LocalProvider::new(vec![dir.clone()]);
        provider.rescan_now().expect("tarama");

        let hits = provider.search("ortak", 2).await.expect("arama");
        assert_eq!(hits.len(), 2, "limit aşılmamalı");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn resolve_source_refuses_paths_outside_the_index() {
        let dir = temp_dir("kaynak");
        copy_fixture(
            &dir,
            "Baska Sanatci - Ogg Parca.ogg",
            "Ortak Sanatci - Ortak Parca.ogg",
        );

        let provider = LocalProvider::new(vec![dir.clone()]);
        provider.rescan_now().expect("tarama");

        // İndeksteki dosya çalınabilir.
        let indexed = provider.search("ortak", 1).await.unwrap();
        let source = provider
            .resolve_source(&indexed[0].id)
            .await
            .expect("çözümleme");
        assert!(matches!(source, Some(AudioSource::LocalFile { .. })));

        // Rastgele bir yol reddedilmeli — bu bir dosya okuma yüzeyi değil.
        let outside = ProviderTrackId::new(ProviderId::new("local"), "/etc/passwd");
        assert_eq!(provider.resolve_source(&outside).await.unwrap(), None);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_file_deleted_after_indexing_is_not_playable() {
        let dir = temp_dir("silinmis");
        copy_fixture(
            &dir,
            "Baska Sanatci - Ogg Parca.ogg",
            "Ortak Sanatci - Ortak Parca.ogg",
        );

        let provider = LocalProvider::new(vec![dir.clone()]);
        provider.rescan_now().expect("tarama");
        let indexed = provider.search("ortak", 1).await.unwrap();

        std::fs::remove_file(dir.join("Ortak Sanatci - Ortak Parca.ogg")).expect("silinmeli");

        // Silinmiş dosya `None` döner. Kök kontrolü `canonicalize`'a dayanıyor
        // ve var olmayan yol çözülemez — şüpheliyi kabul etmemek, "hata
        // mesajı daha güzel olsun" diye kontrolü gevşetmekten iyidir.
        // Kullanıcıya durumu Session anlatır: "çalınabilir kaynak bulunamadı".
        assert_eq!(
            provider.resolve_source(&indexed[0].id).await.unwrap(),
            None,
            "silinmiş dosya çalınabilir görünmemeli"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn resolve_source_rejects_traversal_out_of_the_roots() {
        // `<kök>/../../etc/passwd` gibi yollar kökün altında başlayıp dışına
        // çıkar; `canonicalize` bunu çözer.
        let dir = temp_dir("kacis");
        copy_fixture(&dir, "etiketli.flac", "sarki.flac");
        let provider = LocalProvider::new(vec![dir.clone()]);

        let escaping = dir.join("..").join("..").join("etc").join("passwd");
        let id = ProviderTrackId::new(
            ProviderId::new("local"),
            escaping.to_string_lossy().into_owned(),
        );
        assert_eq!(
            provider.resolve_source(&id).await.unwrap(),
            None,
            "kök dışına çıkan yol reddedilmeli"
        );

        // Kökün altındaki gerçek dosya ise çalınabilir kalmalı.
        let ok_id = ProviderTrackId::new(
            ProviderId::new("local"),
            dir.join("sarki.flac").to_string_lossy().into_owned(),
        );
        assert!(
            provider.resolve_source(&ok_id).await.unwrap().is_some(),
            "kökün altındaki dosya çalınabilmeli"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn health_reports_a_missing_root() {
        let provider = LocalProvider::new(vec![PathBuf::from("/olmayan/dizin")]);
        let health = provider.health().await.expect("sağlık");
        assert!(!health.reachable);
        assert!(
            health
                .detail
                .as_deref()
                .unwrap_or("")
                .contains("erişilemeyen"),
            "{health:?}"
        );
    }
}
