// menu.js — el menú contextual del banco: papel, tinta y pocas cosas claras.

import { state, removeClip, removeAudioClip, duplicateSelected, pasteClipboard,
         detachAudio, addMarker, removeMarker, splitAt, snapshot, touch, emit,
         insertGapAt, liftClip } from "./state.js";
import * as foley from "./foley.js";

let el = null;

function ensure() {
  if (el) return el;
  el = document.createElement("div");
  el.id = "ctx-menu";
  el.className = "hidden";
  document.body.append(el);
  const close = () => el.classList.add("hidden");
  window.addEventListener("pointerdown", (e) => { if (!el.contains(e.target)) close(); });
  window.addEventListener("keydown", (e) => { if (e.code === "Escape") close(); });
  window.addEventListener("blur", close);
  return el;
}

function show(px, py, items) {
  const m = ensure();
  m.innerHTML = "";
  for (const it of items) {
    if (it === "—") {
      const hr = document.createElement("div");
      hr.className = "ctx-sep";
      m.append(hr);
      continue;
    }
    const [label, fn] = it;
    const b = document.createElement("button");
    b.textContent = label;
    b.addEventListener("click", () => {
      m.classList.add("hidden");
      foley.press();
      fn();
    });
    m.append(b);
  }
  m.classList.remove("hidden");
  const mw = 200, mh = items.length * 26 + 12;
  m.style.left = Math.min(px, innerWidth - mw - 8) + "px";
  m.style.top = Math.min(py, innerHeight - mh - 8) + "px";
}

function setFade(c, dur, type) {
  snapshot();
  c.fade = dur;
  if (type && type !== "fade") c.fadeType = type;
  else delete c.fadeType;
  touch(); emit("timeline");
}

/** construye y muestra el menú para lo que haya bajo el cursor */
export function menuFor(px, py, hit, t) {
  const items = [];
  const near = state.markers.find((mk) => Math.abs(mk.t - t) < 0.4);

  if (hit.kind === "junta") {
    const c = hit.it.clip;
    items.push(["corte seco", () => setFade(c, 0, null)]);
    items.push(["fundido ½ s", () => setFade(c, 0.5, "fade")]);
    items.push(["fundido 1 s", () => setFade(c, 1, "fade")]);
    items.push(["fundido a negro", () => setFade(c, c.fade || 1, "fadeblack")]);
    items.push(["fundido a blanco", () => setFade(c, c.fade || 1, "fadewhite")]);
    items.push(["duración…", () => {
      const v = parseFloat(prompt("segundos de fundido", c.fade || 1));
      if (v >= 0) setFade(c, v, v > 0 ? (c.fadeType || "fade") : null);
    }]);
  } else if (hit.kind === "move" || hit.kind === "trimL" || hit.kind === "trimR") {
    const c = hit.it.clip;
    items.push(["cortar aquí", () => splitAt(t)]);
    items.push(["duplicar", () => {
      state.sel = c.id; state.selAudio = null;
      duplicateSelected();
    }]);
    items.push(["separar el audio", () => detachAudio(c.id)]);
    items.push(["renombrar…", () => {
      const v = prompt("nombre del clip", c.label || c.media.replace(/\.[^.]+$/, ""));
      if (v != null) { snapshot(); c.label = v.trim() || undefined; touch(); emit("timeline"); }
    }]);
    items.push(["hueco después", () => insertGapAt(state.clips.indexOf(c) + 1, 2)]);
    items.push("—");
    items.push(["al saco (cerrar el hueco)", () => removeClip(c.id)]);
    items.push(["quitar dejando hueco", () => liftClip(c.id)]);
  } else if (hit.kind === "moveA" || hit.kind === "trimAL" || hit.kind === "trimAR") {
    const a = hit.a;
    items.push(["duplicar", () => {
      state.selAudio = a.id; state.sel = null;
      duplicateSelected();
    }]);
    items.push(["quitar de la pista", () => removeAudioClip(a.id)]);
  } else {
    if (near) {
      items.push([`quitar la marca «${near.name}»`, () => removeMarker(near.id)]);
      items.push(["renombrar la marca…", () => {
        const v = prompt("nombre de la marca", near.name);
        if (v != null) { snapshot(); near.name = v.trim() || near.name; touch(); emit("timeline"); }
      }]);
    } else {
      items.push(["marca aquí  [M]", () => addMarker(t)]);
    }
    items.push(["hueco al final (2 s)", () => insertGapAt(state.clips.length, 2)]);
    items.push(["pegar", () => pasteClipboard()]);
  }
  show(px, py, items);
}
