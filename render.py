#!/usr/bin/env python3
"""film-render — render CLI del film-look lab (headless Chrome + ffmpeg).

Renderiza un vídeo con EXACTAMENTE los shaders del lab (LUT + film emulation)
de forma determinista: Chrome headless ejecuta el WebGL frame a frame y el
servidor lo hornea a ProRes/H.265 con el audio original.

Uso:
  uv run python film-look-lab/render.py --video IN.mp4 --out OUT.mov \\
      [--prefs prefs.json] [--lut grado.cube] [--fps 60] [--codec prores_ks|hevc]

Sin --prefs usa los defaults cinematográficos del lab.
"""
import argparse
import json
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parent
ASSETS = ROOT / "assets"
SERVER = "http://localhost:8377"

VIVALDI = "/Applications/Vivaldi.app/Contents/MacOS/Vivaldi"
CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"


def ensure_server():
    import urllib.request
    try:
        urllib.request.urlopen(SERVER + "/", timeout=2)
        return None
    except Exception:
        p = subprocess.Popen([sys.executable, str(ROOT / "server.py")],
                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                             start_new_session=True)
        time.sleep(1.5)
        return p


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--video", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--prefs")
    ap.add_argument("--lut")
    ap.add_argument("--fps", type=float)
    ap.add_argument("--codec", default="prores_ks", choices=["prores_ks", "hevc_videotoolbox"])
    a = ap.parse_args()

    video = Path(a.video).expanduser().resolve()
    out = Path(a.out).expanduser().resolve()
    assert video.exists(), video

    if a.fps:
        fps = a.fps
    else:
        r = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "v:0",
                            "-show_entries", "stream=r_frame_rate", "-of", "csv=p=0",
                            str(video)], capture_output=True, text=True)
        num, _, den = r.stdout.strip().partition("/")
        fps = round(float(num) / float(den or 1), 3)

    if a.lut:
        subprocess.run(["uv", "run", "python", str(ROOT / "tools/cube_to_bin.py"),
                        str(Path(a.lut).expanduser().resolve()),
                        str(ASSETS / "lut_custom.bin")], cwd=REPO, check=True)
        lut_bin, lut_json = "assets/lut_custom.bin", "assets/lut_custom.json"
    else:
        lut_bin, lut_json = "assets/lut_user.bin", "assets/lut_user.json"

    prefs = json.loads(Path(a.prefs).read_text()) if a.prefs else {}

    # el vídeo debe ser servible por HTTP: symlink en assets si no está ya
    link = ASSETS / ("_render_input" + video.suffix)
    link.unlink(missing_ok=True)
    link.symlink_to(video)

    job = {"video": f"assets/{link.name}", "videoAbs": str(video), "out": str(out),
           "fps": fps, "prefs": prefs, "lutBin": lut_bin, "lutJson": lut_json,
           "codec": a.codec}
    (ASSETS / "_job.json").write_text(json.dumps(job))
    (ASSETS / "_render_status.json").write_text('{"frame": 0, "total": 1}')

    ensure_server()
    browser = CHROME if Path(CHROME).exists() else VIVALDI
    proc = subprocess.Popen([
        browser, "--headless=new", "--use-angle=metal", "--mute-audio",
        "--disable-renderer-backgrounding", f"{SERVER}/?render=1"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    print(f"🎬 renderizando {video.name} → {out} ({fps}fps, {a.codec})")

    try:
        while True:
            time.sleep(3)
            try:
                st = json.loads((ASSETS / "_render_status.json").read_text())
            except Exception:
                continue
            cur, total = st.get("frame", 0), max(st.get("total", 1), 1)
            print(f"\r  {cur}/{total} ({100*cur/total:.0f}%)", end="", flush=True)
            if st.get("done"):
                print(f"\n✅ {out}")
                break
            if proc.poll() is not None and cur == 0:
                print("\n❌ el navegador headless murió antes de empezar")
                return 1
    except KeyboardInterrupt:
        print("\n⛔ abortado (el encode queda truncado)")
    finally:
        proc.terminate()
    return 0


if __name__ == "__main__":
    sys.exit(main())
