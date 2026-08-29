//! Ses hattı: symphonia (çözme) + cpal (çıkış). D-016.
//!
//! Yalnızca `audio` feature'ıyla derlenir.
//!
//! ## Nasıl çalışıyor
//!
//! Çözme bir arka plan iş parçacığında olur ve örnekleri paylaşılan bir
//! halka tamponuna yazar; cpal'ın ses geri çağrısı o tampondan okur. İki
//! taraf arasında kilit **tutulmaz** (ses geri çağrısı bloklanamaz: bloklarsa
//! kullanıcı cızırtı duyar).
//!
//! Pozisyon, çıkışa **gerçekten verilmiş** kare sayısından hesaplanır —
//! çözülmüş kare sayısından değil. Aradaki fark tampon dolusu kadar zamandır;
//! çözülene bakmak ilerleme çubuğunu sesin önüne düşürürdü.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};

use super::anchor::PlayState;

fn audio_err(stage: Stage, detail: impl Into<String>) -> Error {
    Error::new(
        stage,
        ErrorKind::Audio {
            detail: detail.into(),
        },
    )
}

/// Çözücüden çıkışa örnek taşıyan halka tamponu.
///
/// Tek üretici (çözme iş parçacığı), tek tüketici (ses geri çağrısı).
/// `Mutex` var ama tutma süresi bir `memcpy` kadar; ses geri çağrısı
/// bloklanmasın diye kilit alınamazsa **sessizlik yazılır**, beklenmez.
struct RingBuffer {
    samples: Mutex<std::collections::VecDeque<f32>>,
    capacity: usize,
}

impl RingBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            samples: Mutex::new(std::collections::VecDeque::with_capacity(capacity)),
            capacity,
        }
    }

    /// Tampondaki örnek sayısı.
    fn len(&self) -> usize {
        self.samples.lock().map(|s| s.len()).unwrap_or(0)
    }

    /// Yer varsa yazar; dolu ise yazamadığı sayıyı döndürür.
    fn push(&self, data: &[f32]) -> usize {
        let Ok(mut samples) = self.samples.lock() else {
            return data.len();
        };
        let free = self.capacity.saturating_sub(samples.len());
        let take = free.min(data.len());
        samples.extend(&data[..take]);
        data.len() - take
    }

    /// `out`'u doldurur; yetmezse kalanı sessizlikle doldurur ve kaç örnek
    /// gerçekten verildiğini döndürür.
    fn pop_into(&self, out: &mut [f32]) -> usize {
        let Ok(mut samples) = self.samples.lock() else {
            out.fill(0.0);
            return 0;
        };
        let take = samples.len().min(out.len());
        for slot in out.iter_mut().take(take) {
            *slot = samples.pop_front().unwrap_or(0.0);
        }
        out[take..].fill(0.0);
        take
    }

    fn clear(&self) {
        if let Ok(mut samples) = self.samples.lock() {
            samples.clear();
        }
    }
}

/// Çözme iş parçacığı ile oynatıcı arasındaki paylaşılan durum.
struct Shared {
    ring: RingBuffer,
    /// Çıkışa verilmiş toplam kare sayısı. Pozisyonun kaynağı.
    frames_played: AtomicU64,
    /// Çıkışın örnekleme hızı (kare/saniye).
    sample_rate: AtomicU64,
    /// Kanal sayısı.
    channels: AtomicU64,
    /// `PlayState` — atomik olarak taşınabilsin diye `u8`.
    state: AtomicU8,
    /// Çözme iş parçacığına "dur" işareti.
    stop: AtomicBool,
    /// Kaynak sonuna gelindi mi (çözme bitti, tampon boşalınca parça biter).
    drained: AtomicBool,
    /// Çözme sırasında oluşan hata (varsa) — çağıran `take_error` ile alır.
    error: Mutex<Option<String>>,
}

const STATE_STOPPED: u8 = 0;
const STATE_PLAYING: u8 = 1;
const STATE_PAUSED: u8 = 2;
const STATE_BUFFERING: u8 = 3;

fn state_to_u8(state: PlayState) -> u8 {
    match state {
        PlayState::Stopped => STATE_STOPPED,
        PlayState::Playing => STATE_PLAYING,
        PlayState::Paused => STATE_PAUSED,
        PlayState::Buffering => STATE_BUFFERING,
    }
}

fn u8_to_state(raw: u8) -> PlayState {
    match raw {
        STATE_PLAYING => PlayState::Playing,
        STATE_PAUSED => PlayState::Paused,
        STATE_BUFFERING => PlayState::Buffering,
        _ => PlayState::Stopped,
    }
}

impl Shared {
    fn state(&self) -> PlayState {
        u8_to_state(self.state.load(Ordering::Acquire))
    }

    fn set_state(&self, state: PlayState) {
        self.state.store(state_to_u8(state), Ordering::Release);
    }

    /// Çıkışa verilmiş kare sayısından pozisyon (ms).
    fn position_ms(&self) -> u64 {
        let rate = self.sample_rate.load(Ordering::Acquire);
        if rate == 0 {
            return 0;
        }
        self.frames_played
            .load(Ordering::Acquire)
            .saturating_mul(1000)
            / rate
    }

    fn record_error(&self, detail: String) {
        if let Ok(mut slot) = self.error.lock() {
            if slot.is_none() {
                *slot = Some(detail);
            }
        }
    }
}

/// Bir dosyayı çalan ses motoru.
///
/// `Drop` ile akış kapanır ve çözme iş parçacığı durur — sızıntı bırakmaz.
pub struct AudioEngine {
    shared: Arc<Shared>,
    stream: Option<cpal::Stream>,
    decoder: Option<std::thread::JoinHandle<()>>,
    duration_ms: Option<u64>,
}

impl std::fmt::Debug for AudioEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioEngine")
            .field("state", &self.shared.state())
            .field("position_ms", &self.shared.position_ms())
            .finish_non_exhaustive()
    }
}

impl AudioEngine {
    /// Bir dosyayı açar, ses aygıtını kurar ve çalmaya başlar.
    ///
    /// # Errors
    /// Dosya açılamaz/çözülemezse ([`Stage::PlaybackDecode`]) ya da ses aygıtı
    /// kurulamazsa ([`Stage::PlaybackOutput`]). Hangi aşamada olduğu hatada durur.
    pub fn play_file(path: &std::path::Path) -> Result<Self> {
        // — Kaynağı aç ve kabı tanı.
        let file = std::fs::File::open(path)
            .map_err(|source| crate::error::io_err(Stage::PlaybackDecode, path, source))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        let mut format = symphonia::default::get_probe()
            .probe(
                &hint,
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(|source| {
                audio_err(
                    Stage::PlaybackDecode,
                    format!("{} açılamadı: {source}", path.display()),
                )
            })?;

        let track = format.default_track(TrackType::Audio).ok_or_else(|| {
            audio_err(
                Stage::PlaybackDecode,
                format!("{} içinde ses izi yok", path.display()),
            )
        })?;
        let track_id = track.id;

        let duration_ms = track
            .time_base
            .zip(track.duration)
            .and_then(|(tb, dur)| u64::try_from(tb.calc_duration(dur)?.as_millis()).ok());

        let codec_params = track
            .codec_params
            .as_ref()
            .and_then(|params| params.audio())
            .ok_or_else(|| {
                audio_err(
                    Stage::PlaybackDecode,
                    format!("{} için kod çözücü parametreleri okunamadı", path.display()),
                )
            })?
            .clone();

        let mut decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&codec_params, &AudioDecoderOptions::default())
            .map_err(|source| {
                audio_err(
                    Stage::PlaybackDecode,
                    format!("kod çözücü kurulamadı: {source}"),
                )
            })?;

        // — Ses aygıtını aç.
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| audio_err(Stage::PlaybackOutput, "varsayılan ses çıkışı bulunamadı"))?;
        let supported = device.default_output_config().map_err(|source| {
            audio_err(
                Stage::PlaybackOutput,
                format!("çıkış yapılandırması okunamadı: {source}"),
            )
        })?;
        let out_rate = supported.sample_rate();
        let out_channels = supported.channels() as usize;

        // Yaklaşık 2 saniyelik tampon: cızırtıya karşı yeterli, gecikme kabul edilebilir.
        let capacity = out_rate as usize * out_channels * 2;
        let shared = Arc::new(Shared {
            ring: RingBuffer::new(capacity),
            frames_played: AtomicU64::new(0),
            sample_rate: AtomicU64::new(u64::from(out_rate)),
            channels: AtomicU64::new(out_channels as u64),
            state: AtomicU8::new(STATE_BUFFERING),
            stop: AtomicBool::new(false),
            drained: AtomicBool::new(false),
            error: Mutex::new(None),
        });

        // — Çözme iş parçacığı.
        let decode_shared = Arc::clone(&shared);
        let source_rate = codec_params.sample_rate.unwrap_or(out_rate);
        let source_channels = codec_params
            .channels
            .as_ref()
            .map_or(out_channels, symphonia::core::audio::Channels::count);

        let decoder_thread = std::thread::Builder::new()
            .name("tune-decode".to_owned())
            .spawn(move || {
                let mut scratch: Vec<f32> = Vec::new();
                loop {
                    if decode_shared.stop.load(Ordering::Acquire) {
                        break;
                    }
                    // Tampon doluysa bekle: çözme, çalmanın önüne geçmesin.
                    if decode_shared.ring.len() >= capacity.saturating_sub(capacity / 8) {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        continue;
                    }

                    let packet = match format.next_packet() {
                        Ok(Some(packet)) => packet,
                        Ok(None) => {
                            decode_shared.drained.store(true, Ordering::Release);
                            break;
                        }
                        Err(source) => {
                            decode_shared.record_error(format!("paket okunamadı: {source}"));
                            decode_shared.drained.store(true, Ordering::Release);
                            break;
                        }
                    };
                    if packet.track_id != track_id {
                        continue;
                    }

                    match decoder.decode(&packet) {
                        Ok(buffer) => {
                            interleave_f32(&buffer, &mut scratch);
                            resample_into(
                                &scratch,
                                source_rate,
                                source_channels,
                                out_rate,
                                out_channels,
                                &decode_shared,
                                capacity,
                            );
                        }
                        Err(symphonia::core::errors::Error::DecodeError(msg)) => {
                            // Tek bozuk paket parçayı düşürmemeli; say ve devam et.
                            tracing::warn!(hata = %msg, "paket çözülemedi, atlanıyor");
                        }
                        Err(source) => {
                            decode_shared.record_error(format!("çözme durdu: {source}"));
                            decode_shared.drained.store(true, Ordering::Release);
                            break;
                        }
                    }
                }
            })
            .map_err(|source| {
                audio_err(
                    Stage::PlaybackOutput,
                    format!("çözme iş parçacığı başlatılamadı: {source}"),
                )
            })?;

        // — Ses geri çağrısı.
        let cb_shared = Arc::clone(&shared);
        let err_shared = Arc::clone(&shared);
        let config: cpal::StreamConfig = supported.config();
        let stream = device
            .build_output_stream(
                config,
                move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    // Duraklatıldıysa sessizlik yaz; tampon korunur.
                    if cb_shared.state() == PlayState::Paused {
                        out.fill(0.0);
                        return;
                    }
                    let given = cb_shared.ring.pop_into(out);
                    let channels = cb_shared.channels.load(Ordering::Acquire).max(1);
                    cb_shared
                        .frames_played
                        .fetch_add(given as u64 / channels, Ordering::AcqRel);

                    if given > 0 {
                        cb_shared.set_state(PlayState::Playing);
                    } else if cb_shared.drained.load(Ordering::Acquire) {
                        cb_shared.set_state(PlayState::Stopped);
                    } else {
                        // Veri bitti ama kaynak bitmedi: bekliyoruz.
                        cb_shared.set_state(PlayState::Buffering);
                    }
                },
                move |err| {
                    err_shared.record_error(format!("ses akışı hatası: {err}"));
                },
                None,
            )
            .map_err(|source| {
                audio_err(
                    Stage::PlaybackOutput,
                    format!("ses akışı kurulamadı: {source}"),
                )
            })?;

        stream.play().map_err(|source| {
            audio_err(
                Stage::PlaybackOutput,
                format!("ses akışı başlatılamadı: {source}"),
            )
        })?;

        Ok(Self {
            shared,
            stream: Some(stream),
            decoder: Some(decoder_thread),
            duration_ms,
        })
    }

    /// Şu anki durum.
    #[must_use]
    pub fn state(&self) -> PlayState {
        self.shared.state()
    }

    /// Çıkışa verilmiş sesin karşılığı olan pozisyon.
    #[must_use]
    pub fn position_ms(&self) -> u64 {
        self.shared.position_ms()
    }

    /// Parçanın süresi (kaptan okunduysa).
    #[must_use]
    pub fn duration_ms(&self) -> Option<u64> {
        self.duration_ms
    }

    /// Çalma bitti mi (kaynak tükendi ve tampon boşaldı).
    #[must_use]
    pub fn finished(&self) -> bool {
        self.shared.drained.load(Ordering::Acquire) && self.shared.ring.len() == 0
    }

    /// Duraklatır. Tampon korunur, pozisyon donar.
    pub fn pause(&self) {
        self.shared.set_state(PlayState::Paused);
    }

    /// Duraklatılmışsa sürdürür.
    pub fn resume(&self) {
        if self.shared.state() == PlayState::Paused {
            self.shared.set_state(PlayState::Playing);
        }
    }

    /// Çözme sırasında oluşan hatayı alır (varsa) ve temizler.
    ///
    /// Sessizce yutulmasın diye: çağıran bunu okuyup tanı kaydına yazmalı (K9).
    pub fn take_error(&self) -> Option<String> {
        self.shared.error.lock().ok().and_then(|mut e| e.take())
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        self.shared.ring.clear();
        // Akışı önce kapat: geri çağrı artık paylaşılan duruma dokunmasın.
        drop(self.stream.take());
        if let Some(handle) = self.decoder.take() {
            let _ = handle.join();
        }
    }
}

/// Symphonia tamponunu araya geçmiş `f32` örneklere çevirir.
fn interleave_f32(buffer: &GenericAudioBufferRef<'_>, out: &mut Vec<f32>) {
    out.clear();
    let frames = buffer.frames();
    let channels = buffer.spec().channels().count();
    out.reserve(frames * channels);
    // `copy_to_vec_interleaved` tip dönüşümünü de yapar.
    buffer.copy_to_vec_interleaved(out);
}

/// Kaynak örnekleri çıkış hızına/kanal sayısına uydurup tampona yazar.
///
/// En yakın komşu örnekleme: basit ve ucuz. Faz 1'in hedefi doğru ses
/// üretmek; kaliteli yeniden örnekleme (sinc) gerekirse ayrıca ölçülür.
fn resample_into(
    input: &[f32],
    source_rate: u32,
    source_channels: usize,
    out_rate: u32,
    out_channels: usize,
    shared: &Arc<Shared>,
    capacity: usize,
) {
    if input.is_empty() || source_channels == 0 {
        return;
    }
    let in_frames = input.len() / source_channels;
    if in_frames == 0 {
        return;
    }

    // Aynı hız ve kanal sayısı: dönüştürme yok.
    if source_rate == out_rate && source_channels == out_channels {
        write_all(shared, input, capacity);
        return;
    }

    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "örnek indeksleri; ses ölçeğinde kayıp duyulmaz"
    )]
    let out_frames = ((in_frames as f64) * f64::from(out_rate) / f64::from(source_rate)) as usize;
    let mut converted = Vec::with_capacity(out_frames * out_channels);

    for frame in 0..out_frames {
        #[expect(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "en yakın komşu örnekleme"
        )]
        let src_frame = ((frame as f64) * f64::from(source_rate) / f64::from(out_rate)) as usize;
        let src_frame = src_frame.min(in_frames - 1);
        let base = src_frame * source_channels;

        for channel in 0..out_channels {
            // Mono → çok kanal: aynı örnek kopyalanır.
            // Çok kanal → az kanal: fazlası düşer.
            let src_channel = if source_channels == 1 {
                0
            } else {
                channel.min(source_channels - 1)
            };
            converted.push(input.get(base + src_channel).copied().unwrap_or(0.0));
        }
    }
    write_all(shared, &converted, capacity);
}

/// Tampona yazar; dolu ise yer açılana kadar bekler.
fn write_all(shared: &Arc<Shared>, mut data: &[f32], capacity: usize) {
    let _ = capacity;
    while !data.is_empty() {
        if shared.stop.load(Ordering::Acquire) {
            return;
        }
        let leftover = shared.ring.push(data);
        if leftover == 0 {
            return;
        }
        data = &data[data.len() - leftover..];
        std::thread::sleep(std::time::Duration::from_millis(3));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_reports_what_it_could_not_take() {
        let ring = RingBuffer::new(4);
        assert_eq!(ring.push(&[1.0, 2.0]), 0, "yer varken hepsi alınmalı");
        assert_eq!(ring.push(&[3.0, 4.0, 5.0]), 1, "1 örnek sığmamalı");
        assert_eq!(ring.len(), 4);
    }

    #[test]
    fn ring_buffer_pads_with_silence_when_starved() {
        let ring = RingBuffer::new(8);
        ring.push(&[1.0, 2.0]);
        let mut out = [9.0; 4];
        let given = ring.pop_into(&mut out);
        assert_eq!(given, 2, "yalnızca 2 örnek verilebilir");
        assert_eq!(out, [1.0, 2.0, 0.0, 0.0], "kalanı sessizlik olmalı");
    }

    #[test]
    fn state_survives_the_atomic_round_trip() {
        for state in [
            PlayState::Stopped,
            PlayState::Playing,
            PlayState::Paused,
            PlayState::Buffering,
        ] {
            assert_eq!(u8_to_state(state_to_u8(state)), state);
        }
    }

    #[test]
    fn position_is_zero_before_any_frame_is_played() {
        let shared = Shared {
            ring: RingBuffer::new(16),
            frames_played: AtomicU64::new(0),
            sample_rate: AtomicU64::new(48_000),
            channels: AtomicU64::new(2),
            state: AtomicU8::new(STATE_BUFFERING),
            stop: AtomicBool::new(false),
            drained: AtomicBool::new(false),
            error: Mutex::new(None),
        };
        assert_eq!(shared.position_ms(), 0);
        shared.frames_played.store(48_000, Ordering::Release);
        assert_eq!(shared.position_ms(), 1000, "48k kare = 1 saniye");
    }

    #[test]
    fn resampling_mono_to_stereo_duplicates_the_channel() {
        let shared = Arc::new(Shared {
            ring: RingBuffer::new(64),
            frames_played: AtomicU64::new(0),
            sample_rate: AtomicU64::new(8000),
            channels: AtomicU64::new(2),
            state: AtomicU8::new(STATE_PLAYING),
            stop: AtomicBool::new(false),
            drained: AtomicBool::new(false),
            error: Mutex::new(None),
        });
        // 2 kare mono, aynı hız → 2 kare stereo.
        resample_into(&[0.5, -0.5], 8000, 1, 8000, 2, &shared, 64);

        let mut out = [0.0; 4];
        let given = shared.ring.pop_into(&mut out);
        assert_eq!(given, 4);
        assert_eq!(
            out,
            [0.5, 0.5, -0.5, -0.5],
            "mono örnek iki kanala kopyalanmalı"
        );
    }

    #[test]
    fn resampling_doubles_the_frames_when_the_rate_doubles() {
        let shared = Arc::new(Shared {
            ring: RingBuffer::new(1024),
            frames_played: AtomicU64::new(0),
            sample_rate: AtomicU64::new(16_000),
            channels: AtomicU64::new(1),
            state: AtomicU8::new(STATE_PLAYING),
            stop: AtomicBool::new(false),
            drained: AtomicBool::new(false),
            error: Mutex::new(None),
        });
        // 4 kare @8kHz → 16kHz'de 8 kare.
        resample_into(&[1.0, 2.0, 3.0, 4.0], 8000, 1, 16_000, 1, &shared, 1024);
        assert_eq!(shared.ring.len(), 8);
    }
}
