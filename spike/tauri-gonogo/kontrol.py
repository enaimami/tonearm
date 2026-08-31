#!/usr/bin/env python3
"""PLAN §3.1 kontrol deneyi.

Aynı `ui/index.html`'i düz bir tarayıcıya servis eder ve raporu geri alır.
Amaç tek bir ayrımı yapabilmek: **"WebKitGTK yavaş" mı, yoksa "bu makine bu
sayfayı 60 fps çizemiyor" mu?** İkisi aynı sonuca benziyor ama farklı karar
gerektiriyor — ikincisiyse yerel Rust GUI'ye kaçmak da kurtarmaz, çünkü aynı
GPU'ya çarpar.

Ayrı bir kopya yazmadım; kontrol farklı bir sayfayı ölçseydi karşılaştırma
anlamsız olurdu.
"""

import http.server
import json
import pathlib
import socketserver
import subprocess
import sys
import threading

HERE = pathlib.Path(__file__).parent
UI = HERE / "ui"
OUT = HERE / "report-kontrol.json"
PORT = 8731

done = threading.Event()


class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(UI), **kw)

    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(n)
        OUT.write_bytes(body)
        self.send_response(204)
        self.end_headers()
        try:
            if json.loads(body).get("tamam"):
                done.set()
        except json.JSONDecodeError:
            pass

    def log_message(self, *a):
        pass


def main() -> int:
    browser = sys.argv[1] if len(sys.argv) > 1 else "firefox"
    if OUT.exists():
        OUT.unlink()

    with socketserver.TCPServer(("127.0.0.1", PORT), Handler) as srv:
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        url = f"http://127.0.0.1:{PORT}/"
        print(f"{browser} açılıyor: {url}")
        proc = subprocess.Popen(
            [browser, "--new-window", url],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

        if not done.wait(timeout=120):
            print("ZAMAN AŞIMI: kontrol koşumu 120 sn içinde bitmedi")
        proc.terminate()

    if OUT.exists():
        print(f"--- {OUT.name} ---")
        print(OUT.read_text())
        return 0
    print("RAPOR YOK")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
