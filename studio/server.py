#!/usr/bin/env python3
"""filmlook studio — servidor local (stdlib puro, Mac/Windows).

Sirve la UI del estudio + el motor WebGL del lab (app/ui) y da API:
  GET  /api/media                 lista de clips del directorio de medios (ffprobe cacheado)
  GET  /api/thumb?f=X&t=S         miniatura jpeg (ffmpeg, cacheada)
  GET  /media/<fichero>           el vídeo (con soporte Range)
  GET/POST /api/project           proyecto JSON (timeline + prefs)
  POST /api/render                render nativo de la timeline (metal en Mac, core en Windows)
  GET  /api/render/status         progreso del render
  GET  /out/<fichero>             resultados

Uso:  python3 server.py [--media DIR] [--port 8765]
"""

import json
import os
import platform
import re
import shutil
import subprocess
import sys
import threading
import time
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

HERE = os.path.dirname(os.path.abspath(__file__))
LAB = os.path.dirname(HERE)
IS_WIN = platform.system() == "Windows"

MEDIA_DIR = os.environ.get("FL_MEDIA") or os.path.join(os.path.expanduser("~"), "filmlab", "media")
PORT = 8765
for i, a in enumerate(sys.argv):
    if a == "--media" and i + 1 < len(sys.argv): MEDIA_DIR = sys.argv[i + 1]
    if a == "--port" and i + 1 < len(sys.argv): PORT = int(sys.argv[i + 1])
os.makedirs(MEDIA_DIR, exist_ok=True)
OUT_DIR = os.path.join(os.path.dirname(MEDIA_DIR), "out")
THUMB_DIR = os.path.join(os.path.dirname(MEDIA_DIR), ".thumbs")
TMP_DIR = os.path.join(os.path.dirname(MEDIA_DIR), ".tmp")
PROJECT = os.path.join(os.path.dirname(MEDIA_DIR), "project.json")
for d in (OUT_DIR, THUMB_DIR, TMP_DIR): os.makedirs(d, exist_ok=True)

VIDEO_EXT = {".mp4", ".mov", ".m4v", ".mkv"}
FFMPEG = shutil.which("ffmpeg") or (r"C:\ProgramData\chocolatey\bin\ffmpeg.exe" if IS_WIN else "ffmpeg")
FFPROBE = shutil.which("ffprobe") or (r"C:\ProgramData\chocolatey\bin\ffprobe.exe" if IS_WIN else "ffprobe")

# renderer nativo por plataforma (override con FL_RENDERER)
if os.environ.get("FL_RENDERER"):
    RENDERER = os.environ["FL_RENDERER"]
elif IS_WIN:
    RENDERER = os.path.join(LAB, "core", "target", "release", "filmlook-core.exe")
else:
    RENDERER = os.path.join(LAB, "metal", "target", "release", "filmlook-metal")

LUT_IN = os.environ.get("FL_LUT_IN") or os.path.join(HERE, "luts", "entrada", "Directo · sin transformar.cube")
LUT_GRADE = os.environ.get("FL_LUT") or os.path.join(HERE, "luts", "color", "Saorín · 65 puntos.cube")

_probe_cache = {}
def probe(path):
    key = (path, os.path.getmtime(path))
    if key in _probe_cache: return _probe_cache[key]
    try:
        out = subprocess.run([FFPROBE, "-v", "error", "-select_streams", "v:0",
                              "-show_entries", "stream=width,height,r_frame_rate,duration",
                              "-of", "json", path], capture_output=True, timeout=20)
        s = json.loads(out.stdout)["streams"][0]
        num, _, den = s["r_frame_rate"].partition("/")
        fps = float(num) / max(float(den or 1), 1)
        dur = float(s.get("duration") or 0)
        if not dur:
            o2 = subprocess.run([FFPROBE, "-v", "error", "-show_entries", "format=duration",
                                 "-of", "csv=p=0", path], capture_output=True, timeout=20)
            dur = float(o2.stdout.strip() or 0)
        info = {"w": s["width"], "h": s["height"], "fps": round(fps, 3), "dur": round(dur, 3)}
    except Exception:
        info = {"w": 0, "h": 0, "fps": 0, "dur": 0}
    _probe_cache[key] = info
    return info

# ── render en segundo plano ────────────────────────────────────────────────
RENDER = {"state": "idle", "step": "", "pct": 0.0, "log": "", "out": ""}
_render_lock = threading.Lock()

def _run(cmd, log_name, env=None):
    RENDER["log"] += "$ " + " ".join(cmd[:6]) + " …\n"
    r = subprocess.run(cmd, capture_output=True, text=True, env=env)
    tail = (r.stderr or r.stdout or "")[-1200:]
    RENDER["log"] += tail + "\n"
    if r.returncode != 0:
        raise RuntimeError(f"{log_name} falló ({r.returncode})")

def render_job(payload):
    try:
        clips = payload["clips"]
        prefs = payload.get("prefs") or {}
        out_name = re.sub(r"[^\w.\-]", "_", payload.get("out_name") or "timeline")
        prefs_path = os.path.join(TMP_DIR, "render_prefs.json")
        with open(prefs_path, "w") as f: json.dump(prefs, f)
        # gelatinas elegidas en el cuarto oscuro (nombres de la lutoteca) o defaults
        def lut_path(slot, name, fallback):
            if name:
                cand = os.path.join(HERE, "luts", slot, os.path.basename(name))
                if os.path.isfile(cand): return cand
            return fallback
        lut_in = lut_path("entrada", payload.get("lut_in"), LUT_IN)
        lut_grade = lut_path("color", payload.get("lut"), LUT_GRADE)

        pieces = []
        n = max(len(clips), 1)
        for i, c in enumerate(clips):
            src = os.path.join(MEDIA_DIR, os.path.basename(c["file"]))
            cut = os.path.join(TMP_DIR, f"cut_{i}.mp4")
            piece = os.path.join(TMP_DIR, f"piece_{i}.mp4")
            RENDER.update(step=f"clip {i+1}/{n}: corte", pct=(i + 0.1) / n)
            # corte alineado a keyframe (sin recompresión; el motor recomprime después)
            _run([FFMPEG, "-hide_banner", "-loglevel", "error", "-y",
                  "-ss", f"{c['in']:.3f}", "-to", f"{c['out']:.3f}",
                  "-i", src, "-map", "0:v:0", "-map", "0:a?", "-c", "copy", cut], "corte")
            RENDER.update(step=f"clip {i+1}/{n}: film-look", pct=(i + 0.3) / n)
            cmd = [RENDERER, "render", cut, "-o", piece, "--prefs", prefs_path]
            if lut_in: cmd += ["--lut-in", lut_in]
            if lut_grade: cmd += ["--lut", lut_grade]
            if IS_WIN: cmd += ["--codec", "hevc_amf", "--scale", "1.0"]
            else: cmd += ["--codec", "hevc"]
            env = dict(os.environ)
            if IS_WIN:
                env["PATH"] = env.get("PATH", "") + r";C:\ProgramData\chocolatey\bin"
            _run(cmd, "render nativo", env=env)
            pieces.append(piece)

        RENDER.update(step="concatenando", pct=0.95)
        lst = os.path.join(TMP_DIR, "concat.txt")
        with open(lst, "w") as f:
            for p in pieces: f.write("file '" + p.replace("'", "'\\''") + "'\n")
        # en Mac el motor saca .mov si es prores; aquí forzamos mp4 hevc → concat copy
        out_path = os.path.join(OUT_DIR, out_name + ".mp4")
        _run([FFMPEG, "-hide_banner", "-loglevel", "error", "-y",
              "-f", "concat", "-safe", "0", "-i", lst, "-c", "copy", out_path], "concat")
        RENDER.update(state="done", step="terminado", pct=1.0,
                      out="/out/" + os.path.basename(out_path))
    except Exception as e:
        RENDER.update(state="error", step=str(e))
    finally:
        for p in os.listdir(TMP_DIR):
            if p.startswith(("cut_",)):
                try: os.remove(os.path.join(TMP_DIR, p))
                except OSError: pass

# ── HTTP ───────────────────────────────────────────────────────────────────
MIME = {".html": "text/html", ".js": "text/javascript", ".css": "text/css",
        ".json": "application/json", ".bin": "application/octet-stream",
        ".png": "image/png", ".jpg": "image/jpeg", ".mp4": "video/mp4",
        ".mov": "video/quicktime", ".svg": "image/svg+xml"}

class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass

    def _send(self, code, body, ctype="application/json", extra=None):
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        for k, v in (extra or {}).items(): self.send_header(k, v)
        self.end_headers()
        self.wfile.write(body)

    def _file(self, path, ranged=False):
        if not os.path.isfile(path):
            return self._send(404, b"not found", "text/plain")
        ext = os.path.splitext(path)[1].lower()
        ctype = MIME.get(ext, "application/octet-stream")
        size = os.path.getsize(path)
        rng = self.headers.get("Range") if ranged else None
        with open(path, "rb") as f:
            if rng:
                m = re.match(r"bytes=(\d*)-(\d*)", rng)
                a = int(m.group(1) or 0)
                b = int(m.group(2) or size - 1)
                b = min(b, size - 1)
                f.seek(a)
                data = f.read(b - a + 1)
                self.send_response(206)
                self.send_header("Content-Range", f"bytes {a}-{b}/{size}")
                self.send_header("Accept-Ranges", "bytes")
                self.send_header("Content-Type", ctype)
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)
            else:
                data = f.read()
                self._send(200, data, ctype, {"Accept-Ranges": "bytes"})

    def do_GET(self):
        u = urllib.parse.urlparse(self.path)
        q = urllib.parse.parse_qs(u.query)
        p = u.path

        if p == "/" or p == "/index.html":
            return self._file(os.path.join(HERE, "index.html"))
        if p.startswith(("/css/", "/js/", "/assets/", "/zine/")):
            safe = os.path.normpath(urllib.parse.unquote(p)).lstrip("/\\")
            return self._file(os.path.join(HERE, safe))
        if p.startswith("/engine/"):
            rel = p[len("/engine/"):]
            return self._file(os.path.join(LAB, "app", "ui", rel))
        if p.startswith("/media/"):
            name = os.path.basename(urllib.parse.unquote(p[len("/media/"):]))
            return self._file(os.path.join(MEDIA_DIR, name), ranged=True)
        if p.startswith("/out/"):
            name = os.path.basename(urllib.parse.unquote(p[len("/out/"):]))
            return self._file(os.path.join(OUT_DIR, name), ranged=True)

        if p == "/api/media":
            items = []
            for name in sorted(os.listdir(MEDIA_DIR)):
                if os.path.splitext(name)[1].lower() in VIDEO_EXT:
                    info = probe(os.path.join(MEDIA_DIR, name))
                    items.append({"name": name, "url": "/media/" + urllib.parse.quote(name), **info})
            return self._send(200, json.dumps(items).encode())

        if p == "/api/thumb":
            name = os.path.basename(q.get("f", [""])[0])
            t = float(q.get("t", ["1"])[0])
            src = os.path.join(MEDIA_DIR, name)
            key = f"{name}_{t:.1f}.jpg".replace(os.sep, "_")
            dst = os.path.join(THUMB_DIR, key)
            if not os.path.isfile(dst) and os.path.isfile(src):
                subprocess.run([FFMPEG, "-hide_banner", "-loglevel", "error", "-y",
                                "-ss", str(t), "-i", src, "-frames:v", "1",
                                "-vf", "scale=320:-2", dst], capture_output=True, timeout=30)
            return self._file(dst)

        if p == "/api/project":
            if os.path.isfile(PROJECT):
                return self._file(PROJECT)
            return self._send(200, b"null")

        if p == "/api/luts":
            out = {}
            for slot in ("entrada", "color"):
                d = os.path.join(HERE, "luts", slot)
                out[slot] = sorted([f for f in os.listdir(d) if f.lower().endswith(".cube")]) \
                    if os.path.isdir(d) else []
            return self._send(200, json.dumps(out).encode())

        if p.startswith("/luts/"):
            rel = urllib.parse.unquote(p[len("/luts/"):])
            slot, _, name = rel.partition("/")
            if slot in ("entrada", "color"):
                return self._file(os.path.join(HERE, "luts", slot, os.path.basename(name)))
            return self._send(404, b"?", "text/plain")

        if p == "/api/renders":
            items = []
            for name in os.listdir(OUT_DIR):
                if os.path.splitext(name)[1].lower() in VIDEO_EXT:
                    st = os.stat(os.path.join(OUT_DIR, name))
                    items.append({"name": name, "url": "/out/" + urllib.parse.quote(name),
                                  "bytes": st.st_size, "mtime": int(st.st_mtime)})
            items.sort(key=lambda x: -x["mtime"])
            return self._send(200, json.dumps(items).encode())

        if p == "/api/render/status":
            return self._send(200, json.dumps(RENDER).encode())

        return self._send(404, b"?", "text/plain")

    def do_POST(self):
        n = int(self.headers.get("Content-Length") or 0)
        body = json.loads(self.rfile.read(n) or b"{}")
        if self.path == "/api/project":
            with open(PROJECT, "w") as f: json.dump(body, f, indent=1)
            return self._send(200, b"{}")
        if self.path == "/api/render":
            with _render_lock:
                if RENDER["state"] == "running":
                    return self._send(409, json.dumps({"error": "ya hay un render"}).encode())
                RENDER.update(state="running", step="preparando", pct=0.0, log="", out="")
            threading.Thread(target=render_job, args=(body,), daemon=True).start()
            return self._send(200, b"{}")
        return self._send(404, b"?", "text/plain")

if __name__ == "__main__":
    print(f"🎬 filmlook studio · http://localhost:{PORT}")
    print(f"   media: {MEDIA_DIR}")
    print(f"   renderer: {RENDERER}")
    ThreadingHTTPServer(("0.0.0.0", PORT), H).serve_forever()
