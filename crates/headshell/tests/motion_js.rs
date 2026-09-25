//! Hareket katmanının (`ui/motion.js`) saf hesabı (D-072).
//!
//! Webview'de tip denetimi yok ve bir yay formülündeki işaret hatası
//! derleyiciye görünmez: öğe hedefinden uzaklaşıp ekrandan çıkar, ya da
//! `NaN` bir `transform` yazar ve hiç görünmez. Bu dosya formülleri,
//! `anchor_parity_js.rs` ile aynı yoldan — gömülü QuickJS'te, dışarıdan hiçbir
//! şey istemeden (D-070) — sınıyor.
//!
//! Sınanan şey DOM'a dokunmayan kısım: yay çözümü, momentum izdüşümü, lastik
//! bant, hız ölçümü ve tema süresinin okunması. `motion.js`'in üst düzeyi bu
//! yüzden saf tutuluyor; `document` yalnızca çağrılan fonksiyonların içinde.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use rquickjs::function::This;
use rquickjs::{CaughtError, Context, Ctx, Function, Module, Object, Runtime, Value};

fn source() -> String {
    std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("ui")
            .join("motion.js"),
    )
    .expect("ADIM: MOTION_JS — ui/motion.js okunamadı")
}

/// Modülü değerlendirir ve `check`'e dışa aktarımlarını verir.
fn with_module<R>(check: impl for<'js> FnOnce(&Ctx<'js>, Object<'js>) -> R) -> R {
    let runtime = Runtime::new().unwrap();
    let context = Context::full(&runtime).unwrap();
    context.with(|ctx| {
        let evaluated = Module::declare(ctx.clone(), "motion.js", source())
            .and_then(|module| module.eval())
            .and_then(|(module, promise)| promise.finish::<()>().map(|()| module));
        let module = match evaluated {
            Ok(module) => module,
            Err(err) => panic!(
                "ADIM: MOTION_JS — motion.js değerlendirilemedi (üst düzeyde DOM'a mı dokunuyor?): {}",
                CaughtError::from_error(&ctx, err)
            ),
        };
        let exports = Object::new(ctx.clone()).unwrap();
        for name in [
            "springStep",
            "project",
            "rubberband",
            "parseDuration",
            "motionSettings",
            "createVelocityTracker",
        ] {
            let function: Function = module
                .get(name)
                .unwrap_or_else(|_| panic!("motion.js `{name}` dışa aktarmıyor"));
            exports.set(name, function).unwrap();
        }
        check(&ctx, exports)
    })
}

fn call2(exports: &Object<'_>, name: &str, args: (f64, f64)) -> f64 {
    let function: Function = exports.get(name).unwrap();
    function.call(args).unwrap()
}

/// Yayı `seconds` boyunca 60 Hz karelerle ilerletir; `[konum farkı, hız]`.
fn run_spring(
    exports: &Object<'_>,
    offset: f64,
    velocity: f64,
    damping: f64,
    seconds: f64,
) -> (f64, f64) {
    let step: Function = exports.get("springStep").unwrap();
    let (mut x, mut v) = (offset, velocity);
    let frames = (seconds * 60.0).round() as usize;
    for _ in 0..frames {
        let next: Vec<f64> = step.call((x, v, 1.0 / 60.0, damping, 0.36)).unwrap();
        assert!(
            next.iter().all(|n| n.is_finite()),
            "yay sonlu olmayan bir değer üretti (sönüm {damping}): {next:?}"
        );
        (x, v) = (next[0], next[1]);
    }
    (x, v)
}

#[test]
fn every_spring_settles_on_its_target_without_producing_nan() {
    with_module(|_, exports| {
        // Aşmayan (1), aşan (0.8) ve aşırı sönümlü (1.4) yay; biri hızla
        // fırlatılmış. Hepsi iki saniyede hedefin 0.1 birim yakınına oturmalı.
        for (damping, offset, velocity) in [
            (1.0, 240.0, 0.0),
            (0.8, 240.0, 0.0),
            (1.4, 240.0, 0.0),
            (0.8, 0.0, 3000.0),
        ] {
            let (x, v) = run_spring(&exports, offset, velocity, damping, 2.0);
            assert!(
                x.abs() < 0.1 && v.abs() < 1.0,
                "sönüm {damping}: iki saniyede oturmadı (fark {x}, hız {v})"
            );
        }
    });
}

#[test]
fn a_critically_damped_spring_never_overshoots() {
    // Varsayılan yay 1.0: menü, panel, seçim göstergesi hedefini geçmez.
    // Geçseydi sebepsiz bir zıplama olurdu — fırlatılmamış bir şey sekmez.
    with_module(|_, exports| {
        let step: Function = exports.get("springStep").unwrap();
        let (mut x, mut v) = (100.0_f64, 0.0_f64);
        for _ in 0..240 {
            let next: Vec<f64> = step.call((x, v, 1.0 / 120.0, 1.0, 0.36)).unwrap();
            (x, v) = (next[0], next[1]);
            assert!(x >= -1e-9, "1.0 sönümlü yay hedefi geçti: {x}");
        }
    });
}

#[test]
fn the_spring_starts_with_the_velocity_it_was_handed() {
    // Sürüklemeden yaya geçişte dikiş olmamalı: bırakılan hız yayın ilk
    // hızı. Kapalı biçim çözümün türevi burada kolayca işaret kaybeder.
    with_module(|_, exports| {
        let step: Function = exports.get("springStep").unwrap();
        for damping in [0.8_f64, 1.0, 1.4] {
            let next: Vec<f64> = step.call((50.0, 1200.0, 1e-6, damping, 0.36)).unwrap();
            assert!(
                (next[1] - 1200.0).abs() < 1.0,
                "sönüm {damping}: başlangıç hızı 1200 olmalıydı, {} bulundu",
                next[1]
            );
            assert!((next[0] - 50.0).abs() < 0.01, "konum sıçradı: {}", next[0]);
        }
    });
}

#[test]
fn a_big_frame_does_not_throw_the_spring_off() {
    // Kapalı biçimin kazancı: tek bir 0.5 sn'lik adım, 30 küçük adımla aynı
    // yere varır. Sayısal entegrasyon büyük adımda patlardı.
    with_module(|_, exports| {
        let step: Function = exports.get("springStep").unwrap();
        let once: Vec<f64> = step.call((200.0, 0.0, 0.5, 0.8, 0.36)).unwrap();
        let (x, _) = run_spring(&exports, 200.0, 0.0, 0.8, 0.5);
        assert!(
            (once[0] - x).abs() < 1e-6,
            "tek adım {} ≠ küçük adımlar {x}",
            once[0]
        );
    });
}

#[test]
fn projection_is_apples_exponential_decay() {
    with_module(|_, exports| {
        // 1000 px/sn, 0.998 → (1000/1000)·0.998/0.002 = 499 px.
        let projected = call2(&exports, "project", (1000.0, 0.998));
        assert!((projected - 499.0).abs() < 1e-6, "{projected}");
        // Yön korunur.
        assert!(call2(&exports, "project", (-1000.0, 0.998)) < 0.0);
    });
}

#[test]
fn the_rubber_band_resists_more_the_further_it_is_pulled() {
    with_module(|_, exports| {
        let band: Function = exports.get("rubberband").unwrap();
        let at = |overshoot: f64| -> f64 { band.call((overshoot, 300.0)).unwrap() };
        // Az çekilince kabaca 0.55 oranında izler…
        assert!((at(1.0) - 0.55).abs() < 0.01, "{}", at(1.0));
        // …çekildikçe oran düşer…
        assert!(at(400.0) / 400.0 < at(40.0) / 40.0);
        // …ve ne kadar çekilirse çekilsin boyutu geçmez.
        assert!(at(1.0e9) < 300.0);
        // İşaret korunur: sola çekilen sola gider.
        assert!(at(-50.0) < 0.0);
        // Boyutu olmayan bir öğe hiç izlemez (sıfıra bölme yok).
        let zero: f64 = band.call((50.0, 0.0)).unwrap();
        assert_eq!(zero, 0.0);
    });
}

#[test]
fn the_duration_token_is_read_the_way_css_reads_it() {
    with_module(|_, exports| {
        let parse: Function = exports.get("parseDuration").unwrap();
        let read = |text: &str| -> Option<f64> {
            let value: Value = parse.call((text,)).unwrap();
            if value.is_null() {
                None
            } else {
                Some(value.as_number().unwrap())
            }
        };
        assert_eq!(read("120ms"), Some(120.0));
        assert_eq!(
            read(" 120ms "),
            Some(120.0),
            "getPropertyValue boşluk bırakır"
        );
        assert_eq!(read("0.2s"), Some(200.0));
        assert_eq!(read("0ms"), Some(0.0));
        assert_eq!(read("0"), Some(0.0), "birimsiz sıfır CSS'te geçerli");
        assert_eq!(
            read("120"),
            None,
            "birimsiz sıfır dışı sayı CSS'te geçersiz"
        );
        assert_eq!(read("-5ms"), None);
        assert_eq!(read("hızlı"), None);
        assert_eq!(read(""), None);
    });
}

#[test]
fn zero_duration_stops_all_motion_and_reduced_motion_keeps_only_fades() {
    with_module(|ctx, exports| {
        let settings: Function = exports.get("motionSettings").unwrap();
        let read = |ms: Option<f64>, reduced: bool| -> (bool, bool, f64) {
            let duration: Value = match ms {
                Some(ms) => Value::new_number(ctx.clone(), ms),
                None => Value::new_null(ctx.clone()),
            };
            let value: Object = settings.call((duration, reduced)).unwrap();
            (
                value.get("enabled").unwrap(),
                value.get("spatial").unwrap(),
                value.get("response").unwrap(),
            )
        };
        // Yüksek Karşıtlık teması: `--headshell-duration: 0ms` → hiçbir şey.
        assert_eq!(read(Some(0.0), false), (false, false, 0.0));
        // Sistem "hareketi azalt" diyor: konum yok, opaklık var.
        let (enabled, spatial, _) = read(Some(120.0), true);
        assert!(enabled && !spatial);
        // Varsayılan: tepki süre × 3.
        let (enabled, spatial, response) = read(Some(120.0), false);
        assert!(enabled && spatial);
        assert!((response - 0.36).abs() < 1e-9, "{response}");
        // Okunamayan token varsayılana düşer, hareketi kapatmaz.
        let (enabled, _, response) = read(None, false);
        assert!(enabled);
        assert!((response - 0.36).abs() < 1e-9, "{response}");
    });
}

#[test]
fn velocity_comes_from_the_recent_samples_only() {
    with_module(|_, exports| {
        let create: Function = exports.get("createVelocityTracker").unwrap();
        let tracker: Object = create.call(()).unwrap();
        let add: Function = tracker.get("add").unwrap();
        let velocity: Function = tracker.get("velocity").unwrap();
        let sample = |time: f64, value: f64| {
            add.call::<_, ()>((This(tracker.clone()), time, value))
                .unwrap();
        };
        let speed = || -> f64 { velocity.call((This(tracker.clone()),)).unwrap() };

        // Tek örnekle hız yok.
        sample(0.0, 0.0);
        assert_eq!(speed(), 0.0);

        // 16 ms'de bir 8 px: 500 px/sn.
        for i in 1..=6 {
            sample(f64::from(i) * 16.0, f64::from(i) * 8.0);
        }
        let v = speed();
        assert!((v - 500.0).abs() < 1.0, "{v}");

        // Parmak durdu, 300 ms sonra bıraktı: eski hızlı örnekler pencereden
        // düştü, hız neredeyse sıfır.
        sample(396.0, 48.0);
        let v = speed();
        assert!(
            v.abs() < 50.0,
            "durmuş bir parmak hâlâ hızlı sayılıyor: {v}"
        );
    });
}
