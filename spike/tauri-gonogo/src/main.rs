// PLAN §3.1 GO/NO-GO — a THROWAWAY measurement run.
//
// The four things measured:
//   1. scrolling smoothness in a virtualised list of 50,000 rows
//   2. the cost of a CSS animation running at the same time
//   3. IPC round-trip latency (§3.2's fear of "hundreds of messages a second")
//   4. the Rust -> JS event stream rate
//
// The interface writes the report to disk with `save_report` and the app closes
// itself; so the measurement needs no one watching the screen and can be repeated.

use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Serialize, Clone)]
struct Row {
    id: u64,
    title: String,
    artist: String,
    album: String,
    ms: u64,
}

/// The same shape as Phase 4's room primitive (D-015) — so it is a realistic load.
#[derive(Serialize, Clone)]
struct Anchor {
    track: u64,
    wall_ms: u64,
    position_ms: u64,
    rate: f32,
    state: &'static str,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The smallest load: the bare round-trip cost.
#[tauri::command]
fn ping(seq: u64) -> u64 {
    seq
}

/// A realistic small load: what the GUI will ask for the position.
#[tauri::command]
fn anchor(track: u64, position_ms: u64) -> Anchor {
    Anchor {
        track,
        wall_ms: now_ms(),
        position_ms,
        rate: 1.0,
        state: "playing",
    }
}

/// A realistic large load: a page of the list.
#[tauri::command]
fn page(offset: u64, limit: u64) -> Vec<Row> {
    (offset..offset + limit)
        .map(|id| Row {
            id,
            title: format!("Track {id}"),
            artist: format!("Artist {}", id % 997),
            album: format!("Album {}", id % 313),
            ms: 120_000 + (id % 240_000),
        })
        .collect()
}

/// The Rust -> JS event stream. On a separate thread, without blocking the call.
#[tauri::command]
fn emit_burst(app: AppHandle, count: u64) {
    std::thread::spawn(move || {
        for i in 0..count {
            if app.emit("tick", i).is_err() {
                return;
            }
        }
    });
}

/// So wherever the measurement hangs, it shows in run.log (the spirit of K9: a
/// failure must say which stage it is in).
#[tauri::command]
fn mark(msg: String) {
    eprintln!("STEP: {msg}");
}

/// Called after every step. The report is written **incrementally**: if step 7
/// hangs, the numbers of 1-6 must not be lost. Partial success reports too.
#[tauri::command]
fn save_report(json: String) {
    let path = std::env::current_dir()
        .unwrap_or_else(|_| ".".into())
        .join("report.json");
    if let Err(e) = std::fs::write(&path, json) {
        eprintln!("REPORT NOT WRITTEN: {e}");
    }
}

#[tauri::command]
fn quit(app: AppHandle) {
    eprintln!("DONE");
    app.exit(0);
}

/// D-029: the app sets up the environment fix-up itself.
///
/// Not an assumption but what is being tested: GDK reads `GDK_BACKEND` during
/// `gtk_init`, WebKit reads `WEBKIT_DISABLE_DMABUF_RENDERER` when the web process
/// is born — both after Tauri's setup. Is the first line of `main()` early enough?
///
/// The user's own setting **is not overridden**: when someone who wants to run on
/// Wayland on purpose sets `GDK_BACKEND=wayland`, we leave it alone.
#[cfg(target_os = "linux")]
fn fix_environment() {
    use std::os::unix::process::CommandExt;

    const FIXUP: [(&str, &str); 2] = [
        ("GDK_BACKEND", "x11"),
        ("WEBKIT_DISABLE_DMABUF_RENDERER", "1"),
    ];

    // Collect the missing ones. If none is missing we are already in a fixed-up
    // process — either the user set it up, or we are the child of the exec below.
    // That is the loop guard: in the child's eyes nothing is missing.
    let missing: Vec<_> = FIXUP
        .iter()
        .filter(|(name, _)| std::env::var_os(name).is_none())
        .collect();
    if missing.is_empty() {
        eprintln!("ENV: no fix-up needed");
        return;
    }

    let Ok(own_path) = std::env::current_exe() else {
        eprintln!("ENV: could not find my own path, fix-up skipped");
        return;
    };

    // `set_var` is `unsafe` in Rust 2024 and the workspace says
    // `unsafe_code = "forbid"` — `forbid` cannot be overridden with `allow` at the
    // package level. `exec` is a safe call: it replaces the process image, keeps
    // the PID, and the new environment is born into the child. Its only cost is
    // one restart.
    let mut command = std::process::Command::new(own_path);
    command.args(std::env::args_os().skip(1));
    for (name, value) in &missing {
        command.env(name, value);
        eprintln!("ENV: setting {name}={value} and restarting");
    }
    // `exec` only returns **if it fails**.
    let error = command.exec();
    eprintln!("ENV: could not restart ({error}), carrying on without the fix-up");
}

#[cfg(not(target_os = "linux"))]
fn fix_environment() {}

fn main() {
    fix_environment();
    let started = std::time::Instant::now();
    tauri::Builder::default()
        .setup(move |app| {
            // Measure when the window really shows: that is what "start-up" is
            // for the user. Focus is a must: WebKitGTK throttles rAF in an
            // invisible window, and throttled rAF measures the throttling, not
            // the thing.
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
            eprintln!("SETUP_MS: {}", started.elapsed().as_millis());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            anchor,
            page,
            emit_burst,
            mark,
            save_report,
            quit
        ])
        .run(tauri::generate_context!())
        .expect("could not start the tauri run");
}
