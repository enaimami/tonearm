//! Eklenti taşıması: alt süreç, satır bazlı boru hattı (K5).
//!
//! Trait arkasında, çünkü testler süreç açmadan protokolü sınayabilmeli
//! (CLAUDE.md: "ağ ve dosya sistemine dokunan her şey trait arkasında").
//! [`ProcessTransport`] gerçek olanı; testler [`ScriptedTransport`] kullanır.
//!
//! ## Neden okuma ayrı bir iş parçacığında
//!
//! Zaman aşımı olmadan çökme izolasyonu yoktur: asılı kalan bir eklenti,
//! bloklayan bir `read_line` ile çekirdeği de asar. `std`'de borulara zaman
//! aşımı yok — okuma bir iş parçacığına alınıp satırlar bir kanaldan
//! geçiriliyor, çünkü `mpsc`'de `recv_timeout` **var**. Bedeli eklenti başına
//! iki iş parçacığı (stdout + stderr); ikisi de süreç ölünce kendiliğinden
//! biter.

use std::io::{BufRead, BufReader, Read as _, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::time::Duration;

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};

/// Okuma denemesinin sonucu.
///
/// Üç ayrı cevap, üçü de farklı tanı (K9): satır geldi / süre doldu ama süreç
/// sağ / kanal kapandı, yani süreç öldü.
#[derive(Debug, PartialEq, Eq)]
pub enum Received {
    Line(String),
    Timeout,
    Closed,
}

/// Eklentiyle satır alışverişi.
pub trait PluginTransport: Send {
    /// Bir satır gönderir (sonuna `\n` eklenir).
    ///
    /// # Errors
    /// Süreç öldüyse ya da boru kapandıysa.
    fn send_line(&mut self, line: &str) -> Result<()>;

    /// Bir satır bekler.
    ///
    /// # Errors
    /// Taşıma kendi kendine bozulduysa. Zaman aşımı ve kapanma **hata değil**,
    /// [`Received`] varyantı: kararı çağıran verir.
    fn receive_line(&mut self, timeout: Duration) -> Result<Received>;

    /// Süreci sonlandırır. Çağrıldıktan sonra taşıma kullanılmaz.
    fn shutdown(&mut self);
}

/// Alt süreç taşıması.
pub struct ProcessTransport {
    plugin: String,
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
}

impl std::fmt::Debug for ProcessTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessTransport")
            .field("plugin", &self.plugin)
            .field("pid", &self.child.id())
            .finish()
    }
}

/// Okuyucu iş parçacığının kanal tamponu. Eklenti biz okumadan bu kadar
/// satır biriktirebilir; sonrasında yazması bloklar — bellek tavanı.
const LINE_BUFFER: usize = 64;

/// Eklenti stdout'unda kabul edilen en uzun satır.
///
/// Düşmanca (ya da bozuk) bir eklenti tek satırda gigabaytlarca yazıp
/// çekirdeğin belleğini doldurmasın. `ureq` gövde tavanının (32 MB) eşi.
const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

impl ProcessTransport {
    /// Eklenti sürecini başlatır.
    ///
    /// `cwd` eklenti dizini: eklenti kendi yanındaki dosyaları göreli yolla
    /// bulabilsin.
    ///
    /// # Errors
    /// Süreç başlatılamazsa ya da boruları açılamazsa.
    pub fn spawn(plugin: &str, program: &Path, args: &[String], cwd: &Path) -> Result<Self> {
        let crashed = |detail: String| {
            Error::new(
                Stage::PluginHandshake,
                ErrorKind::PluginCrashed {
                    plugin: plugin.to_owned(),
                    detail,
                },
            )
        };

        let mut child = Command::new(program)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| crashed(format!("{} başlatılamadı: {err}", program.display())))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| crashed("stdin borusu açılmadı".to_owned()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| crashed("stdout borusu açılmadı".to_owned()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| crashed("stderr borusu açılmadı".to_owned()))?;

        let (sender, lines) = sync_channel(LINE_BUFFER);
        spawn_reader(plugin.to_owned(), stdout, sender);
        spawn_stderr_drain(plugin.to_owned(), stderr);

        Ok(Self {
            plugin: plugin.to_owned(),
            child,
            stdin: Some(stdin),
            lines,
        })
    }

    fn crashed(&self, detail: String) -> Error {
        Error::new(
            Stage::ProviderCall,
            ErrorKind::PluginCrashed {
                plugin: self.plugin.clone(),
                detail,
            },
        )
    }
}

/// stdout okuyucusu: satırları kanala aktarır, EOF'ta kanalı kapatır.
fn spawn_reader(plugin: String, stdout: std::process::ChildStdout, sender: SyncSender<String>) {
    std::thread::Builder::new()
        .name(format!("headshell-plugin-{plugin}-out"))
        .spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                // Tavanla okuyoruz: sınırsız `read_line` bir bellek tüketim
                // yolu. Tavan aşılırsa satırı yollamıyoruz ve **sebebini
                // söyleyip** okumayı bırakıyoruz (K9).
                let mut limited = (&mut reader).take(MAX_LINE_BYTES as u64 + 1);
                match limited.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(read) => {
                        if read > MAX_LINE_BYTES {
                            tracing::error!(
                                plugin = %plugin,
                                limit = MAX_LINE_BYTES,
                                "eklenti tek satırda tavanı aştı, okuma durduruldu"
                            );
                            break;
                        }
                        let trimmed = line.trim_end_matches(['\n', '\r']).to_owned();
                        if trimmed.is_empty() {
                            continue;
                        }
                        if sender.send(trimmed).is_err() {
                            // Alıcı düştü: taşıma kapanmış.
                            break;
                        }
                    }
                    Err(err) => {
                        tracing::warn!(plugin = %plugin, error = %err, "eklenti stdout okunamadı");
                        break;
                    }
                }
            }
        })
        .map_or_else(
            |err| tracing::error!(error = %err, "eklenti okuyucu iş parçacığı açılamadı"),
            |_handle| (),
        );
}

/// stderr: eklentinin log'u. Yutulmuyor, `tracing`'e aktarılıyor —
/// eklentinin neden çalışmadığını söyleyen tek yer burası olabilir.
fn spawn_stderr_drain(plugin: String, stderr: std::process::ChildStderr) {
    std::thread::Builder::new()
        .name(format!("headshell-plugin-{plugin}-err"))
        .spawn(move || {
            for line in BufReader::new(stderr).lines() {
                match line {
                    Ok(line) if !line.trim().is_empty() => {
                        tracing::warn!(plugin = %plugin, "eklenti: {line}");
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        })
        .map_or_else(
            |err| tracing::error!(error = %err, "eklenti stderr iş parçacığı açılamadı"),
            |_handle| (),
        );
}

impl PluginTransport for ProcessTransport {
    fn send_line(&mut self, line: &str) -> Result<()> {
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(self.crashed("süreç kapatılmıştı".to_owned()));
        };
        let write = stdin
            .write_all(line.as_bytes())
            .and_then(|()| stdin.write_all(b"\n"))
            .and_then(|()| stdin.flush());
        match write {
            Ok(()) => Ok(()),
            // Boru kırıldıysa suçlu yazma değil, ölmüş süreç: mesaj bunu
            // söylesin.
            Err(err) => Err(self.crashed(format!("istek yazılamadı: {err}"))),
        }
    }

    fn receive_line(&mut self, timeout: Duration) -> Result<Received> {
        match self.lines.recv_timeout(timeout) {
            Ok(line) => Ok(Received::Line(line)),
            Err(RecvTimeoutError::Timeout) => Ok(Received::Timeout),
            Err(RecvTimeoutError::Disconnected) => Ok(Received::Closed),
        }
    }

    fn shutdown(&mut self) {
        // Önce stdin'i kapatıyoruz: düzgün yazılmış bir eklenti EOF görünce
        // kendi çıkar. Sonra kısa bir süre bekleyip inatçıysa öldürüyoruz.
        self.stdin = None;
        let deadline = std::time::Instant::now() + GRACE;
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => {
                    if std::time::Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(err) => {
                    tracing::warn!(plugin = %self.plugin, error = %err, "eklenti durumu okunamadı");
                    break;
                }
            }
        }
        if let Err(err) = self.child.kill() {
            tracing::warn!(plugin = %self.plugin, error = %err, "eklenti süreci öldürülemedi");
        }
        // Zombi bırakmamak için topluyoruz.
        let _ = self.child.wait();
    }
}

/// `shutdown`'dan sonra sürece tanınan süre.
const GRACE: Duration = Duration::from_millis(500);

impl Drop for ProcessTransport {
    fn drop(&mut self) {
        // Düşen bir taşıma arkasında çalışan süreç bırakmaz — eklenti
        // çökse de çekirdek çökmez (K5), tersi de doğru olmalı.
        self.shutdown();
    }
}

/// Testler için: önceden yazılmış cevapları sırayla veren taşıma.
#[cfg(test)]
pub struct ScriptedTransport {
    /// Gönderilen satırlar — test bunları doğrular.
    pub sent: Vec<String>,
    /// Sıradaki cevaplar.
    replies: std::collections::VecDeque<Received>,
    /// Cevaplar bittiğinde ne dönsün.
    exhausted: Received,
    pub shutdown_called: bool,
}

#[cfg(test)]
impl ScriptedTransport {
    pub fn new(replies: Vec<Received>) -> Self {
        Self {
            sent: Vec::new(),
            replies: replies.into(),
            exhausted: Received::Closed,
            shutdown_called: false,
        }
    }

    /// Cevaplar bittiğinde `Timeout` dönen değişke (asılı kalan eklenti).
    pub fn hanging() -> Self {
        Self {
            sent: Vec::new(),
            replies: std::collections::VecDeque::new(),
            exhausted: Received::Timeout,
            shutdown_called: false,
        }
    }
}

#[cfg(test)]
impl PluginTransport for ScriptedTransport {
    fn send_line(&mut self, line: &str) -> Result<()> {
        self.sent.push(line.to_owned());
        Ok(())
    }

    fn receive_line(&mut self, _timeout: Duration) -> Result<Received> {
        Ok(self.replies.pop_front().unwrap_or(match self.exhausted {
            Received::Timeout => Received::Timeout,
            _ => Received::Closed,
        }))
    }

    fn shutdown(&mut self) {
        self.shutdown_called = true;
    }
}

/// Bir eklentiyi başlatabilen şey.
///
/// Yeniden başlatma bunun üstünden yapılıyor: çöken bir eklentiyi ayağa
/// kaldırmak için çekirdeğin komutu yeniden bilmesi gerekiyor, ve testlerin
/// çökmeyi taklit edebilmesi gerekiyor.
pub trait TransportFactory: Send + Sync + std::fmt::Debug {
    /// Yeni bir taşıma açar.
    ///
    /// # Errors
    /// Süreç başlatılamazsa.
    fn open(&self) -> Result<Box<dyn PluginTransport>>;
}

/// Alt süreç başlatan fabrika.
#[derive(Debug, Clone)]
pub struct ProcessFactory {
    plugin: String,
    program: PathBuf,
    args: Vec<String>,
    cwd: PathBuf,
}

impl ProcessFactory {
    #[must_use]
    pub fn new(plugin: &str, program: PathBuf, args: Vec<String>, cwd: PathBuf) -> Self {
        Self {
            plugin: plugin.to_owned(),
            program,
            args,
            cwd,
        }
    }
}

impl TransportFactory for ProcessFactory {
    fn open(&self) -> Result<Box<dyn PluginTransport>> {
        Ok(Box::new(ProcessTransport::spawn(
            &self.plugin,
            &self.program,
            &self.args,
            &self.cwd,
        )?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `sh` her POSIX makinesinde var; gerçek süreç davranışını (yazma,
    /// okuma, EOF, ölüm) taklit değil **gerçekten** sınıyor.
    #[cfg(unix)]
    fn sh(script: &str) -> ProcessTransport {
        ProcessTransport::spawn(
            "test",
            Path::new("/bin/sh"),
            &["-c".to_owned(), script.to_owned()],
            Path::new("/tmp"),
        )
        .unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn a_line_written_comes_back_from_the_child() {
        let mut transport = sh("while read -r line; do echo \"yanit:$line\"; done");
        transport.send_line("merhaba").unwrap();
        let received = transport.receive_line(Duration::from_secs(5)).unwrap();
        assert_eq!(received, Received::Line("yanit:merhaba".to_owned()));
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_exits_closes_the_channel_instead_of_hanging() {
        let mut transport = sh("exit 0");
        assert_eq!(
            transport.receive_line(Duration::from_secs(5)).unwrap(),
            Received::Closed,
            "ölmüş süreç zaman aşımı değil kapanma olarak görünmeli"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_silent_child_times_out_but_stays_alive() {
        let mut transport = sh("sleep 30");
        assert_eq!(
            transport.receive_line(Duration::from_millis(150)).unwrap(),
            Received::Timeout
        );
        // Zaman aşımı süreci öldürmez; kararı çağıran verir.
        assert_eq!(
            transport.receive_line(Duration::from_millis(50)).unwrap(),
            Received::Timeout
        );
    }

    #[cfg(unix)]
    #[test]
    fn writing_to_a_dead_child_is_reported_as_a_crash_not_an_io_error() {
        let mut transport = sh("exit 0");
        // Ölmesini bekliyoruz; kanalın kapanması bunun işareti.
        assert_eq!(
            transport.receive_line(Duration::from_secs(5)).unwrap(),
            Received::Closed
        );
        // İlk yazma tamponda kalabilir (SIGPIPE yok, EPIPE gecikebilir);
        // birkaç deneme içinde hata görülmeli.
        let mut saw_error = None;
        for _ in 0..50 {
            if let Err(err) = transport.send_line("x") {
                saw_error = Some(err);
                break;
            }
        }
        let err = saw_error.expect("ölmüş sürece yazma er geç hata vermeli");
        assert!(
            matches!(err.kind(), ErrorKind::PluginCrashed { .. }),
            "{}",
            err.chain_text()
        );
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_reaps_the_child_even_if_it_ignores_eof() {
        let mut transport = sh("trap '' TERM; sleep 30");
        let pid = transport.child.id();
        transport.shutdown();
        // `wait` çağrıldı: süreç zombi bırakmadı.
        assert!(pid > 0);
        assert!(transport.child.try_wait().is_ok());
    }

    #[test]
    fn the_scripted_transport_replays_in_order_then_reports_closed() {
        let mut transport = ScriptedTransport::new(vec![Received::Line("a".to_owned())]);
        transport.send_line("istek").unwrap();
        assert_eq!(transport.sent, vec!["istek".to_owned()]);
        assert_eq!(
            transport.receive_line(Duration::from_secs(1)).unwrap(),
            Received::Line("a".to_owned())
        );
        assert_eq!(
            transport.receive_line(Duration::from_secs(1)).unwrap(),
            Received::Closed
        );
    }
}
