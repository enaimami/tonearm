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

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};
use crate::provider::AudioSource;

use super::anchor::PlayState;

/// URL'nin yol kısmındaki uzantıyı çıkarır (`.../a.flac?x=1` → `flac`).
///
/// Yalnızca bir **ipucu** üretir: bulunamazsa symphonia kabı içeriğinden
/// tanır. Uydurma bir uzantı vermek tanımayı yanlış yöne çevirirdi.
#[cfg(feature = "http-client")]
fn extension_from_url(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next()?;
    let last = path.rsplit('/').next()?;
    let ext = last.rsplit_once('.')?.1;
    let plausible =
        !ext.is_empty() && ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric());
    plausible.then(|| ext.to_ascii_lowercase())
}

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

/// Çıkışa yazılmış tek bir parçanın dilimi (D-024).
///
/// Gapless'ın muhasebesi bu: halka tamponu artık birden çok parçanın
/// örneklerini yan yana taşıyabildiği için "pozisyon" tek bir sayaçtan
/// okunamaz. Her parça çıkış karesi cinsinden nerede başladığını bilir;
/// çalan parça, `frames_played`'in hangi dilime düştüğüdür.
#[derive(Debug, Clone)]
struct Span {
    /// [`AudioEngine::enqueue`]'nun döndürdüğü sıra numarası.
    seq: u64,
    /// Bu parçanın ilk karesinin çıkıştaki mutlak indeksi.
    ///
    /// Sıraya girmiş ama çözülmeye **başlanmamış** parçada `None`: nerede
    /// başlayacağı, kendinden öncekinin kaç kare yazdığına bağlı ve bu
    /// ancak o parça bitince belli olur.
    start_frame: Option<u64>,
    /// Kaç kare yazıldı. Çözme sürerken `None`.
    frames: Option<u64>,
    duration_ms: Option<u64>,
}

/// Çözücüye verilecek iş.
///
/// İki biçim var çünkü iki yol var: kullanıcı "çal" dediğinde kaynak
/// **çağıranın iş parçacığında** açılır (olmayan dosya hemen hata versin,
/// sessizce arka planda kaybolmasın), önden okumada ise çözücünün kendi
/// iş parçacığında açılır — açma gecikmesi tampon boşalırken gizlensin diye.
enum Job {
    Prepared(Box<PreparedTrack>),
    Lazy { seq: u64, source: AudioSource },
}

/// Açılmış, çözülmeye hazır parça.
struct PreparedTrack {
    seq: u64,
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    source_rate: u32,
    source_channels: usize,
    duration_ms: Option<u64>,
}

/// Çözme iş parçacığı ile oynatıcı arasındaki paylaşılan durum.
struct Shared {
    ring: RingBuffer,
    /// Çıkışa verilmiş toplam kare sayısı — **parçalar boyunca artar**,
    /// parça başında sıfırlanmaz. Parça içi pozisyon dilimden hesaplanır.
    frames_played: AtomicU64,
    /// Tampona şimdiye kadar yazılmış toplam kare. Yeni dilimin başlangıcı.
    frames_queued: AtomicU64,
    /// Çıkışın örnekleme hızı (kare/saniye).
    sample_rate: AtomicU64,
    /// Kanal sayısı.
    channels: AtomicU64,
    /// `PlayState` — atomik olarak taşınabilsin diye `u8`.
    state: AtomicU8,
    /// Çözme iş parçacığına "dur" işareti.
    stop: AtomicBool,
    /// Çözecek iş kalmadı: ne elde parça var ne kuyrukta. Tampon da boşalınca
    /// çalma biter. (Eski `drained`'in gapless'taki karşılığı.)
    idle: AtomicBool,
    /// Çözülmeyi bekleyen işler. Gapless burada oluyor: çalan parça biterken
    /// sıradaki zaten sırada, aygıt kapanmıyor.
    pending: Mutex<VecDeque<Job>>,
    /// Çıkışa yazılmış parça dilimleri, sırayla.
    spans: Mutex<Vec<Span>>,
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

    /// Kare sayısını milisaniyeye çevirir.
    fn frames_to_ms(&self, frames: u64) -> u64 {
        let rate = self.sample_rate.load(Ordering::Acquire);
        if rate == 0 {
            return 0;
        }
        frames.saturating_mul(1000) / rate
    }

    /// Çıkışın şu an hangi dilimde olduğunu bulur.
    ///
    /// Yalnızca **başlamış** dilimler sayılır: sıradaki parça daha çalınmadan
    /// "çalıyor" görünürse geçiş erken duyurulur, arayüz de scrobble da
    /// yanlış parçayı gösterir.
    fn current_span(&self) -> Option<Span> {
        let played = self.frames_played.load(Ordering::Acquire);
        let spans = self.spans.lock().ok()?;
        spans
            .iter()
            .rev()
            .find(|span| span.start_frame.is_some_and(|start| start <= played))
            .cloned()
    }

    /// Gösterilecek süre: çalan parçanınki, henüz başlamadıysa sıradakinin.
    ///
    /// Motor yeni kurulduğunda çözme iş parçacığı daha ilk kareyi yazmamış
    /// olabilir; "süre bilinmiyor" demek yerine sıradaki parçanınkini
    /// veriyoruz — kullanıcının duyacağı parça o.
    fn display_duration_ms(&self) -> Option<u64> {
        if let Some(span) = self.current_span() {
            return span.duration_ms;
        }
        let spans = self.spans.lock().ok()?;
        spans
            .iter()
            .find(|span| span.start_frame.is_none())
            .and_then(|span| span.duration_ms)
    }

    /// Bir dilimin içinde çalınmış süre (ms).
    ///
    /// Dilim bittiyse ve çıkış onu geçtiyse sonuç parçanın tamamıdır —
    /// scrobble'ın ihtiyacı olan sayı bu (§1.6).
    fn played_ms_of(&self, span: &Span) -> u64 {
        let Some(start) = span.start_frame else {
            // Hiç başlamadı: çalınmış süresi sıfırdır.
            return 0;
        };
        let played = self.frames_played.load(Ordering::Acquire);
        let offset = played.saturating_sub(start);
        let clamped = match span.frames {
            Some(total) => offset.min(total),
            None => offset,
        };
        self.frames_to_ms(clamped)
    }

    /// Çalan parçanın kendi içindeki pozisyonu (ms).
    fn position_ms(&self) -> u64 {
        self.current_span()
            .map_or(0, |span| self.played_ms_of(&span))
    }

    /// Dilimi kaydeder (henüz başlamadı). Varsa süresini tazeler.
    fn declare_span(&self, seq: u64, duration_ms: Option<u64>) {
        if let Ok(mut spans) = self.spans.lock() {
            if let Some(span) = spans.iter_mut().find(|span| span.seq == seq) {
                span.duration_ms = span.duration_ms.or(duration_ms);
                return;
            }
            spans.push(Span {
                seq,
                start_frame: None,
                frames: None,
                duration_ms,
            });
        }
    }

    /// Dilimi başlatır: ilk karesinin çıkıştaki yeri artık belli.
    fn begin_span(&self, seq: u64) {
        let start = self.frames_queued.load(Ordering::Acquire);
        if let Ok(mut spans) = self.spans.lock()
            && let Some(span) = spans.iter_mut().find(|span| span.seq == seq)
        {
            span.start_frame = Some(start);
        }
    }

    /// Dilimi kapatır: kaç kare yazıldığı kesinleşir.
    fn close_span(&self, seq: u64) {
        let queued = self.frames_queued.load(Ordering::Acquire);
        if let Ok(mut spans) = self.spans.lock()
            && let Some(span) = spans.iter_mut().find(|span| span.seq == seq)
            && let Some(start) = span.start_frame
        {
            span.frames = Some(queued.saturating_sub(start));
        }
    }

    /// Tampona yazılan kareleri sayar (kanal sayısına bölünmüş).
    fn add_queued_samples(&self, samples: usize) {
        let channels = self.channels.load(Ordering::Acquire).max(1);
        self.frames_queued
            .fetch_add(samples as u64 / channels, Ordering::AcqRel);
    }

    fn record_error(&self, detail: String) {
        if let Ok(mut slot) = self.error.lock() {
            if slot.is_none() {
                *slot = Some(detail);
            }
        }
    }
}

/// Ses motoru: **bir** çıkış akışı, sırayla beslenen parçalar (D-024).
///
/// Gapless'ın tamamı burada: cpal akışı ve halka tamponu parçalar arasında
/// **açık kalır**. Çalan parça biterken sıradakinin örnekleri aynı tampona
/// eklenmiştir, aygıt hiç kapanıp açılmaz. Boşluğun eski kaynağı da buydu:
/// her parça için yeni bir aygıt + yeni bir çözücü kuruluyordu.
///
/// `Drop` ile akış kapanır ve çözme iş parçacığı durur — sızıntı bırakmaz.
pub struct AudioEngine {
    shared: Arc<Shared>,
    stream: Option<cpal::Stream>,
    decoder: Option<std::thread::JoinHandle<()>>,
    /// Sıradaki işe verilecek numara.
    next_seq: std::sync::atomic::AtomicU64,
}

impl std::fmt::Debug for AudioEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioEngine")
            .field("state", &self.shared.state())
            .field("position_ms", &self.shared.position_ms())
            .field("queued", &self.queued_len())
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
        // Kaynak **aygıttan önce** açılıyor: bozuk ya da olmayan dosya, ses
        // çıkışı hiç bulunmayan bir ortamda (CI) da çözme aşamasında hata
        // vermeli — "aygıt yok" hatası asıl sebebi gizlerdi.
        let prepared = open_source(&AudioSource::LocalFile {
            path: path.to_path_buf(),
        })?;
        let engine = Self::open()?;
        engine.play_prepared(prepared);
        Ok(engine)
    }

    /// Bir HTTP akışını çalar (§1.3: Subsonic / Jellyfin).
    ///
    /// Baytlar arka planda inerken çözme başlar; ayrıntı
    /// [`super::http_source`]. **K3:** akışı istemci kendi çekiyor, hiçbir
    /// sunucu araya girip röle etmiyor.
    ///
    /// # Errors
    /// Bağlantı kurulamaz ya da sunucu 2xx dışında dönerse
    /// ([`Stage::NetworkRequest`]); ses çözülemezse ([`Stage::PlaybackDecode`]).
    #[cfg(feature = "http-client")]
    pub fn play_http(url: &str, headers: &[crate::net::HttpHeader]) -> Result<Self> {
        let prepared = open_source(&AudioSource::HttpStream {
            url: url.to_owned(),
            headers: headers.to_vec(),
        })?;
        let engine = Self::open()?;
        engine.play_prepared(prepared);
        Ok(engine)
    }

    /// Bir kaynağı **hemen** sıraya koyar; açma çağıranın iş parçacığında olur.
    ///
    /// Olmayan dosya ya da erişilemeyen sunucu burada hata döner — arka planda
    /// sessizce kaybolmaz. Dönen sayı dilim numarasıdır.
    ///
    /// # Errors
    /// Kaynak açılamaz ya da çözülemezse.
    pub fn play_source(&self, source: &AudioSource) -> Result<u64> {
        Ok(self.play_prepared(open_source(source)?))
    }

    /// Açılmış bir parçayı sıraya koyar ve dilim numarasını verir.
    fn play_prepared(&self, mut prepared: PreparedTrack) -> u64 {
        let seq = self.next_seq.fetch_add(1, Ordering::AcqRel);
        prepared.seq = seq;
        // Dilimi **şimdi** bildiriyoruz: süre burada biliniyor ve çağıran
        // motoru kurar kurmaz `duration_ms()` sorabiliyor. Nerede başlayacağı
        // ise çözme iş parçacığı oraya gelince belli olur.
        self.shared.declare_span(seq, prepared.duration_ms);
        // İş **gönderilir gönderilmez** boşta değiliz. Bunu çözme iş
        // parçacığına bırakmak, o uyanana kadar geri çağrının "tampon boş +
        // boşta = bitti" deyip Stopped basmasına yol açıyordu; kullanıcı
        // henüz başlamamış bir parçayı bitmiş görürdü.
        self.shared.idle.store(false, Ordering::Release);
        // Durumu da **şimdi** düzeltiyoruz, ilk geri çağrıyı bekleyerek değil.
        // `open()` ile bu an arasında geri çağrı `idle`'ı doğru görüp `Stopped`
        // basmış olabilir ve o `Stopped` yanlıştır: iş kuyrukta, henüz
        // çalınmadı. Aradaki pencere yerel dosyada birkaç milisaniye, HTTP
        // akışında saniyelerdi (`open_source` indiriyor) — `Session::play`'in
        // döngüsü orada "başlamadan bitti" deyip çıkıyordu, sessizce ve
        // sıfır dinlemeyle. `Buffering` "hattın beklemesi"dir (D-016) ve
        // burada anlatılmak istenen tam olarak odur.
        self.shared.set_state(PlayState::Buffering);
        if let Ok(mut pending) = self.shared.pending.lock() {
            pending.push_back(Job::Prepared(Box::new(prepared)));
        }
        seq
    }

    /// Bir kaynağı **önden okuma** olarak sıraya koyar (gapless).
    ///
    /// Kaynak çözme iş parçacığında açılır: açma gecikmesi çalan parçanın
    /// tamponu boşalırken gizlenir. Açılamazsa hata `take_error` ile
    /// toplanır — çağıran o an bekliyor olmadığı için `Result` dönmüyoruz,
    /// ama hata **yutulmuyor** (K9).
    pub fn enqueue(&self, source: &AudioSource) -> u64 {
        let seq = self.next_seq.fetch_add(1, Ordering::AcqRel);
        self.shared.idle.store(false, Ordering::Release);
        if let Ok(mut pending) = self.shared.pending.lock() {
            pending.push_back(Job::Lazy {
                seq,
                source: source.clone(),
            });
        }
        seq
    }

    /// Sırada bekleyen (henüz çözülmeye başlanmamış) iş sayısı.
    #[must_use]
    pub fn queued_len(&self) -> usize {
        self.shared
            .pending
            .lock()
            .map_or(0, |pending| pending.len())
    }

    /// Çıkışın şu an çaldığı dilimin numarası.
    ///
    /// Bu değer değiştiğinde parça geçişi **duyulmuş** demektir; oynatıcı
    /// scrobble'ı ve kuyruk imlecini buna göre ilerletir.
    #[must_use]
    pub fn current_seq(&self) -> Option<u64> {
        self.shared.current_span().map(|span| span.seq)
    }

    /// Belirli bir dilimin çalınmış süresi (ms). Bilinmiyorsa `None`.
    #[must_use]
    pub fn played_ms_of(&self, seq: u64) -> Option<u64> {
        let spans = self.shared.spans.lock().ok()?;
        let span = spans.iter().rev().find(|span| span.seq == seq)?;
        Some(self.shared.played_ms_of(span))
    }

    /// Belirli bir dilimin **kaptan okunmuş** toplam süresi.
    ///
    /// Etikette süre olmayan dosyalarda tek güvenilir kaynak budur; kütüphane
    /// katalogu etiketten beslendiği için orada `None` olabilir.
    #[must_use]
    pub fn duration_of(&self, seq: u64) -> Option<u64> {
        let spans = self.shared.spans.lock().ok()?;
        spans.iter().rev().find(|span| span.seq == seq)?.duration_ms
    }

    /// Ses aygıtını açar ve çözme iş parçacığını başlatır; henüz parça yok.
    ///
    /// # Errors
    /// Ses aygıtı bulunamaz ya da akış kurulamazsa ([`Stage::PlaybackOutput`]).
    pub fn open() -> Result<Self> {
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
            frames_queued: AtomicU64::new(0),
            sample_rate: AtomicU64::new(u64::from(out_rate)),
            channels: AtomicU64::new(out_channels as u64),
            state: AtomicU8::new(STATE_BUFFERING),
            stop: AtomicBool::new(false),
            idle: AtomicBool::new(true),
            pending: Mutex::new(VecDeque::new()),
            spans: Mutex::new(Vec::new()),
            error: Mutex::new(None),
        });

        // — Çözme iş parçacığı: ömrü motorun ömrü kadar, parçanın değil.
        // Bir parça bitince kuyruktaki sıradakini alıp **aynı tampona**
        // yazmayı sürdürüyor; gapless tam olarak bu.
        let decode_shared = Arc::clone(&shared);
        let decoder_thread = std::thread::Builder::new()
            .name("tune-decode".to_owned())
            .spawn(move || {
                let mut scratch: Vec<f32> = Vec::new();
                loop {
                    if decode_shared.stop.load(Ordering::Acquire) {
                        break;
                    }

                    let Some(job) = take_job(&decode_shared) else {
                        // İş yok: çalma bitmiş olabilir ama motor ayakta.
                        // Çağıran yeni parça verirse buradan devam eder.
                        decode_shared.idle.store(true, Ordering::Release);
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        continue;
                    };

                    let prepared = match job {
                        Job::Prepared(track) => *track,
                        Job::Lazy { seq, source } => match open_source(&source) {
                            Ok(mut track) => {
                                track.seq = seq;
                                track
                            }
                            Err(err) => {
                                // Önden okunan parça açılamadı: yutma, kaydet
                                // ve sıradakine geç (K9).
                                decode_shared
                                    .record_error(err.chain_text().replace('\n', " ").to_string());
                                continue;
                            }
                        },
                    };

                    decode_shared.idle.store(false, Ordering::Release);
                    decode_track(
                        &decode_shared,
                        prepared,
                        out_rate,
                        out_channels,
                        capacity,
                        &mut scratch,
                    );
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
                    } else if cb_shared.idle.load(Ordering::Acquire) {
                        // Tampon boş ve çözecek iş de yok: gerçekten bitti.
                        cb_shared.set_state(PlayState::Stopped);
                    } else {
                        // Veri bitti ama çözülecek parça var: bekliyoruz.
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
            next_seq: AtomicU64::new(0),
        })
    }

    /// Şu anki durum.
    #[must_use]
    pub fn state(&self) -> PlayState {
        self.shared.state()
    }

    /// Çalan parçanın **kendi içindeki** pozisyonu.
    #[must_use]
    pub fn position_ms(&self) -> u64 {
        self.shared.position_ms()
    }

    /// Çalan parçanın süresi (kaptan okunduysa).
    #[must_use]
    pub fn duration_ms(&self) -> Option<u64> {
        self.shared.display_duration_ms()
    }

    /// Çalacak bir şey kalmadı mı (kuyruk boş, çözme bitti, tampon boşaldı).
    ///
    /// Gapless'ta bu **parça bitti** demek değil: sıradaki parça varken motor
    /// bitmiş sayılmaz, yalnızca dilim değişir.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.shared.idle.load(Ordering::Acquire)
            && self.shared.ring.len() == 0
            && self.queued_len() == 0
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

/// Kuyruktan bir iş alır (varsa).
fn take_job(shared: &Arc<Shared>) -> Option<Job> {
    shared.pending.lock().ok()?.pop_front()
}

/// Bir [`AudioSource`]'u açıp çözülmeye hazır hâle getirir.
///
/// `play_source` bunu çağıranın iş parçacığında, önden okuma ise çözme
/// iş parçacığında çağırır — gövde ikisinde de aynı.
fn open_source(source: &AudioSource) -> Result<PreparedTrack> {
    match source {
        AudioSource::LocalFile { path } => {
            let file = std::fs::File::open(path)
                .map_err(|err| crate::error::io_err(Stage::PlaybackDecode, path, err))?;
            let mut hint = Hint::new();
            if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
                hint.with_extension(ext);
            }
            prepare(Box::new(file), hint, &path.display().to_string())
        }
        #[cfg(feature = "http-client")]
        AudioSource::HttpStream { url, headers } => {
            let stream = super::http_source::HttpMediaSource::open(url, headers)?;
            let mut hint = Hint::new();
            // Uzantı yalnızca bir **ipucu**: sunucu `?id=...` gibi uzantısız
            // adresler veriyor, o zaman symphonia kabı içeriğinden tanıyor.
            if let Some(ext) = extension_from_url(url) {
                hint.with_extension(&ext);
            }
            prepare(Box::new(stream), hint, url)
        }
        #[cfg(not(feature = "http-client"))]
        AudioSource::HttpStream { .. } => Err(Error::new(
            Stage::PlaybackResolve,
            ErrorKind::Unsupported {
                provider: "net".to_owned(),
                what: "HTTP akışı (`http-client` feature'ı kapalı derleme)".to_owned(),
                capabilities: "STREAM".to_owned(),
            },
        )),
    }
}

/// Açılmış bir kaynağı tanır, çözücüsünü kurar ve süresini okur.
fn prepare(source: Box<dyn MediaSource>, hint: Hint, label: &str) -> Result<PreparedTrack> {
    let mss = MediaSourceStream::new(source, Default::default());

    let format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|err| audio_err(Stage::PlaybackDecode, format!("{label} açılamadı: {err}")))?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| audio_err(Stage::PlaybackDecode, format!("{label} içinde ses izi yok")))?;
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
                format!("{label} için kod çözücü parametreleri okunamadı"),
            )
        })?
        .clone();

    let source_rate = codec_params.sample_rate.unwrap_or(0);
    let source_channels = codec_params
        .channels
        .as_ref()
        .map_or(0, symphonia::core::audio::Channels::count);

    let decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&codec_params, &AudioDecoderOptions::default())
        .map_err(|err| {
            audio_err(
                Stage::PlaybackDecode,
                format!("kod çözücü kurulamadı: {err}"),
            )
        })?;

    Ok(PreparedTrack {
        // Numara sıraya konurken atanır; açma anında henüz belli değil.
        seq: 0,
        format,
        decoder,
        track_id,
        source_rate,
        source_channels,
        duration_ms,
    })
}

/// Tek bir parçayı sonuna kadar çözüp tampona yazar.
///
/// Dönerken parçanın dilimi kapanmıştır; çağıran döngü sıradaki işe geçer.
/// **Tampon boşaltılmaz** — gapless'ın çalışma biçimi bu: sıradaki parçanın
/// örnekleri bitenin arkasına eklenir.
fn decode_track(
    shared: &Arc<Shared>,
    prepared: PreparedTrack,
    out_rate: u32,
    out_channels: usize,
    capacity: usize,
    scratch: &mut Vec<f32>,
) {
    let PreparedTrack {
        seq,
        mut format,
        mut decoder,
        track_id,
        source_rate,
        source_channels,
        duration_ms,
    } = prepared;

    // Kaptan okunamadıysa çıkışın değerlerine düş: dönüştürme yapılmaz.
    let source_rate = if source_rate == 0 {
        out_rate
    } else {
        source_rate
    };
    let source_channels = if source_channels == 0 {
        out_channels
    } else {
        source_channels
    };

    shared.declare_span(seq, duration_ms);
    shared.begin_span(seq);

    loop {
        if shared.stop.load(Ordering::Acquire) {
            break;
        }
        // Tampon doluysa bekle: çözme, çalmanın önüne geçmesin.
        if shared.ring.len() >= capacity.saturating_sub(capacity / 8) {
            std::thread::sleep(std::time::Duration::from_millis(5));
            continue;
        }

        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(err) => {
                shared.record_error(format!("paket okunamadı: {err}"));
                break;
            }
        };
        if packet.track_id != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(buffer) => {
                interleave_f32(&buffer, scratch);
                resample_into(
                    scratch,
                    source_rate,
                    source_channels,
                    out_rate,
                    out_channels,
                    shared,
                    capacity,
                );
            }
            Err(symphonia::core::errors::Error::DecodeError(msg)) => {
                // Tek bozuk paket parçayı düşürmemeli; say ve devam et.
                tracing::warn!(hata = %msg, "paket çözülemedi, atlanıyor");
            }
            Err(err) => {
                shared.record_error(format!("çözme durdu: {err}"));
                break;
            }
        }
    }

    shared.close_span(seq);
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
///
/// Yazılan her örnek `frames_queued`'a da işlenir: dilim sınırlarının
/// (dolayısıyla pozisyonun) dayandığı sayaç bu.
fn write_all(shared: &Arc<Shared>, mut data: &[f32], capacity: usize) {
    let _ = capacity;
    while !data.is_empty() {
        if shared.stop.load(Ordering::Acquire) {
            return;
        }
        let leftover = shared.ring.push(data);
        let written = data.len() - leftover;
        if written > 0 {
            shared.add_queued_samples(written);
        }
        if leftover == 0 {
            return;
        }
        data = &data[written..];
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

    /// Yeni gönderilen iş, motoru **anında** "bitmemiş" saymalı.
    ///
    /// Regresyon: `open()` ses akışını hemen başlatıyor ve geri çağrı boş
    /// tampon + `idle` görüp `Stopped` basıyordu. `play_source` bu durumu
    /// düzeltmeyip ilk geri çağrıya bıraktığı için arada bir pencere kalıyordu;
    /// `Session::play`'in döngüsü oraya denk gelince "başlamadan bitti" deyip
    /// sessizce çıkıyordu. Pencere yerel dosyada milisaniyeydi, HTTP akışında
    /// saniyeler (`open_source` indiriyor) — yani kusur yalnızca uzak
    /// kaynakta görünüyordu.
    ///
    /// Ses aygıtı yoksa test kendini atlar, sebebini yazarak.
    #[test]
    fn a_freshly_queued_track_is_never_reported_as_stopped() {
        let Ok(engine) = AudioEngine::open() else {
            eprintln!("ses aygıtı yok — atlanıyor (bu bir başarısızlık değil)");
            return;
        };

        // Geri çağrının en az bir kez çalışıp `Stopped` basmasını bekle:
        // testin ön koşulu, kusurun oluştuğu başlangıç durumu.
        std::thread::sleep(std::time::Duration::from_millis(80));
        assert_eq!(
            engine.state(),
            PlayState::Stopped,
            "ön koşul: boş motor durmuş görünmeli"
        );

        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/audio/tagged.flac");
        let source = AudioSource::LocalFile { path: fixture };
        let Ok(_seq) = engine.play_source(&source) else {
            eprintln!("fixture çözülemedi — atlanıyor");
            return;
        };

        // Uyku **yok**: düzeltmenin bütün mesele ettiği şey, durumun geri
        // çağrıyı beklemeden doğru olması.
        assert_ne!(
            engine.state(),
            PlayState::Stopped,
            "iş kuyruktayken motor bitmiş görünemez"
        );
    }

    /// Test için çıplak paylaşılan durum (aygıt açmadan).
    fn shared_for_test(capacity: usize, rate: u64, channels: u64) -> Arc<Shared> {
        Arc::new(Shared {
            ring: RingBuffer::new(capacity),
            frames_played: AtomicU64::new(0),
            frames_queued: AtomicU64::new(0),
            sample_rate: AtomicU64::new(rate),
            channels: AtomicU64::new(channels),
            state: AtomicU8::new(STATE_BUFFERING),
            stop: AtomicBool::new(false),
            idle: AtomicBool::new(true),
            pending: Mutex::new(VecDeque::new()),
            spans: Mutex::new(Vec::new()),
            error: Mutex::new(None),
        })
    }

    #[test]
    fn position_is_zero_before_any_frame_is_played() {
        let shared = shared_for_test(16, 48_000, 2);
        // Dilim yokken pozisyon sıfır: hangi parçanın içinde olduğumuz bilinmiyor.
        assert_eq!(shared.position_ms(), 0);

        shared.declare_span(0, Some(5_000));
        shared.begin_span(0);
        assert_eq!(shared.position_ms(), 0);
        shared.frames_played.store(48_000, Ordering::Release);
        assert_eq!(shared.position_ms(), 1000, "48k kare = 1 saniye");
    }

    /// D-024'ün muhasebesi: tampon iki parçayı yan yana taşırken pozisyon
    /// **parçanın kendi içindeki** konumu vermeli, çıkışın toplamını değil.
    #[test]
    fn position_restarts_at_each_track_boundary() {
        let shared = shared_for_test(16, 48_000, 2);

        // Birinci parça: 2 saniye (96k kare) yazıldı.
        shared.declare_span(0, Some(2_000));
        shared.begin_span(0);
        shared.frames_queued.store(96_000, Ordering::Release);
        shared.close_span(0);

        // İkinci parça hemen arkasına ekleniyor.
        shared.declare_span(1, Some(3_000));
        shared.begin_span(1);

        // Çıkış birinci parçanın ortasında.
        shared.frames_played.store(48_000, Ordering::Release);
        assert_eq!(shared.current_span().map(|s| s.seq), Some(0));
        assert_eq!(shared.position_ms(), 1000);

        // Sınırı geçti: artık ikinci parça çalıyor ve pozisyonu **baştan**.
        shared
            .frames_played
            .store(96_000 + 24_000, Ordering::Release);
        assert_eq!(shared.current_span().map(|s| s.seq), Some(1));
        assert_eq!(shared.position_ms(), 500, "yeni parçanın kendi pozisyonu");
        assert_eq!(
            shared.current_span().and_then(|s| s.duration_ms),
            Some(3_000),
            "süre de yeni parçanın süresi olmalı"
        );

        // Biten parçanın scrobble'ı tam uzunluğunu görmeli, çıkışın toplamını değil.
        let spans = shared.spans.lock().unwrap();
        let first = spans.iter().find(|s| s.seq == 0).unwrap();
        assert_eq!(shared.played_ms_of(first), 2_000);
    }

    #[test]
    fn resampling_mono_to_stereo_duplicates_the_channel() {
        let shared = shared_for_test(64, 8000, 2);
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
        assert_eq!(
            shared.frames_queued.load(Ordering::Acquire),
            2,
            "yazılan kareler dilim muhasebesine işlenmeli"
        );
    }

    #[test]
    fn resampling_doubles_the_frames_when_the_rate_doubles() {
        let shared = shared_for_test(1024, 16_000, 1);
        // 4 kare @8kHz → 16kHz'de 8 kare.
        resample_into(&[1.0, 2.0, 3.0, 4.0], 8000, 1, 16_000, 1, &shared, 1024);
        assert_eq!(shared.ring.len(), 8);
    }
}
