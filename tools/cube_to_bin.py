"""Convierte un .cube a binario float32 RGB para WebGL (assets/lut.bin)."""
import json
import sys
from pathlib import Path

import numpy as np

src, dst = Path(sys.argv[1]), Path(sys.argv[2])
vals = []
size = None
for line in src.read_text().splitlines():
    line = line.strip()
    if not line:
        continue
    if line.startswith("LUT_3D_SIZE"):
        size = int(line.split()[1])
        continue
    if line[0].isalpha() or line.startswith("#"):
        continue
    vals.append([float(x) for x in line.split()])
a = np.array(vals, dtype=np.float32)
assert size and len(vals) == size ** 3, (size, len(vals))
a.tofile(dst)
(dst.with_suffix(".json")).write_text(json.dumps({"size": size}))
print(f"{dst} ({size}³, {a.nbytes/1e6:.1f} MB)")
