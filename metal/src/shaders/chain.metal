// chain.metal — toda la cadena: grade (YUV→RGB→LUTs), down, blur, accum, comp.

#include <metal_stdlib>
using namespace metal;

struct VsOut { float4 pos [[position]]; float2 uv; };

vertex VsOut vs_full(uint vid [[vertex_id]]) {
  float2 p = float2(vid == 1 ? 3.0 : -1.0, vid == 2 ? 3.0 : -1.0);
  VsOut o;
  o.pos = float4(p, 0.0, 1.0);
  // uv.y invertida: NDC +y es ARRIBA pero la fila 0 de la textura es ARRIBA →
  // sin esto cada pass hace un flip vertical y las orientaciones se mezclan
  o.uv = float2(p.x, -p.y) * 0.5 + 0.5;
  return o;
}

// ── GRADE ─────────────────────────────────────────────────────────────────
struct GradeParams {
  uint src_mode, full_range, lut_na, lut_nb;
  uint lut_a_on, lut_b_on;
  float yuv_norm, gain, push_pull, compress, compress_wp, compress_range;
  float src_w, src_h, pad0, pad1;
  // EL ENCUADRE (MOTOR §5, PENDIENTE §1.5): una afín 2×3 y nada más.
  // `enc_a = [m00 m01 m10 m11]` y `enc_b.xy = [m02 m12]` llevan de uv-lienzo a
  // uv-fuente con el conform, los cuartos de vuelta, la escala de cada eje, la
  // posición, el giro sobre el ancla y el volteo ya compuestos en la CPU.
  // `enc_b.zw` son cuántas muestras hay que tomar por eje y `paso` su
  // separación: el filtro de reducción. Lo que caiga fuera de la fuente sale
  // negro: eso es el letterbox, y no cuesta ni un filtro.
  float4 enc_a, enc_b, paso;
  /// cuánto pesa este pase en el destino: 1 = escribe, <1 = se mezcla con lo
  /// que ya hay. Un encadenado es esto y nada más (MOTOR §5bis).
  // el peso del pase, la matriz YUV→RGB (0=709, 1=2020, 2=601) y dónde va
  // sentado el croma. El reparto de bytes es EL MISMO que el del WGSL: los
  // dos leen el mismo búfer y divergir aquí es leer basura.
  float peso;
  uint matriz;
  float croma_x, croma_y;
  // el filtro ND: fuerza, tinte plano, perfil de sombras, guarda de gris
  float4 nd;
};

/// uv del LIENZO → uv de la FUENTE. Devuelve false si cae fuera (letterbox).
static inline bool encuadra(constant GradeParams& P, float2 uv, thread float2& fuera) {
  fuera = float2(P.enc_a.x * uv.x + P.enc_a.y * uv.y + P.enc_b.x,
                 P.enc_a.z * uv.x + P.enc_a.w * uv.y + P.enc_b.y);
  return all(fuera >= 0.0) && all(fuera <= 1.0);
}

static inline float3 tap_src(constant GradeParams& P, texture2d<float> tY,
                             texture2d<float> tUV, texture2d<float> tVideo,
                             sampler samp, float2 uv) {
  if (P.src_mode == 1) return tVideo.sample(samp, uv).rgb;
  float y = tY.sample(samp, uv).r;
  float2 c = tUV.sample(samp, uv).rg;
  float Y, U, V;
  if (P.full_range == 1) { Y = y; U = c.r - 0.5; V = c.g - 0.5; }
  else {
    Y = (y * 1023.0 - 64.0) / 876.0;
    U = (c.r * 1023.0 - 512.0) / 896.0;
    V = (c.g * 1023.0 - 512.0) / 896.0;
  }
  Y = clamp(Y, 0.0, 1.0);
  return float3(Y + 1.5748 * V, Y - 0.1873 * U - 0.4681 * V, Y + 1.8556 * U);
}

/// EL FILTRO DE REDUCCIÓN: al agrandar basta el bilineal, pero al reducir
/// —y conformar 4K a 1080 ya es reducir a la mitad— un solo tap se salta
/// píxeles y aparece el hormigueo. Cuántas muestras hacen falta lo dice la
/// CPU, que conoce la matriz exacta.
float3 sample_src(constant GradeParams& P, texture2d<float> tY, texture2d<float> tUV,
                  texture2d<float> tVideo, sampler samp, float2 uv) {
  int nx = int(P.enc_b.z);
  int ny = int(P.enc_b.w);
  if (nx <= 1 && ny <= 1) return tap_src(P, tY, tUV, tVideo, samp, uv);
  float2 px = P.paso.xy;
  float2 py = P.paso.zw;
  float3 acc = float3(0.0);
  float n = 0.0;
  // el tope, constante — igual que en el WGSL, para que las dos cadenas se
  // lean iguales (aquí Metal no lo necesita, pero divergir se paga)
  for (int i = 0; i < 4; ++i) {
    if (i >= nx) continue;
    float ox = (float(i) + 0.5) / float(nx) - 0.5;
    for (int j = 0; j < 4; ++j) {
      if (j >= ny) continue;
      float oy = (float(j) + 0.5) / float(ny) - 0.5;
      acc += tap_src(P, tY, tUV, tVideo, samp, uv + px * ox + py * oy);
      n += 1.0;
    }
  }
  return acc / max(n, 1.0);
}

float3 lut3(texture3d<float> t, uint n, float3 c_in, sampler samp_n) {
  float3 p = clamp(c_in, 0.0, 1.0) * float(n - 1);
  float3 b = floor(p);
  float3 f = p - b;
  uint3 i0 = uint3(b);
  uint3 mx = uint3(n - 1);
  float3 c000 = t.read(min(i0, mx)).rgb;
  float3 c100 = t.read(min(i0 + uint3(1,0,0), mx)).rgb;
  float3 c010 = t.read(min(i0 + uint3(0,1,0), mx)).rgb;
  float3 c110 = t.read(min(i0 + uint3(1,1,0), mx)).rgb;
  float3 c001 = t.read(min(i0 + uint3(0,0,1), mx)).rgb;
  float3 c101 = t.read(min(i0 + uint3(1,0,1), mx)).rgb;
  float3 c011 = t.read(min(i0 + uint3(0,1,1), mx)).rgb;
  float3 c111 = t.read(min(i0 + uint3(1,1,1), mx)).rgb;
  return mix(mix(mix(c000, c100, f.x), mix(c010, c110, f.x), f.y),
             mix(mix(c001, c101, f.x), mix(c011, c111, f.x), f.y), f.z);
}

struct GradeOut { float4 graded [[color(0)]]; float4 raw [[color(1)]]; };

/// el revelado propiamente dicho, sin decidir a cuántos sitios se escribe
static inline void revela(VsOut in, constant GradeParams& P,
  texture2d<float> tY, texture2d<float> tUV, texture2d<float> tVideo,
  texture3d<float> tLutA, texture3d<float> tLutB,
  sampler samp, sampler samp_n, thread float3& raw, thread float3& graded);

/// EL MÁSTER NO NECESITA `raw`. Ese segundo destino es solo para el
/// comparador de la preview (la cortinilla `wipe`): en el revelado wipe=1 y
/// nadie lo lee jamás — pero se escribía un búfer de resolución completa por
/// fotograma. En 4K son 66 MB tirados por fotograma, casi 6 GB/s a 90 fps.
fragment float4 fs_grade_solo(VsOut in [[stage_in]],
  constant GradeParams& P [[buffer(0)]],
  texture2d<float> tY [[texture(0)]],
  texture2d<float> tUV [[texture(1)]],
  texture2d<float> tVideo [[texture(2)]],
  texture3d<float> tLutA [[texture(3)]],
  texture3d<float> tLutB [[texture(4)]],
  texture2d<float> tHist [[texture(5)]],
  sampler samp [[sampler(0)]],
  sampler samp_n [[sampler(1)]]) {
  float3 raw, graded;
  revela(in, P, tY, tUV, tVideo, tLutA, tLutB, samp, samp_n, raw, graded);
  // EL OBTURADOR, FUNDIDO AQUÍ (MOTOR §3). Era un pase entero aparte que
  // leía `graded` y escribía la historia: en 4K, otros 66 MB de ida y vuelta
  // por fotograma para una sola línea de cuenta. Windows ya lo hacía así;
  // ahora los dos motores hacen lo mismo.
  //
  // Y funciona con las juntas sin más cuidado porque el filtro es LINEAL:
  // mezclar dos imágenes ya arrastradas es lo mismo que arrastrar la mezcla.
  //   mix(mix(A,h,k), mix(B,h,k), p) == mix(mix(A,B,p), h, k)
  if (P.pad0 > 0.001 && P.pad1 < 0.5) {
    graded = mix(graded, tHist.sample(samp, in.uv).rgb, P.pad0);
  }
  // el alfa ES el peso: con mezcla src-alpha activada, escribir con peso 1
  // sustituye y con peso p encadena. No hay «pase de fundido».
  return float4(graded, P.peso);
}

fragment GradeOut fs_grade(VsOut in [[stage_in]],
  constant GradeParams& P [[buffer(0)]],
  texture2d<float> tY [[texture(0)]],
  texture2d<float> tUV [[texture(1)]],
  texture2d<float> tVideo [[texture(2)]],
  texture3d<float> tLutA [[texture(3)]],
  texture3d<float> tLutB [[texture(4)]],
  sampler samp [[sampler(0)]],
  sampler samp_n [[sampler(1)]]) {
  float3 raw, graded;
  revela(in, P, tY, tUV, tVideo, tLutA, tLutB, samp, samp_n, raw, graded);
  GradeOut o;
  o.raw = float4(raw, 1.0);
  o.graded = float4(graded, 1.0);
  return o;
}

static inline void revela(VsOut in, constant GradeParams& P,
  texture2d<float> tY, texture2d<float> tUV, texture2d<float> tVideo,
  texture3d<float> tLutA, texture3d<float> tLutB,
  sampler samp, sampler samp_n, thread float3& raw, thread float3& graded) {
  float2 uv;   // Metal/CVPixelBuffer: top-down, sin flip
  if (!encuadra(P, in.uv, uv)) { raw = float3(0.0); graded = float3(0.0); return; }
  raw = clamp(sample_src(P, tY, tUV, tVideo, samp, uv), 0.0, 1.0);
  raw = clamp(raw * exp2(P.gain), 0.0, 1.0);
  float pp = P.push_pull;
  if (abs(pp) > 0.001) {
    raw = clamp(raw * exp2(pp * 0.7), 0.0, 1.0);
    raw = pow(raw, float3(1.0 + pp * 0.10));
    if (pp > 0.0) raw = mix(raw, float3(1.0), 0.04 * pp);
    else raw = pow(raw, float3(1.0 - pp * 0.06));
  }
  if (P.compress > 0.001) {
    float thr = 1.0 - P.compress_range;
    float3 over = max(raw - thr, 0.0);
    raw = raw - over + over / (1.0 + P.compress * over / max(P.compress_wp, 0.05));
  }
  graded = raw;
  if (P.lut_a_on == 1) graded = clamp(lut3(tLutA, P.lut_na, graded, samp_n), 0.0, 1.0);
  if (P.lut_b_on == 1) graded = clamp(lut3(tLutB, P.lut_nb, graded, samp_n), 0.0, 1.0);
}

// ── DOWN ──────────────────────────────────────────────────────────────────
struct DownParams { float2 texel, pad; };

fragment float4 fs_down(VsOut in [[stage_in]],
  constant DownParams& P [[buffer(0)]],
  texture2d<float> tIn [[texture(0)]],
  sampler samp [[sampler(0)]]) {
  float3 c = tIn.sample(samp, in.uv).rgb * 4.0;
  c += tIn.sample(samp, in.uv + P.texel * float2(-1, -1)).rgb;
  c += tIn.sample(samp, in.uv + P.texel * float2(1, -1)).rgb;
  c += tIn.sample(samp, in.uv + P.texel * float2(-1, 1)).rgb;
  c += tIn.sample(samp, in.uv + P.texel * float2(1, 1)).rgb;
  return float4(c / 8.0, 1.0);
}

// ── BLUR ──────────────────────────────────────────────────────────────────
struct BlurParams { float2 dir; float radius, pad; };

fragment float4 fs_blur(VsOut in [[stage_in]],
  constant BlurParams& P [[buffer(0)]],
  texture2d<float> tIn [[texture(0)]],
  sampler samp [[sampler(0)]]) {
  const float w[5] = {0.227027, 0.1945946, 0.1216216, 0.054054, 0.016216};
  float3 c = tIn.sample(samp, in.uv).rgb * w[0];
  for (int i = 1; i < 5; i++) {
    float2 off = P.dir * float(i) * P.radius;
    c += tIn.sample(samp, in.uv + off).rgb * w[i];
    c += tIn.sample(samp, in.uv - off).rgb * w[i];
  }
  return float4(c, 1.0);
}

// ── ACCUM ─────────────────────────────────────────────────────────────────
struct AccumParams { float feedback; uint reset; float2 pad; };

fragment float4 fs_accum(VsOut in [[stage_in]],
  constant AccumParams& P [[buffer(0)]],
  texture2d<float> tCurr [[texture(0)]],
  texture2d<float> tPrev [[texture(1)]],
  sampler samp [[sampler(0)]]) {
  float3 c = tCurr.sample(samp, in.uv).rgb;
  float3 p = tPrev.sample(samp, in.uv).rgb;
  if (P.reset == 1) return float4(c, 1.0);
  return float4(mix(c, p, P.feedback), 1.0);
}

// ── COMP ──────────────────────────────────────────────────────────────────
struct CompParams {
  float time, seed, wipe, res_w, res_h, texel_x, texel_y;
  float weave_px_x, weave_px_y, weave_rot;
  float hal_amount, hal_hue, hal_sat, hal_thr, hal_spread, hal_white;
  float bloom_amount, bloom_thr, bloom_warm;
  float softness, acutance, color_sep;
  float hue_skew, crosstalk, subtractive, stock_sat, print_;
  float grain_amount, grain_size, grain_rough, grain_chroma, grain_defocus;
  float grain_s, grain_m, grain_h, grain_r, grain_b, film_res, plate_n;
  float vig_amount, vig_size, vig_round, vig_cx, vig_cy, ca;
  float dust, flicker, flicker_rate, breath, breath_rate;
  float frame_inset, frame_corner, frame_wobble;
  float lupa, lupa_cx, lupa_cy;          // hueco de la preview: NO quitar
  float fundido, fundido_color, pad_f, pad_g;
};

float hash(float2 p) {
  float2 q = fract(p * float2(123.34, 456.21));
  q += dot(q, q + 45.32);
  return fract(q.x * q.y);
}
float gnoise(float2 p) {
  return (hash(p) + hash(p + 17.7) + hash(p + 31.3) + hash(p + 47.9)) * 0.5 - 1.0;
}
float vnoise(float2 p) {
  float2 i = floor(p), f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  float a = hash(i), b = hash(i + float2(1, 0)), c = hash(i + float2(0, 1)), d = hash(i + float2(1, 1));
  return mix(mix(a, b, f.x), mix(c, d, f.x), f.y) * 2.0 - 1.0;
}
float3 hsv2rgb(float3 c) {
  float3 t = c.x * 6.0 + float3(0.0, 4.0, 2.0);
  float3 m = t - 6.0 * floor(t / 6.0);
  float3 rgb = clamp(abs(m - 3.0) - 1.0, 0.0, 1.0);
  return c.z * mix(float3(1.0), rgb, c.y);
}
float3 screen(float3 a, float3 b) { return 1.0 - (1.0 - a) * (1.0 - b); }
float bell(float x, float c, float w) { float t = (x - c) / w; return exp(-t * t); }
float bellH(float h, float c, float w) {
  float d = abs(h - c);
  d = min(d, 360.0 - d);
  return bell(d, 0.0, w);
}
float lumOf(float3 c) { return dot(c, float3(0.2126, 0.7152, 0.0722)); }

float3 rgb2hsv(float3 c) {
  float4 K = float4(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
  float4 p = c.b < c.g ? float4(c.gb, K.xy) : float4(c.bg, K.wz);
  float4 q = p.x < c.r ? float4(c.r, p.yzx) : float4(p.xyw, c.r);
  float d = q.x - min(q.w, q.y);
  float e = 1.0e-10;
  return float3(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
}

float3 hueSkew(float3 col_in, float amt) {
  float3 hsv = rgb2hsv(col_in);
  float h = hsv.x * 360.0;
  float l = lumOf(col_in);
  float hi = smoothstep(0.45, 0.85, l);
  float mid = smoothstep(0.30, 0.70, l);
  float lo = 1.0 - smoothstep(0.10, 0.45, l);
  float dh = 0.0;
  dh += bellH(h, 190.0, 30.0) * hi * 18.0;
  dh += bellH(h, 120.0, 35.0) * (0.4 * hi + 0.6 * mid) * -25.0;
  dh += bellH(h, 8.0, 22.0) * hi * 15.0;
  dh += bellH(h, 310.0, 30.0) * lo * 20.0;
  dh += bellH(h, 235.0, 25.0) * lo * -15.0;
  hsv.x = fract(hsv.x + dh * amt / 360.0 + 1.0);
  return hsv2rgb(hsv);
}

fragment float4 fs_comp(VsOut in [[stage_in]],
  constant CompParams& P [[buffer(0)]],
  texture2d<float> tBase [[texture(0)]],
  texture2d<float> tRaw [[texture(1)]],
  texture2d<float> tBlurB [[texture(2)]],
  texture2d<float> tBlurC [[texture(3)]],
  texture2d<float> tBlurD [[texture(4)]],
  texture2d<float> tGrain [[texture(5)]],
  sampler samp [[sampler(0)]],
  sampler samp_rep [[sampler(1)]]) {
  float2 wv = float2(P.weave_px_x, P.weave_px_y) * float2(P.texel_x, P.texel_y);
  float2 uv = in.uv + wv;
  if (P.weave_rot > 0.001) {
    float ang = (sin(P.time * 1.9) + 0.5 * sin(P.time * 3.7 + 1.1)) * 0.0006 * P.weave_rot * (length(float2(P.weave_px_x, P.weave_px_y)) + 0.5);
    float2 c = uv - 0.5;
    uv = float2(c.x * cos(ang) - c.y * sin(ang), c.x * sin(ang) + c.y * cos(ang)) + 0.5;
  }

  float2 caOff = (uv - 0.5) * P.ca * 0.018;
  float3 col = float3(
    tBase.sample(samp, uv + caOff).r,
    tBase.sample(samp, uv).g,
    tBase.sample(samp, uv - caOff).b);

  if (P.crosstalk > 0.001) {
    float l = lumOf(col);
    col.r += P.crosstalk * (0.06 * col.g + 0.04 * col.b) * (0.4 + 0.6 * l);
    col.g += P.crosstalk * 0.04 * col.r;
    col.b += P.crosstalk * 0.03 * col.g;
  }
  if (P.hue_skew > 0.001) col = hueSkew(col, P.hue_skew);
  if (P.subtractive > 0.001) {
    float l0 = lumOf(col);
    float3 ch0 = col - l0;
    float chMag = length(ch0);
    float satW = 0.85 + 0.35 * bell(l0, 0.45, 0.30);
    satW *= 1.0 - 0.55 * smoothstep(0.70, 0.95, l0);
    satW *= 1.0 - 0.35 * smoothstep(0.15, 0.45, chMag) * smoothstep(0.65, 0.9, l0);
    float3 ch1 = ch0 * mix(1.0, satW * P.stock_sat, P.subtractive);
    float darken = 1.0 - P.subtractive * 0.5 * max(length(ch1) - chMag, 0.0);
    col = (l0 + ch1) * darken;
  }
  if (P.print_ > 0.001) {
    float3 sc = col * col * (3.0 - 2.0 * col);
    sc = mix(col, sc, 0.85);
    float l = lumOf(sc);
    sc += float3(0.010, 0.016, 0.020) * (1.0 - smoothstep(0.0, 0.35, l)) * 1.2;
    sc += float3(0.030, 0.018, 0.006) * smoothstep(0.6, 0.95, l);
    float3 ch = sc - l;
    sc = sc - ch + ch / (1.0 + 1.5 * length(ch));
    sc = mix(sc, float3(0.012, 0.016, 0.020), 0.06);
    col = mix(col, clamp(sc, 0.0, 1.0), P.print_);
  }

  if (P.acutance > 0.001) {
    float3 hf = tBase.sample(samp, uv).rgb - tBlurB.sample(samp, uv).rgb;
    col += hf * P.acutance * 0.6;
  }
  if (P.softness > 0.001) col = mix(col, tBlurC.sample(samp, uv).rgb, P.softness * 0.55);

  if (P.hal_amount > 0.001) {
    float3 inner = tBlurC.sample(samp, uv).rgb;
    float3 outer = tBlurD.sample(samp, uv).rgb;
    float mI = smoothstep(P.hal_thr - 0.18, P.hal_thr + 0.18, lumOf(inner));
    float mO = smoothstep(P.hal_thr - 0.18, P.hal_thr + 0.18, lumOf(outer));
    float3 tintI = hsv2rgb(float3(0.055 + P.hal_hue * 0.05, P.hal_sat, 1.0));
    float3 tintO = hsv2rgb(float3(P.hal_hue * 0.045, min(P.hal_sat * 1.1, 1.0), 1.0));
    tintI = mix(tintI, float3(1.0), P.hal_white);
    tintO = mix(tintO, float3(1.0), P.hal_white);
    float sp = clamp(P.hal_spread, 0.0, 1.0);
    float3 hal = inner * tintI * mI * (1.0 - sp * 0.6) + outer * tintO * mO * sp;
    col = screen(col, hal * P.hal_amount * 0.85);
  }

  if (P.bloom_amount > 0.001) {
    float3 b = mix(tBlurB.sample(samp, uv).rgb, tBlurC.sample(samp, uv).rgb, 0.5);
    float m = smoothstep(P.bloom_thr - 0.12, P.bloom_thr + 0.12, lumOf(b));
    float3 tintB = mix(float3(1.0), hsv2rgb(float3(0.07, 1.0, 1.0)), clamp(P.bloom_warm, 0.0, 1.0));
    col = screen(col, b * m * P.bloom_amount * 0.45 * tintB);
  }

  if (P.flicker > 0.001 || P.breath > 0.001) {
    float fast = hash(float2(floor(P.time * (4.0 + P.flicker_rate * 20.0)), 7.0)) - 0.5;
    float slowT = P.time * (0.4 + P.breath_rate * 1.6);
    float s0 = floor(slowT);
    float f0 = fract(slowT);
    f0 = f0 * f0 * (3.0 - 2.0 * f0);
    float slow = mix(hash(float2(s0, 3.7)), hash(float2(s0 + 1.0, 3.7)), f0) - 0.5;
    col *= 1.0 + fast * P.flicker * 0.10 + slow * P.breath * 0.07;
    float3 cshake = float3(hash(float2(s0, 11.1)), hash(float2(s0, 13.7)), hash(float2(s0, 17.3))) - 0.5;
    col *= 1.0 + P.breath * 0.05 * cshake;
  }

  if (P.vig_amount > 0.001) {
    float2 q = (in.uv - float2(P.vig_cx, P.vig_cy)) * float2(1.25, 1.0) / max(P.vig_size, 0.05);
    float dCirc = length(q);
    float dRect = max(abs(q.x), abs(q.y)) * 1.12;
    float d = mix(dRect, dCirc, clamp(P.vig_round, 0.0, 1.0));
    float theta = clamp(d * 1.35, 0.0, 1.45);
    float fall = pow(cos(theta), 4.0);
    col *= mix(1.0, fall, clamp(P.vig_amount, 0.0, 1.0));
  }

  if (P.grain_amount > 0.001) {
    float lum = lumOf(col);
    float2 gp = (in.pos.xy + float2(P.weave_px_x, P.weave_px_y)) / P.plate_n;
    float cell = max(P.grain_size, 0.4);
    float2 seedV = float2(hash(float2(P.seed, 1.31)), hash(float2(P.seed, 7.77)));
    float lod = P.grain_defocus * 2.5;
    float s1 = 1.0 / cell;
    float s2 = 3.5 / cell;
    float nClump = tGrain.sample(samp_rep, gp * s1 + seedV, level(lod)).r * 2.0 - 1.0;
    uint2 iq = (uint2(floor((in.pos.xy + float2(P.weave_px_x, P.weave_px_y)) / max(cell * 0.5, 0.75)))
                + uint2(seedV * 1024.0)) & 1023;
    float crisp = tGrain.read(iq).r * 2.0 - 1.0;
    crisp = sign(crisp) * pow(abs(crisp), 0.65);
    float nFine = tGrain.sample(samp_rep, gp * s2 + seedV * 1.31 + 0.5, level(lod)).r * 2.0 - 1.0;
    float nNeg = mix(nClump, mix(nFine, crisp, 0.75), clamp(P.grain_rough, 0.0, 1.0));
    float nPrint = nClump * 0.8;
    float3 nRGB = float3(
      tGrain.sample(samp_rep, gp * s1 + seedV + float2(0.31, 0.73), level(lod)).r,
      tGrain.sample(samp_rep, gp * s1 + seedV + float2(0.57, 0.11), level(lod)).r,
      tGrain.sample(samp_rep, gp * s1 + seedV + float2(0.83, 0.47), level(lod)).r) * 2.0 - 1.0;
    float3 gNeg = mix(float3(nNeg), nRGB, clamp(P.grain_chroma, 0.0, 1.0));
    float3 gPrint = mix(float3(nPrint), nRGB * 0.7, clamp(P.grain_chroma, 0.0, 1.0));
    gNeg *= float3(P.grain_r, 1.0, P.grain_b);
    gPrint *= float3(P.grain_r, 1.0, P.grain_b);
    float wS = P.grain_s * bell(lum, 0.12, 0.20);
    float wM = P.grain_m * bell(lum, 0.42, 0.30);
    float wH = P.grain_h * bell(lum, 0.85, 0.24);
    float norm = rsqrt(max(cell * 0.5, 1.0));
    col += (gPrint * wS + gNeg * (wM + wH)) * P.grain_amount * 0.30 * norm;
    col += wS * P.grain_amount * 0.012;
    col = mix(col, tBlurB.sample(samp, uv).rgb, P.grain_amount * P.film_res * 0.18);
  }

  if (P.dust > 0.001) {
    float epoch = floor(P.time * 2.0);
    for (int i = 0; i < 6; i++) {
      float fi = float(i);
      float born = hash(float2(fi, epoch));
      if (born < P.dust * 0.45) {
        float2 pos = float2(hash(float2(fi, epoch + 1.3)), hash(float2(fi, epoch + 2.9)));
        float r = 1.0 + 2.5 * hash(float2(fi, epoch + 4.1));
        float d = length((in.uv - pos) * float2(1.78, 1.0) * 960.0);
        float spot = 1.0 - smoothstep(r * 0.4, r, d);
        float dark = step(0.5, hash(float2(fi, epoch + 5.7)));
        col = mix(col, float3(0.02), spot * 0.5 * dark);
        col = mix(col, float3(0.9), spot * 0.25 * (1.0 - dark));
      }
    }
    for (int i = 0; i < 3; i++) {
      float fi = float(i) + 31.0;
      float born = hash(float2(fi, epoch * 0.5));
      if (born < P.dust * 0.22) {
        float x = hash(float2(fi, epoch * 0.5 + 1.1));
        float wdt = 0.4 + hash(float2(fi, 3.3)) * 0.8;
        float line = 1.0 - smoothstep(wdt * 0.5, wdt, abs(in.uv.x - x) * 1920.0);
        float jitter = 0.85 + 0.15 * sin(in.uv.y * 40.0 + fi);
        col *= 1.0 - line * 0.10 * jitter;
      }
    }
  }

  if (P.frame_inset > 0.5) {
    float2 res = float2(P.res_w, P.res_h);
    float2 px = in.uv * res;
    float2 half_ = res * 0.5;
    float ang = atan2(px.y - half_.y, px.x - half_.x);
    float wob = (vnoise(float2(ang * 2.5 + 7.0, 3.0)) + 0.5 * vnoise(float2(ang * 6.0, 9.0))) * P.frame_wobble * 5.0;
    float2 b = half_ - P.frame_inset;
    float2 q = abs(px - half_) - b + P.frame_corner;
    float d = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - P.frame_corner + wob;
    float m = smoothstep(0.0, 1.5, d);
    col = mix(col, float3(0.010, 0.008, 0.006), m);
  }

  if (in.uv.x > P.wipe) col = tRaw.sample(samp, in.uv).rgb;

  // EL FUNDIDO, lo último de todo: sobre la copia ya revelada, con su grano
  // y su halación dentro. Cero coste, cero búferes (MOTOR §5bis).
  if (P.fundido > 0.0001) col = mix(col, float3(P.fundido_color), P.fundido);

  return float4(clamp(col, 0.0, 1.0), 1.0);
}

// ── PACK RGB → YUV 4:2:0 10-bit x420 (BT.709 video range, MSB-aligned) ────
// x420 guarda los 10 bits en los bits ALTOS del u16: unorm16 v ↔ código10 = v*1023
fragment float4 fs_pack_y(VsOut in [[stage_in]],
  texture2d<float> tIn [[texture(0)]],
  sampler samp [[sampler(0)]]) {
  float3 c = tIn.sample(samp, in.uv).rgb;
  float y = (64.0 + 876.0 * dot(c, float3(0.2126, 0.7152, 0.0722))) / 1023.0;
  return float4(y, y, y, 1.0);
}

fragment float4 fs_pack_uv(VsOut in [[stage_in]],
  texture2d<float> tIn [[texture(0)]],
  sampler samp [[sampler(0)]]) {
  float3 c = tIn.sample(samp, in.uv).rgb;   // el target es ½ res: LINEAR promedia 2×2
  float u = (512.0 + 896.0 * dot(c, float3(-0.1146, -0.3854, 0.5))) / 1023.0;
  float v = (512.0 + 896.0 * dot(c, float3(0.5, -0.4542, -0.0458))) / 1023.0;
  return float4(u, v, 0.0, 1.0);  // plano 1 = CbCr: r=Cb, g=Cr
}
