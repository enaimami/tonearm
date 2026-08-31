// tune — masaüstü arayüzü (PLAN §3.2).
//
// **Altın Kural burada da geçerli.** Bu dosya karar vermez: tuşu komuta
// çevirir, çekirdeğin döndürdüğü veriyi çizer. "Duraklat mı sürdür mü",
// "sırada ne var", "hangi çalma sayılır" sorularının cevabı çekirdekte.
//
// Tek istisna bilerek konmuş: **pozisyon tahmini** (`anchor.js`). Formülün
// ikinci kopyası olduğu biliniyor ve doğruluk kümesiyle kilitli (D-033).

import { positionAt, saat } from "./anchor.js";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);

// ————————————————————————————————————— durum
//
// Burada tutulan **her şey çekirdekten geldi**. Kendi başına bir doğruluk
// kaynağı değil, son cevabın kopyası: kuyruk, çapa, meşguliyet.
let capa = null;
let kuyruk = { items: [], position: 0, repeat: "off", shuffle: false };

// ————————————————————————————————————— hata ve bildirim

function uyar(hata, bilgi = false) {
  const kutu = document.createElement("div");
  kutu.className = bilgi ? "uyari bilgi" : "uyari";

  if (bilgi) {
    kutu.textContent = hata;
  } else {
    // Aşama ayrı gösteriliyor: "nerede kırıldı" sorusu ilk bakışta
    // cevaplanmalı (K9). Tam zincir katlanmış duruyor ki panel dolmasın
    // ama kopyalanabilsin.
    const adim = document.createElement("span");
    adim.className = "adim";
    adim.textContent = `ADIM: ${hata.stage ?? "BİLİNMİYOR"}`;
    const ozet = document.createElement("div");
    ozet.textContent = ilkSatir(hata.chain);
    const ayrinti = document.createElement("details");
    const baslik = document.createElement("summary");
    baslik.textContent = "tam zincir";
    const blok = document.createElement("pre");
    blok.textContent = hata.chain ?? String(hata);
    ayrinti.append(baslik, blok);
    kutu.append(adim, ozet, ayrinti);
  }

  $("uyarilar").append(kutu);
  setTimeout(() => kutu.remove(), bilgi ? 6000 : 20000);
}

function ilkSatir(zincir) {
  if (!zincir) return "bilinmeyen hata";
  const satirlar = zincir.split("\n").filter((s) => s.trim() && !s.startsWith("ADIM:"));
  return satirlar[0]?.trim() ?? zincir;
}

/// Komutu çağırır; hata olursa gösterir ve `undefined` döner.
///
/// Yutmuyor: her başarısızlık ekranda aşamasıyla görünüyor. `undefined`
/// dönmesi çağıranın çizimi atlaması için — sessiz boş sonuç değil.
async function cagir(komut, argumanlar = {}) {
  try {
    return await invoke(komut, argumanlar);
  } catch (hata) {
    uyar(hata);
    return undefined;
  }
}

// ————————————————————————————————————— sekmeler

for (const dugme of document.querySelectorAll(".nav")) {
  dugme.addEventListener("click", () => sekmeAc(dugme.dataset.sekme));
}

function sekmeAc(ad) {
  for (const dugme of document.querySelectorAll(".nav")) {
    dugme.setAttribute("aria-current", String(dugme.dataset.sekme === ad));
  }
  for (const bolum of document.querySelectorAll(".sekme")) {
    bolum.hidden = bolum.id !== `sekme-${ad}`;
  }
  if (ad === "saglayici") saglayicilariYenile();
}

// ————————————————————————————————————— oynatıcı çubuğu

function kuyrugaYaz(gorunum) {
  if (!gorunum) return;
  kuyruk = gorunum;

  const liste = $("kuyruk");
  liste.replaceChildren();
  for (const [sira, oge] of gorunum.items.entries()) {
    const satir = document.createElement("li");
    if (sira === gorunum.position) satir.classList.add("calan");
    const no = document.createElement("span");
    no.className = "no";
    no.textContent = String(sira + 1);
    const ad = document.createElement("span");
    ad.textContent = parcaAdi(oge.track);
    satir.append(no, ad);
    // Kuyrukta atlama kararı çekirdekte: burada yalnızca indeks iletiliyor.
    satir.addEventListener("click", async () => kuyrugaYaz(await cagir("jump_to", { index: sira })));
    liste.append(satir);
  }
  $("kuyrukBos").hidden = gorunum.items.length > 0;

  $("btnKaristir").classList.toggle("acik", gorunum.shuffle);
  const tekrar = $("btnTekrar");
  tekrar.classList.toggle("acik", gorunum.repeat !== "off");
  tekrar.textContent = gorunum.repeat === "one" ? "🔂" : "🔁";
  tekrar.title = `tekrar: ${gorunum.repeat}`;

  const calan = gorunum.items[gorunum.position];
  $("simdiAd").textContent = calan ? parcaAdi(calan.track) : "—";
}

function parcaAdi(track) {
  // `TrackRef::display_name`'in karşılığı. Tek satır olduğu için burada
  // duruyor; büyüdüğü an çekirdeğe taşınmalı.
  return track.artist ? `${track.artist} — ${track.title}` : track.title;
}

const DURUM_ISARETI = { playing: "▶", paused: "⏸", buffering: "⋯", stopped: "■" };

function capayiYaz(yeni) {
  if (!yeni) return;
  capa = yeni;
  const isaret = $("simdiDurum");
  isaret.textContent = DURUM_ISARETI[yeni.state] ?? "■";
  isaret.className = `durum ${yeni.state}`;
  ciz();
}

// Çizim döngüsü: pozisyon **çekirdeğe sorulmuyor**, çapadan tahmin ediliyor
// (D-015). IPC duraksarsa çubuk yürümeye devam eder.
function ciz() {
  if (!capa) return;
  const pozisyon = positionAt(capa, Date.now());
  const sure = capa.duration_ms ?? 0;
  const oran = sure > 0 ? Math.min(pozisyon / sure, 1) : 0;
  $("cubukDolu").style.transform = `scaleX(${oran})`;
  $("sayac").textContent = `${saat(pozisyon)} / ${sure > 0 ? saat(sure) : "—:—"}`;
}

function cizimDongusu() {
  ciz();
  requestAnimationFrame(cizimDongusu);
}

$("btnDurdurOynat").addEventListener("click", async () => capayiYaz(await cagir("toggle_pause")));
$("btnDur").addEventListener("click", async () => {
  capayiYaz(await cagir("stop"));
  kuyrugaYaz(await cagir("queue"));
});
$("btnSonraki").addEventListener("click", async () => {
  kuyrugaYaz(await cagir("next"));
  capayiYaz(await cagir("anchor"));
});
$("btnOnceki").addEventListener("click", async () => {
  kuyrugaYaz(await cagir("previous"));
  capayiYaz(await cagir("anchor"));
});
$("btnKaristir").addEventListener("click", async () =>
  kuyrugaYaz(await cagir("set_shuffle", { on: !kuyruk.shuffle })),
);
$("btnTekrar").addEventListener("click", async () => {
  const sonraki = { off: "all", all: "one", one: "off" }[kuyruk.repeat] ?? "off";
  kuyrugaYaz(await cagir("set_repeat", { mode: sonraki }));
});

// ————————————————————————————————————— çal

$("calForm").addEventListener("submit", async (olay) => {
  olay.preventDefault();
  const sorgu = $("calSorgu").value.trim();
  if (!sorgu) return;
  const gorunum = await cagir("play", {
    query: sorgu,
    all: $("calTumu").checked,
    shuffle: false,
  });
  if (!gorunum) return;
  kuyrugaYaz(gorunum);
  capayiYaz(await cagir("anchor"));
  sekmeAc("calan");
});

// ————————————————————————————————————— kütüphane araması

$("araForm").addEventListener("submit", async (olay) => {
  olay.preventDefault();
  const sorgu = $("araSorgu").value.trim();
  if (!sorgu) return;
  const rapor = await cagir("search", { query: sorgu, limit: 50, minMs: null });
  if (!rapor) return;

  const tablo = $("araSonuc");
  const govde = tablo.querySelector("tbody");
  govde.replaceChildren();
  for (const vurus of rapor.hits) {
    const satir = document.createElement("tr");
    satir.append(
      hucre(vurus.title),
      hucre(vurus.artist),
      hucre(String(vurus.play_count), "sayi"),
      hucreDugme("çal", async () => {
        const gorunum = await cagir("play", {
          query: `${vurus.artist} ${vurus.title}`,
          all: false,
          shuffle: false,
        });
        if (!gorunum) return;
        kuyrugaYaz(gorunum);
        capayiYaz(await cagir("anchor"));
        sekmeAc("calan");
      }),
    );
    govde.append(satir);
  }
  tablo.hidden = rapor.hits.length === 0;
  $("araBos").hidden = rapor.hits.length > 0;
  $("araBos").textContent = `"${rapor.query}" için kayıt bulunamadı.`;
});

function hucre(metin, sinif) {
  const td = document.createElement("td");
  td.textContent = metin;
  if (sinif) td.className = sinif;
  return td;
}

function hucreDugme(etiket, islev) {
  const td = document.createElement("td");
  const dugme = document.createElement("button");
  dugme.textContent = etiket;
  dugme.addEventListener("click", islev);
  td.append(dugme);
  return td;
}

// ————————————————————————————————————— istatistik

$("istForm").addEventListener("submit", async (olay) => {
  olay.preventDefault();
  const yilMetni = $("istYil").value.trim();
  const cevap = await cagir("stats", {
    year: yilMetni ? Number(yilMetni) : null,
    top: Number($("istTop").value) || 10,
    minMs: null,
  });
  if (!cevap) return;
  const r = cevap.report;

  $("istOzet").replaceChildren(
    kutu(String(r.plays), "sayılan dinleme"),
    kutu(saatMetni(r.total_ms_played), "toplam süre"),
    kutu(String(r.unique_artists), "sanatçı"),
    kutu(String(r.unique_tracks), "parça"),
    // Kısmi başarı **her zaman** raporlanır: kaç kayıt eşiğin altında kaldı,
    // kaçı kanonik kimliksiz. Sessizce yutulmuyor.
    kutu(String(r.skipped_short), "eşiğin altında"),
    kutu(String(r.without_canonical_id), "kanonik kimliksiz"),
  );

  siralamaYaz($("istSanatci"), r.top_artists, (a) => [a.artist, `${a.plays}`]);
  siralamaYaz($("istParca"), r.top_tracks, (t) => [`${t.artist} — ${t.title}`, `${t.plays}`]);
});

function kutu(buyuk, etiket) {
  const dis = document.createElement("div");
  const b = document.createElement("span");
  b.className = "buyuk";
  b.textContent = buyuk;
  const e = document.createElement("span");
  e.className = "etiket";
  e.textContent = etiket;
  dis.append(b, e);
  return dis;
}

function saatMetni(ms) {
  return `${(ms / 3600000).toFixed(1)} sa`;
}

function siralamaYaz(liste, kayitlar, bicim) {
  liste.replaceChildren();
  for (const kayit of kayitlar) {
    const [ad, sayi] = bicim(kayit);
    const satir = document.createElement("li");
    const s = document.createElement("span");
    s.className = "sayi";
    s.textContent = ` · ${sayi}`;
    satir.append(document.createTextNode(ad), s);
    liste.append(satir);
  }
}

// ————————————————————————————————————— sağlayıcılar

// Yetenek bayrakları `Capabilities` bit maskesi olarak geliyor
// (çekirdekteki sabitlerle aynı sıra).
const YETENEKLER = [
  [1 << 0, "ara"],
  [1 << 1, "gez"],
  [1 << 2, "çal"],
  [1 << 3, "kumanda"],
];

async function saglayicilariYenile() {
  const liste = await cagir("providers");
  if (liste) {
    const govde = $("saglayiciTablo").querySelector("tbody");
    govde.replaceChildren();
    for (const bilgi of liste.providers) {
      const satir = document.createElement("tr");
      const durum = hucre("—");
      satir.append(
        hucre(`${bilgi.display_name} (${bilgi.id})`),
        hucre(yetenekMetni(bilgi.capabilities)),
        durum,
        hucreDugme("sına", async () => {
          const rapor = await cagir("provider_test", { name: bilgi.id });
          if (!rapor) {
            durum.textContent = "hata (bkz. uyarı)";
            return;
          }
          const sayi = rapor.health.track_count;
          durum.textContent = rapor.health.reachable
            ? `ayakta${sayi === null ? "" : ` · ${sayi} parça`}`
            : `ulaşılamıyor — ${rapor.health.detail ?? "sebep bildirilmedi"}`;
        }),
      );
      govde.append(satir);
    }
  }

  const sunucular = await cagir("servers_list");
  if (!sunucular) return;
  const govde = $("sunucuTablo").querySelector("tbody");
  govde.replaceChildren();
  for (const sunucu of sunucular.servers) {
    const satir = document.createElement("tr");
    satir.append(
      hucre(sunucu.id),
      hucre(sunucu.kind),
      hucre(sunucu.url),
      hucre(`${sunucu.username} (${sunucu.auth})`),
      hucreDugme("sil", async () => {
        const rapor = await cagir("server_remove", { name: sunucu.id });
        if (rapor) {
          uyar(`${rapor.id} silindi · kalan: ${rapor.remaining}`, true);
          saglayicilariYenile();
        }
      }),
    );
    govde.append(satir);
  }
}

function yetenekMetni(bitler) {
  const adlar = YETENEKLER.filter(([bit]) => (bitler & bit) !== 0).map(([, ad]) => ad);
  return adlar.length > 0 ? adlar.join(", ") : "yok";
}

$("btnTara").addEventListener("click", () => tara(false));
$("btnTaraBayat").addEventListener("click", () => tara(true));

async function tara(yalnizcaBayat) {
  const rapor = await cagir("provider_scan", { ifStale: yalnizcaBayat });
  if (!rapor) return;
  // "Değişmedi" ile "bakamadım" farklı şeyler: çekirdeğin verdiği sebep
  // olduğu gibi gösteriliyor (K9).
  uyar(
    rapor.scanned
      ? `tarandı · ${rapor.summary.indexed} parça (${rapor.summary.failed} okunamadı) · eklenen ${rapor.write.inserted}, güncellenen ${rapor.write.updated}, düşen ${rapor.write.removed} — ${rapor.reason}`
      : `tarama atlandı — ${rapor.reason}`,
    true,
  );
  saglayicilariYenile();
}

$("sunucuForm").addEventListener("submit", async (olay) => {
  olay.preventDefault();
  const anahtar = $("sunucuAnahtar").value.trim();
  const ad = $("sunucuAd").value.trim();
  const rapor = await cagir("server_add", {
    kind: $("sunucuTur").value,
    url: $("sunucuUrl").value.trim(),
    user: $("sunucuKullanici").value.trim(),
    password: $("sunucuParola").value || null,
    apiKey: anahtar || null,
    name: ad || null,
    verify: $("sunucuDogrula").checked,
  });
  if (!rapor) return;
  // Parola formda kalmasın.
  $("sunucuParola").value = "";
  $("sunucuAnahtar").value = "";
  const notlar = rapor.notes.length > 0 ? ` · ${rapor.notes.join(" · ")}` : "";
  uyar(
    `${rapor.server.id} eklendi${rapor.verified ? " (doğrulandı)" : " (doğrulanmadı)"}${notlar}`,
    true,
  );
  saglayicilariYenile();
});

// ————————————————————————————————————— içe aktarma, çözümleme, tanı

$("ictForm").addEventListener("submit", async (olay) => {
  olay.preventDefault();
  const yol = $("ictYol").value.trim();
  if (yol) iceAktar(yol);
});

async function iceAktar(yol) {
  $("ictYol").value = yol;
  const rapor = await cagir("import", { path: yol });
  if (!rapor) return;
  const blok = $("ictSonuc");
  // Eşleştirme gibi kısmi başarı üreten işlemler **her zaman** özet döndürür:
  // kaç kayıt geldi, kaçı ISRC ile, kaçı bulanık, kaçı eşleşmedi.
  blok.textContent = JSON.stringify(
    { import: rapor.import, identity: rapor.identity, write: rapor.write },
    null,
    2,
  );
  blok.hidden = false;
}

// Sürükle-bırak: yol yazmak yerine arşivi pencereye bırakmak.
const birakAlani = $("birak");
listen("tauri://drag-enter", () => birakAlani.classList.add("uzerinde"));
listen("tauri://drag-leave", () => birakAlani.classList.remove("uzerinde"));
listen("tauri://drag-drop", (olay) => {
  birakAlani.classList.remove("uzerinde");
  const yol = olay.payload?.paths?.[0];
  if (!yol) return;
  sekmeAc("tani");
  iceAktar(yol);
});

$("cozForm").addEventListener("submit", async (olay) => {
  olay.preventDefault();
  const sorgu = $("cozSorgu").value.trim();
  if (!sorgu) return;
  const rapor = await cagir("resolve", { query: sorgu });
  if (!rapor) return;
  const blok = $("cozSonuc");
  blok.textContent = JSON.stringify(rapor.resolution, null, 2);
  blok.hidden = false;
});

$("btnTani").addEventListener("click", async () => {
  const rapor = await cagir("diag");
  const blok = $("taniSonuc");
  blok.textContent =
    rapor === undefined
      ? ""
      : rapor === null
        ? "henüz çalıştırılmış bir komut yok"
        : JSON.stringify(rapor, null, 2);
  blok.hidden = rapor === undefined;
});

// ————————————————————————————————————— olaylar
//
// Olaylar yalnızca **bir şey değiştiğinde** geliyor (D-033). Aradaki
// sessizlikte pozisyon çapadan tahmin ediliyor; bu yüzden "hâlâ çalıyor"
// diye bir mesaj yok.

listen("tune://tick", async (olay) => {
  const rapor = olay.payload;
  capayiYaz(rapor.anchor);
  if (rapor.track_changed || rapor.finished) {
    kuyrugaYaz(await cagir("queue"));
  }
  if (rapor.store_error) {
    // Kayıtlar atılmadı, elde tutuldu ve yeniden denenecek — ama kullanıcı
    // bilsin (K9).
    uyar({
      stage: "LIBRARY_WRITE",
      chain: `${rapor.store_error}\n  → ${rapor.listens_pending} dinleme elde tutuldu, sonraki turda yeniden denenecek`,
    });
  }
});

listen("tune://error", (olay) => uyar(olay.payload));

listen("tune://busy", (olay) => {
  const etiket = olay.payload;
  const alan = $("mesgul");
  alan.textContent = etiket ?? "";
  alan.hidden = !etiket;
});

// ————————————————————————————————————— açılış

(async function baslat() {
  const ortam = await cagir("environment");
  if (ortam) {
    $("ortam").textContent = [
      `sürüm       : ${ortam.version}`,
      `veri dizini : ${ortam.data_dir}`,
      `veritabanı  : ${ortam.database}`,
      `müzik       : ${ortam.music_dirs.length > 0 ? ortam.music_dirs.join(", ") : "tanımlı değil (TUNE_MUSIC_DIRS)"}`,
    ].join("\n");
  }
  kuyrugaYaz(await cagir("queue"));
  capayiYaz(await cagir("anchor"));
  requestAnimationFrame(cizimDongusu);
})();
