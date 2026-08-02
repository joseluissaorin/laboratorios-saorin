// state.js — modelo del proyecto: media, timeline (ripple, sin huecos), prefs.

export const DEFAULT_PREFS = {
  gain: 0.1, pushPull: 0, compImpact: 1.35, compWP: 1, compRange: 0.36,
  shutter: 0.143, grain: 0.13, grainSize: 5, grainRough: 0.47, grainChroma: 0,
  grainDefocus: 0.3, grainShadows: 0.7, grainMids: 1, grainHighs: 0.61,
  grainRed: 1.35, grainBlue: 1.3, filmRes: 1,
  halation: 1.5, halHue: 1, halSat: 0.9, halThr: 0.8, halSpread: 0.6, halWhite: 0.1,
  bloom: 0.6, bloomThr: 0.8, bloomWarm: 0.3,
  softness: 0.1, acutance: 0.11, colorSep: 0.03,
  hueSkew: 0.96, crosstalk: 1, subtractive: 1, stockSat: 1.15, print: 0.06,
  vignette: 0, vigSize: 0.55, vigRound: 1, vigCX: 0.5, vigCY: 0.5,
  chroma: 0, weave: 0.15, weaveRot: 0.3, flicker: 0, breath: 0, breathRate: 0.5,
  dust: 0, frameInset: 0, frameCorner: 40, frameWobble: 1,
  inputLutOn: true, lutOn: true, wipe: 1, resetHistory: false,
  yuvNorm: 65535, fullRange: false,
};

const listeners = {};
export function on(ev, fn) { (listeners[ev] ||= []).push(fn); }
export function emit(ev, ...a) { (listeners[ev] || []).forEach((f) => f(...a)); }

export const state = {
  media: [],                 // [{name,url,dur,fps,w,h,kind}]  kind: "video"|"audio"
  clips: [],                 // [{id, media, in, out, fade?, gain?, mute?, fadeIn?, fadeOut?}]
  audio: [],                 // la pista de audio: [{id, media, in, out, start, gain, mute, fadeIn, fadeOut}]
  markers: [],               // banderitas: [{id, t, name}]
  bin: [],                   // el saco de recortes: [{media, in, out}] — el undo físico
  prefs: { ...DEFAULT_PREFS },
  lutEntrada: "Directo · sin transformar.cube",   // gelatina de entrada por defecto
  lutColor: "Saorín · 65 puntos.cube",              // gelatina de color por defecto
  project: { aspect: "auto", fps: 0 },   // ajustes del proyecto (auto = del primer clip)
  sel: null,                 // id del clip de VÍDEO seleccionado
  selSet: [],                // multi-selección (⇧clic) de clips de vídeo
  range: null,               // {a, b} — el tramo marcado con I/O sobre la bobina
  loop: false,               // reproducir en bucle
  selAudio: null,            // id del clip de AUDIO seleccionado
  t: 0,                      // playhead en segundos de timeline
  playing: false,
  dirty: false,
  room: "mesa",
  // el monitor de FUENTE: una cinta en el proyector sin pasar por la bobina
  source: null,              // {media, t, in, out, playing} | null
};

/* ── la historia: deshacer/rehacer de la bobina ── */
const past = [];
const future = [];

/** llama ANTES de cada gesto que muta la bobina */
export function snapshot() {
  past.push(JSON.stringify({ clips: state.clips, bin: state.bin, audio: state.audio, markers: state.markers }));
  if (past.length > 80) past.shift();
  future.length = 0;
}

/* gesto perezoso: solo consume un paso de historia si algo cambió de verdad,
   y permite cancelar (Esc) devolviendo la bobina a como estaba */
let gesturePre = null;
export function beginGesture() {
  gesturePre = JSON.stringify({ clips: state.clips, bin: state.bin, audio: state.audio, markers: state.markers });
}
export function commitGesture() {
  if (gesturePre === null) return;
  const now = JSON.stringify({ clips: state.clips, bin: state.bin, audio: state.audio, markers: state.markers });
  if (now !== gesturePre) {
    past.push(gesturePre);
    if (past.length > 80) past.shift();
    future.length = 0;
  }
  gesturePre = null;
}
export function cancelGesture() {
  if (gesturePre === null) return false;
  const p = JSON.parse(gesturePre);
  state.clips = p.clips;
  state.bin = p.bin;
  state.audio = p.audio || [];
  state.markers = p.markers || [];
  gesturePre = null;
  touch(); emit("timeline"); emit("bin");
  return true;
}

export function newId() { return nextId++; }

function restore(json) {
  const p = JSON.parse(json);
  state.clips = p.clips;
  state.bin = p.bin;
  state.audio = p.audio || [];
  state.markers = p.markers || [];
  state.sel = null;
  state.selAudio = null;
  touch(); emit("timeline"); emit("bin");
}

export function undo() {
  if (!past.length) return false;
  future.push(JSON.stringify({ clips: state.clips, bin: state.bin, audio: state.audio, markers: state.markers }));
  restore(past.pop());
  return true;
}

export function redo() {
  if (!future.length) return false;
  past.push(JSON.stringify({ clips: state.clips, bin: state.bin, audio: state.audio, markers: state.markers }));
  restore(future.pop());
  return true;
}

let nextId = 1;
export function addClip(mediaName, at = null) {
  const m = state.media.find((x) => x.name === mediaName);
  if (!m) return;
  snapshot();
  // una foto fija entra con 4 s de cuerda (luego se recorta como cualquiera)
  const clip = { id: nextId++, media: m.name, in: 0, out: m.dur > 0 ? m.dur : 4 };
  if (at === null) state.clips.push(clip);
  else state.clips.splice(at, 0, clip);
  touch(); emit("timeline");
  return clip;
}

/** un hueco: negro con silencio, del largo que se quiera */
export function insertGapAt(index, dur = 2) {
  snapshot();
  const g = { id: nextId++, gap: true, dur };
  state.clips.splice(Math.max(0, Math.min(index, state.clips.length)), 0, g);
  touch(); emit("timeline");
  return g;
}

/** lift: el clip se va pero deja su hueco (la bobina no se encoge) */
export function liftClip(id) {
  const i = state.clips.findIndex((x) => x.id === id);
  if (i < 0) return;
  snapshot();
  const c = state.clips[i];
  if (!c.gap) state.bin.unshift({ media: c.media, in: c.in, out: c.out });
  if (state.bin.length > 24) state.bin.pop();
  state.clips.splice(i, 1, { id: nextId++, gap: true, dur: clipDur(c) });
  if (state.sel === id) state.sel = null;
  touch(); emit("timeline"); emit("bin");
}

/** quitar = colgar el recorte en el saco (por si acaso) */
export function removeClip(id) {
  snapshot();
  const c = state.clips.find((x) => x.id === id);
  if (c && !c.gap) state.bin.unshift({ media: c.media, in: c.in, out: c.out });
  if (state.bin.length > 24) state.bin.pop();
  state.clips = state.clips.filter((x) => x.id !== id);
  if (state.sel === id) state.sel = null;
  touch(); emit("timeline"); emit("bin");
}

/** descolgar un recorte del saco y devolverlo a la bobina */
export function rescueFromBin(idx) {
  snapshot();
  const r = state.bin.splice(idx, 1)[0];
  if (!r) return;
  addClipRaw(r.media, r.in, r.out);
  emit("bin");
}

export function addClipRaw(media, in_, out) {
  const clip = { id: nextId++, media, in: in_, out };
  state.clips.push(clip);
  touch(); emit("timeline");
  return clip;
}

export function clipDur(c) {
  if (c.gap) return Math.max(c.dur || 1, 0.2);
  return Math.max((c.out - c.in) / (c.speed || 1), 1 / 60);
}

export function mediaKind(c) {
  if (c.gap) return "gap";
  const m = state.media.find((x) => x.name === c.media);
  return m?.kind || "video";
}

/* ── los ajustes del proyecto: aspecto, resolución y fps ──
   "auto" = tomados del PRIMER clip (el usuario no tiene por qué saber qué es
   1080p29.97). El aspecto es ciudadano de primera: presets por destino. */

export const ASPECTS = {
  "auto": null,
  "16:9": 16 / 9,
  "9:16": 9 / 16,
  "1:1": 1,
  "4:5": 4 / 5,
  "2.39": 2.39,
};

function firstClipMedia() {
  const c = state.clips[0];
  return c ? state.media.find((m) => m.name === c.media) : null;
}

export function projFps() {
  if (state.project.fps > 0) return state.project.fps;
  const m = firstClipMedia();
  return (m && m.fps) || 25;
}

export function projDims() {
  const m = firstClipMedia();
  const ar = ASPECTS[state.project.aspect || "auto"];
  if (ar === null || ar === undefined) {
    if (m && m.w) return { w: m.w, h: m.h };
    return { w: 1920, h: 1080 };
  }
  // clase de calidad según el material: 4K si el primer clip lo es
  const big = m && Math.max(m.w || 0, m.h || 0) >= 2000;
  const base = big ? 2160 : 1080;      // el lado corto de la clase
  let w, h;
  if (ar >= 1) { h = base; w = Math.round(base * ar); }
  else { w = base; h = Math.round(base / ar); }
  return { w: w & ~1, h: h & ~1 };
}

/** cuantiza a la rejilla de frames del proyecto */
export function snapT(t) {
  const f = projFps();
  return Math.round(t * f) / f;
}

/* ── las marcas: banderitas con nombre sobre la regla ── */

export function addMarker(t, name = null) {
  snapshot();
  const m = { id: nextId++, t: snapT(t), name: name || `marca ${state.markers.length + 1}` };
  state.markers.push(m);
  state.markers.sort((a, b) => a.t - b.t);
  touch(); emit("timeline");
  return m;
}

export function removeMarker(id) {
  snapshot();
  state.markers = state.markers.filter((m) => m.id !== id);
  touch(); emit("timeline");
}

/** puntos de navegación: cortes (o marcas) anteriores/siguientes a t */
export function jumpPoints(marks = false) {
  if (marks) return state.markers.map((m) => m.t);
  const pts = [0];
  for (const it of layout()) pts.push(it.end);
  return pts;
}

export function nextPoint(t, marks = false) {
  const pts = jumpPoints(marks).filter((p) => p > t + 0.001);
  return pts.length ? Math.min(...pts) : null;
}
export function prevPoint(t, marks = false) {
  const pts = jumpPoints(marks).filter((p) => p < t - 0.001);
  return pts.length ? Math.max(...pts) : null;
}

/* ── portapapeles de clips (copiar/cortar/pegar/duplicar) ── */

let clipboard = null;   // {kind: "video"|"audio", data: {...}}

export function copySelected() {
  const s = selected();
  if (!s) return false;
  const { id, ...data } = s.clip;
  clipboard = { kind: s.kind, data: JSON.parse(JSON.stringify(data)) };
  return true;
}

export function pasteClipboard() {
  if (!clipboard) return false;
  snapshot();
  if (clipboard.kind === "video") {
    const c = { id: nextId++, ...JSON.parse(JSON.stringify(clipboard.data)) };
    const i = state.clips.findIndex((x) => x.id === state.sel);
    state.clips.splice(i >= 0 ? i + 1 : state.clips.length, 0, c);
    state.sel = c.id; state.selAudio = null;
  } else {
    const a = { id: nextId++, ...JSON.parse(JSON.stringify(clipboard.data)), start: Math.max(0, state.t) };
    state.audio.push(a);
    state.selAudio = a.id; state.sel = null;
  }
  touch(); emit("timeline");
  return true;
}

export function duplicateSelected() {
  return copySelected() && pasteClipboard();
}

/** separar el audio del clip de vídeo: nace en la pista de sonido, el vídeo calla */
export function detachAudio(clipId) {
  const c = state.clips.find((x) => x.id === clipId);
  if (!c) return;
  const it = layout().find((x) => x.clip === c);
  snapshot();
  const a = { id: nextId++, media: c.media, in: c.in, out: c.out,
              start: it ? it.start : 0, gain: c.gain || 0, mute: false,
              fadeIn: c.fadeIn || 0, fadeOut: c.fadeOut || 0 };
  state.audio.push(a);
  c.mute = true;
  state.selAudio = a.id; state.sel = null;
  touch(); emit("timeline");
}

/* ── la pista de audio: música/voz bajo la bobina, con posición libre ── */

export function addAudioAt(mediaName, at) {
  const m = state.media.find((x) => x.name === mediaName);
  if (!m) return;
  snapshot();
  const a = { id: nextId++, media: m.name, in: 0, out: m.dur,
              start: Math.max(0, at), gain: 0, mute: false, fadeIn: 0, fadeOut: 0 };
  state.audio.push(a);
  state.selAudio = a.id;
  state.sel = null;
  touch(); emit("timeline");
  return a;
}

export function removeAudioClip(id) {
  snapshot();
  state.audio = state.audio.filter((a) => a.id !== id);
  if (state.selAudio === id) state.selAudio = null;
  touch(); emit("timeline");
}

/** clips de audio sonando en el tiempo t de timeline → [{a, offset}] */
export function audioActiveAt(t) {
  const out = [];
  for (const a of state.audio) {
    const dur = Math.max(a.out - a.in, 0.01);
    if (t >= a.start && t < a.start + dur) out.push({ a, offset: t - a.start });
  }
  return out;
}

/** el clip de vídeo o audio seleccionado (para el inspector) */
export function selected() {
  if (state.sel != null) {
    const c = state.clips.find((x) => x.id === state.sel);
    if (c) return { kind: "video", clip: c };
  }
  if (state.selAudio != null) {
    const a = state.audio.find((x) => x.id === state.selAudio);
    if (a) return { kind: "audio", clip: a };
  }
  return null;
}

/** posiciones ripple: [{clip, start, end}] sin huecos */
export function layout() {
  let t = 0;
  return state.clips.map((c) => {
    const item = { clip: c, start: t, end: t + clipDur(c) };
    t = item.end;
    return item;
  });
}

export function totalDur() {
  return state.clips.reduce((a, c) => a + clipDur(c), 0);
}

/** clip bajo el tiempo t de timeline → {clip, start, offset} */
export function clipAt(t) {
  for (const it of layout()) {
    if (t < it.end || it === layout().at(-1)) {
      if (t >= it.start) return { ...it, offset: t - it.start };
    }
  }
  return null;
}

export function splitAt(t) {
  t = snapT(t);
  const hit = clipAt(t);
  if (!hit || hit.offset < 0.04 || hit.offset > clipDur(hit.clip) - 0.04) return;
  snapshot();
  const c = hit.clip;
  const i = state.clips.indexOf(c);
  if (c.gap) {
    const right = { id: nextId++, gap: true, dur: clipDur(c) - hit.offset };
    c.dur = hit.offset;
    state.clips.splice(i + 1, 0, right);
  } else {
    const cutSrc = c.in + hit.offset * (c.speed || 1);
    const right = { id: nextId++, media: c.media, in: cutSrc, out: c.out };
    if (c.speed) right.speed = c.speed;
    c.out = cutSrc;
    state.clips.splice(i + 1, 0, right);
  }
  touch(); emit("timeline");
}

export function moveClip(id, toIndex) {
  const i = state.clips.findIndex((c) => c.id === id);
  if (i < 0) return;
  const [c] = state.clips.splice(i, 1);
  state.clips.splice(Math.max(0, Math.min(toIndex, state.clips.length)), 0, c);
  touch(); emit("timeline");
}

export function setPref(k, v) {
  state.prefs[k] = v;
  touch(); emit("prefs", k, v);
}

export function touch() {
  state.dirty = true;
  emit("dirty");
  scheduleSave();
}

let saveTimer = null;
function scheduleSave() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(save, 800);
}

export async function save() {
  const body = {
    v: 1,   // versión del formato de proyecto (desde el día uno)
    clips: state.clips, audio: state.audio, bin: state.bin, prefs: state.prefs,
    project: state.project, markers: state.markers, range: state.range,
    lutEntrada: state.lutEntrada, lutColor: state.lutColor,
    nextId,
  };
  await fetch("/api/project", { method: "POST", body: JSON.stringify(body) });
  state.dirty = false;
  emit("dirty");
}

export async function reloadMedia() {
  state.media = await (await fetch("/api/media")).json();
  emit("media");
}

export async function load() {
  state.media = await (await fetch("/api/media")).json();
  const p = await (await fetch("/api/project")).json();
  if (p) {
    state.clips = (p.clips || []).filter((c) => c.gap || state.media.some((m) => m.name === c.media));
    state.audio = (p.audio || []).filter((a) => state.media.some((m) => m.name === a.media));
    if (p.project) state.project = { aspect: "auto", fps: 0, ...p.project };
    state.markers = p.markers || [];
    state.bin = p.bin || [];
    state.prefs = { ...DEFAULT_PREFS, ...(p.prefs || {}) };
    if ("lutEntrada" in p) state.lutEntrada = p.lutEntrada;
    if ("lutColor" in p) state.lutColor = p.lutColor;
    nextId = p.nextId ||
      (Math.max(0, ...state.clips.map((c) => c.id), ...state.audio.map((a) => a.id)) + 1);
  }
  emit("media"); emit("timeline"); emit("prefs"); emit("bin");
}
