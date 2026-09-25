# GO / NO-GO thresholds

**This file was written BEFORE the measurement.** The reason: setting the
threshold after seeing the results isn't measuring; it's fitting the decision
to the measurement.

The measuring machine is weak on purpose: **Intel HD 6000 (Broadwell GT3,
2015), 4 cores, 8 GB RAM, Wayland, WebKitGTK 2.52.6.** The audience is mostly
Linux, and WebKitGTK is the weakest of the three platforms (PLAN §3.1). What
passes here generally passes; what fails here doesn't deserve the defence "it
was fine on a new machine".

| Measure | GO | BORDERLINE | NO-GO |
|---|---|---|---|
| Scrolling + CSS, median frame | ≤ 18 ms (≈55 fps) | ≤ 25 ms (≈40 fps) | > 25 ms |
| Scrolling + CSS, p95 frame | ≤ 25 ms | ≤ 40 ms | > 40 ms |
| Scrolling + CSS, worst frame | ≤ 120 ms | ≤ 300 ms | > 300 ms |
| IPC round trip p95 | ≤ 5 ms | ≤ 15 ms | > 15 ms |
| Cost of 30 Hz IPC to the frame | median increase ≤ 2 ms | ≤ 5 ms | > 5 ms |
| Event stream (Rust → JS) | ≥ 2000 events/s | ≥ 500 events/s | < 500 events/s |
| Until the window shows | ≤ 1500 ms | ≤ 3000 ms | > 3000 ms |
| Peak RSS (50k rows loaded) | ≤ 250 MB | ≤ 400 MB | > 400 MB |

## Why these numbers

- **The p95 of the frame time decides, not the median.** The user doesn't feel
  the average; they feel the stutter. A 25 ms p95 means a jolt noticed a few
  times a second while scrolling — accepted as borderline.
- **The IPC round trip shouldn't be on the hot path anyway.** §3.2 says the
  position **will be predicted** from the anchor; IPC is only in play on user
  actions (play, pause, search). That's why 5 ms is a generous threshold, not a
  tight one.
- **30 Hz IPC is measured because it is exactly §3.2's fear.** The measurement
  either confirms that fear or says "batching is unnecessary". Both are usable
  information.
- **The event stream threshold was kept low** (2000/s), because the design
  already aims not to send hundreds of messages per second. The number here is
  not a target but information about the ceiling: "batching" can't be designed
  without knowing the ceiling.

## The decision rule

- All **GO** → Tauri is chosen, and we move on to §3.2.
- One or two measures **BORDERLINE**, the rest GO → Tauri is chosen, but the
  borderline measure is written down as a design constraint of §3.2.
- Any **NO-GO** → stop and ask. The PLAN's alternative says "Dioxus / a
  native Rust GUI"; **but Dioxus desktop uses WebKitGTK too** (through wry). If
  this measurement fails, the real alternative isn't a webview but a stack
  that draws natively (egui / Dioxus native) — and its price is losing the CSS
  theme ecosystem. That choice isn't mine; it's the user's.
