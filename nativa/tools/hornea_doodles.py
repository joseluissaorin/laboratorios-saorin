#!/usr/bin/env python3
"""Hornea assets/doodles.png: el atlas de objetos del taller.

Cada objeto se dibuja a 4x y se baja con LANCZOS (antialias). Las coordenadas
del atlas están DUPLICADAS en nativa/src/doodles.rs — si cambias el layout
aquí, cámbialo allí. Se ejecuta a mano en build: python3 tools/hornea_doodles.py
"""
from PIL import Image, ImageDraw, ImageOps, ImageFilter
import math, os, random

RAIZ = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
STUDIO = os.path.join(os.path.dirname(RAIZ), "studio")
S = 4  # supersampling

atlas = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))


def baja(img, w, h):
    return img.resize((w, h), Image.LANCZOS)


# ── la lata metálica (512×512, agujero central transparente) ──────────────
def lata():
    D = 512 * S
    im = Image.new("RGBA", (D, D), (0, 0, 0, 0))
    dr = ImageDraw.Draw(im)
    cx = cy = D // 2
    R = D // 2 - 4 * S
    # cuerpo: anillos concéntricos con brillo metálico (radial + direccional)
    pasos = 240
    for i in range(pasos):
        f = i / pasos            # 0 borde → 1 centro
        r = R * (1 - f)
        # perfil de una tapa de lata real: borde alto, canal, cuerpo, meseta
        if f < 0.045: g = 205 - f * 800          # canto brillante
        elif f < 0.10: g = 148 + (f - 0.045) * 600   # canal oscuro
        elif f < 0.16: g = 196                    # lomo
        elif f < 0.22: g = 168                    # segundo canal
        else: g = 186 - f * 30                    # cuerpo hacia el centro
        g = max(120, min(212, int(g)))
        dr.ellipse([cx - r, cy - r, cx + r, cy + r], fill=(g, g, int(g * 0.98), 255))
    # brillo direccional (arco superior-izquierdo)
    brillo = Image.new("L", (D, D), 0)
    db = ImageDraw.Draw(brillo)
    for i in range(60):
        a0 = 195 + i * 0.5
        db.arc([12 * S + i, 12 * S + i, D - 12 * S - i, D - 12 * S - i],
               a0, a0 + 90 - i, fill=max(0, 90 - i * 2), width=3 * S)
    brillo = brillo.filter(ImageFilter.GaussianBlur(6 * S))
    im.paste(Image.new("RGBA", (D, D), (255, 255, 255, 255)), (0, 0),
             Image.composite(brillo, Image.new("L", (D, D), 0), im.split()[3]))
    # agujero central: transparente (la miniatura asoma por debajo)
    hueco = int(R * 0.46)
    mask = Image.new("L", (D, D), 255)
    dm = ImageDraw.Draw(mask)
    dm.ellipse([cx - hueco, cy - hueco, cx + hueco, cy + hueco], fill=0)
    im.putalpha(Image.composite(im.split()[3], Image.new("L", (D, D), 0), mask))
    # borde del agujero: anillo oscuro + filo claro
    dr = ImageDraw.Draw(im)
    dr.ellipse([cx - hueco, cy - hueco, cx + hueco, cy + hueco],
               outline=(70, 66, 60, 255), width=3 * S)
    dr.ellipse([cx - hueco - 3 * S, cy - hueco - 3 * S, cx + hueco + 3 * S, cy + hueco + 3 * S],
               outline=(225, 222, 215, 200), width=2 * S)
    # borde exterior definido
    dr.ellipse([cx - R, cy - R, cx + R, cy + R], outline=(96, 92, 86, 255), width=2 * S)
    return baja(im, 512, 512)


# ── la foto B/N del laboratorista, con borde de copia ─────────────────────
def foto():
    src = os.path.join(STUDIO, "assets", "matter", "lab_photo.jpg")
    im = Image.open(src).convert("L")
    im = ImageOps.fit(im, (312 * S // 4, 232 * S // 4), Image.LANCZOS)
    im = ImageOps.autocontrast(im, cutoff=1)
    sep = ImageOps.colorize(im, (28, 24, 20), (238, 234, 224), mid=(128, 120, 108))
    marco = Image.new("RGBA", (336, 272), (246, 243, 235, 255))
    marco.paste(sep.convert("RGBA"), (12, 12))
    d = ImageDraw.Draw(marco)
    d.rectangle([0, 0, 335, 271], outline=(180, 174, 160, 255), width=1)
    return marco


# ── celo (del zine) ───────────────────────────────────────────────────────
def celo():
    src = os.path.join(STUDIO, "zine", "img", "celo.png")
    im = Image.open(src).convert("RGBA")
    return ImageOps.fit(im, (176, 80), Image.LANCZOS)


# ── chincheta ─────────────────────────────────────────────────────────────
def chincheta(color=(217, 51, 37)):
    D = 88 * S
    im = Image.new("RGBA", (D, D), (0, 0, 0, 0))
    dr = ImageDraw.Draw(im)
    cx, cy, R = D // 2, D // 2, int(D * 0.40)
    dr.ellipse([cx - R, cy - R + 4 * S, cx + R, cy + R + 4 * S], fill=(0, 0, 0, 60))
    for i in range(R):
        f = i / R
        r = R - i
        c = tuple(int(v * (0.72 + 0.5 * f)) for v in color)
        dr.ellipse([cx - r, cy - r, cx + r, cy + r], fill=c + (255,))
    dr.ellipse([cx - int(R * 0.45) - 6 * S, cy - int(R * 0.55), cx - 6 * S, cy - int(R * 0.12)],
               fill=(255, 255, 255, 130))
    return baja(im, 88, 88)


# ── grapa ─────────────────────────────────────────────────────────────────
def grapa():
    D = 88 * S
    im = Image.new("RGBA", (D, D), (0, 0, 0, 0))
    dr = ImageDraw.Draw(im)
    m, g = 18 * S, 7 * S
    dr.line([(m, D - m), (m, m), (D - m, m), (D - m, D - m)],
            fill=(120, 116, 110, 255), width=g, joint="curve")
    dr.line([(m - g // 3, D - m), (m - g // 3, m)], fill=(180, 176, 170, 200), width=2 * S)
    return baja(im, 88, 88)


# ── botella de baño (marrón, etiqueta en blanco para runtime) ─────────────
def botella():
    W, H = 168 * S, 304 * S
    im = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    dr = ImageDraw.Draw(im)
    marron = (86, 54, 30)
    cuerpo_y = int(H * 0.30)
    dr.rounded_rectangle([6 * S, cuerpo_y, W - 6 * S, H - 6 * S], radius=18 * S,
                         fill=marron + (255,))
    # hombros
    dr.polygon([(int(W * 0.32), int(H * 0.12)), (int(W * 0.68), int(H * 0.12)),
                (W - 6 * S, cuerpo_y + 10 * S), (6 * S, cuerpo_y + 10 * S)],
               fill=marron + (255,))
    # cuello + tapón
    dr.rectangle([int(W * 0.36), int(H * 0.05), int(W * 0.64), int(H * 0.14)],
                 fill=(60, 36, 18, 255))
    dr.rounded_rectangle([int(W * 0.33), int(H * 0.01), int(W * 0.67), int(H * 0.07)],
                         radius=4 * S, fill=(30, 20, 12, 255))
    # brillo vertical
    dr.rounded_rectangle([int(W * 0.14), cuerpo_y + 16 * S, int(W * 0.24), H - 22 * S],
                         radius=8 * S, fill=(255, 240, 220, 46))
    # sombra del canto derecho
    dr.rounded_rectangle([W - 20 * S, cuerpo_y + 12 * S, W - 8 * S, H - 14 * S],
                         radius=8 * S, fill=(20, 10, 4, 80))
    return baja(im, 168, 304)


# ── caja de stock (kraft con doble borde) ─────────────────────────────────
def caja():
    W, H = 344 * S, 168 * S
    im = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    dr = ImageDraw.Draw(im)
    kraft = (203, 176, 135)
    dr.rounded_rectangle([2 * S, 2 * S, W - 2 * S, H - 2 * S], radius=8 * S, fill=kraft + (255,))
    dr.rounded_rectangle([2 * S, 2 * S, W - 2 * S, H - 2 * S], radius=8 * S,
                         outline=(92, 68, 40, 255), width=3 * S)
    dr.rounded_rectangle([12 * S, 12 * S, W - 12 * S, H - 12 * S], radius=5 * S,
                         outline=(92, 68, 40, 200), width=2 * S)
    # solapa (línea de tapa)
    dr.line([(2 * S, int(H * 0.30)), (W - 2 * S, int(H * 0.30))], fill=(92, 68, 40, 140), width=2 * S)
    # esquinas gastadas
    for (x, y) in [(6 * S, 6 * S), (W - 26 * S, 6 * S), (6 * S, H - 26 * S), (W - 26 * S, H - 26 * S)]:
        dr.ellipse([x, y, x + 20 * S, y + 20 * S], fill=(255, 250, 240, 26))
    return baja(im, 344, 168)


# ── cubeta (esmaltada con filo azul) ──────────────────────────────────────
def cubeta():
    W, H = 344 * S, 200 * S
    im = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    dr = ImageDraw.Draw(im)
    dr.rounded_rectangle([2 * S, 2 * S, W - 2 * S, H - 2 * S], radius=16 * S,
                         fill=(238, 236, 228, 255))
    dr.rounded_rectangle([2 * S, 2 * S, W - 2 * S, H - 2 * S], radius=16 * S,
                         outline=(43, 59, 199, 255), width=4 * S)
    # reborde interior (la pared de la cubeta)
    dr.rounded_rectangle([14 * S, 16 * S, W - 14 * S, H - 12 * S], radius=10 * S,
                         fill=(222, 219, 209, 255))
    dr.rounded_rectangle([14 * S, 16 * S, W - 14 * S, H - 12 * S], radius=10 * S,
                         outline=(150, 148, 140, 255), width=2 * S)
    # sombra del borde superior interior
    dr.rounded_rectangle([14 * S, 16 * S, W - 14 * S, 34 * S], radius=8 * S,
                         fill=(120, 118, 112, 70))
    return baja(im, 344, 200)


# ── pinza de tender ───────────────────────────────────────────────────────
def pinza():
    W, H = 96 * S, 208 * S
    im = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    dr = ImageDraw.Draw(im)
    madera = (196, 160, 110)
    oscuro = (150, 116, 72)
    # dos patas
    dr.polygon([(int(W * 0.18), 4 * S), (int(W * 0.46), 4 * S),
                (int(W * 0.40), H - 6 * S), (int(W * 0.10), H - 24 * S)], fill=madera + (255,))
    dr.polygon([(int(W * 0.54), 4 * S), (int(W * 0.82), 4 * S),
                (int(W * 0.90), H - 24 * S), (int(W * 0.60), H - 6 * S)], fill=oscuro + (255,))
    # muelle
    cy = int(H * 0.42)
    dr.ellipse([int(W * 0.30), cy - 14 * S, int(W * 0.70), cy + 14 * S],
               outline=(110, 106, 100, 255), width=5 * S)
    dr.line([(int(W * 0.30), cy + 6 * S), (int(W * 0.16), cy + 30 * S)],
            fill=(110, 106, 100, 255), width=4 * S)
    # vetas
    for k in range(3):
        x = int(W * (0.24 + 0.07 * k))
        dr.line([(x, 12 * S), (x - 4 * S, H - 40 * S)], fill=(150, 116, 72, 90), width=1 * S)
    return baja(im, 96, 208)


# ── washi (4 tiras, bordes rasgados, semitransparente) ────────────────────
def washi(color, alfa=200):
    W, H = 256 * S, 56 * S
    im = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    dr = ImageDraw.Draw(im)
    rnd = random.Random(color[0] * 7 + color[1] * 13 + color[2])
    # borde rasgado: polígono con dientes en los extremos
    pts = [(8 * S, 4 * S)]
    for k in range(6):
        pts.append((W - (14 - rnd.randint(0, 9)) * S, int(H * k / 5)))
    pts += [(W - 8 * S, H - 4 * S), (8 * S, H - 4 * S)]
    for k in range(6):
        pts.append(((14 - rnd.randint(0, 9)) * S, int(H * (5 - k) / 5)))
    dr.polygon(pts, fill=color + (alfa,))
    # rayitas del papel washi
    for k in range(0, W, 9 * S):
        dr.line([(k, 4 * S), (k, H - 4 * S)], fill=(255, 255, 255, 26), width=1 * S)
    return baja(im, 256, 56)


random.seed(7)
atlas.paste(lata(), (0, 0))
atlas.paste(foto(), (512, 0))
atlas.paste(celo(), (848, 0))
atlas.paste(chincheta(), (848, 80))
atlas.paste(grapa(), (936, 80))
atlas.paste(botella(), (512, 272))
atlas.paste(caja(), (680, 272))
atlas.paste(cubeta(), (680, 440))
atlas.paste(pinza(), (512, 576))
atlas.paste(washi((217, 51, 37)), (0, 512))
atlas.paste(washi((242, 199, 68)), (0, 568))
atlas.paste(washi((43, 59, 199)), (0, 624))
atlas.paste(washi((106, 130, 60)), (0, 680))
atlas.paste(chincheta((43, 59, 199)), (848, 168))
atlas.paste(chincheta((242, 199, 68)), (936, 168))

fuera = os.path.join(RAIZ, "assets", "doodles.png")
atlas.save(fuera)
print("→", fuera, atlas.size)
