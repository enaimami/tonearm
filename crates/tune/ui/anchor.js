// Çapadan pozisyon tahmini — formülün JS kopyası (D-015, D-033).
//
// Bu dosya `tune_core::playback::anchor::PlaybackAnchor::position_at`'in
// ikinci kopyasıdır. İkinci kopya olduğunu bilerek yazıyoruz: webview
// pozisyonu çekirdeğe sormadan yürütmek zorunda, yoksa IPC her duraksadığında
// ilerleme çubuğu donardı.
//
// İki kopya zamanla kayar ve kayma kimsenin fark etmediği yerde başlar.
// Kilit: `fixtures/anchor/position_cases.json` — iki tarafın da okuduğu tek
// doğruluk kaynağı. Rust tarafını `tune-core/tests/anchor_parity.rs`,
// bu tarafı `tune/tests/anchor_parity.mjs` bağlıyor.
//
// **`Math.floor`, `Math.round` değil.** Çekirdek `as u64` ile kırpıyor:
// `rate 1.001` ile 100 sn'de 100100 değil 100099 ms çıkıyor
// (100000 × 1.001 ikilik tabanda 100099.999… ediyor). `Math.round` iki kopyayı
// tam buradan ayırırdı. Faz 4'te aynı formül oda senkronunu sürecek.

/// `wall_time` RFC 3339 metninden milisaniye.
///
/// Çekirdek `Timestamp::as_millisecond()` diyor, yani milisaniye altını
/// kırpıyor. `Date.parse` de öyle yapıyor.
function duvarSaatiMs(wallTime) {
  return Date.parse(wallTime);
}

function sureyeKirp(pozisyon, durationMs) {
  if (durationMs === null || durationMs === undefined) {
    return pozisyon;
  }
  return Math.min(pozisyon, durationMs);
}

/// Verilen an için pozisyon (ms).
///
/// `anchor` çekirdekten geldiği gibi: `{ track, wall_time, position_ms,
/// rate, state, duration_ms }`.
export function positionAt(anchor, nowMs) {
  // Zaman yalnızca `playing` iken ilerler. `buffering` ayrı bir durum:
  // ses çıkmıyor, sayaç da yürümemeli — "duraklatıldı" demek kullanıcıyı
  // yanıltır ama sayacı yürütmek de yalan söyler.
  if (anchor.state !== "playing" || anchor.rate <= 0) {
    return sureyeKirp(anchor.position_ms, anchor.duration_ms);
  }
  const gecen = nowMs - duvarSaatiMs(anchor.wall_time);
  // Geriye giden saat pozisyonu geri sarmaz.
  if (gecen <= 0) {
    return sureyeKirp(anchor.position_ms, anchor.duration_ms);
  }
  const ilerleme = Math.floor(gecen * anchor.rate);
  return sureyeKirp(anchor.position_ms + ilerleme, anchor.duration_ms);
}

/// Şu andaki pozisyon.
export function positionNow(anchor) {
  return positionAt(anchor, Date.now());
}

/// Milisaniyeyi `3:07` biçimine çevirir.
export function saat(ms) {
  const saniye = Math.floor(ms / 1000);
  const dakika = Math.floor(saniye / 60);
  return `${dakika}:${String(saniye % 60).padStart(2, "0")}`;
}
