//! Fixing up the window environment on Linux (D-029, D-031).
//!
//! **Why it is needed.** The §3.1 measurement (D-028) found the frame rate
//! dropping 2.4× on the Wayland + DMABUF path: 58.8 → 23.8 fps. Both variables
//! are needed **together**; turning off DMABUF alone makes scrolling
//! *worse*.
//!
//! **Why `exec`.** `std::env::set_var` is `unsafe` in Rust 2024 and the
//! workspace says `unsafe_code = "forbid"` — `forbid` cannot be overridden
//! with `allow` at the package level. `CommandExt::exec` is a safe call: it
//! replaces the process image, keeps the PID (desktop/service integration
//! does not break), and the new environment is born into the child.
//!
//! **The loop guard comes from the structure, not a flag:** only *missing*
//! variables are set. In the eyes of the restarted process nothing is
//! missing, so it does not `exec` a second time.

/// The user's own setting **is not overridden**: when someone who wants to
/// run on Wayland on purpose sets `GDK_BACKEND=wayland`, we leave it alone.
#[cfg(target_os = "linux")]
const FIXUP: [(&str, &str); 2] = [
    ("GDK_BACKEND", "x11"),
    ("WEBKIT_DISABLE_DMABUF_RENDERER", "1"),
];

/// Fixes up the environment and restarts the process. Does nothing if it is
/// not needed.
///
/// If it returns, one of two things happened: no fix was needed, or the
/// restart failed. In the second case we do not silently run slowly — the
/// reason is written (K9).
#[cfg(target_os = "linux")]
pub fn fixup() {
    use std::os::unix::process::CommandExt as _;

    let missing: Vec<_> = FIXUP
        .iter()
        .filter(|(name, _)| std::env::var_os(name).is_none())
        .collect();
    if missing.is_empty() {
        return;
    }

    let Ok(exe) = std::env::current_exe() else {
        eprintln!("STEP: ENV_FIXUP — could not find my own path, fixup skipped");
        return;
    };

    let mut command = std::process::Command::new(exe);
    command.args(std::env::args_os().skip(1));
    for (name, value) in &missing {
        command.env(name, value);
    }
    let applied: Vec<&str> = missing.iter().map(|(name, _)| *name).collect();
    eprintln!(
        "STEP: ENV_FIXUP — setting {} and restarting (D-028)",
        applied.join(", ")
    );

    // `exec` only returns **if it fails**.
    let err = command.exec();
    eprintln!("STEP: ENV_FIXUP — could not restart ({err}), carrying on without the fixup");
}

/// Nothing to fix outside Linux: the measurement was specific to WebKitGTK.
#[cfg(not(target_os = "linux"))]
pub fn fixup() {}
