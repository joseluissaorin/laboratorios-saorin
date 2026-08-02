#!/usr/bin/env python3
"""Texturas procedurales del estudio (materia real, no vectores planos):
- paper.png       papel hueso con fibras (Deliver, diálogos)
- darkpaper.png   carbón cálido con grano de papel (paneles del cuarto oscuro)
- grain_ui.png    tile de grano fotográfico sutil (overlay global)
Salida en studio/assets/. Solo numpy + ffmpeg (PPM intermedio).
"""

import numpy as np
import os
import subprocess
import shutil

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "assets")
os.makedirs(OUT, exist_ok=True)
FFMPEG = shutil.which("ffmpeg") or r"C:\ProgramData\chocolatey\bin\ffmpeg.exe"
rng = np.random.default_rng(1958)

def fbm(n, octaves=5, p=0.55):
    acc = np.zeros((n, n))
    amp, tot = 1.0, 0.0
    for o in range(octaves):
        s = 2 ** o * 4
        小 = rng.standard_normal((s, s))
        big = np.kron(小, np.ones((n // s, n // s)))[:n, :n]
        # suaviza con un pequeño blur separable
        k = max(n // (s * 2), 1)
        if k > 1:
            ker = np.ones(k) / k
            big = np.apply_along_axis(lambda r: np.convolve(r, ker, "same"), 0, big)
            big = np.apply_along_axis(lambda r: np.convolve(r, ker, "same"), 1, big)
        acc += amp * big
        tot += amp
        amp *= p
    return acc / tot

def fibers(n, count=900):
    img = np.zeros((n, n))
    for _ in range(count):
        x, y = rng.uniform(0, n, 2)
        ang = rng.uniform(0, np.pi)
        ln = rng.uniform(6, 42)
        stren = rng.uniform(0.15, 0.6)
        steps = int(ln)
        dx, dy = np.cos(ang), np.sin(ang)
        for s in range(steps):
            xi = int(x + dx * s + rng.normal(0, 0.4)) % n
            yi = int(y + dy * s + rng.normal(0, 0.4)) % n
            img[yi, xi] += stren
    ker = np.ones(2) / 2
    img = np.apply_along_axis(lambda r: np.convolve(r, ker, "same"), 0, img)
    return np.clip(img, 0, 1)

def save_png(name, rgb):
    ppm = os.path.join(OUT, name + ".ppm")
    png = os.path.join(OUT, name + ".png")
    h, w, _ = rgb.shape
    with open(ppm, "wb") as f:
        f.write(f"P6\n{w} {h}\n255\n".encode())
        f.write((np.clip(rgb, 0, 1) * 255).astype(np.uint8).tobytes())
    subprocess.run([FFMPEG, "-hide_banner", "-loglevel", "error", "-y",
                    "-i", ppm, png], check=True)
    os.remove(ppm)
    print("✓", png)

N = 1024

# papel hueso: base cálida + fbm + fibras + motas
base = np.array([0.953, 0.925, 0.866])           # hueso, nunca #FFFFFF
tex = fbm(N, 6)
tex = (tex - tex.mean()) / (tex.std() + 1e-9)
fib = fibers(N)
paper = np.zeros((N, N, 3))
for c in range(3):
    paper[:, :, c] = base[c] + tex * 0.018 - fib * 0.05
motas = rng.random((N, N)) > 0.99985
paper[motas] = [0.72, 0.66, 0.55]
save_png("paper", paper)

# carbón cálido (cuarto oscuro): mismo papel, entintado casi negro
dark_base = np.array([0.098, 0.088, 0.078])
dark = np.zeros((N, N, 3))
for c in range(3):
    dark[:, :, c] = dark_base[c] + tex * 0.010 + fib * 0.016
save_png("darkpaper", dark)

# grano UI: ruido gaussiano fino monocromo, para overlay a baja opacidad
g = rng.standard_normal((512, 512)) * 0.5 + 0.5
gr = np.repeat(np.clip(g, 0, 1)[:, :, None], 3, axis=2)
save_png("grain_ui", gr)

# papel terracota: el papel del pliegue del cuarto oscuro (teñido, con fibras)
terra_base = np.array([0.769, 0.337, 0.196])   # tinta terracota empapando el papel
terra = np.zeros((N, N, 3))
for c in range(3):
    terra[:, :, c] = terra_base[c] * (1 + tex * 0.05) - fib * 0.10 * (1 - terra_base[c])
manchas = fbm(N, 3)
manchas = (manchas - manchas.min()) / (np.ptp(manchas) + 1e-9)
for c in range(3):
    terra[:, :, c] *= 0.92 + manchas * 0.13     # tinte irregular, baño desigual
save_png("terracota", terra)

# ── cinta de empalme: tira amarillenta translúcida con bordes rasgados ──
def save_png_rgba(name, rgba):
    import struct, zlib
    png = os.path.join(OUT, name + ".png")
    h, w, _ = rgba.shape
    raw = b"".join(b"\x00" + (np.clip(rgba[i], 0, 1) * 255).astype(np.uint8).tobytes() for i in range(h))
    def chunk(tag, data):
        c = tag + data
        return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c) & 0xffffffff)
    with open(png, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)))
        f.write(chunk(b"IDAT", zlib.compress(raw)))
        f.write(chunk(b"IEND", b""))
    print("✓", png)

TW, TH = 96, 256
tape = np.zeros((TH, TW, 4))
tbase = np.array([0.90, 0.85, 0.62])
tnoise = rng.standard_normal((TH, TW)) * 0.03
for c in range(3):
    tape[:, :, c] = tbase[c] + tnoise
tape[:, :, 3] = 0.55 + rng.standard_normal((TH, TW)) * 0.04
# bordes superior/inferior rasgados
for x in range(TW):
    top = int(6 + 5 * abs(np.sin(x * 0.4)) + rng.uniform(0, 4))
    bot = int(6 + 5 * abs(np.cos(x * 0.37)) + rng.uniform(0, 4))
    tape[:top, x, 3] = 0
    tape[TH - bot:, x, 3] = 0
save_png_rgba("splice_tape", tape)

# ── trazos de lápiz graso rojo (3 variantes, materia con cera) ──
for k in range(3):
    GW, GH = 220, 56
    st = np.zeros((GH, GW, 4))
    ys = GH / 2 + np.cumsum(rng.normal(0, 1.1, GW))
    ys = np.clip(ys - (ys.mean() - GH / 2), 8, GH - 8)
    for x in range(4, GW - 4):
        th = 4.5 + 2.2 * np.sin(x * 0.09 + k) + rng.uniform(-1, 1)
        y0 = int(ys[x] - th),
        for y in range(int(ys[x] - th), int(ys[x] + th)):
            if 0 <= y < GH:
                a = np.clip(1.2 - abs(y - ys[x]) / th, 0, 1)
                a *= 0.55 + 0.45 * rng.random()          # cera que agarra desigual
                st[y, x, 3] = max(st[y, x, 3], a * 0.92)
    st[:, :, 0] = 0.78; st[:, :, 1] = 0.16; st[:, :, 2] = 0.10
    save_png_rgba(f"grease_{k}", st)
