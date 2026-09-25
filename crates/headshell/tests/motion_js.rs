//! The pure calculation of the motion layer (`ui/motion.js`) (D-072).
//!
//! The webview has no type checking, and a sign error in a spring formula is
//! invisible to the compiler: the element moves away from its target and
//! leaves the screen, or `NaN` writes a `transform` and it never shows up.
//! This file tests the formulas the same way as `anchor_parity_js.rs` — in
//! embedded QuickJS, needing nothing from outside (D-070).
//!
//! What is tested is the part that does not touch the DOM: the spring
//! solution, momentum projection, the rubber band, velocity measurement and
//! reading the theme's duration. That is why the top level of `motion.js` is
//! kept pure; `document` only appears inside the functions that are called.

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
    .expect("STEP: MOTION_JS — could not read ui/motion.js")
}

/// Evaluates the module and hands its exports to `check`.
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
                "STEP: MOTION_JS — could not evaluate motion.js (does it touch the DOM at the top level?): {}",
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
                .unwrap_or_else(|_| panic!("motion.js does not export `{name}`"));
            exports.set(name, function).unwrap();
        }
        check(&ctx, exports)
    })
}

fn call2(exports: &Object<'_>, name: &str, args: (f64, f64)) -> f64 {
    let function: Function = exports.get(name).unwrap();
    function.call(args).unwrap()
}

/// Advances the spring for `seconds` in 60 Hz frames; `[position offset,
/// velocity]`.
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
            "the spring produced a non-finite value (damping {damping}): {next:?}"
        );
        (x, v) = (next[0], next[1]);
    }
    (x, v)
}

#[test]
fn every_spring_settles_on_its_target_without_producing_nan() {
    with_module(|_, exports| {
        // A non-overshooting (1), an overshooting (0.8) and an overdamped (1.4)
        // spring; one of them thrown with velocity. All must settle within 0.1
        // units of the target in two seconds.
        for (damping, offset, velocity) in [
            (1.0, 240.0, 0.0),
            (0.8, 240.0, 0.0),
            (1.4, 240.0, 0.0),
            (0.8, 0.0, 3000.0),
        ] {
            let (x, v) = run_spring(&exports, offset, velocity, damping, 2.0);
            assert!(
                x.abs() < 0.1 && v.abs() < 1.0,
                "damping {damping}: did not settle in two seconds (offset {x}, velocity {v})"
            );
        }
    });
}

#[test]
fn a_critically_damped_spring_never_overshoots() {
    // The default spring is 1.0: a menu, a panel, the selection indicator do not
    // overshoot their target. If they did it would be a pointless bounce —
    // something that was not thrown does not bounce.
    with_module(|_, exports| {
        let step: Function = exports.get("springStep").unwrap();
        let (mut x, mut v) = (100.0_f64, 0.0_f64);
        for _ in 0..240 {
            let next: Vec<f64> = step.call((x, v, 1.0 / 120.0, 1.0, 0.36)).unwrap();
            (x, v) = (next[0], next[1]);
            assert!(
                x >= -1e-9,
                "a spring with 1.0 damping overshot its target: {x}"
            );
        }
    });
}

#[test]
fn the_spring_starts_with_the_velocity_it_was_handed() {
    // There must be no seam going from a drag to the spring: the release
    // velocity is the spring's initial velocity. The derivative of the closed-
    // form solution easily loses its sign here.
    with_module(|_, exports| {
        let step: Function = exports.get("springStep").unwrap();
        for damping in [0.8_f64, 1.0, 1.4] {
            let next: Vec<f64> = step.call((50.0, 1200.0, 1e-6, damping, 0.36)).unwrap();
            assert!(
                (next[1] - 1200.0).abs() < 1.0,
                "damping {damping}: the initial velocity should have been 1200, found {}",
                next[1]
            );
            assert!(
                (next[0] - 50.0).abs() < 0.01,
                "the position jumped: {}",
                next[0]
            );
        }
    });
}

#[test]
fn a_big_frame_does_not_throw_the_spring_off() {
    // The gain of the closed form: a single 0.5 s step lands in the same place
    // as 30 small steps. Numerical integration would blow up on a big step.
    with_module(|_, exports| {
        let step: Function = exports.get("springStep").unwrap();
        let once: Vec<f64> = step.call((200.0, 0.0, 0.5, 0.8, 0.36)).unwrap();
        let (x, _) = run_spring(&exports, 200.0, 0.0, 0.8, 0.5);
        assert!(
            (once[0] - x).abs() < 1e-6,
            "a single step {} ≠ small steps {x}",
            once[0]
        );
    });
}

#[test]
fn projection_is_apples_exponential_decay() {
    with_module(|_, exports| {
        // 1000 px/s, 0.998 → (1000/1000)·0.998/0.002 = 499 px.
        let projected = call2(&exports, "project", (1000.0, 0.998));
        assert!((projected - 499.0).abs() < 1e-6, "{projected}");
        // The direction is kept.
        assert!(call2(&exports, "project", (-1000.0, 0.998)) < 0.0);
    });
}

#[test]
fn the_rubber_band_resists_more_the_further_it_is_pulled() {
    with_module(|_, exports| {
        let band: Function = exports.get("rubberband").unwrap();
        let at = |overshoot: f64| -> f64 { band.call((overshoot, 300.0)).unwrap() };
        // Pulled a little, it follows at roughly 0.55…
        assert!((at(1.0) - 0.55).abs() < 0.01, "{}", at(1.0));
        // …the more it is pulled, the lower the ratio…
        assert!(at(400.0) / 400.0 < at(40.0) / 40.0);
        // …and however far it is pulled, it does not exceed the size.
        assert!(at(1.0e9) < 300.0);
        // The sign is kept: pulled left, it goes left.
        assert!(at(-50.0) < 0.0);
        // An element without a size does not follow at all (no division by zero).
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
            "getPropertyValue leaves whitespace"
        );
        assert_eq!(read("0.2s"), Some(200.0));
        assert_eq!(read("0ms"), Some(0.0));
        assert_eq!(read("0"), Some(0.0), "a unitless zero is valid in CSS");
        assert_eq!(
            read("120"),
            None,
            "a unitless non-zero number is invalid in CSS"
        );
        assert_eq!(read("-5ms"), None);
        assert_eq!(read("fast"), None);
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
        // The High Contrast theme: `--headshell-duration: 0ms` → nothing.
        assert_eq!(read(Some(0.0), false), (false, false, 0.0));
        // The system says "reduce motion": no position, opacity yes.
        let (enabled, spatial, _) = read(Some(120.0), true);
        assert!(enabled && !spatial);
        // The default: response = duration × 3.
        let (enabled, spatial, response) = read(Some(120.0), false);
        assert!(enabled && spatial);
        assert!((response - 0.36).abs() < 1e-9, "{response}");
        // An unreadable token falls back to the default, it does not turn motion
        // off.
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

        // No velocity with a single sample.
        sample(0.0, 0.0);
        assert_eq!(speed(), 0.0);

        // 8 px every 16 ms: 500 px/s.
        for i in 1..=6 {
            sample(f64::from(i) * 16.0, f64::from(i) * 8.0);
        }
        let v = speed();
        assert!((v - 500.0).abs() < 1.0, "{v}");

        // The finger stopped and released 300 ms later: the old fast samples fell
        // out of the window, the velocity is almost zero.
        sample(396.0, 48.0);
        let v = speed();
        assert!(
            v.abs() < 50.0,
            "a finger that stopped still counts as fast: {v}"
        );
    });
}
