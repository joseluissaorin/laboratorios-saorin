// comp.wgsl — FILM COLOR + halation/bloom/softness/acutance + vignette cos⁴ +
// grain (plate, crisp, neg/print) + dust/scratches + breath/flicker + frame +
// CA + weave + wipe. Puerto del shader del lab (física completa).

struct CompParams {
  time: f32, seed: f32, wipe: f32, res_w: f32,
  res_h: f32, texel_x: f32, texel_y: f32, weave_px_x: f32,
  weave_px_y: f32, weave_rot: f32,
  hal_amount: f32, hal_hue: f32, hal_sat: f32, hal_thr: f32, hal_spread: f32, hal_white: f32,
  bloom_amount: f32, bloom_thr: f32, bloom_warm: f32,
  softness: f32, acutance: f32, color_sep: f32,
  hue_skew: f32, crosstalk: f32, subtractive: f32, stock_sat: f32, print: f32,
  grain_amount: f32, grain_size: f32, grain_rough: f32, grain_chroma: f32, grain_defocus: f32,
  grain_s: f32, grain_m: f32, grain_h: f32, grain_r: f32, grain_b: f32, film_res: f32,
  plate_n: f32,
  vig_amount: f32, vig_size: f32, vig_round: f32, vig_cx: f32, vig_cy: f32, ca: f32,
  dust: f32, flicker: f32, flicker_rate: f32, breath: f32, breath_rate: f32,
  frame_inset: f32, frame_corner: f32, frame_wobble: f32,
  // la LUPA cuentahílos: aumento (0 = sin lupa) y su centro en uv
  lupa: f32, lupa_cx: f32, lupa_cy: f32,
  // el fundido a color, lo último de todo (MOTOR §5bis)
  fundido: f32, fundido_color: f32, pad_f: f32, pad_g: f32,
};

@group(0) @binding(0) var<uniform> P: CompParams;
@group(0) @binding(1) var tBase: texture_2d<f32>;
@group(0) @binding(2) var tRaw: texture_2d<f32>;
@group(0) @binding(3) var tBlurB: texture_2d<f32>;
@group(0) @binding(4) var tBlurC: texture_2d<f32>;
@group(0) @binding(5) var tBlurD: texture_2d<f32>;
@group(0) @binding(6) var tGrain: texture_2d<f32>;
@group(0) @binding(7) var samp: sampler;
// LA PLACA DE GRANO SE REPITE. Está sintetizada por FFT justamente para ser
// periódica («REPEAT sin costuras», tools/make_grain.py), así que necesita su
// propio muestreador: con el de recorte, más allá del borde se arrastra el
// último píxel en vez de teselar. El motor del Mac ya lo hacía bien y la
// preview no — parte de los 47 dB que separaban las dos cadenas.
@group(0) @binding(8) var samp_rep: sampler;

struct VsOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
  var p = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
  var o: VsOut;
  o.pos = vec4(p[vi], 0.0, 1.0);
  o.uv = vec2(p[vi].x, -p[vi].y) * 0.5 + 0.5;  // fila 0 = arriba: cada pase preserva orientación (fix fantasma invertido)
  return o;
}

fn hash(p: vec2<f32>) -> f32 {
  var q = fract(p * vec2(123.34, 456.21));
  q += vec2(dot(q, q + 45.32));
  return fract(q.x * q.y);
}
fn gnoise(p: vec2<f32>) -> f32 {
  return (hash(p) + hash(p + 17.7) + hash(p + 31.3) + hash(p + 47.9)) * 0.5 - 1.0;
}
fn vnoise(p: vec2<f32>) -> f32 {
  let i = floor(p);
  var f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  let a = hash(i);
  let b = hash(i + vec2(1.0, 0.0));
  let c = hash(i + vec2(0.0, 1.0));
  let d = hash(i + vec2(1.0, 1.0));
  return mix(mix(a, b, f.x), mix(c, d, f.x), f.y) * 2.0 - 1.0;
}
fn hsv2rgb(c: vec3<f32>) -> vec3<f32> {
  let t = c.x * 6.0 + vec3(0.0, 4.0, 2.0);
  let m = t - 6.0 * floor(t / 6.0);
  let rgb = clamp(abs(m - 3.0) - 1.0, vec3(0.0), vec3(1.0));
  return c.z * mix(vec3(1.0), rgb, vec3(c.y));
}
fn screen(a: vec3<f32>, b: vec3<f32>) -> vec3<f32> {
  return vec3(1.0) - (vec3(1.0) - a) * (vec3(1.0) - b);
}
fn bell(x: f32, c: f32, w: f32) -> f32 {
  let t = (x - c) / w;
  return exp(-t * t);
}
fn bellH(h: f32, c: f32, w: f32) -> f32 {
  var d = abs(h - c);
  d = min(d, 360.0 - d);
  return bell(d, 0.0, w);
}
fn lumOf(c: vec3<f32>) -> f32 {
  return dot(c, vec3(0.2126, 0.7152, 0.0722));
}
fn rgb2hsv(c: vec3<f32>) -> vec3<f32> {
  let K = vec4<f32>(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
  let p = mix(vec4<f32>(c.bg, K.wz), vec4<f32>(c.gb, K.xy), vec4<f32>(step(c.b, c.g)));
  let q = mix(vec4<f32>(p.xyw, c.r), vec4<f32>(c.r, p.yzx), vec4<f32>(step(p.x, c.r)));
  let d = q.x - min(q.w, q.y);
  let e = 1.0e-10;
  return vec3<f32>(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
}
fn hueSkew(col_in: vec3<f32>, amt: f32) -> vec3<f32> {
  var hsv = rgb2hsv(col_in);
  let h = hsv.x * 360.0;
  let l = lumOf(col_in);
  let hi = smoothstep(0.45, 0.85, l);
  let mid = smoothstep(0.30, 0.70, l);
  let lo = 1.0 - smoothstep(0.10, 0.45, l);
  var dh = 0.0;
  dh += bellH(h, 190.0, 30.0) * hi * 18.0;
  dh += bellH(h, 120.0, 35.0) * (0.4 * hi + 0.6 * mid) * -25.0;
  dh += bellH(h, 8.0, 22.0) * hi * 15.0;
  dh += bellH(h, 310.0, 30.0) * lo * 20.0;
  dh += bellH(h, 235.0, 25.0) * lo * -15.0;
  hsv.x = fract(hsv.x + dh * amt / 360.0 + 1.0);
  return hsv2rgb(hsv);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
  let wv = vec2(P.weave_px_x, P.weave_px_y) * vec2(P.texel_x, P.texel_y);
  var uv = in.uv + wv;
  // EL CUENTAHÍLOS: la misma imagen ampliada alrededor de su centro
  if (P.lupa > 0.01) {
    let lc = vec2<f32>(P.lupa_cx, P.lupa_cy);
    uv = (uv - lc) / P.lupa + lc;
  }
  if P.weave_rot > 0.001 {
    let ang = (sin(P.time * 1.9) + 0.5 * sin(P.time * 3.7 + 1.1)) * 0.0006 * P.weave_rot * (length(vec2(P.weave_px_x, P.weave_px_y)) + 0.5);
    let c = uv - vec2(0.5);
    uv = vec2(c.x * cos(ang) - c.y * sin(ang), c.x * sin(ang) + c.y * cos(ang)) + vec2(0.5);
  }

  // base con aberración cromática radial
  let caOff = (uv - vec2(0.5)) * P.ca * 0.018;
  var col = vec3(
    textureSampleLevel(tBase, samp, uv + caOff, 0.0).r,
    textureSampleLevel(tBase, samp, uv, 0.0).g,
    textureSampleLevel(tBase, samp, uv - caOff, 0.0).b);

  // ── FILM COLOR STAGE ──
  if P.crosstalk > 0.001 {
    let l = lumOf(col);
    col.r += P.crosstalk * (0.06 * col.g + 0.04 * col.b) * (0.4 + 0.6 * l);
    col.g += P.crosstalk * 0.04 * col.r;
    col.b += P.crosstalk * 0.03 * col.g;
  }
  if P.hue_skew > 0.001 { col = hueSkew(col, P.hue_skew); }
  if P.subtractive > 0.001 {
    let l0 = lumOf(col);
    var ch0 = col - vec3(l0);
    let chMag = length(ch0);
    var satW = 0.85 + 0.35 * bell(l0, 0.45, 0.30);
    satW *= 1.0 - 0.55 * smoothstep(0.70, 0.95, l0);
    satW *= 1.0 - 0.35 * smoothstep(0.15, 0.45, chMag) * smoothstep(0.65, 0.9, l0);
    let ch1 = ch0 * mix(1.0, satW * P.stock_sat, P.subtractive);
    let darken = 1.0 - P.subtractive * 0.5 * max(length(ch1) - chMag, 0.0);
    col = (vec3(l0) + ch1) * darken;
  }
  if P.print > 0.001 {
    var sc = col * col * (3.0 - 2.0 * col);
    sc = mix(col, sc, 0.85);
    let l = lumOf(sc);
    sc += vec3(0.010, 0.016, 0.020) * (1.0 - smoothstep(0.0, 0.35, l)) * 1.2;
    sc += vec3(0.030, 0.018, 0.006) * smoothstep(0.6, 0.95, l);
    let ch = sc - vec3(l);
    sc = sc - ch + ch / (vec3(1.0) + 1.5 * length(ch));
    sc = mix(sc, vec3(0.012, 0.016, 0.020), 0.06);
    col = mix(col, clamp(sc, vec3(0.0), vec3(1.0)), P.print);
  }

  if P.acutance > 0.001 {
    let hf = textureSampleLevel(tBase, samp, uv, 0.0).rgb - textureSampleLevel(tBlurB, samp, uv, 0.0).rgb;
    col += hf * P.acutance * 0.6;
  }
  if P.softness > 0.001 { col = mix(col, textureSampleLevel(tBlurC, samp, uv, 0.0).rgb, P.softness * 0.55); }

  // halation: naranja cerca, rojo lejos
  if P.hal_amount > 0.001 {
    let inner = textureSampleLevel(tBlurC, samp, uv, 0.0).rgb;
    let outer = textureSampleLevel(tBlurD, samp, uv, 0.0).rgb;
    let mI = smoothstep(P.hal_thr - 0.18, P.hal_thr + 0.18, lumOf(inner));
    let mO = smoothstep(P.hal_thr - 0.18, P.hal_thr + 0.18, lumOf(outer));
    var tintI = hsv2rgb(vec3(0.055 + P.hal_hue * 0.05, P.hal_sat, 1.0));
    var tintO = hsv2rgb(vec3(P.hal_hue * 0.045, min(P.hal_sat * 1.1, 1.0), 1.0));
    tintI = mix(tintI, vec3(1.0), P.hal_white);
    tintO = mix(tintO, vec3(1.0), P.hal_white);
    let sp = clamp(P.hal_spread, 0.0, 1.0);
    let hal = inner * tintI * mI * (1.0 - sp * 0.6) + outer * tintO * mO * sp;
    col = screen(col, hal * P.hal_amount * 0.85);
  }

  // bloom
  if P.bloom_amount > 0.001 {
    let b = mix(textureSampleLevel(tBlurB, samp, uv, 0.0).rgb, textureSampleLevel(tBlurC, samp, uv, 0.0).rgb, 0.5);
    let m = smoothstep(P.bloom_thr - 0.12, P.bloom_thr + 0.12, lumOf(b));
    let tintB = mix(vec3(1.0), hsv2rgb(vec3(0.07, 1.0, 1.0)), clamp(P.bloom_warm, 0.0, 1.0));
    col = screen(col, b * m * P.bloom_amount * 0.45 * tintB);
  }

  // flicker (rápido) + breath (lento, con deriva CMY)
  if P.flicker > 0.001 || P.breath > 0.001 {
    let fast = hash(vec2(floor(P.time * (4.0 + P.flicker_rate * 20.0)), 7.0)) - 0.5;
    let slowT = P.time * (0.4 + P.breath_rate * 1.6);
    let s0 = floor(slowT);
    var f0 = fract(slowT);
    f0 = f0 * f0 * (3.0 - 2.0 * f0);
    let slow = mix(hash(vec2(s0, 3.7)), hash(vec2(s0 + 1.0, 3.7)), f0) - 0.5;
    col *= 1.0 + fast * P.flicker * 0.10 + slow * P.breath * 0.07;
    let cshake = vec3(hash(vec2(s0, 11.1)), hash(vec2(s0, 13.7)), hash(vec2(s0, 17.3))) - 0.5;
    col *= 1.0 + P.breath * 0.05 * cshake;
  }

  // vignette cos⁴
  if P.vig_amount > 0.001 {
    let q = (in.uv - vec2(P.vig_cx, P.vig_cy)) * vec2(1.25, 1.0) / max(P.vig_size, 0.05);
    let dCirc = length(q);
    let dRect = max(abs(q.x), abs(q.y)) * 1.12;
    let d = mix(dRect, dCirc, clamp(P.vig_round, 0.0, 1.0));
    let theta = clamp(d * 1.35, 0.0, 1.45);
    let fall = pow(cos(theta), 4.0);
    col *= mix(1.0, fall, clamp(P.vig_amount, 0.0, 1.0));
  }

  // GRAIN: plate + capa crisp + asimetría negativo/print
  if P.grain_amount > 0.001 {
    let lum = lumOf(col);
    let gp = (in.pos.xy + vec2(P.weave_px_x, P.weave_px_y)) / P.plate_n;
    let cell = max(P.grain_size, 0.4);
    let seedV = vec2(hash(vec2(P.seed, 1.31)), hash(vec2(P.seed, 7.77)));
    let lod = P.grain_defocus * 2.5;
    let s1 = 1.0 / cell;
    let s2 = 3.5 / cell;
    let nClump = textureSampleLevel(tGrain, samp_rep, gp * s1 + seedV, lod).r * 2.0 - 1.0;
    let iq = (vec2<i32>(floor((in.pos.xy + vec2(P.weave_px_x, P.weave_px_y)) / max(cell * 0.5, 0.75)))
              + vec2<i32>(seedV * 1024.0)) & vec2<i32>(1023, 1023);
    var crisp = textureLoad(tGrain, iq, 0).r * 2.0 - 1.0;
    crisp = sign(crisp) * pow(abs(crisp), 0.65);
    let nFine = textureSampleLevel(tGrain, samp_rep, gp * s2 + seedV * 1.31 + 0.5, lod).r * 2.0 - 1.0;
    let nNeg = mix(nClump, mix(nFine, crisp, 0.75), clamp(P.grain_rough, 0.0, 1.0));
    let nPrint = nClump * 0.8;
    let nRGB = vec3(
      textureSampleLevel(tGrain, samp_rep, gp * s1 + seedV + vec2(0.31, 0.73), lod).r,
      textureSampleLevel(tGrain, samp_rep, gp * s1 + seedV + vec2(0.57, 0.11), lod).r,
      textureSampleLevel(tGrain, samp_rep, gp * s1 + seedV + vec2(0.83, 0.47), lod).r) * 2.0 - 1.0;
    var gNeg = mix(vec3(nNeg), nRGB, clamp(P.grain_chroma, 0.0, 1.0));
    var gPrint = mix(vec3(nPrint), nRGB * 0.7, clamp(P.grain_chroma, 0.0, 1.0));
    gNeg *= vec3(P.grain_r, 1.0, P.grain_b);
    gPrint *= vec3(P.grain_r, 1.0, P.grain_b);
    let wS = P.grain_s * bell(lum, 0.12, 0.20);
    let wM = P.grain_m * bell(lum, 0.42, 0.30);
    let wH = P.grain_h * bell(lum, 0.85, 0.24);
    let norm = inverseSqrt(max(cell * 0.5, 1.0));
    col += (gPrint * wS + gNeg * (wM + wH)) * P.grain_amount * 0.30 * norm;
    col += wS * P.grain_amount * 0.012;
    col = mix(col, textureSampleLevel(tBlurB, samp, uv, 0.0).rgb, P.grain_amount * P.film_res * 0.18);
  }

  // dust & scratches
  if P.dust > 0.001 {
    let epoch = floor(P.time * 2.0);
    for (var i = 0; i < 6; i++) {
      let fi = f32(i);
      let born = hash(vec2(fi, epoch));
      if born < P.dust * 0.45 {
        let pos = vec2(hash(vec2(fi, epoch + 1.3)), hash(vec2(fi, epoch + 2.9)));
        let r = 1.0 + 2.5 * hash(vec2(fi, epoch + 4.1));
        let d = length((in.uv - pos) * vec2(1.78, 1.0) * 960.0);
        let spot = 1.0 - smoothstep(r * 0.4, r, d);
        let dark = step(0.5, hash(vec2(fi, epoch + 5.7)));
        col = mix(col, vec3(0.02), spot * 0.5 * dark);
        col = mix(col, vec3(0.9), spot * 0.25 * (1.0 - dark));
      }
    }
    for (var i = 0; i < 3; i++) {
      let fi = f32(i) + 31.0;
      let born = hash(vec2(fi, epoch * 0.5));
      if born < P.dust * 0.22 {
        let x = hash(vec2(fi, epoch * 0.5 + 1.1));
        let wdt = 0.4 + hash(vec2(fi, 3.3)) * 0.8;
        let line = 1.0 - smoothstep(wdt * 0.5, wdt, abs(in.uv.x - x) * 1920.0);
        let jitter = 0.85 + 0.15 * sin(in.uv.y * 40.0 + fi);
        col *= 1.0 - line * 0.10 * jitter;
      }
    }
  }

  // frame / film gate
  if P.frame_inset > 0.5 {
    let res = vec2(P.res_w, P.res_h);
    let px = in.uv * res;
    let half_ = res * 0.5;
    let ang = atan2(px.y - half_.y, px.x - half_.x);
    let wob = (vnoise(vec2(ang * 2.5 + 7.0, 3.0)) + 0.5 * vnoise(vec2(ang * 6.0, 9.0))) * P.frame_wobble * 5.0;
    let b = half_ - P.frame_inset;
    let q = abs(px - half_) - b + P.frame_corner;
    let d = length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - P.frame_corner + wob;
    let m = smoothstep(0.0, 1.5, d);
    col = mix(col, vec3(0.010, 0.008, 0.006), m);
  }

  if in.uv.x > P.wipe { col = textureSampleLevel(tRaw, samp, in.uv, 0.0).rgb; }

  // EL FUNDIDO, lo último: sobre la copia ya revelada, con su grano y su
  // halación dentro. Cero coste, cero búferes (MOTOR §5bis).
  if (P.fundido > 0.0001) { col = mix(col, vec3(P.fundido_color), P.fundido); }

  // ── EL TRAMADO, LO ÚLTIMO DE TODO ───────────────────────────────────
  // La gelatina de entrada estira las sombras muchísimo (log → 709), y al
  // escribir a 8 bits eso se ve como escalones en cielos y paredes lisas. Un
  // grano de ±½ escalón repartido en TRIÁNGULO —no uniforme— convierte el
  // escalón en un ruido que el ojo no separa, que es lo que hace cualquier
  // cadena seria antes de cuantizar. A 10 bits no estorba: media escalón de
  // 10 bits es invisible.
  //
  // Triangular = suma de dos uniformes independientes. Con uno solo el ruido
  // queda correlacionado con la señal y se oye —se ve— la modulación.
  // `hash` y no `vnoise`: el segundo es ruido de valor INTERPOLADO y como
  // tramado se vería como un patrón blando en vez de como grano fino. Aquí
  // hace falta blanco, píxel a píxel.
  let pix = in.uv * vec2(P.res_w, P.res_h);
  let d1 = hash(pix + vec2(P.seed, 11.0));
  let d2 = hash(pix + vec2(37.0, P.seed + 5.0));
  // TODO CON EL TIPO PUESTO: `vec3<f32>(…)`, no `vec3(…)`.
  //
  // WGSL no difunde un escalar sobre un vector en una suma (GLSL sí), pero
  // además —y esto es lo que costó el viaje— un `vec3(expr)` sin tipo puede
  // quedarse en el tipo ABSTRACTO de los literales según la versión de naga.
  // El validador del Mac lo acepta y el que corre wgpu en Windows dice que el
  // valor devuelto no casa con el tipo de la función. Se escribe explícito y
  // se acabó la discusión.
  let tram = vec3<f32>((d1 + d2 - 1.0) * (0.5 / 255.0));
  return vec4<f32>(clamp(col + tram, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
