#!/usr/bin/env python3
"""YouTube Music sağlayıcı eklentisi (protokol api 1).

Faz 2 §2.5'in eklentisi (D-048). SoundCloud eklentisiyle aynı kurallar:
Rust'ta tek satır yok, yalnızca Python standart kütüphanesi (`pip install`
gerektirmez), ses röle edilmez (K3), DRM aşan hiçbir şey yok (D-026).

## İş bölümü: arama InnerTube'dan, akış yt-dlp'den

İkisi de ölçülerek seçildi (D-048):

- **Arama** YouTube Music'in kendi InnerTube ucuna gidiyor. yt-dlp'nin arama
  çıktısı yalnızca `title` + `id` veriyor — sanatçı yok, süre yok — ve K6'nın
  bulanık eşleşme halkası bunlar olmadan çalışmaz.
- **Akış** yt-dlp alt sürecine gidiyor. YouTube'un imza/nsig işi bu projenin
  işi değil; bozulduğunda yt-dlp güncellenir, biz değil.

## İki tuzak, ikisi de ölçülmüş

1. **`bestaudio` çalınamaz.** Çekirdeğin symphonia'sında opus çözücüsü ve webm
   kabı yok; `bestaudio` opus/webm seçer. `AUDIO_FORMAT` bu yüzden m4a'ya
   (AAC-LC) sabitli.
2. **Düz GET kısıtlanıyor.** Aynı adres düz istekte 32 KB/s, `Range: bytes=0-`
   başlığıyla 8 MB/s veriyor — 250 kat. Başlık akış kaynağıyla birlikte
   çekirdeğe geçiriliyor. Şifre çözme değil, sunucunun kendi desteklediği
   standart bir HTTP başlığı.
"""

import json
import os
import re
import shutil
import subprocess
import sys
import urllib.error
import urllib.request

API = 1
VERSION = "0.1.0"

INNERTUBE_URL = "https://music.youtube.com/youtubei/v1/search"
WATCH_URL = "https://music.youtube.com/watch?v="

# InnerTube istemci bağlamı. Anahtar gerekmiyor (ölçüldü, D-048); istemci adı
# WEB_REMIX çünkü aradığımız şey YouTube Music kataloğu, YouTube'un tamamı değil.
INNERTUBE_CLIENT = {
    "clientName": "WEB_REMIX",
    "clientVersion": "1.20240101.01.00",
    "hl": "en",
    "gl": "US",
}
# Arama süzgeci: yalnızca şarkılar. Süzgeçsiz sorgu kanal sayfası, çalma
# listesi ve "10 saatlik loop" videosu da döndürüyor.
SONGS_FILTER = "EgWKAQIIAWoKEAoQCRADEAQQBQ=="

BROWSER_UA = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36"

# Çözebildiğimiz en iyi ses. Sıra önemli: 140 (AAC-LC ~130 kbps) yoksa
# 139 (~49 kbps) alınır; ikisi de yoksa hata döner — sessizce opus seçilmez.
AUDIO_FORMAT = "140/139/bestaudio[ext=m4a]"

# Çekirdeğin çağrı zaman aşımı 20 sn. yt-dlp imza çözerken 8-10 sn
# harcayabiliyor; bütçe bunun altında kalmalı ki hata *bizden* çıksın ve
# sebebini söyleyebilelim (K9).
YTDLP_TIMEOUT = 16.0
HTTP_TIMEOUT = 8.0

DURATION_PATTERN = re.compile(r"^(?:(\d+):)?(\d{1,2}):(\d{2})$")


class PluginError(Exception):
    """Çağrıyı başarısız kılar ama süreci öldürmez (protokol §4)."""


state = {
    "secrets": {},
    "data_dir": None,
    # yt-dlp komutu (liste) ve nereden bulunduğu; health() bunu raporluyor.
    "ytdlp": None,
    "ytdlp_source": None,
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


# --- InnerTube ------------------------------------------------------------


def innertube_search(query, limit):
    """YouTube Music'in şarkı aramasını çağırır ve ham satırları döndürür."""
    payload = json.dumps(
        {
            "context": {"client": dict(INNERTUBE_CLIENT)},
            "query": query,
            "params": SONGS_FILTER,
        }
    ).encode("utf-8")

    request = urllib.request.Request(
        INNERTUBE_URL,
        data=payload,
        headers={
            "Content-Type": "application/json",
            "User-Agent": BROWSER_UA,
            "Origin": "https://music.youtube.com",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=HTTP_TIMEOUT) as response:
            body = response.read()
    except urllib.error.HTTPError as err:
        raise PluginError(
            f"YouTube Music araması {err.code} döndürdü ({err.reason}); "
            "InnerTube yüzeyi değişmiş olabilir"
        ) from err
    except urllib.error.URLError as err:
        raise PluginError(f"YouTube Music'e ulaşılamadı: {err.reason}") from err

    try:
        document = json.loads(body.decode("utf-8", "replace"))
    except json.JSONDecodeError as err:
        raise PluginError(f"YouTube Music JSON olmayan bir cevap verdi: {err}") from err

    return collect_song_rows(document)[:limit]


def collect_song_rows(document):
    """Cevabın içinden şarkı satırlarını toplar.

    InnerTube'un ağacı derin ve haber vermeden değişiyor; bu yüzden yolu
    adım adım yürümek yerine `musicResponsiveListItemRenderer` anahtarını
    **arıyoruz.** Yapı bir kat değişirse arama hâlâ çalışır; anahtarın kendisi
    kaybolursa sıfır satır döner ve çağıran bunu "yüzey değişti" diye raporlar.

    **Sıra korunur.** YouTube'un verdiği sıra alaka sırasıdır ve `limit` onu
    baştan kesiyor; gezinme sırayı bozarsa `limit=6` en iyi eşleşmeyi değil
    rastgele altı satırı verir. Bu yüzden çocuklar yığına **ters** basılıyor:
    `pop()` onları belge sırasında geri veriyor.
    """
    rows = []
    stack = [document]
    while stack:
        node = stack.pop()
        if isinstance(node, dict):
            row = node.get("musicResponsiveListItemRenderer")
            if isinstance(row, dict):
                rows.append(row)
                continue
            stack.extend(reversed(list(node.values())))
        elif isinstance(node, list):
            stack.extend(reversed(node))
    return rows


def column_runs(row, index):
    """`flexColumns[index]` içindeki metin parçaları."""
    columns = row.get("flexColumns") or []
    if index >= len(columns):
        return []
    column = columns[index].get("musicResponsiveListItemFlexColumnRenderer") or {}
    return (column.get("text") or {}).get("runs") or []


def video_id(row):
    """Satırın `videoId`'si. İki yerde durabiliyor; ikisine de bakıyoruz."""
    play = (
        ((row.get("overlay") or {}).get("musicItemThumbnailOverlayRenderer") or {}).get("content")
        or {}
    ).get("musicPlayButtonRenderer") or {}
    found = ((play.get("playNavigationEndpoint") or {}).get("watchEndpoint") or {}).get("videoId")
    if found:
        return found
    for run in column_runs(row, 0):
        endpoint = (run.get("navigationEndpoint") or {}).get("watchEndpoint") or {}
        if endpoint.get("videoId"):
            return endpoint["videoId"]
    return None


def parse_duration_ms(text):
    """`4:11` ya da `1:02:30` → milisaniye. Tanımadıysa `None`."""
    matched = DURATION_PATTERN.match(text.strip())
    if not matched:
        return None
    hours, minutes, seconds = matched.groups()
    total = int(hours or 0) * 3600 + int(minutes) * 60 + int(seconds)
    return total * 1000


def split_metadata_runs(runs):
    """İkinci sütunu ` • ` ayırıcılarına göre gruplara böler.

    Gözlenen biçim: `sanatçı [• albüm] • süre`. Albüm her satırda yok, ve
    sanatçı birden çok parça olabiliyor (`vagabond`, `,`, `shilou.`, `&`,
    `vibe`). Bu yüzden ayırıcıyı sabit bir konum değil, metnin kendisi
    belirliyor.
    """
    groups = [[]]
    for run in runs:
        text = run.get("text") or ""
        if text.strip() == "•":
            groups.append([])
        else:
            groups[-1].append(text)
    return ["".join(group).strip() for group in groups if "".join(group).strip()]


def row_to_track(row):
    """Bir InnerTube satırını protokolün parça biçimine çevirir.

    Çeviremediğinde `None` döner — çağıran bunları **sayıp raporluyor** (K9).
    """
    identifier = video_id(row)
    if not identifier:
        return None

    title_runs = column_runs(row, 0)
    title = "".join(run.get("text") or "" for run in title_runs).strip()
    if not title:
        return None

    groups = split_metadata_runs(column_runs(row, 1))
    duration_ms = None
    if groups:
        duration_ms = parse_duration_ms(groups[-1])
        if duration_ms is not None:
            groups = groups[:-1]

    artist = groups[0] if groups else ""
    album = groups[1] if len(groups) > 1 else ""

    if not artist:
        # Sanatçısız bir satır bulanık eşleşmeye giremez (K6). Düşürüyoruz
        # ama sayılıyor — "Bilinmeyen sanatçı" diye uydurmaktan iyidir.
        return None

    track = {
        "id": identifier,
        "artist": artist,
        "title": title,
        "duration_ms": duration_ms or 0,
    }
    if album:
        track["album"] = album
    return track


# --- yt-dlp ---------------------------------------------------------------


def ytdlp_command():
    """yt-dlp'yi bulur: `TUNE_YTDLP` → `PATH` → `python3 -m yt_dlp`.

    Bulunamazsa hata — "boş sonuç" değil. Kurulu olmayan bir araç ile
    bulunamayan bir parça iki ayrı tanıdır (K9).
    """
    if state["ytdlp"]:
        return state["ytdlp"]

    override = os.environ.get("TUNE_YTDLP")
    if override:
        # Çalıştırma biti yoksa yorumlayıcıya veriyoruz: yt-dlp'nin tek dosyalık
        # zipapp'i indirildiği gibi çalıştırılabilir olmayabilir.
        executable = os.access(override, os.X_OK)
        state["ytdlp"] = [override] if executable else [sys.executable, override]
        state["ytdlp_source"] = "TUNE_YTDLP"
        return state["ytdlp"]

    found = shutil.which("yt-dlp")
    if found:
        state["ytdlp"] = [found]
        state["ytdlp_source"] = "PATH"
        return state["ytdlp"]

    probe = subprocess.run(
        [sys.executable, "-m", "yt_dlp", "--version"],
        capture_output=True,
        timeout=YTDLP_TIMEOUT,
        check=False,
    )
    if probe.returncode == 0:
        state["ytdlp"] = [sys.executable, "-m", "yt_dlp"]
        state["ytdlp_source"] = "python -m yt_dlp"
        return state["ytdlp"]

    # Mesaj bilerek işletim sisteminden bağımsız (D-049): burada `pacman -S`
    # yazmak Arch dışındaki her kullanıcıya yanlış tavsiye vermek olur.
    raise PluginError(
        "yt-dlp bulunamadı. Kurulum yolları: "
        "https://github.com/yt-dlp/yt-dlp#installation — tek dosyalık sürüm "
        "root yetkisi istemez. Kuruluysa yolunu `TUNE_YTDLP` ortam "
        "değişkeninde verebilirsiniz."
    )


def ytdlp_json(video):
    """Tek bir parçanın ses biçimini yt-dlp ile çözer."""
    command = ytdlp_command() + [
        "--no-warnings",
        "--no-playlist",
        "-f",
        AUDIO_FORMAT,
        "-J",
        WATCH_URL + video,
    ]
    try:
        finished = subprocess.run(
            command, capture_output=True, timeout=YTDLP_TIMEOUT, check=False
        )
    except subprocess.TimeoutExpired as err:
        raise PluginError(
            f"yt-dlp {YTDLP_TIMEOUT:.0f} sn içinde cevap vermedi; "
            "YouTube yavaşlamış ya da imza çözümü uzuyor olabilir"
        ) from err
    except OSError as err:
        raise PluginError(f"yt-dlp çalıştırılamadı: {err}") from err

    if finished.returncode != 0:
        # yt-dlp'nin kendi mesajını olduğu gibi geçiriyoruz (D-048): "bir şey
        # olmadı" yerine "Sign in to confirm you're not a bot" görünsün.
        detail = finished.stderr.decode("utf-8", "replace").strip().splitlines()
        last = detail[-1] if detail else f"çıkış kodu {finished.returncode}"
        raise PluginError(f"yt-dlp: {last}")

    try:
        return json.loads(finished.stdout.decode("utf-8", "replace"))
    except json.JSONDecodeError as err:
        raise PluginError(f"yt-dlp JSON olmayan bir cevap verdi: {err}") from err


def pick_audio_url(document):
    """Seçilen biçimin adresini döndürür.

    `-f` verildiğinde yt-dlp adresi tepe düzeyde `url` olarak veriyor; birden
    çok akış birleştirilmişse `requested_formats` altında. İkisine de bakıp
    sessiz bir video akışına düşmediğimizi doğruluyoruz.

    Eleme `acodec == "none"` üzerinden, `acodec` **dolu mu** üzerinden değil:
    alanı hiç göndermeyen bir yt-dlp sürümünde ikincisi geçerli bir sesi
    "biçim bulunamadı" diye reddederdi — olmayan bir kusuru rapor etmek de
    K9'un yasakladığı yanlış tanılardan biri.
    """
    candidates = document.get("requested_formats") or [document]
    for candidate in candidates:
        if candidate.get("acodec") == "none":
            continue
        if candidate.get("url"):
            return candidate
    return None


# --- metotlar -------------------------------------------------------------


def handshake(params):
    state["secrets"] = params.get("secrets") or {}
    state["data_dir"] = params.get("data_dir")
    return {
        "api": API,
        "name": "ytmusic",
        "display_name": "YouTube Music",
        "plugin_version": VERSION,
        "capabilities": ["search", "stream"],
    }


def health(_params):
    """İki bağımsız şeyi ayrı ayrı yoklar: arama ucu ve yt-dlp.

    Tek bir "çalışmıyor" cevabı hangisinin bozulduğunu gizlerdi (K9).
    """
    notes = []
    reachable = True

    try:
        rows = innertube_search("a", 1)
        notes.append(f"InnerTube araması {len(rows)} satır döndürdü")
    except PluginError as err:
        reachable = False
        notes.append(f"arama: {err}")

    try:
        command = ytdlp_command()
        version = subprocess.run(
            command + ["--version"], capture_output=True, timeout=YTDLP_TIMEOUT, check=False
        )
        if version.returncode == 0:
            found = version.stdout.decode("utf-8", "replace").strip()
            notes.append(f"yt-dlp {found} ({state['ytdlp_source']})")
        else:
            reachable = False
            notes.append(f"yt-dlp --version çıkış kodu {version.returncode}")
    except (PluginError, OSError, subprocess.TimeoutExpired) as err:
        reachable = False
        notes.append(f"yt-dlp: {err}")

    return {
        "reachable": reachable,
        # Katalog boyutu sorguya göre değişir, sağlayıcının toplamı değil.
        # "Bilmiyorum" demek yanlış bir sayı vermekten iyidir.
        "track_count": None,
        "detail": "; ".join(notes),
    }


def search(params):
    query = (params.get("query") or "").strip()
    if not query:
        return {"tracks": []}
    limit = max(1, min(int(params.get("limit") or 20), 100))

    rows = innertube_search(query, limit)
    if not rows:
        # Sıfır satır iki şey olabilir: sonuç yok ya da yüzey değişti.
        # Ayırmıyoruz ama sessiz de kalmıyoruz.
        log("info", f"'{query}' için şarkı satırı gelmedi")
        return {"tracks": []}

    tracks = []
    skipped = 0
    for row in rows:
        track = row_to_track(row)
        if track is None:
            skipped += 1
            continue
        tracks.append(track)

    if skipped:
        # K9: düşürülen kayıt sayılır ve raporlanır, sessizce yutulmaz.
        log("warn", f"aramada {skipped} satır çevrilemedi (kimlik/sanatçı/başlık eksik)")
    return {"tracks": tracks}


def resolve_source(params):
    video = str(params.get("id") or "").strip()
    if not video:
        raise PluginError("parça kimliği boş")

    document = ytdlp_json(video)
    chosen = pick_audio_url(document)
    if chosen is None:
        # "Çalamıyorum" ile "yok" farklı tanılardır (K9).
        raise PluginError(
            "yt-dlp bu parça için çözebildiğimiz bir ses biçimi vermedi "
            f"(istenen: {AUDIO_FORMAT})"
        )

    return {
        "source": {
            "kind": "http_stream",
            "url": chosen["url"],
            # Ölçüldü (D-048): bu başlık olmadan aynı adres 32 KB/s, onunla
            # 8 MB/s veriyor. Şifre çözme değil — sunucunun 206 ile cevapladığı
            # standart bir menzil isteği.
            "headers": [{"name": "Range", "value": "bytes=0-"}],
        }
    }


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
