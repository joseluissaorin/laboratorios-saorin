"""Hornea un .drx (versión 'solo color') a un .cube vía Resolve.

HaldCLUT 512² (= LUT 64³) en una timeline 512×512, ApplyGradeFromDRX, render
DPX 10-bit RGB (4:4:4, sin subsampling de croma), y conversión a .cube.

Uso:  uv run python film-look-lab/tools/bake_lut.py <bake.drx> <out.cube>
"""
import subprocess
import sys
import time
from pathlib import Path

import numpy as np
from PIL import Image

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "src"))
from autodavinci import resolve_api  # noqa: E402

ASSETS = Path(__file__).resolve().parents[1] / "assets"
HALD = ASSETS / "hald.png"
LEVEL = 8          # 8³ = 512 px por lado = LUT de 64³
N = LEVEL ** 2     # muestras por canal (64)


def make_hald():
    if HALD.exists():
        return
    subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y",
                    "-f", "lavfi", "-i", f"haldclutsrc={LEVEL}",
                    "-frames:v", "1", str(HALD)], check=True)
    # sanity: la esquina (0,0) debe ser negro y (511,511) blanco
    im = np.asarray(Image.open(HALD).convert("RGB"))
    assert im[0, 0].tolist() == [0, 0, 0], im[0, 0]
    print(f"hald listo: {HALD} ({im.shape})")


def render_graded_hald(drx: Path, out_dir: Path) -> Path:
    resolve = resolve_api.ensure_resolve()
    project = resolve_api.ensure_project(resolve, "_lutbake")
    resolve_api.configure_project(project, 512, 512, 24)
    pool = project.GetMediaPool()
    items = resolve_api.import_media(project, [str(HALD)])
    item = items[str(HALD)]

    tl = pool.CreateEmptyTimeline(f"bake_{int(time.time())}")
    project.SetCurrentTimeline(tl)
    appended = pool.AppendToTimeline([item])
    tl_item = appended[0]
    graph = tl_item.GetNodeGraph()
    if graph is None or not graph.ApplyGradeFromDRX(str(drx.resolve()), 0):
        raise RuntimeError(f"Resolve rechazó el .drx de bake: {drx}")
    print("grade aplicado al hald")

    out_dir.mkdir(parents=True, exist_ok=True)
    project.SetCurrentRenderFormatAndCodec("dpx", "RGB10")
    if not project.SetRenderSettings({
        "SelectAllFrames": False, "MarkIn": 0, "MarkOut": 0,
        "TargetDir": str(out_dir), "CustomName": "hald_graded",
        "FormatWidth": 512, "FormatHeight": 512,
        "ExportVideo": True, "ExportAudio": False,
    }):
        raise RuntimeError("SetRenderSettings falló")
    project.StopRendering()
    project.DeleteAllRenderJobs()
    job = project.AddRenderJob()
    if not job or not project.StartRendering([job]):
        raise RuntimeError("no arrancó el render")
    while project.IsRenderingInProgress():
        time.sleep(1)
    st = project.GetRenderJobStatus(job)
    if st.get("CompletionPercentage") != 100:
        raise RuntimeError(f"render incompleto: {st}")
    dpx = next(out_dir.glob("hald_graded*.dpx"), None)
    if dpx is None:
        raise RuntimeError(f"no apareció el DPX en {out_dir}")
    print(f"hald graduado: {dpx}")
    return dpx


def dpx_to_cube(dpx: Path, out_cube: Path):
    png = dpx.with_suffix(".png")
    subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y",
                    "-i", str(dpx), str(png)], check=True)
    im = np.asarray(Image.open(png).convert("RGB")).astype(np.float64) / 255.0
    side = N ** 3  # 262144 píxeles = 64³
    assert im.shape[0] * im.shape[1] == side, im.shape
    flat = im.reshape(-1, 3)
    # hald identity: idx = y*512+x; b = idx%N; g = (idx//N)%N; r = idx//N²
    idx = np.arange(side)
    b, g, r = idx % N, (idx // N) % N, idx // (N * N)
    cube = np.zeros((N ** 3, 3))
    cube[r + N * g + N * N * b] = flat  # orden .cube: R el más rápido
    with open(out_cube, "w") as f:
        f.write('TITLE "baked via Resolve HaldCLUT"\nLUT_3D_SIZE %d\n' % N)
        for row in cube:
            f.write("%.6f %.6f %.6f\n" % tuple(row))
    print(f"cube escrito: {out_cube} ({N}³)")


if __name__ == "__main__":
    drx, out_cube = Path(sys.argv[1]), Path(sys.argv[2])
    make_hald()
    dpx = render_graded_hald(drx, ASSETS / "bake")
    dpx_to_cube(dpx, out_cube)
