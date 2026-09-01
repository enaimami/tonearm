#!/usr/bin/env python3
"""SoundCloud sağlayıcı eklentisi (protokol api 1).

Faz 2 §2.2'nin referans eklentisi. İşi katalog sunmak değil, **protokolün
gerçekten dil bağımsız olduğunu kanıtlamak**: Rust'ta tek satır yok, yalnızca
Python standart kütüphanesi var (`pip install` gerektirmez).

Kapsam bilerek dar — `search` + `stream`. `browse` yok çünkü api 1'de karşılığı
olan bir metot yok; olmayan bir tel biçimini tahmin etmiyoruz.

Ses röle edilmez (K3): çözülen adres istemciye verilir, akışı o çeker.
DRM aşan hiçbir şey yok (D-026): SoundCloud'un kendi `progressive` MP3
transcoding'i imzalı bir adres döndürüyor, biz onu olduğu gibi geçiriyoruz.

Ölçüm (2026-09-01, 200 parçalık örnek): parçaların %99'unda `progressive`
varyant var, %1'i yalnızca HLS sunuyor. HLS çözücü yazmadık; o %1 için
açık hata dönüyoruz — sessizce boş sonuç değil (K9).
"""

import argparse
import json
import os
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

API = 1
VERSION = "0.1.0"
USER_AGENT = f"tune-soundcloud/{VERSION}"

HOME_URL = "https://soundcloud.com/"
API_BASE = "https://api-v2.soundcloud.com"

# Tek bir isteğin üst sınırı. Çekirdeğin çağrı zaman aşımı 20 sn; keşif
# birden çok istek yapabildiği için tek istek bunun küçük bir dilimi olmalı.
REQUEST_TIMEOUT = 8.0
# Keşfin toplam bütçesi. Aşılırsa "bulamadım" deriz — çekirdeği bekletmek yerine.
DISCOVERY_BUDGET = 15.0

ASSET_PATTERN = re.compile(r"https://a-v2\.sndcdn\.com/assets/[^\"']+\.js")
CLIENT_ID_PATTERN = re.compile(r"client_id[=:]\"?([A-Za-z0-9]{32})")
CLIENT_ID_SHAPE = re.compile(r"[A-Za-z0-9]{32}")


class PluginError(Exception):
    """Çağrıyı başarısız kılar ama süreci öldürmez (protokol §4)."""


def parse_args():
    parser = argparse.ArgumentParser(add_help=False)
    # Sırf keşif yolunu sınamak için: sır verilmiş olsa bile keşfe zorlar.
    parser.add_argument("--force-discovery", action="store_true")
    return parser.parse_args()


OPTIONS = parse_args()

state = {
    "secrets": {},
    "data_dir": None,
    "client_id": None,
    # "sır" | "önbellek" | "keşif" — health() bunu raporluyor, D-043 gereği.
    "client_id_source": None,
}


# --- protokol yazımı ------------------------------------------------------


def send(message):
    sys.stdout.write(json.dumps(message, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def reply(request_id, result):
    send({"jsonrpc": "2.0", "id": request_id, "result": result})


def fail(request_id, code, message):
    send({"jsonrpc": "2.0", "id": request_id, "error": {"code": code, "message": message}})


def log(level, message):
    send({"jsonrpc": "2.0", "method": "log", "params": {"level": level, "message": message}})


# --- HTTP -----------------------------------------------------------------


def fetch(url, timeout=REQUEST_TIMEOUT):
    """Ham gövdeyi döndürür. HTTP durum kodunu koruyarak hata yükseltir."""
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return response.read()
    except urllib.error.HTTPError as err:
        if err.code == 429:
            raise PluginError("SoundCloud kotayı doldurdu (429); bir süre bekleyin") from err
        raise
    except urllib.error.URLError as err:
        raise PluginError(f"SoundCloud'a ulaşılamadı: {err.reason}") from err


def fetch_json(url, timeout=REQUEST_TIMEOUT):
    return json.loads(fetch(url, timeout).decode("utf-8", "replace"))


def api_url(path, **params):
    params["client_id"] = client_id()
    return f"{API_BASE}{path}?{urllib.parse.urlencode(params)}"


def api_get(path, **params):
    """`api-v2`'ye çağrı. client_id süresi dolmuşsa **bir kez** tazeleyip tekrar dener."""
    try:
        return fetch_json(api_url(path, **params))
    except urllib.error.HTTPError as err:
        # 404 bir hata değil "yok" cevabıdır; onu burada yutmak, çağıranın
        # ayırt etmesi gereken iki durumu tek tanıya indirger (K9).
        if err.code == 404:
            raise
        if err.code not in (401, 403):
            raise PluginError(f"SoundCloud {err.code} döndürdü: {err.reason}") from err
        # Kullanıcının verdiği anahtarı biz tazeleyemeyiz — kendisi bilmeli.
        if state["client_id_source"] == "sır":
            raise PluginError(
                "Verdiğiniz client_id reddedildi (HTTP "
                f"{err.code}). `tune secret remove plugin:soundcloud client_id` "
                "derseniz eklenti kendisi keşfeder."
            ) from err
        log("info", f"client_id reddedildi (HTTP {err.code}), yeniden keşfediliyor")
        forget_client_id()
        try:
            return fetch_json(api_url(path, **params))
        except urllib.error.HTTPError as retry_err:
            raise PluginError(
                f"Taze client_id ile de reddedildik (HTTP {retry_err.code}); "
                "SoundCloud'un web yüzeyi değişmiş olabilir."
            ) from retry_err


# --- client_id çözümü (D-043) ---------------------------------------------
#
# Üç kaynak, bu sırayla: kullanıcının sırrı → diskteki önbellek → keşif.
# Sıra kasıtlı: kullanıcı bir anahtar verdiyse onu kullanırız, arkasından
# dolanmayız.


def cache_path():
    if not state["data_dir"]:
        return None
    return os.path.join(state["data_dir"], "client_id.txt")


def read_cache():
    path = cache_path()
    if not path or not os.path.exists(path):
        return None
    try:
        with open(path, encoding="utf-8") as handle:
            value = handle.read().strip()
        # Bozuk bir önbellek dosyası sessizce kabul edilirse hata SoundCloud'un
        # 401'i olarak, yani yanlış yerde görünür.
        return value if CLIENT_ID_SHAPE.fullmatch(value) else None
    except OSError as err:
        log("warn", f"client_id önbelleği okunamadı: {err}")
        return None


def write_cache(value):
    path = cache_path()
    if not path:
        return
    try:
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(value)
    except OSError as err:
        # Önbellek bir hızlandırmadır; yazılamaması işi durdurmaz ama sessiz de kalmaz.
        log("warn", f"client_id önbelleğe yazılamadı: {err}")


def forget_client_id():
    state["client_id"] = None
    state["client_id_source"] = None
    path = cache_path()
    if path and os.path.exists(path):
        try:
            os.remove(path)
        except OSError as err:
            log("warn", f"client_id önbelleği silinemedi: {err}")


def discover_client_id():
    """SoundCloud'un web istemcisinin kullandığı anahtarı JS varlıklarından çıkarır.

    Dokümante bir uç nokta değil; haber vermeden bozulabilir. Bozulduğunda
    ne olacağı açık: `PluginError` ve kullanıcıya "kendi client_id'ni ver".
    """
    deadline = time.monotonic() + DISCOVERY_BUDGET
    home = fetch(HOME_URL).decode("utf-8", "replace")
    assets = ASSET_PATTERN.findall(home)
    if not assets:
        raise PluginError("SoundCloud ana sayfasında JS varlığı bulunamadı")

    # Sondan başa: anahtar pratikte son paketlerde duruyor, baştan taramak
    # gereksiz yere birkaç megabayt indirmek olur.
    for asset in reversed(assets):
        if time.monotonic() > deadline:
            break
        try:
            body = fetch(asset, timeout=min(REQUEST_TIMEOUT, max(1.0, deadline - time.monotonic())))
        except (PluginError, urllib.error.HTTPError) as err:
            log("warn", f"JS varlığı okunamadı ({asset}): {err}")
            continue
        found = CLIENT_ID_PATTERN.search(body.decode("utf-8", "replace"))
        if found:
            return found.group(1)

    raise PluginError(
        "client_id keşfedilemedi (SoundCloud'un web yüzeyi değişmiş olabilir). "
        "`tune secret set plugin:soundcloud client_id` ile kendi anahtarınızı verebilirsiniz."
    )


def client_id():
    """Tembel çözüm: el sıkışmada değil, ilk gerçek çağrıda.

    El sıkışmanın zaman aşımı 5 saniye ve protokol "ağa çıkmayın" diyor;
    keşif ağ işidir, buraya ait.
    """
    if state["client_id"]:
        return state["client_id"]

    secret = state["secrets"].get("client_id")
    if secret and not OPTIONS.force_discovery:
        state["client_id"] = secret.strip()
        state["client_id_source"] = "sır"
        return state["client_id"]

    cached = read_cache()
    if cached and not OPTIONS.force_discovery:
        state["client_id"] = cached
        state["client_id_source"] = "önbellek"
        return state["client_id"]

    discovered = discover_client_id()
    state["client_id"] = discovered
    state["client_id_source"] = "keşif"
    write_cache(discovered)
    log("info", "client_id keşfedildi ve önbelleğe alındı")
    return discovered


# --- parça dönüşümü -------------------------------------------------------


def track_to_result(track):
    """`api-v2` parçasını protokolün parça biçimine çevirir.

    SoundCloud'da albüm kavramı yok (parçalar setlere ait); `album` boş
    bırakılıyor. ISRC yalnızca `publisher_metadata` altında, çoğu parçada yok.
    """
    publisher = track.get("publisher_metadata") or {}
    user = track.get("user") or {}
    artist = publisher.get("artist") or user.get("username") or "Bilinmeyen sanatçı"
    title = track.get("title") or "Adsız"

    # 30 saniyelik önizleme, tam parça değil. Süresi zaten 30000 geliyor;
    # başlıkta da söylüyoruz ki kullanıcı çalarken şaşırmasın. api 1'de
    # bunu taşıyacak ayrı bir alan yok.
    if track.get("policy") == "SNIP":
        title = f"{title} [önizleme]"

    result = {
        "id": str(track.get("id")),
        "artist": artist,
        "title": title,
        "duration_ms": int(track.get("duration") or 0),
    }
    isrc = publisher.get("isrc")
    if isrc:
        result["isrc"] = isrc
    return result


def progressive_url(track):
    """Progressive (düz HTTP) MP3 transcoding'inin çözüm adresi.

    HLS varyantları bilerek atlanıyor: çekirdeğin `AudioSource::HttpStream`'i
    düz bir akış bekliyor ve bir HLS çözücü yazmak §2.2'nin işi değil.
    """
    media = track.get("media") or {}
    for transcoding in media.get("transcodings") or []:
        if (transcoding.get("format") or {}).get("protocol") == "progressive":
            return transcoding.get("url")
    return None


# --- metotlar -------------------------------------------------------------


def handshake(params):
    state["secrets"] = params.get("secrets") or {}
    state["data_dir"] = params.get("data_dir")
    return {
        "api": API,
        "name": "soundcloud",
        "display_name": "SoundCloud",
        "plugin_version": VERSION,
        "capabilities": ["search", "stream"],
    }


def health(_params):
    try:
        # En ucuz gerçek çağrı: client_id'yi de, uç noktayı da birlikte sınar.
        payload = api_get("/search/tracks", q="a", limit=1)
    except PluginError as err:
        # Ulaşamamak bir sağlık cevabıdır, hata değil (protokol §health).
        return {"reachable": False, "track_count": None, "detail": str(err)}
    except urllib.error.HTTPError as err:
        return {"reachable": False, "track_count": None, "detail": f"HTTP {err.code}: {err.reason}"}

    total = payload.get("total_results")
    return {
        "reachable": True,
        # Katalog boyutu sorguya göre değişen bir sayı, sağlayıcının toplamı
        # değil. "Bilmiyorum" demek, yanlış bir sayı vermekten iyidir.
        "track_count": None,
        "detail": f"SoundCloud API v2, client_id kaynağı: {state['client_id_source']}"
        + (f", örnek sorgu {total} sonuç veriyor" if isinstance(total, int) else ""),
    }


def search(params):
    query = (params.get("query") or "").strip()
    if not query:
        return {"tracks": []}
    limit = max(1, min(int(params.get("limit") or 20), 200))

    payload = api_get("/search/tracks", q=query, limit=limit)
    tracks = []
    skipped = 0
    for track in payload.get("collection") or []:
        if track.get("kind") != "track" or track.get("id") is None:
            skipped += 1
            continue
        tracks.append(track_to_result(track))

    if skipped:
        # K9: düşürülen kayıt sayılır ve raporlanır, sessizce yutulmaz.
        log("warn", f"aramada {skipped} kayıt parça olmadığı için atlandı")
    return {"tracks": tracks}


def resolve_source(params):
    track_id = str(params.get("id") or "").strip()
    if not track_id:
        raise PluginError("parça kimliği boş")

    try:
        track = api_get(f"/tracks/{urllib.parse.quote(track_id)}")
    except urllib.error.HTTPError as err:
        if err.code == 404:
            # "Yok" bir cevaptır, hata değil.
            return {"source": None}
        raise PluginError(f"parça okunamadı: HTTP {err.code}") from err

    if track.get("streamable") is False or track.get("policy") == "BLOCK":
        return {"source": None}

    transcoding = progressive_url(track)
    if not transcoding:
        # "Çalamıyorum" ile "yok" farklı tanılardır (K9). Ölçtük: parçaların
        # ~%1'i bu durumda.
        raise PluginError(
            "bu parça yalnızca HLS sunuyor; eklenti HLS çözmüyor "
            "(parçaların ~%1'i böyle)"
        )

    resolved = fetch_json(f"{transcoding}?client_id={urllib.parse.quote(client_id())}")
    url = resolved.get("url")
    if not url:
        raise PluginError("SoundCloud akış adresi döndürmedi")

    # Adres imzalı ve süreli — önbelleğe alınmaz, her çalmada yeniden çözülür.
    return {"source": {"kind": "http_stream", "url": url, "headers": []}}


HANDLERS = {
    "handshake": handshake,
    "health": health,
    "search": search,
    "resolve_source": resolve_source,
}


def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue

        method = message.get("method")
        request_id = message.get("id")

        if method == "shutdown":
            return

        handler = HANDLERS.get(method)
        if handler is None:
            fail(request_id, -32601, f"metot yok: {method}")
            continue

        try:
            reply(request_id, handler(message.get("params") or {}))
        except PluginError as err:
            fail(request_id, -32000, str(err))
        except Exception as err:  # noqa: BLE001 — eklenti çökmez, hata döner
            fail(request_id, -32000, f"{type(err).__name__}: {err}")


if __name__ == "__main__":
    main()
