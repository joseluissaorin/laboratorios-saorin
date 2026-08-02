"""Genera una grain plate tileable (ruido gaussiano periódico vía FFT).

El ruido se sintetiza en frecuencia con fases aleatorias → periódico por
construcción (REPEAT sin costuras) e isotrópico (sin banding axial/diagonal,
el defecto de los hashes procedurales). Espectro ~plano con leve rolloff 1/f^0.5
para que los clumps grandes tengan algo más de energía, como el grano real.

Salida: assets/grain.bin (float16, N×N) + assets/grain.json {size}.
"""
import json
from pathlib import Path

import numpy as np

N = 1024
rng = np.random.default_rng(7)

fx = np.fft.fftfreq(N)[:, None]
fy = np.fft.fftfreq(N)[None, :]
f = np.sqrt(fx * fx + fy * fy)
f[0, 0] = 1e-6
spectrum = f ** -0.25          # entre blanco y 1/f^0.5
spectrum[0, 0] = 0.0
phases = rng.uniform(0, 2 * np.pi, (N, N))
z = spectrum * (np.cos(phases) + 1j * np.sin(phases))
z[0, 0] = 0
noise = np.real(np.fft.ifft2(z))
noise -= noise.mean()
noise /= noise.std()
# compresión a gaussiana acotada suave (evita outliers feos) y a [0,1]
noise = np.tanh(noise / 2.2)
noise = (noise * 0.5 + 0.5).astype(np.float16)

out = Path(__file__).resolve().parents[1] / "assets"
noise.tofile(out / "grain.bin")
(out / "grain.json").write_text(json.dumps({"size": N}))
print(f"grain plate: {out/'grain.bin'} ({N}², std {float(noise.std()):.3f})")
