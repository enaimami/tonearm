//! Ses parmak izi (Chromaprint) — kimlik zincirinin **4. halkası** için girdi.
//!
//! Zincirin ilk üç halkası metne bakıyor: ISRC, MusicBrainz araması, bulanık
//! eşleşme. Üçü de etiketlerin doğru olduğunu varsayar. Etiketi `track01.mp3`
//! olan bir dosyada üçü de çaresizdir — parmak izi **sesin kendisine** bakan
//! tek halka.
//!
//! ## Neden `rusty-chromaprint`, `fpcalc` değil (D-045)
//!
//! Öneri Chromaprint'in resmî CLI'sini alt süreç olarak çağırmaktı: bağımlılık
//! ağacına tek satır eklemez. Kullanıcı saf Rust'ı seçti — PCM zaten
//! symphonia'dan geliyor ve kullanıcıdan hiçbir kurulum istenmiyor. Bedeli
//! ölçüldü: ağaç 49'dan **88 crate'e** çıkıyor (`rustfft` + `rubato`). Bu
//! yüzden ayrı bir `fingerprint` feature'ı arkasında; `audio` açıp parmak izi
//! istemeyen bir derleme (mobil çalma) o ağacı taşımaz.
//!
//! ## Yapılandırma sözleşme
//!
//! [`Configuration::preset_test2`] AcoustID'nin sunucu tarafında beklediği
//! algoritma. Değiştirmek, üretilen parmak izini AcoustID'nin veritabanıyla
//! karşılaştırılamaz kılar — sessizce "eşleşme yok" döner. Bu yüzden burada
//! sabit ve seçenek olarak sunulmuyor.

use std::path::Path;

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};

/// Bir dosyadan çıkarılmış parmak izi.
///
/// `uniffi` uyumlu: alanlar düz veri, ömür yok.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    /// Ham alt-parmak izleri. Karşılaştırma için; AcoustID'ye bu gitmiyor.
    pub raw: Vec<u32>,
    /// Sesin uzunluğu, **tam saniye**.
    ///
    /// AcoustID sorguda bunu ayrı bir parametre olarak istiyor ve eşleşmeyi
    /// süreye göre de daraltıyor. Kaptan okunamazsa çözülen örneklerden
    /// hesaplanır — tahmin değil, sayım.
    pub duration_secs: u32,
}

impl Fingerprint {
    /// AcoustID'nin beklediği biçim: sıkıştırılmış + URL-güvenli base64.
    #[cfg(feature = "fingerprint")]
    #[must_use]
    pub fn to_acoustid_string(&self) -> String {
        use rusty_chromaprint::{Configuration, FingerprintCompressor};
        let config = Configuration::preset_test2();
        let compressed = FingerprintCompressor::from(&config).compress(&self.raw);
        base64_url_nopad(&compressed)
    }
}

/// URL-güvenli base64, **dolgusuz**.
///
/// AcoustID parmak izini sorgu dizesinde bekliyor: standart alfabenin `+` ve
/// `/` karakterleri orada kaçırılmak zorunda kalır, `=` dolgusu ise bazı
/// aracılar tarafından kırpılır. Kendimiz yazıyoruz — tek kullanım için bir
/// kodlama crate'i eklemek, zaten 39 crate büyüyen bir ağaca bir tane daha
/// eklerdi.
#[cfg(feature = "fingerprint")]
fn base64_url_nopad(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).map_or(0, |b| u32::from(*b));
        let b2 = chunk.get(2).map_or(0, |b| u32::from(*b));
        let triple = (b0 << 16) | (b1 << 8) | b2;
        // Girdinin kaç baytı varsa o kadar 6-bitlik hane yazılır: 1 bayt → 2
        // hane, 2 bayt → 3 hane, 3 bayt → 4 hane.
        let digits = chunk.len() + 1;
        for i in 0..digits {
            let shift = 18 - 6 * i;
            let index = ((triple >> shift) & 0x3f) as usize;
            out.push(ALPHABET[index] as char);
        }
    }
    out
}

/// Bir ses dosyasının parmak izini çıkarır.
///
/// # Errors
/// Dosya açılamaz, kap tanınmaz, kod çözücü kurulamaz ya da içinde ses izi
/// yoksa. Bozuk **tek paketler** hata değil: sayılır, atlanır ve toplamı
/// raporlanır — bir çizik parmak izini tümden kaybettirmemeli.
#[cfg(feature = "fingerprint")]
pub fn fingerprint_file(path: &Path) -> Result<Fingerprint> {
    use rusty_chromaprint::{Configuration, Fingerprinter};
    use symphonia::core::codecs::audio::AudioDecoderOptions;
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::formats::{FormatOptions, TrackType};
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;

    let label = path.display().to_string();
    let file = std::fs::File::open(path)
        .map_err(|err| crate::error::io_err(Stage::IdentityResolve, path, err))?;

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
        hint.with_extension(ext);
    }
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|err| fingerprint_err(format!("{label} açılamadı: {err}")))?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| fingerprint_err(format!("{label} içinde ses izi yok")))?;
    let track_id = track.id;
    let codec_params = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .ok_or_else(|| fingerprint_err(format!("{label} için kod çözücü parametreleri okunamadı")))?
        .clone();

    let sample_rate = codec_params
        .sample_rate
        .ok_or_else(|| fingerprint_err(format!("{label} örnekleme hızını bildirmiyor")))?;
    let channels = codec_params
        .channels
        .as_ref()
        .map_or(0, symphonia::core::audio::Channels::count);
    if channels == 0 {
        return Err(fingerprint_err(format!(
            "{label} kanal sayısını bildirmiyor"
        )));
    }

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&codec_params, &AudioDecoderOptions::default())
        .map_err(|err| fingerprint_err(format!("kod çözücü kurulamadı: {err}")))?;

    let config = Configuration::preset_test2();
    let mut printer = Fingerprinter::new(&config);
    let channel_count = u32::try_from(channels).unwrap_or(u32::MAX);
    printer
        .start(sample_rate, channel_count)
        .map_err(|err| fingerprint_err(format!("parmak izi başlatılamadı: {err}")))?;

    let mut samples: Vec<i16> = Vec::new();
    let mut frames_seen: u64 = 0;
    let mut skipped_packets: usize = 0;

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(err) => return Err(fingerprint_err(format!("paket okunamadı: {err}"))),
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buffer) => {
                frames_seen += buffer.frames() as u64;
                samples.clear();
                buffer.copy_to_vec_interleaved(&mut samples);
                printer.consume(&samples);
            }
            Err(symphonia::core::errors::Error::DecodeError(msg)) => {
                // Tek bozuk paket parmak izini düşürmemeli — ama sessiz de
                // kalmamalı: yeterince paket düşerse parmak izi eşleşmez ve
                // sebebi görünür olmalı (K9).
                skipped_packets += 1;
                tracing::warn!(hata = %msg, "paket çözülemedi, parmak izinde atlanıyor");
            }
            Err(err) => return Err(fingerprint_err(format!("çözme durdu: {err}"))),
        }
    }
    printer.finish();

    if skipped_packets > 0 {
        tracing::warn!(
            dosya = %label,
            atlanan = skipped_packets,
            "parmak izi eksik paketlerle üretildi"
        );
    }

    let raw = printer.fingerprint().to_vec();
    if raw.is_empty() {
        // Sessizce boş parmak izi döndürmek, AcoustID'den "eşleşme yok"
        // aldırıp kusuru orada aratırdı. Sesin kısalığı ayrı bir tanıdır.
        return Err(fingerprint_err(format!(
            "{label} parmak izi üretemeyecek kadar kısa ({frames_seen} çerçeve)"
        )));
    }

    let duration_secs = u32::try_from(frames_seen / u64::from(sample_rate)).unwrap_or(u32::MAX);
    Ok(Fingerprint { raw, duration_secs })
}

/// Bir ses dosyasının parmak izini çıkarır.
///
/// # Errors
/// Bu derlemede `fingerprint` feature'ı kapalı olduğu için **her zaman** hata
/// döner. Sessizce "eşleşme yok" demiyoruz: zincirin 4. halkasının neden
/// atlandığı derleme kararıdır ve öyle söylenir (K9).
#[cfg(not(feature = "fingerprint"))]
pub fn fingerprint_file(path: &Path) -> Result<Fingerprint> {
    let _ = path;
    Err(Error::new(
        Stage::IdentityResolve,
        ErrorKind::Unsupported {
            provider: "chromaprint".to_owned(),
            what: "ses parmak izi (`fingerprint` feature'ı kapalı derleme)".to_owned(),
            capabilities: "NONE".to_owned(),
        },
    ))
}

/// Parmak izi aşamasının hatası.
#[cfg(feature = "fingerprint")]
fn fingerprint_err(detail: impl Into<String>) -> Error {
    Error::new(
        Stage::IdentityResolve,
        ErrorKind::Audio {
            detail: detail.into(),
        },
    )
}

#[cfg(all(test, feature = "fingerprint"))]
mod tests {
    use super::*;

    /// Parmak izi üretebilecek kadar uzun sentetik fixture — üretimi
    /// `fixtures/audio/README.md`'de yazılı.
    const SAMPLE: &str = "fingerprint_sample.flac";

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/audio")
            .join(name)
    }

    #[test]
    fn a_real_file_produces_a_stable_fingerprint() {
        let path = fixture(SAMPLE);
        let first = fingerprint_file(&path).expect("fixture parmak izi verebilmeli");
        let second = fingerprint_file(&path).expect("ikinci koşum");

        assert!(!first.raw.is_empty());
        assert_eq!(first.duration_secs, 15, "fixture 15 saniye");
        assert_eq!(
            first, second,
            "aynı dosya iki kez aynı parmak izini vermeli"
        );
    }

    /// Chromaprint'in ilk öğesi birkaç saniyelik pencere ister; daha kısa ses
    /// parmak izi **üretemez**. Bu bir kusur değil bir tanıdır ve öyle
    /// söylenmeli — boş bir parmak izi AcoustID'den "eşleşme yok" aldırır ve
    /// kusuru yanlış yerde arattırır (K9).
    #[test]
    fn a_file_too_short_to_fingerprint_says_so_instead_of_returning_empty() {
        // 2 saniyelik fixture — eşiğin kasıtlı olarak altında.
        let err = fingerprint_file(&fixture("Test Artist - Mp3 Track.mp3")).unwrap_err();
        let text = err.chain_text();
        assert!(text.starts_with("ADIM: IDENTITY_RESOLVE"), "{text}");
        assert!(text.contains("kısa"), "{text}");
    }

    /// Bozuk dosya sessizce boş dönmemeli — hangi aşamada battığını söylemeli.
    #[test]
    fn a_corrupt_file_reports_the_stage_instead_of_returning_nothing() {
        let err = fingerprint_file(&fixture("corrupt.flac")).unwrap_err();
        let text = err.chain_text();
        assert!(text.starts_with("ADIM: IDENTITY_RESOLVE"), "{text}");
    }

    #[test]
    fn a_missing_file_is_an_io_error_not_an_empty_fingerprint() {
        let err = fingerprint_file(&fixture("yok-boyle-bir-dosya.mp3")).unwrap_err();
        assert!(err.chain_text().contains("IDENTITY_RESOLVE"));
    }

    /// AcoustID biçimi: URL'de kaçırılması gereken karakter kalmamalı.
    #[test]
    fn the_acoustid_string_is_url_safe_and_unpadded() {
        let encoded = fingerprint_file(&fixture(SAMPLE))
            .expect("fixture")
            .to_acoustid_string();
        assert!(!encoded.is_empty());
        assert!(
            !encoded.contains(['+', '/', '=']),
            "URL-güvenli alfabe dışı karakter: {encoded}"
        );
    }

    /// Kodlayıcı bilinen vektörlerde doğru mu (RFC 4648 §10, URL alfabesi).
    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_url_nopad(b""), "");
        assert_eq!(base64_url_nopad(b"f"), "Zg");
        assert_eq!(base64_url_nopad(b"fo"), "Zm8");
        assert_eq!(base64_url_nopad(b"foo"), "Zm9v");
        assert_eq!(base64_url_nopad(b"foob"), "Zm9vYg");
        assert_eq!(base64_url_nopad(b"fooba"), "Zm9vYmE");
        assert_eq!(base64_url_nopad(b"foobar"), "Zm9vYmFy");
        // `+` ve `/` üreten baytlar URL alfabesinde `-` ve `_` olmalı.
        assert_eq!(base64_url_nopad(&[0xfb, 0xff]), "-_8");
    }
}
