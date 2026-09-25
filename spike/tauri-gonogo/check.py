#!/usr/bin/env python3
"""The PLAN §3.1 control experiment.

Serves the same `ui/index.html` to a plain browser and takes the report back.
The goal is to be able to make a single distinction: **is "WebKitGTK slow", or
"this machine cannot draw this page at 60 fps"?** The two look like the same
result but call for different decisions — if it is the latter, running off to
a native Rust GUI does not save us either, because it hits the same GPU.

I did not write a separate copy; had the control measured a different page,
the comparison would be meaningless.
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
OUT = HERE / "report-control.json"
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
            if json.loads(body).get("done"):
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
        print(f"opening {browser}: {url}")
        proc = subprocess.Popen(
            [browser, "--new-window", url],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

        if not done.wait(timeout=120):
            print("TIMEOUT: the control run did not finish within 120 s")
        proc.terminate()

    if OUT.exists():
        print(f"--- {OUT.name} ---")
        print(OUT.read_text())
        return 0
    print("NO REPORT")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
