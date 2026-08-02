"""Extrae los parámetros Film Look Creator de un .drx y genera variantes.

Uso:
  python drx_tool.py params <in.drx> <out.json>   # todos los parámetros por nodo
  python drx_tool.py bake   <in.drx> <out.drx>    # versión "solo color" para hornear a LUT

El cuerpo del .drx es protobuf-ish comprimido con zstd (prefijo 0x81 + frame).
Cada parámetro:  0a <len> <nombre>  12 <len> <submsg>
  submsg = 11 <double LE>          (fixed64)
         | 18 <varint>             (bool/int)
         | 32 0a 0d <f32> 15 <f32> (wrapper fixed32, p.ej. filmGateRatio)
         | 2a <len> <cadena>       (enum/preset como string)
"""
import binascii
import json
import re
import struct
import sys
from pathlib import Path

import zstandard

ZSTD_MAGIC = bytes.fromhex("28b52ffd")

# Parámetros espaciales/temporales: NO se pueden hornear en una LUT (no son
# funciones puntuales de color). Se apagan para el bake y viven en el shader.
SPATIAL_BOOLS_OFF = [
    b"halationIsEnable", b"grainIsEnable", b"bloomIsEnable",
    b"gateWeaveIsEnable", b"flickerIsEnable", b"vignetteIsEnable",
    b"filmGateIsEnable", b"splitIsEnable",
]
SPATIAL_DOUBLES_ZERO = [
    b"halationAmount", b"grainAmount", b"bloomAmount", b"vignetteAmount",
    b"gateWeaveAmount", b"flickerAmount", b"softness",
]


def _bodies(txt: str):
    for m in re.finditer(r"<Body>(.*?)</Body>", txt, re.S):
        hexbody = m.group(1).strip()
        try:
            data = binascii.unhexlify(hexbody)
        except binascii.Error:
            continue
        i = data.find(ZSTD_MAGIC)
        if i < 0:
            continue
        try:
            dec = zstandard.ZstdDecompressor().decompressobj().decompress(data[i:])
        except Exception:
            continue
        yield hexbody, data[:i], dec


def _parse_params(dec: bytes) -> list[dict]:
    """Recorre el blob y devuelve [{'name','type','value','offset'} ...]."""
    out = []
    for m in re.finditer(rb"\x0a([\x04-\x40])([a-zA-Z][a-zA-Z0-9]{2,63})\x12", dec):
        name = m.group(2).decode()
        p = m.end()
        ln = dec[p]
        sub = dec[p + 1: p + 1 + ln]
        val, typ = None, None
        if sub[:1] == b"\x11" and ln == 9:
            val, typ = struct.unpack("<d", sub[1:9])[0], "double"
        elif sub[:1] == b"\x18":
            val, typ = sub[1], "bool" if sub[1] in (0, 1) else "int"
        elif sub[:3] == b"\x32\x0a\x0d" and ln >= 10:
            val, typ = struct.unpack("<f", sub[4:8])[0], "float32w"
        elif sub[:1] == b"\x2a":
            sl = sub[1]
            try:
                val, typ = sub[2:2 + sl].decode(), "string"
            except Exception:
                continue
        if typ:
            out.append({"name": name, "type": typ, "value": val, "offset": m.start()})
    return out


def _split_nodes(params: list[dict]) -> list[dict]:
    """Agrupa por aparición (cada nodo FilmLook repite la misma secuencia)."""
    nodes: list[dict] = []
    seen: set[str] = set()
    cur: dict = {}
    for p in params:
        if p["name"] in seen:  # vuelve el primer nombre: nuevo nodo
            nodes.append(cur)
            cur, seen = {}, set()
        seen.add(p["name"])
        cur[p["name"]] = p["value"]
    if cur:
        nodes.append(cur)
    return nodes


def cmd_params(src: Path, dst: Path):
    txt = src.read_bytes().decode("latin-1")
    all_nodes = []
    for _, _, dec in _bodies(txt):
        all_nodes.extend(_split_nodes(_parse_params(dec)))
    dst.write_text(json.dumps(all_nodes, indent=2, ensure_ascii=False))
    print(f"{len(all_nodes)} nodo(s) → {dst}")
    for i, n in enumerate(all_nodes):
        print(f"── nodo {i} ({len(n)} parámetros)")
        for k, v in n.items():
            print(f"   {k:28s} {v}")


def _patch(dec: bytes) -> tuple[bytes, int]:
    buf = bytearray(dec)
    n = 0
    for name in SPATIAL_BOOLS_OFF:
        pos = 0
        while True:
            k = buf.find(name, pos)
            if k < 0:
                break
            pos = k + 1
            j = bytes(buf[k:k + 24]).find(bytes.fromhex("120218"))
            if j < 0:
                continue
            off = k + j + 3
            if buf[off] == 1:
                buf[off] = 0
                n += 1
    for name in SPATIAL_DOUBLES_ZERO:
        pos = 0
        while True:
            k = buf.find(name, pos)
            if k < 0:
                break
            pos = k + 1
            j = bytes(buf[k:k + 24]).find(bytes.fromhex("120911"))
            if j < 0:
                continue
            off = k + j + 3
            old = struct.unpack("<d", buf[off:off + 8])[0]
            if old != 0.0:
                buf[off:off + 8] = struct.pack("<d", 0.0)
                n += 1
    return bytes(buf), n


def cmd_bake(src: Path, dst: Path):
    txt = src.read_bytes().decode("latin-1")
    out = txt
    total = 0
    for hexbody, prefix, dec in _bodies(txt):
        new, n = _patch(dec)
        if not n:
            continue
        total += n
        recomp = zstandard.ZstdCompressor(level=19).compress(new)
        out = out.replace(hexbody, (prefix + recomp).hex(), 1)
    dst.write_text(out, encoding="latin-1")
    print(f"{total} parámetros espaciales desactivados → {dst}")


if __name__ == "__main__":
    cmd, src, dst = sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3])
    {"params": cmd_params, "bake": cmd_bake}[cmd](src, dst)
