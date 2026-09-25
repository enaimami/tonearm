//! Binds the anchor formula in JS to the accuracy set (D-033, D-070).
//!
//! `headshell-core/tests/anchor_parity.rs` already binds the Rust side. Drift
//! is caught **when both sides read the same file**; a one-sided test only
//! says its own copy agrees with itself.
//!
//! ## Embedded QuickJS instead of `node`
//!
//! This test once ran `node`, and on a machine without `node` it failed — on
//! purpose: if it could be skipped the formulas would drift silently (D-032).
//! The rule was right, but its price was making the machine that runs the
//! test install a runtime. Since D-069 there is a JS engine inside the core;
//! `ui/anchor.js` is now evaluated with it. The test is still **never
//! skipped** under any condition, and it needs nothing from outside (D-070).
//!
//! The engine difference is not a source of bugs: the formula is only
//! arithmetic and `Date.parse`, both defined in ECMAScript. The webview's
//! engine (WebKit or WebView2) and QuickJS produce the same IEEE-754 numbers.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use rquickjs::{CaughtError, Context, Function, Module, Object, Runtime, Value};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

#[test]
fn the_javascript_copy_agrees_with_the_shared_truth_set() {
    let source = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("ui")
            .join("anchor.js"),
    )
    .expect("STEP: ANCHOR_PARITY — could not read ui/anchor.js");
    let truth: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            repo_root()
                .join("fixtures")
                .join("anchor")
                .join("position_cases.json"),
        )
        .expect("STEP: ANCHOR_PARITY — could not read the accuracy set"),
    )
    .expect("STEP: ANCHOR_PARITY — the accuracy set is not JSON");
    let cases = truth["cases"]
        .as_array()
        .filter(|cases| !cases.is_empty())
        .expect("STEP: ANCHOR_PARITY — the accuracy set is empty");

    let runtime = Runtime::new().unwrap();
    let context = Context::full(&runtime).unwrap();
    let failures: Vec<String> = context.with(|ctx| {
        let evaluated = Module::declare(ctx.clone(), "anchor.js", source)
            .and_then(|module| module.eval())
            .and_then(|(module, promise)| promise.finish::<()>().map(|()| module));
        let module = match evaluated {
            Ok(module) => module,
            Err(err) => panic!(
                "STEP: ANCHOR_PARITY — could not evaluate anchor.js: {}",
                CaughtError::from_error(&ctx, err)
            ),
        };
        let position_at: Function = module.get("positionAt").unwrap();
        let date: Object = ctx.globals().get("Date").unwrap();
        let parse: Function = date.get("parse").unwrap();

        let mut failures = Vec::new();
        for case in cases {
            let name = case["name"].as_str().unwrap_or("?");
            let anchor: Value = ctx.json_parse(case["anchor"].to_string()).unwrap();
            let now: f64 = parse.call((case["now"].as_str().unwrap(),)).unwrap();
            let got: f64 = position_at.call((anchor, now)).unwrap();
            let expected = case["expected_ms"].as_f64().unwrap();
            // Exact equality, no tolerance: a one-millisecond difference between
            // the copies (`round` instead of `floor`) is exactly the drift this
            // test exists to catch.
            if got.to_bits() != expected.to_bits() {
                failures.push(format!("  {name}\n    expected {expected}, got {got}"));
            }
        }
        failures
    });

    assert!(
        failures.is_empty(),
        "STEP: ANCHOR_PARITY — {}/{} cases drifted:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}
