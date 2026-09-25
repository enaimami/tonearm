#!/usr/bin/env bash
# The PLAN §3.1 measurement run. The app runs the test itself, writes report.json
# and closes — so it needs no human eye and can be repeated.
set -u
cd "$(dirname "$0")" || exit 1

BIN=target/release/tauri-gonogo
[ -x "$BIN" ] || { echo "first: cargo build --release"; exit 1; }
rm -f report.json

echo "binary size: $(du -h "$BIN" | cut -f1)"
echo "starting (the window stays open ~30 s and closes itself)…"

T0=$(date +%s.%N)
"$BIN" 2> run.log &
PID=$!

PEAK=0
TICKS=0
LIMIT=750   # 150 s: if the interface hangs, it must not lock up the session
while kill -0 "$PID" 2>/dev/null; do
  RSS=$(awk '/^VmRSS:/{print $2}' "/proc/$PID/status" 2>/dev/null)
  [ -n "${RSS:-}" ] && [ "$RSS" -gt "$PEAK" ] && PEAK=$RSS
  TICKS=$((TICKS + 1))
  if [ "$TICKS" -gt "$LIMIT" ]; then
    echo "TIMEOUT: did not finish within 150 s, killing it"
    kill "$PID" 2>/dev/null
    break
  fi
  sleep 0.2
done
T1=$(date +%s.%N)

echo
echo "total run:  $(awk "BEGIN{printf \"%.2f\", $T1-$T0}") s"
echo "RSS peak:   $((PEAK / 1024)) MB"
grep -E 'SETUP_MS|REPORT' run.log 2>/dev/null

if [ -f report.json ]; then
  echo "--- report.json ---"
  cat report.json
else
  echo "NO REPORT — run.log:"
  tail -30 run.log
  exit 1
fi
