// mesa.js — la sala de montaje: la estantería de latas y el saco de recortes.

import { state, on, addClip, addAudioAt, rescueFromBin, reloadMedia, save, snapshot } from "./state.js";
import { showFrameAt, openSource } from "./viewer.js";
import * as foley from "./foley.js";

/* la transparencia de los proxies: puntito por lata + estado global */
const proxyState = new Map();   // name → true (listo) | false (cociéndose)
async function pollProxyBadges() {
  let pending = 0;
  for (const m of state.media) {
    if (m.kind === "audio" || proxyState.get(m.name) === true) continue;
    pending++;
    try {
      const r = await (await fetch(`/api/proxy?f=${encodeURIComponent(m.name)}`)).json();
      proxyState.set(m.name, !!r.ready);
    } catch {}
  }
  document.querySelectorAll(".lata").forEach((el) => {
    const name = el.dataset.media;
    if (!name) return;
    el.classList.toggle("proxy-ok", proxyState.get(name) === true);
    el.classList.toggle("proxy-cociendo", proxyState.get(name) === false);
  });
  // el estado global, susurrado bajo el botón de importar
  const listos = [...proxyState.values()].filter(Boolean).length;
  const total = state.media.filter((m) => m.kind !== "audio").length;
  let nota = document.getElementById("proxy-nota");
  if (!nota) {
    nota = document.createElement("div");
    nota.id = "proxy-nota";
    nota.className = "susurro";
    document.getElementById("btn-importar")?.after(nota);
  }
  nota.textContent = total && listos < total
    ? `proxies: ${listos}/${total} cociéndose…`
    : total ? "proxies listos · scrub instantáneo" : "";
  if (pending > 0) setTimeout(pollProxyBadges, 3000);
}

export function initMesa() {
  on("media", renderLatas);
  on("media", pollProxyBadges);
  pollProxyBadges();
  on("bin", renderSaco);
  renderLatas();
  renderSaco();
  const imp = document.getElementById("btn-importar");
  imp?.addEventListener("click", async () => {
    foley.press();
    imp.disabled = true;
    imp.classList.add("espera");
    try {
      const r = await (await fetch("/api/import/dialog")).json();
      if (r.added?.length) {
        await reloadMedia();
        foley.thunk();
      } else {
        // el diálogo se cerró sin elegir, o el formato no vale: dilo
        const n = document.getElementById("proxy-nota");
        if (n) {
          n.textContent = "no entró material — arrastra las cintas a la ventana";
          setTimeout(() => { n.textContent = ""; }, 4000);
        }
      }
    } finally {
      imp.disabled = false;
      imp.classList.remove("espera");
    }
  });
}

const thumbUrl = (name, t) => `/api/thumb?f=${encodeURIComponent(name)}&t=${Math.max(0.2, t).toFixed(1)}`;

let contacto = null;
function hojaDeContactos() {
  if (contacto) return contacto;
  contacto = document.createElement("div");
  contacto.id = "contacto";
  contacto.className = "hidden";
  document.body.append(contacto);
  return contacto;
}

function muestraContacto(m, lataEl) {
  const c = hojaDeContactos();
  const ts = [0.08, 0.24, 0.42, 0.58, 0.74, 0.92].map((f) => f * m.dur);
  c.innerHTML = `
    <div class="c-grid">${ts.map((t) =>
      `<img loading="lazy" src="${thumbUrl(m.name, t)}" title="${t.toFixed(1)} s">`).join("")}</div>
    <div class="c-pie">
      <span class="c-nombre">${m.name.replace(/\.[^.]+$/, "")}</span>
      <span class="c-datos">${m.dur.toFixed(1)} s · ${m.w}×${m.h} · ${m.fps} fps</span>
    </div>
    <div class="c-nota">dos toques y a la bobina</div>`;
  const r = lataEl.getBoundingClientRect();
  c.classList.remove("hidden");
  const top = Math.max(10, Math.min(innerHeight - 270, r.top - 40));
  c.style.left = (r.right + 14) + "px";
  c.style.top = top + "px";
}

function escondeContacto() {
  if (contacto) contacto.classList.add("hidden");
}

function renderLatas() {
  const cont = document.getElementById("latas");
  cont.innerHTML = "";
  state.media.forEach((m, i) => {
    const esAudio = m.kind === "audio";
    const esFoto = m.kind === "image";
    const el = document.createElement("div");
    el.className = esAudio ? "lata lata-audio" : "lata";
    if (m.missing) el.classList.add("offline");
    el.dataset.media = m.name;
    el.style.setProperty("--rot", ((i % 5) - 2) * 1.6 + "deg");
    el.title = esAudio
      ? `${m.name}\n(dos toques: a la pista de sonido, en la aguja)`
      : esFoto
        ? `${m.name}\n(dos toques: a la bobina como foto fija, 4 s)`
        : `${m.name}\n(dos toques para abrirla en la bobina)` +
          (m.vfr ? "\n⚠️ cadencia variable (se conforma al revelar)" : "");
    const cinta = document.createElement("div");
    cinta.className = "cinta";
    cinta.textContent = m.name.replace(/\.[^.]+$/, "");
    const dur = document.createElement("div");
    dur.className = "dur";
    dur.textContent = esFoto ? "foto" : m.dur.toFixed(0) + " s" + (m.vfr ? " · vfr" : "");
    // la tira asoma por debajo de la tapa: fotogramas reales (o la onda, si es sonido)
    const asoma = document.createElement("div");
    asoma.className = "tira-asoma";
    if (esAudio) {
      const im = document.createElement("img");
      im.loading = "lazy";
      im.draggable = false;
      im.className = "onda";
      im.src = `/api/wave?f=${encodeURIComponent(m.name)}`;
      asoma.append(im);
    } else if (esFoto) {
      const im = document.createElement("img");
      im.loading = "lazy";
      im.draggable = false;
      im.src = thumbUrl(m.name, 0.2);
      asoma.append(im);
    } else {
      for (const f of [0.15, 0.5, 0.85]) {
        const im = document.createElement("img");
        im.loading = "lazy";
        im.draggable = false;
        im.src = thumbUrl(m.name, f * m.dur);
        asoma.append(im);
      }
    }
    el.append(asoma, cinta, dur);
    if (m.missing) {
      el.title = `${m.name}\n⚠️ el fichero original no está (¿disco desconectado?)\nla bobina lo recuerda: volverá al reconectar`;
      dur.textContent = "offline";
      // el botón de relink: buscar la carpeta donde vive ahora
      const busca = document.createElement("button");
      busca.className = "lata-busca";
      busca.textContent = "buscar…";
      busca.title = "buscar el material en otra carpeta (recursivo, re-enlaza a todos sus hermanos)";
      busca.addEventListener("click", async (e2) => {
        e2.stopPropagation();
        foley.press();
        try {
          const r = await (await fetch("/api/media/relink", {
            method: "POST", body: JSON.stringify({ name: m.name }),
          })).json();
          if (r.relinked?.length) { await reloadMedia(); foley.bell?.(); }
        } catch {}
      });
      el.append(busca);
    }
    let clickTimer = null;
    if (!esAudio && !esFoto && !m.missing) {
      el.addEventListener("mouseenter", () => { muestraContacto(m, el); foley.tick(); });
      el.addEventListener("mouseleave", escondeContacto);
      el.addEventListener("click", () => {
        document.querySelectorAll(".lata.abierta").forEach((x) => x.classList.remove("abierta"));
        el.classList.add("abierta");
        foley.press();
        // un toque (si no llega el segundo): la cinta al proyector, SIN bobina
        clearTimeout(clickTimer);
        clickTimer = setTimeout(() => { escondeContacto(); openSource(m.name); }, 240);
      });
    }
    el.addEventListener("dblclick", () => clearTimeout(clickTimer));
    // clic derecho: renombrar la cinta (el nombre lógico de la estantería)
    el.addEventListener("contextmenu", async (e) => {
      e.preventDefault();
      const v = prompt("nombre de la cinta", m.name.replace(/\.[^.]+$/, ""));
      if (v == null || !v.trim()) return;
      try {
        const r = await (await fetch("/api/media/rename", {
          method: "POST",
          body: JSON.stringify({ from: m.name, to: v.trim() }),
        })).json();
        if (r.name) {
          snapshot();
          for (const c of state.clips) if (c.media === m.name) c.media = r.name;
          for (const a of state.audio) if (a.media === m.name) a.media = r.name;
          for (const rr of state.bin) if (rr.media === m.name) rr.media = r.name;
          await save();
          await reloadMedia();
          foley.thunk();
        }
      } catch {}
    });
    // quitar de la estantería (la referencia; lo físico va a la papelera)
    const quita = document.createElement("button");
    quita.className = "lata-quitar";
    quita.textContent = "×";
    quita.title = "quitar de la estantería";
    quita.addEventListener("click", async (e) => {
      e.stopPropagation();
      foley.squeak();
      await fetch("/api/media/remove", { method: "POST", body: JSON.stringify({ name: m.name }) });
      await reloadMedia();
    });
    el.append(quita);
    el.addEventListener("dblclick", () => {
      foley.thunk();
      escondeContacto();
      if (m.missing) return;
      if (esAudio) {
        addAudioAt(m.name, state.t);
        return;
      }
      const clip = addClip(m.name);
      if (clip && state.clips.length === 1) showFrameAt(0);
    });
    cont.append(el);
  });
  if (!state.media.length) {
    cont.innerHTML = `<div class="nota-mano">la estantería está vacía —<br>
      pulsa <b>+ trae material</b><br>y elige tus cintas</div>`;
  }
}

function renderSaco() {
  const cont = document.getElementById("saco");
  cont.innerHTML = "";
  state.bin.forEach((r, i) => {
    const el = document.createElement("div");
    el.className = "recorte";
    el.style.setProperty("--rot", ((i % 5) - 2) * 2.2 + "deg");
    el.title = `${r.media} · ${r.in.toFixed(1)}–${r.out.toFixed(1)} s\n(clic para devolverlo a la bobina)`;
    const perf = document.createElement("div");
    perf.className = "perf";
    for (let k = 0; k < 5; k++) perf.append(document.createElement("i"));
    const info = document.createElement("div");
    info.className = "r-info";
    info.textContent = (r.out - r.in).toFixed(1) + " s";
    el.append(perf, info);
    el.addEventListener("click", () => {
      foley.thunk();
      rescueFromBin(i);
    });
    cont.append(el);
  });
  if (!state.bin.length) {
    cont.innerHTML = `<div class="nota-mano" style="font-size:11px">aquí cuelga<br>lo que cortas</div>`;
  }
}
