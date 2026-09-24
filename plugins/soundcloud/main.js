// SoundCloud sağlayıcı eklentisi (api 2, D-069).
//
// Faz 2 §2.2'nin referans eklentisi; D-069'da Python'dan JS'e taşındı. İşi
// katalog sunmak değil, eklenti sözleşmesinin çekirdeğin dışında yazılabilir
// olduğunu kanıtlamak: burada Rust yok, yalnızca motorun verdiği `host`
// nesnesi var. Kullanıcının makinesinde hiçbir şey kurulu olması gerekmiyor.
//
// Kapsam bilerek dar — `search` + `stream`. Ses röle edilmez (K3): çözülen
// adres çekirdeğe verilir, akışı o çeker. DRM aşan hiçbir şey yok (D-026):
// SoundCloud'un kendi `progressive` MP3 transcoding'i imzalı bir adres
// döndürüyor, biz onu olduğu gibi geçiriyoruz.
//
// Ölçüm (2026-09-01, 200 parçalık örnek): parçaların %99'unda `progressive`
// varyant var, %1'i yalnızca HLS sunuyor. HLS çözücü yazmadık; o %1 için
// açık hata dönüyoruz — sessizce boş sonuç değil (K9).

const VERSION = "0.2.0";
const HOME_URL = "https://soundcloud.com/";
const API_BASE = "https://api-v2.soundcloud.com";
const HEADERS = { "User-Agent": `headshell-soundcloud/${VERSION}` };

// Keşfin toplam bütçesi. Aşılırsa "bulamadım" deriz — çağrının kendi süresi
// (20 sn) dolmadan, sebebini söyleyebilecekken.
const DISCOVERY_BUDGET_MS = 15000;

const ASSET_PATTERN = /https:\/\/a-v2\.sndcdn\.com\/assets\/[^"']+\.js/g;
const CLIENT_ID_PATTERN = /client_id[=:]"?([A-Za-z0-9]{32})/;
const CLIENT_ID_SHAPE = /^[A-Za-z0-9]{32}$/;

// client_id ve nereden geldiği: "sır" | "önbellek" | "keşif". health() bunu
// raporluyor (D-043).
let clientId = null;
let clientIdSource = null;

/** Sunucu 2xx dışında bir kod döndürdü. `status` çağıranın ayırt etmesi için. */
class StatusError extends Error {
  constructor(status, url) {
    super(`SoundCloud HTTP ${status} döndürdü (${url.split("?")[0]})`);
    this.status = status;
  }
}

// --- HTTP ----------------------------------------------------------------------

function get(url) {
  let res;
  try {
    res = host.http.get(url, HEADERS);
  } catch (err) {
    throw new Error(`SoundCloud'a ulaşılamadı: ${err.message}`);
  }
  if (res.status === 429) {
    throw new Error("SoundCloud kotayı doldurdu (429); bir süre bekleyin");
  }
  if (!res.ok) throw new StatusError(res.status, url);
  return res.body;
}

function getJson(url) {
  const body = get(url);
  try {
    return JSON.parse(body);
  } catch (err) {
    throw new Error(`SoundCloud JSON olmayan bir cevap verdi: ${err.message}`);
  }
}

function query(params) {
  return Object.entries(params)
    .map(([key, value]) => `${encodeURIComponent(key)}=${encodeURIComponent(String(value))}`)
    .join("&");
}

function apiUrl(path, params) {
  return `${API_BASE}${path}?${query({ ...params, client_id: resolveClientId() })}`;
}

/** `api-v2`'ye çağrı. client_id reddedilirse **bir kez** tazeleyip tekrar dener. */
function apiGet(path, params = {}) {
  try {
    return getJson(apiUrl(path, params));
  } catch (err) {
    // 404 bir hata değil "yok" cevabıdır; onu burada yutmak, çağıranın
    // ayırt etmesi gereken iki durumu tek tanıya indirger (K9).
    if (!(err instanceof StatusError) || (err.status !== 401 && err.status !== 403)) throw err;
    // Kullanıcının verdiği anahtarı biz tazeleyemeyiz — kendisi bilmeli.
    if (clientIdSource === "sır") {
      throw new Error(
        `Verdiğiniz client_id reddedildi (HTTP ${err.status}). ` +
          "`headshell secret remove plugin:soundcloud client_id` derseniz eklenti kendisi keşfeder.",
      );
    }
    host.log.info(`client_id reddedildi (HTTP ${err.status}), yeniden keşfediliyor`);
    forgetClientId();
    try {
      return getJson(apiUrl(path, params));
    } catch (retry) {
      if (retry instanceof StatusError) {
        throw new Error(
          `Taze client_id ile de reddedildik (HTTP ${retry.status}); ` +
            "SoundCloud'un web yüzeyi değişmiş olabilir.",
        );
      }
      throw retry;
    }
  }
}

// --- client_id çözümü (D-043) ------------------------------------------------
//
// Üç kaynak, bu sırayla: kullanıcının sırrı → motorun deposundaki önbellek →
// keşif. Sıra kasıtlı: kullanıcı bir anahtar verdiyse onu kullanırız,
// arkasından dolanmayız.

function forgetClientId() {
  clientId = null;
  clientIdSource = null;
  host.storage.remove("client_id");
}

/**
 * SoundCloud'un web istemcisinin kullandığı anahtarı JS varlıklarından çıkarır.
 *
 * Belgelenmiş bir uç nokta değil; haber vermeden bozulabilir. Bozulduğunda ne
 * olacağı açık: bir hata ve kullanıcıya "kendi client_id'ni ver".
 */
function discoverClientId() {
  const deadline = Date.now() + DISCOVERY_BUDGET_MS;
  const home = get(HOME_URL);
  const assets = home.match(ASSET_PATTERN) ?? [];
  if (assets.length === 0) throw new Error("SoundCloud ana sayfasında JS varlığı bulunamadı");

  // Sondan başa: anahtar pratikte son paketlerde duruyor, baştan taramak
  // gereksiz yere birkaç megabayt indirmek olur.
  for (const asset of assets.reverse()) {
    if (Date.now() > deadline) break;
    let body;
    try {
      body = get(asset);
    } catch (err) {
      host.log.warn(`JS varlığı okunamadı (${asset}): ${err.message}`);
      continue;
    }
    const found = body.match(CLIENT_ID_PATTERN);
    if (found) return found[1];
  }
  throw new Error(
    "client_id keşfedilemedi (SoundCloud'un web yüzeyi değişmiş olabilir). " +
      "`headshell secret set plugin:soundcloud client_id` ile kendi anahtarınızı verebilirsiniz.",
  );
}

/**
 * Tembel çözüm: yüklemede değil, ilk gerçek çağrıda. Motor yükleme sırasında
 * ağa çıkmaya zaten izin vermiyor; keşif ağ işidir, buraya ait.
 */
function resolveClientId() {
  if (clientId) return clientId;

  const secret = host.secrets.get("client_id");
  if (secret !== null && secret.trim() !== "") {
    clientId = secret.trim();
    clientIdSource = "sır";
    return clientId;
  }

  const cached = host.storage.get("client_id");
  // Bozuk bir önbellek sessizce kabul edilirse hata SoundCloud'un 401'i
  // olarak, yani yanlış yerde görünür.
  if (cached !== null && CLIENT_ID_SHAPE.test(cached)) {
    clientId = cached;
    clientIdSource = "önbellek";
    return clientId;
  }

  clientId = discoverClientId();
  clientIdSource = "keşif";
  try {
    host.storage.set("client_id", clientId);
    host.log.info("client_id keşfedildi ve önbelleğe alındı");
  } catch (err) {
    // Önbellek bir hızlandırmadır; yazılamaması işi durdurmaz ama sessiz de kalmaz.
    host.log.warn(`client_id önbelleğe yazılamadı: ${err.message}`);
  }
  return clientId;
}

// --- parça dönüşümü --------------------------------------------------------------

/**
 * `api-v2` parçasını sözleşmenin parça biçimine çevirir.
 *
 * SoundCloud'da albüm kavramı yok (parçalar setlere ait); `album` boş kalıyor.
 * ISRC yalnızca `publisher_metadata` altında, çoğu parçada yok.
 */
function toTrack(track) {
  const publisher = track.publisher_metadata ?? {};
  const user = track.user ?? {};
  let title = track.title || "Adsız";
  // 30 saniyelik önizleme, tam parça değil. Süresi zaten 30000 geliyor;
  // başlıkta da söylüyoruz ki kullanıcı çalarken şaşırmasın.
  if (track.policy === "SNIP") title = `${title} [önizleme]`;

  const result = {
    id: String(track.id),
    artist: publisher.artist || user.username || "Bilinmeyen sanatçı",
    title,
    duration_ms: Math.trunc(Number(track.duration) || 0),
  };
  if (publisher.isrc) result.isrc = publisher.isrc;
  return result;
}

/**
 * Progressive (düz HTTP) MP3 transcoding'inin çözüm adresi. HLS varyantları
 * bilerek atlanıyor: çekirdek düz bir akış bekliyor.
 */
function progressiveUrl(track) {
  const transcodings = track.media?.transcodings ?? [];
  const progressive = transcodings.find((t) => t.format?.protocol === "progressive");
  return progressive?.url ?? null;
}

// --- sözleşme --------------------------------------------------------------------

export function health() {
  let payload;
  try {
    // En ucuz gerçek çağrı: client_id'yi de, uç noktayı da birlikte sınar.
    payload = apiGet("/search/tracks", { q: "a", limit: 1 });
  } catch (err) {
    // Ulaşamamak bir sağlık cevabıdır, hata değil.
    return { reachable: false, detail: err.message };
  }
  const total = payload.total_results;
  return {
    reachable: true,
    // Katalog boyutu sorguya göre değişen bir sayı, sağlayıcının toplamı
    // değil. "Bilmiyorum" demek, yanlış bir sayı vermekten iyidir.
    track_count: null,
    detail:
      `SoundCloud API v2, client_id kaynağı: ${clientIdSource}` +
      (Number.isInteger(total) ? `, örnek sorgu ${total} sonuç veriyor` : ""),
  };
}

export function search(text, limit) {
  const q = String(text ?? "").trim();
  if (q === "") return [];
  const size = Math.max(1, Math.min(Number(limit) || 20, 200));

  const payload = apiGet("/search/tracks", { q, limit: size });
  const tracks = [];
  let skipped = 0;
  for (const track of payload.collection ?? []) {
    if (track.kind !== "track" || track.id === undefined || track.id === null) {
      skipped += 1;
      continue;
    }
    tracks.push(toTrack(track));
  }
  // K9: düşürülen kayıt sayılır ve raporlanır, sessizce yutulmaz.
  if (skipped > 0) host.log.warn(`aramada ${skipped} kayıt parça olmadığı için atlandı`);
  return tracks;
}

export function resolve_source(id) {
  const trackId = String(id ?? "").trim();
  if (trackId === "") throw new Error("parça kimliği boş");

  let track;
  try {
    track = apiGet(`/tracks/${encodeURIComponent(trackId)}`);
  } catch (err) {
    // "Yok" bir cevaptır, hata değil.
    if (err instanceof StatusError && err.status === 404) return null;
    throw err;
  }
  if (track.streamable === false || track.policy === "BLOCK") return null;

  const transcoding = progressiveUrl(track);
  if (!transcoding) {
    // "Çalamıyorum" ile "yok" farklı tanılardır (K9). Ölçtük: parçaların ~%1'i böyle.
    throw new Error(
      "bu parça yalnızca HLS sunuyor; eklenti HLS çözmüyor (parçaların ~%1'i böyle)",
    );
  }

  const resolved = getJson(`${transcoding}?${query({ client_id: resolveClientId() })}`);
  if (!resolved.url) throw new Error("SoundCloud akış adresi döndürmedi");

  // Adres imzalı ve süreli — önbelleğe alınmaz, her çalmada yeniden çözülür.
  return { kind: "http_stream", url: resolved.url, headers: [] };
}
