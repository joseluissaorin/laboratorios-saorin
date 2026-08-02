// foley.js — sonido del estudio, 100% procedural (WebAudio, sin assets).
// Cada interacción tiene su materia: mecánica de proyector, cinta, tijera, campana.

let ctx = null;
let master = null;
let muted = localStorage.getItem("svs_mute") === "1";
let motorNodes = null;

function ac() {
  if (!ctx) {
    ctx = new (window.AudioContext || window.webkitAudioContext)();
    master = ctx.createGain();
    master.gain.value = muted ? 0 : 0.5;
    master.connect(ctx.destination);
  }
  if (ctx.state === "suspended") ctx.resume();
  return ctx;
}

export function setMuted(m) {
  muted = m;
  localStorage.setItem("svs_mute", m ? "1" : "0");
  if (master) master.gain.value = m ? 0 : 0.5;
  if (m) stopMotor();
}
export function isMuted() { return muted; }

let noiseBuf = null;
function noise() {
  const c = ac();
  if (!noiseBuf) {
    noiseBuf = c.createBuffer(1, c.sampleRate, c.sampleRate);
    const d = noiseBuf.getChannelData(0);
    for (let i = 0; i < d.length; i++) d[i] = Math.random() * 2 - 1;
  }
  return noiseBuf;
}

function env(node, t0, a, peak, d) {
  node.gain.setValueAtTime(0.0001, t0);
  node.gain.exponentialRampToValueAtTime(peak, t0 + a);
  node.gain.exponentialRampToValueAtTime(0.0001, t0 + a + d);
}

function burst({ f = 2000, q = 4, peak = 0.3, a = 0.002, d = 0.05, type = "bandpass" }) {
  const c = ac(), t = c.currentTime;
  const src = c.createBufferSource(); src.buffer = noise();
  const flt = c.createBiquadFilter(); flt.type = type; flt.frequency.value = f; flt.Q.value = q;
  const g = c.createGain();
  env(g, t, a, peak, d);
  src.connect(flt); flt.connect(g); g.connect(master);
  src.start(t); src.stop(t + a + d + 0.05);
}

function tone({ f = 220, peak = 0.2, a = 0.004, d = 0.12, type = "sine", bend = 0 }) {
  const c = ac(), t = c.currentTime;
  const o = c.createOscillator(); o.type = type; o.frequency.value = f;
  if (bend) o.frequency.exponentialRampToValueAtTime(Math.max(f + bend, 30), t + a + d);
  const g = c.createGain();
  env(g, t, a, peak, d);
  o.connect(g); g.connect(master);
  o.start(t); o.stop(t + a + d + 0.05);
}

/* ── vocabulario ── */

// tic seco de slider (madera pequeña)
let lastTick = 0;
export function tick() {
  const now = performance.now();
  if (now - lastTick < 45) return;   // no ametralla
  lastTick = now;
  burst({ f: 3200, q: 6, peak: 0.06, d: 0.018 });
}

// botón: pulsación con cuerpo
export function press() {
  burst({ f: 1400, q: 3, peak: 0.10, d: 0.03 });
  tone({ f: 190, peak: 0.05, d: 0.05, type: "triangle" });
}

// clip cae en la timeline: thunk de madera + papel
export function thunk() {
  tone({ f: 120, peak: 0.30, d: 0.10, type: "sine", bend: -60 });
  burst({ f: 900, q: 1.2, peak: 0.12, d: 0.05, type: "lowpass" });
}

// cuchilla: dos tijeretazos metálicos
export function snip() {
  burst({ f: 5200, q: 8, peak: 0.22, d: 0.03 });
  setTimeout(() => burst({ f: 4200, q: 8, peak: 0.16, d: 0.04 }), 55);
}

// arrancar/soltar un arrastre de cinta
export function grab() { burst({ f: 700, q: 2, peak: 0.07, d: 0.04, type: "lowpass" }); }
export function release() { burst({ f: 500, q: 2, peak: 0.05, d: 0.06, type: "lowpass" }); }

// scrub: roce de cinta proporcional a la velocidad
let scrubG = null;
export function scrub(speed) {
  const c = ac();
  if (!scrubG) {
    const src = c.createBufferSource(); src.buffer = noise(); src.loop = true;
    const flt = c.createBiquadFilter(); flt.type = "bandpass"; flt.frequency.value = 1200; flt.Q.value = 0.8;
    scrubG = c.createGain(); scrubG.gain.value = 0;
    src.connect(flt); flt.connect(scrubG); scrubG.connect(master);
    src.start();
  }
  const v = Math.min(Math.abs(speed) * 0.02, 0.12);
  scrubG.gain.setTargetAtTime(v, c.currentTime, 0.03);
}
export function scrubEnd() {
  if (scrubG) scrubG.gain.setTargetAtTime(0, ac().currentTime, 0.06);
}

// proyector: motor + obturador de 3 palas mientras reproduce (muy al fondo)
export function motorStart() {
  if (motorNodes || muted) return;
  const c = ac();
  const hum = c.createOscillator(); hum.type = "sawtooth"; hum.frequency.value = 49;
  const humF = c.createBiquadFilter(); humF.type = "lowpass"; humF.frequency.value = 130;
  const humG = c.createGain(); humG.gain.value = 0.0;
  hum.connect(humF); humF.connect(humG); humG.connect(master);

  const flap = c.createBufferSource(); flap.buffer = noise(); flap.loop = true;
  const flapF = c.createBiquadFilter(); flapF.type = "bandpass"; flapF.frequency.value = 2600; flapF.Q.value = 2;
  const flapG = c.createGain(); flapG.gain.value = 0;
  const lfo = c.createOscillator(); lfo.type = "square"; lfo.frequency.value = 24;
  const lfoG = c.createGain(); lfoG.gain.value = 0.012;
  lfo.connect(lfoG); lfoG.connect(flapG.gain);
  flap.connect(flapF); flapF.connect(flapG); flapG.connect(master);

  hum.start(); flap.start(); lfo.start();
  humG.gain.setTargetAtTime(0.035, c.currentTime, 0.4);
  motorNodes = { hum, flap, lfo, humG, flapG };
}
export function motorStop() { stopMotor(); }
function stopMotor() {
  if (!motorNodes) return;
  const c = ctx, t = c.currentTime;
  motorNodes.humG.gain.setTargetAtTime(0, t, 0.15);
  motorNodes.flapG.gain.cancelScheduledValues(t);
  motorNodes.flapG.gain.setTargetAtTime(0, t, 0.1);
  const n = motorNodes;
  setTimeout(() => { try { n.hum.stop(); n.flap.stop(); n.lfo.stop(); } catch {} }, 600);
  motorNodes = null;
}

// campana de laboratorio: render terminado
export function bell() {
  const c = ac(), t = c.currentTime;
  for (const [f, p, d] of [[1318, 0.25, 1.6], [1975, 0.12, 1.1], [3227, 0.05, 0.7]]) {
    const o = c.createOscillator(); o.type = "sine"; o.frequency.value = f;
    const g = c.createGain();
    g.gain.setValueAtTime(0.0001, t);
    g.gain.exponentialRampToValueAtTime(p, t + 0.005);
    g.gain.exponentialRampToValueAtTime(0.0001, t + d);
    o.connect(g); g.connect(master);
    o.start(t); o.stop(t + d + 0.1);
  }
}

// cambio de página: hoja de papel
export function page() {
  burst({ f: 1800, q: 0.9, peak: 0.10, a: 0.01, d: 0.14, type: "bandpass" });
}

/* ── vocabulario del laboratorio ── */

// trinquete de la manivela (un diente)
let lastRatchet = 0;
export function ratchet() {
  const now = performance.now();
  if (now - lastRatchet < 28) return;
  lastRatchet = now;
  burst({ f: 2400, q: 5, peak: 0.09, d: 0.016 });
  burst({ f: 620, q: 2, peak: 0.04, d: 0.02, type: "lowpass" });
}

// chirrido del lápiz graso al marcar
export function squeak() {
  const c = ac(), t = c.currentTime;
  const o = c.createOscillator(); o.type = "sawtooth";
  o.frequency.setValueAtTime(900, t);
  o.frequency.exponentialRampToValueAtTime(1400, t + 0.06);
  o.frequency.exponentialRampToValueAtTime(700, t + 0.13);
  const flt = c.createBiquadFilter(); flt.type = "bandpass"; flt.frequency.value = 1600; flt.Q.value = 3;
  const g = c.createGain();
  env(g, t, 0.01, 0.07, 0.13);
  o.connect(flt); flt.connect(g); g.connect(master);
  o.start(t); o.stop(t + 0.2);
}

// la empalmadora baja: CHUNK
export function chunk() {
  tone({ f: 95, peak: 0.4, d: 0.09, type: "sine", bend: -40 });
  burst({ f: 3000, q: 1.2, peak: 0.28, a: 0.001, d: 0.03 });
  setTimeout(() => burst({ f: 500, q: 1.5, peak: 0.12, d: 0.06, type: "lowpass" }), 30);
}

// verter un baño (preset)
export function pour() {
  const c = ac(), t = c.currentTime;
  const src = c.createBufferSource(); src.buffer = noise();
  const flt = c.createBiquadFilter(); flt.type = "bandpass"; flt.Q.value = 1.4;
  flt.frequency.setValueAtTime(600, t);
  flt.frequency.exponentialRampToValueAtTime(1500, t + 0.35);
  const g = c.createGain();
  g.gain.setValueAtTime(0.0001, t);
  g.gain.exponentialRampToValueAtTime(0.16, t + 0.08);
  g.gain.exponentialRampToValueAtTime(0.0001, t + 0.5);
  src.connect(flt); flt.connect(g); g.connect(master);
  src.start(t); src.stop(t + 0.6);
}

// el cordón de la luz: click + péndulo
export function cord() {
  burst({ f: 1900, q: 4, peak: 0.2, d: 0.03 });
  setTimeout(() => tone({ f: 150, peak: 0.1, d: 0.12, type: "triangle" }), 60);
}

// burbujeo de las cubetas (bucle mientras revela)
let bubbleNodes = null;
export function bubbleStart() {
  if (bubbleNodes || muted) return;
  const c = ac();
  const src = c.createBufferSource(); src.buffer = noise(); src.loop = true;
  const flt = c.createBiquadFilter(); flt.type = "bandpass"; flt.frequency.value = 420; flt.Q.value = 2.4;
  const g = c.createGain(); g.gain.value = 0;
  const lfo = c.createOscillator(); lfo.type = "sine"; lfo.frequency.value = 2.7;
  const lg = c.createGain(); lg.gain.value = 0.02;
  lfo.connect(lg); lg.connect(g.gain);
  src.connect(flt); flt.connect(g); g.connect(master);
  src.start(); lfo.start();
  g.gain.setTargetAtTime(0.05, c.currentTime, 0.4);
  bubbleNodes = { src, lfo, g };
}
export function bubbleStop() {
  if (!bubbleNodes) return;
  bubbleNodes.g.gain.setTargetAtTime(0, ctx.currentTime, 0.3);
  const n = bubbleNodes;
  setTimeout(() => { try { n.src.stop(); n.lfo.stop(); } catch {} }, 900);
  bubbleNodes = null;
}

// beep de la cola de cabecera
export function beep() { tone({ f: 800, peak: 0.22, a: 0.002, d: 0.06, type: "sine" }); }
