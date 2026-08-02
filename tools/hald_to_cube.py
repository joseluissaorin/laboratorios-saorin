"""Convierte un HaldCLUT graduado (cualquier imagen que lea ffmpeg) a .cube.

Uso:  uv run python film-look-lab/tools/hald_to_cube.py <hald_graduado.(png|tif|dpx)> <out.cube> [nivel]

El hald de nivel L es una imagen de (L³)×(L³) píxeles que representa una LUT
de (L²)³. Nivel 8 → 512×512 → LUT_3D_SIZE 64.
"""
import subprocess
import sys
from pathlib import Path

import numpy as np
from PIL import Image

src, out = Path(sys.argv[1]), Path(sys.argv[2])
level = int(sys.argv[3]) if len(sys.argv) > 3 else 8
N = level * level
side = N ** 3

png = out.with_suffix(".tmp.png")
subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y",
                "-i", str(src), str(png)], check=True)
im = np.asarray(Image.open(png).convert("RGB")).astype(np.float64) / 255.0
assert im.shape[0] * im.shape[1] == side, f"{im.shape} no es un hald de nivel {level}"
flat = im.reshape(-1, 3)
idx = np.arange(side)
b, g, r = idx % N, (idx // N) % N, idx // (N * N)
cube = np.zeros((N ** 3, 3))
cube[r + N * g + N * N * b] = flat
with open(out, "w") as f:
    f.write(f'TITLE "{src.name} via hald_to_cube"\nLUT_3D_SIZE {N}\n')
    for row in cube:
        f.write("%.6f %.6f %.6f\n" % tuple(row))
png.unlink()
print(f"{out} ({N}³)")
