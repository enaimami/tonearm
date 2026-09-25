// Estimating the position from the anchor — the JS copy of the formula
// (D-015, D-033).
//
// This file is the second copy of
// `headshell_core::playback::anchor::PlaybackAnchor::position_at`. We write it
// knowing it is a second copy: the webview has to advance the position
// without asking the core, otherwise the progress bar would freeze every
// time IPC stalls.
//
// Two copies drift over time, and the drift starts where nobody notices. The
// lock: `fixtures/anchor/position_cases.json` — the single source of truth
// both sides read. `headshell-core/tests/anchor_parity.rs` binds the Rust
// side, `headshell/tests/anchor_parity_js.rs` this side — by evaluating the
// file in embedded QuickJS, without needing `node` (D-070).
//
// **`Math.floor`, not `Math.round`.** The core truncates with `as u64`: with
// `rate 1.001`, 100 s gives 100099 ms, not 100100 (100000 × 1.001 comes to
// 100099.999… in binary). `Math.round` would split the two copies exactly
// here. In Phase 4 the same formula will drive room sync.

/// Milliseconds from the `wall_time` RFC 3339 text.
///
/// The core says `Timestamp::as_millisecond()`, which truncates below the
/// millisecond. `Date.parse` does the same.
function wallClockMs(wallTime) {
  return Date.parse(wallTime);
}

function clampToDuration(position, durationMs) {
  if (durationMs === null || durationMs === undefined) {
    return position;
  }
  return Math.min(position, durationMs);
}

/// The position for the given moment (ms).
///
/// `anchor` as it came from the core: `{ track, wall_time, position_ms, rate,
/// state, duration_ms }`.
export function positionAt(anchor, nowMs) {
  // Time only advances while `playing`. `buffering` is a separate state:
  // no sound comes out, so the counter must not move either — saying
  // "paused" misleads the user, but moving the counter lies too.
  if (anchor.state !== "playing" || anchor.rate <= 0) {
    return clampToDuration(anchor.position_ms, anchor.duration_ms);
  }
  const elapsed = nowMs - wallClockMs(anchor.wall_time);
  // A clock going backwards does not rewind the position.
  if (elapsed <= 0) {
    return clampToDuration(anchor.position_ms, anchor.duration_ms);
  }
  const advanced = Math.floor(elapsed * anchor.rate);
  return clampToDuration(anchor.position_ms + advanced, anchor.duration_ms);
}

/// The position right now.
export function positionNow(anchor) {
  return positionAt(anchor, Date.now());
}

/// Turns milliseconds into the `3:07` format.
export function clock(ms) {
  const seconds = Math.floor(ms / 1000);
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${String(seconds % 60).padStart(2, "0")}`;
}
