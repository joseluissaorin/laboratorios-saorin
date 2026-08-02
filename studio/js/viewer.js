// viewer.js — la moviola (mesa) y la ampliadora (cuarto oscuro): el mismo
// vidrio, dos salas. Motor WebGL del lab + WebCodecs. Con manivela de verdad.

import { Pipeline } from "/engine/js/pipeline.js";
import { ClipDecoder } from "./engine-decode.js";
import { state, on, clipAt, totalDur, emit, addClipRaw, clipDur, layout, audioActiveAt,
         projFps, projDims, mediaKind, splitAt, newId, snapshot, touch } from "./state.js";
import { bindPipe } from "./luts.js";
import * as foley from "./foley.js";

const canvas = document.getElementById("viewer");
const overlay = document.getElementById("viewer-overlay");
const fpsEl = document.getElementById("viewer-fps");

let pipe = null;
let decoders = new Map();          // másteres YA enhebrados (media → ClipDecoder)
const masterLoading = new Map();   // másteres enhebrándose en segundo plano
// proxies all-intra para scrub Y reproducción instantánea
const proxyReady = new Map();      // media → url
let proxyDecoders = new Map();     // proxies YA abiertos
const proxyLoading = new Map();
let scrubFast = false;
export function setScrubFast(v) { scrubFast = v; }

async function pollProxies() {
  for (const m of state.media) {
    if (proxyReady.has(m.name)) continue;
    try {
      const r = await (await fetch(`/api/proxy?f=${encodeURIComponent(m.name)}`)).json();
      if (r.ready) proxyReady.set(m.name, r.url);
    } catch {}
  }
  if (proxyReady.size < state.media.length) setTimeout(pollProxies, 3000);
}

/* ── LA REGLA DE ORO: abrir un decodificador JAMÁS bloquea la vista ──
   El máster (moov en la cola en los MP4 de cámara: hay que tragarse el
   fichero entero) se enhebra en segundo plano. El proxy es pequeño y es el
   caballo de batalla. Mientras nada esté listo, el póster (miniatura)
   responde al instante. */

function ensureMaster(mediaName) {
  if (decoders.has(mediaName) || masterLoading.has(mediaName)) return;
  const m = state.media.find((x) => x.name === mediaName);
  if (!m || m.missing || (m.kind && m.kind !== "video")) return;
  const p = new ClipDecoder().open(m.url).then((d) => {
    masterLoading.delete(mediaName);
    decoders.set(mediaName, d);
    if (decoders.size > 3) {
      const [k, old] = decoders.entries().next().value;
      old.close(); decoders.delete(k);
    }
    // el máster acaba de llegar: si estamos en pausa sobre él, repintar fino
    if (!state.playing) showFrameAt(state.t);
    return d;
  }).catch(() => { masterLoading.delete(mediaName); });
  masterLoading.set(mediaName, p);
}

function ensureProxyDec(mediaName) {
  if (proxyDecoders.has(mediaName) || proxyLoading.has(mediaName)) return;
  const url = proxyReady.get(mediaName);
  if (!url) return;
  const p = new ClipDecoder().open(url).then((d) => {
    proxyLoading.delete(mediaName);
    proxyDecoders.set(mediaName, d);
    if (proxyDecoders.size > 4) {
      const [oldName, old] = proxyDecoders.entries().next().value;
      old.close(); proxyDecoders.delete(oldName);
    }
    if (!state.playing) showFrameAt(state.t);
    return d;
  }).catch(() => { proxyLoading.delete(mediaName); });
  proxyLoading.set(mediaName, p);
}

/** el mejor decodificador DISPONIBLE YA (sin esperar): máster > proxy > nada */
function bestDecoder(mediaName, preferProxy = false) {
  ensureProxyDec(mediaName);
  ensureMaster(mediaName);
  if (preferProxy) {
    return proxyDecoders.get(mediaName) || decoders.get(mediaName) || null;
  }
  return decoders.get(mediaName) || proxyDecoders.get(mediaName) || null;
}
/* ── el sonido de la moviola: sidecars m4a sincronizados con la aguja ── */
const audioPool = new Map();   // "v<id>"|"a<id>"|"src" → HTMLAudioElement
const activeAudio = new Set();

function audioFor(key, media) {
  if (audioPool.has(key)) {
    const e = audioPool.get(key);
    audioPool.delete(key); audioPool.set(key, e);
    return e;
  }
  const el = new Audio(`/api/audio?f=${encodeURIComponent(media)}`);
  el.preload = "auto";
  audioPool.set(key, el);
  if (audioPool.size > 8) {
    const [k, old] = audioPool.entries().next().value;
    old.pause(); old.removeAttribute("src");
    audioPool.delete(k); activeAudio.delete(k);
  }
  return el;
}

/** volumen de un clip en su offset: ganancia (dB) × envolvente de fundidos */
function volOf(c, offT, dur) {
  if (c.mute) return 0;
  let db = c.gain || 0;
  if (c.env && c.env.length >= 2) {
    // la banda elástica: interpolación lineal en dB entre puntos {t, db}
    const pts = c.env;
    if (offT <= pts[0].t) db = pts[0].db;
    else if (offT >= pts[pts.length - 1].t) db = pts[pts.length - 1].db;
    else {
      for (let i2 = 0; i2 < pts.length - 1; i2++) {
        if (offT >= pts[i2].t && offT <= pts[i2 + 1].t) {
          const f2 = (offT - pts[i2].t) / Math.max(pts[i2 + 1].t - pts[i2].t, 0.001);
          db = pts[i2].db + (pts[i2 + 1].db - pts[i2].db) * f2;
          break;
        }
      }
    }
  }
  let v = Math.pow(10, db / 20);
  const fi = c.fadeIn || 0, fo = c.fadeOut || 0;
  if (fi > 0.005) v *= Math.min(1, Math.max(0, offT / fi));
  if (fo > 0.005) v *= Math.min(1, Math.max(0, (dur - offT) / fo));
  return Math.min(1, Math.max(0, v));
}

function syncOne(key, media, desired, vol, rate2 = 1) {
  const el = audioFor(key, media);
  el.volume = vol;
  if (el.playbackRate !== rate2) { try { el.playbackRate = rate2; } catch {} }
  if (el.paused) {
    try { el.currentTime = desired; el.play().catch(() => {}); } catch {}
  } else if (Math.abs(el.currentTime - desired) > 0.12) {
    try { el.currentTime = desired; } catch {}
  }
  return key;
}

/** por frame durante la reproducción: qué suena y a qué volumen */
function syncAudio(t) {
  const wanted = new Set();
  const hit = clipAt(t);
  if (hit && mediaKind(hit.clip) === "video") {
    const spd = hit.clip.speed || 1;
    wanted.add(syncOne("v" + hit.clip.id, hit.clip.media,
      hit.clip.in + hit.offset * spd, volOf(hit.clip, hit.offset, clipDur(hit.clip)), spd));
    // pre-roll de la junta: el audio del siguiente clip ya enhebrado
    const items = layout();
    const i = items.findIndex((x) => x.clip === hit.clip);
    if (i >= 0 && i < items.length - 1 && items[i].end - t < 1.0) {
      const nx = items[i + 1].clip;
      const el = audioFor("v" + nx.id, nx.media);
      if (el.paused && Math.abs(el.currentTime - nx.in) > 0.2) {
        try { el.currentTime = nx.in; } catch {}
      }
    }
  }
  for (const { a, offset } of audioActiveAt(t)) {
    wanted.add(syncOne("a" + a.id, a.media,
      a.in + offset, volOf(a, offset, Math.max(a.out - a.in, 0.01))));
  }
  for (const key of [...activeAudio]) {
    if (!wanted.has(key)) {
      audioPool.get(key)?.pause();
      activeAudio.delete(key);
    }
  }
  for (const k of wanted) activeAudio.add(k);
}

export function stopAudio() {
  for (const el of audioPool.values()) el.pause();
  activeAudio.clear();
}

/* el scrub suena: un mordisco de ~90 ms al arrastrar la aguja (estilo cinta) */
let blipTimer = null;
let lastBlip = 0;
export function scrubBlip(t) {
  const now = performance.now();
  if (now - lastBlip < 70) return;
  lastBlip = now;
  const hit = clipAt(t);
  if (!hit) return;
  const el = audioFor("v" + hit.clip.id, hit.clip.media);
  try {
    el.currentTime = hit.clip.in + hit.offset;
    el.volume = volOf(hit.clip, hit.offset, clipDur(hit.clip)) * 0.9;
    el.play().catch(() => {});
    clearTimeout(blipTimer);
    blipTimer = setTimeout(() => el.pause(), 90);
  } catch {}
}

let current = null;
let lastFrame = null;
let frameIdx = 0;
let wallStart = 0, tStart = 0;
let fpsAvg = 0, lastTick = 0;
let wipeOn = false;

export async function initViewer() {
  pipe = new Pipeline(canvas, null, 2);
  pipe.setQuality(0.5);
  bindPipe(pipe);
  try {
    const gMeta = await (await fetch("/engine/assets/grain.json")).json();
    const gBuf = new Uint16Array(await (await fetch("/engine/assets/grain.bin")).arrayBuffer());
    pipe.setGrainPlate(gBuf, gMeta.size);
  } catch (e) { console.warn("placa de grano:", e); }

  on("timeline", () => {
    if (state.playing) return;   // la proyección no se interrumpe por un autosave
    current = null;
    showFrameAt(state.t);
  });
  on("prefs", () => { if (!state.playing) repaint(); });
  on("media", pollProxies);
  pollProxies();
  initCrank();
  requestAnimationFrame(tick);
}

/** mudanza del vidrio entre salas */
export function moveCanvasTo(el) {
  el.appendChild(canvas);
}
export function canvasHome() {
  document.querySelector(".moviola-marco").insertBefore(canvas, document.querySelector(".esquinas"));
}

/** SOLO para la fuente: espera al mejor decodificador, con póster mientras */
async function decoderFor(mediaName) {
  const meta = state.media.find((x) => x.name === mediaName);
  if (meta?.missing) {
    overlay.classList.remove("hidden");
    overlay.textContent = "«" + mediaName + "» está offline — reconecta su disco";
    throw new Error("media offline");
  }
  const ya = bestDecoder(mediaName);
  if (ya) return ya;
  // aún no hay nada abierto: esperar al primero que llegue (proxy o máster)
  const waits = [];
  if (proxyLoading.has(mediaName)) waits.push(proxyLoading.get(mediaName));
  if (masterLoading.has(mediaName)) waits.push(masterLoading.get(mediaName));
  if (!waits.length) throw new Error("sin decodificador");
  await Promise.race(waits);
  const d = bestDecoder(mediaName);
  if (!d) throw new Error("sin decodificador");
  return d;
}

/* el encuadre por clip (escala/giro/posición/encaje) compuesto a lienzo de
   proyecto: la preview enseña EXACTAMENTE lo que saldrá del revelado */
let compCanvas = null, compCtx = null;
function composeFrame(f, clip) {
  const { w: PW, h: PH } = projDims();
  const tf = clip.tf || {};
  const s = tf.scale || 1;
  const rot = ((tf.rot || 0) * Math.PI) / 180;
  const fill = tf.fit === "fill";
  const iw = f.displayWidth, ih = f.displayHeight;
  const identity = !fill && s === 1 && !rot && !tf.x && !tf.y && iw === PW && ih === PH;
  if (identity) return f;                        // camino rápido: cero copias
  const cw = Math.max(2, Math.round(PW * quality)), ch = Math.max(2, Math.round(PH * quality));
  if (!compCanvas) {
    compCanvas = document.createElement("canvas");
    compCtx = compCanvas.getContext("2d");
  }
  if (compCanvas.width !== cw || compCanvas.height !== ch) {
    compCanvas.width = cw; compCanvas.height = ch;
  }
  const g = compCtx;
  g.setTransform(1, 0, 0, 1, 0, 0);
  g.fillStyle = "#000";
  g.fillRect(0, 0, cw, ch);
  const base = fill ? Math.max(cw / iw, ch / ih) : Math.min(cw / iw, ch / ih);
  const k = base * s;
  g.translate(cw / 2 + (tf.x || 0) * cw, ch / 2 + (tf.y || 0) * ch);
  g.rotate(rot);
  try { g.drawImage(f, (-iw * k) / 2, (-ih * k) / 2, iw * k, ih * k); } catch {}
  let nf;
  try { nf = new VideoFrame(compCanvas, { timestamp: f.timestamp || 0 }); }
  catch { return f; }
  f.close();
  return nf;
}

let quality = 0.5;
export function setPreviewQuality(q) {
  quality = q;
  if (pipe) pipe.setQuality(q);
  showFrameAt(state.t);
}

function fitCanvas(w, h) {
  const cw = Math.round(w * quality), ch = Math.round(h * quality);
  if (canvas.width !== cw || canvas.height !== ch) {
    canvas.width = cw; canvas.height = ch;
  }
}

function P() {
  return { ...state.prefs, wipe: wipeOn ? 0.5 : 1.0 };
}

export function setWipe(v) { wipeOn = v; repaint(); }

function repaint() {
  if (!lastFrame || !pipe) return;
  pipe.setSourceFrame(lastFrame);
  pipe.render(P(), state.t, frameIdx % 997);
}

const POSTER = new URLSearchParams(location.search).has("poster");

/** fotograma desde la miniatura, por la MISMA cadena WebGL (fallback sin WebCodecs) */
async function posterFrame(media, srcT) {
  const img = new Image();
  img.src = `/api/thumb?f=${encodeURIComponent(media)}&t=${Math.max(0.2, srcT).toFixed(1)}`;
  await img.decode();
  return new VideoFrame(img, { timestamp: 0 });
}

let paintSeq = 0;
export async function showFrameAt(t) {
  const hit = clipAt(t);
  if (!hit) return;
  const seq = ++paintSeq;
  try {
    return await showFrameAtInner(t, hit, seq);
  } catch { /* clip offline: el overlay ya lo cuenta */ }
}

function blackFrame() {
  const { w: PW, h: PH } = projDims();
  const cw = Math.max(2, Math.round(PW * quality)), ch = Math.max(2, Math.round(PH * quality));
  const cv2 = document.createElement("canvas");
  cv2.width = cw; cv2.height = ch;
  cv2.getContext("2d").fillRect(0, 0, cw, ch);
  return new VideoFrame(cv2, { timestamp: 0 });
}

async function showFrameAtInner(t, hit, seq) {
  const kind = mediaKind(hit.clip);
  const srcT = hit.clip.in + hit.offset * (hit.clip.speed || 1);
  let f = null;
  if (kind === "gap") {
    f = blackFrame();
  } else if (kind === "image") {
    f = await posterFrame(hit.clip.media, 0.2);
    f = composeFrame(f, hit.clip);
  } else if (POSTER) {
    f = await posterFrame(hit.clip.media, srcT);
  } else {
    // máster si está enhebrado; proxy si no; y PÓSTER al instante mientras
    // los carretes llegan en segundo plano (jamás "enhebrando" bloqueante)
    const dec = bestDecoder(hit.clip.media, scrubFast);
    if (dec) {
      if (dec === decoders.get(hit.clip.media)) current = { clip: hit.clip, dec };
      else current = null;
      f = await dec.seek(srcT);
      if (f) f = composeFrame(f, hit.clip);
    } else {
      f = await posterFrame(hit.clip.media, srcT);
      f = composeFrame(f, hit.clip);
    }
  }
  if (!f) return;
  if (seq !== undefined && seq !== paintSeq) { f.close(); return; }   // llegó tarde
  if (state.playing) { f.close(); return; }   // la proyección manda en el vidrio
  const pd = projDims();
  fitCanvas(pd.w, pd.h);
  if (lastFrame) lastFrame.close();
  lastFrame = f;
  frameIdx++;
  repaint();
  updateTc(t);
}

/* ── el monitor de FUENTE: la cinta en el proyector, sin bobina ── */
const fuenteBar = () => document.getElementById("fuente-bar");

export async function openSource(mediaName) {
  pause();
  const m = state.media.find((x) => x.name === mediaName);
  if (!m) return;
  state.source = { media: mediaName, t: 0, in: null, out: null, playing: false, dur: m.dur, fps: m.fps || 25 };
  fuenteBar()?.classList.remove("hidden");
  document.querySelector(".moviola-marco")?.classList.add("fuente");
  updateFuenteBar();
  await showSourceFrame(0);
  emit("source");
}

export function closeSource() {
  if (!state.source) return;
  state.source = null;
  fuenteBar()?.classList.add("hidden");
  document.querySelector(".moviola-marco")?.classList.remove("fuente");
  showFrameAt(state.t);
  emit("source");
}

export async function showSourceFrame(t) {
  const s = state.source;
  if (!s) return;
  s.t = Math.max(0, Math.min(s.dur - 0.001, t));
  let f = null;
  if (POSTER) {
    f = await posterFrame(s.media, s.t);
  } else {
    const dec = bestDecoder(s.media);
    f = dec ? await dec.seek(s.t) : await posterFrame(s.media, s.t);
  }
  if (!f) return;
  fitCanvas(f.displayWidth, f.displayHeight);
  if (lastFrame) lastFrame.close();
  lastFrame = f;
  frameIdx++;
  repaint();
  updateTc(s.t);
  updateFuenteBar();
}

export function markIn() {
  if (!state.source) return;
  state.source.in = state.source.t;
  if (state.source.out !== null && state.source.out <= state.source.in) state.source.out = null;
  updateFuenteBar();
}

export function markOut() {
  if (!state.source) return;
  state.source.out = state.source.t;
  if (state.source.in !== null && state.source.in >= state.source.out) state.source.in = null;
  updateFuenteBar();
}

/** el tramo marcado pasa a la bobina — EN la aguja (⏎) o al final (⇧⏎) */
export function sourceToReel(alFinal = false) {
  const s = state.source;
  if (!s) return;
  const a = s.in ?? 0;
  const b = s.out ?? s.dur;
  if (alFinal || !state.clips.length) {
    addClipRaw(s.media, a, b);
  } else {
    // en la aguja: si cae en mitad de un clip, primero se parte por ahí
    const hit = clipAt(state.t);
    if (hit && hit.offset > 0.05 && hit.offset < clipDur(hit.clip) - 0.05) {
      splitAt(state.t);
    }
    const hit2 = clipAt(state.t);
    const idx = hit2
      ? state.clips.indexOf(hit2.clip) + (hit2.offset > clipDur(hit2.clip) / 2 ? 1 : 0)
      : state.clips.length;
    snapshot();
    state.clips.splice(idx, 0, { id: newId(), media: s.media, in: a, out: b });
    touch(); emit("timeline");
  }
  foley.thunk();
  updateFuenteBar();
}

export function toggleSource() {
  const s = state.source;
  if (!s) return;
  s.playing = !s.playing;
  if (s.playing) {
    s.wall = performance.now() / 1000;
    s.tPlay = s.t >= s.dur - 0.05 ? 0 : s.t;
    foley.motorStart();
  } else { foley.motorStop(); stopAudio(); }
}

function updateFuenteBar() {
  const b = fuenteBar();
  if (!b || !state.source) return;
  const s = state.source;
  const f = (v) => v === null ? "—" : v.toFixed(2) + "s";
  b.querySelector(".f-nombre").textContent = s.media.replace(/\.[^.]+$/, "");
  b.querySelector(".f-marcas").textContent = `entrada ${f(s.in)} · salida ${f(s.out)}`;
}

let rate = 1;

export function play() {
  rate = 1;
  playRaw();
}

function playRaw() {
  if (state.playing || !state.clips.length) return;
  state.playing = true;
  wallStart = performance.now() / 1000;
  tStart = rate >= 0
    ? (state.t >= totalDur() - 0.05 ? 0 : state.t)
    : (state.t <= 0.05 ? totalDur() - 0.001 : state.t);
  state.t = tStart;
  emit("transport");
  proyector();            // ← el bucle propio del proyector
}

/* ── EL PROYECTOR ──
   Un único bucle secuencial: pide UN frame, lo pinta, y solo entonces pide el
   siguiente. Antes esto vivía dentro de requestAnimationFrame, que se
   redispara cada 16 ms SIN esperar al anterior: decenas de decodificaciones
   concurrentes pisándose sobre el mismo carrete (una recreaba el decoder
   mientras otra le pedía frames) = imagen congelada con tirones de goma. */
let proyectando = false;
async function proyector() {
  if (proyectando) return;
  proyectando = true;
  try {
    while (state.playing && pipe) {
      const wall = performance.now() / 1000;
      const target = tStart + (wall - wallStart) * rate;

      if (target >= totalDur()) {
        if (state.loop && rate === 1) { tStart = 0; wallStart = wall; current = null; continue; }
        pause(); rate = 1; foley.release(); break;
      }
      if (rate < 0 && target <= 0) { state.t = 0; pause(); rate = 1; await showFrameAt(0); break; }

      const hit = clipAt(target);
      if (!hit) { pause(); break; }
      const kind = mediaKind(hit.clip);
      const spd = hit.clip.speed || 1;
      const srcT = hit.clip.in + hit.offset * spd;

      // huecos, fotos y lanzadera: un fotograma por vuelta, sin decoder
      if (kind !== "video" || rate !== 1 || spd !== 1) {
        if (rate !== 1) stopAudio();
        await pintaSuelto(target, hit, kind, srcT);
        state.t = target;
        updateTc(target); emit("time");
        if (rate === 1) syncAudio(target);
        await respira(16);
        continue;
      }

      // el carrete: proxy si lo hay (tiempo real garantizado), si no el máster
      let dec = bestDecoder(hit.clip.media, true);
      if (!dec) {
        await pintaSuelto(target, hit, kind, srcT);   // póster mientras carga
        state.t = target; updateTc(target); syncAudio(target); emit("time");
        await respira(60);
        continue;
      }
      if (!current || current.clip !== hit.clip || current.dec !== dec) {
        current = { clip: hit.clip, dec };
        const f0 = await dec.seek(srcT);
        if (f0) { current.lastTs = f0.timestamp / 1e6; pinta(f0, hit.clip, target); }
      } else {
        const last = current.lastTs ?? -1;
        const paso = 1 / (dec.fps || 30);
        // marcar el paso: si el fotograma pintado aún vale para este instante,
        // no se decodifica de más (antes iba al doble de velocidad y luego
        // tenía que rebobinar con un seek)
        if (last >= 0 && srcT < last + paso * 0.85) {
          const espera = Math.max(1, Math.min(20, (last + paso - srcT) * 1000 / (rate || 1)));
          state.t = target; updateTc(target); syncAudio(target); emit("time");
          await respira(espera);
          continue;
        }
        let f = null;
        if (srcT - last > 2.0 || srcT < last - 0.3) {
          f = await dec.seek(srcT);
        } else {
          let n = 0;
          for (;;) {
            const nf = await dec.next();
            if (!nf) break;
            if (nf.timestamp / 1e6 >= srcT - 0.5 / (dec.fps || 30)) { f = nf; break; }
            nf.close();
            if (++n > 30) { f = nf; break; }
          }
        }
        if (f) {
          const real = f.timestamp / 1e6;
          // si el carrete no da para tiempo real, el RELOJ le espera: nunca
          // se queda la imagen congelada mientras el sonido se va solo
          const deriva = srcT - real;
          if (deriva > 0.10) wallStart += deriva / (rate || 1);
          current.lastTs = real;
          pinta(f, hit.clip, target);
        }
      }

      prefetchJunta(target, hit);
      state.t = target;
      updateTc(target);
      syncAudio(target);
      emit("time");
      await respira(2);
    }
  } catch (e) {
    console.warn("proyector:", e);
  } finally {
    proyectando = false;
  }
}

const respira = (ms) => new Promise((r) => setTimeout(r, ms));

function pinta(f, clip, t) {
  const c = composeFrame(f, clip);
  if (lastFrame) lastFrame.close();
  lastFrame = c;
  const pd = projDims();
  fitCanvas(pd.w, pd.h);
  frameIdx++;
  pipe.setSourceFrame(lastFrame);
  pipe.render(P(), t, frameIdx % 997);
  pintados++;
}
let pintados = 0;
export function framesPintados() { return pintados; }

async function pintaSuelto(t, hit, kind, srcT) {
  try {
    let f = kind === "gap" ? blackFrame()
          : kind === "image" ? await posterFrame(hit.clip.media, 0.2)
          : await posterFrame(hit.clip.media, srcT);
    if (f) pinta(f, hit.clip, t);
  } catch {}
}

function prefetchJunta(target, hit) {
  const items = layout();
  const i = items.findIndex((x) => x.clip === hit.clip);
  const nx = i >= 0 && i < items.length - 1 ? items[i + 1].clip : null;
  if (nx && !nx.gap && nx.media !== hit.clip.media && mediaKind(nx) === "video" &&
      items[i].end - target < 1.0 && prefetched !== nx.id) {
    prefetched = nx.id;
    bestDecoder(nx.media, true);   // solo abrirlo; nada de robarle frames
  }
}

/** J/K/L: lanzadera. dir +1 = L (1×→2×→4×), dir −1 = J (−1×→−2×→−4×) */
export function shuttle(dir) {
  if (!state.clips.length) return;
  const seq = [1, 2, 4];
  const cur = Math.abs(rate);
  if (!state.playing || Math.sign(rate) !== Math.sign(dir)) {
    rate = dir;
    if (!state.playing) playRaw();
    else { wallStart = performance.now() / 1000; tStart = state.t; }
  } else {
    const nxt = seq[(seq.indexOf(cur) + 1) % seq.length];
    rate = dir * nxt;
    wallStart = performance.now() / 1000;
    tStart = state.t;
  }
  emit("transport");
}

export function pause() {
  if (!state.playing) return;
  state.playing = false;
  stopAudio();
  emit("transport");
}

export function toggle() { state.playing ? pause() : play(); }

export async function stepFrames(n) {
  if (state.source) {
    state.source.playing = false;
    await showSourceFrame(state.source.t + n / (state.source.fps || 25));
    return;
  }
  pause();
  const fps = projFps();
  state.t = Math.max(0, Math.min(totalDur() - 0.001, state.t + n / fps));
  await showFrameAt(state.t);
  emit("time");
}

/* ── la manivela: repasar la película con la mano ── */
let crankVel = 0;          // vueltas/s
let crankAngle = 0;
let crankLast = null;
let crankAccum = 0;

function initCrank() {
  const el = document.getElementById("manivela");
  const arm = document.getElementById("manivela-brazo");
  if (!el) return;
  const center = () => {
    const r = el.getBoundingClientRect();
    return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
  };
  el.addEventListener("pointerdown", (e) => {
    el.setPointerCapture(e.pointerId);
    pause();
    const c = center();
    crankLast = Math.atan2(e.clientY - c.y, e.clientX - c.x);
    crankVel = 0;
  });
  el.addEventListener("pointermove", (e) => {
    if (crankLast === null) return;
    const c = center();
    const a = Math.atan2(e.clientY - c.y, e.clientX - c.x);
    let d = a - crankLast;
    if (d > Math.PI) d -= 2 * Math.PI;
    if (d < -Math.PI) d += 2 * Math.PI;
    crankLast = a;
    crankAngle += d;
    crankVel = d * 60;                        // aproximación de velocidad
    arm.style.transform = `rotate(${crankAngle}rad)`;
    crankAccum += d / (2 * Math.PI);          // 1 vuelta = 1 segundo de película
    stepCrank();
  });
  const done = () => { crankLast = null; };   // la inercia sigue en tick()
  el.addEventListener("pointerup", done);
  el.addEventListener("pointercancel", done);
}

let shuttlePending = false;
let prefetched = null;
let crankStepping = false;
let crankWasSpinning = false;
async function stepCrank() {
  crankWasSpinning = true;
  scrubFast = true;
  if (crankStepping) return;
  const fps = projFps();
  const frames = Math.trunc(crankAccum * fps);
  if (!frames) return;
  crankAccum -= frames / fps;
  crankStepping = true;
  foley.ratchet();
  state.t = Math.max(0, Math.min(totalDur() - 0.001, state.t + frames / fps));
  await showFrameAt(state.t);
  emit("time");
  crankStepping = false;
}

async function tick(now) {
  requestAnimationFrame(tick);

  // inercia de la manivela
  if (crankLast === null && crankWasSpinning && Math.abs(crankVel) <= 0.35) {
    crankWasSpinning = false;
    scrubFast = false;
    showFrameAt(state.t);           // el frame exacto en máster al frenar
  }
  if (crankLast === null && Math.abs(crankVel) > 0.35) {
    crankVel *= 0.94;
    crankAngle += crankVel / 60;
    const arm = document.getElementById("manivela-brazo");
    if (arm) arm.style.transform = `rotate(${crankAngle}rad)`;
    crankAccum += (crankVel / 60) / (2 * Math.PI);
    stepCrank();
  }

  // la fuente gira por su cuenta (también con un solo carril)
  if (state.source?.playing && pipe && !fuenteBusy) {
    fuenteBusy = true;
    try {
    const s = state.source;
    const wall = performance.now() / 1000;
    const target = s.tPlay + (wall - s.wall);
    if (target >= s.dur) { s.playing = false; foley.motorStop(); stopAudio(); foley.release(); fuenteBusy = false; return; }
    const dec = await decoderFor(s.media).catch(() => null);
    if (dec) {
      let f = null;
      for (;;) {
        const nf = await dec.next();
        if (!nf) break;
        if (nf.timestamp / 1e6 >= target - 0.5 / (dec.fps || 25)) { f = nf; break; }
        nf.close();
      }
      if (!f) f = await dec.seek(target);
      if (f) {
        if (lastFrame) lastFrame.close();
        lastFrame = f;
        fitCanvas(f.displayWidth, f.displayHeight);
        frameIdx++;
        pipe.setSourceFrame(lastFrame);
        pipe.render(P(), target, frameIdx % 997);
      }
    }
    s.t = target;
    updateTc(target);
    syncOne("src", s.media, target, 1);
    } finally { fuenteBusy = false; }
    return;
  }

  if (!state.playing) return;

  // el trabajo de imagen lo hace el PROYECTOR (bucle propio, secuencial);
  // aquí solo se mide la cadencia real de pintado
  const dt = now / 1000 - lastTick;
  lastTick = now / 1000;
  if (dt > 0) {
    const inst = (framesPintados() - fpsUlt) / dt;
    fpsUlt = framesPintados();
    fpsAvg = fpsAvg ? fpsAvg * 0.85 + inst * 0.15 : inst;
  }
  if (fpsEl) fpsEl.textContent = fpsAvg.toFixed(0) + " fps";
}

let fpsUlt = 0;
let fuenteBusy = false;

function updateTc(t) {
  const fps = projFps();
  const fr = Math.floor((t % 1) * fps);
  const s = Math.floor(t);
  const txt =
    String(Math.floor(s / 3600)).padStart(2, "0") + ":" +
    String(Math.floor(s / 60) % 60).padStart(2, "0") + ":" +
    String(s % 60).padStart(2, "0") + ":" +
    String(fr).padStart(2, "0");
  const a = document.getElementById("tc");
  const b = document.getElementById("tc-dark");
  if (a) a.textContent = txt;
  if (b) b.textContent = txt;
}
