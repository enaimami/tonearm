// tune — masaüstü arayüzü (PLAN §3.2).
//
// **Altın Kural burada da geçerli.** Bu dosya karar vermez: tuşu komuta
// çevirir, çekirdeğin döndürdüğü veriyi çizer. "Duraklat mı sürdür mü",
// "sırada ne var", "hangi çalma sayılır" sorularının cevabı çekirdekte.
//
// Tek istisna bilerek konmuş: **pozisyon tahmini** (`anchor.js`). Formülün
// ikinci kopyası olduğu biliniyor ve doğruluk kümesiyle kilitli (D-033).
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

// ————————————————————————————————————— sekmeler

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
  $("queueEmpty").hidden = view.items.length > 0;

  $("btnShuffle").classList.toggle("on", view.shuffle);
  const repeat = $("btnRepeat");
  repeat.classList.toggle("on", view.repeat !== "off");
  repeat.textContent = view.repeat === "one" ? "🔂" : "🔁";
  repeat.title = `tekrar: ${view.repeat}`;

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

$("btnPlayPause").addEventListener("click", async () => renderAnchor(await call("toggle_pause")));
$("btnStop").addEventListener("click", async () => {
  renderAnchor(await call("stop"));
  renderQueue(await call("queue"));
});
$("btnNext").addEventListener("click", async () => {
  renderQueue(await call("next"));
  renderAnchor(await call("anchor"));
});
$("btnPrev").addEventListener("click", async () => {
  renderQueue(await call("previous"));
  renderAnchor(await call("anchor"));
});
$("btnShuffle").addEventListener("click", async () =>
  renderQueue(await call("set_shuffle", { on: !queue.shuffle })),
);
$("btnRepeat").addEventListener("click", async () => {
  const nextMode = { off: "all", all: "one", one: "off" }[queue.repeat] ?? "off";
  renderQueue(await call("set_repeat", { mode: nextMode }));
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

  const table = $("searchResult");
  const tbody = table.querySelector("tbody");
  tbody.replaceChildren();
  for (const hit of report.hits) {
    const row = document.createElement("tr");
    row.append(
      cell(hit.title),
      cell(hit.artist),
      cell(String(hit.play_count), "num"),
      cellButton("çal", async () => {
        const view = await call("play", {
          query: `${hit.artist} ${hit.title}`,
          all: false,
          shuffle: false,
        });
        if (!view) return;
        renderQueue(view);
        renderAnchor(await call("anchor"));
        openPanel("now");
      }),
    );
    tbody.append(row);
  }
  table.hidden = report.hits.length === 0;
  $("searchEmpty").hidden = report.hits.length > 0;
  $("searchEmpty").textContent = `"${report.query}" için kayıt bulunamadı.`;
});

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

// ————————————————————————————————————— içe aktarma, çözümleme, tanı

$("importForm").addEventListener("submit", async (event) => {
  event.preventDefault();
  const path = $("importPath").value.trim();
  if (path) runImport(path);
});

async function runImport(path) {
  $("importPath").value = path;
  const report = await call("import", { path });
  if (!report) return;
  const block = $("importResult");
  // Eşleştirme gibi kısmi başarı üreten işlemler **her zaman** özet döndürür:
  // kaç kayıt geldi, kaçı ISRC ile, kaçı bulanık, kaçı eşleşmedi.
  block.textContent = JSON.stringify(
    { import: report.import, identity: report.identity, write: report.write },
    null,
    2,
  );
  block.hidden = false;
}

// Sürükle-bırak: yol yazmak yerine arşivi pencereye bırakmak.
const dropzone = $("dropzone");
listen("tauri://drag-enter", () => dropzone.classList.add("over"));
listen("tauri://drag-leave", () => dropzone.classList.remove("over"));
listen("tauri://drag-drop", (event) => {
  dropzone.classList.remove("over");
  const path = event.payload?.paths?.[0];
  if (!path) return;
  openPanel("diag");
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
});

// ————————————————————————————————————— olaylar
//
// Olaylar yalnızca **bir şey değiştiğinde** geliyor (D-033). Aradaki
// sessizlikte pozisyon çapadan tahmin ediliyor; bu yüzden "hâlâ çalıyor"
// diye bir mesaj yok.

listen("tune://tick", async (event) => {
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

listen("tune://error", (event) => toast(event.payload));

listen("tune://busy", (event) => {
  const label = event.payload;
  const field = $("busy");
  field.textContent = label ?? "";
  field.hidden = !label;
});

// ————————————————————————————————————— açılış

(async function boot() {
  const env = await call("environment");
  if (env) {
    $("envBlock").textContent = [
      `sürüm       : ${env.version}`,
      `veri dizini : ${env.data_dir}`,
      `veritabanı  : ${env.database}`,
      `müzik       : ${env.music_dirs.length > 0 ? env.music_dirs.join(", ") : "tanımlı değil (TUNE_MUSIC_DIRS)"}`,
    ].join("\n");
  }
  renderQueue(await call("queue"));
  renderAnchor(await call("anchor"));
  requestAnimationFrame(drawLoop);
})();
