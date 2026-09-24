//! HTTP akışını symphonia'nın okuyabileceği bir kaynağa çevirir (§1.3).
//!
//! Neden [`crate::net::HttpClient`] değil: o trait gövdeyi **tamamen** belleğe
//! alır ve üstveri çağrıları için tasarlandı. Ses için "hepsi inince başla"
//! kabul edilebilir değil — 40 MB'lık bir FLAC'ta saniyelerce sessizlik
//! demek. Buradaki kaynak indirmeyi arka planda sürdürürken çözücü ilk
//! baytları okumaya başlayabiliyor.
//!
//! ## Neden tamponu tümüyle bellekte tutuyoruz
//!
//! symphonia kabı tanırken **geriye doğru** arama yapıyor (FLAC/MP4 başlıkları).
//! İnen baytları atmak, her aramada yeni bir HTTP isteği (Range) açmak
//! demekti; bu, karmaşıklığı sunucu uyumluluğuna bağlar (her sunucu Range
//! desteklemiyor). Tipik bir parça birkaç on MB; tavan [`MAX_BUFFER_BYTES`]
//! ile açıkça sınırlı ve aşılırsa **hata** veriyor, sessizce kesmiyor.

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use symphonia::core::io::MediaSource;

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};
use crate::net::{HttpHeader, UreqClient};

/// Tek bir parça için bellek tavanı. Aşılırsa okuma hata verir.
const MAX_BUFFER_BYTES: usize = 256 * 1024 * 1024;

/// İndirme iş parçacığının bir seferde okuduğu blok.
const CHUNK: usize = 64 * 1024;

/// Okuyucunun veri beklerken uyanma aralığı.
const WAIT: std::time::Duration = std::time::Duration::from_millis(50);

/// Arka planda inen, önden okunabilen HTTP kaynağı.
pub struct HttpMediaSource {
    shared: Arc<Shared>,
    pos: u64,
    /// `Content-Length` biliniyorsa toplam uzunluk. Bilinmiyorsa kaynak
    /// "aranamaz" sayılır — symphonia o zaman akış kipinde çalışır.
    total: Option<u64>,
}

struct Shared {
    data: Mutex<Vec<u8>>,
    /// İndirme bitti mi (başarıyla ya da hatayla).
    done: AtomicBool,
    /// İndirme hatası; okuyucu bunu `io::Error`'a çevirir.
    error: Mutex<Option<String>>,
    ready: Condvar,
}

impl std::fmt::Debug for HttpMediaSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpMediaSource")
            .field("pos", &self.pos)
            .field("total", &self.total)
            .finish()
    }
}

impl HttpMediaSource {
    /// Akışı açar ve indirmeyi arka planda başlatır.
    ///
    /// # Errors
    /// Bağlantı kurulamazsa ya da sunucu 2xx dışında bir kod dönerse
    /// ([`Stage::NetworkRequest`]) — yani "çalmaya başladım ama ses yok"
    /// durumu oluşmadan önce.
    pub fn open(url: &str, headers: &[HttpHeader]) -> Result<Self> {
        let client = UreqClient::for_streams();
        let (total, mut reader) = client.open_stream(url, headers)?;

        if let Some(len) = total
            && len > MAX_BUFFER_BYTES as u64
        {
            return Err(Error::new(
                Stage::PlaybackDecode,
                ErrorKind::Audio {
                    detail: format!(
                        "{url} {len} bayt; tek parça için tavan {MAX_BUFFER_BYTES} bayt"
                    ),
                },
            ));
        }

        let shared = Arc::new(Shared {
            data: Mutex::new(Vec::with_capacity(
                usize::try_from(total.unwrap_or(0))
                    .unwrap_or(0)
                    .min(CHUNK * 16),
            )),
            done: AtomicBool::new(false),
            error: Mutex::new(None),
            ready: Condvar::new(),
        });

        let writer = Arc::clone(&shared);
        let label = url.to_owned();
        std::thread::Builder::new()
            .name("headshell-http-stream".to_owned())
            .spawn(move || {
                let mut chunk = vec![0u8; CHUNK];
                loop {
                    match reader.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => {
                            let Ok(mut data) = writer.data.lock() else {
                                writer.fail("indirme tamponu kilitlenemedi".to_owned());
                                break;
                            };
                            if data.len() + n > MAX_BUFFER_BYTES {
                                drop(data);
                                writer.fail(format!(
                                    "{label} bellek tavanını aştı ({MAX_BUFFER_BYTES} bayt)"
                                ));
                                break;
                            }
                            data.extend_from_slice(&chunk[..n]);
                            drop(data);
                            writer.ready.notify_all();
                        }
                        Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                        Err(err) => {
                            writer.fail(format!("{label} akışı kesildi: {err}"));
                            break;
                        }
                    }
                }
                writer.done.store(true, Ordering::Release);
                writer.ready.notify_all();
            })
            .map_err(|source| {
                Error::new(
                    Stage::PlaybackDecode,
                    ErrorKind::Audio {
                        detail: format!("indirme iş parçacığı başlatılamadı: {source}"),
                    },
                )
            })?;

        Ok(Self {
            shared,
            pos: 0,
            total,
        })
    }
}

impl Shared {
    fn fail(&self, detail: String) {
        if let Ok(mut slot) = self.error.lock() {
            *slot = Some(detail);
        }
        self.done.store(true, Ordering::Release);
        self.ready.notify_all();
    }

    fn take_error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|mut slot| slot.take())
    }
}

impl Read for HttpMediaSource {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let mut data = self
            .shared
            .data
            .lock()
            .map_err(|_| io::Error::other("indirme tamponu kilitlenemedi"))?;

        loop {
            let available = data.len() as u64;
            if self.pos < available {
                let start = usize::try_from(self.pos)
                    .map_err(|_| io::Error::other("konum makine sözcüğüne sığmıyor"))?;
                let take = out.len().min(data.len() - start);
                out[..take].copy_from_slice(&data[start..start + take]);
                self.pos += take as u64;
                return Ok(take);
            }
            if self.shared.done.load(Ordering::Acquire) {
                // Hata varsa dosya sonu gibi davranmıyoruz: sessizce kırpılmış
                // bir parça, hata veren bir parçadan daha kötüdür.
                return match self.shared.take_error() {
                    Some(detail) => Err(io::Error::other(detail)),
                    None => Ok(0),
                };
            }
            let (guard, _) = self
                .shared
                .ready
                .wait_timeout(data, WAIT)
                .map_err(|_| io::Error::other("indirme tamponu kilitlenemedi"))?;
            data = guard;
        }
    }
}

impl Seek for HttpMediaSource {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let target = match from {
            SeekFrom::Start(n) => n,
            SeekFrom::Current(delta) => add_offset(self.pos, delta)?,
            SeekFrom::End(delta) => {
                let total = self.total.ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::Unsupported,
                        "sunucu uzunluk bildirmedi; sondan arama yapılamıyor",
                    )
                })?;
                add_offset(total, delta)?
            }
        };
        // İnmemiş bir noktaya atlamak yasak değil: okuma o baytlar gelene
        // kadar bekler.
        self.pos = target;
        Ok(self.pos)
    }
}

fn add_offset(base: u64, delta: i64) -> io::Result<u64> {
    let result = i128::from(base) + i128::from(delta);
    if result < 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "dosya başından öncesine arama",
        ));
    }
    u64::try_from(result).map_err(|_| io::Error::other("arama konumu taşıyor"))
}

impl MediaSource for HttpMediaSource {
    fn is_seekable(&self) -> bool {
        self.total.is_some()
    }

    fn byte_len(&self) -> Option<u64> {
        self.total
    }
}
