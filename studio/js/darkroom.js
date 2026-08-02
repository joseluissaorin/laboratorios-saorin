// darkroom.js — el cuarto oscuro: los baños (presets), los stocks, las
// gelatinas (LUTs) en la ampliadora, y el panel de instrumentos de aguja.

import { state, on, emit, touch, setPref, DEFAULT_PREFS } from "./state.js";
import { PRESETS, STOCKS } from "./presets.js";
import { listLuts, applyGel, gelShortName } from "./luts.js";
import * as foley from "./foley.js";

/* ── los baños: botellas de químicos ── */
function renderBotellas() {
  const cont = document.getElementById("botellas");
  cont.innerHTML = "";
  PRESETS.forEach(([name, values], i) => {
    const b = document.createElement("div");
    b.className = "botella";
    b.style.setProperty("--rot", ((i % 3) - 1) * 2.0 + "deg");
    b.title = "verter el baño «" + name + "»";
    b.innerHTML = `<div class="cuello"></div><div class="tapon"></div>
      <div class="vidrio"></div><div class="etiq">${name}</div>`;
    if (name.startsWith("saorín")) b.classList.add("activa");
    b.addEventListener("click", () => {
      Object.assign(state.prefs, values);
      // las gelatinas no se tocan al verter un baño
      state.prefs.inputLutOn = !!state.lutEntrada;
      state.prefs.lutOn = !!state.lutColor;
      cont.querySelectorAll(".botella").forEach((x) => x.classList.remove("activa"));
      b.classList.add("activa");
      document.querySelectorAll("#stock-row .chip").forEach((x) => x.classList.remove("on"));
      foley.pour();
      touch(); emit("prefs");
    });
    cont.append(b);
  });
}

function renderStocks() {
  const row = document.getElementById("stock-row");
  row.innerHTML = "";
  STOCKS.forEach(([name, overlay], i) => {
    const b = document.createElement("button");
    b.className = "chip";
    b.textContent = name;
    b.style.setProperty("--rot", ((i % 3) - 1) * -0.8 + "deg");
    b.addEventListener("click", () => {
      const wasOn = b.classList.contains("on");
      row.querySelectorAll(".chip").forEach((x) => x.classList.remove("on"));
      if (!wasOn) { Object.assign(state.prefs, overlay); b.classList.add("on"); }
      foley.press();
      touch(); emit("prefs");
    });
    row.append(b);
  });
}

/* ── las gelatinas (LUTs) ── */
async function renderGels() {
  const luts = await listLuts();
  const slotE = document.querySelector("#gel-entrada span");
  const slotC = document.querySelector("#gel-color span");
  const refresh = () => {
    slotE.textContent = gelShortName(state.lutEntrada);
    slotC.textContent = gelShortName(state.lutColor);
  };
  refresh();
  on("luts", refresh);

  const mkFila = (contId, slot, names) => {
    const cont = document.getElementById(contId);
    cont.innerHTML = "";
    const todos = [...names, null];   // null = sin gelatina
    todos.forEach((name, i) => {
      const f = document.createElement("div");
      f.className = "gel-ficha" + (name === null ? " ninguna" : "");
      f.style.setProperty("--rot", ((i % 3) - 1) * 1.2 + "deg");
      f.innerHTML = `<span>${gelShortName(name)}</span>`;
      const cur = slot === "entrada" ? state.lutEntrada : state.lutColor;
      if (name === cur) f.classList.add("puesta");
      f.addEventListener("click", async () => {
        foley.press();
        await applyGel(slot, name);
        cont.querySelectorAll(".gel-ficha").forEach((x) => x.classList.remove("puesta"));
        f.classList.add("puesta");
      });
      cont.append(f);
    });
  };
  mkFila("gels-entrada", "entrada", luts.entrada || []);
  mkFila("gels-color", "color", luts.color || []);

  const cajon = document.getElementById("cajon");
  const abre = () => { cajon.classList.remove("cerrado"); foley.grab(); };
  const cierra = () => { cajon.classList.add("cerrado"); foley.release(); };
  document.getElementById("abre-cajon").addEventListener("click", abre);
  document.getElementById("cierra-cajon").addEventListener("click", cierra);
  document.getElementById("gel-entrada").addEventListener("click", abre);
  document.getElementById("gel-color").addEventListener("click", abre);
}

/* ── el panel de instrumentos: agujas de galvanómetro ── */
const GROUPS = [
  ["el revelado", [
    ["gain", "exposición", -2, 2, 0.01],
    ["pushPull", "push / pull", -2, 2, 0.01],
    ["compImpact", "compresión", 0, 3, 0.01],
    ["compRange", "rango", 0, 1, 0.01],
    ["shutter", "obturador", 0, 0.9, 0.005],
  ]],
  ["el color del stock", [
    ["hueSkew", "deriva de tono", 0, 1.5, 0.01],
    ["crosstalk", "contagio de capas", 0, 1.5, 0.01],
    ["subtractive", "color subtractivo", 0, 1, 0.01],
    ["stockSat", "saturación", 0, 2, 0.01],
    ["print", "positivado 2383", 0, 1, 0.01],
  ]],
  ["la halación", [
    ["halation", "halación", 0, 3, 0.01],
    ["halThr", "umbral", 0, 1, 0.01],
    ["halSpread", "extensión", 0, 1, 0.01],
    ["halHue", "tono", 0, 2, 0.01],
    ["halWhite", "blanqueo", 0, 1, 0.01],
    ["bloom", "velo (bloom)", 0, 2, 0.01],
    ["bloomThr", "umbral del velo", 0, 1, 0.01],
    ["bloomWarm", "calidez del velo", 0, 1, 0.01],
  ]],
  ["el grano", [
    ["grain", "cantidad", 0, 1, 0.005],
    ["grainSize", "tamaño", 0.4, 8, 0.05],
    ["grainRough", "aspereza", 0, 1, 0.01],
    ["grainChroma", "croma", 0, 1, 0.01],
    ["grainDefocus", "desenfoque", 0, 1, 0.01],
    ["grainShadows", "en sombras", 0, 2, 0.01],
    ["grainMids", "en medios", 0, 2, 0.01],
    ["grainHighs", "en altas", 0, 2, 0.01],
    ["filmRes", "resolución", 0, 1, 0.01],
  ]],
  ["la óptica", [
    ["softness", "suavidad", 0, 1, 0.01],
    ["acutance", "acutancia", 0, 1, 0.01],
    ["chroma", "aberración", 0, 1, 0.01],
    ["vignette", "viñeta", 0, 1, 0.01],
    ["vigSize", "tamaño viñeta", 0.1, 1, 0.01],
  ]],
  ["la mecánica", [
    ["weave", "gate weave", 0, 1, 0.01],
    ["flicker", "parpadeo", 0, 1, 0.01],
    ["breath", "respiración", 0, 1, 0.01],
    ["dust", "polvo y rayas", 0, 1, 0.01],
    ["frameInset", "ventanilla", 0, 120, 1],
  ]],
];

const gauges = [];   // {canvas, key, min, max, needle:{x,v}}

function drawGauge(g) {
  const c = g.canvas, x = c.getContext("2d");
  const W = c.width, H = c.height;
  x.clearRect(0, 0, W, H);
  // arco de esfera
  const cx = W / 2, cy = H * 1.55, r = H * 1.28;
  x.strokeStyle = "#6e2c1c"; x.lineWidth = 1.5;
  x.beginPath(); x.arc(cx, cy, r, Math.PI * 1.28, Math.PI * 1.72); x.stroke();
  // ticks
  for (let i = 0; i <= 8; i++) {
    const a = Math.PI * (1.28 + 0.44 * i / 8);
    const r0 = r - (i % 4 === 0 ? 7 : 4);
    x.beginPath();
    x.moveTo(cx + Math.cos(a) * r0, cy + Math.sin(a) * r0);
    x.lineTo(cx + Math.cos(a) * (r + 1), cy + Math.sin(a) * (r + 1));
    x.stroke();
  }
  // aguja
  const frac = (g.needle.x - g.min) / (g.max - g.min);
  const a = Math.PI * (1.28 + 0.44 * Math.max(0, Math.min(1, frac)));
  x.strokeStyle = "#ff5a3c"; x.lineWidth = 2;
  x.beginPath();
  x.moveTo(cx, cy - 2);
  x.lineTo(cx + Math.cos(a) * (r + 3), cy + Math.sin(a) * (r + 3));
  x.stroke();
}

function gaugeLoop() {
  requestAnimationFrame(gaugeLoop);
  for (const g of gauges) {
    const target = state.prefs[g.key] ?? 0;
    const n = g.needle;
    const dt = 0.016;
    n.v += (target - n.x) * 260 * dt - n.v * 16 * dt;   // sub-amortiguado: tiembla
    n.x += n.v * dt;
    if (Math.abs(n.x - target) > 1e-5 || Math.abs(n.v) > 1e-4) drawGauge(g);
  }
}

function renderInstrumentos() {
  const root = document.getElementById("controls");
  root.innerHTML = "";
  for (const [title, rows] of GROUPS) {
    const det = document.createElement("details");
    det.className = "inst-group";
    det.open = title === "el revelado" || title === "la halación";
    const sum = document.createElement("summary");
    sum.textContent = title;
    det.append(sum);
    for (const [key, label, min, max, step] of rows) {
      const row = document.createElement("div");
      row.className = "inst";
      row.title = "arrastra a los lados · dos toques = de fábrica";
      const lab = document.createElement("label");
      lab.textContent = label;
      const cnv = document.createElement("canvas");
      cnv.width = 120; cnv.height = 26;
      const val = document.createElement("span");
      val.className = "val";
      val.textContent = (+(state.prefs[key] ?? 0)).toFixed(2);
      row.append(lab, cnv, val);
      det.append(row);
      const g = { canvas: cnv, key, min, max, needle: { x: state.prefs[key] ?? 0, v: 0 } };
      gauges.push(g);
      drawGauge(g);

      // arrastre horizontal en toda la fila = girar el mando
      let dragX = null;
      row.addEventListener("pointerdown", (e) => {
        row.setPointerCapture(e.pointerId);
        dragX = e.clientX;
        foley.grab();
      });
      row.addEventListener("pointermove", (e) => {
        if (dragX === null) return;
        const dx = e.clientX - dragX;
        dragX = e.clientX;
        const range = max - min;
        let v = (state.prefs[key] ?? 0) + dx * range / 220;
        v = Math.max(min, Math.min(max, Math.round(v / step) * step));
        if (v !== state.prefs[key]) {
          setPref(key, v);
          val.textContent = v.toFixed(2);
          foley.tick();
        }
      });
      const done = () => { if (dragX !== null) { dragX = null; foley.release(); } };
      row.addEventListener("pointerup", done);
      row.addEventListener("pointercancel", done);
      row.addEventListener("dblclick", () => {
        const v = DEFAULT_PREFS[key] ?? 0;
        setPref(key, v);
        val.textContent = (+v).toFixed(2);
        foley.press();
      });
    }
    root.append(det);
  }
  on("prefs", () => {
    root.querySelectorAll(".inst").forEach((row, i) => {
      const g = gauges[i];
      if (g) row.querySelector(".val").textContent = (+(state.prefs[g.key] ?? 0)).toFixed(2);
    });
  });
}

export function initDarkroom() {
  renderBotellas();
  renderStocks();
  renderGels();
  renderInstrumentos();
  gaugeLoop();
}
