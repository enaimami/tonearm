//! The audio fingerprint (Chromaprint) — the input for the identity chain's
//! **4th link**.
//!
//! The first three links of the chain look at text: the ISRC, a MusicBrainz
//! search, a fuzzy match. All three assume the tags are right. On a file
//! tagged `track01.mp3` all three are helpless — the fingerprint is the only
//! link that looks at **the audio itself**.
//!
//! ## Why `rusty-chromaprint` and not `fpcalc` (D-045)
//!
//! The proposal was to call Chromaprint's official CLI as a subprocess: it
//! adds not a single line to the dependency tree. The user chose pure Rust —
//! the PCM already comes from symphonia and nothing has to be installed by
//! the user. The price was measured: the tree grows from 49 to **88 crates**
//! (`rustfft` + `rubato`). That is why it sits behind a separate
//! `fingerprint` feature; a build that turns on `audio` but does not want
//! fingerprints (mobile playback) does not carry that tree.
//!
//! ## The configuration is a contract
//!
//! [`Configuration::preset_test2`] is the algorithm AcoustID expects on the
//! server side. Changing it makes the fingerprints produced incomparable with
//! AcoustID's database — it silently returns "no match". That is why it is
//! fixed here and not offered as an option.

use std::path::Path;

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};

/// A fingerprint extracted from a file.
///
/// `uniffi` compatible: the fields are plain data, no lifetimes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    /// The raw sub-fingerprints. For comparison; this is not what goes to
    /// AcoustID.
    pub raw: Vec<u32>,
    /// The length of the audio, in **whole seconds**.
    ///
    /// AcoustID wants this as a separate parameter in the query and also narrows
    /// the match by duration. If it cannot be read from the container it is
    /// computed from the decoded samples — a count, not a guess.
    pub duration_secs: u32,
}

impl Fingerprint {
    /// The format AcoustID expects: compressed + URL-safe base64.
    #[cfg(feature = "fingerprint")]
    #[must_use]
    pub fn to_acoustid_string(&self) -> String {
        use rusty_chromaprint::{Configuration, FingerprintCompressor};
        let config = Configuration::preset_test2();
        let compressed = FingerprintCompressor::from(&config).compress(&self.raw);
        base64_url_nopad(&compressed)
    }
}

/// URL-safe base64, **unpadded**.
///
/// AcoustID expects the fingerprint in the query string: the standard
/// alphabet's `+` and `/` characters would have to be escaped there, and the
/// `=` padding gets trimmed by some intermediaries. We write it ourselves —
/// adding an encoding crate for a single use would add one more crate to a
/// tree that has already grown by 39.
#[cfg(feature = "fingerprint")]
fn base64_url_nopad(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).map_or(0, |b| u32::from(*b));
        let b2 = chunk.get(2).map_or(0, |b| u32::from(*b));
        let triple = (b0 << 16) | (b1 << 8) | b2;
        // As many 6-bit digits are written as the input has bytes: 1 byte → 2
        // digits, 2 bytes → 3 digits, 3 bytes → 4 digits.
        let digits = chunk.len() + 1;
        for i in 0..digits {
            let shift = 18 - 6 * i;
            let index = ((triple >> shift) & 0x3f) as usize;
            out.push(ALPHABET[index] as char);
        }
    }
    out
}

/// Extracts an audio file's fingerprint.
///
/// # Errors
/// If the file cannot be opened, the container is not recognised, the
/// decoder cannot be set up or there is no audio track in it. Corrupt
/// **single packets** are not an error: they are counted, skipped and the
/// total is reported — one scratch must not lose the whole fingerprint.
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
        .map_err(|err| fingerprint_err(format!("could not open {label}: {err}")))?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| fingerprint_err(format!("no audio track in {label}")))?;
    let track_id = track.id;
    let codec_params = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .ok_or_else(|| {
            fingerprint_err(format!("could not read the decoder parameters for {label}"))
        })?
        .clone();

    let sample_rate = codec_params
        .sample_rate
        .ok_or_else(|| fingerprint_err(format!("{label} does not report a sample rate")))?;
    let channels = codec_params
        .channels
        .as_ref()
        .map_or(0, symphonia::core::audio::Channels::count);
    if channels == 0 {
        return Err(fingerprint_err(format!(
            "{label} does not report a channel count"
        )));
    }

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&codec_params, &AudioDecoderOptions::default())
        .map_err(|err| fingerprint_err(format!("could not set up the decoder: {err}")))?;

    let config = Configuration::preset_test2();
    let mut printer = Fingerprinter::new(&config);
    let channel_count = u32::try_from(channels).unwrap_or(u32::MAX);
    printer
        .start(sample_rate, channel_count)
        .map_err(|err| fingerprint_err(format!("could not start the fingerprint: {err}")))?;

    let mut samples: Vec<i16> = Vec::new();
    let mut frames_seen: u64 = 0;
    let mut skipped_packets: usize = 0;

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(err) => return Err(fingerprint_err(format!("could not read a packet: {err}"))),
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
                // A single corrupt packet must not drop the fingerprint — but it
                // must not stay silent either: if enough packets are dropped the
                // fingerprint will not match, and the reason must be visible (K9).
                skipped_packets += 1;
                tracing::warn!(error = %msg, "could not decode a packet; skipping it in the fingerprint");
            }
            Err(err) => return Err(fingerprint_err(format!("decoding stopped: {err}"))),
        }
    }
    printer.finish();

    if skipped_packets > 0 {
        tracing::warn!(
            file = %label,
            skipped = skipped_packets,
            "the fingerprint was produced with missing packets"
        );
    }

    let raw = printer.fingerprint().to_vec();
    if raw.is_empty() {
        // Silently returning an empty fingerprint would get a "no match"
        // from AcoustID and send people hunting for the fault there. Audio
        // that is too short is a separate diagnosis.
        return Err(fingerprint_err(format!(
            "{label} is too short to produce a fingerprint ({frames_seen} frames)"
        )));
    }

    let duration_secs = u32::try_from(frames_seen / u64::from(sample_rate)).unwrap_or(u32::MAX);
    Ok(Fingerprint { raw, duration_secs })
}

/// Extracts an audio file's fingerprint.
///
/// # Errors
/// In this build the `fingerprint` feature is off, so it **always** returns
/// an error. We do not silently say "no match": skipping the chain's 4th link
/// is a build decision and is said as such (K9).
#[cfg(not(feature = "fingerprint"))]
pub fn fingerprint_file(path: &Path) -> Result<Fingerprint> {
    let _ = path;
    Err(Error::new(
        Stage::IdentityResolve,
        ErrorKind::Unsupported {
            provider: "chromaprint".to_owned(),
            what: "audio fingerprint (a build with the `fingerprint` feature off)".to_owned(),
            capabilities: "NONE".to_owned(),
        },
    ))
}

/// The error of the fingerprint stage.
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

    /// A synthetic fixture long enough to produce a fingerprint — how it is made
    /// is written in `fixtures/audio/README.md`.
    const SAMPLE: &str = "fingerprint_sample.flac";

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/audio")
            .join(name)
    }

    #[test]
    fn a_real_file_produces_a_stable_fingerprint() {
        let path = fixture(SAMPLE);
        let first = fingerprint_file(&path).expect("the fixture must yield a fingerprint");
        let second = fingerprint_file(&path).expect("second run");

        assert!(!first.raw.is_empty());
        assert_eq!(first.duration_secs, 15, "the fixture is 15 seconds");
        assert_eq!(
            first, second,
            "the same file must give the same fingerprint twice"
        );
    }

    /// Chromaprint's first item needs a window of a few seconds; shorter audio
    /// **cannot** produce a fingerprint. This is not a flaw but a diagnosis and
    /// must be said as such — an empty fingerprint gets a "no match" from
    /// AcoustID and sends people hunting for the fault in the wrong place (K9).
    #[test]
    fn a_file_too_short_to_fingerprint_says_so_instead_of_returning_empty() {
        // A 2-second fixture — deliberately below the threshold.
        let err = fingerprint_file(&fixture("Test Artist - Mp3 Track.mp3")).unwrap_err();
        let text = err.chain_text();
        assert!(text.starts_with("STEP: IDENTITY_RESOLVE"), "{text}");
        assert!(text.contains("too short"), "{text}");
    }

    /// A corrupt file must not come back empty silently — it must say at which
    /// stage it failed.
    #[test]
    fn a_corrupt_file_reports_the_stage_instead_of_returning_nothing() {
        let err = fingerprint_file(&fixture("corrupt.flac")).unwrap_err();
        let text = err.chain_text();
        assert!(text.starts_with("STEP: IDENTITY_RESOLVE"), "{text}");
    }

    #[test]
    fn a_missing_file_is_an_io_error_not_an_empty_fingerprint() {
        let err = fingerprint_file(&fixture("no-such-file.mp3")).unwrap_err();
        assert!(err.chain_text().contains("IDENTITY_RESOLVE"));
    }

    /// The AcoustID format: no character that would need escaping in a URL may
    /// remain.
    #[test]
    fn the_acoustid_string_is_url_safe_and_unpadded() {
        let encoded = fingerprint_file(&fixture(SAMPLE))
            .expect("fixture")
            .to_acoustid_string();
        assert!(!encoded.is_empty());
        assert!(
            !encoded.contains(['+', '/', '=']),
            "a character outside the URL-safe alphabet: {encoded}"
        );
    }

    /// Is the encoder right on known vectors (RFC 4648 §10, URL alphabet).
    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_url_nopad(b""), "");
        assert_eq!(base64_url_nopad(b"f"), "Zg");
        assert_eq!(base64_url_nopad(b"fo"), "Zm8");
        assert_eq!(base64_url_nopad(b"foo"), "Zm9v");
        assert_eq!(base64_url_nopad(b"foob"), "Zm9vYg");
        assert_eq!(base64_url_nopad(b"fooba"), "Zm9vYmE");
        assert_eq!(base64_url_nopad(b"foobar"), "Zm9vYmFy");
        // Bytes that produce `+` and `/` must be `-` and `_` in the URL alphabet.
        assert_eq!(base64_url_nopad(&[0xfb, 0xff]), "-_8");
    }
}
