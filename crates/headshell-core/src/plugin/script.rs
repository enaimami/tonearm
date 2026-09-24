//! Eklenti motoru: gömülü QuickJS (D-069).
//!
//! Her eklenti **kendi iş parçacığında, kendi QuickJS çalışma zamanında**
//! koşar. İş parçacığı ilk çağrıda açılır (tembel, api 1'deki süreç gibi),
//! betiği ES modülü olarak değerlendirir ve sonra iş bekler: çağıran bir
//! kanal üzerinden fonksiyon adı + argüman gönderir, cevabı zaman aşımıyla
//! bekler.
//!
//! ## Neden ayrı bir iş parçacığı
//!
//! 1. **Zaman aşımı.** JS döngüde takılırsa QuickJS'in kesme kancası onu
//!    süre dolunca durdurur — `try/catch` bile bunu yakalayamaz (ölçüldü).
//!    Ama JS bir motor çağrısında (HTTP, araç) beklerken kanca çalışamaz;
//!    o durumda çağıranı kurtaran şey cevabı kanaldan **süreyle**
//!    beklemesidir. Aynı iş parçacığında olsaydık bekleyen bir HTTP
//!    isteği çekirdeği de bekletirdi.
//! 2. **`Send`.** QuickJS çalışma zamanı iş parçacıkları arasında
//!    taşınamaz; `Provider` ise `Send + Sync` olmak zorunda. Çalışma
//!    zamanını kendi iş parçacığında doğurup orada öldürmek, `rquickjs`'in
//!    `parallel` feature'ına gerek bırakmıyor.
//!
//! ## Neyi tutar, neyi tutmaz
//!
//! Tutar: zaman (kesme + bekleme süresi), bellek (çalışma zamanı başına
//! tavan), yığın (derin özyineleme istisnaya döner), ağ ve dosya (eklenti
//! dış dünyaya yalnızca `host`'un kapılarından çıkar, bkz. [`super::host`]).
//! JS'in fırlattığı her şey — bellek taşması dahil — bir istisnadır ve
//! çekirdeği düşürmez.
//!
//! Tutmaz: QuickJS'in **kendi** C kodundaki bir çökme. api 1'de eklenti ayrı
//! süreçti ve süreç ölürse çekirdek yaşardı; api 2'de eklenti çekirdeğin
//! adres uzayında. Bu takas D-069'da bilerek yapıldı: karşılığında
//! eklentiler kurulum istemiyor, izinler zorlanıyor ve motor mobile
//! gidebiliyor (iOS alt süreç açtırmıyor).

use std::cell::Cell;
use std::rc::Rc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use rquickjs::function::Args;
use rquickjs::{CaughtError, Context, Ctx, Function, Module, Runtime, Value};

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result, io_err};
use crate::provider::Capabilities;

use super::ScriptSpec;
use super::host::{self, HostState};
use super::protocol::export;

/// Bir eklentinin çalışma zamanının bellek tavanı.
///
/// InnerTube'un arama cevabı ~1-2 MB JSON ve QuickJS'te ayrıştırılmış hâli
/// bunun birkaç katı. Tavan aşılırsa JS "out of memory" istisnası alır —
/// çekirdek değil, o çağrı düşer.
const MEMORY_LIMIT: usize = 128 * 1024 * 1024;

/// JS yığınının tavanı. İş parçacığı yığınından ([`THREAD_STACK`]) küçük
/// olmalı: QuickJS kendi sınırını denetler, işletim sisteminin sınırına
/// çarpmadan önce "Maximum call stack size exceeded" fırlatır.
const JS_STACK_LIMIT: usize = 1024 * 1024;

/// Eklenti iş parçacığının yığını.
const THREAD_STACK: usize = 8 * 1024 * 1024;

/// Süre dolduktan sonra cevabı beklemeye devam edilen pay.
///
/// Kesme kancası JS'i süre dolunca durdurur ama cevabın kanaldan gelmesi
/// birkaç milisaniye sürer; bu pay o yarışı kapatıyor. Pay da dolarsa
/// eklenti bir motor çağrısında takılı demektir ve bırakılır.
const GRACE: Duration = Duration::from_secs(2);

/// Kapatırken iş parçacığının bitmesini bekleme süresi — sır dosyalarını
/// silmesine vakit kalsın diye.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);

/// Motorun eklentiye her şeyden önce verdiği küçük katman: `console` ve
/// `host`'un dondurulması.
///
/// `console` ECMAScript'in değil tarayıcıların nesnesi; QuickJS'te yok. Ama
/// eklenti yazarının eli ona gidiyor — `host.log`'a bağlanıyor.
const PRELUDE: &str = r#"
"use strict";
(() => {
  const show = (value) => {
    if (typeof value === "string") return value;
    if (value === undefined) return "undefined";
    if (value instanceof Error) return value.stack ? `${value}\n${value.stack}` : String(value);
    try {
      const text = JSON.stringify(value);
      return text === undefined ? String(value) : text;
    } catch (_) {
      return String(value);
    }
  };
  const line = (values) => values.map(show).join(" ");
  globalThis.console = Object.freeze({
    log: (...values) => host.log.info(line(values)),
    info: (...values) => host.log.info(line(values)),
    warn: (...values) => host.log.warn(line(values)),
    error: (...values) => host.log.error(line(values)),
    debug: (...values) => host.log.debug(line(values)),
  });
  for (const key of Object.keys(host)) {
    const value = host[key];
    if (value !== null && typeof value === "object") Object.freeze(value);
  }
  Object.freeze(host);
})();
"#;

/// Bir çağrının sonucu, iş parçacığından çağırana.
#[derive(Debug)]
enum Outcome {
    Value(serde_json::Value),
    /// JS bir hata fırlattı.
    Threw {
        message: String,
        location: String,
    },
    /// Kesme kancası süreyi doldurdu.
    Interrupted,
    /// Eklenti sözleşmeye uymadı (fonksiyon yok, dönüş JSON'a çevrilemiyor,
    /// söz hiç çözülmüyor).
    Contract(String),
}

struct Job {
    function: &'static str,
    args: Vec<serde_json::Value>,
    timeout: Duration,
    reply: mpsc::SyncSender<Outcome>,
}

/// Bir eklentinin iş parçacığıyla konuşan uç.
pub(crate) struct ScriptWorker {
    plugin: String,
    jobs: Option<mpsc::Sender<Job>>,
    done: mpsc::Receiver<()>,
}

impl std::fmt::Debug for ScriptWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptWorker")
            .field("plugin", &self.plugin)
            .finish_non_exhaustive()
    }
}

impl ScriptWorker {
    /// İş parçacığını açar, betiği değerlendirir, dışa aktarımları denetler.
    ///
    /// # Errors
    /// Betik okunamazsa, değerlendirme hata fırlatırsa, süre dolarsa ya da
    /// beyan edilen bir yeteneğin fonksiyonu dışa aktarılmamışsa — hepsi
    /// [`Stage::PluginStart`]'ta.
    pub(crate) fn start(spec: ScriptSpec, timeout: Duration) -> Result<Self> {
        let plugin = spec.plugin.clone();
        let (job_tx, job_rx) = mpsc::channel::<Job>();
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<()>>(1);
        let (done_tx, done_rx) = mpsc::sync_channel::<()>(1);

        std::thread::Builder::new()
            .name(format!("eklenti:{plugin}"))
            .stack_size(THREAD_STACK)
            .spawn(move || {
                worker_main(spec, timeout, &job_rx, &ready_tx);
                let _ = done_tx.send(());
            })
            .map_err(|err| crashed(&plugin, format!("iş parçacığı açılamadı: {err}")))?;

        match ready_rx.recv_timeout(timeout + GRACE) {
            Ok(Ok(())) => Ok(Self {
                plugin,
                jobs: Some(job_tx),
                done: done_rx,
            }),
            Ok(Err(err)) => Err(err),
            // Kanal düşürülünce iş parçacığı yüklemeyi bitirdiği an çıkar.
            Err(RecvTimeoutError::Timeout) => Err(timed_out(&plugin, "yükleme", timeout)),
            Err(RecvTimeoutError::Disconnected) => Err(crashed(
                &plugin,
                "iş parçacığı yükleme sırasında düştü".to_owned(),
            )),
        }
    }

    /// Dışa aktarılmış bir fonksiyonu çağırır; dönüşü JSON olarak verir.
    ///
    /// # Errors
    /// Zaman aşımı ([`ErrorKind::PluginTimeout`]), JS hatası
    /// ([`ErrorKind::PluginThrew`]), sözleşme ihlali
    /// ([`ErrorKind::PluginContract`]) ya da düşmüş iş parçacığı
    /// ([`ErrorKind::PluginCrashed`]).
    pub(crate) fn call(
        &mut self,
        function: &'static str,
        args: Vec<serde_json::Value>,
        timeout: Duration,
    ) -> Result<serde_json::Value> {
        let Some(jobs) = &self.jobs else {
            return Err(crashed(&self.plugin, "motor kapatılmıştı".to_owned()));
        };
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        jobs.send(Job {
            function,
            args,
            timeout,
            reply: reply_tx,
        })
        .map_err(|_| crashed(&self.plugin, "iş parçacığı artık yok".to_owned()))?;

        match reply_rx.recv_timeout(timeout + GRACE) {
            Ok(Outcome::Value(value)) => Ok(value),
            Ok(Outcome::Threw { message, location }) => Err(Error::new(
                Stage::ProviderCall,
                ErrorKind::PluginThrew {
                    plugin: self.plugin.clone(),
                    method: function.to_owned(),
                    message,
                    location,
                },
            )),
            Ok(Outcome::Interrupted) | Err(RecvTimeoutError::Timeout) => {
                Err(timed_out(&self.plugin, function, timeout))
            }
            Ok(Outcome::Contract(detail)) => Err(contract(&self.plugin, function, detail)),
            Err(RecvTimeoutError::Disconnected) => Err(crashed(
                &self.plugin,
                format!("{function} çağrısı sırasında iş parçacığı düştü"),
            )),
        }
    }

    /// İş parçacığını durdurur ve kısa bir süre bitmesini bekler.
    ///
    /// Beklemenin sebebi temizlik: iş parçacığı çıkarken eklentiye verilen
    /// sır dosyalarını siliyor ([`super::host`]). Bir çağrıda takılı kalmış
    /// bir iş parçacığı beklenmez; o, takıldığı çağrı bitince kendi çıkar.
    pub(crate) fn shutdown(&mut self) {
        if self.jobs.take().is_some() {
            let _ = self.done.recv_timeout(SHUTDOWN_WAIT);
        }
    }
}

impl Drop for ScriptWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn crashed(plugin: &str, detail: String) -> Error {
    Error::new(
        Stage::PluginStart,
        ErrorKind::PluginCrashed {
            plugin: plugin.to_owned(),
            detail,
        },
    )
}

fn timed_out(plugin: &str, method: &str, timeout: Duration) -> Error {
    let stage = if method == "yükleme" {
        Stage::PluginStart
    } else {
        Stage::ProviderCall
    };
    Error::new(
        stage,
        ErrorKind::PluginTimeout {
            plugin: plugin.to_owned(),
            method: method.to_owned(),
            seconds: timeout.as_secs(),
        },
    )
}

fn contract(plugin: &str, method: &str, detail: String) -> Error {
    Error::new(
        Stage::ProviderCall,
        ErrorKind::PluginContract {
            plugin: plugin.to_owned(),
            method: method.to_owned(),
            detail,
        },
    )
}

/// İş parçacığının gövdesi: kur, yükle, hazır de, iş bekle.
fn worker_main(
    spec: ScriptSpec,
    load_timeout: Duration,
    jobs: &mpsc::Receiver<Job>,
    ready: &mpsc::SyncSender<Result<()>>,
) {
    let plugin = spec.plugin.clone();
    let source = match std::fs::read_to_string(&spec.main) {
        Ok(source) => source,
        Err(err) => {
            let _ = ready.send(Err(io_err(Stage::PluginStart, &spec.main, err)));
            return;
        }
    };
    let module_name = spec.module_name.clone();
    let capabilities = spec.capabilities;

    let runtime = match Runtime::new() {
        Ok(runtime) => runtime,
        Err(err) => {
            let _ = ready.send(Err(crashed(&plugin, format!("QuickJS kurulamadı: {err}"))));
            return;
        }
    };
    runtime.set_memory_limit(MEMORY_LIMIT);
    runtime.set_max_stack_size(JS_STACK_LIMIT);

    // Kesme kancası ve motor çağrıları aynı süreyi görür: kanca JS'i,
    // motor çağrıları kendilerini (HTTP'ye çıkmadan önce) durdurur.
    let deadline: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
    let fired = Rc::new(Cell::new(false));
    {
        let deadline = Rc::clone(&deadline);
        let fired = Rc::clone(&fired);
        runtime.set_interrupt_handler(Some(Box::new(move || {
            let hit = deadline.get().is_some_and(|at| Instant::now() >= at);
            if hit {
                fired.set(true);
            }
            hit
        })));
    }

    let context = match Context::full(&runtime) {
        Ok(context) => context,
        Err(err) => {
            let _ = ready.send(Err(crashed(
                &plugin,
                format!("QuickJS bağlamı kurulamadı: {err}"),
            )));
            return;
        }
    };
    let host = Rc::new(HostState::new(spec, Rc::clone(&deadline)));

    context.with(|ctx| {
        let module = match load(&ctx, &host, &source, &module_name, load_timeout, &fired) {
            Ok(module) => module,
            Err(err) => {
                let _ = ready.send(Err(err));
                return;
            }
        };
        if let Err(detail) = check_exports(&module, capabilities) {
            let _ = ready.send(Err(Error::new(
                Stage::PluginStart,
                ErrorKind::PluginContract {
                    plugin: plugin.clone(),
                    method: "yükleme".to_owned(),
                    detail,
                },
            )));
            return;
        }
        if ready.send(Ok(())).is_err() {
            // Çağıran beklemekten vazgeçti (yükleme süresi doldu).
            return;
        }

        // Kanal kapanınca (`shutdown` ya da sağlayıcı düştü) döngü biter.
        while let Ok(job) = jobs.recv() {
            fired.set(false);
            deadline.set(Some(Instant::now() + job.timeout));
            let outcome = invoke(&ctx, &module, job.function, job.args, &fired);
            deadline.set(None);
            // Çağıran beklemekten vazgeçtiyse cevabın gidecek yeri yok.
            let _ = job.reply.send(outcome);
        }
    });
    // `host` burada düşüyor: sır dosyaları siliniyor.
    drop(host);
}

/// Önce `host` + `console`, sonra betik. Değerlendirme süreyle sınırlı ve
/// sırasında ağa çıkılamaz.
fn load<'js>(
    ctx: &Ctx<'js>,
    host: &Rc<HostState>,
    source: &str,
    module_name: &str,
    timeout: Duration,
    fired: &Rc<Cell<bool>>,
) -> Result<Module<'js, rquickjs::module::Evaluated>> {
    let plugin = host.plugin().to_owned();
    let start_err = |detail: String| {
        Error::new(
            Stage::PluginStart,
            ErrorKind::PluginCrashed {
                plugin: plugin.clone(),
                detail,
            },
        )
    };

    host::install(ctx, Rc::clone(host))
        .map_err(|err| start_err(format!("motor API'si kurulamadı: {err}")))?;
    let prelude: std::result::Result<(), _> = ctx.eval(PRELUDE);
    if let Err(err) = prelude {
        return Err(start_err(format!(
            "başlangıç katmanı değerlendirilemedi: {}",
            caught_text(ctx, err)
        )));
    }

    host.set_loading(true);
    host.set_deadline(Some(Instant::now() + timeout));
    let evaluated = (|| {
        let declared = Module::declare(ctx.clone(), module_name, source)?;
        let (module, promise) = declared.eval()?;
        promise.finish::<()>()?;
        Ok(module)
    })();
    host.set_deadline(None);
    host.set_loading(false);

    evaluated.map_err(|err: rquickjs::Error| {
        if fired.get() {
            return timed_out(&plugin, "yükleme", timeout);
        }
        let (message, location) = describe_caught(CaughtError::from_error(ctx, err));
        Error::new(
            Stage::PluginStart,
            ErrorKind::PluginThrew {
                plugin: plugin.clone(),
                method: "yükleme".to_owned(),
                message,
                location,
            },
        )
    })
}

/// Beyan edilen her yeteneğin fonksiyonu dışa aktarılmış mı.
fn check_exports(
    module: &Module<'_, rquickjs::module::Evaluated>,
    capabilities: Capabilities,
) -> std::result::Result<(), String> {
    let mut wanted = vec![(export::HEALTH, "her eklenti")];
    if capabilities.contains(Capabilities::SEARCH) {
        wanted.push((export::SEARCH, "`search` yeteneği"));
    }
    if capabilities.contains(Capabilities::STREAM) {
        wanted.push((export::RESOLVE_SOURCE, "`stream` yeteneği"));
    }
    let missing: Vec<String> = wanted
        .into_iter()
        .filter(|(name, _)| {
            !module
                .get::<_, Value>(*name)
                .is_ok_and(|value| value.is_function())
        })
        .map(|(name, why)| format!("`{name}` ({why} için gerekli)"))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "betik şu fonksiyonları dışa aktarmıyor: {} — `export function …` ile verilmeli",
            missing.join(", ")
        ))
    }
}

/// Bir fonksiyonu çağırır ve sonucu JSON'a çevirir.
fn invoke<'js>(
    ctx: &Ctx<'js>,
    module: &Module<'js, rquickjs::module::Evaluated>,
    function: &'static str,
    args: Vec<serde_json::Value>,
    fired: &Rc<Cell<bool>>,
) -> Outcome {
    let Ok(callee) = module.get::<_, Function>(function) else {
        return Outcome::Contract(format!("`{function}` dışa aktarılmamış"));
    };

    let mut list = Args::new(ctx.clone(), args.len());
    for arg in args {
        let parsed = serde_json::to_string(&arg)
            .map_err(|err| err.to_string())
            .and_then(|text| ctx.json_parse(text).map_err(|err| err.to_string()));
        match parsed.map(|value| list.push_arg(value)) {
            Ok(Ok(())) => {}
            Ok(Err(err)) => return Outcome::Contract(format!("argüman verilemedi: {err}")),
            Err(err) => return Outcome::Contract(format!("argüman verilemedi: {err}")),
        }
    }

    let returned: rquickjs::Result<Value> = callee.call_arg(list);
    let value = match returned {
        Ok(value) => value,
        Err(err) => return failure(ctx, err, fired),
    };
    // `async function` bir söz döndürür; iş kuyruğu çözülene kadar yürütülür.
    let value = match value.as_promise() {
        Some(promise) => match promise.finish::<Value>() {
            Ok(value) => value,
            Err(rquickjs::Error::WouldBlock) => {
                return Outcome::Contract(format!(
                    "`{function}` bir söz (Promise) döndürdü ve söz hiç çözülmedi — motorda \
                     zamanlayıcı yok, beklenecek bir iş de kalmadı"
                ));
            }
            Err(err) => return failure(ctx, err, fired),
        },
        None => value,
    };

    match ctx.json_stringify(value) {
        Ok(Some(text)) => match text.to_string() {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(json) => Outcome::Value(json),
                Err(err) => Outcome::Contract(format!("dönüş JSON'a çevrilemedi: {err}")),
            },
            Err(err) => Outcome::Contract(format!("dönüş okunamadı: {err}")),
        },
        // `undefined` ve fonksiyon JSON'da yok; "değer yok" sayılıyor.
        Ok(None) => Outcome::Value(serde_json::Value::Null),
        Err(err) => match failure(ctx, err, fired) {
            Outcome::Threw { message, .. } => {
                Outcome::Contract(format!("dönüş JSON'a çevrilemedi: {message}"))
            }
            other => other,
        },
    }
}

fn failure(ctx: &Ctx<'_>, err: rquickjs::Error, fired: &Rc<Cell<bool>>) -> Outcome {
    if fired.get() {
        return Outcome::Interrupted;
    }
    let (message, location) = describe_caught(CaughtError::from_error(ctx, err));
    Outcome::Threw { message, location }
}

/// Yakalanan hatayı `(mesaj, konum)` ikilisine çevirir.
///
/// Konum yığının **ilk** satırı: `main.js:42:7`. Tamamı değil — tanı
/// raporu tek satırlık olmalı, ve eklenti yazarının ihtiyacı olan şey
/// hatanın çıktığı yer.
fn describe_caught(caught: CaughtError<'_>) -> (String, String) {
    match caught {
        CaughtError::Exception(exception) => {
            let message = exception
                .message()
                .filter(|message| !message.is_empty())
                .unwrap_or_else(|| "(mesajsız hata)".to_owned());
            let location = exception
                .stack()
                .as_deref()
                .and_then(first_frame)
                .map(|frame| format!(" ({frame})"))
                .unwrap_or_default();
            (message, location)
        }
        CaughtError::Value(value) => {
            let text = value
                .as_string()
                .and_then(|text| text.to_string().ok())
                .unwrap_or_else(|| format!("{value:?}"));
            (
                format!("hata nesnesi olmayan bir değer fırlattı: {text}"),
                String::new(),
            )
        }
        CaughtError::Error(err) => (format!("motor hatası: {err}"), String::new()),
    }
}

/// `"    at search (main.js:42:7)\n..."` → `main.js:42:7`.
fn first_frame(stack: &str) -> Option<String> {
    let line = stack.lines().map(str::trim).find(|line| !line.is_empty())?;
    let inner = line
        .rsplit_once('(')
        .and_then(|(_, rest)| rest.strip_suffix(')'))
        .unwrap_or_else(|| line.trim_start_matches("at ").trim());
    (!inner.is_empty()).then(|| inner.to_owned())
}

fn caught_text(ctx: &Ctx<'_>, err: rquickjs::Error) -> String {
    let (message, location) = describe_caught(CaughtError::from_error(ctx, err));
    format!("{message}{location}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_stack_frame_is_the_location() {
        assert_eq!(
            first_frame("    at search (main.js:42:7)\n    at <eval> (main.js:1:1)\n").as_deref(),
            Some("main.js:42:7")
        );
        assert_eq!(
            first_frame("    at main.js:3:1\n").as_deref(),
            Some("main.js:3:1")
        );
        assert_eq!(first_frame(""), None);
    }
}
