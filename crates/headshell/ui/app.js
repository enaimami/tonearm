// headshell — masaüstü arayüzü (PLAN §3.2, D-072).
//
// **Altın Kural burada da geçerli.** Bu dosya karar vermez: tuşu komuta
// çevirir, çekirdeğin döndürdüğü veriyi çizer. "Duraklat mı sürdür mü",
// "sırada ne var", "hangi çalma sayılır", "eklenti çalışabilir mi" soruların
// cevabı çekirdekte.
//
// Tek istisna bilerek konmuş: **pozisyon tahmini** (`anchor.js`). Formülün
// ikinci kopyası olduğu biliniyor ve doğruluk kümesiyle kilitli (D-033).
//
// Çekirdeğin metin üreten yardımcıları (`status_text`, `describe`) burada
// **tekrarlanmıyor**. CLI tek satıra sığdırmak için aralarında öncelik
// seçiyor; arayüzün böyle bir sıkışıklığı yok, hepsini yan yana gösteriyor.
// Seçim yapmayan kopya kaymaz (D-057).
//
// Hareket `motion.js`'te (yaylar, izdüşüm, sürükleme); burada yalnızca neyin
// nereye gittiği yazıyor. Metin her zaman `textContent` ile: çekirdekten ya
// da katalogdan gelen hiçbir veri işaretleme olarak yorumlanmaz
// (`ui_contract.rs` `innerHTML`'i yasaklıyor).
//
// Tanımlayıcılar İngilizce (D-036); kullanıcıya görünen metin Türkçe.

import { positionAt, clock } from "./anchor.js";
import {
  animate,
  flip,
  horizontalDrag,
  invalidateMotion,
  place,
  presentation,
  project,
  rubberband,
  watchMotionPreference,
} from "./motion.js";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);

// ————————————————————————————————————— öğe kurma

/// Küçük bir öğe kurucu. `text` her zaman `textContent`'e gider.
function h(tag, props = {}, ...children) {
  const el = document.createElement(tag);
  for (const [key, value] of Object.entries(props)) {
    if (value === undefined || value === null || value === false) continue;
    if (key === "class") el.className = value;
    else if (key === "text") el.textContent = value;
    else if (key === "dataset") Object.assign(el.dataset, value);
    else if (key.startsWith("on")) el.addEventListener(key.slice(2), value);
    else el.setAttribute(key, value === true ? "" : String(value));
  }
  for (const child of children.flat()) {
    if (child === null || child === undefined || child === false) continue;
    el.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return el;
}

const SVG = "http://www.w3.org/2000/svg";

/// `index.html`'deki simge setinden bir simge. Ad bir dize sabiti olarak
/// yazılmalı: `ui_contract.rs` `icon` ve `iconRef` çağrılarına verilen her
/// adın sette tanımlı olduğunu denetliyor.
function icon(name) {
  const svg = document.createElementNS(SVG, "svg");
  svg.setAttribute("class", "icon");
  svg.setAttribute("aria-hidden", "true");
  const use = document.createElementNS(SVG, "use");
  use.setAttribute("href", iconRef(name));
  svg.append(use);
  return svg;
}

function iconRef(name) {
  return `#i-${name}`;
}

// ————————————————————————————————————— biçim

const numbers = new Intl.NumberFormat("tr-TR");
const percent = new Intl.NumberFormat("tr-TR", { style: "percent", maximumFractionDigits: 1 });

function fmt(n) {
  return numbers.format(n);
}

/// CLI'nin `duration()` biçimi: `3sa 12dk` ya da `12dk`.
function durationText(ms) {
  const minutes = Math.floor(ms / 60000);
  const hours = Math.floor(minutes / 60);
  return hours > 0 ? `${fmt(hours)}sa ${minutes % 60}dk` : `${minutes}dk`;
}

function hoursText(ms) {
  return `${new Intl.NumberFormat("tr-TR", { maximumFractionDigits: 1 }).format(ms / 3600000)} sa`;
}

/// Zincirin ilk sebebi: `ADIM:` satırı ve `→` öneki atılmış hâli.
function firstLine(chain) {
  if (!chain) return "bilinmeyen hata";
  const lines = chain
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith("ADIM:"));
  return (lines[0] ?? chain).replace(/^→\s*/, "");
}

// ————————————————————————————————————— uyarılar
//
// Bir hata aşamasıyla görünür (K9): "nerede kırıldı" ilk bakışta
// cevaplanmalı. Tam zincir katlı durur ve tek tuşla kopyalanır — `diag`'ın
// son çalıştırması o arada başka bir komutla değişmiş olabilir, ama bu blok
// değişmez.
//
// Uyarı aşağıdan (oynatıcının yanından) gelir ve sağa gider: kapatma düğmesi,
// süre dolması ve sağa sürükleme aynı yoldan çıkar.

const TOAST_LIFETIME = { info: 6000, error: 20000 };
const toastTimers = new Map();

function toast(err, info = false) {
  const box = h("div", { class: info ? "toast info" : "toast", role: info ? "status" : "alert" });

  if (info) {
    box.append(h("div", { text: err }));
  } else {
    const stage = err?.stage ?? "BİLİNMİYOR";
    const chain = err?.chain ?? String(err);
    box.append(
      h("span", { class: "stage", text: `ADIM: ${stage}` }),
      h("div", { text: firstLine(err?.chain) }),
      h("details", {}, h("summary", { text: "tam zincir" }), h("pre", { text: chain })),
      h(
        "div",
        { class: "toast-actions" },
        h(
          "button",
          {
            type: "button",
            onclick: () =>
              copyText(chain.startsWith("ADIM:") ? chain : `ADIM: ${stage}\n  ${chain}`, "hata zinciri panoya kopyalandı"),
          },
          icon("copy"),
          "kopyala",
        ),
      ),
    );
  }

  box.append(
    h(
      "button",
      {
        class: "close",
        type: "button",
        title: "kapat",
        "aria-label": "uyarıyı kapat",
        onclick: () => dismissToast(box),
      },
      icon("close"),
    ),
  );

  const stack = $("toasts");
  flip([...stack.children], () => stack.append(box));
  animate(box, { y: 0, opacity: 1 }, { from: { y: 18, opacity: 0 } });

  const lifetime = info ? TOAST_LIFETIME.info : TOAST_LIFETIME.error;
  armToast(box, lifetime);
  // Okunurken gitmesin: imleç üstündeyken sayaç durur.
  box.addEventListener("pointerenter", () => clearTimeout(toastTimers.get(box)));
  box.addEventListener("pointerleave", () => armToast(box, Math.min(lifetime, 4000)));

  horizontalDrag(box, {
    ignore: "button, summary, pre",
    onStart: () => {
      clearTimeout(toastTimers.get(box));
      return presentation(box, "x");
    },
    // Sola çekmek kapatmaz: direnç artar, bırakınca geri gelir.
    onMove: (x) =>
      place(box, {
        x: x < 0 ? rubberband(x, box.offsetWidth) : x,
        opacity: 1 - Math.max(0, x) / (box.offsetWidth * 1.4),
      }),
    // Karar bırakılan noktadan değil, hızın taşıyacağı yerden: kısa bir
    // fiske de kapatır.
    onEnd: (velocity) => {
      const x = presentation(box, "x");
      if (x + project(velocity) > box.offsetWidth * 0.5) {
        dismissToast(box, velocity);
      } else {
        animate(box, { x: 0, opacity: 1 }, { velocity: { x: velocity }, dampingRatio: 0.8 });
        armToast(box, 4000);
      }
    },
  });
}

function armToast(box, ms) {
  clearTimeout(toastTimers.get(box));
  toastTimers.set(
    box,
    setTimeout(() => dismissToast(box), ms),
  );
}

function dismissToast(box, velocity = 0) {
  if (!box.isConnected) return;
  clearTimeout(toastTimers.get(box));
  toastTimers.delete(box);
  animate(
    box,
    { x: box.offsetWidth + 48, opacity: 0 },
    {
      velocity: { x: Math.max(velocity, 0) },
      onRest: () => {
        const stack = $("toasts");
        flip(
          [...stack.children].filter((el) => el !== box),
          () => box.remove(),
        );
      },
    },
  );
}

function dismissAllToasts() {
  for (const box of [...$("toasts").children]) dismissToast(box);
}

/// Komutu çağırır; hata olursa gösterir ve `undefined` döner.
///
/// Yutmuyor: her başarısızlık ekranda aşamasıyla görünüyor. `undefined`
/// dönmesi çağıranın çizimi atlaması için — sessiz boş sonuç değil.
async function call(command, args = {}) {
  try {
    return await invoke(command, args);
  } catch (err) {
    toast(err);
    return undefined;
  }
}

async function copyText(text, done) {
  try {
    await navigator.clipboard.writeText(text);
    toast(done, true);
  } catch (err) {
    // Pano reddedilirse sessiz kalmıyoruz: kullanıcı kopyalandığını sanıp
    // boş bir hata raporu gönderirdi.
    toast({ stage: "CONFIG_LOAD", chain: `ADIM: CONFIG_LOAD\n  → pano yazılamadı: ${err}` });
  }
}

// ————————————————————————————————————— dosya diyaloğu
//
// Eklentinin JS sarmalayıcısı npm'den gelir; bu arayüzde bundler yok, o
// yüzden komut doğrudan çağrılıyor. İzin `capabilities/default.json`'da
// `open`/`save` ile sınırlı: diyalog bir **yol** döndürür, o yolu okuyup
// yazan taraf çekirdektir.

async function pickFile(options) {
  return filePath(await call("plugin:dialog|open", { options }));
}

async function pickSavePath(options) {
  return filePath(await call("plugin:dialog|save", { options }));
}

/// Diyalog masaüstünde düz yol döndürür; mobil `content://` biçimini
/// nesne olarak verir. İkisini de kabul etmek bir varsayımı kaldırıyor.
function filePath(picked) {
  if (!picked) return null;
  if (typeof picked === "string") return picked;
  return picked.path ?? null;
}

// ————————————————————————————————————— bölümler
//
// Sıra kenar çubuğunun sırası ve `Ctrl`+sayı kısayolu bu sıradan gelir
// (`ui_contract.rs` ikisinin aynı kümeyi adlandırdığını denetliyor). Yeni bir
// bölüm buraya, kenar çubuğunda bir gruba ve `ON_OPEN`'a eklenir.

const PANELS = ["now", "library", "stats", "sleeve", "import", "providers", "plugins", "theme", "diag"];

// Bölüm açılınca yapılan iş. Hepsi yerel okuma: ağa çıkan hiçbir şey burada
// yok — katalog yalnızca düğmeyle okunur (D-071).
const ON_OPEN = {
  stats: () => {
    if (!stats.loaded) runStats(null);
  },
  sleeve: () => {
    sleeveFormat.placeThumb(true);
    if (!sleeve.previewed) previewSleeve();
  },
  providers: refreshProviders,
  plugins: () => {
    pluginTabs.placeThumb(true);
    refreshPlugins();
  },
  theme: refreshThemes,
  diag: refreshDiag,
};

let currentPanel = "now";
const scrollByPanel = new Map();
const content = $("content");

function openPanel(name, { instant = false } = {}) {
  if (!PANELS.includes(name)) return;
  const previous = currentPanel;
  currentPanel = name;

  for (const button of document.querySelectorAll(".nav")) {
    button.setAttribute("aria-current", String(button.dataset.panel === name));
  }
  movePill(instant);

  if (previous !== name) {
    scrollByPanel.set(previous, content.scrollTop);
    for (const section of document.querySelectorAll(".panel")) {
      section.hidden = section.id !== `panel-${name}`;
    }
    content.scrollTop = scrollByPanel.get(name) ?? 0;
    updateScrollEdge();
    // Listede aşağıdaki bölüm aşağıdan gelir: hareket nereye gidildiğini
    // söylesin.
    const section = document.getElementById(`panel-${name}`);
    const direction = PANELS.indexOf(name) > PANELS.indexOf(previous) ? 1 : -1;
    if (!instant) {
      animate(section, { y: 0, opacity: 1 }, { from: { y: 10 * direction, opacity: 0 }, responseScale: 0.7 });
    }
  }
  ON_OPEN[name]?.();
}

/// Seçim göstergesini seçili sekmenin altına götürür. Hızlı art arda
/// seçimlerde yay devralınır: gösterge yolun ortasından döner.
function movePill(instant) {
  const pill = $("navPill");
  const active = document.querySelector(`.nav[data-panel="${currentPanel}"]`);
  if (!active) return;
  const y = active.offsetTop;
  if (instant || pill.hidden) {
    place(pill, { y });
    pill.hidden = false;
  } else {
    animate(pill, { y });
  }
}

for (const button of document.querySelectorAll(".nav")) {
  // Seçim fare indiği an: bırakmayı beklemek tepkiyi geciktirir (masaüstü
  // kenar çubukları da böyle). `click` klavye ve dokunma için.
  button.addEventListener("pointerdown", (event) => {
    if (event.button === 0 && event.pointerType !== "touch") openPanel(button.dataset.panel);
  });
  button.addEventListener("click", () => openPanel(button.dataset.panel));
}

// "başlarken" kartındaki kısayollar.
for (const button of document.querySelectorAll(".go")) {
  button.addEventListener("click", () => {
    const target = button.dataset.go;
    if (target === "focus-play") {
      $("playQuery").focus();
    } else if (target === "plugins") {
      openPanel("plugins");
      pluginTabs.select($("tabCatalog"));
    } else {
      openPanel(target);
    }
  });
}

// Kaydırma kenarı: içerik üst çubuğun altına girince gölge belirir.
function updateScrollEdge() {
  $("mainColumn").classList.toggle("scrolled", content.scrollTop > 2);
}
content.addEventListener("scroll", updateScrollEdge, { passive: true });

window.addEventListener("resize", () => {
  movePill(true);
  sleeveFormat.placeThumb(true);
  pluginTabs.placeThumb(true);
});

// ————————————————————————————————————— parçalı denetim

/// Parçalı denetim: başparmak seçilen parçanın altına yayla kayar. Aynı
/// yapı hem seçenek (`radiogroup`) hem sekme (`tablist`) için.
function segmented(container, onSelect) {
  const thumb = container.querySelector(".segmented-thumb");
  const segments = [...container.querySelectorAll(".segment")];
  const attribute = container.getAttribute("role") === "tablist" ? "aria-selected" : "aria-checked";
  const selected = () => segments.find((s) => s.getAttribute(attribute) === "true") ?? segments[0];

  function placeThumb(instant = false) {
    const current = selected();
    if (!current.offsetWidth) return; // gizli bölümde ölçü yok; açılınca yerleşir
    thumb.style.width = `${current.offsetWidth}px`;
    const x = current.offsetLeft - 2;
    if (instant) place(thumb, { x });
    else animate(thumb, { x });
  }

  function select(segment, { focus = false } = {}) {
    if (!segment || segment.getAttribute(attribute) === "true") return;
    for (const s of segments) {
      s.setAttribute(attribute, String(s === segment));
      s.tabIndex = s === segment ? 0 : -1;
    }
    placeThumb();
    if (focus) segment.focus();
    onSelect(segment);
  }

  for (const segment of segments) {
    segment.tabIndex = segment.getAttribute(attribute) === "true" ? 0 : -1;
    segment.addEventListener("pointerdown", (event) => {
      if (event.button === 0 && event.pointerType !== "touch") select(segment);
    });
    segment.addEventListener("click", () => select(segment));
    segment.addEventListener("keydown", (event) => {
      const step = { ArrowRight: 1, ArrowLeft: -1 }[event.key];
      if (!step) return;
      event.preventDefault();
      const index = (segments.indexOf(segment) + step + segments.length) % segments.length;
      select(segments[index], { focus: true });
    });
  }
  return { select, placeThumb, selected };
}

// ————————————————————————————————————— oynatıcı
//
// Burada tutulan **her şey çekirdekten geldi**. Kendi başına bir doğruluk
// kaynağı değil, son cevabın kopyası: kuyruk, çapa.

let anchor = null;
let queue = { items: [], position: 0, repeat: "off", shuffle: false };

// Tekrar kipinin kullanıcıya gösterilen adı (D-036). Anahtarlar tel
// değerleridir; bilinmeyen bir kip gelirse ham değer gösterilir — sessizce
// "kapalı" demek, yanlış durumu doğru gibi göstermek olurdu (K9).
const REPEAT_LABELS = { off: "kapalı", all: "tümü", one: "tek parça" };
const STATE_TEXT = {
  playing: "çalıyor",
  paused: "duraklatıldı",
  buffering: "arabelleğe alınıyor",
  stopped: "durdu",
};

function renderQueue(view) {
  if (!view) return;
  queue = view;

  $("queue").replaceChildren(...view.items.map((item, index) => queueRow(item, index, index === view.position)));
  const empty = view.items.length === 0;
  $("queueBlock").hidden = empty;
  // Kuyruk boşken yerini "başlarken" kartı alıyor: boş bir liste kullanıcıya
  // ne yapacağını söylemez.
  $("onboarding").hidden = !empty;
  $("queueCount").textContent = empty ? "" : `· ${fmt(view.items.length)} parça`;

  const shuffle = $("btnShuffle");
  shuffle.classList.toggle("on", view.shuffle);
  shuffle.setAttribute("aria-pressed", String(view.shuffle));
  const repeat = $("btnRepeat");
  repeat.classList.toggle("on", view.repeat !== "off");
  repeat.setAttribute("aria-pressed", String(view.repeat !== "off"));
  $("repeatIcon").setAttribute("href", view.repeat === "one" ? iconRef("repeat-one") : iconRef("repeat"));
  // Tel değeri (`off`/`all`/`one`) İngilizce kalır — JSON anahtarıdır.
  // Kullanıcının okuduğu metin Türkçedir (D-036).
  repeat.title = `tekrar: ${REPEAT_LABELS[view.repeat] ?? view.repeat}`;

  renderNow();
}

function queueRow(item, index, current) {
  const track = item.track;
  return h(
    "li",
    { class: current ? "current" : null, "aria-current": current ? "true" : null },
    h(
      "button",
      {
        class: "q-row",
        type: "button",
        title: current ? "çalan parça" : "bu parçaya geç",
        // Kuyrukta atlama kararı çekirdekte: burada yalnızca indeks iletiliyor.
        onclick: async () => {
          renderQueue(await call("jump_to", { index }));
          renderAnchor(await call("anchor"));
        },
      },
      h("span", { class: "index", text: String(index + 1) }),
      h(
        "span",
        { class: "q-main" },
        h("span", { class: "q-title", text: track.title }),
        h("span", { class: "q-artist", text: track.artist }),
      ),
      h("span", { class: "chip", text: item.id.provider, title: "sağlayıcı" }),
      h("span", { class: "q-time", text: track.duration_ms ? clock(track.duration_ms) : "" }),
    ),
  );
}

/// Çalan parça: oynatıcı çubuğu ve "çalan" bölümünün kartı. Parça kuyruktan,
/// durum çapadan geliyor.
function renderNow() {
  const current = queue.items[queue.position];
  const track = current?.track;
  const state = anchor?.state ?? "stopped";

  $("nowTitle").textContent = track ? track.title : "—";
  $("nowArtist").textContent = track ? track.artist : "";

  const card = $("nowCard");
  card.hidden = !current;
  if (!current) return;
  card.className = `now-card ${state}`;
  $("nowCardState").textContent = STATE_TEXT[state] ?? state;
  $("nowCardTitle").textContent = track.title;
  $("nowCardArtist").textContent = [track.artist, track.album].filter(Boolean).join(" · ");
  $("nowCardSource").textContent = current.id.provider;
  $("nowCardDuration").textContent = track.duration_ms ? clock(track.duration_ms) : "";
}

function renderAnchor(next) {
  if (!next) return;
  anchor = next;
  const state = next.state;
  $("nowState").className = `state ${state}`;
  const running = state === "playing" || state === "buffering";
  $("playPauseIcon").setAttribute("href", running ? iconRef("pause") : iconRef("play"));
  $("btnPlayPause").setAttribute("aria-label", running ? "duraklat" : "çal");
  $("player").classList.toggle("buffering", state === "buffering");
  renderNow();
  startDrawing();
}

// Çizim döngüsü: pozisyon **çekirdeğe sorulmuyor**, çapadan tahmin ediliyor
// (D-015). IPC duraksarsa çubuk yürümeye devam eder. Döngü yalnızca çalarken
// koşar ve DOM'a yalnızca görünen bir şey değişince yazar: duraklamış bir
// parça her karede aynı metni yeniden yazdırmasın.
const barFill = $("barFill");
const timeLabel = $("timeLabel");
let drawing = 0;
let drawnRatio = -1;
let drawnLabel = "";

function draw() {
  if (!anchor) return;
  const position = positionAt(anchor, Date.now());
  const duration = anchor.duration_ms ?? 0;
  const ratio = duration > 0 ? Math.min(position / duration, 1) : 0;
  if (Math.abs(ratio - drawnRatio) > 0.0004) {
    barFill.style.transform = `scaleX(${ratio})`;
    drawnRatio = ratio;
  }
  const label = `${clock(position)} / ${duration > 0 ? clock(duration) : "—:—"}`;
  if (label !== drawnLabel) {
    timeLabel.textContent = label;
    drawnLabel = label;
  }
}

function drawFrame() {
  drawing = 0;
  draw();
  if (anchor?.state === "playing") drawing = requestAnimationFrame(drawFrame);
}

function startDrawing() {
  if (!drawing) drawing = requestAnimationFrame(drawFrame);
}

async function togglePause() {
  // Kuyruk boşsa çalınacak bir şey yok: düğme ne yapılacağını gösteriyor.
  if (queue.items.length === 0) {
    $("playQuery").focus();
    return;
  }
  renderAnchor(await call("toggle_pause"));
}

async function playNext() {
  renderQueue(await call("next"));
  renderAnchor(await call("anchor"));
}

async function playPrevious() {
  renderQueue(await call("previous"));
  renderAnchor(await call("anchor"));
}

$("btnPlayPause").addEventListener("click", togglePause);
$("btnNext").addEventListener("click", playNext);
$("btnPrev").addEventListener("click", playPrevious);
$("btnStop").addEventListener("click", async () => {
  renderAnchor(await call("stop"));
  renderQueue(await call("queue"));
});
$("btnShuffle").addEventListener("click", async () =>
  renderQueue(await call("set_shuffle", { on: !queue.shuffle })),
);
$("btnRepeat").addEventListener("click", async () => {
  const nextMode = { off: "all", all: "one", one: "off" }[queue.repeat] ?? "off";
  renderQueue(await call("set_repeat", { mode: nextMode }));
});

// ————————————————————————————————————— çal

function playQuery() {
  return $("playQuery").value.trim();
}

async function startPlay(query, all) {
  const view = await call("play", { query, all, shuffle: false });
  if (!view) return;
  renderQueue(view);
  renderAnchor(await call("anchor"));
  openPanel("now");
}

$("playForm").addEventListener("submit", (event) => {
  event.preventDefault();
  const query = playQuery();
  if (query) startPlay(query, false);
});

// Shift+Enter: eşleşen her parçayı kuyruğa al. Kuyruğu **çekirdek** kuruyor;
// burada sonuçlar toplanıp gönderilmiyor.
$("playQuery").addEventListener("keydown", (event) => {
  if (event.key !== "Enter" || !event.shiftKey) return;
  event.preventDefault();
  const query = playQuery();
  if (query) startPlay(query, true);
});

$("btnPlayAll").addEventListener("click", () => {
  const query = playQuery();
  if (query) startPlay(query, true);
  else $("playQuery").focus();
});

// ————————————————————————————————————— klavye
//
// Kısayol bir yazı kutusundayken çalışmaz: boşluk tuşu arama kutusuna
// boşluk yazmalı, oynatmayı durdurmamalı.

function isTyping(target) {
  if (!target) return false;
  const tag = target.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || target.isContentEditable;
}

document.addEventListener("keydown", (event) => {
  if (event.defaultPrevented) return;
  if (helpOpen) {
    if (event.key === "Escape") closeHelp();
    // Pencere açıkken odak onun içinde kalır; arkadaki kısayollar susar.
    if (event.key === "Tab") {
      event.preventDefault();
      $("btnHelpClose").focus();
    }
    return;
  }
  if (event.key === "Escape") {
    dismissAllToasts();
    return;
  }
  // Ctrl+1…9: bölümler. Yazı kutusundayken de çalışır — bir sayı tuşuna
  // Ctrl ile basmak yazmak değildir.
  if ((event.ctrlKey || event.metaKey) && !event.altKey) {
    const slot = Number(event.key);
    if (Number.isInteger(slot) && slot >= 1 && slot <= PANELS.length) {
      event.preventDefault();
      openPanel(PANELS[slot - 1]);
    }
    return;
  }
  if (isTyping(event.target) || event.ctrlKey || event.metaKey || event.altKey) return;

  if (event.key === " ") {
    event.preventDefault();
    togglePause();
  } else if (event.key === "ArrowRight") {
    event.preventDefault();
    playNext();
  } else if (event.key === "ArrowLeft") {
    event.preventDefault();
    playPrevious();
  } else if (event.key === "/") {
    event.preventDefault();
    $("playQuery").focus();
  } else if (event.key === "?") {
    event.preventDefault();
    openHelp();
  }
});

// ————————————————————————————————————— kısayol penceresi
//
// Açan düğmeden büyür, kapanırken ona döner (uzamsal süreklilik). Kapanırken
// yeniden açılırsa yarıda döner; bitmesini beklemez.

let helpOpen = false;
let helpReturnFocus = null;

function openHelp() {
  const sheet = $("helpSheet");
  const box = $("helpBox");
  if (!helpOpen) helpReturnFocus = document.activeElement;
  helpOpen = true;
  const fresh = sheet.hidden;
  sheet.hidden = false;

  const origin = $("btnHelp").getBoundingClientRect();
  const rect = box.getBoundingClientRect();
  box.style.transformOrigin = `${origin.left + origin.width / 2 - rect.left}px ${origin.top + origin.height / 2 - rect.top}px`;

  animate(sheet, { opacity: 1 }, fresh ? { from: { opacity: 0 } } : {});
  animate(box, { scale: 1, opacity: 1 }, fresh ? { from: { scale: 0.92, opacity: 0 } } : {});
  $("btnHelpClose").focus();
}

function closeHelp() {
  if (!helpOpen) return;
  helpOpen = false;
  const sheet = $("helpSheet");
  const box = $("helpBox");
  animate(sheet, { opacity: 0 });
  animate(box, { scale: 0.92, opacity: 0 }, { onRest: () => (sheet.hidden = true) });
  helpReturnFocus?.focus?.();
}

$("btnHelp").addEventListener("click", openHelp);
$("btnHelpClose").addEventListener("click", closeHelp);
$("helpSheet").addEventListener("pointerdown", (event) => {
  // Kutunun dışına basmak kapatır; içine basmak kapatmaz.
  if (event.target === $("helpSheet")) closeHelp();
});

// ————————————————————————————————————— kütüphane araması

const SEARCH_LIMIT = 100;
let lastSearchQuery = null;

$("searchForm").addEventListener("submit", async (event) => {
  event.preventDefault();
  const query = $("searchQuery").value.trim();
  if (!query) return;
  const report = await call("search", { query, limit: SEARCH_LIMIT, minMs: null });
  if (!report) return;
  // Son sorgunun kopyası — "hepsini kuyruğa al" sorguyu tekrar yazdırmasın.
  lastSearchQuery = report.query;

  const hits = report.hits;
  $("searchResult")
    .querySelector("tbody")
    .replaceChildren(
      ...hits.map((hit) =>
        h(
          "tr",
          {},
          h("td", { text: hit.title }),
          h("td", { text: hit.artist }),
          h("td", { class: "dim", text: hit.album ?? "" }),
          h("td", { class: "num", text: fmt(hit.play_count) }),
          h("td", { class: "num dim", text: durationText(hit.ms_played) }),
          h(
            "td",
            {},
            h(
              "button",
              { type: "button", onclick: () => startPlay(`${hit.artist} ${hit.title}`, false) },
              icon("play"),
              "çal",
            ),
          ),
        ),
      ),
    );
  const found = hits.length > 0;
  $("searchIdle").hidden = true;
  $("searchResult").hidden = !found;
  $("btnSearchPlayAll").hidden = !found;
  $("searchCount").hidden = !found;
  $("searchCount").textContent =
    hits.length >= SEARCH_LIMIT ? `ilk ${fmt(SEARCH_LIMIT)} sonuç` : `${fmt(hits.length)} sonuç`;
  $("searchEmpty").hidden = found;
  $("searchEmpty").textContent = `"${report.query}" için kayıt bulunamadı.`;
});

// Kuyruğu **çekirdek** kuruyor: burada sonuç satırları toplanıp gönderilmiyor,
// aynı sorgu `all` bayrağıyla tekrar veriliyor. Eşleştirmenin ikinci bir
// kopyası arayüzde yaşamasın.
$("btnSearchPlayAll").addEventListener("click", () => {
  if (lastSearchQuery) startPlay(lastSearchQuery, true);
});

// ————————————————————————————————————— istatistik
//
// Yıl çubukları hem grafik hem seçici: bir çubuğa basmak o yıla daraltır,
// seçili çubuğa yeniden basmak tüm zamanlara döner. Çubukların kendisi tüm
// zamanlar raporunun `by_year`'ından; yıl seçilince değişmezler, yalnızca
// hangisinin seçili olduğu değişir.

const stats = { loaded: false, year: null, years: [] };

async function runStats(year) {
  const response = await call("stats", {
    year,
    top: Number($("statsTop").value) || 10,
    minMs: null,
  });
  if (!response) return;
  const report = response.report;
  stats.loaded = true;
  stats.year = year;
  if (year === null) {
    stats.years = report.by_year;
    renderYears();
  }
  renderStats(report);
  updateYearSelection();
}

function figure(big, label, muted = false) {
  return h(
    "div",
    { class: muted ? "muted" : null },
    h("span", { class: "big", text: big }),
    h("span", { class: "label", text: label }),
  );
}

function renderStats(r) {
  const scope = r.query.year ?? "tüm zamanlar";
  $("statsScope").textContent = `${scope} · kapsamda ${fmt(r.listens_in_scope)} kayıt`;
  $("statsSummary").replaceChildren(
    figure(fmt(r.plays), "sayılan dinleme"),
    figure(hoursText(r.total_ms_played), "toplam süre"),
    figure(fmt(r.unique_artists), "sanatçı"),
    figure(fmt(r.unique_tracks), "parça"),
  );
  // Kısmi başarı **her zaman** raporlanır: kaç kayıt eşiğin altında kaldı,
  // kaçı kanonik kimliksiz, kaçı yıl dışında. Sessizce yutulmuyor (K9).
  const notes = [
    `${fmt(r.skipped_short)} kısa çalma eşiğin altında`,
    `${fmt(r.without_canonical_id)} kayıt kanonik kimliksiz`,
  ];
  if (r.query.year !== null) notes.push(`${fmt(r.out_of_scope)} kayıt yıl dışında`);
  $("statsNotes").textContent = notes.join(" · ");
  // Sıfır dinleme bir hata değil ama boş bir tablo da cevap değil: ne
  // yapılacağı yazıyor.
  $("statsEmpty").hidden = r.plays > 0;
  $("statsLists").hidden = r.plays === 0;

  renderRanking($("statsArtists"), r.top_artists, (a) => [a.artist, `${fmt(a.unique_tracks)} parça`, a.plays]);
  renderRanking($("statsTracks"), r.top_tracks, (t) => [t.title, t.artist, t.plays]);
  renderRanking($("statsAlbums"), r.top_albums, (a) => [a.album, a.artist, a.plays]);
}

function renderRanking(list, entries, format) {
  const max = Math.max(1, ...entries.map((entry) => format(entry)[2]));
  const bars = [];
  list.replaceChildren(
    ...entries.map((entry, index) => {
      const [name, sub, plays] = format(entry);
      const bar = h("span", { class: "rank-bar", "aria-hidden": "true" });
      bars.push([bar, plays / max]);
      return h(
        "li",
        {},
        bar,
        h("span", { class: "rank-no", text: String(index + 1) }),
        h("span", { class: "rank-name", title: `${name} — ${sub}` }, name, h("span", { class: "rank-sub", text: ` · ${sub}` })),
        h("span", { class: "num", text: fmt(plays) }),
      );
    }),
  );
  for (const [bar, ratio] of bars) animate(bar, { scaleX: ratio }, { from: { scaleX: 0 } });
}

function renderYears() {
  const years = stats.years;
  $("statsYearsBlock").hidden = years.length < 2;
  const max = Math.max(1, ...years.map((year) => year.plays));
  const chart = $("statsYears");
  const fills = [];
  chart.replaceChildren(
    ...years.map((entry) => {
      const fill = h("span", { class: "year-fill" });
      fills.push([fill, entry.plays / max]);
      return h(
        "button",
        {
          class: "year",
          type: "button",
          dataset: { year: String(entry.year) },
          title: `${entry.year}: ${fmt(entry.plays)} dinleme · ${durationText(entry.ms_played)}`,
          "aria-pressed": "false",
          onclick: () => runStats(stats.year === entry.year ? null : entry.year),
        },
        h("span", { class: "year-track" }, fill),
        h("span", { class: "year-label", text: String(entry.year) }),
      );
    }),
  );
  fills.forEach(([fill, ratio], index) =>
    animate(fill, { scaleY: ratio }, { from: { scaleY: 0 }, responseScale: 1 + index * 0.04 }),
  );
}

function updateYearSelection() {
  for (const bar of $("statsYears").children) {
    bar.setAttribute("aria-pressed", String(Number(bar.dataset.year) === stats.year));
  }
}

$("btnStatsRefresh").addEventListener("click", () => runStats(stats.year));
$("statsTop").addEventListener("change", () => runStats(stats.year));

// ————————————————————————————————————— sleeve kartı (Faz 0.5)
//
// Kartı **çekirdek** çiziyor (`sleeve::render_svg`); burada yapılan tek şey
// gelen SVG'yi göstermek. İkinci bir çizici arayüzde yaşasaydı kaydedilen
// dosya ile ekrandaki kart zamanla ayrışırdı.

const sleeve = { previewed: false, format: "square" };

const sleeveFormat = segmented($("sleeveForm").querySelector(".segmented"), (segment) => {
  sleeve.format = segment.dataset.format;
  if (sleeve.previewed) previewSleeve();
});

function sleeveArgs() {
  const yearText = $("sleeveYear").value.trim();
  return {
    year: yearText ? Number(yearText) : null,
    story: sleeve.format === "story",
  };
}

async function previewSleeve() {
  const svg = await call("sleeve_svg", sleeveArgs());
  if (svg === undefined) return;
  sleeve.previewed = true;
  const image = h("img", { alt: "sleeve kartı önizlemesi" });
  // `data:` URI — CSP `img-src 'self' data:` buna izin veriyor. SVG bir
  // resim olarak yükleniyor, belgeye karışmıyor.
  image.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
  $("sleevePreview").replaceChildren(image);
  $("sleevePreview").hidden = false;
  $("sleeveEmpty").hidden = true;
  $("btnSleeveSave").disabled = false;
  animate(image, { scale: 1, opacity: 1 }, { from: { scale: 0.97, opacity: 0 } });
}

$("sleeveForm").addEventListener("submit", (event) => {
  event.preventDefault();
  previewSleeve();
});

$("btnSleeveSave").addEventListener("click", async () => {
  const args = sleeveArgs();
  const path = await pickSavePath({
    title: "sleeve kartını kaydet",
    defaultPath: `headshell-sleeve-${args.year ?? "tum-zamanlar"}${args.story ? "-story" : ""}.png`,
    filters: [
      { name: "PNG", extensions: ["png"] },
      { name: "SVG", extensions: ["svg"] },
    ],
  });
  if (!path) return;
  const response = await call("sleeve", { ...args, out: path });
  if (!response) return;
  const written = response.written;
  toast(
    written
      ? `kart yazıldı: ${written.path} · ${fmt(written.bytes)} bayt`
      : "kart hesaplandı ama dosya yazılmadı",
    true,
  );
});

// ————————————————————————————————————— içe aktarma

// Tel değerleri (D-036); tanınmayan biçim ham adıyla gösterilir.
const EXPORT_LABELS = {
  spotify_extended: "Spotify genişletilmiş dinleme geçmişi",
  spotify_account: "Spotify hesap verisi",
  apple_music: "Apple Music",
  google_takeout: "Google Takeout",
};

// Atlama nedenleri tel değeri olarak geliyor (`not_music`, `missing_title`…).
// Tanımadığımız bir neden gelirse ham değeri gösteriyoruz — yeni bir nedeni
// sessizce yutmak, atlanan kaydı görünmez kılmak olurdu (K9).
const SKIP_LABELS = {
  not_music: "müzik değil",
  missing_title: "parça adı yok",
  missing_artist: "sanatçı adı yok",
  bad_timestamp: "zaman damgası okunmadı",
  bad_duration: "süre okunmadı",
};

// Kimlik zincirinin halkaları, K6'nın sırasıyla.
const CHAIN = [
  ["by_isrc", "ISRC", "m-isrc"],
  ["by_mbid", "MBID", "m-mbid"],
  ["by_fuzzy", "bulanık", "m-fuzzy"],
  ["by_fingerprint", "parmak izi", "m-fingerprint"],
  ["by_local_key", "yerel anahtar", "m-local"],
];

$("importForm").addEventListener("submit", (event) => {
  event.preventDefault();
  const path = $("importPath").value.trim();
  if (path) runImport(path);
});

$("btnImportPick").addEventListener("click", async () => {
  const path = await pickFile({
    title: "export arşivi seç",
    multiple: false,
    filters: [{ name: "Export arşivi", extensions: ["zip"] }],
  });
  if (path) runImport(path);
});

async function runImport(path) {
  $("importPath").value = path;
  const report = await call("import", { path });
  if (!report) return;

  // Eşleştirme gibi kısmi başarı üreten işlemler **her zaman** özet döndürür:
  // kaç kayıt geldi, kaçı ISRC ile, kaçı bulanık, kaçı eşleşmedi.
  const i = report.import;
  const d = report.identity;
  const w = report.write;
  $("importSource").textContent =
    `${i.source} · ${EXPORT_LABELS[i.export] ?? i.export} · ${fmt(i.files_matched)} dosya`;
  $("importSummary").replaceChildren(
    figure(fmt(i.records_total), "ham kayıt"),
    figure(fmt(i.listens), "dinlemeye dönüştü"),
    figure(fmt(w.inserted), "yeni yazıldı"),
    figure(fmt(w.duplicates), "zaten vardı", true),
    figure(fmt(w.new_tracks), "yeni parça"),
    figure(fmt(i.with_isrc), "ISRC taşıyan", true),
  );
  renderChain(d);

  // Atlananlar sebebiyle birlikte: "0 kayıt geldi" ile "geldi ama elendi"
  // ayrı tanılardır (K9).
  const skipped = Object.entries(i.skipped ?? {});
  const skippedLine = $("importSkipped");
  skippedLine.hidden = skipped.length === 0;
  skippedLine.textContent = `atlanan: ${skipped.map(([why, n]) => `${SKIP_LABELS[why] ?? why} ${fmt(n)}`).join(" · ")}`;

  $("importResult").textContent = JSON.stringify({ import: i, identity: d, write: w }, null, 2);
  $("importReport").hidden = false;

  // Geçmiş değişti: istatistik ve kart bir sonraki açılışta yeniden hesaplanır.
  stats.loaded = false;
  sleeve.previewed = false;
  toast(`içe aktarıldı · ${fmt(w.inserted)} yeni dinleme`, true);
}

/// Hangi halkanın kaç kaydı çözdüğü: oranlar parçanın bütüne oranı, bir
/// eşik değil. "Otoriteli" sayılanı seçmek çekirdeğin işi (`ResolveSummary`).
function renderChain(summary) {
  const total = summary.total;
  const bar = h("div", { class: "chain-bar", role: "img", "aria-label": "kimlik zinciri dağılımı" });
  const legend = h("ul", { class: "chain-legend" });
  for (const [key, label, className] of CHAIN) {
    const count = summary[key] ?? 0;
    if (count > 0) {
      const segment = h("span", { class: `chain-seg ${className}`, title: `${label}: ${fmt(count)}` });
      segment.style.flexGrow = String(count);
      bar.append(segment);
    }
    legend.append(
      h(
        "li",
        {},
        h("span", { class: `swatch-dot ${className}`, "aria-hidden": "true" }),
        `${label} `,
        h("b", { text: fmt(count) }),
      ),
    );
  }
  legend.append(h("li", {}, `toplam `, h("b", { text: fmt(total) })));
  $("importChain").replaceChildren(bar, legend);
}

// Sürükle-bırak: yol yazmak yerine arşivi pencereye bırakmak. Pencereye
// girildiği an içe aktarma bölümü açılır ki bırakılacak yer görünsün.
const dropzone = $("dropzone");
listen("tauri://drag-enter", () => {
  openPanel("import");
  dropzone.classList.add("over");
});
listen("tauri://drag-leave", () => dropzone.classList.remove("over"));
listen("tauri://drag-drop", (event) => {
  dropzone.classList.remove("over");
  const path = event.payload?.paths?.[0];
  if (!path) return;
  openPanel("import");
  runImport(path);
});

// ————————————————————————————————————— sağlayıcılar

// Yetenek bayrakları `Capabilities` bit maskesi olarak geliyor
// (çekirdekteki sabitlerle aynı sıra).
const CAPABILITIES = [
  [1 << 0, "ara"],
  [1 << 1, "gez"],
  [1 << 2, "çal"],
  [1 << 3, "kumanda"],
];

// Katalog girdisinin yetenekleri metin olarak geliyor.
const CAPABILITY_NAMES = { search: "ara", browse: "gez", stream: "çal", control: "kumanda" };

let environment = null;

async function refreshProviders() {
  renderMusicDirs();

  const list = await call("providers");
  if (list) {
    $("providerTable")
      .querySelector("tbody")
      .replaceChildren(...list.providers.map(providerRow));
    $("providerTable").hidden = list.providers.length === 0;
    $("providerEmpty").hidden = list.providers.length > 0;
  }

  const servers = await call("servers_list");
  if (!servers) return;
  $("serverTable")
    .querySelector("tbody")
    .replaceChildren(
      ...servers.servers.map((server) =>
        h(
          "tr",
          {},
          h("td", { text: server.id }),
          h("td", { class: "dim", text: server.kind }),
          h("td", { class: "mono", text: server.url }),
          h("td", { text: `${server.username} (${server.auth})` }),
          h(
            "td",
            {},
            armedButton("sil", async () => {
              const report = await call("server_remove", { name: server.id });
              if (report) {
                toast(`${report.id} silindi · kalan: ${fmt(report.remaining)}`, true);
                refreshProviders();
              }
            }),
          ),
        ),
      ),
    );
  $("serverTable").hidden = servers.servers.length === 0;
  $("serverEmpty").hidden = servers.servers.length > 0;
}

function renderMusicDirs() {
  const dirs = environment?.music_dirs ?? [];
  $("musicDirs").replaceChildren(
    ...(dirs.length > 0
      ? dirs.map((dir) => h("li", { text: dir }))
      : [h("li", { class: "empty", text: "tanımlı değil" })]),
  );
}

function providerRow(info) {
  const status = h("span", { class: "health", text: "—" });
  const caps = CAPABILITIES.filter(([bit]) => (info.capabilities & bit) !== 0).map(([, name]) =>
    h("span", { class: "tag", text: name }),
  );
  return h(
    "tr",
    {},
    h("td", {}, h("div", { text: info.display_name }), h("div", { class: "dim mono", text: info.id })),
    h("td", {}, h("div", { class: "caps" }, caps.length > 0 ? caps : h("span", { class: "dim", text: "yok" }))),
    h("td", {}, status),
    h(
      "td",
      {},
      h(
        "button",
        {
          type: "button",
          onclick: async () => {
            const report = await call("provider_test", { name: info.id });
            // "Bakmadım" ile "ulaşamadım" ayrı: hata uyarıda, sağlık cevabı burada.
            if (!report) {
              status.className = "health down";
              status.replaceChildren(h("span", { class: "dot" }), " hata (bkz. uyarı)");
              return;
            }
            const health = report.health;
            const count = health.track_count;
            status.className = health.reachable ? "health up" : "health down";
            status.replaceChildren(
              h("span", { class: "dot" }),
              health.reachable
                ? ` ayakta${count === null ? "" : ` · ${fmt(count)} parça`}`
                : ` ulaşılamıyor`,
              health.reachable
                ? null
                : h("span", { class: "health-detail", text: ` — ${health.detail ?? "sebep bildirilmedi"}` }),
            );
          },
        },
        "sına",
      ),
    ),
  );
}

$("btnScan").addEventListener("click", () => scan(false));
$("btnScanStale").addEventListener("click", () => scan(true));

async function scan(onlyStale) {
  const report = await call("provider_scan", { ifStale: onlyStale });
  if (!report) return;
  // "Değişmedi" ile "bakamadım" farklı şeyler: çekirdeğin verdiği sebep
  // olduğu gibi gösteriliyor (K9).
  toast(
    report.scanned
      ? `tarandı · ${fmt(report.summary.indexed)} parça (${fmt(report.summary.failed)} okunamadı) · eklenen ${fmt(report.write.inserted)}, güncellenen ${fmt(report.write.updated)}, düşen ${fmt(report.write.removed)} — ${report.reason}`
      : `tarama atlandı — ${report.reason}`,
    true,
  );
  refreshProviders();
}

$("serverForm").addEventListener("submit", async (event) => {
  event.preventDefault();
  const apiKey = $("serverApiKey").value.trim();
  const name = $("serverName").value.trim();
  const report = await call("server_add", {
    kind: $("serverKind").value,
    url: $("serverUrl").value.trim(),
    user: $("serverUser").value.trim(),
    password: $("serverPassword").value || null,
    apiKey: apiKey || null,
    name: name || null,
    verify: $("serverVerify").checked,
  });
  // Parola formda kalmasın — başarısız denemede de.
  $("serverPassword").value = "";
  $("serverApiKey").value = "";
  if (!report) return;
  const notes = report.notes.length > 0 ? ` · ${report.notes.join(" · ")}` : "";
  toast(`${report.server.id} eklendi${report.verified ? " (doğrulandı)" : " (doğrulanmadı)"}${notes}`, true);
  refreshProviders();
});

/// Geri alınamaz bir eylemin düğmesi: ilk basış yalnızca sorar, ikincisi
/// yapar. Tarayıcının `confirm`'ü yerine düğmenin kendisi — webview'ler onu
/// tutarlı göstermiyor ve diyalog izni yalnızca dosya seçimine açık.
function armedButton(label, action) {
  const button = h("button", { type: "button", class: "danger", text: label });
  let armed = null;
  button.addEventListener("click", async () => {
    if (!armed) {
      button.textContent = `emin misiniz? ${label}`;
      armed = setTimeout(() => {
        armed = null;
        button.textContent = label;
      }, 4000);
      return;
    }
    clearTimeout(armed);
    armed = null;
    button.textContent = label;
    await action();
  });
  return button;
}

// ————————————————————————————————————— eklentiler (Faz 2 yüzeyi)
//
// Üç şey **ayrı ayrı** gösteriliyor ve hiçbiri diğerini bastırmıyor: manifest
// sorunu, motorun kurması gereken eserler (D-055) ve onay durumu (D-040).
// Hangi düğmenin öne çıkacağı bir görünüm kararı; eklentinin çalışıp
// çalışamayacağına çekirdek karar veriyor (`PluginEntry::is_loadable`).

const CONSENT = {
  approved: ["onaylı", "chip chip-ok"],
  not_asked: ["onay bekliyor", "chip chip-warn"],
  needs_approval: ["yeni izin istiyor", "chip chip-warn"],
  disabled: ["kapalı", "chip"],
};

const pluginTabs = segmented(document.querySelector("#panel-plugins .segmented"), (tab) => {
  for (const id of ["pluginsInstalled", "pluginsCatalog", "pluginsSecrets"]) {
    $(id).hidden = tab.getAttribute("aria-controls") !== id;
  }
  if (tab.id === "tabSecrets") refreshSecrets();
});

async function refreshPlugins() {
  const list = await call("plugins");
  if (list) {
    $("pluginList").replaceChildren(...list.plugins.map(pluginCard));
    $("pluginEmpty").hidden = list.plugins.length > 0;

    const s = list.summary;
    $("pluginSummary").textContent =
      `${fmt(s.discovered)} eklenti · ${fmt(s.ready)} hazır · ${fmt(s.needs_install)} kurulum bekliyor · ` +
      `${fmt(s.awaiting_approval)} onay bekliyor · ${fmt(s.disabled)} kapalı · ` +
      `${fmt(s.incompatible)} sürüm uyumsuz · ${fmt(s.broken)} bozuk`;
    // Olmayan bir korumaya güven verilmez, var olanın sınırı da söylenir:
    // iki not listeyle birlikte duruyor ve biri her zaman görünüyor.
    $("pluginEnforce").hidden = list.permissions_enforced;
    $("pluginEnforced").hidden = !list.permissions_enforced;
    updatePluginBadge(s);
  }
  if (pluginTabs.selected().id === "tabSecrets") refreshSecrets();
}

/// Kenar çubuğunda, kullanıcıdan bir şey bekleyen eklenti sayısı: onay ya da
/// kurulum. Sayılar çekirdeğin özetinden.
function updatePluginBadge(summary) {
  const waiting = summary.awaiting_approval + summary.needs_install;
  const badge = $("navPluginBadge");
  badge.hidden = waiting === 0;
  badge.textContent = String(waiting);
  badge.title = `${waiting} eklenti sizi bekliyor`;
}

function pluginCard(entry) {
  const consent = entry.consent?.state;
  const [consentText, consentClass] = CONSENT[consent] ?? [consent, "chip"];
  const head = h(
    "div",
    { class: "plugin-head" },
    h("span", { class: "name", text: entry.display_name ?? entry.name }),
    h("span", {
      class: "author",
      text: [entry.name, entry.version, entry.api === null || entry.api === undefined ? null : `api ${entry.api}`]
        .filter(Boolean)
        .join(" · "),
    }),
    consent ? h("span", { class: consentClass, text: consentText }) : null,
  );

  const card = h("li", { class: "plugin" }, head);

  // Manifest okunamadıysa sebebi burada durur — düğmelerin çalışmama sebebi
  // görünmeden kalmasın (K9).
  if (entry.problem) card.append(h("p", { class: "plugin-problem", text: entry.problem }));

  if (consent === "needs_approval") {
    card.append(
      h("p", { class: "perms" }, h("b", { text: "yeni istediği ağ: " }), (entry.consent.extra?.net ?? []).join(", ") || "—"),
    );
  }
  const net = entry.permissions?.net ?? [];
  card.append(
    h("p", { class: "perms" }, h("b", { text: "ağ: " }), net.length > 0 ? net.join(", ") : "istemiyor"),
  );

  // Motorun indireceği eserler ayrı satırda (D-055): indirmeyi eklenti değil
  // motor yapıyor, o yüzden eklentinin izin listesine karışmıyor.
  for (const requirement of entry.requires ?? []) {
    card.append(
      h(
        "p",
        { class: "plugin-need" },
        h("b", { text: "motor: " }),
        `${requirement.name} ${requirement.version} (${requirement.platform}) — ${requirementText(requirement.state)}`,
      ),
    );
  }

  const actions = h("div", { class: "actions" });
  // Kurulum yalnızca kurulumla düzelecek bir eksik varsa: platformu
  // desteklenmeyen bir eser için "kur" boşa bir düğme olurdu.
  const installable = (entry.requires ?? []).some(
    (requirement) => requirement.state === "Missing" || requirement.state?.Corrupt,
  );
  if (consent === "not_asked" || consent === "needs_approval") {
    actions.append(pluginButton("onayla", "plugin_approve", entry.name, true));
  }
  if (installable) actions.append(pluginButton("araçları kur", "plugin_install", entry.name, true));
  if (consent === "disabled") actions.append(pluginButton("aç", "plugin_enable", entry.name, true));
  if (consent === "approved") actions.append(pluginButton("kapat", "plugin_disable", entry.name));
  actions.append(h("span", { class: "spacer" }));
  if (consent && consent !== "not_asked") {
    actions.append(pluginButton("onayı unut", "plugin_forget", entry.name));
  }
  actions.append(
    // Kaldırma geri alınamaz (dizin `state/` ile birlikte gider).
    armedButton("kaldır", async () => {
      const report = await call("plugin_remove", { name: entry.name });
      if (!report) return;
      const kept = report.kept_secrets.length ? ` · kalan sırlar: ${report.kept_secrets.join(", ")}` : "";
      toast(
        `${report.name} kaldırıldı · onay ${report.consent_forgotten ? "unutuldu" : "kaydı yoktu"}${kept}`,
        true,
      );
      refreshPlugins();
    }),
  );
  card.append(actions);
  return card;
}

/// Eser durumu dıştan etiketli geliyor: `"Missing"` ya da
/// `{ Installed: { path } }` / `{ Corrupt: { expected, found } }` /
/// `{ Unsupported: { platform, available } }`.
function requirementText(state) {
  if (state === "Missing") return "kurulmamış";
  if (typeof state === "object" && state !== null) {
    if (state.Installed) return `kurulu · ${state.Installed.path}`;
    if (state.Corrupt) {
      return `karma tutmuyor (beklenen ${short(state.Corrupt.expected)}, bulunan ${short(state.Corrupt.found)})`;
    }
    // Kurulum bunu düzeltmez; "kur" düğmesi bu yüzden gösterilmiyor.
    if (state.Unsupported) {
      return `bu platform (${state.Unsupported.platform}) için yayın yok · beyan edilenler: ${state.Unsupported.available.join(", ")}`;
    }
  }
  // Bilinmeyen bir durum sessizce "iyi" sayılmaz.
  return JSON.stringify(state);
}

function short(hash) {
  return typeof hash === "string" ? hash.slice(0, 12) : String(hash);
}

function pluginButton(label, command, name, primary = false) {
  return h(
    "button",
    {
      type: "button",
      class: primary ? "primary" : null,
      onclick: async () => {
        const report = await call(command, { name });
        if (!report) return;
        // Kurulum raporunun şekli onay raporundan farklı: ikisi de kendi
        // alanlarıyla özetleniyor, ortak bir "sonuç" tipi uydurulmuyor.
        toast(report.action ? consentText(report) : installText(name, report), true);
        refreshPlugins();
      },
    },
    label,
  );
}

// Onay komutunun adı tel değeri (D-036); tanınmayan ad ham gösterilir.
const ACTION_LABELS = {
  approve: "onaylandı",
  disable: "kapatıldı",
  enable: "açıldı",
  forget: "onay unutuldu",
};

function consentText(report) {
  const state = report.status?.state;
  return `${report.name}: ${ACTION_LABELS[report.action] ?? report.action} · ${CONSENT[state]?.[0] ?? state ?? "—"}`;
}

function installText(name, report) {
  const origin = report.fetched ? `katalogdan ${report.fetched.version} indirildi` : "araçlar";
  const tools = report.ready ? "hazır" : "eksik kaldı";
  const consent = CONSENT[report.consent?.state]?.[0] ?? report.consent?.state ?? "—";
  return `${name}: ${origin} · ${fmt(report.declared)} eser, ${tools} · ${consent}`;
}

$("btnPluginReload").addEventListener("click", refreshPlugins);

// ————————————————————————————————————— katalog (D-071)
//
// Katalog **yalnızca düğmeyle** okunur: panel açılınca ağa çıkılmaz ("bir
// export'u içe aktarmak kimseyi sessizce ağa bağlamaz"ın arayüzdeki hâli).
// Durum ve sebep çekirdekten geliyor; burada yalnızca yazılıyor.

const INSTALL = {
  not_installed: [null, null],
  current: ["kurulu · güncel", "chip chip-ok"],
  update_available: ["güncelleme var", "chip chip-info"],
  manual: ["elle kurulmuş", "chip"],
  modified: ["yerelde değiştirilmiş", "chip chip-warn"],
  unreadable: ["köken kaydı okunamadı", "chip chip-err"],
};

async function refreshCatalog() {
  const report = await call("plugin_catalog");
  if (!report) return;
  $("catalogList").replaceChildren(...report.plugins.map((plugin) => catalogCard(plugin, report.platform)));
  const s = report.summary;
  $("catalogSummary").textContent =
    `${fmt(s.listed)} eklenti · ${fmt(s.installable)} kurulabilir · ${fmt(s.installed)} kurulu · ` +
    `${fmt(s.updates)} güncelleme · ${fmt(s.problems)} kurulamaz`;
  const delisted = $("catalogDelisted");
  delisted.hidden = report.delisted.length === 0;
  delisted.textContent = report.delisted.length
    ? `Katalogdan çekilmiş ama bu makinede kurulu: ${report.delisted.join(", ")}. ` +
      "Kaldırmak için kurulu sekmesinde \"kaldır\"."
    : "";
}

function catalogCard(plugin, platform) {
  const state = plugin.installed?.state;
  const [stateText, stateClass] = plugin.problem ? ["kurulamaz", "chip chip-err"] : (INSTALL[state] ?? [state, "chip"]);
  const card = h(
    "li",
    { class: "plugin" },
    h(
      "div",
      { class: "plugin-head" },
      h("span", { class: "name", text: plugin.display_name ?? plugin.name }),
      h("span", { class: "author", text: [plugin.name, plugin.version].filter(Boolean).join(" · ") }),
      stateText ? h("span", { class: stateClass, text: stateText }) : null,
    ),
  );

  if (plugin.description) card.append(h("p", { class: "plugin-desc", text: plugin.description }));
  // Kurulamayan girdi gizlenmiyor: kullanıcı aradığını neden kuramadığını görsün (K9).
  if (plugin.problem) card.append(h("p", { class: "plugin-problem", text: plugin.problem }));
  const detail = installDetail(plugin.installed);
  if (detail) card.append(h("p", { class: "plugin-need", text: detail }));

  if (!plugin.problem) {
    const caps = (plugin.capabilities ?? []).map((cap) => CAPABILITY_NAMES[cap] ?? cap);
    if (caps.length > 0) card.append(h("p", { class: "perms" }, h("b", { text: "yapabildiği: " }), caps.join(" · ")));
    const net = plugin.permissions?.net ?? [];
    card.append(h("p", { class: "perms" }, h("b", { text: "ağ: " }), net.length > 0 ? net.join(", ") : "istemiyor"));
    for (const requirement of plugin.requires ?? []) {
      const here = requirement.assets?.[platform];
      card.append(
        h(
          "p",
          { class: "plugin-need" },
          h("b", { text: "motor: " }),
          here
            ? `${requirement.name} ${requirement.version} (${platform})`
            : `${requirement.name} ${requirement.version} — bu platform (${platform}) için yayın yok`,
        ),
      );
    }
  }

  const actions = h("div", { class: "actions" });
  if (!plugin.problem && state === "not_installed") {
    actions.append(catalogButton("kur", "plugin_install", { name: plugin.name }));
  }
  if (!plugin.problem && state === "update_available") {
    actions.append(catalogButton("güncelle", "plugin_update", { name: plugin.name }));
  }
  if (actions.childElementCount) card.append(actions);
  return card;
}

function installDetail(installed) {
  switch (installed?.state) {
    case "update_available":
      return `kurulu ${installed.installed} → katalogda ${installed.available}`;
    case "modified":
      return `kurulu ${installed.version}; elle değiştirilen dosyalar: ${installed.files.join(", ")} — güncelleme üstüne yazmaz`;
    case "manual":
      return "elle kurulmuş (köken kaydı yok) — katalog ona dokunmaz";
    case "unreadable":
      return `köken kaydı okunamadı: ${installed.detail}`;
    default:
      return null;
  }
}

function catalogButton(label, command, args) {
  return h(
    "button",
    {
      type: "button",
      class: "primary",
      onclick: async () => {
        const report = await call(command, args);
        if (!report) return;
        toast(command === "plugin_update" ? updateText(report) : installText(args.name, report), true);
        refreshCatalog();
        // Kurulan eklenti onay bekler (D-040): sonucun yaşadığı yere geçilir.
        if (command === "plugin_install") pluginTabs.select($("tabInstalled"));
        refreshPlugins();
      },
    },
    label,
  );
}

function updateText(report) {
  const s = report.summary;
  const lines = report.plugins.map((plugin) => {
    const outcome = plugin.outcome;
    let text;
    switch (outcome.state) {
      case "updated":
        text = `güncellendi ${outcome.from} → ${outcome.to}`;
        // Araç değişikliği onay istemez (D-071) ama söylenir.
        for (const change of outcome.tools_changed) {
          text += ` · araç değişti: ${change.name} ${change.from ?? "—"} → ${change.to ?? "—"}`;
        }
        if (outcome.permissions_added.net.length) {
          text += ` · yeni izin istiyor: ${outcome.permissions_added.net.join(", ")}`;
        }
        break;
      case "current":
        text = `güncel (${outcome.version})`;
        break;
      case "skipped":
        text = `atlandı — ${outcome.reason}`;
        break;
      default:
        text = `GÜNCELLENEMEDİ — ${outcome.error ?? JSON.stringify(outcome)}`;
    }
    return `${plugin.name}: ${text}`;
  });
  const total =
    `${fmt(s.checked)} eklenti: ${fmt(s.updated)} güncellendi, ${fmt(s.current)} güncel, ` +
    `${fmt(s.skipped)} atlandı, ${fmt(s.failed)} başarısız`;
  return lines.length ? `${lines.join(" · ")} (${total})` : "katalogdan kurulmuş eklenti yok";
}

$("btnCatalogLoad").addEventListener("click", refreshCatalog);
$("btnCatalogUpdateAll").addEventListener("click", async () => {
  const report = await call("plugin_update", { name: null });
  if (!report) return;
  const text = updateText(report);
  if (report.summary.failed === 0) {
    toast(text, true);
  } else {
    // Düşen bir güncelleme hata olarak kalır; aşama düşenin kendi
    // zincirinden okunuyor (K9), uydurulmuyor.
    const failed = report.plugins.find((plugin) => plugin.outcome.state === "failed");
    const stage = failed?.outcome.error?.match(/ADIM: (\S+)/)?.[1];
    toast({ stage, chain: text });
  }
  refreshPlugins();
  refreshCatalog();
});

// ————————————————————————————————————— sırlar (D-042)

async function refreshSecrets() {
  const list = await call("secrets");
  if (!list) return;
  const rows = [];
  for (const [namespace, keys] of Object.entries(list.namespaces)) {
    for (const key of keys) {
      rows.push(
        h(
          "tr",
          {},
          h("td", { class: "mono", text: namespace }),
          h("td", { class: "mono", text: key }),
          h(
            "td",
            {},
            armedButton("sil", async () => {
              const report = await call("secret_remove", { namespace, key });
              if (!report) return;
              toast(`${report.namespace}/${report.key} ${report.changed ? "silindi" : "zaten yoktu"}`, true);
              refreshSecrets();
            }),
          ),
        ),
      );
    }
  }
  $("secretTable").querySelector("tbody").replaceChildren(...rows);
  $("secretTable").hidden = rows.length === 0;
  $("secretEmpty").hidden = rows.length > 0;
}

$("secretForm").addEventListener("submit", async (event) => {
  event.preventDefault();
  const report = await call("secret_set", {
    namespace: $("secretNamespace").value.trim(),
    key: $("secretKey").value.trim(),
    value: $("secretValue").value,
  });
  // Değer formda kalmasın — başarısız denemede de.
  $("secretValue").value = "";
  if (!report) return;
  toast(`${report.namespace}/${report.key} kaydedildi`, true);
  refreshSecrets();
});

// ————————————————————————————————————— tema (§3.3)
//
// Uygulama tek satır: tema deposunun verdiği CSS'i `<style>` etiketine
// yazmak. Doğrulama, `api` sürüm kontrolü, "genişletilmiş mi" kararı ve
// önizleme renkleri Rust tarafında (`src/theme.rs`); burada yalnızca
// gösteriliyor.

function applyTheme(theme) {
  if (!theme) return;
  // `textContent` — `innerHTML` değil: tema CSS'i metin olarak konuyor,
  // içindeki `</style>` bir etiket olarak yorumlanmıyor.
  $("themeCss").textContent = theme.css ?? "";
  // Süre token'ı değişmiş olabilir: yaylar yeni temadan okusun.
  invalidateMotion();
  if (theme.problem) toast({ stage: "CONFIG_LOAD", chain: theme.problem });
}

async function refreshThemes() {
  const list = await call("themes_list");
  if (!list) return;

  $("themeDir").textContent = [`tema dizini    : ${list.dir}`, `sözleşme sürümü: api ${list.api}`].join("\n");

  // Varsayılan da bir seçenek: temadan geri dönüş yolu görünür olmalı.
  $("themeList").replaceChildren(
    themeCard({ id: null, name: "varsayılan", author: "headshell", builtin: true, preview: list.default_preview }, list.active),
    ...list.themes.map((theme) => themeCard(theme, list.active)),
  );

  $("themeRejected").replaceChildren(
    ...list.rejected.map((entry) => h("li", {}, h("span", { class: "id", text: entry.id }), ` — ${entry.reason}`)),
  );
  $("themeRejectedEmpty").hidden = list.rejected.length > 0;
}

function themeCard(theme, activeId) {
  const current = (theme.id ?? null) === (activeId ?? null);
  const card = h("li", { class: current ? "theme current" : "theme" });
  card.append(swatch(theme.preview));

  const name = h("span", { class: "name", text: theme.name });
  if (theme.author) name.append(h("span", { class: "author", text: ` · ${theme.author}` }));
  card.append(name);

  const foot = h("div", { class: "theme-foot" });
  if (theme.builtin && theme.id) foot.append(h("span", { class: "tag", text: "yerleşik" }));
  // D-038: reddetmiyoruz, işaretliyoruz. Etiket kullanıcıya "bu tema
  // sözleşmenin dışına çıkıyor, garantisi yok" diyor.
  if (theme.extended) foot.append(h("span", { class: "tag warn", text: "genişletilmiş · garantisi yok" }));
  foot.append(
    current
      ? h("button", { type: "button", disabled: true }, icon("check"), "kullanılıyor")
      : h(
          "button",
          {
            type: "button",
            onclick: async () => {
              const active = await call("theme_select", { id: theme.id ?? null });
              if (!active) return;
              applyTheme(active);
              refreshThemes();
            },
          },
          "uygula",
        ),
  );
  card.append(foot);
  return card;
}

/// Temanın kendi token'larıyla çizilmiş küçük bir pencere. Değerler ham CSS
/// metni; geçersiz bir değer örneği boş bırakır, kartı düşürmez.
function swatch(preview) {
  const box = h("div", { class: "swatch", "aria-hidden": "true" });
  const parts = [
    ["sw-side", "surface"],
    ["sw-line one", "text"],
    ["sw-line two", "text"],
    ["sw-accent", "accent"],
  ];
  if (preview?.bg) box.style.backgroundColor = preview.bg;
  for (const [className, token] of parts) {
    const part = h("span", { class: className });
    if (preview?.[token]) part.style.backgroundColor = preview[token];
    box.append(part);
  }
  return box;
}

$("btnThemeReload").addEventListener("click", refreshThemes);

// ————————————————————————————————————— tanı

async function refreshDiag() {
  // Metin çekirdeğin kendi `render()`'ı — `headshell diag`'ın bastığının
  // aynısı (`diag_text`). JSON hâli katlı duruyor.
  const text = await call("diag_text");
  const block = $("diagResult");
  block.hidden = text === undefined;
  $("btnDiagCopy").hidden = !text;
  block.textContent = text ?? "henüz çalıştırılmış bir komut yok";
  const report = await call("diag");
  $("diagRawFold").hidden = !report;
  $("diagRaw").textContent = report ? JSON.stringify(report, null, 2) : "";
}

$("btnDiag").addEventListener("click", refreshDiag);

// Tanı raporu kopyalanmak için var; elle seçtirmek onu kullanılmaz kılıyordu.
$("btnDiagCopy").addEventListener("click", () => {
  const text = $("diagResult").textContent;
  if (text) copyText(text, "tanı raporu panoya kopyalandı");
});

// Kimlik zincirinin halkaları — tel değerleri (D-036).
const METHOD_LABELS = {
  isrc: "ISRC",
  mbid: "MBID (üstveri araması)",
  fuzzy: "bulanık eşleşme",
  fingerprint: "ses parmak izi",
  local_key: "yerel anahtar — otoritesiz",
};

$("resolveForm").addEventListener("submit", async (event) => {
  event.preventDefault();
  const query = $("resolveQuery").value.trim();
  if (!query) return;
  const report = await call("resolve", { query });
  if (!report) return;
  const res = report.resolution;
  const facts = [
    ["sorgu", `${report.artist} - ${report.title}`],
    ["kimlik", res.canonical_id],
    ["yöntem", `${METHOD_LABELS[res.method] ?? res.method} · güven ${percent.format(res.confidence)}`],
    [
      "eşleşme",
      res.matched
        ? `${res.matched.artist} - ${res.matched.title} [${res.matched.mbid}]`
        : "yok (üstveri kaynağı aday döndürmedi)",
    ],
  ];
  if (res.matched?.disambiguation) facts.push(["not", res.matched.disambiguation]);
  // Beraberlik sessiz kalmamalı: seçim eşdeğerler arasından yapıldıysa
  // kullanıcı bunu görmeli, yoksa keyfi bir seçimi kesin bir cevap sanır.
  if (res.tied_candidates > 1) {
    facts.push(["belirsiz", `${fmt(res.tied_candidates)} aday aynı skoru aldı; seçim belirlenimci ama keyfi`]);
  }
  $("resolveFacts").replaceChildren(
    ...facts.flatMap(([label, value]) => [h("dt", { text: label }), h("dd", { text: value })]),
  );
  $("resolveResult").textContent = JSON.stringify(res, null, 2);
  $("resolveCard").hidden = false;
});

// ————————————————————————————————————— olaylar
//
// Olaylar yalnızca **bir şey değiştiğinde** geliyor (D-033). Aradaki
// sessizlikte pozisyon çapadan tahmin ediliyor; bu yüzden "hâlâ çalıyor"
// diye bir mesaj yok.

listen("headshell://tick", async (event) => {
  const report = event.payload;
  renderAnchor(report.anchor);
  if (report.track_changed || report.finished) {
    renderQueue(await call("queue"));
  }
  if (report.listens_recorded > 0) {
    // Geçmiş büyüdü: istatistik ve kart bir sonraki açılışta yeniden
    // hesaplanır. Ekrandaki sayılar bayat kalıp yeni dinlemeyi yok saymasın.
    stats.loaded = false;
    sleeve.previewed = false;
  }
  if (report.store_error) {
    // Kayıtlar atılmadı, elde tutuldu ve yeniden denenecek — ama kullanıcı
    // bilsin (K9).
    toast({
      stage: "LIBRARY_WRITE",
      chain: `ADIM: LIBRARY_WRITE\n  → ${report.store_error}\n  → ${report.listens_pending} dinleme elde tutuldu, sonraki turda yeniden denenecek`,
    });
  }
});

listen("headshell://error", (event) => toast(event.payload));

listen("headshell://busy", (event) => {
  const label = event.payload;
  const field = $("busy");
  field.textContent = label ?? "";
  field.hidden = !label;
});

// ————————————————————————————————————— açılış

(async function boot() {
  // Tema **ilk iş**: varsayılan renklerle bir kare çizip sonra temaya
  // atlamak, her açılışta bir yanıp sönme olurdu.
  applyTheme(await call("theme_active"));
  watchMotionPreference();
  openPanel("now", { instant: true });

  environment = (await call("environment")) ?? null;
  if (environment) {
    $("appVersion").textContent = `headshell ${environment.version}`;
    $("envBlock").textContent = [
      `sürüm       : ${environment.version}`,
      `veri dizini : ${environment.data_dir}`,
      `veritabanı  : ${environment.database}`,
      `müzik       : ${environment.music_dirs.length > 0 ? environment.music_dirs.join(", ") : "tanımlı değil (HEADSHELL_MUSIC_DIRS)"}`,
    ].join("\n");
  }
  renderQueue(await call("queue"));
  renderAnchor(await call("anchor"));

  // Kenar çubuğundaki eklenti rozeti: diskten okunur, ağa çıkılmaz.
  const plugins = await call("plugins");
  if (plugins) updatePluginBadge(plugins.summary);
})();
