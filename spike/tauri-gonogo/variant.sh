#!/usr/bin/env bash
# Is WebKitGTK's low frame rate a configuration problem, or the engine itself?
# Firefox draws the same page on the same machine at 58.8 fps; so it is not the
# hardware. What is left is WebKitGTK's acceleration path. Tries the known ways out.
set -u
cd "$(dirname "$0")" || exit 1
BIN=target/release/tauri-gonogo

run_variant() {
  local label="$1"; shift
  rm -f report.json
  echo "=== $label ==="
  env "$@" "$BIN" > /dev/null 2>&1 &
  local pid=$!
  local t=0
  while kill -0 "$pid" 2>/dev/null; do
    t=$((t+1)); [ "$t" -gt 600 ] && { kill "$pid" 2>/dev/null; break; }
    sleep 0.2
  done
  if [ -f report.json ]; then
    cp report.json "report-$label.json"
    python3 - "$label" <<'PY'
import json, sys
d = json.load(open("report.json"))
for f in d.get("phases", []):
    print(f"  {f['name']:34s} {f.get('fps_median','?'):>6} fps"
          f"   p50 {f.get('frame_p50_ms','?')} ms   p95 {f.get('frame_p95_ms','?')} ms")
PY
  else
    echo "  NO REPORT"
  fi
}

run_variant baseline       DUMMY=1
run_variant dmabuf-off     WEBKIT_DISABLE_DMABUF_RENDERER=1
run_variant x11            GDK_BACKEND=x11
run_variant x11-dmabuf-off GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1
