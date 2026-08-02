// revelado.js — la sala húmeda: cubetas, cuerda de secado, latas selladas.

import { state, totalDur, layout, projDims, projFps } from "./state.js";
import { on } from "./state.js";
import * as foley from "./foley.js";

const PASOS = ["revelador", "paro", "fijador", "lavado"];

/** si hay tramo marcado y la casilla activa: solo lo que cae dentro */
function clipsParaRevelar() {
  const r = state.range;
  const usar = document.getElementById("rev-range")?.checked && r && r.b > r.a;
  if (!usar) return state.clips;
  const out = [];
  let t = 0;
  for (const c of state.clips) {
    const d = c.gap ? Math.max(c.dur || 1, 0.2) : (c.out - c.in) / (c.speed || 1);
    const s0 = t, e0 = t + d;
    t = e0;
    const a = Math.max(s0, r.a), b = Math.min(e0, r.b);
    if (b - a < 0.02) continue;
    if (c.gap) {
      out.push({ ...c, dur: b - a });
    } else {
      const spd = c.speed || 1;
      out.push({ ...c, in: c.in + (a - s0) * spd, out: c.in + (b - s0) * spd });
    }
  }
  return out;
}

function audioParaRevelar() {
  const r = state.range;
  const usar = document.getElementById("rev-range")?.checked && r && r.b > r.a;
  if (!usar) return state.audio;
  const out = [];
  for (const a of state.audio) {
    const d = Math.max(a.out - a.in, 0.01);
    const s0 = a.start, e0 = a.start + d;
    const x0 = Math.max(s0, r.a), x1 = Math.min(e0, r.b);
    if (x1 - x0 < 0.02) continue;
    out.push({
      ...a,
      start: x0 - r.a,
      in: a.in + (x0 - s0),
      out: a.in + (x1 - s0),
    });
  }
  return out;
}

const MASTERS = {
  "hevc-alta": { codec: "hevc", bitrate: 60000000 },
  "hevc-media": { codec: "hevc", bitrate: 30000000 },
  "prores422hq": { codec: "prores422hq" },
  "prores4444": { codec: "prores4444" },
};

export function initRevelado() {
  const btn = document.getElementById("btn-render");
  const btnCancel = document.getElementById("btn-cancel");
  const stepEl = document.getElementById("rev-step");
  const logEl = document.getElementById("render-log");
  const codecSel = document.getElementById("rev-codec");

  // los últimos ajustes de revelado se recuerdan
  try {
    const prev = JSON.parse(localStorage.getItem("fl-export") || "{}");
    if (prev.codec && MASTERS[prev.codec]) codecSel.value = prev.codec;
    if (prev.fh != null) document.getElementById("rev-fh").value = prev.fh;
    if (prev.ft != null) document.getElementById("rev-ft").value = prev.ft;
  } catch {}

  // el motor que revela, a la vista (y de qué clase es)
  (async () => {
    try {
      const e = await (await fetch("/api/engine")).json();
      const el = document.querySelector(".colofon ul");
      if (el && e.name) {
        const li = document.createElement("li");
        li.textContent = `motor de revelado: ${e.name} · zero-copy nativo` +
          (e.exists ? "" : " · ⚠️ NO ENCONTRADO");
        el.append(li);
      }
    } catch {}
  })();

  btnCancel.addEventListener("click", async () => {
    await fetch("/api/render/cancel", { method: "POST" });
    stepEl.textContent = "parando…";
  });

  document.querySelector('[data-room="rev"]').addEventListener("click", () => {
    refreshResumen();
    const lr = document.getElementById("lbl-range");
    if (lr) lr.classList.toggle("hidden", !(state.range && state.range.b > state.range.a));
  });
  refreshResumen();
  renderReveladas();

  btn.addEventListener("click", async () => {
    if (!state.clips.length) {
      stepEl.textContent = "la bobina está vacía — no hay nada que revelar";
      return;
    }
    btn.disabled = true;
    logEl.classList.remove("hidden");
    logEl.textContent = "";
    clearTendedero();
    localStorage.setItem("fl-export", JSON.stringify({
      codec: codecSel.value,
      fh: document.getElementById("rev-fh").value,
      ft: document.getElementById("rev-ft").value,
    }));
    const pd = projDims();
    const payload = {
      master: {
        ...(MASTERS[codecSel.value] || MASTERS["hevc-alta"]),
        loudnorm: document.getElementById("rev-loud")?.checked || false,
      },
      out_name: document.getElementById("out-name").value || "bobina",
      project: {
        w: pd.w, h: pd.h, fps: projFps(),
        fadeHead: +document.getElementById("rev-fh")?.value || 0,
        fadeTail: +document.getElementById("rev-ft")?.value || 0,
      },
      prefs: state.prefs,
      lut_in: state.lutEntrada,
      lut: state.lutColor,
      clips: clipsParaRevelar().map((c) => c.gap ? {
        gap: true, in: 0, out: Math.max(c.dur || 1, 0.2),
        ...(c.fade > 0.01 ? { fade: c.fade } : {}),
        ...(c.fadeType ? { fadeType: c.fadeType } : {}),
      } : {
        file: c.media, in: c.in, out: c.out,
        ...(c.speed && c.speed !== 1 ? { speed: c.speed } : {}),
        ...(c.fade > 0.01 ? { fade: c.fade } : {}),
        ...(c.fadeType ? { fadeType: c.fadeType } : {}),
        ...(c.gain ? { gain: c.gain } : {}),
        ...(c.mute ? { mute: true } : {}),
        ...(c.fadeIn > 0.005 ? { fadeIn: c.fadeIn } : {}),
        ...(c.fadeOut > 0.005 ? { fadeOut: c.fadeOut } : {}),
        ...(c.tf ? { tf: c.tf } : {}),
      }),
      audio: audioParaRevelar().map((a) => ({
        file: a.media, in: a.in, out: a.out, start: a.start,
        ...(a.env && a.env.length >= 2 ? { env: a.env } : {}),
        ...(a.gain ? { gain: a.gain } : {}),
        ...(a.mute ? { mute: true } : {}),
        ...(a.fadeIn > 0.005 ? { fadeIn: a.fadeIn } : {}),
        ...(a.fadeOut > 0.005 ? { fadeOut: a.fadeOut } : {}),
      })),
    };
    const r = await fetch("/api/render", { method: "POST", body: JSON.stringify(payload) });
    if (!r.ok) { stepEl.textContent = "el laboratorio está ocupado"; btn.disabled = false; return; }
    foley.pour();
    foley.bubbleStart();
    btnCancel.classList.remove("hidden");
    poll();
  });

  let colgados = 0;
  async function poll() {
    const s = await (await fetch("/api/render/status")).json();
    // qué cubeta burbujea (por el avance)
    const idx = Math.min(PASOS.length - 1, Math.floor(s.pct * PASOS.length));
    document.querySelectorAll(".cubeta").forEach((c, i) => {
      c.classList.toggle("activa", s.state === "running" && i === idx);
      if (!c.querySelector(".tira-dentro")) {
        const t = document.createElement("div");
        t.className = "tira-dentro";
        c.prepend(t);
      }
    });
    // ETA suavizada a partir del avance real
    let eta = "";
    if (s.state === "running" && s.started > 0 && s.pct > 0.04) {
      const el2 = Date.now() / 1000 - s.started;
      const rest = (el2 * (1 - s.pct)) / s.pct;
      if (rest > 1) eta = ` · quedan ~${rest > 90 ? Math.round(rest / 60) + " min" : Math.round(rest) + " s"}`;
    }
    stepEl.textContent = (s.step || "") + eta;
    logEl.textContent = s.log || "";
    logEl.scrollTop = logEl.scrollHeight;

    // cuelga cada clip terminado a secar
    const items = layout();
    const doneClips = Math.floor(s.pct * items.length);
    while (colgados < doneClips && colgados < items.length) {
      cuelga(items[colgados].clip);
      colgados++;
    }

    if (s.state === "running") return setTimeout(poll, 700);
    document.querySelectorAll(".cubeta").forEach((c) => c.classList.remove("activa"));
    foley.bubbleStop();
    btn.disabled = false;
    document.getElementById("btn-cancel").classList.add("hidden");
    colgados = 0;
    if (s.state === "done") {
      foley.bell();
      stepEl.textContent = "revelada y sellada en su lata";
      renderReveladas();
    } else if (s.state === "error") {
      stepEl.textContent = "se veló la película: " + (s.step || "error");
    }
  }
}

function refreshResumen() {
  const el = document.getElementById("rev-resumen");
  if (el) el.textContent =
    `${state.clips.length} empalme(s) · ${totalDur().toFixed(1)} s · ` +
    `gelatinas: ${(state.lutEntrada || "ninguna").replace(/\.cube$/, "")} + ${(state.lutColor || "ninguna").replace(/\.cube$/, "")}`;
}

function clearTendedero() {
  document.getElementById("tendedero").innerHTML = "";
}

function cuelga(clip) {
  const t = document.createElement("div");
  t.className = "colgado";
  t.innerHTML = `<div class="pinza"></div>
    <img draggable="false" src="/api/thumb?f=${encodeURIComponent(clip.media)}&t=${Math.max(0.2, clip.in + 0.5).toFixed(1)}">`;
  document.getElementById("tendedero").append(t);
  foley.tick();
}

async function renderReveladas() {
  const cont = document.getElementById("reveladas");
  let items = [];
  try { items = await (await fetch("/api/renders")).json(); } catch {}
  cont.innerHTML = "";
  items.slice(0, 12).forEach((r, i) => {
    const el = document.createElement("a");
    el.className = "lata-rev";
    el.href = r.url;
    el.target = "_blank";
    el.style.setProperty("--rot", ((i % 5) - 2) * 2.4 + "deg");
    const d = new Date(r.mtime * 1000);
    el.title = `${r.name} · ${(r.bytes / 1e6).toFixed(0)} MB · ${d.toLocaleString("es-ES")}`;
    el.innerHTML = `<div class="cinta">${r.name.replace(/\.[^.]+$/, "")}</div>
      <div class="sello">REVELADA</div>
      <svg class="sello-circular" viewBox="0 0 120 120">
        <use href="#d-stamp"/>
        <text><textPath href="#stamp-arc" startOffset="4">LABORATORIOS·SAORIN·</textPath></text>
      </svg>`;
    cont.append(el);
  });
  if (!items.length) {
    cont.innerHTML = `<div class="nota-mano">aún no hay bobinas —<br>la primera está al caer</div>`;
  }
}
