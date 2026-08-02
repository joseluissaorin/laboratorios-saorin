// ui.js — LUT (baked .cube) + film emulation sobre <video>. Sin demuxers.

import { Pipeline } from "./pipeline.js";

// Dos partes: LUT (grade horneado) + FILM EMULATION física en shader.
// Referencias: IPOL 192 (grain silver-halide), cos⁴ vignetting, halation por
// reflexión en la base (cálida), bloom = veiling glare, shutter = integración
// temporal IIR (≈ obturador de 360°+).
const FILM_DEFAULT = {
  gain: 0, pushPull: 0, compImpact: 0, compWP: 1.0, compRange: 0.5,
  shutter: 0.35, shutterAuto: true, shutterSens: 1.0,
  grain: 0.28, grainSize: 2.6, grainRough: 0.35, grainChroma: 0.25, grainDefocus: 0.55,
  grainShadows: 0.8, grainMids: 1.0, grainHighs: 0.5, grainRed: 1.0, grainBlue: 1.25, filmRes: 0.5,
  halation: 0.8, halHue: 1.0, halSat: 0.9, halThr: 0.5, halSpread: 0.7, halWhite: 0.0,
  bloom: 0.4, bloomThr: 0.72, bloomWarm: 0.15,
  softness: 0.25, acutance: 0, colorSep: 0,
  hueSkew: 1.0, crosstalk: 0.3, subtractive: 0.6, stockSat: 1.0, print: 0.5,
  vignette: 0.45, vigSize: 0.55, vigRound: 1.0, vigCX: 0.5, vigCY: 0.5,
  chroma: 0, weave: 0, weaveRot: 0.3, flicker: 0, breath: 0, breathRate: 0.5, dust: 0,
  frameInset: 0, frameCorner: 40, frameWobble: 0.5,
};
// La Chimera (Rohrwacher/Louvart): S16 500T vida diaria — grano 16mm presente,
// halation débil (rem-jet), negros no machacados, sin difusión de lente.
const PRESET_CHIMERA_S16 = { ...FILM_DEFAULT,
  grain: 0.45, grainSize: 3.4, grainRough: 0.5, grainChroma: 0.35, grainDefocus: 0.3,
  grainShadows: 0.7, grainMids: 1.0, grainHighs: 0.6, grainBlue: 1.3, filmRes: 0.7,
  halation: 0.25, halHue: 1.0, halSat: 0.9, halThr: 0.8, halSpread: 0.6, halWhite: 0.1,
  bloom: 0.2, bloomThr: 0.8, bloomWarm: 0.3,
  softness: 0.1, vignette: 0.2, weave: 0.15, chroma: 0,
};
// La Chimera · sueños/recuerdos: Bolex H16 de cuerda — grano grueso, jitter,
// flicker de exposición, más suave.
const PRESET_CHIMERA_BOLEX = { ...FILM_DEFAULT,
  grain: 0.6, grainSize: 4.5, grainRough: 0.55, grainChroma: 0.35, grainDefocus: 0.4,
  grainShadows: 0.8, grainMids: 1.1, grainHighs: 0.6, grainBlue: 1.3, filmRes: 0.9,
  halation: 0.3, halThr: 0.75, halSpread: 0.6, halWhite: 0.1,
  bloom: 0.25, bloomWarm: 0.35,
  softness: 0.4, vignette: 0.35, weave: 0.5, flicker: 0.3,
};
// CineStill 800T: sin rem-jet → halation aura grande, tungsteno.
const PRESET_CINESTILL = { ...FILM_DEFAULT,
  grain: 0.35, grainSize: 3.0, grainRough: 0.4, filmRes: 0.5,
  halation: 1.2, halHue: 1.0, halSat: 1.0, halThr: 0.4, halSpread: 1.0, halWhite: 0.0,
  bloom: 0.5, bloomThr: 0.65, bloomWarm: 0.4,
  softness: 0.2, vignette: 0.3,
};
// Stocks: la física de color cambia por emulsión (ver ensayo técnico).
const STOCK_50D = { hueSkew: 1.2, crosstalk: 0.35, subtractive: 0.8, stockSat: 1.2, print: 0.7, compImpact: 0.3 };
const STOCK_250D = { hueSkew: 1.0, crosstalk: 0.3, subtractive: 0.7, stockSat: 1.0, print: 0.6, compImpact: 0.15 };
const STOCK_500T = { hueSkew: 1.1, crosstalk: 0.35, subtractive: 0.65, stockSat: 0.95, print: 0.6, compImpact: 0.2, halation: 0.6 };
const STOCK_FUJI = { hueSkew: 0.8, crosstalk: 0.25, subtractive: 0.5, stockSat: 0.85, print: 0.4, compImpact: 0.4 };
const STOCK_C800 = { hueSkew: 1.0, crosstalk: 0.3, subtractive: 0.65, stockSat: 0.95, print: 0.6, halation: 1.2, halSpread: 1.0, halThr: 0.4 };

const FILM_OFF = { ...FILM_DEFAULT, grain: 0, halation: 0, bloom: 0, softness: 0, vignette: 0, weave: 0, flicker: 0, dust: 0, chroma: 0, shutter: 0, shutterAuto: false };

const P = { ...FILM_DEFAULT, lutOn: true, wipe: 1.0, resetHistory: true };

const SLIDERS = [
  ["EXPOSURE + LAB", [
    ["gain", "gain (stops)", -3, 3, 0.05],
    ["pushPull", "push/pull (stops)", -2, 2, 0.05],
    ["compImpact", "film compression", 0, 3, 0.05],
    ["compRange", "compression range", 0.05, 1, 0.01],
    ["colorSep", "color separation", 0, 2, 0.01],
  ]],
  ["SHUTTER", [
    ["shutterAuto", "auto (detecta movimiento)", "bool"],
    ["shutter", "motion blur manual", 0, 0.92, 0.01],
    ["shutterSens", "sensibilidad auto", 0, 2.5, 0.05],
  ]],
  ["GRAIN", [
    ["grain", "amount", 0, 1, 0.01],
    ["grainSize", "size (px)", 0.5, 8, 0.05],
    ["grainRough", "roughness", 0, 1, 0.01],
    ["grainChroma", "chroma", 0, 1, 0.01],
    ["grainDefocus", "defocus", 0, 1, 0.01],
    ["grainShadows", "en sombras", 0, 1.5, 0.01],
    ["grainMids", "en medios", 0, 1.5, 0.01],
    ["grainHighs", "en altas", 0, 1.5, 0.01],
    ["grainRed", "capa roja", 0, 2, 0.01],
    ["grainBlue", "capa azul", 0, 2, 0.01],
    ["filmRes", "film resolution", 0, 1, 0.01],
  ]],
  ["FILM COLOR (física de emulsión)", [
    ["hueSkew", "hue skews (matiz×exposición)", 0, 2, 0.01],
    ["crosstalk", "crosstalk entre capas", 0, 1, 0.01],
    ["subtractive", "saturación sustractiva", 0, 1, 0.01],
    ["stockSat", "saturación del stock", 0.5, 1.5, 0.01],
    ["print", "print 2383 (S-curve+cast)", 0, 1, 0.01],
  ]],
  ["HALATION", [
    ["halation", "amount", 0, 1.5, 0.01],
    ["halHue", "hue", 0, 1, 0.01],
    ["halSat", "saturation", 0, 1, 0.01],
    ["halWhite", "whiten", 0, 1, 0.01],
    ["halThr", "threshold", 0, 1, 0.01],
    ["halSpread", "spread (radio)", 0, 1, 0.01],
  ]],
  ["BLOOM", [
    ["bloom", "amount", 0, 1.5, 0.01],
    ["bloomThr", "threshold", 0, 1, 0.01],
    ["bloomWarm", "tinte cálido", 0, 1, 0.01],
  ]],
  ["DIFFUSION", [
    ["softness", "softness", 0, 1, 0.01],
    ["acutance", "acutance (edge halo)", 0, 2, 0.01],
  ]],
  ["ÓPTICA · VIGNETTE + CA", [
    ["vignette", "vignette", 0, 1, 0.01],
    ["vigSize", "size", 0.1, 2, 0.01],
    ["vigRound", "roundness", 0, 1, 0.01],
    ["vigCX", "centro X", 0, 1, 0.01],
    ["vigCY", "centro Y", 0, 1, 0.01],
    ["chroma", "aberración cromática", 0, 10, 0.05],
  ]],
  ["PROYECCIÓN", [
    ["weave", "gate weave", 0, 1, 0.01],
    ["weaveRot", "weave rotación", 0, 1, 0.01],
    ["flicker", "flicker (rápido)", 0, 1, 0.01],
    ["breath", "film breath (lento)", 0, 1, 0.01],
    ["breathRate", "breath rate", 0, 1, 0.01],
    ["dust", "dust & scratches", 0, 1, 0.01],
  ]],
  ["FRAME / FILM GATE", [
    ["frameInset", "marco (px)", 0, 120, 1],
    ["frameCorner", "esquinas", 0, 100, 1],
    ["frameWobble", "borde imperfecto", 0, 1, 0.01],
  ]],
];

const $ = (s) => document.querySelector(s);
const status = (t) => {
  $("#status").textContent = t;
  fetch("/log", { method: "POST", body: t }).catch(() => {});
};
window.addEventListener("error", e => status("ERR " + e.message));
window.addEventListener("unhandledrejection", e => status("REJ " + (e.reason?.message || e.reason)));

function buildUI(onChange){
  const panel = $("#controls");
  const preset = document.createElement("div");
  preset.className = "row";
  preset.innerHTML = `
    <button id="pD1">default</button>
    <button id="pS16">La Chimera · S16</button>
    <button id="pBolex">La Chimera · Bolex</button>
    <button id="pC800">CineStill 800T</button>
    <button id="pOff">FX off</button>
    <button id="pLut">cargar LUT / hald…</button>
    <input type="file" id="lutFile" accept=".cube,.tif,.tiff,.dpx,.png" style="display:none">
    <button id="pVid">elegir vídeo…</button>
    <input type="file" id="vidFile" accept="video/*,.mp4,.mov,.m4v" style="display:none">
    <button id="pExp">exportar prefs</button>
    <button id="pImp">importar prefs</button>
    <input type="file" id="prefsFile" accept=".json" style="display:none">`;
  panel.appendChild(preset);
  const lutrow = document.createElement("div");
  lutrow.className = "row";
  lutrow.innerHTML = `
    <label class="chk"><input type="checkbox" id="lutOn" checked> LUT (grade horneado)</label>
    <label class="chk wipe">wipe <input type="range" id="wipe" min="0" max="1" step="0.01" value="1"></label>
    <span id="lutName" class="note"></span>`;
  panel.appendChild(lutrow);
  $("#pD1").onclick = () => { Object.assign(P, FILM_DEFAULT); sync(); onChange(); };
  $("#pS16").onclick = () => { Object.assign(P, PRESET_CHIMERA_S16); sync(); onChange(); };
  $("#pBolex").onclick = () => { Object.assign(P, PRESET_CHIMERA_BOLEX); sync(); onChange(); };
  $("#pC800").onclick = () => { Object.assign(P, PRESET_CINESTILL); sync(); onChange(); };
  $("#pOff").onclick = () => { Object.assign(P, FILM_OFF); sync(); onChange(); };
  // stocks (solo tocan la etapa de color; conservan texturas)
  const stocks = [["50D", STOCK_50D], ["250D", STOCK_250D], ["500T", STOCK_500T], ["Fuji Eterna", STOCK_FUJI], ["CineStill 800T", STOCK_C800]];
  const srow = document.createElement("div");
  srow.className = "row";
  srow.innerHTML = `<span class="note">stock:</span>` + stocks.map((s, i) => `<button data-stock="${i}">${s[0]}</button>`).join("");
  panel.appendChild(srow);
  srow.querySelectorAll("[data-stock]").forEach(b => b.onclick = () => {
    Object.assign(P, stocks[+b.dataset.stock][1]); sync(); onChange();
  });
  $("#pExp").onclick = () => {
    const a = document.createElement("a");
    a.href = URL.createObjectURL(new Blob([JSON.stringify(P, null, 1)], { type: "application/json" }));
    a.download = "filmlook-prefs.json";
    a.click();
  };
  $("#pImp").onclick = () => $("#prefsFile").click();
  $("#prefsFile").onchange = async (e) => {
    const f = e.target.files[0];
    if (!f) return;
    try {
      Object.assign(P, JSON.parse(await f.text()));
      sync(); onChange();
      status("prefs importadas: " + f.name);
    } catch (err) { status("prefs inválidas: " + err.message); }
  };
  $("#pVid").onclick = () => $("#vidFile").click();
  $("#pLut").onclick = () => $("#lutFile").click();
  $("#lutFile").onchange = async (e) => {
    const f = e.target.files[0];
    if (!f) return;
    status("convirtiendo " + f.name + "…");
    const r = await fetch("/upload_lut", { method: "POST", headers: { "X-Filename": f.name }, body: await f.arrayBuffer() });
    if (!r.ok){ status("LUT falló: " + await r.text()); return; }
    const meta = await (await fetch("assets/lut_custom.json?x=" + Date.now())).json();
    const buf = new Float32Array(await (await fetch("assets/lut_custom.bin?x=" + Date.now())).arrayBuffer());
    window.__pipe.setLut(buf, meta.size);
    $("#lutName").textContent = "· " + f.name;
    P.lutOn = true; $("#lutOn").checked = true;
    status("LUT cargada: " + f.name);
    onChange();
  };
  $("#lutOn").onchange = (e) => { P.lutOn = e.target.checked; onChange(); };
  $("#wipe").oninput = (e) => { P.wipe = +e.target.value; onChange(); };

  for (const [section, items] of SLIDERS){
    const h = document.createElement("h3"); h.textContent = section;
    panel.appendChild(h);
    for (const [key, label, a, b, step] of items){
      const row = document.createElement("div");
      row.className = "slider";
      if (a === "bool"){
        row.innerHTML = `<label class="chk">${label}<input type="checkbox" data-k="${key}"></label>`;
        row.querySelector("input").onchange = (e) => { P[key] = e.target.checked; onChange(); };
      } else {
        row.innerHTML = `<span>${label}</span><input type="range" data-k="${key}" min="${a}" max="${b}" step="${step}"><em></em>`;
        row.querySelector("input").oninput = (e) => {
          P[key] = +e.target.value;
          row.querySelector("em").textContent = (+e.target.value).toFixed(2);
          onChange();
        };
      }
      panel.appendChild(row);
    }
  }
  const note = document.createElement("p");
  note.className = "note";
  note.textContent = "LUT horneada del .drx (solo color) + emulación de película en shader. «look del .drx» carga los valores con los que se horneó.";
  panel.appendChild(note);
}

function sync(){
  document.querySelectorAll("[data-k]").forEach((el) => {
    const k = el.dataset.k;
    if (el.type === "checkbox") el.checked = !!P[k];
    else { el.value = P[k]; el.parentElement.querySelector("em").textContent = (+P[k]).toFixed(2); }
  });
}

async function main(){
  // ── MODO RENDER (headless CLI): ?render=1 → lee _job.json, renderiza
  // frame a frame de forma determinista y POSTea cada PNG al servidor.
  if (new URLSearchParams(location.search).get("render")) return renderMode();

  buildUI(() => { dirty = true; });
  sync();
  status("cargando LUT…");

  let lutMeta, lutBuf;
  try {   // preferida: la horneada por el usuario; fallback: la nuestra
    lutMeta = await (await fetch("assets/lut_user.json")).json();
    lutBuf = new Float32Array(await (await fetch("assets/lut_user.bin")).arrayBuffer());
    $("#lutName").textContent = "· bake Resolve (tiff)";
  } catch {
    lutMeta = await (await fetch("assets/lut.json")).json();
    lutBuf = new Float32Array(await (await fetch("assets/lut.bin")).arrayBuffer());
    $("#lutName").textContent = "· bake auto";
  }
  const canvas = $("#view");
  const pipe = new Pipeline(canvas, lutBuf, lutMeta.size);
  window.__pipe = pipe;
  // grain plate (ruido FFT tileable)
  const gMeta = await (await fetch("assets/grain.json")).json();
  const gBuf = new Uint16Array(await (await fetch("assets/grain.bin")).arrayBuffer());
  pipe.setGrainPlate(gBuf, gMeta.size);
  status("LUT ok · cargando vídeo…");

  // vídeo: original 4K HEVC si el navegador lo traga; si no, proxy 1080p
  const video = document.createElement("video");
  video.muted = true; video.loop = true; video.playsInline = true;
  // SIN crossOrigin: los vídeos son same-origin (servidor) o blob: — ponerlo
  // contamina la textura WebGL y el canvas queda en negro sin error.
  const srcUrl = await new Promise((res) => {
    const probe = document.createElement("video");
    probe.muted = true;
    probe.onerror = () => res("assets/proxy.mp4");
    probe.onloadeddata = () => res("assets/source.mp4");
    probe.src = "assets/source.mp4";
    setTimeout(() => res("assets/proxy.mp4"), 8000);
  });
  video.src = srcUrl;
  await new Promise((res, rej) => {
    video.onloadeddata = res;
    video.onerror = () => rej(new Error("no se pudo cargar " + srcUrl));
  });
  canvas.width = video.videoWidth; canvas.height = video.videoHeight;
  status(`${srcUrl.includes("source") ? "original 4K HEVC" : "proxy 1080p"} · ${video.videoWidth}×${video.videoHeight} · ${video.duration.toFixed(1)}s`);
  video.play().catch(() => {});

  // elegir vídeo local (blob URL — el fichero no sale del navegador cliente;
  // funciona también desde Tailscale en otros ordenadores)
  $("#vidFile").onchange = async (e) => {
    const f = e.target.files[0];
    if (!f) return;
    status("cargando vídeo local: " + f.name);
    video.src = URL.createObjectURL(f);
    video.onerror = () => status("ERROR vídeo local: " + (video.error?.message || "?") + " (codec no soportado?)");
    video.onloadeddata = () => status(`vídeo local ok: ${f.name} · ${video.videoWidth}×${video.videoHeight} · ${video.duration.toFixed(1)}s`);
    await new Promise((res) => { video.onloadeddata = () => { video.onloadeddata = null; res(); }; setTimeout(res, 15000); });
    if (!video.videoWidth){ status("vídeo local sin pistas decodificables (¿HEVC en este navegador?)"); return; }
    canvas.width = video.videoWidth; canvas.height = video.videoHeight;
    P.resetHistory = true;
    video.play().catch(() => {});
    status(`vídeo local: ${f.name} · ${video.videoWidth}×${video.videoHeight}`);
    dirty = true;
  };

  $("#play").onclick = () => video.paused ? video.play() : video.pause();
  const seekBar = $("#seek"), tcode = $("#tcode");
  seekBar.oninput = () => { video.currentTime = +seekBar.value * video.duration; dirty = true; };
  addEventListener("keydown", (e) => { if (e.code === "Space"){ e.preventDefault(); video.paused ? video.play() : video.pause(); } });
  $("#ref").onclick = () => $("#refimg").classList.toggle("on");

  let dirty = true, frames = 0, fpsT = performance.now(), lastShot = 0, lastT = -1, lastMotionRead = 0, motionAvg = 0;
  function loop(){
    requestAnimationFrame(loop);
    tick();
  }
  // setInterval de refuerzo: en pestañas en segundo plano rAF no dispara
  // (clientes remotos) y sin esto no hay render ni telemetría.
  let lastTick = 0;
  setInterval(() => { const n = performance.now(); if (n - lastTick > 250) tick(); }, 100);
  function tick(){
    lastTick = performance.now();
    const now = lastTick;
    if (now - lastShot > 3000){
      lastShot = now;
      try {
        const s = document.createElement("canvas");
        s.width = 960; s.height = Math.round(960 * canvas.height / canvas.width) || 540;
        s.getContext("2d").drawImage(canvas, 0, 0, s.width, s.height);
        s.toBlob(b => b && fetch("/shot", { method: "POST", body: b }), "image/png");
      } catch (e) {}
    }
    if (video.readyState < 2) return;
    let jump = false;
    if (Math.abs(video.currentTime - lastT) > 1e-4){
      jump = Math.abs(video.currentTime - lastT) > 0.5;
      lastT = video.currentTime; dirty = true;
    }
    // auto shutter: métrica de movimiento con zona muerta + slew limitado.
    // Objetivo ≈ 360° (2× el blur cinematográfico de 180°) como techo típico.
    if (P.shutterAuto && !video.paused && now - lastMotionRead > 250){
      lastMotionRead = now;
      const m = pipe.readMotion();
      motionAvg = motionAvg * 0.8 + m * 0.2;
      const target = Math.min(0.55, Math.max(0, motionAvg - 0.004) * 3.0 * P.shutterSens);
      P.shutter += (target - P.shutter) * 0.3;
      const el = document.querySelector('[data-k="shutter"]');
      if (el){ el.value = P.shutter; el.parentElement.querySelector("em").textContent = P.shutter.toFixed(2); }
    }
    if (!dirty && !(P.shutterAuto && !video.paused)) return;
    dirty = false;
    P.resetHistory = jump;
    try {
      pipe.setSourceVideo(video);
      pipe.render(Object.assign(P, { yuvNorm: 65535, fullRange: false }), video.currentTime, Math.floor(video.currentTime * 60));
      frames++;
      if (!window.__probed){   // telemetría: brillo real del canvas (diagnóstico remoto)
        window.__probed = true;
        const gl2 = pipe.gl, px = new Uint8Array(4 * 4 * 4);
        gl2.readPixels(canvas.width>>1, canvas.height>>1, 4, 4, gl2.RGBA, gl2.UNSIGNED_BYTE, px);
        const mean = px.filter((_, i) => i % 4 < 3).reduce((a, b) => a + b, 0) / 48;
        fetch("/log", { method: "POST", body: `probe canvas=${canvas.width}x${canvas.height} video=${video.videoWidth}x${video.videoHeight} rs=${video.readyState} mean=${mean.toFixed(1)} vf=${!!window.VideoFrame}` }).catch(() => {});
      }
    } catch (e) { status("render ERR: " + e.message); }
    if (now - fpsT > 1000){ $("#fps").textContent = frames + " fps"; frames = 0; fpsT = now; }
    if (video.duration){ seekBar.value = video.currentTime / video.duration; tcode.textContent = video.currentTime.toFixed(2) + "s"; }
  }
  loop();
}

async function renderMode(){
  document.body.innerHTML = '<pre id="rlog" style="color:#7fd4a0;background:#111;height:100vh;margin:0;padding:20px;font:14px monospace"></pre>';
  const log = (t) => { document.querySelector("#rlog").textContent += t + "\n"; };
  const job = await (await fetch("assets/_job.json?x=" + Date.now())).json();
  Object.assign(P, job.prefs || {});
  P.shutterAuto = false;                 // determinismo total
  P.wipe = 1.0;
  const lutMeta = await (await fetch(job.lutJson)).json();
  const lutBuf = new Float32Array(await (await fetch(job.lutBin)).arrayBuffer());
  const gMeta = await (await fetch("assets/grain.json")).json();
  const gBuf = new Uint16Array(await (await fetch("assets/grain.bin")).arrayBuffer());
  const canvas = document.createElement("canvas");
  document.body.appendChild(canvas);
  canvas.style.display = "none";
  const pipe = new Pipeline(canvas, lutBuf, lutMeta.size);
  pipe.setGrainPlate(gBuf, gMeta.size);
  const video = document.createElement("video");
  video.muted = true; video.playsInline = true;
  video.src = job.video;
  await new Promise((res, rej) => { video.onloadeddata = res; video.onerror = () => rej(new Error("no carga vídeo")); });
  canvas.width = video.videoWidth; canvas.height = video.videoHeight;
  const fps = job.fps, total = Math.floor(video.duration * fps);
  log(`render: ${video.videoWidth}×${video.videoHeight} · ${total} frames @ ${fps}fps`);
  await fetch("/render_start", { method: "POST", body: JSON.stringify({
    fps, total, out: job.out, audio_src: job.videoAbs, codec: job.codec || "prores_ks" }) });
  const t0 = performance.now();
  for (let i = 0; i < total; i++){
    const t = i / fps;
    video.currentTime = t;
    await new Promise(r => { video.onseeked = r; });
    pipe.setSourceVideo(video);
    P.resetHistory = i === 0;
    pipe.render(Object.assign(P, { yuvNorm: 65535, fullRange: false }), t, i);
    const blob = await new Promise(r => canvas.toBlob(r, "image/png"));
    await fetch("/render_frame", { method: "POST", body: blob });
    if (i % 20 === 0){
      const el = (performance.now() - t0) / 1000, rate = i / Math.max(el, 0.1);
      log(`  ${i}/${total} · ${rate.toFixed(1)} fps · ETA ${((total - i) / Math.max(rate, 0.1) / 60).toFixed(1)} min`);
    }
  }
  await fetch("/render_done", { method: "POST", body: "{}" });
  log("RENDER DONE → " + job.out);
  document.title = "RENDER DONE";
}

main().catch(e => status("ERROR: " + e.message));
