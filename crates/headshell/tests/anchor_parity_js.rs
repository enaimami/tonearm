//! JS'teki çapa formülünü doğruluk kümesine bağlar (D-033, D-070).
//!
//! Rust tarafını `headshell-core/tests/anchor_parity.rs` zaten bağlıyor. Kayma
//! **iki taraf da aynı dosyayı okuduğunda** yakalanır; tek taraflı bir test
//! yalnızca kendi kopyasının kendisiyle tutarlı olduğunu söyler.
//!
//! ## `node` yerine gömülü QuickJS
//!
//! Bu test bir zamanlar `node` çalıştırıyordu ve `node` kurulu olmayan bir
//! makinede — bilerek — düşüyordu: atlanabilir olsaydı formüller sessizce
//! kayardı (D-032). Kural doğruydu ama bedeli testi koşturan makineye bir
//! çalışma zamanı kurdurmaktı. D-069'dan beri çekirdeğin içinde bir JS motoru
//! var; `ui/anchor.js` artık onunla değerlendiriliyor. Test hâlâ **hiçbir
//! koşulda atlanmıyor** ve dışarıda hiçbir şey istemiyor (D-070).
//!
//! Motor farkı bir kusur kaynağı değil: formül aritmetik ve `Date.parse`'tan
//! ibaret, ikisi de ECMAScript'te tanımlı. Webview'in motoru (WebKit ya da
//! WebView2) ile QuickJS aynı IEEE-754 sayılarını üretir.

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
    .expect("ADIM: ANCHOR_PARITY — ui/anchor.js okunamadı");
    let truth: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            repo_root()
                .join("fixtures")
                .join("anchor")
                .join("position_cases.json"),
        )
        .expect("ADIM: ANCHOR_PARITY — doğruluk kümesi okunamadı"),
    )
    .expect("ADIM: ANCHOR_PARITY — doğruluk kümesi JSON değil");
    let cases = truth["cases"]
        .as_array()
        .filter(|cases| !cases.is_empty())
        .expect("ADIM: ANCHOR_PARITY — doğruluk kümesi boş");

    let runtime = Runtime::new().unwrap();
    let context = Context::full(&runtime).unwrap();
    let failures: Vec<String> = context.with(|ctx| {
        let evaluated = Module::declare(ctx.clone(), "anchor.js", source)
            .and_then(|module| module.eval())
            .and_then(|(module, promise)| promise.finish::<()>().map(|()| module));
        let module = match evaluated {
            Ok(module) => module,
            Err(err) => panic!(
                "ADIM: ANCHOR_PARITY — anchor.js değerlendirilemedi: {}",
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
            // Kesin eşitlik, tolerans yok: kopyalar arasındaki bir
            // milisaniyelik fark (`floor` yerine `round`) tam da bu testin
            // yakalamak için var olduğu kayma.
            if got.to_bits() != expected.to_bits() {
                failures.push(format!("  {name}\n    beklenen {expected}, bulunan {got}"));
            }
        }
        failures
    });

    assert!(
        failures.is_empty(),
        "ADIM: ANCHOR_PARITY — {}/{} vaka kaydı:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}
