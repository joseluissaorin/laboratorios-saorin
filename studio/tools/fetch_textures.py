#!/usr/bin/env python3
"""Materia real para Laboratorios Saorín: descarga escaneos con licencia libre
(dominio público / CC0 / CC-BY) desde Wikimedia Commons, y deja constancia de
cada crédito en assets/CREDITS.txt (el proceso se nombra — ética del hacedor).
"""

import json
import os
import re
import subprocess
import shutil
import urllib.parse
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "assets", "matter")
os.makedirs(OUT, exist_ok=True)
FFMPEG = shutil.which("ffmpeg") or r"C:\ProgramData\chocolatey\bin\ffmpeg.exe"

UA = {"User-Agent": "LaboratoriosSaorin/1.0 (uso editorial)"}
OK_LICENSES = re.compile(r"(public domain|pd-|cc0|cc-by(?!-nc)(?!-nd))", re.I)

WANTED = [
    # (nombre destino, término de búsqueda, nº candidatos a mirar)
    ("paper_scan", "old paper texture scan", 8),
    ("cardboard", "cardboard texture scan", 8),
    ("film_strip", "35mm film strip scan", 8),
    ("film_can", "film reel metal can", 8),
    ("gauge_face", "vintage galvanometer dial", 8),
    ("masking_tape", "masking tape texture", 8),
]

def api(params):
    url = "https://commons.wikimedia.org/w/api.php?" + urllib.parse.urlencode(params)
    req = urllib.request.Request(url, headers=UA)
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.load(r)

def search(term, n):
    d = api({
        "action": "query", "format": "json",
        "generator": "search", "gsrsearch": f"filetype:bitmap {term}",
        "gsrnamespace": 6, "gsrlimit": n,
        "prop": "imageinfo",
        "iiprop": "url|extmetadata|size",
        "iiurlwidth": 1600,
    })
    return list(d.get("query", {}).get("pages", {}).values())

credits = []
for dest, term, n in WANTED:
    try:
        pages = search(term, n)
    except Exception as e:
        print(f"× {dest}: búsqueda falló ({e})")
        continue
    got = False
    for p in sorted(pages, key=lambda x: -x.get("imageinfo", [{}])[0].get("width", 0)):
        ii = p.get("imageinfo", [{}])[0]
        meta = ii.get("extmetadata", {})
        lic = (meta.get("LicenseShortName", {}).get("value", "") + " " +
               meta.get("License", {}).get("value", ""))
        if not OK_LICENSES.search(lic):
            continue
        if ii.get("width", 0) < 800:
            continue
        url = ii.get("thumburl") or ii.get("url")
        try:
            raw = os.path.join(OUT, dest + ".src")
            req = urllib.request.Request(url, headers=UA)
            with urllib.request.urlopen(req, timeout=60) as r, open(raw, "wb") as f:
                f.write(r.read())
            dst = os.path.join(OUT, dest + ".jpg")
            subprocess.run([FFMPEG, "-hide_banner", "-loglevel", "error", "-y",
                            "-i", raw, "-vf", "scale='min(1600,iw)':-2", "-q:v", "4", dst],
                           check=True, timeout=120)
            os.remove(raw)
            artist = re.sub(r"<[^>]+>", "", meta.get("Artist", {}).get("value", "desconocido")).strip()
            credits.append(f"{dest}.jpg — «{p.get('title','')}» · {artist} · {lic.strip()} · commons.wikimedia.org")
            print(f"✓ {dest}: {p.get('title','')} [{lic.strip()}]")
            got = True
            break
        except Exception as e:
            continue
    if not got:
        print(f"× {dest}: sin candidato con licencia libre")

with open(os.path.join(OUT, "CREDITS.txt"), "w") as f:
    f.write("Materia real de Laboratorios Saorín — créditos\n" + "=" * 46 + "\n")
    f.write("\n".join(credits) + "\n")
print(f"\n{len(credits)} texturas · créditos en assets/matter/CREDITS.txt")
