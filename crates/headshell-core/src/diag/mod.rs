//! Tanılama: her başarısızlık hangi aşamada olduğunu söyler.
//!
//! Bu modül projenin bash prototipinden miras aldığı tek şeydir — bir şey
//! bozulduğunda *nerede* bozulduğunu tek blokta, kopyala-yapıştır edilebilir
//! biçimde söylemek.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// İşin hangi aşamasında olduğumuz. Her hata bir aşamaya bağlıdır.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Stage {
    /// Yapılandırma/veri dizini çözümleme.
    ConfigLoad,
    /// Export arşivini açma, içindekileri listeleme.
    ImportRead,
    /// Arşivin hangi sağlayıcıya ait olduğunu belirleme.
    ImportDetect,
    /// Kayıtları ayrıştırma (JSON/CSV → `Listen`).
    ImportParse,
    /// Kanonik kimlik çözümleme zinciri.
    IdentityResolve,
    /// Kütüphane veritabanını açma/şema göçü.
    LibraryOpen,
    /// Kütüphaneye yazma.
    LibraryWrite,
    /// Kütüphaneden okuma/arama.
    LibraryQuery,
    /// İstatistik hesaplama.
    StatsCompute,
    /// Sağlayıcı eklentisiyle konuşma.
    ProviderCall,
    /// Eklenti keşfi: manifest okuma, doğrulama, izin onayı (Faz 2).
    PluginLoad,
    /// Eklenti motoru: Python'u bulma, beyan edilen eserleri çözme ve kurma.
    ///
    /// `PluginLoad`'dan ayrı: "manifest bozuk" ile "manifest doğru ama
    /// istediği eser kurulu değil" farklı tanılardır ve farklı şeyler
    /// gerektirir — biri eklentiyi düzeltmeyi, öteki bir kurulum adımını (K9).
    PluginRuntime,
    /// Eklenti sürecini başlatma ve el sıkışma — sürüm uyumu burada denetlenir.
    ///
    /// `ProviderCall`'dan ayrı: "eklenti hiç açılmadı" ile "eklenti açıldı ama
    /// bu çağrıya hayır dedi" farklı tanılardır (K9).
    PluginHandshake,
    /// HTTP taşıma katmanı: bağlanma, zaman aşımı, TLS, durum kodu.
    ///
    /// `ProviderCall`'dan ayrı: "sunucuya ulaşamadım" ile "sunucu isteğimi
    /// reddetti" farklı sorunlardır ve farklı çözümleri vardır (K9).
    NetworkRequest,
    /// Sleeve kartı üretimi veya yazımı.
    SleeveRender,
    /// Çalınacak kaynağı bulma (sağlayıcıdan `AudioSource` alma).
    PlaybackResolve,
    /// Ses çözme (symphonia): kap açma, kod çözücü kurma.
    PlaybackDecode,
    /// Ses çıkışı (cpal): aygıt açma, akış kurma.
    PlaybackOutput,
}

impl Stage {
    /// Log ve rapor için sabit ad: `IDENTITY_RESOLVE`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ConfigLoad => "CONFIG_LOAD",
            Self::ImportRead => "IMPORT_READ",
            Self::ImportDetect => "IMPORT_DETECT",
            Self::ImportParse => "IMPORT_PARSE",
            Self::IdentityResolve => "IDENTITY_RESOLVE",
            Self::LibraryOpen => "LIBRARY_OPEN",
            Self::LibraryWrite => "LIBRARY_WRITE",
            Self::LibraryQuery => "LIBRARY_QUERY",
            Self::StatsCompute => "STATS_COMPUTE",
            Self::ProviderCall => "PROVIDER_CALL",
            Self::PluginLoad => "PLUGIN_LOAD",
            Self::PluginRuntime => "PLUGIN_RUNTIME",
            Self::PluginHandshake => "PLUGIN_HANDSHAKE",
            Self::NetworkRequest => "NETWORK_REQUEST",
            Self::SleeveRender => "SLEEVE_RENDER",
            Self::PlaybackResolve => "PLAYBACK_RESOLVE",
            Self::PlaybackDecode => "PLAYBACK_DECODE",
            Self::PlaybackOutput => "PLAYBACK_OUTPUT",
        }
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Çalıştığımız ortam. Hata raporunun ilk bloğu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvInfo {
    pub headshell_version: String,
    pub os: String,
    pub arch: String,
    pub data_dir: Option<PathBuf>,
}

impl EnvInfo {
    /// Derleme zamanı bilinen ortam bilgisini toplar.
    #[must_use]
    pub fn collect(data_dir: Option<PathBuf>) -> Self {
        Self {
            headshell_version: env!("CARGO_PKG_VERSION").to_owned(),
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            data_dir,
        }
    }
}

/// Son çalıştırmanın tanı raporu. `headshell diag` bunu basar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagReport {
    /// Hangi komut çalıştı (`import fixtures/spotify.zip`).
    pub command: String,
    pub started_at: jiff::Timestamp,
    pub finished_at: jiff::Timestamp,
    pub env: EnvInfo,
    /// Komut hata ile bittiyse hangi aşamada.
    pub failed_at: Option<Stage>,
    /// Hata zinciri: en dıştaki hata önce, `source()` sırasıyla.
    pub error_chain: Vec<String>,
    /// Sayılar: `records.total`, `identity.by_isrc`, ...
    pub counters: BTreeMap<String, i64>,
    /// Hata olmayan ama kaydedilmeye değer gözlemler.
    pub notes: Vec<String>,
}

impl DiagReport {
    /// Komut başarıyla bitti mi.
    #[must_use]
    pub fn succeeded(&self) -> bool {
        self.failed_at.is_none()
    }

    /// Kopyalanıp yapıştırılabilir tek blok. GUI de aynı metni gösterecek,
    /// bu yüzden biçimleme CLI'de değil burada.
    #[must_use]
    pub fn render(&self) -> String {
        use fmt::Write as _;
        let mut out = String::new();
        let _ = writeln!(out, "── headshell diag ───────────────────────────────");
        let _ = writeln!(out, "komut     : {}", self.command);
        let _ = writeln!(
            out,
            "sonuç     : {}",
            if self.succeeded() {
                "BAŞARILI"
            } else {
                "BAŞARISIZ"
            }
        );
        let _ = writeln!(out, "başlangıç : {}", self.started_at);
        let _ = writeln!(out, "bitiş     : {}", self.finished_at);
        let _ = writeln!(
            out,
            "sürüm     : headshell {} ({} {})",
            self.env.headshell_version, self.env.os, self.env.arch
        );
        if let Some(dir) = &self.env.data_dir {
            let _ = writeln!(out, "veri dizini: {}", dir.display());
        }

        if let Some(stage) = self.failed_at {
            let _ = writeln!(out, "\nADIM: {stage}");
            for (depth, err) in self.error_chain.iter().enumerate() {
                let _ = writeln!(out, "  {}{}", "  ".repeat(depth), err);
            }
        }

        if !self.counters.is_empty() {
            let _ = writeln!(out, "\nsayılar:");
            let width = self.counters.keys().map(String::len).max().unwrap_or(0);
            for (key, value) in &self.counters {
                let _ = writeln!(out, "  {key:<width$} = {value}");
            }
        }

        if !self.notes.is_empty() {
            let _ = writeln!(out, "\nnotlar:");
            for note in &self.notes {
                let _ = writeln!(out, "  - {note}");
            }
        }
        let _ = writeln!(out, "────────────────────────────────────────────");
        out
    }
}

/// Bir çalıştırma boyunca tanı bilgisi biriktirir.
///
/// Çekirdeğin içindeki işlemler buna sayı yazar; komut bittiğinde
/// [`Recorder::finish`] ile rapora dönüşür.
#[derive(Debug)]
pub struct Recorder {
    command: String,
    started_at: jiff::Timestamp,
    env: EnvInfo,
    counters: BTreeMap<String, i64>,
    notes: Vec<String>,
}

impl Recorder {
    #[must_use]
    pub fn start(command: impl Into<String>, data_dir: Option<PathBuf>) -> Self {
        Self {
            command: command.into(),
            started_at: jiff::Timestamp::now(),
            env: EnvInfo::collect(data_dir),
            counters: BTreeMap::new(),
            notes: Vec::new(),
        }
    }

    /// Bir sayacı artırır. Yoksa oluşturur.
    pub fn count(&mut self, key: impl Into<String>, delta: i64) {
        *self.counters.entry(key.into()).or_insert(0) += delta;
    }

    /// Bir sayacı mutlak değere ayarlar.
    pub fn set(&mut self, key: impl Into<String>, value: i64) {
        self.counters.insert(key.into(), value);
    }

    /// Hataya dönüşmeyen ama rapora girmesi gereken gözlem.
    pub fn note(&mut self, note: impl Into<String>) {
        self.notes.push(note.into());
    }

    /// Şu ana kadar biriken sayaçlar.
    #[must_use]
    pub fn counters(&self) -> &BTreeMap<String, i64> {
        &self.counters
    }

    /// Çalıştırmayı kapatır ve raporu üretir.
    #[must_use]
    pub fn finish(self, outcome: Result<(), &crate::Error>) -> DiagReport {
        let (failed_at, error_chain) = match outcome {
            Ok(()) => (None, Vec::new()),
            Err(err) => (Some(err.stage()), error_chain(err)),
        };
        DiagReport {
            command: self.command,
            started_at: self.started_at,
            finished_at: jiff::Timestamp::now(),
            env: self.env,
            failed_at,
            error_chain,
            counters: self.counters,
            notes: self.notes,
        }
    }
}

/// Hatanın **nedenlerini** düz metin listesine açar.
///
/// Zincir kasten `err`'in kendisinden değil `err.source()`'tan başlar:
/// [`crate::Error`]'un `Display`'i `"ADIM: {stage}"` olduğu için ilk halka
/// `failed_at`'ten zaten basılan başlığın kopyası olurdu (D-007).
/// `error_chain` yalnızca gerçek nedenleri içerir.
fn error_chain(err: &dyn std::error::Error) -> Vec<String> {
    let mut chain = Vec::new();
    let mut current = err.source();
    while let Some(cause) = current {
        chain.push(cause.to_string());
        current = cause.source();
    }
    chain
}

/// Son çalıştırmanın raporunu diske yazar.
///
/// # Errors
/// Dosya yazılamazsa.
pub fn save_last_run(path: &std::path::Path, report: &DiagReport) -> crate::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|source| crate::error::io_err(Stage::ConfigLoad, parent, source))?;
        }
    }
    let body = serde_json::to_vec_pretty(report).map_err(|source| {
        crate::Error::new(
            Stage::ConfigLoad,
            crate::ErrorKind::Json {
                entry: path.display().to_string(),
                source,
            },
        )
    })?;
    std::fs::write(path, body)
        .map_err(|source| crate::error::io_err(Stage::ConfigLoad, path, source))
}

/// Son çalıştırmanın raporunu okur.
///
/// Dosya yoksa `Ok(None)` — henüz hiç komut çalışmamış olabilir.
///
/// # Errors
/// Dosya okunamazsa ya da bozuksa.
pub fn load_last_run(path: &std::path::Path) -> crate::Result<Option<DiagReport>> {
    let body = match std::fs::read(path) {
        Ok(body) => body,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(crate::error::io_err(Stage::ConfigLoad, path, source)),
    };
    serde_json::from_slice(&body).map(Some).map_err(|source| {
        crate::Error::new(
            Stage::ConfigLoad,
            crate::ErrorKind::Json {
                entry: path.display().to_string(),
                source,
            },
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> DiagReport {
        DiagReport {
            command: "import fixtures/spotify.zip".to_owned(),
            started_at: jiff::Timestamp::UNIX_EPOCH,
            finished_at: jiff::Timestamp::UNIX_EPOCH,
            env: EnvInfo {
                headshell_version: "0.0.0".to_owned(),
                os: "linux".to_owned(),
                arch: "x86_64".to_owned(),
                data_dir: None,
            },
            failed_at: Some(Stage::ImportParse),
            // Zincir yalnızca nedenleri taşır; "ADIM: ..." başlığı
            // `failed_at`'ten bir kez basılır (D-007).
            error_chain: vec![
                "JSON ayrıştırılamadı: Streaming_History_Audio_0.json".to_owned(),
                "beklenmeyen alan".to_owned(),
            ],
            counters: BTreeMap::from([("records.total".to_owned(), 12)]),
            notes: vec!["2 kayıt atlandı".to_owned()],
        }
    }

    #[test]
    fn render_names_the_stage() {
        let text = sample().render();
        assert!(text.contains("ADIM: IMPORT_PARSE"), "{text}");
        assert!(text.contains("records.total = 12"), "{text}");
        assert!(text.contains("BAŞARISIZ"), "{text}");
    }

    #[test]
    fn error_chain_does_not_repeat_the_stage_header() {
        // D-007: `Error`'un Display'i "ADIM: {stage}" olduğu için zincir
        // ondan başlarsa başlık iki kez basılırdı.
        let err = crate::error::io_err(
            Stage::ImportRead,
            "/yok/dosya.zip",
            std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"),
        );
        let report = Recorder::start("import /yok/dosya.zip", None).finish(Err(&err));

        assert_eq!(report.failed_at, Some(Stage::ImportRead));
        assert!(
            !report
                .error_chain
                .iter()
                .any(|line| line.starts_with("ADIM:")),
            "zincir başlığı tekrar etmemeli: {:?}",
            report.error_chain
        );
        assert_eq!(
            report.error_chain,
            vec![
                "dosya işlemi başarısız: /yok/dosya.zip".to_owned(),
                "no such file".to_owned(),
            ]
        );
        assert_eq!(report.render().matches("ADIM: IMPORT_READ").count(), 1);
    }

    #[test]
    fn recorder_counts_and_finishes_clean() {
        let mut rec = Recorder::start("stats", None);
        rec.count("records.total", 3);
        rec.count("records.total", 2);
        rec.set("identity.by_isrc", 4);
        let report = rec.finish(Ok(()));
        assert!(report.succeeded());
        assert_eq!(report.counters["records.total"], 5);
        assert_eq!(report.counters["identity.by_isrc"], 4);
    }

    #[test]
    fn stage_names_are_screaming_snake() {
        assert_eq!(Stage::IdentityResolve.to_string(), "IDENTITY_RESOLVE");
        let json = serde_json::to_string(&Stage::ImportParse).unwrap();
        assert_eq!(json, "\"IMPORT_PARSE\"");
    }
}
