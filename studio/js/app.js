// app.js — Laboratorios Saorín: la cola de cabecera, las tres salas,
// el pliegue del cuarto oscuro, el teclado y el sonido del taller.

import { clipAt } from "./state.js";
import { state, on, load, removeClip, removeAudioClip, selected, undo, redo,
         beginGesture, commitGesture, touch, emit, projFps, projDims,
         addMarker, nextPoint, prevPoint, copySelected, pasteClipboard,
         duplicateSelected, liftClip, snapT, reloadMedia } from "./state.js";
import { initViewer, toggle, stepFrames, setWipe, showFrameAt, moveCanvasTo, canvasHome, pause,
         closeSource, markIn, markOut, sourceToReel, toggleSource, openSource, shuttle } from "./viewer.js";
import { initTimeline, blade, cancelDrag, zoomFit } from "./timeline.js";
import { setPreviewQuality } from "./viewer.js";
import { initMesa } from "./mesa.js";
import { initDarkroom } from "./darkroom.js";
import { initRevelado } from "./revelado.js";
import { applyFromState } from "./luts.js";
import * as foley from "./foley.js";

/* ── las salas y el pliegue ── */
let room = "mesa";
let plegando = false;

async function go(dest) {
  if (dest === room || plegando) return;
  const involucraOscuro = dest === "dark" || room === "dark";
  foley.page();
  if (involucraOscuro) {
    // el papel terracota envuelve la pantalla, y dentro cambia la sala
    plegando = true;
    const p = document.getElementById("pliegue");
    p.classList.remove("hidden");
    foley.cord();
    requestAnimationFrame(() => p.classList.add("cerrando"));
    await new Promise((r) => setTimeout(r, 460));
    swapRoom(dest);
    await new Promise((r) => setTimeout(r, 120));
    p.classList.remove("cerrando");
    await new Promise((r) => setTimeout(r, 460));
    p.classList.add("hidden");
    plegando = false;
  } else {
    swapRoom(dest);
  }
}

function swapRoom(dest) {
  room = dest;
  state.room = dest;
  document.body.className = "room-" + dest;
  document.querySelectorAll(".puerta").forEach((b) =>
    b.classList.toggle("active", b.dataset.room === dest));
  if (dest === "dark") {
    moveCanvasTo(document.getElementById("ampliadora-hueco"));
  } else {
    canvasHome();
  }
  if (dest === "rev") pause();
}

document.querySelectorAll(".puerta").forEach((b) => {
  b.addEventListener("click", () => go(b.dataset.room));
});

/* ── sonido del taller ── */
const muteBtn = document.getElementById("btn-mute");
muteBtn.classList.toggle("muted", foley.isMuted());
muteBtn.addEventListener("click", () => {
  foley.setMuted(!foley.isMuted());
  muteBtn.classList.toggle("muted", foley.isMuted());
  if (!foley.isMuted()) foley.press();
});

/* ── transporte ── */
document.getElementById("btn-play").addEventListener("click", toggle);
document.getElementById("btn-play-dark").addEventListener("click", toggle);
document.getElementById("btn-back").addEventListener("click", () => { stepFrames(-1); foley.tick(); });
document.getElementById("btn-fwd").addEventListener("click", () => { stepFrames(1); foley.tick(); });
document.getElementById("btn-start").addEventListener("click", () => { state.t = 0; showFrameAt(0); foley.press(); });
document.getElementById("btn-end").addEventListener("click", () => { stepFrames(1e9); foley.press(); });
document.getElementById("chk-wipe").addEventListener("change", (e) => { setWipe(e.target.checked); foley.press(); });
document.getElementById("chk-fina")?.addEventListener("change", (e) => {
  setPreviewQuality(e.target.checked ? 1 : 0.5);
  foley.press();
});
// doble clic en el vidrio: pantalla completa (esc sale, como manda la casa)
document.querySelector(".moviola-marco")?.addEventListener("dblclick", () => {
  const marco = document.querySelector(".moviola-marco");
  if (document.fullscreenElement) document.exitFullscreen();
  else marco.requestFullscreen?.();
});

on("transport", () => {
  for (const id of ["playmark"]) {
    const use = document.querySelector("#" + id + " use");
    if (use) use.setAttribute("href", state.playing ? "#mk-pause" : "#mk-play");
  }
  const useD = document.querySelector("#btn-play-dark use");
  if (useD) useD.setAttribute("href", state.playing ? "#mk-pause" : "#mk-play");
  document.getElementById("btn-play").classList.toggle("playing", state.playing);
  document.getElementById("btn-play-dark").classList.toggle("playing", state.playing);
  if (state.playing) foley.motorStart(); else foley.motorStop();
});

on("dirty", () => {
  document.getElementById("save-state").classList.toggle("dirty", state.dirty);
});

/* ── el inspector de sonido del clip seleccionado ── */
const insp = document.getElementById("insp");
const inspNombre = document.getElementById("insp-nombre");
const iGain = document.getElementById("i-gain");
const iGainV = document.getElementById("i-gain-v");
const iMute = document.getElementById("i-mute");
const iFin = document.getElementById("i-fin");
const iFout = document.getElementById("i-fout");

const inspTf = document.getElementById("insp-tf");
const tScale = document.getElementById("t-scale");
const tScaleV = document.getElementById("t-scale-v");
const tRot = document.getElementById("t-rot");
const tX = document.getElementById("t-x");
const tY = document.getElementById("t-y");
const tFill = document.getElementById("t-fill");
const tSpeed = document.getElementById("t-speed");

function refreshInsp() {
  const s = selected();
  insp.classList.toggle("hidden", !s);
  document.getElementById("pista-hint")?.classList.toggle("hidden", !!s);
  if (!s) return;
  const c = s.clip;
  inspNombre.textContent = (s.kind === "audio" ? "♪ " : "") +
    c.media.replace(/\.[^.]+$/, "").slice(0, 18);
  iGain.value = c.gain || 0;
  iGainV.textContent = `${c.gain > 0 ? "+" : ""}${c.gain || 0} dB`;
  iMute.checked = !!c.mute;
  iFin.value = c.fadeIn || 0;
  iFout.value = c.fadeOut || 0;
  // el encuadre: solo clips de vídeo
  inspTf.classList.toggle("hidden", s.kind !== "video");
  if (s.kind === "video") {
    const tf = c.tf || {};
    tScale.value = tf.scale || 1;
    tScaleV.textContent = Math.round((tf.scale || 1) * 100) + "%";
    tRot.value = tf.rot || 0;
    tX.value = tf.x || 0;
    tY.value = tf.y || 0;
    tFill.checked = tf.fit === "fill";
    tSpeed.value = c.speed || 1;
  }
}
on("timeline", refreshInsp);

/* la ficha del proyecto: aspecto + resolución/fps reales */
const projSel = document.getElementById("proj-aspect");
const projInfo = document.getElementById("proj-info");
function refreshProj() {
  projSel.value = state.project.aspect || "auto";
  const d = projDims();
  projInfo.textContent = `${d.w}×${d.h} · ${(+projFps().toFixed(2))} fps`;
}
projSel.addEventListener("change", () => {
  state.project.aspect = projSel.value;
  foley.press();
  touch(); emit("timeline");
  refreshProj();
});
on("timeline", refreshProj);
on("media", refreshProj);

/* el encuadre por clip */
function tfApply(fn) {
  const s = selected();
  if (!s || s.kind !== "video") return;
  s.clip.tf = s.clip.tf || {};
  fn(s.clip.tf);
  // limpiar identidades para no ensuciar el proyecto
  const tf = s.clip.tf;
  if ((tf.scale || 1) === 1 && !(tf.rot || 0) && !(tf.x || 0) && !(tf.y || 0) && tf.fit !== "fill") {
    delete s.clip.tf;
  }
  touch(); emit("timeline");
}
tScale.addEventListener("pointerdown", beginGesture);
tScale.addEventListener("input", () => {
  tfApply((tf) => { tf.scale = +tScale.value; });
  tScaleV.textContent = Math.round(+tScale.value * 100) + "%";
});
tScale.addEventListener("change", () => { commitGesture(); foley.tick(); });
for (const [el, key] of [[tRot, "rot"], [tX, "x"], [tY, "y"]]) {
  el.addEventListener("change", () => {
    beginGesture();
    tfApply((tf) => { tf[key] = +el.value || 0; });
    commitGesture(); foley.tick();
  });
}
tSpeed.addEventListener("change", () => {
  const s2 = selected();
  if (!s2 || s2.kind !== "video" || s2.clip.gap) return;
  beginGesture();
  const v = Math.max(0.25, Math.min(4, +tSpeed.value || 1));
  if (v === 1) delete s2.clip.speed; else s2.clip.speed = v;
  commitGesture();
  touch(); emit("timeline");
  foley.tick();
});
tFill.addEventListener("change", () => {
  beginGesture();
  tfApply((tf) => { if (tFill.checked) tf.fit = "fill"; else delete tf.fit; });
  commitGesture(); foley.press();
});

function inspApply(fn) {
  const s = selected();
  if (!s) return;
  fn(s.clip);
  touch(); emit("timeline");
}
iGain.addEventListener("pointerdown", beginGesture);
iGain.addEventListener("input", () => {
  inspApply((c) => { c.gain = +iGain.value; });
  iGainV.textContent = `${+iGain.value > 0 ? "+" : ""}${iGain.value} dB`;
});
iGain.addEventListener("change", () => { commitGesture(); foley.tick(); });
iMute.addEventListener("change", () => {
  beginGesture();
  inspApply((c) => { c.mute = iMute.checked; });
  commitGesture(); foley.press();
});
for (const [el, key] of [[iFin, "fadeIn"], [iFout, "fadeOut"]]) {
  el.addEventListener("change", () => {
    beginGesture();
    inspApply((c) => { c[key] = Math.max(0, +el.value || 0); });
    commitGesture(); foley.tick();
  });
}

/* ── la barra de fuente ── */
document.getElementById("f-in")?.addEventListener("click", () => { markIn(); foley.tick(); });
document.getElementById("f-out")?.addEventListener("click", () => { markOut(); foley.tick(); });
document.getElementById("f-add")?.addEventListener("click", () => sourceToReel());
document.getElementById("f-close")?.addEventListener("click", () => { closeSource(); foley.press(); });

/* ── teclado ── */
// los controles no-de-texto sueltan el foco al usarse: los atajos nunca
// acaban escribiendo en un slider ni toggling un checkbox por accidente
document.addEventListener("pointerup", () => {
  const el = document.activeElement;
  if (el && el !== document.body &&
      (el.tagName === "BUTTON" ||
       (el.tagName === "INPUT" && !["text", "number", "search"].includes(el.type)))) {
    el.blur();
  }
});

const isTyping = (el) =>
  el && (el.tagName === "TEXTAREA" || el.isContentEditable ||
    (el.tagName === "INPUT" && ["text", "number", "search", "email", "url", "password"].includes(el.type)));

window.addEventListener("keydown", (e) => {
  if (isTyping(e.target)) {
    if (e.code === "Escape") e.target.blur();
    return;
  }
  // deshacer/rehacer de la bobina
  if ((e.metaKey || e.ctrlKey) && e.code === "KeyZ") {
    e.preventDefault();
    if (e.shiftKey ? redo() : undo()) foley.thunk();
    return;
  }
  if ((e.metaKey || e.ctrlKey) && e.code === "KeyY") {
    e.preventDefault();
    if (redo()) foley.thunk();
    return;
  }
  // portapapeles de clips
  if ((e.metaKey || e.ctrlKey) && e.code === "KeyC") {
    if (copySelected()) { e.preventDefault(); foley.tick(); }
    return;
  }
  if ((e.metaKey || e.ctrlKey) && e.code === "KeyX") {
    const s2 = selected();
    if (s2 && copySelected()) {
      e.preventDefault();
      if (s2.kind === "video") removeClip(s2.clip.id);
      else removeAudioClip(s2.clip.id);
      foley.chunk();
    }
    return;
  }
  if ((e.metaKey || e.ctrlKey) && e.code === "KeyV") {
    if (pasteClipboard()) { e.preventDefault(); foley.thunk(); }
    return;
  }
  if ((e.metaKey || e.ctrlKey) && e.code === "KeyD") {
    e.preventDefault();
    if (duplicateSelected()) foley.thunk();
    return;
  }
  // la fuente tiene su propio transporte
  if (state.source) {
    switch (e.code) {
      case "Space": e.preventDefault(); toggleSource(); return;
      case "Escape": closeSource(); foley.press(); return;
      case "KeyI": markIn(); foley.tick(); return;
      case "KeyO": markOut(); foley.tick(); return;
      case "Enter": sourceToReel(e.shiftKey); return;
      case "ArrowLeft": stepFrames(e.shiftKey ? -Math.round(state.source.fps || 25) : -1); return;
      case "ArrowRight": stepFrames(e.shiftKey ? Math.round(state.source.fps || 25) : 1); return;
    }
  }
  switch (e.code) {
    case "Space": e.preventDefault(); toggle(); break;
    case "Escape": if (cancelDrag()) foley.press(); break;
    case "ArrowLeft": stepFrames(e.shiftKey ? -Math.round(projFps()) : -1); break;
    case "ArrowRight": stepFrames(e.shiftKey ? Math.round(projFps()) : 1); break;
    case "KeyB": if (room === "mesa") blade(); break;
    case "Delete": case "Backspace":
      if (room !== "mesa") break;
      if (state.selSet.length > 1) {
        // multi-selección: todos al saco de una vez
        for (const id2 of [...state.selSet]) {
          if (e.shiftKey) liftClip(id2); else removeClip(id2);
        }
        state.selSet = [];
        foley.thunk();
      } else if (state.sel != null) {
        if (e.shiftKey) liftClip(state.sel); else removeClip(state.sel);
        foley.thunk();
      } else if (state.selAudio != null) { removeAudioClip(state.selAudio); foley.thunk(); }
      break;
    case "KeyW": {
      const c = document.getElementById("chk-wipe");
      c.checked = !c.checked; setWipe(c.checked); break;
    }
    case "KeyD": go(room === "dark" ? "mesa" : "dark"); break;
    case "Home": state.t = 0; showFrameAt(0); break;
    case "End": stepFrames(1e9); break;
    case "KeyM": if (room === "mesa") { addMarker(state.t); foley.tick(); } break;
    case "KeyI":
      // el tramo: entrada del rango sobre la bobina (⇧I lo quita)
      if (room !== "mesa") break;
      if (e.shiftKey) { state.range = null; }
      else {
        const b0 = state.range?.b ?? null;
        state.range = { a: snapT(state.t), b: b0 !== null && b0 > state.t ? b0 : snapT(state.t) };
      }
      touch(); emit("timeline"); foley.tick();
      break;
    case "KeyO":
      if (room !== "mesa") break;
      {
        const a0 = state.range?.a ?? 0;
        state.range = { a: Math.min(a0, snapT(state.t)), b: snapT(state.t) };
      }
      touch(); emit("timeline"); foley.tick();
      break;
    case "KeyR":
      state.loop = !state.loop;
      foley.press();
      break;
    case "KeyZ":
      if (e.shiftKey && !(e.metaKey || e.ctrlKey)) { zoomFit(); foley.tick(); }
      break;
    case "Slash": if (e.shiftKey) { toggleCheatsheet(); foley.page(); } break;
    case "KeyJ": shuttle(-1); break;
    case "KeyK": pause(); break;
    case "KeyL": shuttle(1); break;
    case "ArrowUp": {
      const p2 = prevPoint(state.t, e.shiftKey);
      if (p2 !== null) { pause(); state.t = p2; showFrameAt(p2); foley.tick(); }
      e.preventDefault(); break;
    }
    case "ArrowDown": {
      const n2 = nextPoint(state.t, e.shiftKey);
      if (n2 !== null) { pause(); state.t = n2; showFrameAt(n2); foley.tick(); }
      e.preventDefault(); break;
    }
  }
});

/* ── la cola de cabecera ── */
function leader() {
  return new Promise((resolve) => {
    const cv = document.getElementById("leader");
    const g = cv.getContext("2d");
    cv.width = innerWidth; cv.height = innerHeight;
    const t0 = performance.now();
    const DUR = 2600;
    let beeped = -1;
    let skip = false;
    cv.addEventListener("click", () => { skip = true; }, { once: true });
    (function frame(now) {
      const el = now - t0;
      if (skip || el > DUR) {
        cv.classList.add("hidden");
        return resolve();
      }
      requestAnimationFrame(frame);
      const n = 3 - Math.floor(el / (DUR / 3));           // 3, 2, 1
      const sweep = (el % (DUR / 3)) / (DUR / 3);         // barrido del radar
      const W = cv.width, H = cv.height;
      // papel de la cola: gris azulado con parpadeo de proyector
      const flick = 0.94 + 0.06 * Math.random();
      g.fillStyle = `rgb(${Math.round(146 * flick)}, ${Math.round(149 * flick)}, ${Math.round(158 * flick)})`;
      g.fillRect(0, 0, W, H);
      const cx = W / 2, cy = H / 2, R = Math.min(W, H) * 0.34;
      // barrido
      g.fillStyle = "#7d818c";
      g.beginPath();
      g.moveTo(cx, cy);
      g.arc(cx, cy, R * 1.35, -Math.PI / 2, -Math.PI / 2 + sweep * Math.PI * 2);
      g.closePath(); g.fill();
      // círculos
      g.strokeStyle = "#3a3d46"; g.lineWidth = 3;
      for (const r of [R, R * 0.72]) { g.beginPath(); g.arc(cx, cy, r, 0, 7); g.stroke(); }
      // retícula
      g.lineWidth = 1.5;
      g.beginPath(); g.moveTo(0, cy); g.lineTo(W, cy); g.stroke();
      g.beginPath(); g.moveTo(cx, 0); g.lineTo(cx, H); g.stroke();
      // el número
      g.fillStyle = "#23252c";
      g.font = `900 ${R * 1.15}px 'Arial Narrow', Impact, sans-serif`;
      g.textAlign = "center"; g.textBaseline = "middle";
      g.fillText(String(n), cx, cy + R * 0.06);
      // rótulo
      g.font = "12px 'Courier New', monospace";
      g.fillStyle = "#3a3d46";
      g.fillText("LABORATORIOS SAORÍN · cola de cabecera · " + new Date().getFullYear(), cx, H - 28);
      // polvo
      for (let i = 0; i < 14; i++) {
        g.fillStyle = "#00000022";
        g.fillRect(Math.random() * W, Math.random() * H, 1.6, Math.random() * 8);
      }
      if (n !== beeped && n <= 2) { beeped = n; foley.beep(); }
    })(t0);
  });
}

/* ── drag&drop del Finder: los bytes suben y el taller los guarda ── */
window.addEventListener("dragover", (e) => { e.preventDefault(); });
window.addEventListener("drop", async (e) => {
  e.preventDefault();
  // en la app nativa esto no llega (lo coge Tauri con las rutas reales);
  // en un navegador suelto, subimos los bytes
  const files = [...(e.dataTransfer?.files || [])];
  if (!files.length) return;
  foley.press();
  let added = 0;
  for (const f of files) {
    try {
      const r = await (await fetch(`/api/upload?name=${encodeURIComponent(f.name)}`, {
        method: "POST", body: f,
      })).json();
      if (r.name) added++;
    } catch {}
  }
  if (added) { await reloadMedia(); foley.thunk(); }
});

/* ── la chuleta de atajos: tecla ? ── */
function toggleCheatsheet() {
  let el = document.getElementById("chuleta");
  if (el) { el.remove(); return; }
  el = document.createElement("div");
  el.id = "chuleta";
  el.innerHTML = `<div class="ch-inner">
    <div class="ch-title">la chuleta del taller</div>
    <div class="ch-cols"><dl>
      <dt>espacio</dt><dd>proyectar / parar</dd>
      <dt>J · K · L</dt><dd>lanzadera (atrás · alto · adelante)</dd>
      <dt>← →</dt><dd>un fotograma (⇧ = un segundo)</dd>
      <dt>↑ ↓</dt><dd>corte anterior / siguiente (⇧ = marcas)</dd>
      <dt>B</dt><dd>empalmadora: marcar, luego cortar</dd>
      <dt>M</dt><dd>marca aquí</dd>
      <dt>⌫</dt><dd>el clip al saco de recortes</dd>
    </dl><dl>
      <dt>⌘Z · ⌘⇧Z</dt><dd>deshacer / rehacer</dd>
      <dt>⌘C · ⌘X · ⌘V · ⌘D</dt><dd>copiar · cortar · pegar · duplicar</dd>
      <dt>I · O · ⏎</dt><dd>fuente: entrada · salida · a la bobina</dd>
      <dt>W</dt><dd>tira de prueba a/b</dd>
      <dt>D</dt><dd>el cuarto oscuro</dd>
      <dt>esc</dt><dd>cancelar el gesto / cerrar</dd>
      <dt>clic derecho</dt><dd>el menú del banco</dd>
    </dl></div>
    <div class="ch-nota">pulsa ? para cerrar</div>
  </div>`;
  el.addEventListener("click", () => el.remove());
  document.body.append(el);
}

/* ── la ampliadora NATIVA: ventana wgpu propia, resolución completa ──
   La webview tiene techo (WebGL a media resolución); esta ventana usa el
   MISMO motor del render, así que lo que ves es exactamente el máster. */
let nativaOn = false;
let nativaUlt = 0;
export function nativaEmpuja(force = false) {
  if (!nativaOn) return;
  const ahora = performance.now();
  if (!force && ahora - nativaUlt < 90) return;   // ~11 órdenes/s
  nativaUlt = ahora;
  const hit = clipAt(state.t);
  if (!hit || hit.clip.gap) return;
  fetch("/api/preview", {
    method: "POST",
    body: JSON.stringify({
      clip: hit.clip.media,
      t: hit.clip.in + hit.offset * (hit.clip.speed || 1),
      play: state.playing,
      prefs: state.prefs,
      lut_in: state.lutEntrada,
      lut: state.lutColor,
    }),
  }).catch(() => {});
}
document.getElementById("btn-nativa")?.addEventListener("click", async () => {
  nativaOn = !nativaOn;
  document.getElementById("btn-nativa").classList.toggle("fuerte", nativaOn);
  foley.press();
  if (nativaOn) nativaEmpuja(true);
  else await fetch("/api/preview/stop", { method: "POST" }).catch(() => {});
});
on("time", () => nativaEmpuja());
on("transport", () => nativaEmpuja(true));
on("prefs", () => nativaEmpuja(true));

/* ── la estantería se refresca sola cuando sueltas ficheros (drag nativo) ── */
(function vigilaEstanteria() {
  let v = -1;
  setInterval(async () => {
    try {
      const r = await (await fetch("/api/media-version")).json();
      if (v < 0) { v = r.v; return; }
      if (r.v !== v) {
        v = r.v;
        await reloadMedia();
        foley.thunk();
      }
    } catch {}
  }, 1200);
})();

/* ── los ajustes del taller ── */
document.getElementById("btn-ajustes")?.addEventListener("click", async () => {
  foley.press();
  let eng = {}, dirs = {};
  try { eng = await (await fetch("/api/engine")).json(); } catch {}
  const el = document.createElement("div");
  el.id = "chuleta";   // reutiliza el telón de la chuleta
  const fina = localStorage.getItem("fl-fina") === "1";
  el.innerHTML = `<div class="ch-inner">
    <div class="ch-title">ajustes del taller</div>
    <dl style="display:grid;grid-template-columns:auto 1fr;gap:8px 16px;text-align:left">
      <dt class="susurro-titulo">lupa fina</dt>
      <dd><label class="susurro"><input type="checkbox" id="aj-fina" ${fina ? "checked" : ""}> preview a resolución completa</label></dd>
      <dt class="susurro-titulo">sonido del taller</dt>
      <dd><label class="susurro"><input type="checkbox" id="aj-foley" ${foley.isMuted() ? "" : "checked"}> foley de la interfaz</label></dd>
      <dt class="susurro-titulo">motor</dt>
      <dd class="susurro">${eng.name || "—"} · tier ${eng.tier || "?"}${eng.zero_copy ? " · zero-copy" : ""}</dd>
      <dt class="susurro-titulo">atajos</dt>
      <dd class="susurro">pulsa <b>?</b> para la chuleta completa</dd>
    </dl>
    <div class="ch-nota">clic fuera para cerrar</div>
  </div>`;
  el.addEventListener("click", (e) => { if (e.target === el) el.remove(); });
  el.querySelector("#aj-fina").addEventListener("change", (e) => {
    localStorage.setItem("fl-fina", e.target.checked ? "1" : "0");
    setPreviewQuality(e.target.checked ? 1 : 0.5);
  });
  el.querySelector("#aj-foley").addEventListener("change", (e) => {
    foley.setMuted(!e.target.checked);
  });
  document.body.append(el);
});
// la lupa fina se recuerda entre sesiones
if (localStorage.getItem("fl-fina") === "1") setPreviewQuality(1);

/* ── la portada: elegir bobina al entrar (pantalla de bienvenida) ── */
async function portada() {
  let info = { current: "", projects: [] };
  try { info = await (await fetch("/api/projects")).json(); } catch {}
  return new Promise((done) => {
    const el = document.createElement("div");
    el.id = "portada";
    const lista = [
      { v: "", txt: "bobina clásica" },
      ...info.projects.map((n) => ({ v: n, txt: n })),
    ];
    el.innerHTML = `<div class="p-inner">
      <div class="p-masthead"><span class="shout" data-ink="LABORATORIOS">LABORATORIOS</span>
      <span class="shout tinta" data-ink="SAORÍN">SAORÍN</span></div>
      <div class="susurro-titulo">¿qué bobina montamos hoy?</div>
      <div class="p-lista"></div>
      <button class="inkbtn" id="p-nueva">＋ nueva bobina</button>
    </div>`;
    const cont = el.querySelector(".p-lista");
    for (const b of lista) {
      const btn = document.createElement("button");
      btn.className = "p-bobina" + ((b.v === info.current) ? " actual" : "");
      btn.textContent = "· " + b.txt + (b.v === info.current ? "  (donde estaba)" : "");
      btn.addEventListener("click", async () => {
        foley.thunk();
        await fetch("/api/projects/open", { method: "POST", body: JSON.stringify({ name: b.v }) });
        el.remove();
        done();
      });
      cont.append(btn);
    }
    el.querySelector("#p-nueva").addEventListener("click", async () => {
      const name = prompt("nombre de la nueva bobina");
      if (!name || !name.trim()) return;
      await fetch("/api/projects/new", { method: "POST", body: JSON.stringify({ name: name.trim() }) });
      el.remove();
      done();
    });
    document.body.append(el);
  });
}

/* ── las bobinas (proyectos): el selector del rótulo ── */
async function initProjects() {
  const sel = document.getElementById("proj-sel");
  if (!sel) return;
  let info = { current: "", projects: [] };
  try { info = await (await fetch("/api/projects")).json(); } catch {}
  sel.innerHTML = "";
  const mk = (v, txt) => {
    const o = document.createElement("option");
    o.value = v; o.textContent = txt;
    sel.append(o);
  };
  mk("", "· bobina clásica");
  for (const n of info.projects) mk(n, "· " + n);
  mk("__new__", "＋ nueva bobina…");
  sel.value = info.projects.includes(info.current) ? info.current : "";
  sel.addEventListener("change", async () => {
    if (sel.value === "__new__") {
      const name = prompt("nombre de la nueva bobina");
      if (!name || !name.trim()) { sel.value = info.current || ""; return; }
      await fetch("/api/projects/new", { method: "POST", body: JSON.stringify({ name: name.trim() }) });
    } else {
      await fetch("/api/projects/open", { method: "POST", body: JSON.stringify({ name: sel.value }) });
    }
    location.reload();   // la bobina nueva entra limpia, con todo su estado
  });
}

/* ── arranque ── */
(async () => {
  // el cajón de marcas dibujadas del estudio (doodles del kit)
  try {
    const svg = await (await fetch("/zine/doodles.svg")).text();
    const div = document.createElement("div");
    div.innerHTML = svg;
    document.body.prepend(div.firstElementChild);
  } catch {}
  const q0 = new URLSearchParams(location.search);
  if (!q0.has("rapido") && !q0.has("sala")) await portada();
  await initProjects();
  await load();
  // si el motor del visor no puede (GPU caída, WebGL fuera), el resto del
  // taller sigue en pie: nunca un fallo del vidrio tira la mesa entera
  try {
    await initViewer();
    await applyFromState();   // las gelatinas del proyecto (o las de la casa)
  } catch (e) {
    console.warn("el visor no arranca (se sigue sin proyector):", e);
    document.getElementById("viewer-overlay")?.classList.remove("hidden");
    const ov = document.getElementById("viewer-overlay");
    if (ov) ov.textContent = "el proyector no arranca (¿GPU?) — la mesa sigue operativa";
  }
  initTimeline();
  initMesa();
  initDarkroom();
  initRevelado();
  const q = new URLSearchParams(location.search);
  if (q.has("rapido")) document.getElementById("leader").classList.add("hidden");
  else await leader();
  if (q.get("sala")) swapRoom(q.get("sala"));
  if (q.has("cajon")) document.getElementById("cajon").classList.remove("cerrado");
  if (q.get("fuente")) openSource(q.get("fuente"));
  if (q.get("lupa")) {
    const z = document.getElementById("tl-zoom");
    z.value = q.get("lupa");
    z.dispatchEvent(new Event("input"));
  }
  if (q.get("aguja")) {
    state.t = parseFloat(q.get("aguja"));
    showFrameAt(state.t);
  }
  if (q.has("contacto")) {
    const l = document.querySelector(".lata");
    if (l) l.dispatchEvent(new Event("mouseenter"));
  }
  if (state.clips.length) showFrameAt(0);

  // banco de pruebas: ?probar=6 → reproduce 6 s y reporta la cadencia real
  if (q.has("probar")) {
    const segs = parseFloat(q.get("probar")) || 6;
    const di = (m) => fetch("/api/log", { method: "POST", body: m });
    await new Promise((r) => setTimeout(r, 2500));   // que abran los carretes
    const { framesPintados } = await import("./viewer.js");
    const t0 = performance.now();
    const p0 = framesPintados();
    state.t = 0;
    toggle();                                        // play
    await new Promise((r) => setTimeout(r, segs * 1000));
    const dt = (performance.now() - t0) / 1000;
    const n = framesPintados() - p0;
    di(`REPRODUCCIÓN: ${n} frames en ${dt.toFixed(1)} s = ${(n / dt).toFixed(1)} fps · aguja=${state.t.toFixed(2)} s · playing=${state.playing}`);
    pause();
    // y el scrub: 12 saltos seguidos
    const s0 = performance.now();
    for (let i = 0; i < 12; i++) {
      state.t = (i * 0.37) % Math.max(0.5, totalDurSafe());
      await showFrameAt(state.t);
    }
    di(`SCRUB: 12 saltos en ${(performance.now() - s0).toFixed(0)} ms (${((performance.now() - s0) / 12).toFixed(0)} ms/salto)`);
    di("FIN");
  }
})();

function totalDurSafe() {
  try { return state.clips.reduce((a, c) => a + Math.max((c.out - c.in) / (c.speed || 1), 0.1), 0); }
  catch { return 1; }
}
