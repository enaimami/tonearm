// YouTube Music sağlayıcı eklentisi (api 2, D-048 → D-069).
//
// SoundCloud eklentisiyle aynı kurallar: ses röle edilmez (K3), DRM aşan
// hiçbir şey yok (D-026). D-069'da Python'dan JS'e taşındı ve artık
// kullanıcının makinesinde **hiçbir şey kurulu olması gerekmiyor**: arama
// motorun HTTP kapısından, akış motorun kurduğu yt-dlp'den geçiyor — ve
// motor yt-dlp'nin bu platform için derlenmiş, kendi Python'unu içinde
// taşıyan ikilisini indiriyor.
//
// ## İş bölümü: arama InnerTube'dan, akış yt-dlp'den
//
// İkisi de ölçülerek seçildi (D-048):
//
// - **Arama** YouTube Music'in kendi InnerTube ucuna gidiyor. yt-dlp'nin
//   arama çıktısı yalnızca `title` + `id` veriyor — sanatçı yok, süre yok — ve
//   K6'nın bulanık eşleşme halkası bunlar olmadan çalışmaz.
// - **Akış** yt-dlp'ye gidiyor. YouTube'un imza/nsig işi bu projenin işi
//   değil; bozulduğunda yt-dlp güncellenir, biz değil.
//
// ## İki tuzak, ikisi de ölçülmüş
//
// 1. **`bestaudio` çalınamaz.** Çekirdeğin symphonia'sında opus çözücüsü ve
//    webm kabı yok; `bestaudio` opus/webm seçer. Biçim m4a'ya (AAC-LC) sabitli.
// 2. **Düz GET kısıtlanıyor.** Aynı adres düz istekte 32 KB/s, `Range:
//    bytes=0-` başlığıyla 8 MB/s veriyor — 250 kat. Başlık akış kaynağıyla
//    birlikte çekirdeğe geçiriliyor.

const INNERTUBE_URL = "https://music.youtube.com/youtubei/v1/search";
const WATCH_URL = "https://music.youtube.com/watch?v=";

// InnerTube istemci bağlamı. Anahtar gerekmiyor (ölçüldü, D-048); istemci adı
// WEB_REMIX çünkü aradığımız şey YouTube Music kataloğu, YouTube'un tamamı değil.
const INNERTUBE_CLIENT = {
  clientName: "WEB_REMIX",
  clientVersion: "1.20240101.01.00",
  hl: "en",
  gl: "US",
};
// Arama süzgeci: yalnızca şarkılar. Süzgeçsiz sorgu kanal sayfası, çalma
// listesi ve "10 saatlik loop" videosu da döndürüyor.
const SONGS_FILTER = "EgWKAQIIAWoKEAoQCRADEAQQBQ==";

const BROWSER_UA =
  "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36";

// Çözebildiğimiz en iyi ses. Sıra önemli: 140 (AAC-LC ~130 kbps) yoksa 139
// (~49 kbps) alınır; ikisi de yoksa hata döner — sessizce opus seçilmez.
const AUDIO_FORMAT = "140/139/bestaudio[ext=m4a]";

// Çağrının bütçesi 20 sn. yt-dlp imza çözerken 8-10 sn harcayabiliyor, ve
// kendi kendine yeten ikili her açılışta kendini geçici bir dizine açıyor;
// bütçe yine de çağrınınkinin altında kalmalı ki hata *bizden* çıksın ve
// sebebini söyleyebilelim (K9).
const YTDLP_TIMEOUT_MS = 16000;

const DURATION_PATTERN = /^(?:(\d+):)?(\d{1,2}):(\d{2})$/;

// --- InnerTube -------------------------------------------------------------------

/** YouTube Music'in şarkı aramasını çağırır ve ham satırları döndürür. */
function innertubeSearch(text, limit) {
  const payload = JSON.stringify({
    context: { client: { ...INNERTUBE_CLIENT } },
    query: text,
    params: SONGS_FILTER,
  });
  let res;
  try {
    res = host.http.post(INNERTUBE_URL, payload, {
      "Content-Type": "application/json",
      "User-Agent": BROWSER_UA,
      Origin: "https://music.youtube.com",
    });
  } catch (err) {
    throw new Error(`YouTube Music'e ulaşılamadı: ${err.message}`);
  }
  if (!res.ok) {
    throw new Error(
      `YouTube Music araması ${res.status} döndürdü; InnerTube yüzeyi değişmiş olabilir`,
    );
  }
  let document;
  try {
    document = JSON.parse(res.body);
  } catch (err) {
    throw new Error(`YouTube Music JSON olmayan bir cevap verdi: ${err.message}`);
  }
  return collectSongRows(document).slice(0, limit);
}

/**
 * Cevabın içinden şarkı satırlarını toplar.
 *
 * InnerTube'un ağacı derin ve haber vermeden değişiyor; bu yüzden yolu adım
 * adım yürümek yerine `musicResponsiveListItemRenderer` anahtarını
 * **arıyoruz.** Yapı bir kat değişirse arama hâlâ çalışır; anahtarın kendisi
 * kaybolursa sıfır satır döner ve çağıran bunu "yüzey değişti" diye raporlar.
 *
 * **Sıra korunur.** YouTube'un verdiği sıra alaka sırasıdır ve `limit` onu
 * baştan kesiyor. Çocuklar yığına **ters** basılıyor ki `pop()` onları belge
 * sırasında geri versin.
 */
function collectSongRows(document) {
  const rows = [];
  const stack = [document];
  while (stack.length > 0) {
    const node = stack.pop();
    if (Array.isArray(node)) {
      for (let i = node.length - 1; i >= 0; i -= 1) stack.push(node[i]);
    } else if (node !== null && typeof node === "object") {
      const row = node.musicResponsiveListItemRenderer;
      if (row !== null && typeof row === "object") {
        rows.push(row);
        continue;
      }
      const values = Object.values(node);
      for (let i = values.length - 1; i >= 0; i -= 1) stack.push(values[i]);
    }
  }
  return rows;
}

/** `flexColumns[index]` içindeki metin parçaları. */
function columnRuns(row, index) {
  const column = row.flexColumns?.[index]?.musicResponsiveListItemFlexColumnRenderer;
  return column?.text?.runs ?? [];
}

/** Satırın `videoId`'si. İki yerde durabiliyor; ikisine de bakıyoruz. */
function videoId(row) {
  const play =
    row.overlay?.musicItemThumbnailOverlayRenderer?.content?.musicPlayButtonRenderer;
  const found = play?.playNavigationEndpoint?.watchEndpoint?.videoId;
  if (found) return found;
  for (const run of columnRuns(row, 0)) {
    const id = run.navigationEndpoint?.watchEndpoint?.videoId;
    if (id) return id;
  }
  return null;
}

/** `4:11` ya da `1:02:30` → milisaniye. Tanımadıysa `null`. */
function parseDurationMs(text) {
  const matched = DURATION_PATTERN.exec(text.trim());
  if (!matched) return null;
  const [, hours, minutes, seconds] = matched;
  return (Number(hours ?? 0) * 3600 + Number(minutes) * 60 + Number(seconds)) * 1000;
}

/**
 * İkinci sütunu ` • ` ayırıcılarına göre gruplara böler.
 *
 * Gözlenen biçim: `sanatçı [• albüm] • süre`. Albüm her satırda yok, ve
 * sanatçı birden çok parça olabiliyor (`vagabond`, `,`, `shilou.`, `&`,
 * `vibe`). Bu yüzden ayırıcıyı sabit bir konum değil, metnin kendisi belirliyor.
 */
function splitMetadataRuns(runs) {
  const groups = [[]];
  for (const run of runs) {
    const text = run.text ?? "";
    if (text.trim() === "•") groups.push([]);
    else groups[groups.length - 1].push(text);
  }
  return groups.map((group) => group.join("").trim()).filter((group) => group !== "");
}

/**
 * Bir InnerTube satırını sözleşmenin parça biçimine çevirir.
 * Çeviremediğinde `null` döner — çağıran bunları **sayıp raporluyor** (K9).
 */
function rowToTrack(row) {
  const id = videoId(row);
  if (!id) return null;
  const title = columnRuns(row, 0)
    .map((run) => run.text ?? "")
    .join("")
    .trim();
  if (!title) return null;

  let groups = splitMetadataRuns(columnRuns(row, 1));
  let durationMs = null;
  if (groups.length > 0) {
    durationMs = parseDurationMs(groups[groups.length - 1]);
    if (durationMs !== null) groups = groups.slice(0, -1);
  }
  const artist = groups[0] ?? "";
  const album = groups[1] ?? "";
  // Sanatçısız bir satır bulanık eşleşmeye giremez (K6). Düşürüyoruz ama
  // sayılıyor — "Bilinmeyen sanatçı" diye uydurmaktan iyidir.
  if (!artist) return null;

  const track = { id, artist, title, duration_ms: durationMs ?? 0 };
  if (album) track.album = album;
  return track;
}

// --- yt-dlp ------------------------------------------------------------------

/**
 * yt-dlp'yi motor üzerinden çalıştırır.
 *
 * Eklenti yt-dlp'yi **aramaz, kurmaz, güncellemez** (D-049, D-055): hangi
 * sürüm, hangi platform, nereden — manifestin `requires`'ı söylüyor, motor
 * kuruyor. Kurulu değilse motorun kendi hatası ne yapılacağını söylüyor.
 */
function ytdlp(args) {
  return host.tools.run("yt-dlp", args, { timeoutMs: YTDLP_TIMEOUT_MS });
}

/** Tek bir parçanın ses biçimini yt-dlp ile çözer. */
function ytdlpJson(video) {
  const args = ["--no-warnings", "--no-playlist", "-f", AUDIO_FORMAT, "-J"];
  // YouTube veri merkezi adreslerine bot duvarı çıkarıyor (D-061) ve yt-dlp
  // çerezi yalnızca dosyadan okuyor. Motor sırrı `0600` bir geçici dosyaya
  // yazıyor ve motor kapanınca siliyor; sır yoksa `null` ve yt-dlp çerezsiz
  // çalışıyor — çerez bir gereklilik değil, bir kaçış yolu.
  const cookies = host.secrets.file("cookies");
  if (cookies !== null) args.push("--cookies", cookies);
  args.push(WATCH_URL + video);

  const run = ytdlp(args);
  if (run.code !== 0) {
    // yt-dlp'nin kendi mesajını olduğu gibi geçiriyoruz (D-048): "bir şey
    // olmadı" yerine "Sign in to confirm you're not a bot" görünsün.
    const lines = run.stderr.trim().split("\n").filter((line) => line.trim() !== "");
    throw new Error(`yt-dlp: ${lines[lines.length - 1] ?? `çıkış kodu ${run.code}`}`);
  }
  try {
    return JSON.parse(run.stdout);
  } catch (err) {
    throw new Error(`yt-dlp JSON olmayan bir cevap verdi: ${err.message}`);
  }
}

/**
 * Seçilen biçimi döndürür.
 *
 * `-f` verildiğinde yt-dlp adresi tepe düzeyde `url` olarak veriyor; birden çok
 * akış birleştirilmişse `requested_formats` altında. İkisine de bakıp sessiz
 * bir video akışına düşmediğimizi doğruluyoruz. Eleme `acodec === "none"`
 * üzerinden: alanı hiç göndermeyen bir sürümde geçerli sesi reddetmemek için.
 */
function pickAudio(document) {
  const candidates = document.requested_formats ?? [document];
  return candidates.find((c) => c.acodec !== "none" && c.url) ?? null;
}

// --- sözleşme --------------------------------------------------------------------

/**
 * İki bağımsız şeyi ayrı ayrı yoklar: arama ucu ve yt-dlp. Tek bir
 * "çalışmıyor" cevabı hangisinin bozulduğunu gizlerdi (K9).
 */
export function health() {
  const notes = [];
  let reachable = true;

  try {
    const rows = innertubeSearch("a", 1);
    notes.push(`InnerTube araması ${rows.length} satır döndürdü`);
  } catch (err) {
    reachable = false;
    notes.push(`arama: ${err.message}`);
  }

  try {
    const run = ytdlp(["--version"]);
    if (run.code === 0) {
      notes.push(`yt-dlp ${run.stdout.trim()} (motor, ${host.platform})`);
    } else {
      reachable = false;
      notes.push(`yt-dlp --version çıkış kodu ${run.code}`);
    }
  } catch (err) {
    reachable = false;
    notes.push(`yt-dlp: ${err.message}`);
  }

  // Katalog boyutu sorguya göre değişir; "bilmiyorum" yanlış bir sayıdan iyidir.
  return { reachable, track_count: null, detail: notes.join("; ") };
}

export function search(text, limit) {
  const q = String(text ?? "").trim();
  if (q === "") return [];
  const size = Math.max(1, Math.min(Number(limit) || 20, 100));

  const rows = innertubeSearch(q, size);
  if (rows.length === 0) {
    // Sıfır satır iki şey olabilir: sonuç yok ya da yüzey değişti.
    // Ayırmıyoruz ama sessiz de kalmıyoruz.
    host.log.info(`'${q}' için şarkı satırı gelmedi`);
    return [];
  }

  const tracks = [];
  let skipped = 0;
  for (const row of rows) {
    const track = rowToTrack(row);
    if (track === null) skipped += 1;
    else tracks.push(track);
  }
  // K9: düşürülen kayıt sayılır ve raporlanır, sessizce yutulmaz.
  if (skipped > 0) {
    host.log.warn(`aramada ${skipped} satır çevrilemedi (kimlik/sanatçı/başlık eksik)`);
  }
  return tracks;
}

export function resolve_source(id) {
  const video = String(id ?? "").trim();
  if (video === "") throw new Error("parça kimliği boş");

  const chosen = pickAudio(ytdlpJson(video));
  if (chosen === null) {
    // "Çalamıyorum" ile "yok" farklı tanılardır (K9).
    throw new Error(
      `yt-dlp bu parça için çözebildiğimiz bir ses biçimi vermedi (istenen: ${AUDIO_FORMAT})`,
    );
  }
  return {
    kind: "http_stream",
    url: chosen.url,
    // Ölçüldü (D-048): bu başlık olmadan aynı adres 32 KB/s, onunla 8 MB/s
    // veriyor. Şifre çözme değil — sunucunun 206 ile cevapladığı standart
    // bir menzil isteği.
    headers: [{ name: "Range", value: "bytes=0-" }],
  };
}
