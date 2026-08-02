// luts.js — la lutoteca: gelatinas de entrada (cámara→709) y de color (el alma).
// Parsea .cube en el navegador y las mete en el motor WebGL en caliente.

import { state, touch, emit } from "./state.js";

let pipe = null;
export function bindPipe(p) { pipe = p; }

const IDENT = new Float32Array([0,0,0, 1,0,0, 0,1,0, 1,1,0, 0,0,1, 1,0,1, 0,1,1, 1,1,1]);
const cache = new Map();

export async function listLuts() {
  return await (await fetch("/api/luts")).json();
}

async function loadCube(slot, name) {
  const key = slot + "/" + name;
  if (cache.has(key)) return cache.get(key);
  const text = await (await fetch(`/luts/${slot}/${encodeURIComponent(name)}`)).text();
  let n = 0;
  const vals = [];
  for (const line of text.split("\n")) {
    const l = line.trim();
    if (!l || l.startsWith("#")) continue;
    if (l.startsWith("LUT_3D_SIZE")) { n = parseInt(l.slice(11).trim()); continue; }
    if (/^[A-Za-z]/.test(l)) continue;
    for (const tok of l.split(/\s+/)) vals.push(parseFloat(tok));
  }
  if (!n || vals.length !== n * n * n * 3) throw new Error("cube inválido: " + name);
  const out = { n, data: new Float32Array(vals) };
  cache.set(key, out);
  return out;
}

/** pone una gelatina en su ranura (null = sin gelatina) y actualiza el motor */
export async function applyGel(slot, name) {
  if (slot === "entrada") state.lutEntrada = name;
  else state.lutColor = name;
  if (pipe) {
    try {
      if (name) {
        const { n, data } = await loadCube(slot, name);
        if (slot === "entrada") pipe.setInputLut(data, n);
        else pipe.setLut(data, n);
      } else {
        if (slot === "entrada") pipe.setInputLut(IDENT, 2);
        else pipe.setLut(IDENT, 2);
      }
    } catch (e) { console.error("gelatina", e); }
  }
  state.prefs.inputLutOn = !!state.lutEntrada;
  state.prefs.lutOn = !!state.lutColor;
  touch(); emit("prefs"); emit("luts");
}

/** carga las gelatinas del proyecto al arrancar */
export async function applyFromState() {
  await applyGel("entrada", state.lutEntrada);
  await applyGel("color", state.lutColor);
}

export function gelShortName(name) {
  return name ? name.replace(/\.cube$/i, "") : "— sin gelatina —";
}
