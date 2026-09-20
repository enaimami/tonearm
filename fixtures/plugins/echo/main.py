#!/usr/bin/env python3
"""Sınama eklentisi: protokolün Rust dışında da yürüdüğünün kanıtı.

Bu dosya `tonearm-core`'un eklenti protokolünü (api 1) **eksiksiz** uygulayan
en küçük örnektir; §2.2'nin gerçek SoundCloud eklentisi de aynı iskeleti
kullanacak. Ağa çıkmaz, dosya okumaz — sabit bir katalogla cevap verir.

Davranışını komut satırı bayraklarıyla bozabilirsiniz; testler böyle
kötü eklenti taklidi yapıyor (ortam değişkeni değil: manifestteki ``exec``
zaten argüman taşıyor ve testin süreç ortamını değiştirmesi gerekmiyor):

- ``--api N``        — el sıkışmada bildirilecek api sürümü.
- ``--crash-on M``   — bu metot çağrılınca cevap vermeden ölür.
- ``--hang-on M``    — bu metot çağrılınca sonsuza kadar bekler.
- ``--noise``        — el sıkışmadan önce stdout'a JSON olmayan satır yazar.
"""

import argparse
import json
import sys
import time

API = 1

CATALOG = [
    {
        "id": "track-1",
        "artist": "Ezhel",
        "title": "Geceler",
        "album": "Müptezhel",
        "duration_ms": 213000,
        "isrc": "TR1234567890",
    },
    {
        "id": "track-2",
        "artist": "Sezen Aksu",
        "title": "Gülümse",
        "album": "Gülümse",
        "duration_ms": 254000,
        # Bilerek biçimsiz: çekirdek bunu düşürüp saymalı, kabul etmemeli.
        "isrc": "bu-bir-isrc-degil",
    },
]

state = {"secrets": {}, "data_dir": None}


def parse_args():
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--api", type=int, default=API)
    parser.add_argument("--crash-on", default=None)
    parser.add_argument("--hang-on", default=None)
    parser.add_argument("--noise", action="store_true")
    return parser.parse_args()


OPTIONS = parse_args()


def send(message):
    sys.stdout.write(json.dumps(message, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def reply(request_id, result):
    send({"jsonrpc": "2.0", "id": request_id, "result": result})


def fail(request_id, code, message):
    send({"jsonrpc": "2.0", "id": request_id, "error": {"code": code, "message": message}})


def log(message):
    send({"jsonrpc": "2.0", "method": "log", "params": {"level": "info", "message": message}})


def handshake(params):
    state["secrets"] = params.get("secrets", {})
    state["data_dir"] = params.get("data_dir")
    return {
        "api": OPTIONS.api,
        "name": "echo",
        "display_name": "Echo (sınama)",
        "plugin_version": "0.1.0",
        "capabilities": ["search", "stream"],
    }


def search(params):
    query = params.get("query", "").casefold()
    limit = params.get("limit", 10)
    hits = [
        track
        for track in CATALOG
        if query in track["artist"].casefold() or query in track["title"].casefold()
    ]
    return {"tracks": hits[:limit]}


def resolve_source(params):
    track_id = params.get("id")
    if not any(track["id"] == track_id for track in CATALOG):
        # "Yok" bir cevaptır, hata değil.
        return {"source": None}
    return {
        "source": {
            "kind": "http_stream",
            "url": f"https://ornek.gecersiz/{track_id}.mp3",
            "headers": [{"name": "authorization", "value": state["secrets"].get("token", "")}],
        }
    }


def health(_params):
    return {
        "reachable": True,
        "track_count": len(CATALOG),
        # Sırrın **değeri** değil, varlığı raporlanıyor.
        "detail": "sır var" if state["secrets"] else "sır yok",
    }


HANDLERS = {
    "handshake": handshake,
    "health": health,
    "search": search,
    "resolve_source": resolve_source,
}


def main():
    if OPTIONS.noise:
        print("hazırım! (bu satır JSON değil ve çekirdek onu atlamalı)", flush=True)

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

        if method and method == OPTIONS.crash_on:
            sys.stderr.write(f"kasten çöküyorum: {method}\n")
            sys.exit(3)

        if method and method == OPTIONS.hang_on:
            time.sleep(3600)

        handler = HANDLERS.get(method)
        if handler is None:
            # api 1'de olmayan bir metot: standart JSON-RPC cevabı.
            fail(request_id, -32601, f"metot yok: {method}")
            continue

        try:
            reply(request_id, handler(message.get("params") or {}))
        except Exception as err:  # noqa: BLE001 — eklenti çökmemeli, hata dönmeli
            fail(request_id, -32000, str(err))


if __name__ == "__main__":
    log("echo eklentisi başladı")
    main()
