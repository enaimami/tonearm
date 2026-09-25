// headshell — hareket katmanı (D-072).
//
// Arayüzdeki her konum hareketi buradan geçer: yaylar, momentum izdüşümü,
// lastik bant, hız ölçümü. Apple'ın "Designing Fluid Interfaces" (WWDC 2018)
// konuşmasının web karşılığı; bağımlılık yok (bundler yok, npm yok).
//
// Üç kural:
//
// 1. **Yalnızca `transform` ve `opacity`** (D-028). WebKitGTK'da başka bir
//    özelliği canlandırmak kare hızını 58.8'den 47.6'ya düşürüyor.
// 2. **Her hareket kesilebilir.** Yeni bir hedef öğenin *ekrandaki*
//    değerinden ve hızından başlar; eski hareketin bitmesi beklenmez ve hız
//    sıfırlanmaz. Yarıda yakalanan bir öğe zıplamaz, geri döndürülen bir
//    hareket duvara çarpmaz.
// 3. **Süre temanın.** Yayların tepkisi `--headshell-duration`'dan gelir
//    (tepki = süre × 3; varsayılan 120ms → 0.36 sn). `0ms` hiç hareket yok
//    demek — Yüksek Karşıtlık teması bunu kullanıyor. `prefers-reduced-motion`
//    konum hareketini kapatır, opaklık geçişi kalır.
//
// Dosyanın üst düzeyi saf: DOM'a yalnızca çağrılan fonksiyonların içinde
// dokunuluyor. `tests/motion_js.rs` onu bu yüzden gömülü QuickJS'te
// değerlendirebiliyor — `anchor.js` ile aynı yol (D-070).

/// Yay tepkisinin tema süresine oranı. 120ms → 0.36 sn: Apple'ın taşıma ve
/// yeniden konumlama için verdiği 0.3–0.4 sn aralığının içi.
export const RESPONSE_PER_DURATION = 3;

/// Token okunamazsa kullanılan süre — `style.css`'teki varsayılanla aynı.
export const FALLBACK_DURATION_MS = 120;

// ————————————————————————————————————— saf hesap

/// `"120ms"`, `"0.2s"`, `"0"` → milisaniye. Okunamazsa `null`.
///
/// Birimsiz yalnızca `0` geçerli; CSS de öyle sayıyor.
export function parseDuration(text) {
  const value = String(text ?? "").trim();
  const match = /^(\d*\.?\d+)(ms|s)?$/.exec(value);
  if (!match) return null;
  const number = Number(match[1]);
  if (!Number.isFinite(number)) return null;
  if (!match[2]) return number === 0 ? 0 : null;
  return match[2] === "s" ? number * 1000 : number;
}

/// Tema süresi + erişilebilirlik tercihi → neyin canlanacağı.
///
/// `enabled` opaklık dahil her şey için, `spatial` konum ve ölçek için.
/// Okunamayan süre varsayılana düşer: tema yazarının yazım hatası arayüzü
/// hareketsiz bırakmasın, ama hareketi açmak için de tahmin yürütülmesin.
export function motionSettings(durationMs, reducedMotion) {
  const ms = durationMs ?? FALLBACK_DURATION_MS;
  const enabled = ms > 0;
  return {
    enabled,
    spatial: enabled && !reducedMotion,
    response: (ms * RESPONSE_PER_DURATION) / 1000,
  };
}

/// Bir yayın `t` saniye sonraki hâli: `[konum farkı, hız]`.
///
/// Apple'ın iki parametresi: **sönüm oranı** (1 = aşmadan oturur, 1'in altı
/// hedefi geçip salınır) ve **tepki** (sn). Tepki bir süre değil — yayın
/// sabit bir süresi yok, oturma zamanı ikisinden doğar.
///
/// `offset` hedefe göre konum (konum − hedef), `velocity` birim/sn. Kapalı
/// biçim çözüm: kare atlasa da, adım ne kadar büyük olursa olsun sapmaz.
export function springStep(offset, velocity, t, dampingRatio, response) {
  const omega = (2 * Math.PI) / response;
  const zeta = dampingRatio;
  if (zeta < 1) {
    const damped = omega * Math.sqrt(1 - zeta * zeta);
    const decay = Math.exp(-zeta * omega * t);
    const a = offset;
    const b = (velocity + zeta * omega * offset) / damped;
    const cos = Math.cos(damped * t);
    const sin = Math.sin(damped * t);
    return [
      decay * (a * cos + b * sin),
      decay * ((b * damped - zeta * omega * a) * cos - (a * damped + zeta * omega * b) * sin),
    ];
  }
  if (zeta === 1) {
    const decay = Math.exp(-omega * t);
    const b = velocity + omega * offset;
    return [decay * (offset + b * t), decay * (b - omega * (offset + b * t))];
  }
  // Aşırı sönümlü: iki gerçek kök, salınım yok.
  const root = Math.sqrt(zeta * zeta - 1);
  const r1 = -omega * (zeta - root);
  const r2 = -omega * (zeta + root);
  const c2 = (velocity - r1 * offset) / (r2 - r1);
  const c1 = offset - c2;
  const e1 = Math.exp(r1 * t);
  const e2 = Math.exp(r2 * t);
  return [c1 * e1 + c2 * e2, c1 * r1 * e1 + c2 * r2 * e2];
}

/// Bırakılan bir hareketin nerede duracağı (px). Apple'ın örnek kodundaki
/// üstel yavaşlama; fizik kitabının `v²/2a`'sı değil. `0.998` kaydırma
/// hissi, `0.99` daha kısa.
///
/// Hedef bırakılan noktaya göre değil **buna göre** seçilir: küçük bir fiske
/// büyük bir sonuç doğurur.
export function project(velocity, decelerationRate = 0.998) {
  return ((velocity / 1000) * decelerationRate) / (1 - decelerationRate);
}

/// Sınırın ötesine çekilen mesafenin ne kadarının izleneceği.
///
/// Sert duruş "dondu" diye okunur; artan direnç "duyuyorum ama burada başka
/// bir şey yok" diye. Çekilen mesafe ne kadar büyürse büyüsün sonuç
/// `dimension`'ı geçmez ve işaret korunur.
export function rubberband(overshoot, dimension, constant = 0.55) {
  if (!(dimension > 0)) return 0;
  return (overshoot * dimension * constant) / (dimension + constant * Math.abs(overshoot));
}

/// Son `windowMs` içindeki örneklerden hız (birim/sn).
///
/// Yalnızca son iki örneğe bakmak titrek bir hız verir; uzun bir pencere de
/// parmak durduktan sonra bırakılan bir hareketi hâlâ hızlı sanır.
export function createVelocityTracker(windowMs = 100) {
  const samples = [];
  return {
    add(time, value) {
      samples.push([time, value]);
      while (samples.length > 2 && time - samples[0][0] > windowMs) samples.shift();
    },
    velocity() {
      if (samples.length < 2) return 0;
      const [t0, v0] = samples[0];
      const [t1, v1] = samples[samples.length - 1];
      return t1 > t0 ? ((v1 - v0) / (t1 - t0)) * 1000 : 0;
    },
  };
}

// ————————————————————————————————————— tema ve tercih

let settings = null;

/// Şu anki hareket ayarı. Tema değişince [`invalidateMotion`] çağrılır.
export function currentMotion() {
  if (!settings) {
    const raw = getComputedStyle(document.documentElement).getPropertyValue(
      "--headshell-duration",
    );
    const reduced = globalThis.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
    settings = motionSettings(parseDuration(raw), reduced);
  }
  return settings;
}

/// Tema ya da sistem tercihi değişti: bir sonraki hareket yeniden okusun.
export function invalidateMotion() {
  settings = null;
}

/// Sistemin hareket tercihini izler. Açılışta bir kez çağrılır.
export function watchMotionPreference() {
  globalThis
    .matchMedia?.("(prefers-reduced-motion: reduce)")
    .addEventListener?.("change", invalidateMotion);
}

// ————————————————————————————————————— öğe yayları
//
// Her öğenin ekrandaki değerleri burada tutulur (`x`, `y`, `scaleX`,
// `scaleY`, `opacity`) ve tek bir `requestAnimationFrame` döngüsü koşan
// bütün yayları ilerletir. Yeni bir hedef aynı özellikteki yayı **devralır**
// (kural 2).

const RESTING = { x: 0, y: 0, scaleX: 1, scaleY: 1, opacity: 1 };
const PRECISION = { x: 0.1, y: 0.1, scaleX: 0.0005, scaleY: 0.0005, opacity: 0.002 };

const bodies = new WeakMap();
const active = new Set();
let frame = 0;
let lastTime = 0;

function bodyOf(el) {
  let body = bodies.get(el);
  if (!body) {
    body = {
      el,
      values: { ...RESTING },
      velocities: { x: 0, y: 0, scaleX: 0, scaleY: 0, opacity: 0 },
      springs: new Map(),
    };
    bodies.set(el, body);
  }
  return body;
}

/// `scale` iki eksene birden yazmanın kısaltması.
function expand(values) {
  const out = {};
  for (const [prop, value] of Object.entries(values)) {
    if (prop === "scale") {
      out.scaleX = value;
      out.scaleY = value;
    } else {
      out[prop] = value;
    }
  }
  return out;
}

function write(body) {
  const { x, y, scaleX, scaleY, opacity } = body.values;
  // Dinlenen öğe dönüşüm taşımaz: sürekli bir katman metni bazı motorlarda
  // bulanıklaştırıyor. Bu yüzden yayla sürülen bir öğenin **stil
  // dosyasında kendi `transform`'u olmamalı** — birim dönüşüme oturan öğe
  // satır içi değeri bırakır ve stil dosyasınınkine geri düşer (tam boya
  // varan bir çubuk `scaleX(0)`'a dönüp kayboluyordu). Başlangıç hâli
  // `from` ile verilir.
  const still = x === 0 && y === 0 && scaleX === 1 && scaleY === 1;
  body.el.style.transform = still
    ? ""
    : `translate3d(${x}px, ${y}px, 0) scale(${scaleX}, ${scaleY})`;
  body.el.style.opacity = opacity === 1 ? "" : String(Math.min(1, Math.max(0, opacity)));
}

function cancel(body, prop) {
  const spring = body.springs.get(prop);
  if (spring) {
    // Yarıda kalan yayın `onRest`'i çağrılmaz: hareket bitmedi, kesildi.
    active.delete(spring);
    body.springs.delete(prop);
  }
}

/// Ekrandaki değeri anında yazar; o özellikteki yay durur.
export function place(el, values) {
  const body = bodyOf(el);
  for (const [prop, value] of Object.entries(expand(values))) {
    cancel(body, prop);
    body.values[prop] = value;
    body.velocities[prop] = 0;
  }
  write(body);
}

/// Özelliğin ekranda şu anki değeri.
export function presentation(el, prop) {
  return bodyOf(el).values[prop];
}

/// Özellikleri yayla hedefe götürür.
///
/// Seçenekler: `from` (başlangıç — verilmezse ekrandaki değer), `velocity`
/// (birim/sn; bırakılan bir hareketin hızı buradan devredilir),
/// `dampingRatio` (varsayılan 1: aşma yok — yalnızca momentum taşıyan bir
/// hareketin ardından 1'in altı), `responseScale`, `onRest` (hepsi
/// oturunca; hareket kesilirse çağrılmaz).
export function animate(el, targets, options = {}) {
  const motion = currentMotion();
  const body = bodyOf(el);
  const { dampingRatio = 1, onRest } = options;
  const response = motion.response * (options.responseScale ?? 1);

  for (const [prop, value] of Object.entries(expand(options.from ?? {}))) {
    cancel(body, prop);
    body.values[prop] = value;
    body.velocities[prop] = 0;
  }
  const velocity = expand(options.velocity ?? {});

  let pending = 0;
  const settled = () => {
    pending -= 1;
    if (pending === 0) onRest?.();
  };

  for (const [prop, target] of Object.entries(expand(targets))) {
    const moves = prop === "opacity" ? motion.enabled : motion.spatial;
    if (!moves || !(response > 0)) {
      cancel(body, prop);
      body.values[prop] = target;
      body.velocities[prop] = 0;
      continue;
    }
    const previous = body.springs.get(prop);
    if (previous) active.delete(previous);
    if (velocity[prop] !== undefined) body.velocities[prop] = velocity[prop];
    const spring = { body, prop, target, dampingRatio, response, onRest: settled };
    body.springs.set(prop, spring);
    active.add(spring);
    pending += 1;
  }

  write(body);
  if (pending === 0) {
    onRest?.();
  } else if (!frame) {
    lastTime = performance.now();
    frame = requestAnimationFrame(step);
  }
}

function step(now) {
  frame = 0;
  // Sekme arka plandayken biriken süre tek karede harcanmasın: öğe
  // ışınlanmasın, yavaşlasın.
  const dt = Math.min(Math.max((now - lastTime) / 1000, 0), 1 / 24);
  lastTime = now;

  const touched = new Set();
  const finished = [];
  for (const spring of active) {
    const { body, prop, target } = spring;
    const [offset, velocity] = springStep(
      body.values[prop] - target,
      body.velocities[prop],
      dt,
      spring.dampingRatio,
      spring.response,
    );
    const precision = PRECISION[prop];
    if (Math.abs(offset) < precision && Math.abs(velocity) < precision * 10) {
      body.values[prop] = target;
      body.velocities[prop] = 0;
      finished.push(spring);
    } else {
      body.values[prop] = target + offset;
      body.velocities[prop] = velocity;
    }
    touched.add(body);
  }
  for (const body of touched) write(body);
  for (const spring of finished) {
    active.delete(spring);
    if (spring.body.springs.get(spring.prop) === spring) spring.body.springs.delete(spring.prop);
    spring.onRest();
  }
  if (active.size > 0) frame = requestAnimationFrame(step);
}

/// Düzen değişiminin öncesini ve sonrasını ölçer, farkı yayla kapatır
/// (FLIP). Bir kardeş kalkınca ötekiler zıplamaz, yerine kayar.
export function flip(elements, mutate) {
  const before = new Map(elements.map((el) => [el, el.getBoundingClientRect().top]));
  mutate();
  for (const [el, top] of before) {
    if (!el.isConnected) continue;
    const delta = top - el.getBoundingClientRect().top;
    if (Math.abs(delta) < 0.5) continue;
    // Devam eden bir hareketin üstüne eklenir: ekrandaki konum korunur.
    const body = bodyOf(el);
    body.values.y += delta;
    write(body);
    animate(el, { y: 0 });
  }
}

// ————————————————————————————————————— sürükleme

/// Yatay sürükleme: tutulan noktayı koruyarak 1:1 izler, bırakınca hızı verir.
///
/// `threshold` px geçilmeden sürükleme başlamaz — tıklama ve metin seçimi
/// yolda kalmasın. Önce dikey hareket baskın çıkarsa sürükleme hiç başlamaz
/// ve jest sayfaya kalır. `ignore` seçicisine uyan bir öğede başlayan basış
/// (düğme, katlanan ayrıntı) sürüklemez.
///
/// `onStart()` öğenin ekrandaki `x`'ini döndürür: hareketin ortasında
/// yakalanan öğe olduğu yerden devam eder.
export function horizontalDrag(el, { threshold = 8, ignore, onStart, onMove, onEnd }) {
  let pointer = null;

  el.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    if (ignore && event.target.closest(ignore)) return;
    pointer = {
      id: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      origin: 0,
      dragging: false,
      tracker: createVelocityTracker(),
    };
    pointer.tracker.add(event.timeStamp, event.clientX);
  });

  el.addEventListener("pointermove", (event) => {
    if (!pointer || event.pointerId !== pointer.id) return;
    const dx = event.clientX - pointer.startX;
    const dy = event.clientY - pointer.startY;
    if (!pointer.dragging) {
      if (Math.abs(dy) > threshold && Math.abs(dy) > Math.abs(dx)) {
        pointer = null;
        return;
      }
      if (Math.abs(dx) < threshold) return;
      pointer.dragging = true;
      el.setPointerCapture(event.pointerId);
      pointer.origin = onStart?.() ?? 0;
      // Eşik aşıldığı an ölçü buradan başlar: öğe eşik kadar sıçramaz.
      pointer.startX = event.clientX;
    }
    pointer.tracker.add(event.timeStamp, event.clientX);
    onMove?.(pointer.origin + (event.clientX - pointer.startX));
  });

  const finish = (event) => {
    if (!pointer || event.pointerId !== pointer.id) return;
    const ended = pointer;
    pointer = null;
    if (ended.dragging) onEnd?.(ended.tracker.velocity());
  };
  el.addEventListener("pointerup", finish);
  el.addEventListener("pointercancel", finish);
}
