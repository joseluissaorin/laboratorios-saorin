#!/usr/bin/env python3
"""Servidor estático para film-look lab + endpoints de telemetría.

POST /shot  → guarda el cuerpo en assets/_shot.png (snapshot del canvas)
POST /log   → añade una línea a assets/_log.txt
"""
import http.server
import os
import socketserver
import time

PORT = 8377
ROOT = os.path.dirname(os.path.abspath(__file__))
os.chdir(ROOT)


class H(http.server.SimpleHTTPRequestHandler):
    extensions_map = {
        **http.server.SimpleHTTPRequestHandler.extensions_map,
        ".js": "text/javascript",
        ".mjs": "text/javascript",
        ".bin": "application/octet-stream",
        ".mp4": "video/mp4",
        ".cube": "text/plain",
    }

    def log_message(self, fmt, *a):
        line = fmt % a
        if "_log" not in line and "_shot" not in line and "_render" not in line:
            with open("assets/_access.txt", "a") as f:
                f.write(f"{time.strftime('%H:%M:%S')} {self.client_address[0]} {line}\n")

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(n)
        if self.path == "/shot":
            with open("assets/_shot.png", "wb") as f:
                f.write(body)
        elif self.path == "/log":
            with open("assets/_log.txt", "a") as f:
                f.write(f"{time.strftime('%H:%M:%S')} {body.decode('utf-8', 'replace')}\n")
        elif self.path == "/render_start":
            import json, subprocess
            job = json.loads(body)
            self.server.ffmpeg = subprocess.Popen([
                "ffmpeg", "-hide_banner", "-loglevel", "error", "-y",
                "-f", "image2pipe", "-framerate", str(job["fps"]), "-i", "-",
                "-i", job["audio_src"],
                "-map", "0:v", "-map", "1:a?",
                "-c:v", job.get("codec", "prores_ks"), "-profile:v", "4",
                "-c:a", "aac", "-b:a", "256k",
                "-pix_fmt", "yuv444p10le" if job.get("codec", "prores_ks") == "prores_ks" else "yuv420p10le",
                job["out"]], stdin=subprocess.PIPE)
            self.server.render_total = job["total"]
            self.server.render_done = False
            with open("assets/_render_status.json", "w") as f:
                f.write('{"frame": 0, "total": %d}' % job["total"])
        elif self.path == "/render_frame":
            try:
                self.server.ffmpeg.stdin.write(body)
            except BrokenPipeError:
                pass
            with open("assets/_render_status.json", "w") as f:
                import re
                try:
                    cur = int(open("assets/_render_status.json").read().split('"frame": ')[1].split(",")[0]) + 1
                except Exception:
                    cur = 0
                f.write('{"frame": %d, "total": %d}' % (cur, self.server.render_total))
        elif self.path == "/render_done":
            try:
                self.server.ffmpeg.stdin.close()
                self.server.ffmpeg.wait(timeout=300)
            except Exception:
                pass
            with open("assets/_render_status.json", "w") as f:
                f.write('{"frame": %d, "total": %d, "done": true}' % (self.server.render_total, self.server.render_total))
        elif self.path == "/upload_lut":
            ok, msg = self._convert_lut(body)
            self.send_response(200 if ok else 500)
            self.end_headers()
            self.wfile.write(msg.encode())
            return
        self.send_response(204)
        self.end_headers()

    def _convert_lut(self, body):
        """Recibe .cube o un hald (tif/dpx/png) y produce assets/lut_custom.bin."""
        import subprocess
        name = self.headers.get("X-Filename", "upload.cube")
        ext = os.path.splitext(name)[1].lower()
        src = f"assets/_upload{ext}"
        with open(src, "wb") as f:
            f.write(body)
        repo = os.path.dirname(ROOT)
        try:
            if ext == ".cube":
                cube = src
            else:  # hald graduado
                cube = "assets/_upload.cube"
                subprocess.run(
                    ["uv", "run", "python", "film-look-lab/tools/hald_to_cube.py",
                     os.path.abspath(src), os.path.abspath(cube)],
                    cwd=repo, check=True, capture_output=True, timeout=120)
            subprocess.run(
                ["uv", "run", "python", "film-look-lab/tools/cube_to_bin.py",
                 os.path.abspath(cube), os.path.abspath("assets/lut_custom.bin")],
                cwd=repo, check=True, capture_output=True, timeout=120)
            return True, "ok"
        except subprocess.CalledProcessError as e:
            return False, (e.stderr or b"").decode("utf-8", "replace")[-400:]
        except Exception as e:
            return False, str(e)


socketserver.ThreadingTCPServer.allow_reuse_address = True
with socketserver.ThreadingTCPServer(("0.0.0.0", PORT), H) as s:
    print(f"http://localhost:{PORT}/")
    s.serve_forever()
