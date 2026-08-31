#!/usr/bin/env bash
# PLAN §3.1 ölçüm koşumu. Uygulama testi kendi çalıştırır, report.json yazar
# ve kapanır — insan gözü gerektirmesin, tekrarlanabilir olsun diye.
set -u
cd "$(dirname "$0")" || exit 1

BIN=target/release/tauri-gonogo
[ -x "$BIN" ] || { echo "önce: cargo build --release"; exit 1; }
rm -f report.json

echo "ikili boyutu: $(du -h "$BIN" | cut -f1)"
echo "başlatılıyor (pencere ~30 sn açık kalacak, kendi kapanır)…"

T0=$(date +%s.%N)
"$BIN" 2> run.log &
PID=$!

PEAK=0
TICKS=0
LIMIT=750   # 150 sn: arayüz takılırsa oturumu kilitlemesin
while kill -0 "$PID" 2>/dev/null; do
  RSS=$(awk '/^VmRSS:/{print $2}' "/proc/$PID/status" 2>/dev/null)
  [ -n "${RSS:-}" ] && [ "$RSS" -gt "$PEAK" ] && PEAK=$RSS
  TICKS=$((TICKS + 1))
  if [ "$TICKS" -gt "$LIMIT" ]; then
    echo "ZAMAN AŞIMI: 90 sn içinde bitmedi, öldürülüyor"
    kill "$PID" 2>/dev/null
    break
  fi
  sleep 0.2
done
T1=$(date +%s.%N)

echo
echo "toplam koşum: $(awk "BEGIN{printf \"%.2f\", $T1-$T0}") sn"
echo "RSS tepe:     $((PEAK / 1024)) MB"
grep -E 'KURULUM_MS|RAPOR' run.log 2>/dev/null

if [ -f report.json ]; then
  echo "--- report.json ---"
  cat report.json
else
  echo "RAPOR YOK — run.log:"
  tail -30 run.log
  exit 1
fi
