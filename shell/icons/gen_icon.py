#!/usr/bin/env python3
"""Icono de LABORATORIOS SAORÍN — composición zine impresa.

Papel hueso troquelado con sombra dura, tira de 35mm en ultramar con su
desregistro terracota, S rotunda en Arial Black. Dos tintas + hueso, grano
de papel, nada de degradados brillantes.
"""
import math
import os
import random

from PIL import Image, ImageDraw, ImageFilter, ImageFont, ImageOps

S = 1024
BONE = (242, 238, 228, 255)        # #f2eee4
INK = (43, 59, 199, 255)           # ultramar #2b3bc7
TERRA = (180, 90, 56, 255)         # terracota
SHADOW = (24, 28, 60, 255)         # sombra dura (tinta azul muy oscura)

random.seed(7)
HERE = os.path.dirname(os.path.abspath(__file__))


def rounded(draw, box, r, fill):
    draw.rounded_rectangle(box, radius=r, fill=fill)


def paper_layer():
    """papel hueso con grano y borde ligeramente cálido"""
    p = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(p)
    m = 92                     # margen del troquel
    rounded(d, (m, m, S - m, S - m), 150, BONE)
    # grano de papel: ruido suave multiplicado
    noise = Image.effect_noise((S, S), 22).convert("L")
    noise = noise.point(lambda v: 235 + (v - 128) // 8)
    grain = Image.merge("RGBA", [noise, noise, noise, Image.new("L", (S, S), 255)])
    p = Image.composite(Image.blend(p, Image.alpha_composite(p, grain), 0.35), p, p.split()[3])
    return p, m


def film_strip(w, h, ink):
    """tira de 35mm vertical: perforaciones y tres fotogramas huecos"""
    strip = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    d = ImageDraw.Draw(strip)
    d.rectangle((0, 0, w, h), fill=ink)
    # perforaciones (troquel = agujero de verdad: alpha 0)
    hole_w, hole_h, pitch = 46, 34, 96
    for y in range(28, h - 20, pitch):
        for x in (16, w - 16 - hole_w):
            d.rounded_rectangle((x, y, x + hole_w, y + hole_h), radius=10, fill=(0, 0, 0, 0))
    # fotogramas huecos
    fx0, fx1 = 92, w - 92
    fh, gap = 236, 42
    y = 54
    while y + fh < h - 30:
        d.rounded_rectangle((fx0, y, fx1, y + fh), radius=14, fill=(0, 0, 0, 0))
        y += fh + gap
    return strip


def main():
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))

    paper, m = paper_layer()
    # sombra dura del troquel (offset, sin blur: impresión, no CSS)
    shadow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    sd = ImageDraw.Draw(shadow)
    rounded(sd, (m, m, S - m, S - m), 150, SHADOW)
    paper_rot = paper.rotate(-2.2, resample=Image.BICUBIC, expand=False)
    shadow_rot = shadow.rotate(-2.2, resample=Image.BICUBIC, expand=False)
    img.alpha_composite(shadow_rot, (16, 20))
    img.alpha_composite(paper_rot, (0, 0))

    # tira de película: terracota desregistrada debajo, ultramar encima
    sw, sh = 320, 900
    strip_t = film_strip(sw, sh, TERRA).rotate(-7, resample=Image.BICUBIC, expand=True)
    strip_i = film_strip(sw, sh, INK).rotate(-7, resample=Image.BICUBIC, expand=True)
    sx, sy = 560, 40
    img.alpha_composite(strip_t, (sx - 12, sy + 12))
    img.alpha_composite(strip_i, (sx, sy))

    # S rotunda: terracota desregistrada + ultramar
    font = ImageFont.truetype("/System/Library/Fonts/Supplemental/Arial Black.ttf", 620)
    txt = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    td = ImageDraw.Draw(txt)
    td.text((150, 190), "S", font=font, fill=TERRA)
    img.alpha_composite(txt.rotate(0), (14, 12))
    txt2 = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    td2 = ImageDraw.Draw(txt2)
    td2.text((150, 190), "S", font=font, fill=INK)
    img.alpha_composite(txt2, (0, 0))

    # recorta la tinta que se sale del papel (el troquel manda)
    mask = paper_rot.split()[3].point(lambda v: 255 if v > 8 else 0)
    cut = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    cut.paste(img, (0, 0))
    inked = Image.composite(img, Image.new("RGBA", (S, S), (0, 0, 0, 0)), mask)
    # …pero la sombra vive fuera del papel
    final = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    final.alpha_composite(shadow_rot, (16, 20))
    final.alpha_composite(inked, (0, 0))

    final.save(os.path.join(HERE, "icon.png"))
    print("icon.png listo")


if __name__ == "__main__":
    main()
