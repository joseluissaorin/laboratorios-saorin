// ui.js — film-look lab (app Tauri): LUT + film emulation, ficheros nativos,
// render offline vía ffmpeg. Sin servidor, sin red.

import { Pipeline } from "./pipeline.js";

const T = window.__TAURI__;
const invoke = T.core.invoke;
const { open, save } = T.dialog;
const { convertFileSrc } = T.core;

// ── Estado y presets (idénticos al lab web) ────────────────────────────────
// Default = los prefs de José Luis (filmlook-prefs.json) — "saorín · revelado"
const FILM_DEFAULT = {
  gain: 0.1, pushPull: 0, compImpact: 1.35, compWP: 1, compRange: 0.36,
  shutter: 0.15, shutterAuto: true, shutterSens: 0.1,
  grain: 0.13, grainSize: 5, grainRough: 0.47, grainChroma: 0, grainDefocus: 0.3,
  grainShadows: 0.7, grainMids: 1, grainHighs: 0.61, grainRed: 1.35, grainBlue: 1.3, filmRes: 1,
  halation: 1.5, halHue: 1, halSat: 0.9, halThr: 0.8, halSpread: 0.6, halWhite: 0.1,
  bloom: 0.6, bloomThr: 0.8, bloomWarm: 0.3,
  softness: 0.1, acutance: 0.11, colorSep: 0.03,
  hueSkew: 0.96, crosstalk: 1, subtractive: 1, stockSat: 1.15, print: 0.06,
  vignette: 0, vigSize: 0.55, vigRound: 1, vigCX: 0.5, vigCY: 0.5,
  chroma: 0, weave: 0.15, weaveRot: 0.3, flicker: 0, breath: 0, breathRate: 0.5, dust: 0,
  frameInset: 0, frameCorner: 40, frameWobble: 1,
};
const STOCK_50D = { hueSkew: 1.2, crosstalk: 0.35, subtractive: 0.8, stockSat: 1.2, print: 0.7, compImpact: 0.3 };
const STOCK_250D = { hueSkew: 1.0, crosstalk: 0.3, subtractive: 0.7, stockSat: 1.0, print: 0.6, compImpact: 0.15 };
const STOCK_500T = { hueSkew: 1.1, crosstalk: 0.35, subtractive: 0.65, stockSat: 0.95, print: 0.6, compImpact: 0.2, halation: 0.6 };
const STOCK_FUJI = { hueSkew: 0.8, crosstalk: 0.25, subtractive: 0.5, stockSat: 0.85, print: 0.4, compImpact: 0.4 };
const STOCK_C800 = { hueSkew: 1.0, crosstalk: 0.3, subtractive: 0.65, stockSat: 0.95, print: 0.6, halation: 1.2, halSpread: 1.0, halThr: 0.4 };
const PRESET_CHIMERA_S16 = { ...FILM_DEFAULT,
  grain: 0.45, grainSize: 3.4, grainRough: 0.5, grainChroma: 0.35, grainDefocus: 0.3,
  grainShadows: 0.7, grainMids: 1.0, grainHighs: 0.6, grainBlue: 1.3, filmRes: 0.7,
  halation: 0.25, halHue: 1.0, halSat: 0.9, halThr: 0.8, halSpread: 0.6, halWhite: 0.1,
  bloom: 0.2, bloomThr: 0.8, bloomWarm: 0.3, softness: 0.1, vignette: 0.2, weave: 0.15, chroma: 0 };
const PRESET_CHIMERA_BOLEX = { ...FILM_DEFAULT,
  grain: 0.6, grainSize: 4.5, grainRough: 0.55, grainChroma: 0.35, grainDefocus: 0.4,
  grainShadows: 0.8, grainMids: 1.1, grainHighs: 0.6, grainBlue: 1.3, filmRes: 0.9,
  halation: 0.3, halThr: 0.75, halSpread: 0.6, halWhite: 0.1, bloom: 0.25, bloomWarm: 0.35,
  softness: 0.4, vignette: 0.35, weave: 0.5, flicker: 0.3 };
const PRESET_CINESTILL = { ...FILM_DEFAULT,
  grain: 0.35, grainSize: 3.0, grainRough: 0.4, filmRes: 0.5,
  halation: 1.2, halHue: 1.0, halSat: 1.0, halThr: 0.4, halSpread: 1.0, halWhite: 0.0,
  bloom: 0.5, bloomThr: 0.65, bloomWarm: 0.4, softness: 0.2, vignette: 0.3 };
const FILM_OFF = { ...FILM_DEFAULT, grain: 0, halation: 0, bloom: 0, softness: 0, vignette: 0, weave: 0, flicker: 0, dust: 0, chroma: 0, shutter: 0, shutterAuto: false };

const P = { ...FILM_DEFAULT, lutOn: true, inputLutOn: true, wipe: 1.0, resetHistory: true };

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
const status = (t) => { $("#status").textContent = t; };
window.addEventListener("error", e => status("ERR " + e.message));
window.addEventListener("unhandledrejection", e => status("REJ " + (e.reason?.message || e.reason)));

function buildUI(onChange){
  const panel = $("#controls");
  const preset = document.createElement("div");
  preset.className = "row";
  preset.innerHTML = `
    <button id="pD1">saorín · revelado</button>
    <button id="pS16">La Chimera · S16</button>
    <button id="pBolex">La Chimera · Bolex</button>
    <button id="pC800">CineStill 800T</button>
    <button id="pOff">FX off</button>
    <button id="pVid">elegir vídeo…</button>
    <button id="pExp">exportar prefs</button>
    <button id="pImp">importar prefs</button>`;
  panel.appendChild(preset);
  const lutrow = document.createElement("div");
  lutrow.className = "row";
  lutrow.innerHTML = `
    <label class="chk"><input type="checkbox" id="inLutOn" checked> entrada log→709</label>
    <button id="pLutA">cambiar…</button><span id="lutAName" class="note">directo (sin transformar)</span>`;
  panel.appendChild(lutrow);
  const lutrow2 = document.createElement("div");
  lutrow2.className = "row";
  lutrow2.innerHTML = `
    <label class="chk"><input type="checkbox" id="lutOn" checked> LUT grade</label>
    <button id="pLutB">cambiar…</button><span id="lutBName" class="note">pre 709 saorín</span>
    <label class="chk wipe">wipe <input type="range" id="wipe" min="0" max="1" step="0.01" value="1"></label>`;
  panel.appendChild(lutrow2);
  const qrow = document.createElement("div");
  qrow.className = "row";
  qrow.innerHTML = `<label class="chk">calidad <select id="qSel" style="background:#2c2721;color:var(--fg);border:1px solid #4a4136;border-radius:6px">
    <option value="auto">auto (benchmark)</option>
    <option value="1">ultra (nativa)</option>
    <option value="0.5">rápida (½)</option>
    <option value="0.35">patata (⅓)</option>
  </select></label><span id="qInfo" class="note"></span>`;
  panel.appendChild(qrow);
  $("#qSel").onchange = (e) => {
    const v = e.target.value;
    P.quality = v;
    if (v !== "auto"){ window.__pipe.setQuality(+v); $("#qInfo").textContent = ""; }
    onChange();
  };
  P.quality = "auto";
  $("#pD1").onclick = () => { Object.assign(P, FILM_DEFAULT); sync(); onChange(); };
  $("#pS16").onclick = () => { Object.assign(P, PRESET_CHIMERA_S16); sync(); onChange(); };
  $("#pBolex").onclick = () => { Object.assign(P, PRESET_CHIMERA_BOLEX); sync(); onChange(); };
  $("#pC800").onclick = () => { Object.assign(P, PRESET_CINESTILL); sync(); onChange(); };
  $("#pOff").onclick = () => { Object.assign(P, FILM_OFF); sync(); onChange(); };

  const stocks = [["50D", STOCK_50D], ["250D", STOCK_250D], ["500T", STOCK_500T], ["Fuji Eterna", STOCK_FUJI], ["CineStill 800T", STOCK_C800]];
  const srow = document.createElement("div");
  srow.className = "row";
  srow.innerHTML = `<span class="note">stock:</span>` + stocks.map((s, i) => `<button data-stock="${i}">${s[0]}</button>`).join("");
  panel.appendChild(srow);
  srow.querySelectorAll("[data-stock]").forEach(b => b.onclick = () => {
    Object.assign(P, stocks[+b.dataset.stock][1]); sync(); onChange();
  });

  $("#lutOn").onchange = (e) => { P.lutOn = e.target.checked; onChange(); };
  $("#inLutOn").onchange = (e) => { P.inputLutOn = e.target.checked; onChange(); };
  $("#wipe").oninput = (e) => { P.wipe = +e.target.value; onChange(); };
  const loadLut = async (slot) => {
    const path = await open({ filters: [{ name: "LUT/Hald", extensions: ["cube", "tif", "tiff", "dpx", "png"] }] });
    if (!path) return;
    status("convirtiendo LUT…");
    try {
      const r = await invoke("convert_lut", { path });
      const data = new Float32Array(new Uint8Array(r.bytes).buffer);
      if (slot === "A"){ window.__pipe.setInputLut(data, r.size); $("#lutAName").textContent = path.split("/").pop(); P.inputLutOn = true; $("#inLutOn").checked = true; }
      else { window.__pipe.setLut(data, r.size); $("#lutBName").textContent = path.split("/").pop(); P.lutOn = true; $("#lutOn").checked = true; }
      status("LUT cargada");
      onChange();
    } catch (err) { status("LUT falló: " + err); }
  };
  $("#pLutA").onclick = () => loadLut("A");
  $("#pLutB").onclick = () => loadLut("B");

  $("#pExp").onclick = async () => {
    const path = await save({ defaultPath: "filmlook-prefs.json", filters: [{ name: "JSON", extensions: ["json"] }] });
    if (path) await invoke("write_text", { path, text: JSON.stringify(P, null, 1) });
  };
  $("#pImp").onclick = async () => {
    const path = await open({ filters: [{ name: "JSON", extensions: ["json"] }] });
    if (!path) return;
    try {
      const bytes = await invoke("read_bytes", { path });
      Object.assign(P, JSON.parse(new TextDecoder().decode(new Uint8Array(bytes))));
      sync(); onChange();
      status("prefs importadas");
    } catch (err) { status("prefs inválidas: " + err.message); }
  };

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
}

function sync(){
  document.querySelectorAll("[data-k]").forEach((el) => {
    const k = el.dataset.k;
    if (el.type === "checkbox") el.checked = !!P[k];
    else { el.value = P[k]; el.parentElement.querySelector("em").textContent = (+P[k]).toFixed(2); }
  });
}

async function main(){
  buildUI(() => { dirty = true; });
  sync();
  status("cargando LUT…");

  const canvas = $("#view");
  const pipe = new Pipeline(canvas);
  window.__pipe = pipe;
  // LUT A: entrada log→709 (la del fabricante de tu cámara, si la tiene)
  //         · LUT B: el grade, sobre señal 709
  const inMeta = await (await fetch("assets/input_lut.json")).json();
  const inBuf = new Float32Array(await (await fetch("assets/input_lut.bin")).arrayBuffer());
  pipe.setInputLut(inBuf, inMeta.size);
  const lutMeta = await (await fetch("assets/grade_lut.json")).json();
  const lutBuf = new Float32Array(await (await fetch("assets/grade_lut.bin")).arrayBuffer());
  pipe.setLut(lutBuf, lutMeta.size);
  const gMeta = await (await fetch("assets/grain.json")).json();
  const gBuf = new Uint16Array(await (await fetch("assets/grain.bin")).arrayBuffer());
  pipe.setGrainPlate(gBuf, gMeta.size);
  // buffers vivos para poder clonar el pipeline en el render offscreen
  window.__lutState = { inBuf, inN: inMeta.size, lutBuf, lutN: lutMeta.size, gBuf, gN: gMeta.size };
  status("elige un vídeo para empezar");

  const video = document.createElement("video");
  video.muted = true; video.loop = true; video.playsInline = true;
  let videoPath = null, videoUrl = null;

  // DEMO: autocarga el clip de prueba si existe (temporal)
  const demo = "";   // pon aquí la ruta de un vídeo tuyo para probar
  videoPath = demo;
  videoUrl = convertFileSrc(demo);
  video.src = videoUrl;
  video.onloadeddata = () => {
    canvas.width = video.videoWidth; canvas.height = video.videoHeight;
    video.play().catch(() => {});
    status(`DEMO · ${video.videoWidth}×${video.videoHeight} · ${video.duration.toFixed(1)}s`);
    dirty = true;
    // AUTOTEST render (120 frames a /tmp, mide la ruta raw PBO+IPC)
    if (!window.__renderTested){
      window.__renderTested = true;
      setTimeout(async () => {
        try {
          const t0 = performance.now();
          const done = await renderFlow({ out: "/tmp/render_autotest.mov", fps: 24, codec: "prores_ks", maxFrames: 120,
            onProgress: (i, total) => { if (i % 20 === 0) status(`autotest render ${i}/${total}`); } });
          const rate = done / ((performance.now() - t0) / 1000);
          const s = window.__stages || {};
          status(`AUTOTEST ${rate.toFixed(1)}fps [${window.__renderPath}] · dec ${(s.decode||0).toFixed(0)} up ${(s.upload||0).toFixed(0)} render ${(s.render||0).toFixed(0)} rb ${(s.readback||0).toFixed(0)} http ${(s.ipc||0).toFixed(0)} ms/f`);
        } catch (e) { status("autotest ERROR: " + (e.message || e)); }
      }, 4000);
    }
  };

  $("#pVid").onclick = async () => {
    const path = await open({ filters: [{ name: "Vídeo", extensions: ["mp4", "mov", "m4v", "mkv", "webm"] }] });
    if (!path) return;
    videoPath = path;
    videoUrl = convertFileSrc(path);
    video.src = videoUrl;
    await new Promise((res) => { video.onloadeddata = res; video.onerror = () => { status("codec no soportado por esta webview"); res(); }; setTimeout(res, 20000); });
    if (!video.videoWidth) return;
    canvas.width = video.videoWidth; canvas.height = video.videoHeight;
    P.resetHistory = true;
    video.play().catch(() => {});
    status(`${path.split("/").pop()} · ${video.videoWidth}×${video.videoHeight} · ${video.duration.toFixed(1)}s`);
    dirty = true;
  };

  $("#play").onclick = () => video.paused ? video.play() : video.pause();
  const seekBar = $("#seek"), tcode = $("#tcode");
  // scrubbing: buscar en vivo al arrastrar; renderiza cada frame aterrizado
  let scrubbing = false;
  seekBar.addEventListener("input", () => {
    if (!video.duration) return;
    scrubbing = true;
    video.currentTime = +seekBar.value * video.duration;
    tcode.textContent = (+seekBar.value * video.duration).toFixed(2) + "s";
  });
  seekBar.addEventListener("change", () => { scrubbing = false; });
  video.addEventListener("seeked", () => { dirty = true; });
  addEventListener("keydown", (e) => { if (e.code === "Space"){ e.preventDefault(); video.paused ? video.play() : video.pause(); } });
  $("#ref").onclick = () => $("#refimg").classList.toggle("on");

  // ── RENDER offline: pipeline OCULTO separado (la preview no se toca),
  // modal con destino/codec/fps, progreso con ETA y cancelación.
  let rendering = false, cancelRender = false;
  $("#renderBtn").onclick = () => {
    if (!video.videoWidth || !videoPath){ status("carga un vídeo primero"); return; }
    if (rendering){ $("#rmodal").classList.add("on"); return; }
    $("#rmodal").classList.add("on");
  };
  let outPath = null;
  $("#rOut").onclick = async () => {
    const def = (videoPath || "render").replace(/\.[^.]+$/, "") + "_film.mov";
    outPath = await save({ defaultPath: def, filters: [{ name: "Vídeo", extensions: ["mov", "mp4"] }] });
    if (outPath){
      $("#rOutName").textContent = outPath.split("/").pop();
      $("#rCodec").value = outPath.endsWith(".mp4") ? "hevc_videotoolbox" : "prores_ks";
    }
  };
  $("#rCancel").onclick = () => { cancelRender = true; };

  async function renderFlow({ out, fps, codec, maxFrames = Infinity, onProgress = () => {} }){
    const S = window.__lutState;
    const rc = document.createElement("canvas");
    rc.width = video.videoWidth; rc.height = video.videoHeight;
    const rp = new Pipeline(rc);
    rp.setInputLut(S.inBuf, S.inN);
    rp.setLut(S.lutBuf, S.lutN);
    rp.setGrainPlate(S.gBuf, S.gN);
    const RP = { ...P, shutterAuto: false, wipe: 1.0, yuvNorm: 65535, fullRange: false };
    const SRV = "http://127.0.0.1:8741";
    await fetch(SRV + "/render_start", { method: "POST",
      body: JSON.stringify({ out, fps, width: rc.width, height: rc.height, audioSrc: videoPath, codec }) });
    const t0 = performance.now();
    const acc = { decode: 0, upload: 0, render: 0, readback: 0, ipc: 0 };
    let done = 0;

    // vía rápida: WebCodecs secuencial (sin seeks, decode a ritmo de hardware)
    let dec = null;
    try {
      if (!window.MP4Box){
        await new Promise((res, rej) => {
          const s = document.createElement("script");
          s.src = "lib/mp4box.all.min.js"; s.onload = res; s.onerror = rej;
          document.head.appendChild(s);
        });
      }
      const { SeqDecoder } = await import("./decode.js");
      dec = new SeqDecoder();
      if (!(await dec.open(videoUrl))) dec = null;
    } catch (e) { dec = null; }

    if (dec){
      const nativeFps = dec.fps || fps;
      const total = Math.min(Math.floor(dec.duration * fps), maxFrames);
      let o = 0, frame = null;
      const pull = async () => {
        const t = performance.now();
        const f = await dec.next();
        acc.decode += performance.now() - t;
        return f;
      };
      frame = await pull();
      while (frame && o < total && !cancelRender){
        const target = o / fps;
        while (frame && frame.timestamp / 1e6 < target - 0.5 / nativeFps){ frame.close(); frame = await pull(); }
        if (!frame) break;
        let t = performance.now();
        rp.setSourceFrame(frame);
        acc.upload += performance.now() - t; t = performance.now();
        RP.resetHistory = o === 0;
        rp.render(RP, target, o);
        acc.render += performance.now() - t; t = performance.now();
        const buf = rp.readbackPBO();
        acc.readback += performance.now() - t; t = performance.now();
        if (o > 0) await fetch(SRV + "/render_frame", { method: "POST", body: buf.slice(0) });
        acc.ipc += performance.now() - t;
        frame.close();
        frame = await pull();
        o++;
        onProgress(o, total, t0);
      }
      const last = rp.readbackPBO();
      if (!cancelRender) await fetch(SRV + "/render_frame", { method: "POST", body: last.slice(0) });
      done = o;
    } else {
      // fallback: seeks (WebKit sin WebCodecs/HEVC)
      const rv = document.createElement("video");
      rv.muted = true; rv.playsInline = true;
      rv.src = videoUrl;
      await new Promise((res, rej) => { rv.onloadeddata = res; rv.onerror = () => rej(new Error("no carga el vídeo para render")); });
      const total = Math.min(Math.floor(rv.duration * fps), maxFrames);
      let i = 0;
      for (; i < total && !cancelRender; i++){
        let t = performance.now();
        rv.currentTime = i / fps;
        await new Promise(r => { rv.onseeked = r; });
        acc.decode += performance.now() - t; t = performance.now();
        rp.setSourceVideo(rv);
        acc.upload += performance.now() - t; t = performance.now();
        RP.resetHistory = i === 0;
        rp.render(RP, i / fps, i);
        acc.render += performance.now() - t; t = performance.now();
        const buf = rp.readbackPBO();
        acc.readback += performance.now() - t; t = performance.now();
        if (i > 0) await fetch(SRV + "/render_frame", { method: "POST", body: buf.slice(0) });
        acc.ipc += performance.now() - t;
        onProgress(i, total, t0);
        if (i % 5 === 0) await new Promise(r => setTimeout(r, 0));
      }
      const last = rp.readbackPBO();
      if (!cancelRender) await fetch(SRV + "/render_frame", { method: "POST", body: last.slice(0) });
      rv.src = ""; rv.load();
      done = i;
    }
    await fetch(SRV + "/render_done", { method: "POST" });
    rp.gl.getExtension("WEBGL_lose_context")?.loseContext();
    window.__stages = Object.fromEntries(Object.entries(acc).map(([k, v]) => [k, v / Math.max(done, 1)]));
    window.__renderPath = dec ? "webcodecs" : "seeks";
    return done;
  }

  $("#rStart").onclick = async () => {
    if (!outPath){ status("elige destino primero"); $("#rOut").click(); if (!outPath) return; }
    rendering = true; cancelRender = false;
    $("#rStart").disabled = true; $("#rCancel").disabled = false;
    const info = $("#rinfo"), fill = $("#rfill");
    try {
      const nativeFps = await invoke("probe_fps", { path: videoPath }).catch(() => 24);
      const sel = +$("#rFps").value;
      const fps = sel > 0 ? sel : Math.round(nativeFps * 1000) / 1000;
      const codec = $("#rCodec").value;
      const done = await renderFlow({ out: outPath, fps, codec,
        onProgress: (i, total, t0) => {
          const el = (performance.now() - t0) / 1000, rate = i / Math.max(el, 0.1);
          fill.style.width = (100 * i / total).toFixed(1) + "%";
          info.textContent = `${i}/${total} · ${rate.toFixed(1)} fps · ETA ${((total - i) / Math.max(rate, 0.1) / 60).toFixed(1)} min`;
        } });
      fill.style.width = "100%";
      info.textContent = cancelRender ? `⛔ cancelado en el frame ${done}` : `✅ ${outPath}`;
      status(cancelRender ? "render cancelado (parcial)" : "render terminado: " + outPath.split("/").pop());
    } catch (e) {
      info.textContent = "ERROR: " + (e.message || e);
      status("render ERROR: " + (e.message || e));
      try { await invoke("render_done"); } catch {}
    }
    rendering = false;
    $("#rStart").disabled = false; $("#rCancel").disabled = true;
    setTimeout(() => $("#rmodal").classList.remove("on"), 2500);
  };

  let dirty = true, frames = 0, fpsT = performance.now(), lastT = -1, lastMotionRead = 0, motionAvg = 0, benchT = 0, benchN = 0;
  function loop(){
    requestAnimationFrame(loop);
    const now = performance.now();
    if (video.readyState < 2) return;
    let jump = false;
    if (Math.abs(video.currentTime - lastT) > 1e-4){
      jump = Math.abs(video.currentTime - lastT) > 0.5;
      lastT = video.currentTime; dirty = true;
    }
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
      let tR = performance.now();
      pipe.render(Object.assign(P, { yuvNorm: 65535, fullRange: false }), video.currentTime, Math.floor(video.currentTime * 60));
      if (P.quality === "auto"){ pipe.gl.finish(); }   // medir GPU real solo en bench
      const dt = performance.now() - tR;
      frames++;
      // auto-benchmark: con los primeros ~50 frames elige tier de calidad
      if (P.quality === "auto"){
        benchT += dt; benchN++;
        if (benchN === 50){
          const fps = 1000 / (benchT / benchN);
          const tier = fps > 160 ? 1 : fps > 70 ? 0.5 : 0.35;
          pipe.setQuality(tier);
          P.quality = "done";
          $("#qInfo").textContent = `· auto → ${tier === 1 ? "ultra" : tier === 0.5 ? "rápida" : "patata"} (${fps.toFixed(0)} fps medidos)`;
        }
      }
    } catch (e) { status("render ERR: " + e.message); }
    if (!window.__dbg2){
      window.__dbg2 = true;
      pipe._glWatch = (msg) => status("GL ERR " + msg);
      setTimeout(() => { if (!pipe._glErrFound) status("chain ok, sin GL errors — chain: " + JSON.stringify(pipe.debugChain())); }, 3000);
    }
    if (now - fpsT > 1000){ $("#fps").textContent = frames + " fps"; frames = 0; fpsT = now; }
    if (video.duration && !scrubbing){ seekBar.value = video.currentTime / video.duration; tcode.textContent = video.currentTime.toFixed(2) + "s"; }
  }
  loop();
}

main().catch(e => status("ERROR: " + e.message));
