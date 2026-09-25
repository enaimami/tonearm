//! K7 surface check: can the public types be expressed with `uniffi`?
//!
//! This test exists to catch a drift of the `PlayOptions<'a>` kind. That type
//! carried a lifetime on the public surface for months; nobody noticed,
//! because `cargo clippy` does not count it as a flaw — the rule was written
//! in PLAN.md, not in the code. Now it is in the code.
//!
//! **What is tested:** whether a public `struct` / `enum` / `type` took a
//! lifetime parameter. If it did, `uniffi` cannot express it as a record
//! (D-052). Also whether a public signature takes a closure parameter — K7
//! forbids that too.
//!
//! **What is not tested:** ergonomic constructors in the style of `pub fn
//! new(x: impl Into<String>)`. D-052 left those outside the rule: `uniffi`
//! only looks at the marked item, and in Phase 6 a `#[uniffi::constructor]`
//! is added next to them.
//!
//! **This is not real `uniffi` scaffolding generation.** The real check needs
//! the core's types marked with `#[derive(uniffi::Record)]`; that job belongs
//! to Phase 6, and D-052's "known gap" (four traits carrying boxed futures)
//! would already turn it red today. This test does not replace that check; it
//! is a cheap filter that comes before it.

// K8 exempts tests; here `expect` is not a flaw: if the file cannot be read,
// the check must not pass silently empty, it must fail loudly.
#![allow(clippy::expect_used)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The public types that **are allowed** to carry a lifetime.
///
/// All three are the same thing: a hand-written boxed future instead of
/// `async fn`. The trait has to be `dyn` compatible (K7 leaves
/// `Arc<dyn Trait>` free), and in Rust there is no other way to do that
/// without macros.
///
/// This list is **a debt record, not an exemption**: `uniffi` cannot express
/// `Pin<Box<dyn Future + Send + 'a>>` in the return value of a trait method.
/// In Phase 6 these four traits will be rewritten (D-052). Adding a new name
/// to the list means growing that debt — ask first.
const BOXED_FUTURE_ALIASES: &[&str] = &["ProviderFuture", "HttpFuture", "LookupFuture"];

#[test]
fn no_public_type_carries_a_lifetime() {
    let mut findings = String::new();

    for file in core_sources() {
        let source = std::fs::read_to_string(&file).expect("the source file must be readable");
        let body = without_test_modules(&source);

        for (line_no, line) in body.lines().enumerate() {
            let Some((kind, name, generics)) = public_type_header(line) else {
                continue;
            };
            if !generics.contains('\'') {
                continue;
            }
            if BOXED_FUTURE_ALIASES.contains(&name) {
                continue;
            }
            let _ = writeln!(
                findings,
                "  {}:{} — pub {kind} {name}{generics}",
                display_path(&file),
                line_no + 1,
            );
        }
    }

    assert!(
        findings.is_empty(),
        "STEP: K7_SURFACE\n\
         A public type has a lifetime; `uniffi` cannot express it as a record:\n\
         {findings}\n\
         Fix: turn the borrowed field into an owned type (`&'a str` → `String`).\n\
         Reasoning: PLAN.md §2 K7 and D-052. If this really cannot be avoided,\n\
         ask before extending the list — `BOXED_FUTURE_ALIASES` is a debt record."
    );
}

#[test]
fn no_public_signature_takes_a_closure() {
    let mut findings = String::new();

    for file in core_sources() {
        let source = std::fs::read_to_string(&file).expect("the source file must be readable");
        let body = without_test_modules(&source);

        for (line_no, signature) in public_signatures(&body) {
            if signature.contains("impl Fn")
                || signature.contains("F: Fn")
                || signature.contains("dyn Fn")
            {
                let _ = writeln!(
                    findings,
                    "  {}:{} — {}",
                    display_path(&file),
                    line_no,
                    signature.trim(),
                );
            }
        }
    }

    assert!(
        findings.is_empty(),
        "STEP: K7_SURFACE\n\
         A public signature takes a closure parameter; K7 forbids this\n\
         because `uniffi` cannot pass closures to the bindings:\n\
         {findings}\n\
         Fix: take an `Arc<dyn Trait>` instead of a closure — K7 leaves that free\n\
         and `uniffi` models it as a callback interface."
    );
}

/// All the `.rs` files under `headshell-core/src`.
fn core_sources() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect(&root, &mut files);
    assert!(!files.is_empty(), "core sources not found: {root:?}");
    files.sort();
    files
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).expect("the source directory must be readable");
    for entry in entries {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// The source with its test modules **removed**.
///
/// Tests are not subject to K7: borrowing a `&str` there is fine. But only
/// the test block drops, not the rest of the file — cutting off everything
/// after the first `#[cfg(test)]` left every public type defined *after* the
/// test module outside the check. This flaw came out while testing the check
/// itself: an injected violation was not caught because it was at the end of
/// the file.
///
/// We skip by counting braces; the skipped lines are blanked, not deleted, so
/// line numbers are kept.
fn without_test_modules(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut depth = 0usize;
    let mut in_test_module = false;
    let mut armed = false;

    for line in source.lines() {
        if in_test_module {
            depth += line.matches('{').count();
            depth = depth.saturating_sub(line.matches('}').count());
            if depth == 0 {
                in_test_module = false;
            }
            out.push('\n');
            continue;
        }

        if line.trim_start().starts_with("#[cfg(test)]") {
            armed = true;
            out.push('\n');
            continue;
        }

        // If `#[cfg(test)]` marks a `mod`, skip the block; if it marks a
        // `use` or a single item, only that line drops.
        if armed {
            armed = false;
            if line.contains("mod ") {
                let opens = line.matches('{').count();
                let closes = line.matches('}').count();
                if opens > closes {
                    in_test_module = true;
                    depth = opens - closes;
                }
                out.push('\n');
                continue;
            }
        }

        out.push_str(line);
        out.push('\n');
    }

    out
}

/// Splits a `pub struct/enum/type Name<...>` line.
///
/// Returns (kind, name, generic list). `None` if there are no generics.
fn public_type_header(line: &str) -> Option<(&'static str, &str, &str)> {
    let kind = ["struct", "enum", "type"]
        .into_iter()
        .find(|kind| line.starts_with(&format!("pub {kind} ")))?;

    let rest = line["pub ".len() + kind.len() + 1..].trim_start();
    let open = rest.find('<')?;
    let name = &rest[..open];
    // If there is a space between the name and `<`, this is not a type header.
    if name.is_empty() || name.contains(' ') {
        return None;
    }
    let close = rest.rfind('>')?;
    if close <= open {
        return None;
    }
    Some((kind, name, &rest[open..=close]))
}

/// Returns the public function signatures (multi-line ones joined).
fn public_signatures(body: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = body.lines().collect();
    let mut out = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if !(trimmed.starts_with("pub fn ") || trimmed.starts_with("pub async fn ")) {
            continue;
        }

        // Collect until the signature closes with `)`; do not enter the body.
        let mut signature = String::new();
        for line in lines.iter().skip(index).take(20) {
            signature.push_str(line.trim());
            signature.push(' ');
            if line.contains(')') {
                break;
            }
        }
        out.push((index + 1, signature));
    }

    out
}

/// Show the path relative to the repository root in the error message — an
/// absolute path is noise.
fn display_path(path: &Path) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    path.strip_prefix(root).map_or_else(
        |_| path.display().to_string(),
        |rel| rel.display().to_string(),
    )
}
