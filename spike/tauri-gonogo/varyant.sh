#!/usr/bin/env bash
# WebKitGTK'nın düşük kare hızı bir yapılandırma sorunu mu, motorun kendisi mi?
# Firefox aynı makinede aynı sayfayı 58.8 fps çiziyor; demek ki donanım değil.
# Geriye WebKitGTK'nın hızlandırma yolu kalıyor. Bilinen kaçış yollarını dener.
set -u
cd "$(dirname "$0")" || exit 1
BIN=target/release/tauri-gonogo

koş() {
  local etiket="$1"; shift
  rm -f report.json
  echo "=== $etiket ==="
  env "$@" "$BIN" > /dev/null 2>&1 &
  local pid=$!
  local t=0
  while kill -0 "$pid" 2>/dev/null; do
    t=$((t+1)); [ "$t" -gt 600 ] && { kill "$pid" 2>/dev/null; break; }
    sleep 0.2
  done
  if [ -f report.json ]; then
    cp report.json "report-$etiket.json"
    python3 - "$etiket" <<'PY'
import json, sys
d = json.load(open("report.json"))
for f in d.get("fazlar", []):
    print(f"  {f['name']:34s} {f.get('fps_medyan','?'):>6} fps"
          f"   p50 {f.get('kare_p50_ms','?')} ms   p95 {f.get('kare_p95_ms','?')} ms")
PY
  else
    echo "  RAPOR YOK"
  fi
}

koş temel            DUMMY=1
koş dmabuf-kapali    WEBKIT_DISABLE_DMABUF_RENDERER=1
koş x11              GDK_BACKEND=x11
koş x11-dmabuf-kapali GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1
