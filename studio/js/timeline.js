// timeline.js — la bobina sobre el banco: una tira de película de verdad
// (fotogramas reales, perforaciones, cinta de empalme en cada corte), regla a
// lápiz, la aguja con banderita, y la empalmadora en dos gestos: marcar (lápiz
// graso) y cortar (CHUNK).

import { state, on, emit, layout, totalDur, clipDur, removeClip, moveClip, touch, snapshot,
         beginGesture, commitGesture, cancelGesture, newId, snapT } from "./state.js";

const DB_TOP = 12, DB_BOT = -36;   // el rango de la banda elástica
import { menuFor } from "./menu.js";
import { showFrameAt, pause, setScrubFast, scrubBlip } from "./viewer.js";
import * as foley from "./foley.js";

const cv = document.getElementById("timeline");
const ctx = cv.getContext("2d");
let pxs = 18;
let scrollX = 0;
let drag = null;

const RULER_H = 24;
const TRACK_Y = RULER_H + 18;
const STRIP_H = 84;
// la pista de sonido, bajo la tira de película
const TRACK_A_Y = TRACK_Y + STRIP_H + 10;
const TRACK_A_H = 32;
const aDur = (a) => Math.max(a.out - a.in, 0.01);

// materia
const tex = {};
for (const n of ["splice_tape", "grease_0", "grease_1", "grease_2"]) {
  tex[n] = new Image(); tex[n].src = "assets/" + n + ".png";
}
// formas de onda (una por cinta, el clip pinta su ventana)
const waves = new Map();
function wave(media) {
  if (!media) return null;
  if (!waves.has(media)) {
    const im = new Image();
    im.src = `/api/wave?f=${encodeURIComponent(media)}`;
    waves.set(media, im);
  }
  const im = waves.get(media);
  return im.complete && im.naturalWidth ? im : null;
}

// fotogramas reales de los clips (cache de miniaturas por segundo)
const thumbs = new Map();
function thumb(media, t) {
  const key = media + "@" + Math.round(t);
  if (!thumbs.has(key)) {
    const im = new Image();
    im.src = `/api/thumb?f=${encodeURIComponent(media)}&t=${Math.max(0.2, Math.round(t)).toFixed(1)}`;
    thumbs.set(key, im);
  }
  const im = thumbs.get(key);
  return im.complete && im.naturalWidth ? im : null;
}

// la marca de lápiz graso pendiente de corte
export let mark = null;   // {t, variant}

const springX = new Map();
let needle = { x: 0, v: 0 };
let magnetFlash = null;    // {x, until} — el imán acaba de morder
let tooltip = null;        // {x, y, text} — el dato vivo del trim
export const view = { insertIdx: null };   // línea de inserción del arrastre

export function hitAt(mx, my) { return hitTest(mx, my); }

/** ⇧Z: toda la bobina cabe en el banco */
export function zoomFit() {
  const W = cv.width / devicePixelRatio;
  const d = Math.max(totalDur(), 1);
  pxs = Math.max(4, Math.min(140, (W - 40) / d));
  scrollX = 0;
  const z = document.getElementById("tl-zoom");
  if (z) z.value = pxs;
}
export function timeAt(mx) { return Math.max(0, x2t(mx)); }

export function initTimeline() {
  const zoom = document.getElementById("tl-zoom");
  zoom.addEventListener("input", () => { pxs = +zoom.value; foley.tick(); });

  new ResizeObserver(resize).observe(cv.parentElement);
  cv.addEventListener("pointerdown", down);
  cv.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    const rect = cv.getBoundingClientRect();
    menuFor(e.clientX, e.clientY, hitTest(e.clientX - rect.left, e.clientY - rect.top),
            timeAt(e.clientX - rect.left));
  });
  cv.addEventListener("pointermove", move);
  cv.addEventListener("pointerup", up);
  cv.addEventListener("wheel", (e) => {
    e.preventDefault();
    if (e.ctrlKey || e.metaKey) {
      pxs = Math.max(4, Math.min(140, pxs * (e.deltaY < 0 ? 1.15 : 0.87)));
      zoom.value = pxs;
    } else scrollX = Math.max(0, scrollX + e.deltaX + e.deltaY);
  }, { passive: false });

  const updateTotal = () => {
    const tot = document.getElementById("banco-total");
    if (tot) {
      const s = totalDur();
      tot.textContent = String(Math.floor(s / 60)).padStart(2, "0") + ":" +
        String(Math.floor(s % 60)).padStart(2, "0");
    }
  };
  on("timeline", updateTotal);
  updateTotal();

  resize();
  requestAnimationFrame(loop);
}

/** la empalmadora: primera vez marca con lápiz, segunda corta por la marca */
export function blade() {
  if (mark === null) {
    if (!state.clips.length) return;
    mark = { t: state.t, variant: Math.floor(Math.random() * 3) };
    foley.squeak();
  } else {
    const t = mark.t;
    mark = null;
    splitAtT(t);
    foley.chunk();
  }
}

function splitAtT(t) {
  t = snapT(t);              // el corte cae SIEMPRE en un frame del proyecto
  // como state.splitAt pero con umbral fino
  const hit = clipAtLocal(t);
  if (!hit || hit.offset < 0.03 || hit.offset > clipDur(hit.clip) - 0.03) return;
  snapshot();
  const c = hit.clip;
  const i = state.clips.indexOf(c);
  if (c.gap) {
    const right = { id: newId(), gap: true, dur: clipDur(c) - hit.offset };
    c.dur = hit.offset;
    state.clips.splice(i + 1, 0, right);
  } else {
    const cutSrc = c.in + hit.offset * (c.speed || 1);
    const right = { id: newId(), media: c.media, in: cutSrc, out: c.out };
    if (c.speed) right.speed = c.speed;
    c.out = cutSrc;
    state.clips.splice(i + 1, 0, right);
  }
  touch(); emit("timeline");
}

function clipAtLocal(t) {
  for (const it of layout())
    if (t >= it.start && (t < it.end || it === layout().at(-1)))
      return { ...it, offset: t - it.start };
  return null;
}

function resize() {
  const r = cv.parentElement.getBoundingClientRect();
  cv.width = Math.max(200, r.width * devicePixelRatio);
  cv.height = Math.max(80, (r.height - 34) * devicePixelRatio);
  cv.style.height = (r.height - 34) + "px";
}

const x2t = (x) => (x + scrollX) / pxs;
const t2x = (t) => t * pxs - scrollX;

function hitTest(mx, my) {
  if (my < RULER_H + 6) return { kind: "playhead" };
  if (my >= TRACK_Y && my <= TRACK_Y + STRIP_H) {
    // la junta manda: clic en la cinta de empalme = ciclar el fundido
    const items = layout();
    for (let i = 0; i < items.length - 1; i++) {
      const xj = t2x(items[i].end);
      if (Math.abs(mx - xj) <= 8) return { kind: "junta", it: items[i] };
    }
    for (const it of items) {
      const x0 = t2x(it.start), x1 = t2x(it.end);
      if (mx >= x0 - 5 && mx <= x0 + 7 && (it.clip.gap || it.clip.in > 0.01)) return { kind: "trimL", it };
      if (mx >= x1 - 7 && mx <= x1 + 5) return { kind: "trimR", it };
      if (mx > x0 && mx < x1) return { kind: "move", it };
    }
  }
  if (my >= TRACK_A_Y - 6 && my <= TRACK_A_Y + TRACK_A_H + 6) {
    // primero los puntos de la banda elástica (siempre mandan)
    for (const a of state.audio) {
      for (const pt of a.env || []) {
        const px2 = t2x(a.start + pt.t);
        const py2 = TRACK_A_Y + ((DB_TOP - pt.db) / (DB_TOP - DB_BOT)) * TRACK_A_H;
        if (Math.hypot(mx - px2, my - py2) <= 7) return { kind: "envPt", a, pt };
      }
    }
    for (const a of state.audio) {
      const x0 = t2x(a.start), x1 = t2x(a.start + aDur(a));
      if (Math.abs(mx - x0) <= 6) return { kind: "trimAL", a };
      if (Math.abs(mx - x1) <= 6) return { kind: "trimAR", a };
      if (mx > x0 && mx < x1) return { kind: "moveA", a };
    }
  }
  return { kind: "playhead" };
}

let lastScrubX = 0;
function down(e) {
  cv.setPointerCapture(e.pointerId);
  const rect = cv.getBoundingClientRect();
  const mx = e.clientX - rect.left, my = e.clientY - rect.top;
  const h = hitTest(mx, my);
  if (h.kind === "playhead") {
    pause();
    setScrubFast(true);
    drag = { kind: "playhead" };
    lastScrubX = mx;
    scrubTo(mx);
  } else if (h.kind === "junta") {
    // el fundido se cicla: nada → ½s → 1s → 2s → nada
    snapshot();
    const c = h.it.clip;
    const pasos = [0, 0.5, 1, 2];
    c.fade = pasos[(pasos.indexOf(c.fade || 0) + 1) % pasos.length];
    foley.squeak();
    touch(); emit("timeline");
  } else if (h.kind === "envPt") {
    if (e.altKey) {
      // alt sobre un punto: se quita
      beginGesture();
      h.a.env = (h.a.env || []).filter((q2) => q2 !== h.pt);
      if (h.a.env.length < 2) delete h.a.env;
      commitGesture();
      touch(); emit("timeline");
      foley.chunk();
      return;
    }
    beginGesture();
    state.selAudio = h.a.id; state.sel = null;
    drag = { kind: "envPt", a: h.a, pt: h.pt };
    foley.grab();
    emit("timeline");
  } else if (h.kind === "move") {
    if (e.altKey) {
      // alt+arrastre: slip — la ventana se mueve DENTRO del material
      beginGesture();
      state.sel = h.it.clip.id; state.selAudio = null;
      drag = { kind: "slip", clip: h.it.clip, lastX: mx };
      foley.grab();
      emit("timeline");
      return;
    }
    if (e.shiftKey) {
      // ⇧clic: multi-selección
      const id2 = h.it.clip.id;
      const i2 = state.selSet.indexOf(id2);
      if (i2 >= 0) state.selSet.splice(i2, 1);
      else state.selSet.push(id2);
      state.sel = id2; state.selAudio = null;
      emit("timeline");
      foley.tick();
      return;
    }
    state.selSet = [];
    beginGesture();
    state.sel = h.it.clip.id;
    state.selAudio = null;
    drag = { kind: "move", clip: h.it.clip, grabDx: mx - t2x(h.it.start), x: mx };
    foley.grab();
    emit("timeline");
  } else if (h.kind === "moveA" || h.kind === "trimAL" || h.kind === "trimAR") {
    if (h.kind === "moveA" && e.altKey) {
      // alt sobre la cinta de audio: nace un punto de la banda elástica
      beginGesture();
      const offT = Math.max(0, x2t(mx) - h.a.start);
      const db = Math.max(DB_BOT, Math.min(DB_TOP,
        DB_TOP - ((my - TRACK_A_Y) / TRACK_A_H) * (DB_TOP - DB_BOT)));
      h.a.env = h.a.env || [];
      h.a.env.push({ t: +offT.toFixed(3), db: +db.toFixed(1) });
      h.a.env.sort((q1, q2) => q1.t - q2.t);
      commitGesture();
      touch(); emit("timeline");
      foley.tick();
      return;
    }
    beginGesture();
    state.selAudio = h.a.id;
    state.sel = null;
    drag = { kind: h.kind, a: h.a, grabDx: mx - t2x(h.a.start) };
    foley.grab();
    emit("timeline");
  } else {
    beginGesture();
    state.sel = h.it.clip.id;
    state.selAudio = null;
    drag = { kind: h.kind, clip: h.it.clip };
    foley.grab();
    emit("timeline");
  }
}

/** Esc: el gesto en vuelo se cancela y el clip vuelve a su sitio */
export function cancelDrag() {
  tooltip = null;
  view.insertIdx = null;
  if (!drag) return false;
  const kind = drag.kind;
  drag = null;
  if (kind === "playhead") { setScrubFast(false); showFrameAt(state.t); return true; }
  cancelGesture();
  foley.release();
  showFrameAt(state.t);
  return true;
}

let scrubPending = false;
function scrubTo(mx) {
  let t = Math.max(0, Math.min(totalDur(), x2t(mx)));
  // imán: la aguja se pega a las juntas (8 px)
  let magnet = false;
  for (const it of layout()) {
    for (const edge of [it.start, it.end]) {
      if (Math.abs(t2x(edge) - mx) <= 8) {
        t = edge; magnet = true;
        magnetFlash = { x: t2x(edge), until: performance.now() + 260 };
        break;
      }
    }
  }
  if (!magnet) t = snapT(t);
  state.t = Math.max(0, Math.min(totalDur(), t));
  if (!scrubPending) {
    scrubPending = true;
    showFrameAt(state.t).finally(() => { scrubPending = false; });
  }
  emit("time");
}

function move(e) {
  const rect = cv.getBoundingClientRect();
  const mx = e.clientX - rect.left, my = e.clientY - rect.top;
  if (drag && (drag.kind === "move" || drag.kind === "moveA")) cv.style.cursor = "grabbing";
  if (!drag) {
    const h = hitTest(mx, my);
    cv.style.cursor = h.kind === "trimL" || h.kind === "trimR" || h.kind === "trimAL" || h.kind === "trimAR"
      ? "ew-resize"
      : h.kind === "move" || h.kind === "moveA" ? "grab"
      : h.kind === "junta" ? "pointer" : "default";
    return;
  }
  if (drag.kind === "playhead") {
    foley.scrub(mx - lastScrubX);
    lastScrubX = mx;
    scrubBlip(x2t(mx));
    return scrubTo(mx);
  }
  if (drag.kind === "slip") {
    const c = drag.clip;
    if (!c.gap) {
      const m2 = state.media.find((x) => x.name === c.media);
      const d2 = (mx - drag.lastX) / pxs * (c.speed || 1);
      drag.lastX = mx;
      const span = c.out - c.in;
      let nin = c.in - d2;   // arrastrar a la derecha enseña material anterior
      nin = Math.max(0, Math.min((m2 ? m2.dur : 1e9) - span, nin));
      if (Math.abs(nin - c.in) > 0.001) {
        c.in = nin; c.out = nin + span;
        tooltip = { x: mx, y: TRACK_Y - 8, text: `slip · fuente ${c.in.toFixed(2)}–${c.out.toFixed(2)}` };
        touch(); emit("timeline");
      }
    }
    return;
  }
  if (drag.kind === "envPt") {
    const a = drag.a, pt = drag.pt;
    pt.t = +Math.max(0, Math.min(aDur(a), x2t(mx) - a.start)).toFixed(3);
    pt.db = +Math.max(DB_BOT, Math.min(DB_TOP,
      DB_TOP - ((my - TRACK_A_Y) / TRACK_A_H) * (DB_TOP - DB_BOT))).toFixed(1);
    a.env.sort((q1, q2) => q1.t - q2.t);
    tooltip = { x: mx, y: TRACK_A_Y - 10, text: `${pt.db > 0 ? "+" : ""}${pt.db} dB` };
    touch(); emit("timeline");
    return;
  }
  const m = state.media.find((x) => x.name === drag.clip?.media);
  const fmtTrim = (c) => {
    const d = clipDur(c);
    return `${d.toFixed(2)} s · fuente ${c.in.toFixed(2)}–${c.out.toFixed(2)}`;
  };
  if ((drag.kind === "trimR" || drag.kind === "trimL") && drag.clip?.gap) {
    const it = layout().find((i2) => i2.clip === drag.clip);
    const want = drag.kind === "trimR"
      ? x2t(mx) - it.start
      : it.end - x2t(mx);
    drag.clip.dur = snapT(Math.max(0.2, want));
    tooltip = { x: mx, y: TRACK_Y - 8, text: `hueco · ${drag.clip.dur.toFixed(2)} s` };
    touch(); emit("timeline");
    return;
  }
  if (drag.kind === "trimR") {
    const it = layout().find((i) => i.clip === drag.clip);
    const want = x2t(mx) - it.start;
    const prev = drag.clip.out;
    const wanted = drag.clip.in + want;
    const clamped = Math.max(drag.clip.in + 0.08, Math.min(m ? m.dur : 1e9, wanted));
    drag.clip.out = snapT(clamped);
    // tope de material: resistencia visible
    if (m && wanted > m.dur + 0.01) magnetFlash = { x: mx, until: performance.now() + 200, tope: true };
    tooltip = { x: mx, y: TRACK_Y - 8, text: fmtTrim(drag.clip) };
    if (Math.abs(prev - drag.clip.out) > 0.02) foley.tick();
    touch(); emit("timeline");
  } else if (drag.kind === "trimL") {
    const it = layout().find((i) => i.clip === drag.clip);
    const delta = x2t(mx) - it.start;
    const prev = drag.clip.in;
    const wanted = drag.clip.in + delta;
    if (m && wanted < -0.01) magnetFlash = { x: mx, until: performance.now() + 200, tope: true };
    drag.clip.in = snapT(Math.max(0, Math.min(drag.clip.out - 0.08, wanted)));
    tooltip = { x: mx, y: TRACK_Y - 8, text: fmtTrim(drag.clip) };
    if (Math.abs(prev - drag.clip.in) > 0.02) foley.tick();
    touch(); emit("timeline");
  } else if (drag.kind === "moveA") {
    const a = drag.a;
    let t = Math.max(0, x2t(mx - drag.grabDx));
    // imán: el principio o el final del clip se pegan a juntas de vídeo y aguja
    const cands = [state.t, 0];
    for (const it of layout()) { cands.push(it.start, it.end); }
    for (const other of state.audio) {
      if (other !== a) cands.push(other.start, other.start + aDur(other));
    }
    for (const c of cands) {
      if (Math.abs(t2x(c) - t2x(t)) <= 8) {
        t = c; magnetFlash = { x: t2x(c), until: performance.now() + 260 };
        break;
      }
      if (Math.abs(t2x(c) - t2x(t + aDur(a))) <= 8) {
        t = Math.max(0, c - aDur(a));
        magnetFlash = { x: t2x(c), until: performance.now() + 260 };
        break;
      }
    }
    t = snapT(t);
    if (Math.abs(t - a.start) > 0.001) { a.start = t; foley.tick(); touch(); emit("timeline"); }
  } else if (drag.kind === "trimAL") {
    const a = drag.a;
    const m2 = state.media.find((x) => x.name === a.media);
    const delta = x2t(mx) - a.start;
    const nin = Math.max(0, Math.min(a.out - 0.1, a.in + delta));
    const applied = nin - a.in;
    if (Math.abs(applied) > 0.001) {
      a.in = nin; a.start = Math.max(0, a.start + applied);
      foley.tick(); touch(); emit("timeline");
    }
    void m2;
  } else if (drag.kind === "trimAR") {
    const a = drag.a;
    const m2 = state.media.find((x) => x.name === a.media);
    const want = x2t(mx) - a.start;
    const prev = a.out;
    a.out = Math.max(a.in + 0.1, Math.min(m2 ? m2.dur : 1e9, a.in + want));
    if (Math.abs(prev - a.out) > 0.02) { foley.tick(); touch(); emit("timeline"); }
  } else if (drag.kind === "move") {
    drag.x = mx;
    const t = x2t(mx - drag.grabDx) + clipDur(drag.clip) / 2;
    let idx = 0, acc = 0;
    for (const c of state.clips) {
      if (c === drag.clip) continue;
      if (t > acc + clipDur(c) / 2) idx++;
      acc += clipDur(c);
    }
    view.insertIdx = idx;
    const cur = state.clips.indexOf(drag.clip);
    if (idx !== cur) { moveClip(drag.clip.id, idx); foley.tick(); }
  }
}

function up() {
  tooltip = null;
  view.insertIdx = null;
  if (drag) {
    if (drag.kind === "playhead") {
      foley.scrubEnd();
      setScrubFast(false);
      showFrameAt(state.t);         // el frame exacto en máster al soltar
    } else {
      commitGesture();              // solo consume historia si algo cambió
      foley.release(); showFrameAt(state.t);
    }
  }
  drag = null;
}

function fmt(s) {
  return String(Math.floor(s / 60)).padStart(2, "0") + ":" + String(Math.floor(s % 60)).padStart(2, "0");
}

function spring(cur, target, dt) {
  const k = 220, damp = 26;
  cur.v += (target - cur.x) * k * dt - cur.v * damp * dt;
  cur.x += cur.v * dt;
  return cur;
}

let lastT = 0;
function loop(now) {
  requestAnimationFrame(loop);
  const dt = Math.min((now - lastT) / 1000 || 0.016, 0.05);
  lastT = now;
  draw(dt);
}

// jitter determinista para la regla a lápiz
function jit(i, k) { return (Math.sin(i * 12.9898 + k * 78.233) * 43758.5453) % 1; }

function draw(dt) {
  const W = cv.width / devicePixelRatio, H = cv.height / devicePixelRatio;
  ctx.setTransform(devicePixelRatio, 0, 0, devicePixelRatio, 0, 0);
  ctx.clearRect(0, 0, W, H);

  // regla a lápiz: raya base ligeramente curva + ticks temblones
  ctx.strokeStyle = "#2b3bc7";
  ctx.lineWidth = 1.4;
  ctx.beginPath();
  ctx.moveTo(0, RULER_H + jit(0, 1));
  for (let x = 0; x <= W; x += 40)
    ctx.lineTo(x, RULER_H + jit(x, 1) * 1.6 - 0.8);
  ctx.stroke();
  const step = pxs >= 60 ? 1 : pxs >= 24 ? 5 : pxs >= 10 ? 10 : 30;
  ctx.font = "9px 'Courier New', monospace";
  ctx.fillStyle = "rgba(43,59,199,0.72)";
  const t0 = Math.max(0, Math.floor(scrollX / pxs / step) * step);
  for (let t = t0; t2x(t) < W; t += step) {
    const x = t2x(t);
    ctx.strokeStyle = "#2b3bc7";
    ctx.beginPath();
    ctx.moveTo(x + jit(t, 2), RULER_H - 6 - jit(t, 3) * 3);
    ctx.lineTo(x - jit(t, 4), RULER_H);
    ctx.stroke();
    ctx.fillText(fmt(t), x + 3, RULER_H - 8);
  }

  // clips: la tira con muelle
  for (const it of layout()) {
    const targetX = t2x(it.start);
    let s = springX.get(it.clip.id);
    if (!s) { s = { x: targetX, v: 0 }; springX.set(it.clip.id, s); }
    const isDragged = drag?.kind === "move" && drag.clip === it.clip;
    if (isDragged) { s.x = drag.x - drag.grabDx; s.v = 0; }
    else spring(s, targetX, dt);
    const w = Math.max((it.end - it.start) * pxs, 8);
    if (s.x + w < 0 || s.x > W) continue;
    drawStrip(s.x, w, it, isDragged);
  }
  for (const id of [...springX.keys()])
    if (!state.clips.some((c) => c.id === id)) springX.delete(id);

  // cinta de empalme en cada junta
  if (tex.splice_tape.complete) {
    let acc = 0;
    const items = layout();
    for (let i = 0; i < items.length - 1; i++) {
      acc = items[i].end;
      const x = t2x(acc);
      if (x < -20 || x > W + 20) continue;
      ctx.save();
      ctx.translate(x, TRACK_Y + STRIP_H / 2);
      ctx.rotate((jit(i, 7) - 0.5) * 0.10);
      ctx.globalAlpha = 0.9;
      ctx.drawImage(tex.splice_tape, -7, -STRIP_H / 2 - 7, 14, STRIP_H + 14);
      ctx.restore();
      // el fundido: dos triángulos que se cruzan + su rótulo graso
      const fade = items[i].clip.fade || 0;
      if (fade > 0.01) {
        const fw = Math.max(fade * pxs, 14);
        ctx.save();
        ctx.globalAlpha = 0.8;
        ctx.strokeStyle = "#b45a38";
        ctx.lineWidth = 2.2;
        ctx.beginPath();
        ctx.moveTo(x - fw / 2, TRACK_Y + STRIP_H - 12);
        ctx.lineTo(x + fw / 2, TRACK_Y + 12);
        ctx.moveTo(x - fw / 2, TRACK_Y + 12);
        ctx.lineTo(x + fw / 2, TRACK_Y + STRIP_H - 12);
        ctx.stroke();
        ctx.font = "600 13px 'Caveat', cursive";
        ctx.fillStyle = "#b45a38";
        ctx.fillText(fade + "s" + (items[i].clip.fadeType === "fadeblack" ? " ·neg" :
                 items[i].clip.fadeType === "fadewhite" ? " ·bla" : ""), x + fw / 2 + 3, TRACK_Y + 16);
        ctx.restore();
      }
    }
  }

  // ── la pista de sonido ──
  {
    // el raíl, a lápiz
    ctx.strokeStyle = "rgba(43,59,199,0.35)";
    ctx.lineWidth = 1;
    ctx.setLineDash([4, 5]);
    ctx.beginPath();
    ctx.moveTo(0, TRACK_A_Y + TRACK_A_H / 2);
    ctx.lineTo(W, TRACK_A_Y + TRACK_A_H / 2);
    ctx.stroke();
    ctx.setLineDash([]);
    for (const a of state.audio) {
      const x = t2x(a.start), w = Math.max(aDur(a) * pxs, 10);
      if (x + w < 0 || x > W) continue;
      const selA = state.selAudio === a.id;
      ctx.save();
      // la cinta magnética
      ctx.fillStyle = a.mute ? "#4a453c" : "#2a2620";
      ctx.beginPath();
      ctx.roundRect(x, TRACK_A_Y, w, TRACK_A_H, 3);
      ctx.fill();
      // la onda, en terracota
      const wim = wave(a.media);
      const m2 = state.media.find((mm) => mm.name === a.media);
      if (wim && m2 && m2.dur > 0) {
        const sx = (a.in / m2.dur) * wim.naturalWidth;
        const sw = Math.max(1, (aDur(a) / m2.dur) * wim.naturalWidth);
        ctx.globalAlpha = a.mute ? 0.3 : 0.9;
        ctx.drawImage(wim, sx, 0, sw, wim.naturalHeight, x + 2, TRACK_A_Y + 3, w - 4, TRACK_A_H - 6);
        ctx.globalAlpha = 1;
      }
      // fundidos de audio: rampas dibujadas
      ctx.strokeStyle = "#f2eee4";
      ctx.lineWidth = 1.4;
      if ((a.fadeIn || 0) > 0.005) {
        const fw = Math.min(a.fadeIn * pxs, w);
        ctx.beginPath();
        ctx.moveTo(x, TRACK_A_Y + TRACK_A_H - 2);
        ctx.lineTo(x + fw, TRACK_A_Y + 2);
        ctx.stroke();
      }
      if ((a.fadeOut || 0) > 0.005) {
        const fw = Math.min(a.fadeOut * pxs, w);
        ctx.beginPath();
        ctx.moveTo(x + w - fw, TRACK_A_Y + 2);
        ctx.lineTo(x + w, TRACK_A_Y + TRACK_A_H - 2);
        ctx.stroke();
      }
      // la banda elástica: la línea de volumen con sus puntos
      if (a.env && a.env.length >= 2) {
        ctx.strokeStyle = "#f2c744";
        ctx.lineWidth = 1.6;
        ctx.beginPath();
        const yOf = (db) => TRACK_A_Y + ((DB_TOP - db) / (DB_TOP - DB_BOT)) * TRACK_A_H;
        ctx.moveTo(x, yOf(a.env[0].db));
        for (const pt of a.env) ctx.lineTo(t2x(a.start + pt.t), yOf(pt.db));
        ctx.lineTo(x + w, yOf(a.env[a.env.length - 1].db));
        ctx.stroke();
        for (const pt of a.env) {
          ctx.beginPath();
          ctx.fillStyle = "#f2c744";
          ctx.arc(t2x(a.start + pt.t), yOf(pt.db), 3.4, 0, 7);
          ctx.fill();
        }
      }
      if (selA) {
        ctx.strokeStyle = "#d93325";
        ctx.lineWidth = 2;
        ctx.strokeRect(x - 1, TRACK_A_Y - 1, w + 2, TRACK_A_H + 2);
      }
      // rótulo + ganancia
      if (w > 54) {
        ctx.font = "600 12px 'Caveat', 'Bradley Hand', cursive";
        ctx.fillStyle = "#b45a38";
        const tag = a.media.replace(/\.[^.]+$/, "").slice(0, Math.floor(w / 8));
        const extra = a.mute ? " · mudo" : (a.gain ? ` · ${a.gain > 0 ? "+" : ""}${a.gain} dB` : "");
        ctx.fillText(tag + extra, x + 4, TRACK_A_Y - 3);
      }
      ctx.restore();
    }
  }

  // la marca de lápiz graso (pendiente de corte)
  if (mark !== null) {
    const img = tex["grease_" + mark.variant];
    const x = t2x(mark.t);
    if (img.complete && x > -40 && x < W + 40) {
      ctx.save();
      ctx.translate(x, TRACK_Y + STRIP_H / 2);
      ctx.rotate(Math.PI / 2);
      ctx.globalAlpha = 0.9;
      ctx.drawImage(img, -STRIP_H / 2 - 8, -14, STRIP_H + 16, 28);
      ctx.restore();
    }
  }

  // auto-scroll: la aguja no se escapa del banco durante la proyección
  if (state.playing && !drag) {
    const px0 = t2x(state.t);
    if (px0 > W * 0.82) scrollX += px0 - W * 0.82;
    else if (px0 < W * 0.06) scrollX = Math.max(0, scrollX + px0 - W * 0.06);
  }
  scrollX = Math.max(0, Math.min(scrollX, Math.max(0, totalDur() * pxs - W * 0.5)));

  // el tramo marcado con I/O: banda ámbar sobre la regla
  if (state.range && state.range.b > state.range.a) {
    const x0 = t2x(state.range.a), x1 = t2x(state.range.b);
    ctx.save();
    ctx.fillStyle = "rgba(232,160,26,0.25)";
    ctx.fillRect(x0, 0, x1 - x0, RULER_H);
    ctx.strokeStyle = "#e8a01a";
    ctx.lineWidth = 2;
    ctx.strokeRect(x0, 1, x1 - x0, RULER_H - 2);
    ctx.restore();
  }

  // las marcas: banderitas sobre la regla
  for (const mk of state.markers) {
    const x = t2x(mk.t);
    if (x < -20 || x > W + 20) continue;
    ctx.save();
    ctx.strokeStyle = "#e8a01a";
    ctx.fillStyle = "#e8a01a";
    ctx.lineWidth = 1.6;
    ctx.beginPath();
    ctx.moveTo(x, 2);
    ctx.lineTo(x, RULER_H - 4);
    ctx.stroke();
    ctx.beginPath();
    ctx.moveTo(x, 2);
    ctx.lineTo(x + 9, 5.5);
    ctx.lineTo(x, 9);
    ctx.closePath();
    ctx.fill();
    ctx.font = "600 11px 'Caveat', cursive";
    ctx.fillText(mk.name, x + 11, 9);
    ctx.restore();
  }

  // la línea de inserción del arrastre
  if (view.insertIdx !== null && drag?.kind === "move") {
    let t = 0;
    let i = 0;
    for (const c of state.clips) {
      if (c === drag.clip) continue;
      if (i === view.insertIdx) break;
      t += clipDur(c);
      i++;
    }
    const x = t2x(t);
    ctx.save();
    ctx.strokeStyle = "#e8501a";
    ctx.lineWidth = 3;
    ctx.setLineDash([6, 4]);
    ctx.beginPath();
    ctx.moveTo(x, TRACK_Y - 6);
    ctx.lineTo(x, TRACK_Y + STRIP_H + 6);
    ctx.stroke();
    ctx.restore();
  }

  // el flash del imán (o el tope de material, en rojo)
  if (magnetFlash && performance.now() < magnetFlash.until) {
    ctx.save();
    ctx.globalAlpha = Math.max(0, (magnetFlash.until - performance.now()) / 260);
    ctx.strokeStyle = magnetFlash.tope ? "#d93325" : "#f2c744";
    ctx.lineWidth = magnetFlash.tope ? 4 : 2.5;
    ctx.beginPath();
    ctx.moveTo(magnetFlash.x, RULER_H);
    ctx.lineTo(magnetFlash.x, TRACK_A_Y + TRACK_A_H);
    ctx.stroke();
    ctx.restore();
  }

  // la aguja
  spring(needle, t2x(state.t), dt);
  const px = drag?.kind === "playhead" ? t2x(state.t) : needle.x;
  ctx.strokeStyle = "#d93325";
  ctx.lineWidth = 1.7;
  ctx.beginPath();
  ctx.moveTo(px + 0.4, 2);
  ctx.quadraticCurveTo(px - 0.8, H * 0.5, px + 0.5, H - 2);
  ctx.stroke();
  ctx.fillStyle = "#d93325";
  ctx.beginPath();
  ctx.moveTo(px - 6, 1.5); ctx.quadraticCurveTo(px, -1.6, px + 6.5, 2);
  ctx.lineTo(px + 0.5, 10.5); ctx.closePath(); ctx.fill();

  if (tooltip) {
    ctx.save();
    ctx.font = "700 11px 'Courier New', monospace";
    const tw = ctx.measureText(tooltip.text).width + 12;
    const tx = Math.max(4, Math.min(W - tw - 4, tooltip.x - tw / 2));
    ctx.fillStyle = "#2b3bc7";
    ctx.fillRect(tx, tooltip.y - 14, tw, 17);
    ctx.fillStyle = "#f2eee4";
    ctx.fillText(tooltip.text, tx + 6, tooltip.y - 1);
    ctx.restore();
  }
}

function drawStrip(x, w, it, lifted) {
  const y = TRACK_Y, h = STRIP_H;
  const clip = it.clip;
  const sel = state.sel === clip.id || state.selSet.includes(clip.id);
  if (clip.gap) {
    // el hueco: negro rayado a lápiz, sin perforaciones
    ctx.save();
    ctx.fillStyle = "#14120e";
    ctx.fillRect(x, y, w, h);
    ctx.strokeStyle = "rgba(242,238,228,0.15)";
    ctx.lineWidth = 1;
    for (let hx = x - h; hx < x + w; hx += 14) {
      ctx.beginPath();
      ctx.moveTo(Math.max(x, hx), y + Math.max(0, x - hx));
      ctx.lineTo(Math.min(x + w, hx + h), y + Math.min(h, x + w - hx));
      ctx.stroke();
    }
    ctx.font = "600 13px 'Caveat', cursive";
    ctx.fillStyle = "rgba(242,238,228,0.5)";
    if (w > 40) ctx.fillText("hueco", x + 6, y + h / 2 + 4);
    if (sel) {
      ctx.strokeStyle = "#d93325"; ctx.lineWidth = 2;
      ctx.strokeRect(x - 1, y - 1, w + 2, h + 2);
    }
    ctx.restore();
    return;
  }
  ctx.save();
  if (lifted) {
    ctx.shadowColor = "#1a1a1a55"; ctx.shadowBlur = 12; ctx.shadowOffsetY = 6;
    ctx.translate(0, -4);
  } else {
    ctx.shadowColor = "#1a1a1a30"; ctx.shadowBlur = 3; ctx.shadowOffsetY = 2;
  }
  // el acetato
  ctx.fillStyle = "#1d1b16";
  ctx.fillRect(x, y, w, h);
  ctx.shadowColor = "transparent";

  // fotogramas reales
  const frameH = h - 26;
  const mAsp = state.media.find((mm) => mm.name === clip.media);
  const frameW = frameH * (mAsp && mAsp.h > 0 && mAsp.w > 0 ? mAsp.w / mAsp.h : 16 / 9);
  const clipStart = it.start;
  ctx.save();
  ctx.beginPath(); ctx.rect(x + 1, y + 13, w - 2, frameH); ctx.clip();
  for (let fx = 0; fx < w + frameW; fx += frameW + 3) {
    const tlT = clipStart + fx / pxs;
    const srcT = clip.in + (tlT - clipStart);
    if (srcT > clip.out) break;
    const im = thumb(clip.media, srcT);
    if (im) ctx.drawImage(im, x + fx + 1, y + 13, frameW, frameH);
    else { ctx.fillStyle = "#26231d"; ctx.fillRect(x + fx + 1, y + 13, frameW, frameH); }
    ctx.strokeStyle = "#0d0b08"; ctx.lineWidth = 3;
    ctx.strokeRect(x + fx + 0.5, y + 12.5, frameW + 1, frameH + 1);
  }
  ctx.restore();

  // la onda de sonido, revelada sobre el borde inferior de la tira
  const wim = wave(clip.media);
  const m = state.media.find((mm) => mm.name === clip.media);
  if (wim && m && m.dur > 0) {
    const sx = (clip.in / m.dur) * wim.naturalWidth;
    const sw = Math.max(1, ((clip.out - clip.in) / m.dur) * wim.naturalWidth);
    ctx.save();
    ctx.globalAlpha = 0.85;
    ctx.drawImage(wim, sx, 0, sw, wim.naturalHeight, x + 1, y + h - 24, w - 2, 15);
    ctx.restore();
  }

  // perforaciones
  ctx.fillStyle = "#f2eee4";
  const pitch = 12;
  for (let px2 = x + 5; px2 < x + w - 7; px2 += pitch) {
    ctx.beginPath(); ctx.roundRect(px2, y + 4, 6.5, 5, 1.5); ctx.fill();
    ctx.beginPath(); ctx.roundRect(px2, y + h - 9, 6.5, 5, 1.5); ctx.fill();
  }

  if (sel) {
    ctx.strokeStyle = "#d93325"; ctx.lineWidth = 2;
    ctx.strokeRect(x - 1, y - 1, w + 2, h + 2);
  }

  // rótulo a mano sobre el borde (como se rotula el negativo)
  if (w > 60) {
    ctx.save();
    ctx.translate(x + 6, y - 3);
    ctx.rotate(-0.01 + (clip.id % 3) * 0.01);
    ctx.font = "600 15px 'Caveat', 'Bradley Hand', cursive";
    ctx.fillStyle = "#2b3bc7";
    ctx.fillText((clip.label || clip.media.replace(/\.[^.]+$/, "")).slice(0, Math.floor(w / 8)), 0, 0);
    ctx.restore();
  }
  ctx.restore();
}
