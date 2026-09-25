// headshell — masaüstü arayüzü (PLAN §3.2).
//
// **Altın Kural burada da geçerli.** Bu dosya karar vermez: tuşu komuta
// çevirir, çekirdeğin döndürdüğü veriyi çizer. "Duraklat mı sürdür mü",
// "sırada ne var", "hangi çalma sayılır" sorularının cevabı çekirdekte.
//
// Tek istisna bilerek konmuş: **pozisyon tahmini** (`anchor.js`). Formülün
// ikinci kopyası olduğu biliniyor ve doğruluk kümesiyle kilitli (D-033).
//
// Çekirdeğin metin üreten yardımcıları (`status_text`, `describe`) burada
// **tekrarlanmıyor**. CLI tek satıra sığdırmak için aralarında öncelik
// seçiyor; arayüzün böyle bir sıkışıklığı yok, üçünü de yan yana gösteriyor.
// Seçim yapmayan kopya kaymaz.
//
// Tanımlayıcılar İngilizce (D-036); kullanıcıya görünen metin Türkçe.

import { positionAt, clock } from "./anchor.js";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);

// ————————————————————————————————————— durum
//
// Burada tutulan **her şey çekirdekten geldi**. Kendi başına bir doğruluk
// kaynağı değil, son cevabın kopyası: kuyruk, çapa, meşguliyet.
let anchor = null;
let queue = { items: [], position: 0, repeat: "off", shuffle: false };
// Son arama sonucu — "hepsini kuyruğa al" düğmesi sorguyu tekrar yazdırmasın
// diye tutuluyor. Sonuçların kendisi değil, sorgunun kopyası.
let lastSearchQuery = null;

// Tekrar kipinin kullanıcıya gösterilen adı (D-036). Anahtarlar tel
// değerleridir; bilinmeyen bir kip gelirse ham değer gösterilir — sessizce
// "kapalı" demek, yanlış durumu doğru gibi göstermek olurdu (K9).
const REPEAT_LABELS = { off: "kapalı", all: "tümü", one: "tek" };

// ————————————————————————————————————— hata ve bildirim

function toast(err, info = false) {
  const box = document.createElement("div");
  box.className = info ? "toast info" : "toast";

  if (info) {
    box.textContent = err;
  } else {
    // Aşama ayrı gösteriliyor: "nerede kırıldı" sorusu ilk bakışta
    // cevaplanmalı (K9). Tam zincir katlanmış duruyor ki panel dolmasın
    // ama kopyalanabilsin.
    const stage = document.createElement("span");
    stage.className = "stage";
    stage.textContent = `ADIM: ${err.stage ?? "BİLİNMİYOR"}`;
    const summary = document.createElement("div");
    summary.textContent = firstLine(err.chain);
    const details = document.createElement("details");
    const label = document.createElement("summary");
    label.textContent = "tam zincir";
    const block = document.createElement("pre");
    block.textContent = err.chain ?? String(err);
    details.append(label, block);
    box.append(stage, summary, details);
  }

  // Kapatma düğmesi: bir hata 20 saniye ekranda durur ve o sırada altındaki
  // düğmeyi kapatır. Kendiliğinden gitmesini beklemek zorunda kalmasın.
  const close = document.createElement("button");
  close.className = "close";
  close.type = "button";
  close.textContent = "×";
  close.title = "kapat";
  close.setAttribute("aria-label", "uyarıyı kapat");
  close.addEventListener("click", () => box.remove());
  box.append(close);

  $("toasts").append(box);
  setTimeout(() => box.remove(), info ? 6000 : 20000);
}

function firstLine(chain) {
  if (!chain) return "bilinmeyen hata";
  const lines = chain.split("\n").filter((s) => s.trim() && !s.startsWith("ADIM:"));
  return lines[0]?.trim() ?? chain;
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

// ————————————————————————————————————— dosya diyaloğu
//
// Eklentinin JS sarmalayıcısı npm'den gelir; bu arayüzde bundler yok, o
// yüzden komut doğrudan çağrılıyor. İzin `capabilities/default.json`'da
// `open`/`save` ile sınırlı: diyalog bir **yol** döndürür, o yolu okuyup
// yazan taraf çekirdektir.

async function pickFile(options) {
  const picked = await call("plugin:dialog|open", { options });
  return filePath(picked);
}

async function pickSavePath(options) {
  const picked = await call("plugin:dialog|save", { options });
  return filePath(picked);
}

/// Diyalog masaüstünde düz yol döndürür; mobil `content://` biçimini
/// nesne olarak verir. İkisini de kabul etmek bir varsayımı kaldırıyor.
function filePath(picked) {
  if (!picked) return null;
  if (typeof picked === "string") return picked;
  return picked.path ?? null;
}

// ————————————————————————————————————— sekmeler

const PANELS = ["now", "library", "stats", "import", "providers", "plugins", "theme", "diag"];

for (const button of document.querySelectorAll(".nav")) {
  button.addEventListener("click", () => openPanel(button.dataset.panel));
}

function openPanel(name) {
  for (const button of document.querySelectorAll(".nav")) {
    button.setAttribute("aria-current", String(button.dataset.panel === name));
  }
  for (const section of document.querySelectorAll(".panel")) {
    section.hidden = section.id !== `panel-${name}`;
  }
  if (name === "providers") refreshProviders();
  if (name === "theme") refreshThemes();
  if (name === "plugins") refreshPlugins();
}

// "başlarken" kartındaki kısayollar.
for (const button of document.querySelectorAll(".go")) {
  button.addEventListener("click", () => {
    const target = button.dataset.go;
    if (target === "focus-play") {
      $("playQuery").focus();
      return;
    }
    openPanel(target);
  });
}

// ————————————————————————————————————— oynatıcı çubuğu

function renderQueue(view) {
  if (!view) return;
  queue = view;

  const list = $("queue");
  list.replaceChildren();
  for (const [index, item] of view.items.entries()) {
    const row = document.createElement("li");
    if (index === view.position) row.classList.add("current");
    const number = document.createElement("span");
    number.className = "index";
    number.textContent = String(index + 1);
    const name = document.createElement("span");
    name.textContent = trackName(item.track);
    row.append(number, name);
    // Kuyrukta atlama kararı çekirdekte: burada yalnızca indeks iletiliyor.
    row.addEventListener("click", async () => renderQueue(await call("jump_to", { index })));
    list.append(row);
  }
  // Kuyruk boşken yerini "başlarken" kartı alıyor: boş bir liste kullanıcıya
  // ne yapacağını söylemez.
  $("onboarding").hidden = view.items.length > 0;

  $("btnShuffle").classList.toggle("on", view.shuffle);
  const repeat = $("btnRepeat");
  repeat.classList.toggle("on", view.repeat !== "off");
  repeat.textContent = view.repeat === "one" ? "🔂" : "🔁";
  // Tel değeri (`off`/`all`/`one`) İngilizce kalır — JSON anahtarıdır.
  // Kullanıcının okuduğu metin Türkçedir (D-036).
  repeat.title = `tekrar: ${REPEAT_LABELS[view.repeat] ?? view.repeat}`;

  const current = view.items[view.position];
  $("nowTitle").textContent = current ? trackName(current.track) : "—";
}

function trackName(track) {
  // `TrackRef::display_name`'in karşılığı. Tek satır olduğu için burada
  // duruyor; büyüdüğü an çekirdeğe taşınmalı.
  return track.artist ? `${track.artist} — ${track.title}` : track.title;
}

const STATE_MARK = { playing: "▶", paused: "⏸", buffering: "⋯", stopped: "■" };

function renderAnchor(next) {
  if (!next) return;
  anchor = next;
  const mark = $("nowState");
  mark.textContent = STATE_MARK[next.state] ?? "■";
  mark.className = `state ${next.state}`;
  draw();
}

// Çizim döngüsü: pozisyon **çekirdeğe sorulmuyor**, çapadan tahmin ediliyor
// (D-015). IPC duraksarsa çubuk yürümeye devam eder.
function draw() {
  if (!anchor) return;
  const position = positionAt(anchor, Date.now());
  const duration = anchor.duration_ms ?? 0;
  const ratio = duration > 0 ? Math.min(position / duration, 1) : 0;
  $("barFill").style.transform = `scaleX(${ratio})`;
  $("timeLabel").textContent = `${clock(position)} / ${duration > 0 ? clock(duration) : "—:—"}`;
}

function drawLoop() {
  draw();
  requestAnimationFrame(drawLoop);
}

async function togglePause() {
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
$("btnStop").addEventListener("click", async () => {
  renderAnchor(await call("stop"));
  renderQueue(await call("queue"));
});
$("btnNext").addEventListener("click", playNext);
$("btnPrev").addEventListener("click", playPrevious);
$("btnShuffle").addEventListener("click", async () =>
  renderQueue(await call("set_shuffle", { on: !queue.shuffle })),
);
$("btnRepeat").addEventListener("click", async () => {
  const nextMode = { off: "all", all: "one", one: "off" }[queue.repeat] ?? "off";
  renderQueue(await call("set_repeat", { mode: nextMode }));
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
  if (event.key === "Escape") {
    $("helpSheet").hidden = true;
    $("toasts").replaceChildren();
    return;
  }
  // Ctrl+1…8: bölümler. Yazı kutusundayken de çalışır — bir sayı tuşuna
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
    $("helpSheet").hidden = false;
  }
});

$("btnHelp").addEventListener("click", () => {
  $("helpSheet").hidden = false;
});
$("btnHelpClose").addEventListener("click", () => {
  $("helpSheet").hidden = true;
});
$("helpSheet").addEventListener("click", (event) => {
  // Kutunun dışına tıklamak kapatır; içine tıklamak kapatmaz.
  if (event.target === $("helpSheet")) $("helpSheet").hidden = true;
});

// ————————————————————————————————————— çal

$("playForm").addEventListener("submit", async (event) => {
  event.preventDefault();
  const query = $("playQuery").value.trim();
  if (!query) return;
  const view = await call("play", {
    query,
    all: $("playAll").checked,
    shuffle: false,
  });
  if (!view) return;
  renderQueue(view);
  renderAnchor(await call("anchor"));
  openPanel("now");
});

// ————————————————————————————————————— kütüphane araması

$("searchForm").addEventListener("submit", async (event) => {
  event.preventDefault();
  const query = $("searchQuery").value.trim();
  if (!query) return;
  const report = await call("search", { query, limit: 50, minMs: null });
  if (!report) return;
  lastSearchQuery = report.query;

  const table = $("searchResult");
  const tbody = table.querySelector("tbody");
  tbody.replaceChildren();
  for (const hit of report.hits) {
    const row = document.createElement("tr");
    row.append(
      cell(hit.title),
      cell(hit.artist),
      cell(String(hit.play_count), "num"),
      cellButton("çal", () => startPlay(`${hit.artist} ${hit.title}`, false)),
    );
    tbody.append(row);
  }
  table.hidden = report.hits.length === 0;
  $("btnSearchPlayAll").hidden = report.hits.length === 0;
  $("searchEmpty").hidden = report.hits.length > 0;
  $("searchEmpty").textContent = `"${report.query}" için kayıt bulunamadı.`;
});

// Kuyruğu **çekirdek** kuruyor: burada sonuç satırları toplanıp gönderilmiyor,
// aynı sorgu `all` bayrağıyla tekrar veriliyor. Eşleştirmenin ikinci bir
// kopyası arayüzde yaşamasın.
$("btnSearchPlayAll").addEventListener("click", () => {
  if (lastSearchQuery) startPlay(lastSearchQuery, true);
});

async function startPlay(query, all) {
  const view = await call("play", { query, all, shuffle: false });
  if (!view) return;
  renderQueue(view);
  renderAnchor(await call("anchor"));
  openPanel("now");
}

function cell(text, className) {
  const td = document.createElement("td");
  td.textContent = text;
  if (className) td.className = className;
  return td;
}

function cellButton(label, handler) {
  const td = document.createElement("td");
  const button = document.createElement("button");
  button.textContent = label;
  button.addEventListener("click", handler);
  td.append(button);
  return td;
}

// ————————————————————————————————————— istatistik

$("statsForm").addEventListener("submit", async (event) => {
  event.preventDefault();
  const yearText = $("statsYear").value.trim();
  const response = await call("stats", {
    year: yearText ? Number(yearText) : null,
    top: Number($("statsTop").value) || 10,
    minMs: null,
  });
  if (!response) return;
  const r = response.report;

  $("statsSummary").replaceChildren(
    figure(String(r.plays), "sayılan dinleme"),
    figure(hoursText(r.total_ms_played), "toplam süre"),
    figure(String(r.unique_artists), "sanatçı"),
    figure(String(r.unique_tracks), "parça"),
    // Kısmi başarı **her zaman** raporlanır: kaç kayıt eşiğin altında kaldı,
    // kaçı kanonik kimliksiz. Sessizce yutulmuyor.
    figure(String(r.skipped_short), "eşiğin altında"),
    figure(String(r.without_canonical_id), "kanonik kimliksiz"),
  );
  // Sıfır dinleme bir hata değil ama boş bir tablo da cevap değil: ne
  // yapılacağı yazıyor.
  $("statsEmpty").hidden = r.plays > 0;

  renderRanking($("statsArtists"), r.top_artists, (a) => [a.artist, `${a.plays}`]);
  renderRanking($("statsTracks"), r.top_tracks, (t) => [`${t.artist} — ${t.title}`, `${t.plays}`]);
});

function figure(big, label) {
  const wrap = document.createElement("div");
  const b = document.createElement("span");
  b.className = "big";
  b.textContent = big;
  const l = document.createElement("span");
  l.className = "label";
  l.textContent = label;
  wrap.append(b, l);
  return wrap;
}

// Atlama nedenleri tel değeri olarak geliyor (`not_music`, `missing_title`…).
// Kullanıcının okuduğu metin Türkçe (D-036); tanımadığımız bir neden gelirse
// ham değeri gösteriyoruz — yeni bir nedeni sessizce yutmak, atlanan kaydı
// görünmez kılmak olurdu (K9).
const SKIP_LABELS = {
  not_music: "müzik değil",
  missing_title: "parça adı yok",
  missing_artist: "sanatçı adı yok",
  bad_timestamp: "zaman damgası okunmadı",
  bad_duration: "süre okunmadı",
};

function hoursText(ms) {
  return `${(ms / 3600000).toFixed(1)} sa`;
}

function renderRanking(list, entries, format) {
  list.replaceChildren();
  for (const entry of entries) {
    const [name, count] = format(entry);
    const row = document.createElement("li");
    const c = document.createElement("span");
    c.className = "num";
    c.textContent = ` · ${count}`;
    row.append(document.createTextNode(name), c);
    list.append(row);
  }
}

// ————————————————————————————————————— sleeve kartı (Faz 0.5)
//
// Kartı **çekirdek** çiziyor (`sleeve::render_svg`); burada yapılan tek şey
// gelen SVG'yi göstermek. İkinci bir çizici arayüzde yaşasaydı kaydedilen
// dosya ile ekrandaki kart zamanla ayrışırdı.

function sleeveArgs() {
  const yearText = $("sleeveYear").value.trim();
  return {
    year: yearText ? Number(yearText) : null,
    story: $("sleeveFormat").value === "story",
  };
}

$("sleeveForm").addEventListener("submit", async (event) => {
  event.preventDefault();
  const svg = await call("sleeve_svg", sleeveArgs());
  if (svg === undefined) return;
  const box = $("sleevePreview");
  const image = document.createElement("img");
  // `data:` URI — CSP `img-src 'self' data:` buna izin veriyor. SVG bir
  // resim olarak yükleniyor, belgeye karışmıyor.
  image.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
  image.alt = "sleeve kartı önizlemesi";
  box.replaceChildren(image);
  box.hidden = false;
  $("btnSleeveSave").disabled = false;
});

$("btnSleeveSave").addEventListener("click", async () => {
  const story = $("sleeveFormat").value === "story";
  const path = await pickSavePath({
    title: "sleeve kartını kaydet",
    defaultPath: `headshell-sleeve${story ? "-story" : ""}.png`,
    filters: [
      { name: "PNG", extensions: ["png"] },
      { name: "SVG", extensions: ["svg"] },
    ],
  });
  if (!path) return;
  const response = await call("sleeve", { ...sleeveArgs(), out: path });
  if (!response) return;
  const written = response.written;
  toast(
    written
      ? `kart yazıldı: ${written.path} · ${written.bytes} bayt`
      : "kart hesaplandı ama dosya yazılmadı",
    true,
  );
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

async function refreshProviders() {
  const list = await call("providers");
  if (list) {
    const tbody = $("providerTable").querySelector("tbody");
    tbody.replaceChildren();
    for (const info of list.providers) {
      const row = document.createElement("tr");
      const status = cell("—");
      row.append(
        cell(`${info.display_name} (${info.id})`),
        cell(capabilityText(info.capabilities)),
        status,
        cellButton("sına", async () => {
          const report = await call("provider_test", { name: info.id });
          if (!report) {
            status.textContent = "hata (bkz. uyarı)";
            return;
          }
          const count = report.health.track_count;
          status.textContent = report.health.reachable
            ? `ayakta${count === null ? "" : ` · ${count} parça`}`
            : `ulaşılamıyor — ${report.health.detail ?? "sebep bildirilmedi"}`;
        }),
      );
      tbody.append(row);
    }
    $("providerTable").hidden = list.providers.length === 0;
    $("providerEmpty").hidden = list.providers.length > 0;
  }

  const servers = await call("servers_list");
  if (!servers) return;
  const tbody = $("serverTable").querySelector("tbody");
  tbody.replaceChildren();
  for (const server of servers.servers) {
    const row = document.createElement("tr");
    row.append(
      cell(server.id),
      cell(server.kind),
      cell(server.url),
      cell(`${server.username} (${server.auth})`),
      cellButton("sil", async () => {
        const report = await call("server_remove", { name: server.id });
        if (report) {
          toast(`${report.id} silindi · kalan: ${report.remaining}`, true);
          refreshProviders();
        }
      }),
    );
    tbody.append(row);
  }
  $("serverTable").hidden = servers.servers.length === 0;
  $("serverEmpty").hidden = servers.servers.length > 0;
}

function capabilityText(bits) {
  const names = CAPABILITIES.filter(([bit]) => (bits & bit) !== 0).map(([, name]) => name);
  return names.length > 0 ? names.join(", ") : "yok";
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
      ? `tarandı · ${report.summary.indexed} parça (${report.summary.failed} okunamadı) · eklenen ${report.write.inserted}, güncellenen ${report.write.updated}, düşen ${report.write.removed} — ${report.reason}`
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
  if (!report) return;
  // Parola formda kalmasın.
  $("serverPassword").value = "";
  $("serverApiKey").value = "";
  const notes = report.notes.length > 0 ? ` · ${report.notes.join(" · ")}` : "";
  toast(
    `${report.server.id} eklendi${report.verified ? " (doğrulandı)" : " (doğrulanmadı)"}${notes}`,
    true,
  );
  refreshProviders();
});

// ————————————————————————————————————— eklentiler (Faz 2 yüzeyi)
//
// Bu panel Faz 3'te yoktu: arayüz yazıldığında Faz 2 ertelenmişti (D-027) ve
// kabuk eklentilerden habersiz kaldı. Kullanıcı SoundCloud'u ya da YouTube
// Music'i açmak için CLI'ye düşüyordu.
//
// Üç şey **ayrı ayrı** gösteriliyor ve hiçbiri diğerini bastırmıyor: manifest
// sorunu, motorun kurması gereken eserler (D-055) ve onay durumu (D-040).
// CLI tek satıra sığdırmak için aralarında öncelik seçiyor; burada seçmeye
// gerek yok, o yüzden seçim mantığının ikinci bir kopyası da yok.

const CONSENT_LABELS = {
  approved: "onaylı",
  not_asked: "onay sorulmadı",
  needs_approval: "yeni izin istiyor",
  disabled: "kapalı",
};

async function refreshPlugins() {
  const list = await call("plugins");
  if (list) {
    const box = $("pluginList");
    box.replaceChildren();
    for (const entry of list.plugins) box.append(pluginRow(entry));
    $("pluginEmpty").hidden = list.plugins.length > 0;

    const s = list.summary;
    $("pluginSummary").textContent =
      `${s.discovered} eklenti · ${s.ready} hazır · ${s.needs_install} kurulum bekliyor · ` +
      `${s.awaiting_approval} onay bekliyor · ${s.disabled} kapalı · ` +
      `${s.incompatible} sürüm uyumsuz · ${s.broken} bozuk`;
    // Olmayan bir korumaya güven verilmez, var olanın sınırı da söylenir:
    // iki not listeyle birlikte duruyor ve biri her zaman görünüyor.
    $("pluginEnforce").hidden = list.permissions_enforced;
    $("pluginEnforced").hidden = !list.permissions_enforced;
  }
  refreshSecrets();
}

function pluginRow(entry) {
  const row = document.createElement("li");
  row.className = "plugin";

  const head = document.createElement("div");
  head.className = "plugin-head";
  const name = document.createElement("span");
  name.className = "name";
  name.textContent = entry.display_name ?? entry.name;
  const id = document.createElement("span");
  id.className = "author";
  const version = entry.version ? ` ${entry.version}` : "";
  const api = entry.api === null || entry.api === undefined ? "" : ` · api ${entry.api}`;
  id.textContent = ` ${entry.name}${version}${api}`;
  head.append(name, id);

  const state = entry.consent?.state;
  if (state) head.append(tag(CONSENT_LABELS[state] ?? state, state !== "approved"));
  row.append(head);

  // Manifest okunamadıysa sebebi burada durur — düğmelerin çalışmama sebebi
  // görünmeden kalmasın (K9).
  if (entry.problem) {
    const problem = document.createElement("p");
    problem.className = "plugin-problem";
    problem.textContent = entry.problem;
    row.append(problem);
  }

  if (entry.permissions && !isEmptyPermissions(entry.permissions)) {
    row.append(permissionLine("istediği ağ", entry.permissions.net));
  }

  // Motorun indireceği eserler ayrı satırda (D-055): indirmeyi eklenti değil
  // motor yapıyor, o yüzden eklentinin izin listesine karışmıyor.
  for (const requirement of entry.requires ?? []) {
    const line = document.createElement("p");
    line.className = "plugin-need";
    line.textContent =
      `motor: ${requirement.name} ${requirement.version} (${requirement.platform}) — ` +
      requirementText(requirement.state);
    row.append(line);
  }

  const actions = document.createElement("div");
  actions.className = "actions";
  actions.append(
    pluginButton("onayla", "plugin_approve", entry.name),
    pluginButton("araçları kur", "plugin_install", entry.name),
    state === "disabled"
      ? pluginButton("aç", "plugin_enable", entry.name)
      : pluginButton("kapat", "plugin_disable", entry.name),
    pluginButton("onayı unut", "plugin_forget", entry.name),
    removeButton(entry.name),
  );
  row.append(actions);
  return row;
}

/// Kaldırma geri alınamaz (dizin `state/` ile birlikte gider): ilk tıklama
/// yalnızca soruyor, ikincisi kaldırıyor. Tarayıcının `confirm`'ü yerine
/// düğmenin kendisi — webview'ler onu tutarlı göstermiyor ve diyalog izni
/// yalnızca dosya seçimine açık.
function removeButton(name) {
  const button = document.createElement("button");
  button.textContent = "kaldır";
  let armed = null;
  button.addEventListener("click", async () => {
    if (!armed) {
      button.textContent = "emin misiniz? kaldır";
      armed = setTimeout(() => {
        armed = null;
        button.textContent = "kaldır";
      }, 4000);
      return;
    }
    clearTimeout(armed);
    armed = null;
    const report = await call("plugin_remove", { name });
    if (!report) return;
    const kept = report.kept_secrets.length
      ? ` · kalan sırlar: ${report.kept_secrets.join(", ")}`
      : "";
    toast(
      `${report.name} kaldırıldı · onay ${report.consent_forgotten ? "unutuldu" : "kaydı yoktu"}${kept}`,
      true,
    );
    refreshPlugins();
  });
  return button;
}

function isEmptyPermissions(permissions) {
  return (permissions.net?.length ?? 0) === 0;
}

function permissionLine(label, values) {
  const line = document.createElement("p");
  line.className = "perms";
  line.textContent = `${label}: ${values?.length ? values.join(", ") : "—"}`;
  return line;
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
    // Kurulum bunu düzeltmez; "kur" düğmesine basmak boşa olur.
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

function pluginButton(label, command, name) {
  const button = document.createElement("button");
  button.textContent = label;
  button.addEventListener("click", async () => {
    const report = await call(command, { name });
    if (!report) return;
    // Kurulum raporunun şekli onay raporundan farklı: ikisi de kendi
    // alanlarıyla özetleniyor, ortak bir "sonuç" tipi uydurulmuyor.
    toast(report.action ? consentText(report) : installText(name, report), true);
    refreshPlugins();
  });
  return button;
}

function consentText(report) {
  return `${report.name}: ${report.action} · durum ${report.status?.state ?? "—"}`;
}

function installText(name, report) {
  const origin = report.fetched ? `katalogdan ${report.fetched.version} indirildi` : "araçlar";
  const tools = report.ready ? "hazır" : "eksik kaldı";
  const consent = CONSENT_LABELS[report.consent?.state] ?? report.consent?.state ?? "—";
  return `${name}: ${origin} · ${report.declared} eser, ${tools} · onay: ${consent}`;
}

// ————————————————————————————————————— katalog (D-071)
//
// Katalog **yalnızca düğmeyle** okunur: panel açılınca ağa çıkılmaz ("bir
// export'u içe aktarmak kimseyi sessizce ağa bağlamaz"ın arayüzdeki hâli).
// Durum ve sebep çekirdekten geliyor; burada yalnızca yazılıyor.

const INSTALL_LABELS = {
  not_installed: "kurulu değil",
  current: "kurulu · güncel",
  update_available: "güncelleme var",
  manual: "elle kurulmuş",
  modified: "yerelde değiştirilmiş",
  unreadable: "köken kaydı okunamadı",
};

async function refreshCatalog() {
  const report = await call("plugin_catalog");
  if (!report) return;
  const box = $("catalogList");
  box.replaceChildren();
  for (const plugin of report.plugins) box.append(catalogRow(plugin, report.platform));
  const s = report.summary;
  $("catalogSummary").textContent =
    `${s.listed} eklenti · ${s.installable} kurulabilir · ${s.installed} kurulu · ` +
    `${s.updates} güncelleme · ${s.problems} kurulamaz`;
  const delisted = $("catalogDelisted");
  delisted.hidden = report.delisted.length === 0;
  delisted.textContent = report.delisted.length
    ? `Katalogdan çekilmiş ama bu makinede kurulu: ${report.delisted.join(", ")}. ` +
      "Kaldırmak için yukarıdaki listede \"kaldır\"."
    : "";
}

function catalogRow(plugin, platform) {
  const row = document.createElement("li");
  row.className = "plugin";

  const head = document.createElement("div");
  head.className = "plugin-head";
  const name = document.createElement("span");
  name.className = "name";
  name.textContent = plugin.display_name ?? plugin.name;
  const id = document.createElement("span");
  id.className = "author";
  id.textContent = ` ${plugin.name}${plugin.version ? ` ${plugin.version}` : ""}`;
  head.append(name, id);
  const state = plugin.installed?.state;
  if (plugin.problem) {
    head.append(tag("kurulamaz", true));
  } else if (state) {
    const calm = state === "not_installed" || state === "current";
    head.append(tag(INSTALL_LABELS[state] ?? state, !calm));
  }
  row.append(head);

  if (plugin.description) {
    const description = document.createElement("p");
    description.className = "hint";
    description.textContent = plugin.description;
    row.append(description);
  }
  // Kurulamayan girdi gizlenmiyor: kullanıcı aradığını neden kuramadığını görsün (K9).
  if (plugin.problem) {
    const problem = document.createElement("p");
    problem.className = "plugin-problem";
    problem.textContent = plugin.problem;
    row.append(problem);
  }
  const detail = installDetail(plugin.installed);
  if (detail) {
    const line = document.createElement("p");
    line.className = "plugin-need";
    line.textContent = detail;
    row.append(line);
  }
  if (!plugin.problem) {
    row.append(permissionLine("istediği ağ", plugin.permissions?.net));
    for (const requirement of plugin.requires ?? []) {
      const line = document.createElement("p");
      line.className = "plugin-need";
      const here = requirement.assets?.[platform];
      line.textContent = here
        ? `motor: ${requirement.name} ${requirement.version} (${platform})`
        : `motor: ${requirement.name} ${requirement.version} — bu platform (${platform}) için yayın yok`;
      row.append(line);
    }
  }

  const actions = document.createElement("div");
  actions.className = "actions";
  if (!plugin.problem && state === "not_installed") {
    actions.append(catalogButton("kur", "plugin_install", { name: plugin.name }));
  }
  if (!plugin.problem && state === "update_available") {
    actions.append(catalogButton("güncelle", "plugin_update", { name: plugin.name }));
  }
  if (actions.childElementCount) row.append(actions);
  return row;
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
  const button = document.createElement("button");
  button.textContent = label;
  button.addEventListener("click", async () => {
    const report = await call(command, args);
    if (!report) return;
    toast(command === "plugin_update" ? updateText(report) : installText(args.name, report), true);
    refreshPlugins();
    refreshCatalog();
  });
  return button;
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
    `${s.checked} eklenti: ${s.updated} güncellendi, ${s.current} güncel, ` +
    `${s.skipped} atlandı, ${s.failed} başarısız`;
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
  const tbody = $("secretTable").querySelector("tbody");
  tbody.replaceChildren();
  let count = 0;
  for (const [namespace, keys] of Object.entries(list.namespaces)) {
    for (const key of keys) {
      count += 1;
      const row = document.createElement("tr");
      row.append(
        cell(namespace),
        cell(key),
        cellButton("sil", async () => {
          const report = await call("secret_remove", { namespace, key });
          if (!report) return;
          toast(`${report.namespace}/${report.key} ${report.changed ? "silindi" : "zaten yoktu"}`, true);
          refreshSecrets();
        }),
      );
      tbody.append(row);
    }
  }
  $("secretTable").hidden = count === 0;
  $("secretEmpty").hidden = count > 0;
}

$("secretForm").addEventListener("submit", async (event) => {
  event.preventDefault();
  const report = await call("secret_set", {
    namespace: $("secretNamespace").value.trim(),
    key: $("secretKey").value.trim(),
    value: $("secretValue").value,
  });
  if (!report) return;
  // Değer formda kalmasın.
  $("secretValue").value = "";
  toast(`${report.namespace}/${report.key} kaydedildi`, true);
  refreshSecrets();
});

$("btnPluginReload").addEventListener("click", refreshPlugins);

// ————————————————————————————————————— tema (§3.3)
//
// Uygulama tek satır: çekirdeğin — burada tema deposunun — verdiği CSS'i
// `<style>` etiketine yazmak. Doğrulama, `api` sürüm kontrolü ve "genişletilmiş
// mi" kararı Rust tarafında (`src/theme.rs`); burada yalnızca gösteriliyor.

function applyTheme(theme) {
  if (!theme) return;
  // `textContent` — `innerHTML` değil: tema CSS'i metin olarak konuyor,
  // içindeki `</style>` bir etiket olarak yorumlanmıyor.
  $("themeCss").textContent = theme.css ?? "";
  if (theme.problem) toast({ stage: "CONFIG_LOAD", chain: theme.problem });
}

async function refreshThemes() {
  const list = await call("themes_list");
  if (!list) return;

  $("themeDir").textContent = [
    `tema dizini    : ${list.dir}`,
    `sözleşme sürümü: api ${list.api}`,
  ].join("\n");

  const box = $("themeList");
  box.replaceChildren();
  // Varsayılan da bir seçenek: temadan geri dönüş yolu görünür olmalı.
  box.append(themeRow({ id: null, name: "varsayılan", builtin: true }, list.active));
  for (const theme of list.themes) box.append(themeRow(theme, list.active));

  const rejected = $("themeRejected");
  rejected.replaceChildren();
  for (const entry of list.rejected) {
    const row = document.createElement("li");
    const id = document.createElement("span");
    id.className = "id";
    id.textContent = entry.id;
    row.append(id, document.createTextNode(` — ${entry.reason}`));
    rejected.append(row);
  }
  $("themeRejectedEmpty").hidden = list.rejected.length > 0;
}

function themeRow(theme, activeId) {
  const row = document.createElement("li");
  row.className = "theme";
  if ((theme.id ?? null) === (activeId ?? null)) row.classList.add("current");

  const name = document.createElement("span");
  name.className = "name";
  name.textContent = theme.name;
  if (theme.author) {
    const author = document.createElement("span");
    author.className = "author";
    author.textContent = ` · ${theme.author}`;
    name.append(author);
  }
  row.append(name);

  if (theme.builtin && theme.id) row.append(tag("yerleşik"));
  // D-038: reddetmiyoruz, işaretliyoruz. Etiket kullanıcıya "bu tema
  // sözleşmenin dışına çıkıyor, garantisi yok" diyor.
  if (theme.extended) row.append(tag("genişletilmiş · garantisi yok", true));

  const button = document.createElement("button");
  button.textContent = "uygula";
  button.addEventListener("click", async () => {
    const active = await call("theme_select", { id: theme.id ?? null });
    if (!active) return;
    applyTheme(active);
    refreshThemes();
  });
  row.append(button);
  return row;
}

function tag(text, warn = false) {
  const span = document.createElement("span");
  span.className = warn ? "tag warn" : "tag";
  span.textContent = text;
  return span;
}

$("btnThemeReload").addEventListener("click", refreshThemes);

// ————————————————————————————————————— içe aktarma, çözümleme, tanı

$("importForm").addEventListener("submit", async (event) => {
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
  // kaç kayıt geldi, kaçı ISRC ile, kaçı bulanık, kaçı eşleşmedi. Eskiden
  // burası ham JSON basıyordu; sayıların hepsi oradaydı ama kimse okumuyordu.
  const i = report.import;
  const d = report.identity;
  const w = report.write;
  $("importSummary").replaceChildren(
    figure(String(i.records_total), "ham kayıt"),
    figure(String(i.listens), "dinlemeye dönüştü"),
    figure(String(w.inserted), "yeni yazıldı"),
    figure(String(w.duplicates), "zaten vardı"),
    figure(String(w.new_tracks), "yeni parça"),
    figure(`${d.by_isrc}/${d.by_mbid}/${d.by_fuzzy}`, "ISRC / MBID / bulanık"),
  );
  $("importSummary").hidden = false;

  // Atlananlar sebebiyle birlikte: "0 kayıt geldi" ile "geldi ama elendi"
  // ayrı tanılardır (K9).
  const skipped = Object.entries(i.skipped ?? {});
  const skippedLine = $("importSkipped");
  if (skipped.length > 0) {
    const detail = skipped.map(([why, n]) => `${SKIP_LABELS[why] ?? why} ${n}`).join(" · ");
    skippedLine.textContent = `atlanan: ${detail}`;
    skippedLine.hidden = false;
  } else {
    skippedLine.hidden = true;
  }

  $("importResult").textContent = JSON.stringify(
    { import: i, identity: d, write: w },
    null,
    2,
  );
  $("importRaw").hidden = false;
}

// Sürükle-bırak: yol yazmak yerine arşivi pencereye bırakmak.
const dropzone = $("dropzone");
listen("tauri://drag-enter", () => dropzone.classList.add("over"));
listen("tauri://drag-leave", () => dropzone.classList.remove("over"));
listen("tauri://drag-drop", (event) => {
  dropzone.classList.remove("over");
  const path = event.payload?.paths?.[0];
  if (!path) return;
  openPanel("import");
  runImport(path);
});

$("resolveForm").addEventListener("submit", async (event) => {
  event.preventDefault();
  const query = $("resolveQuery").value.trim();
  if (!query) return;
  const report = await call("resolve", { query });
  if (!report) return;
  const block = $("resolveResult");
  block.textContent = JSON.stringify(report.resolution, null, 2);
  block.hidden = false;
});

$("btnDiag").addEventListener("click", async () => {
  const report = await call("diag");
  const block = $("diagResult");
  block.textContent =
    report === undefined
      ? ""
      : report === null
        ? "henüz çalıştırılmış bir komut yok"
        : JSON.stringify(report, null, 2);
  block.hidden = report === undefined;
  $("btnDiagCopy").hidden = report === undefined;
});

// Tanı raporu kopyalanmak için var; elle seçtirmek onu kullanılmaz kılıyordu.
$("btnDiagCopy").addEventListener("click", async () => {
  const text = $("diagResult").textContent;
  if (!text) return;
  try {
    await navigator.clipboard.writeText(text);
    toast("tanı raporu panoya kopyalandı", true);
  } catch (err) {
    // Pano reddedilirse sessiz kalmıyoruz: kullanıcı kopyalandığını sanıp
    // boş bir hata raporu gönderirdi.
    toast({ stage: "CONFIG_LOAD", chain: `pano yazılamadı: ${err}` });
  }
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
  if (report.store_error) {
    // Kayıtlar atılmadı, elde tutuldu ve yeniden denenecek — ama kullanıcı
    // bilsin (K9).
    toast({
      stage: "LIBRARY_WRITE",
      chain: `${report.store_error}\n  → ${report.listens_pending} dinleme elde tutuldu, sonraki turda yeniden denenecek`,
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

  const env = await call("environment");
  if (env) {
    $("envBlock").textContent = [
      `sürüm       : ${env.version}`,
      `veri dizini : ${env.data_dir}`,
      `veritabanı  : ${env.database}`,
      `müzik       : ${env.music_dirs.length > 0 ? env.music_dirs.join(", ") : "tanımlı değil (HEADSHELL_MUSIC_DIRS)"}`,
    ].join("\n");
  }
  renderQueue(await call("queue"));
  renderAnchor(await call("anchor"));
  requestAnimationFrame(drawLoop);
})();
