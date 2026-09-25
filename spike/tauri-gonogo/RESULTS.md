# §3.1 GO / NO-GO — results

**Date:** 2026-08-31
**Machine:** Intel HD 6000 (Broadwell GT3, 2015), 4 cores, 8 GB, Wayland
**Engine:** WebKitGTK 2.52.6 (webkit2gtk-4.1), Tauri 2, 431 crates, 11 MB binary
**Thresholds:** `THRESHOLDS.md` — written **before the measurement**.

## Decision: GO — but conditional

Tauri is acceptable. **The condition:** on Linux it isn't acceptable without
two environment variables being set.

## Finding 1 — The virtualized list is not a problem

50,000 rows, continuous scrolling, node-pooled virtualization:
**58.8 fps, zero dropped frames.** The part of this measurement that passed
most easily.

## Finding 2 — In the default environment CSS animation drops the frame rate 2.4×

| Phase | Default (Wayland + DMABUF) | `GDK_BACKEND=x11` + `WEBKIT_DISABLE_DMABUF_RENDERER=1` |
|---|---|---|
| idle | 58.8 fps | 58.8 fps |
| scroll | 52.6 fps | 58.8 fps |
| scroll + disciplined CSS | **23.8 fps** | **58.8 fps** |
| scroll + naive CSS | **18.5 fps** | **47.6 fps** |
| scroll + CSS + IPC 30 Hz | **23.8 fps** | **55.6 fps** |

## Finding 3 — The culprit is not the hardware but the engine's path

The control experiment (`check.py`): **on the same machine, on the same page,
on the same GPU**, Firefox 154 draws **58.8 fps** in all four phases — naive
CSS included.

This distinction decided the outcome. If it were the hardware's ceiling,
escaping to a native Rust GUI wouldn't have saved it either, because it would
hit the same GPU. Without the control, the wrong decision would have been
made.

## Finding 4 — A single-variable fix is misleading

`WEBKIT_DISABLE_DMABUF_RENDERER=1` **alone makes scrolling worse**
(52.6 → 30.3 fps). `GDK_BACKEND=x11` alone doesn't rescue the CSS phase
(23.8 → 26.3 fps). Only **both together** help.

An investigation that settled for trying the variables one at a time would
conclude "this workaround doesn't work" and give a NO-GO.

## Finding 5 — §3.2's fear isn't real at this scale

- IPC round trip: **p50 1 ms, p95 1 ms, p99 2 ms** (1000 samples).
- Fetching a 200-row page: **p50 1 ms**.
- 30 Hz anchor polling adds **1 ms** to the frame time.
- Rust → JS event stream: **~10,000–11,000 events/s**.

"Hundreds of messages per second = stutter" **was not confirmed** on this
engine. Batching isn't needed for performance.

Predicting from the anchor (§3.2) is still the right design — but its reason
isn't performance: (a) if IPC stalls, the interface doesn't freeze, (b) Phase
4's room primitive is the same type anyway (D-015), so there's no need to keep
two separate position concepts.

## Finding 6 — Naive CSS is still expensive, but now acceptable

Even in the corrected environment, `height` / `box-shadow` / `filter` /
`background-position` animations take 58.8 → 47.6 fps; the disciplined set
(`transform` + `opacity`) doesn't drop it at all.

This is not a Tauri flaw but **a constraint that will go into §3.3's theme
contract**: if the theme author isn't told which properties can be animated,
the difference shows up on the user's machine.

## Reproducing

```bash
cargo build --release
GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 ./measure.sh   # Tauri
./variant.sh                                                    # 4 environment variants
python3 check.py firefox                                        # the control experiment
```

## Limits — what wasn't measured

- **One machine, one driver.** Mesa / Broadwell. Nvidia, AMD and newer Intel
  weren't tried; whether the fix is needed or harmless there is unknown.
- `GDK_BACKEND=x11` needs XWayland. What happens on a pure Wayland setup
  without XWayland wasn't tried.
- Window resizing, multiple windows and high DPI weren't measured.
- The measurement was done at 1100×800. At full-screen 4K the pixel count
  grows ~9×.
- WebKit rounds `performance.now()` to 1 ms; there's no sub-ms resolution.
