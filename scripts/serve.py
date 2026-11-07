#!/usr/bin/env python3
"""Serve the WASM build and stream gzip CSVs as ordinary CSV responses."""
import argparse
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import shutil
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parents[1]
DATASETS = {"regensburg", "bayern", "deutschland"}


class Handler(SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(ROOT / "dist"), **kwargs)

    def do_GET(self):
        path = urlsplit(self.path).path
        if path.startswith("/data/"):
            name = path.removeprefix("/data/")
            if name == "manifest.json":
                file = ROOT / "assets/data/manifest.json"
                content_type = "application/json"
                compressed = False
            elif name.endswith(".csv") and name[:-4] in DATASETS:
                file = ROOT / "assets/data" / (name + ".gz")
                content_type = "text/csv; charset=utf-8"
                compressed = True
            else:
                self.send_error(404)
                return
            if not file.exists():
                self.send_error(404, "Run scripts/prepare_data.py first")
                return
            self.send_response(200)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(file.stat().st_size))
            self.send_header("Cache-Control", "public, max-age=3600")
            if compressed:
                self.send_header("Content-Encoding", "gzip")
                self.send_header("Vary", "Accept-Encoding")
            self.end_headers()
            if self.command == "HEAD":
                return
            try:
                with file.open("rb") as data:
                    shutil.copyfileobj(data, self.wfile, 1024 * 1024)
            except (BrokenPipeError, ConnectionResetError):
                pass  # Expected when a user cancels a large import.
            return
        if self.command == "HEAD":
            super().do_HEAD()
        else:
            super().do_GET()

    def do_HEAD(self):
        self.do_GET()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=8787)
    parser.add_argument("--host", default="127.0.0.1")
    args = parser.parse_args()
    print(f"vibegis: http://{args.host}:{args.port}", flush=True)
    ThreadingHTTPServer((args.host, args.port), Handler).serve_forever()
