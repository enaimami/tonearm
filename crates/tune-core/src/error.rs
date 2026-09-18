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

    /// Sağlayıcı bu yeteneğe sahip değil.
    ///
    /// "Yapamıyorum" ile "sonuç yok" farklı şeylerdir; ikincisi boş liste,
    /// birincisi bu hata (K9).
    #[error("{provider} bunu yapamıyor: {what} (yetenekleri: {capabilities})")]
    Unsupported {
        provider: String,
        what: String,
        capabilities: String,
    },

    #[error("ses hattı hatası: {detail}")]
    Audio { detail: String },

    /// Taşıma katmanı hatası: bağlanamadı, zaman aşımı, TLS, DNS.
    ///
    /// Uygulama katmanı hatasından (`RemoteApi`) ayrı: "sunucuya
    /// ulaşamadım" ile "sunucu hayır dedi" farklı tanılardır (K9).
    #[error("ağ isteği başarısız: {url} ({detail})")]
    Network { url: String, detail: String },

    /// Sunucu 2xx dışında bir durum kodu döndürdü.
    #[error("sunucu HTTP {status} döndürdü: {url} — {detail}")]
    HttpStatus {
        url: String,
        status: u16,
        detail: String,
    },

    /// Eklenti manifesti okunamadı ya da geçersiz (Faz 2, §2.1).
    #[error("eklenti manifesti geçersiz: {path} — {detail}")]
    PluginManifest { path: PathBuf, detail: String },

    /// Eklenti motorunun bir adımı tökezledi (D-055).
    ///
    /// `step` **hangi adımda** olduğunu söyler — Python arama, eser indirme,
    /// karma doğrulama. Motorun bütün başarısızlıkları aynı cümleye
    /// çıkmamalı: "yt-dlp yok" ile "yt-dlp indirilemedi" ile "indirilen
    /// yt-dlp'nin karması tutmadı" üç ayrı tanıdır ve üçünün çözümü
    /// farklıdır (K9).
    #[error("eklenti motoru — {step}: {detail}")]
    PluginRuntime { step: String, detail: String },

    /// Eklentinin protokol sürümü çekirdeğinkiyle uyuşmuyor.
    ///
    /// Faz 2'nin "bitti sayılır" ölçütünün yarısı bu hata: uyumsuz eklenti
    /// **yüklenmez**, çekirdek çökmez, kullanıcı neyin uyuşmadığını görür.
    #[error(
        "{plugin} eklentisi bu sürümle konuşamıyor: eklenti api {plugin_api}, çekirdek api {host_api}"
    )]
    PluginIncompatible {
        plugin: String,
        plugin_api: u32,
        host_api: u32,
    },

    /// Eklenti izinleri onaylanmamış ya da onay geri alınmış.
    #[error("{plugin} eklentisi onaylanmadı: {detail}")]
    PluginNotApproved { plugin: String, detail: String },

    /// Eklenti süreci başlatılamadı, öldü ya da kanalı kapattı.
    #[error("{plugin} eklenti süreci çalışmıyor: {detail}")]
    PluginCrashed { plugin: String, detail: String },

    /// Eklenti verilen sürede cevap vermedi.
    ///
    /// Zaman aşımı çökmeden ayrı: asılı kalan bir eklenti ölmüş bir
    /// eklentiden farklı bir sorundur ve farklı çözülür (K9).
    #[error("{plugin} eklentisi {method} çağrısına {seconds} sn içinde cevap vermedi")]
    PluginTimeout {
        plugin: String,
        method: String,
        seconds: u64,
    },

    /// Eklenti JSON-RPC hata nesnesi döndürdü — yani süreç sağ, işi reddetti.
    #[error("{plugin} eklentisi {method} çağrısını reddetti: {message} (kod {code})")]
    PluginRpc {
        plugin: String,
        method: String,
        code: i64,
        message: String,
    },

    /// Sunucu HTTP 200 döndü ama gövdede hata var (Subsonic'in yaptığı gibi).
    #[error("{server} isteği reddetti: {message} (kod {code}, uç nokta: {endpoint})")]
    RemoteApi {
        server: String,
        endpoint: String,
        code: i64,
        message: String,
    },
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
