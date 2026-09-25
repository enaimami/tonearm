//! Turns an HTTP stream into a source symphonia can read (§1.3).
//!
//! Why not [`crate::net::HttpClient`]: that trait takes the body into memory
//! **whole** and was designed for metadata calls. For audio, "start when it
//! has all arrived" is not acceptable — on a 40 MB FLAC it means seconds of
//! silence. The source here keeps downloading in the background while the
//! decoder can start reading the first bytes.
//!
//! ## Why we keep the whole buffer in memory
//!
//! While recognising the container, symphonia seeks **backwards** (FLAC/MP4
//! headers). Throwing away the downloaded bytes would mean opening a new HTTP
//! request (Range) for every seek; that ties the complexity to server
//! compatibility (not every server supports Range). A typical track is a few
//! tens of MB; the cap is explicitly limited with [`MAX_BUFFER_BYTES`], and
//! exceeding it gives an **error**, it does not cut off silently.

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use symphonia::core::io::MediaSource;

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};
use crate::net::{HttpHeader, UreqClient};

/// The memory cap for a single track. Exceeding it makes reading fail.
const MAX_BUFFER_BYTES: usize = 256 * 1024 * 1024;

/// The block the download thread reads at a time.
const CHUNK: usize = 64 * 1024;

/// How often the reader wakes up while waiting for data.
const WAIT: std::time::Duration = std::time::Duration::from_millis(50);

/// An HTTP source that downloads in the background and can be read ahead.
pub struct HttpMediaSource {
    shared: Arc<Shared>,
    pos: u64,
    /// The total length, if `Content-Length` is known. If not, the source
    /// counts as "not seekable" — symphonia then works in streaming mode.
    total: Option<u64>,
}

struct Shared {
    data: Mutex<Vec<u8>>,
    /// Has the download finished (successfully or with an error).
    done: AtomicBool,
    /// The download error; the reader turns it into an `io::Error`.
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
    /// Opens the stream and starts the download in the background.
    ///
    /// # Errors
    /// If no connection can be made or the server returns a code outside 2xx
    /// ([`Stage::NetworkRequest`]) — that is, before a "started playing but no
    /// sound" situation can arise.
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
                        "{url} is {len} bytes; the cap for a single track is {MAX_BUFFER_BYTES} bytes"
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
                                writer.fail("could not lock the download buffer".to_owned());
                                break;
                            };
                            if data.len() + n > MAX_BUFFER_BYTES {
                                drop(data);
                                writer.fail(format!(
                                    "{label} exceeded the memory cap ({MAX_BUFFER_BYTES} bytes)"
                                ));
                                break;
                            }
                            data.extend_from_slice(&chunk[..n]);
                            drop(data);
                            writer.ready.notify_all();
                        }
                        Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                        Err(err) => {
                            writer.fail(format!("the {label} stream broke off: {err}"));
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
                        detail: format!("could not start the download thread: {source}"),
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
            .map_err(|_| io::Error::other("could not lock the download buffer"))?;

        loop {
            let available = data.len() as u64;
            if self.pos < available {
                let start = usize::try_from(self.pos).map_err(|_| {
                    io::Error::other("the position does not fit into a machine word")
                })?;
                let take = out.len().min(data.len() - start);
                out[..take].copy_from_slice(&data[start..start + take]);
                self.pos += take as u64;
                return Ok(take);
            }
            if self.shared.done.load(Ordering::Acquire) {
                // If there is an error we do not act as if it were the end of the
                // file: a silently truncated track is worse than one that fails.
                return match self.shared.take_error() {
                    Some(detail) => Err(io::Error::other(detail)),
                    None => Ok(0),
                };
            }
            let (guard, _) = self
                .shared
                .ready
                .wait_timeout(data, WAIT)
                .map_err(|_| io::Error::other("could not lock the download buffer"))?;
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
                        "the server reported no length; cannot seek from the end",
                    )
                })?;
                add_offset(total, delta)?
            }
        };
        // Jumping to a point not yet downloaded is not forbidden: reading
        // waits until those bytes arrive.
        self.pos = target;
        Ok(self.pos)
    }
}

fn add_offset(base: u64, delta: i64) -> io::Result<u64> {
    let result = i128::from(base) + i128::from(delta);
    if result < 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "seek before the start of the file",
        ));
    }
    u64::try_from(result).map_err(|_| io::Error::other("the seek position overflows"))
}

impl MediaSource for HttpMediaSource {
    fn is_seekable(&self) -> bool {
        self.total.is_some()
    }

    fn byte_len(&self) -> Option<u64> {
        self.total
    }
}
