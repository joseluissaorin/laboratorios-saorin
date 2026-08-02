// pipeline.js — WebGL2: fuente → Rec.709 → gain → LUT 3D → slow shutter
// (acumulador temporal) → blurs 1/2,1/4,1/8 → halation/bloom/softness →
// óptica (CA, cos⁴ vignette) → grain físico → dust/scratches → weave/flicker.

const VS = `#version 300 es
layout(location=0) in vec2 p;
out vec2 vUv;
void main(){ vUv = p*0.5+0.5; gl_Position = vec4(p,0.,1); }`;

const FS_GRADE = `#version 300 es
precision highp float; precision highp int; precision highp usampler2D; precision highp sampler3D;
uniform usampler2D uY, uU, uV;
uniform sampler2D uVideo;
uniform int uSrcMode;
uniform float uYNorm;
uniform int uFullRange;
uniform sampler3D uLutA; uniform int uLutNA; uniform int uLutAOn;
uniform sampler3D uLutB; uniform int uLutNB; uniform int uLutBOn;
uniform float uGain;
uniform float uPushPull;                      // -2..+2 stops (remap de respuesta)
uniform float uCompress, uCompressWP, uCompressRange;  // esponja de altas luces
uniform vec2 uSrcSize;
in vec2 vUv;
layout(location=0) out vec4 outGraded;
layout(location=1) out vec4 outRaw;

float bilin(usampler2D t, vec2 size, vec2 uv){
  vec2 pos = clamp(uv, 0.0, 1.0) * size - 0.5;
  vec2 fl = floor(pos); vec2 f = pos - fl;
  ivec2 i0 = ivec2(fl); ivec2 mx = ivec2(size) - 1;
  float c00 = float(texelFetch(t, clamp(i0,              ivec2(0), mx), 0).r);
  float c10 = float(texelFetch(t, clamp(i0 + ivec2(1,0), ivec2(0), mx), 0).r);
  float c01 = float(texelFetch(t, clamp(i0 + ivec2(0,1), ivec2(0), mx), 0).r);
  float c11 = float(texelFetch(t, clamp(i0 + ivec2(1,1), ivec2(0), mx), 0).r);
  return mix(mix(c00, c10, f.x), mix(c01, c11, f.x), f.y);
}

vec3 sampleSrc(vec2 uv){
  if (uSrcMode == 1) return texture(uVideo, uv).rgb;
  float y = bilin(uY, uSrcSize, uv) / uYNorm;
  vec2 cs = uSrcSize * 0.5;
  float u = bilin(uU, cs, uv) / uYNorm;
  float v = bilin(uV, cs, uv) / uYNorm;
  float Y, U, V;
  if (uFullRange == 1) { Y = y; U = u - 0.5; V = v - 0.5; }
  else {
    Y = (y * 1023.0 - 64.0) / 876.0;
    U = (u * 1023.0 - 512.0) / 896.0;
    V = (v * 1023.0 - 512.0) / 896.0;
  }
  Y = clamp(Y, 0.0, 1.0);
  return vec3(Y + 1.5748 * V, Y - 0.1873 * U - 0.4681 * V, Y + 1.8556 * U);
}

vec3 lut3(sampler3D t3, int n, vec3 c){
  vec3 p = clamp(c, 0.0, 1.0) * float(n - 1);
  vec3 b = floor(p); vec3 f = p - b;
  ivec3 i0 = ivec3(b); int mx = n - 1;
  vec3 c000 = texelFetch(t3, i0, 0).rgb;
  vec3 c100 = texelFetch(t3, clamp(i0+ivec3(1,0,0), ivec3(0), ivec3(mx)), 0).rgb;
  vec3 c010 = texelFetch(t3, clamp(i0+ivec3(0,1,0), ivec3(0), ivec3(mx)), 0).rgb;
  vec3 c110 = texelFetch(t3, clamp(i0+ivec3(1,1,0), ivec3(0), ivec3(mx)), 0).rgb;
  vec3 c001 = texelFetch(t3, clamp(i0+ivec3(0,0,1), ivec3(0), ivec3(mx)), 0).rgb;
  vec3 c101 = texelFetch(t3, clamp(i0+ivec3(1,0,1), ivec3(0), ivec3(mx)), 0).rgb;
  vec3 c011 = texelFetch(t3, clamp(i0+ivec3(0,1,1), ivec3(0), ivec3(mx)), 0).rgb;
  vec3 c111 = texelFetch(t3, clamp(i0+ivec3(1,1,1), ivec3(0), ivec3(mx)), 0).rgb;
  return mix(mix(mix(c000,c100,f.x), mix(c010,c110,f.x), f.y),
             mix(mix(c001,c101,f.x), mix(c011,c111,f.x), f.y), f.z);
}

void main(){
  vec2 uv = vec2(vUv.x, 1.0 - vUv.y);
  vec3 raw = clamp(sampleSrc(uv), 0.0, 1.0);
  raw = clamp(raw * exp2(uGain), 0.0, 1.0);

  // PUSH/PULL (FilmBox/Dehancer): remap de TODA la respuesta — exposición +
  // gamma del negativo + sombras — no un simple gain.
  float pp = uPushPull;
  if (abs(pp) > 0.001){
    raw = clamp(raw * exp2(pp * 0.7), 0.0, 1.0);
    raw = pow(raw, vec3(1.0 + pp * 0.10));            // gamma del negativo
    if (pp > 0.0) raw = mix(raw, vec3(1.0), 0.04 * pp);   // push: sombras levantadas
    else raw = pow(raw, vec3(1.0 - pp * 0.06));           // pull: respuesta más plana
  }

  // FILM COMPRESSION (Dehancer): la emulsión es una esponja, no un vaso —
  // las altas luces se comprimen hacia un techo, nunca clippean.
  if (uCompress > 0.001){
    float thr = 1.0 - uCompressRange;
    vec3 over = max(raw - thr, 0.0);
    raw = raw - over + over / (1.0 + uCompress * over / max(uCompressWP, 0.05));
  }

  outRaw = vec4(raw, 1.0);
  vec3 graded = raw;
  if (uLutAOn == 1) graded = clamp(lut3(uLutA, uLutNA, graded), 0.0, 1.0);
  if (uLutBOn == 1) graded = clamp(lut3(uLutB, uLutNB, graded), 0.0, 1.0);
  outGraded = vec4(graded, 1.0);
}`;

const FS_DOWN = `#version 300 es
precision highp float;
uniform sampler2D uTex; uniform vec2 uTexel;
in vec2 vUv; out vec4 o;
void main(){
  vec3 c = texture(uTex, vUv).rgb * 4.0
    + texture(uTex, vUv + uTexel*vec2(-1,-1)).rgb
    + texture(uTex, vUv + uTexel*vec2( 1,-1)).rgb
    + texture(uTex, vUv + uTexel*vec2(-1, 1)).rgb
    + texture(uTex, vUv + uTexel*vec2( 1, 1)).rgb;
  o = vec4(c / 8.0, 1.0);
}`;

const FS_BLUR = `#version 300 es
precision highp float;
uniform sampler2D uTex; uniform vec2 uDir; uniform float uRadius;
in vec2 vUv; out vec4 o;
void main(){
  float w[5]; w[0]=0.227027; w[1]=0.1945946; w[2]=0.1216216; w[3]=0.054054; w[4]=0.016216;
  vec3 c = texture(uTex, vUv).rgb * w[0];
  for (int i=1;i<5;i++){
    vec2 off = uDir * float(i) * uRadius;
    c += texture(uTex, vUv + off).rgb * w[i];
    c += texture(uTex, vUv - off).rgb * w[i];
  }
  o = vec4(c, 1.0);
}`;

// Slow shutter: integración temporal real (IIR exponencial ≈ obturador largo).
const FS_ACCUM = `#version 300 es
precision highp float;
uniform sampler2D uCurr, uPrev;
uniform float uFeedback; uniform int uReset;
in vec2 vUv; out vec4 o;
void main(){
  vec3 c = texture(uCurr, vUv).rgb;
  vec3 p = texture(uPrev, vUv).rgb;
  o = vec4(uReset == 1 ? c : mix(c, p, uFeedback), 1.0);
}`;

// Métrica de movimiento inter-frame (sobre la señal PRE-shutter, 1/8 res).
const FS_MOTION = `#version 300 es
precision highp float;
uniform sampler2D uA, uB;
in vec2 vUv; out vec4 o;
void main(){
  vec3 a = texture(uA, vUv).rgb, b = texture(uB, vUv).rgb;
  o = vec4(dot(abs(a - b), vec3(0.3333)), 0.0, 0.0, 1.0);
}`;

const FS_COMP = `#version 300 es
precision highp float;
uniform sampler2D uBase, uRaw, uBlurB, uBlurC, uBlurD;
uniform vec2 uTexel;
uniform float uTime, uSeed;
uniform vec2 uWeavePx;
// halation
uniform float uHalAmount, uHalHue, uHalSat, uHalThr, uHalSpread, uHalWhite;
// bloom
uniform float uBloomAmount, uBloomThr, uBloomWarm;
// softness
uniform float uSoftness;
// grain (modelo silver-halide: clumps + componente fina, respuesta tonal)
uniform sampler2D uGrainTex; uniform float uPlateN;
uniform float uGrainAmount, uGrainSize, uGrainRough, uGrainChroma, uGrainDefocus;
uniform float uGrainS, uGrainM, uGrainH, uGrainR, uGrainB, uFilmRes;
// óptica
uniform float uVigAmount, uVigSize, uVigRound, uVigCX, uVigCY, uCA;
// proyección
uniform float uDust, uFlicker, uFlickerRate, uBreath, uBreathRate, uWeaveRot;
// laboratorio
uniform float uAcut, uColorSep;
// FILM COLOR: física espectral de la emulsión (post-LUT)
uniform float uHueSkew, uCrosstalk, uSubtractive, uStockSat, uPrint;
// frame / film gate
uniform float uFrameInset, uFrameCorner, uFrameWobble;
uniform vec2 uRes;
uniform float uWipe;
in vec2 vUv; out vec4 o;

float hash(vec2 p){ p = fract(p*vec2(123.34, 456.21)); p += dot(p, p+45.32); return fract(p.x*p.y); }
float gnoise(vec2 p){
  return (hash(p) + hash(p+17.7) + hash(p+31.3) + hash(p+47.9)) * 0.5 - 1.0;
}
float vnoise(vec2 p){   // value noise suave (clumps)
  vec2 i = floor(p), f = fract(p);
  f = f*f*(3.0-2.0*f);
  float a = hash(i), b = hash(i+vec2(1,0)), c = hash(i+vec2(0,1)), d = hash(i+vec2(1,1));
  return mix(mix(a,b,f.x), mix(c,d,f.x), f.y) * 2.0 - 1.0;
}
vec3 hsv2rgb(vec3 c){
  vec3 rgb = clamp(abs(mod(c.x*6.0+vec3(0,4,2), 6.0)-3.0)-1.0, 0.0, 1.0);
  return c.z * mix(vec3(1.0), rgb, c.y);
}
vec3 screen(vec3 a, vec3 b){ return 1.0 - (1.0-a)*(1.0-b); }
float bell(float x, float c, float w){ float t = (x-c)/w; return exp(-t*t); }
float bellH(float h, float c, float w){   // campana circular en grados de matiz
  float d = abs(h - c); d = min(d, 360.0 - d);
  return bell(d, 0.0, w);
}
vec3 rgb2hsv(vec3 c){
  vec4 K = vec4(0.0, -1.0/3.0, 2.0/3.0, -1.0);
  vec4 p = mix(vec4(c.bg, K.wz), vec4(c.gb, K.xy), step(c.b, c.g));
  vec4 q = mix(vec4(p.xyw, c.r), vec4(c.r, p.yzx), step(p.x, c.r));
  float d = q.x - min(q.w, q.y), e = 1.0e-10;
  return vec3(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
}
float lumOf(vec3 c){ return dot(c, vec3(0.2126, 0.7152, 0.0722)); }

// HUE SKEWS: matiz y luminancia ACOPLADOS (en digital son independientes).
// Reglas documentadas de la emulsión: cian→azul, verde→amarillo, rojo→naranja
// en altas; magenta→rojo, azul→cian en sombras; el amarillo (piel) no rota.
vec3 hueSkew(vec3 col, float amt){
  vec3 hsv = rgb2hsv(col);
  float h = hsv.x * 360.0, l = lumOf(col);
  float hi = smoothstep(0.45, 0.85, l);
  float mid = smoothstep(0.30, 0.70, l);
  float lo = 1.0 - smoothstep(0.10, 0.45, l);
  float dh = 0.0;
  dh += bellH(h, 190.0, 30.0) * hi * 18.0;                    // cian → azul (altas)
  dh += bellH(h, 120.0, 35.0) * (0.4*hi + 0.6*mid) * -25.0;   // verde → amarillo/oliva
  dh += bellH(h, 8.0, 22.0) * hi * 15.0;                      // rojo → naranja (altas)
  dh += bellH(h, 310.0, 30.0) * lo * 20.0;                    // magenta → rojo (sombras)
  dh += bellH(h, 235.0, 25.0) * lo * -15.0;                   // azul → cian (sombras teal)
  hsv.x = fract(hsv.x + dh * amt / 360.0 + 1.0);
  return hsv2rgb(hsv);
}

void main(){
  vec2 wv = uWeavePx * uTexel;
  vec2 uv = vUv + wv;
  // weave con componente de rotación (la película también gira en la ventanilla)
  if (uWeaveRot > 0.001){
    float ang = (sin(uTime * 1.9) + 0.5 * sin(uTime * 3.7 + 1.1)) * 0.0006 * uWeaveRot * length(uWeavePx + 0.5);
    vec2 c = uv - 0.5;
    uv = vec2(c.x * cos(ang) - c.y * sin(ang), c.x * sin(ang) + c.y * cos(ang)) + 0.5;
  }

  // base con aberración cromática radial (lente)
  vec2 caOff = (uv - 0.5) * uCA * 0.018;
  vec3 base;
  base.r = texture(uBase, uv + caOff).r;
  base.g = texture(uBase, uv).g;
  base.b = texture(uBase, uv - caOff).b;
  vec3 col = base;

  // ── FILM COLOR STAGE ────────────────────────────────────────────────
  // 1. CROSSTALK estructural: la capa roja está DEBAJO — la luz la alcanza
  //    atravesando azul y verde; los canales se contaminan con la exposición.
  if (uCrosstalk > 0.001){
    float l = lumOf(col);
    col.r += uCrosstalk * (0.06 * col.g + 0.04 * col.b) * (0.4 + 0.6 * l);
    col.g += uCrosstalk * 0.04 * col.r;
    col.b += uCrosstalk * 0.03 * col.g;
  }
  // 2. HUE SKEWS (cada capa tiene su propia curva → rotaciones por exposición)
  if (uHueSkew > 0.001) col = hueSkew(col, uHueSkew);
  // 3. SATURACIÓN SUSTRACTIVA: más saturado = más denso = más oscuro; los
  //    medios aguantan, sombras/altas caen; en altas se desatura PRIMERO lo
  //    más saturado; las sombras conservan residuo de matiz (negro con hue).
  if (uSubtractive > 0.001){
    float l0 = lumOf(col);
    vec3 ch0 = col - l0;
    float chMag = length(ch0);
    float satW = 0.85 + 0.35 * bell(l0, 0.45, 0.30);
    satW *= 1.0 - 0.55 * smoothstep(0.70, 0.95, l0);
    satW *= 1.0 - 0.35 * smoothstep(0.15, 0.45, chMag) * smoothstep(0.65, 0.9, l0);
    vec3 ch1 = ch0 * mix(1.0, satW * uStockSat, uSubtractive);
    float darken = 1.0 - uSubtractive * 0.5 * max(length(ch1) - chMag, 0.0);
    col = (vec3(l0) + ch1) * darken;
  }
  // 4. PRINT (2383): S-curve, D-min elevado con cast teal en sombras y cálido
  //    en altas, techo de gamut (compresión no lineal de saturación).
  if (uPrint > 0.001){
    vec3 sc = col * col * (3.0 - 2.0 * col);
    sc = mix(col, sc, 0.85);
    float l = lumOf(sc);
    sc += vec3(0.010, 0.016, 0.020) * (1.0 - smoothstep(0.0, 0.35, l)) * 1.2;
    sc += vec3(0.030, 0.018, 0.006) * smoothstep(0.6, 0.95, l);
    vec3 ch = sc - l;
    sc = sc - ch + ch / (1.0 + 1.5 * length(ch));     // techo de gamut
    sc = mix(sc, vec3(0.012, 0.016, 0.020), 0.06);    // negro no negro
    col = mix(col, clamp(sc, 0.0, 1.0), uPrint);
  }
  // ─────────────────────────────────────────────────────────────────────

  // ACUTANCE (FilmBox, adjacency effect): boost de alta frecuencia con halo
  if (uAcut > 0.001){
    vec3 hf = texture(uBase, uv).rgb - texture(uBlurB, uv).rgb;
    col += hf * uAcut * 0.6;
  }

  // softness / difusión (1/4)
  if (uSoftness > 0.001) col = mix(col, texture(uBlurC, uv).rgb, uSoftness * 0.55);

  // halation: dos lóbulos con hue dependiente del radio (física Dehancer: la
  // luz reflejada en la base se pre-filtra — naranja cerca, rojo lejos).
  if (uHalAmount > 0.001){
    vec3 inner = texture(uBlurC, uv).rgb;
    vec3 outer = texture(uBlurD, uv).rgb;
    float mI = smoothstep(uHalThr - 0.18, uHalThr + 0.18, lumOf(inner));
    float mO = smoothstep(uHalThr - 0.18, uHalThr + 0.18, lumOf(outer));
    vec3 tintI = hsv2rgb(vec3(0.055 + uHalHue * 0.05, uHalSat, 1.0));        // naranja (capa verde activada)
    vec3 tintO = hsv2rgb(vec3(uHalHue * 0.045, min(uHalSat * 1.1, 1.0), 1.0)); // rojo (solo capa roja)
    tintI = mix(tintI, vec3(1.0), uHalWhite);
    tintO = mix(tintO, vec3(1.0), uHalWhite);
    float sp = clamp(uHalSpread, 0.0, 1.0);
    vec3 hal = inner * tintI * mI * (1.0 - sp * 0.6) + outer * tintO * mO * sp;
    col = screen(col, hal * uHalAmount * 0.85);
  }

  // bloom: veiling glare de lente (1/2 + 1/4), tinte blanco↔cálido
  if (uBloomAmount > 0.001){
    vec3 b = mix(texture(uBlurB, uv).rgb, texture(uBlurC, uv).rgb, 0.5);
    float m = smoothstep(uBloomThr - 0.12, uBloomThr + 0.12, lumOf(b));
    vec3 tintB = mix(vec3(1.0), hsv2rgb(vec3(0.07, 1.0, 1.0)), clamp(uBloomWarm, 0.0, 1.0));
    col = screen(col, b * m * uBloomAmount * 0.45 * tintB);
  }

  // flicker (rápido, obturador) + film breath (lento: emulsión/desarrollo) —
  // dos escalas temporales, con deriva de color sustractiva en el breath
  if (uFlicker > 0.001 || uBreath > 0.001){
    float fast = hash(vec2(floor(uTime * (4.0 + uFlickerRate*20.0)), 7.0)) - 0.5;
    float slowT = uTime * (0.4 + uBreathRate * 1.6);
    float s0 = floor(slowT), f0 = fract(slowT);
    f0 = f0 * f0 * (3.0 - 2.0 * f0);                       // random walk suave
    float slow = mix(hash(vec2(s0, 3.7)), hash(vec2(s0 + 1.0, 3.7)), f0) - 0.5;
    col *= 1.0 + fast * uFlicker * 0.10 + slow * uBreath * 0.07;
    vec3 cshake = vec3(hash(vec2(s0, 11.1)), hash(vec2(s0, 13.7)), hash(vec2(s0, 17.3))) - 0.5;
    col *= 1.0 + uBreath * 0.05 * cshake;                  // deriva CMY
  }

  // vignette cos⁴ (ley óptica), centro y forma ajustables
  if (uVigAmount > 0.001){
    vec2 q = (vUv - vec2(uVigCX, uVigCY)) * vec2(1.25, 1.0) / max(uVigSize, 0.05);
    float dCirc = length(q);
    float dRect = max(abs(q.x), abs(q.y)) * 1.12;
    float d = mix(dRect, dCirc, clamp(uVigRound, 0.0, 1.0));
    float theta = clamp(d * 1.35, 0.0, 1.45);
    float fall = pow(cos(theta), 4.0);                    // cos⁴ law
    col *= mix(1.0, fall, clamp(uVigAmount, 0.0, 1.0));
  }

  // GRAIN con asimetría negativo/print (Dehancer/FilmBox): en altas luces el
  // grano es de NEGATIVO (crisp, duro); en sombras es de PRINT (suave, levanta
  // el negro — el negro digital plano delata lo digital).
  if (uGrainAmount > 0.001){
    float lum = lumOf(col);
    vec2 gp = (gl_FragCoord.xy + uWeavePx) / uPlateN;
    float cell = max(uGrainSize, 0.4);
    vec2 seedV = vec2(hash(vec2(uSeed, 1.31)), hash(vec2(uSeed, 7.77)));
    float lod = uGrainDefocus * 2.5;
    float s1 = 1.0 / cell;
    float s2 = 3.5 / cell;
    float nClump = textureLod(uGrainTex, gp * s1 + seedV, lod).r * 2.0 - 1.0;
    ivec2 iq = ivec2(floor((gl_FragCoord.xy + uWeavePx) / max(cell * 0.5, 0.75)));
    iq = (iq + ivec2(seedV * 1024.0)) & 1023;
    float crisp = texelFetch(uGrainTex, iq, 0).r * 2.0 - 1.0;
    crisp = sign(crisp) * pow(abs(crisp), 0.65);
    float nFine  = textureLod(uGrainTex, gp * s2 + seedV * 1.31 + 0.5, lod).r * 2.0 - 1.0;
    float nNeg = mix(nClump, mix(nFine, crisp, 0.75), clamp(uGrainRough, 0.0, 1.0)); // negativo: duro
    float nPrint = nClump * 0.8;                                                        // print: suave
    vec3 nRGB = vec3(
      textureLod(uGrainTex, gp * s1 + seedV + vec2(0.31, 0.73), lod).r,
      textureLod(uGrainTex, gp * s1 + seedV + vec2(0.57, 0.11), lod).r,
      textureLod(uGrainTex, gp * s1 + seedV + vec2(0.83, 0.47), lod).r) * 2.0 - 1.0;
    vec3 gNeg = mix(vec3(nNeg), nRGB, clamp(uGrainChroma, 0.0, 1.0));
    vec3 gPrint = mix(vec3(nPrint), nRGB * 0.7, clamp(uGrainChroma, 0.0, 1.0));
    gNeg *= vec3(uGrainR, 1.0, uGrainB);
    gPrint *= vec3(uGrainR, 1.0, uGrainB);
    float wS = uGrainS * bell(lum, 0.12, 0.20);
    float wM = uGrainM * bell(lum, 0.42, 0.30);
    float wH = uGrainH * bell(lum, 0.85, 0.24);
    float norm = inversesqrt(max(cell * 0.5, 1.0));
    col += (gPrint * wS + gNeg * (wM + wH)) * uGrainAmount * 0.30 * norm;
    col += wS * uGrainAmount * 0.012;                  // print grain levanta el negro
    col = mix(col, texture(uBlurB, uv).rgb, uGrainAmount * uFilmRes * 0.18);
  }

  // COLOR SEPARATION (Dehancer developer): satura primero lo más saturado
  if (uColorSep > 0.001){
    float l = lumOf(col);
    vec3 ch = col - l;
    col = vec3(l) + ch * (1.0 + uColorSep * smoothstep(0.04, 0.35, length(ch)));
  }

  // dust & scratches (proyección: estáticos en pantalla, con TTL)
  if (uDust > 0.001){
    float epoch = floor(uTime * 2.0);
    for (int i = 0; i < 6; i++){                       // motas de polvo
      float fi = float(i);
      float born = hash(vec2(fi, epoch));
      if (born < uDust * 0.45){
        vec2 pos = vec2(hash(vec2(fi, epoch + 1.3)), hash(vec2(fi, epoch + 2.9)));
        float r = 1.0 + 2.5 * hash(vec2(fi, epoch + 4.1));
        float d = length((vUv - pos) * vec2(1.78, 1.0) * 960.0);
        float spot = 1.0 - smoothstep(r * 0.4, r, d);
        col = mix(col, vec3(0.02), spot * 0.5 * step(0.5, hash(vec2(fi, epoch + 5.7))));
        col = mix(col, vec3(0.9), spot * 0.25 * step(hash(vec2(fi, epoch + 5.7)), 0.5));
      }
    }
    for (int i = 0; i < 3; i++){                       // rayas verticales
      float fi = float(i) + 31.0;
      float born = hash(vec2(fi, epoch * 0.5));
      if (born < uDust * 0.22){
        float x = hash(vec2(fi, epoch * 0.5 + 1.1));
        float wdt = 0.4 + hash(vec2(fi, 3.3)) * 0.8;
        float line = 1.0 - smoothstep(wdt * 0.5, wdt, abs(vUv.x - x) * 1920.0);
        float jitter = 0.85 + 0.15 * sin(vUv.y * 40.0 + fi);
        col *= 1.0 - line * 0.10 * jitter;
      }
    }
  }

  // FRAME / film gate: rectángulo redondeado con borde irregular (la ventanilla
  // real no es perfecta) y negro cálido
  if (uFrameInset > 0.5){
    vec2 px = vUv * uRes;
    vec2 half_ = uRes * 0.5;
    float ang = atan(px.y - half_.y, px.x - half_.x);
    float wob = (vnoise(vec2(ang * 2.5 + 7.0, 3.0)) + 0.5 * vnoise(vec2(ang * 6.0, 9.0))) * uFrameWobble * 5.0;
    vec2 b = half_ - uFrameInset;
    vec2 q = abs(px - half_) - b + uFrameCorner;
    float d = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - uFrameCorner + wob;
    float m = smoothstep(0.0, 1.5, d);
    col = mix(col, vec3(0.010, 0.008, 0.006), m);
  }

  if (vUv.x > uWipe) col = texture(uRaw, vUv).rgb;

  o = vec4(clamp(col, 0.0, 1.0), 1.0);
}`;

function compile(gl, type, src){
  const s = gl.createShader(type);
  gl.shaderSource(s, src); gl.compileShader(s);
  if (!gl.getShaderParameter(s, gl.COMPILE_STATUS))
    throw new Error(gl.getShaderInfoLog(s) + "\n" + src);
  return s;
}
function program(gl, fs){
  const p = gl.createProgram();
  gl.attachShader(p, compile(gl, gl.VERTEX_SHADER, VS));
  gl.attachShader(p, compile(gl, gl.FRAGMENT_SHADER, fs));
  gl.linkProgram(p);
  if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p));
  const uniforms = {};
  const n = gl.getProgramParameter(p, gl.ACTIVE_UNIFORMS);
  for (let i = 0; i < n; i++){ const u = gl.getActiveUniform(p, i); uniforms[u.name] = gl.getUniformLocation(p, u.name); }
  return { p, u: uniforms };
}

export class Pipeline {
  constructor(canvas, lutData, lutN){ if (!lutData){ lutData = new Float32Array([0,0,0, 1,0,0, 0,1,0, 1,1,0, 0,0,1, 1,0,1, 0,1,1, 1,1,1]); lutN = 2; }
    const gl = canvas.getContext("webgl2", { antialias: false, alpha: false, preserveDrawingBuffer: true });
    if (!gl) throw new Error("WebGL2 no disponible");
    if (!gl.getExtension("EXT_color_buffer_float"))
      throw new Error("EXT_color_buffer_float no disponible");
    this.gl = gl; this.canvas = canvas;
    this.pGrade = program(gl, FS_GRADE);
    this.pDown  = program(gl, FS_DOWN);
    this.pBlur  = program(gl, FS_BLUR);
    this.pAccum = program(gl, FS_ACCUM);
    this.pMotion = program(gl, FS_MOTION);
    this.pComp  = program(gl, FS_COMP);
    const buf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1,-1, 3,-1, -1,3]), gl.STATIC_DRAW);
    const vao = gl.createVertexArray();
    gl.bindVertexArray(vao);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
    this.lutNA = 0; this.lutNB = 0;
    this.tLutA = gl.createTexture();
    this.tLutB = gl.createTexture();
    for (const t3 of [this.tLutA, this.tLutB]){
      gl.bindTexture(gl.TEXTURE_3D, t3);
      gl.texImage3D(gl.TEXTURE_3D, 0, gl.RGB16F, lutN, lutN, lutN, 0, gl.RGB, gl.FLOAT, lutData);
    }
    for (const prm of ["S","T","R"]) gl.texParameteri(gl.TEXTURE_3D, gl["TEXTURE_WRAP_" + prm], gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_3D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_3D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    this.srcMode = -1;
    this.tY = gl.createTexture(); this.tU = gl.createTexture(); this.tV = gl.createTexture();
    this.tVideo = gl.createTexture();
    for (const t of [this.tY, this.tU, this.tV, this.tVideo]){
      gl.bindTexture(gl.TEXTURE_2D, t);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    }
    for (const t of [this.tY, this.tU, this.tV]){
      gl.bindTexture(gl.TEXTURE_2D, t);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.R16UI, 1, 1, 0, gl.RED_INTEGER, gl.UNSIGNED_SHORT, new Uint16Array([0]));
    }
    this.fbos = null;
    this.histInit = false;
  }

  _fbo(w, h){
    const gl = this.gl;
    const t = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, t);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA16F, w, h, 0, gl.RGBA, gl.HALF_FLOAT, null);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    const f = gl.createFramebuffer();
    gl.bindFramebuffer(gl.FRAMEBUFFER, f);
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, t, 0);
    return { f, t, w, h };
  }

  _buildFbos(w, h){
    const s = this.quality || 1;
    w = Math.max(2, Math.round(w * s)); h = Math.max(2, Math.round(h * s));
    this.fbos = {
      graded: this._fbo(w, h), raw: this._fbo(w, h),
      hA: this._fbo(w, h), hB: this._fbo(w, h),
      b0: this._fbo(w>>1, h>>1), b1: this._fbo(w>>1, h>>1),
      c0: this._fbo(w>>2, h>>2), c1: this._fbo(w>>2, h>>2),
      d0: this._fbo(w>>3, h>>3), d1: this._fbo(w>>3, h>>3),
      gB: this._fbo(w>>1, h>>1), gC: this._fbo(w>>2, h>>2),
      gD: this._fbo(w>>3, h>>3), gPrev: this._fbo(w>>3, h>>3),
      motion: this._fbo(32, 18),
    };
    this.histInit = false;
    this.motionPx = new Float32Array(32 * 18 * 4);
    this.fbMRT = null;          // las texturas cambiaron: recrear adjuntos MRT
    this._sceneKey = null;
  }

  // media de la métrica de movimiento (0..~0.3); llamar tras render()
  readMotion(){
    const gl = this.gl, F = this.fbos;
    if (!F) return 0;
    gl.bindFramebuffer(gl.FRAMEBUFFER, F.motion.f);
    gl.readPixels(0, 0, 32, 18, gl.RGBA, gl.FLOAT, this.motionPx);
    let s = 0;
    for (let i = 0; i < 32 * 18; i++) s += this.motionPx[i * 4];
    return s / (32 * 18);
  }

  setLut(lutData, lutN2){
    const gl = this.gl;
    this.lutNB = lutN2;
    gl.bindTexture(gl.TEXTURE_3D, this.tLutB);
    this._lutParams();
    gl.texImage3D(gl.TEXTURE_3D, 0, gl.RGB16F, lutN2, lutN2, lutN2, 0, gl.RGB, gl.FLOAT, lutData);
  }

  setInputLut(lutData, lutN2){
    const gl = this.gl;
    this.lutNA = lutN2;
    gl.bindTexture(gl.TEXTURE_3D, this.tLutA);
    this._lutParams();
    gl.texImage3D(gl.TEXTURE_3D, 0, gl.RGB16F, lutN2, lutN2, lutN2, 0, gl.RGB, gl.FLOAT, lutData);
  }

  _lutParams(){
    const gl = this.gl;
    for (const prm of ["S","T","R"]) gl.texParameteri(gl.TEXTURE_3D, gl["TEXTURE_WRAP_" + prm], gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_3D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_3D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
  }

  setQuality(scale){
    if (scale !== this.quality){
      this.quality = scale;
      if (this.srcW) this._buildFbos(this.srcW, this.srcH);
    }
  }

  // Readback asíncrono con doble PBO (el frame enviado es el anterior al actual)
  readbackPBO(){
    const gl = this.gl, w = this.canvas.width, h = this.canvas.height;
    const size = w * h * 4;
    if (!this._pbos){
      this._pbos = [gl.createBuffer(), gl.createBuffer()];
      for (const b of this._pbos){
        gl.bindBuffer(gl.PIXEL_PACK_BUFFER, b);
        gl.bufferData(gl.PIXEL_PACK_BUFFER, size, gl.STREAM_READ);
      }
      this._pboIdx = 0;
      this._pboData = new Uint8Array(size);
    }
    gl.bindBuffer(gl.PIXEL_PACK_BUFFER, this._pbos[this._pboIdx]);
    gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, 0);
    gl.bindBuffer(gl.PIXEL_PACK_BUFFER, this._pbos[1 - this._pboIdx]);
    gl.getBufferSubData(gl.PIXEL_PACK_BUFFER, 0, this._pboData);
    this._pboIdx = 1 - this._pboIdx;
    gl.bindBuffer(gl.PIXEL_PACK_BUFFER, null);
    return this._pboData;
  }

  debugChain(){
    const gl = this.gl, F = this.fbos, out = {};
    const rd = (name, fb) => {
      gl.bindFramebuffer(gl.FRAMEBUFFER, fb.f);
      const px = new Float32Array(4);
      gl.readPixels(fb.w>>1, fb.h>>1, 1, 1, gl.RGBA, gl.FLOAT, px);
      out[name] = [...px].map(v => v.toFixed(2)).join(",");
    };
    for (const k of ["graded","raw","hA","b0","c0","d0"]) rd(k, F[k]);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    out.err = gl.getError();
    return out;
  }

  setGrainPlate(u16, n){
    const gl = this.gl;
    this.plateN = n;
    if (!this.tGrain) this.tGrain = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, this.tGrain);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.R16F, n, n, 0, gl.RED, gl.HALF_FLOAT, u16);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.REPEAT);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.REPEAT);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.generateMipmap(gl.TEXTURE_2D);
  }

  setSourceYUV(y, u, v, w, h, cw, ch){
    const gl = this.gl;
    const up = (t, data, pw, ph) => {
      gl.bindTexture(gl.TEXTURE_2D, t);
      gl.pixelStorei(gl.UNPACK_ALIGNMENT, 2);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.R16UI, pw, ph, 0, gl.RED_INTEGER, gl.UNSIGNED_SHORT, data);
    };
    up(this.tY, y, w, h); up(this.tU, u, cw, ch); up(this.tV, v, cw, ch);
    this.srcMode = 0; this.srcW = w; this.srcH = h;
    if (!this.fbos || this.fbos.graded.w !== w) this._buildFbos(w, h);
  }

  // VideoFrame (WebCodecs) directo a textura — usado por el render secuencial
  setSourceFrame(vf){
    const gl = this.gl;
    gl.bindTexture(gl.TEXTURE_2D, this.tVideo);
    gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, gl.RGBA, gl.UNSIGNED_BYTE, vf);
    this.srcMode = 1;
    const w = vf.codedWidth, h = vf.codedHeight;
    this.srcW = w; this.srcH = h;
    if (!this.fbos || this.fbos.graded.w !== Math.max(2, Math.round(w * (this.quality || 1))))
      this._buildFbos(w, h);
    this._lastVideo = null;   // invalida la cache de <video>
  }

  setSourceVideo(videoEl){
    const gl = this.gl;
    // cache: no re-subir 33 MB si es el mismo frame del mismo vídeo
    if (this._lastVideo === videoEl && this._lastVT === videoEl.currentTime && this.fbos) return;
    this._lastVideo = videoEl; this._lastVT = videoEl.currentTime;
    gl.bindTexture(gl.TEXTURE_2D, this.tVideo);
    gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
    if (window.VideoFrame){          // robusto en Chrome/Windows (ruta GPU)
      try {
        const vf = new VideoFrame(videoEl);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, gl.RGBA, gl.UNSIGNED_BYTE, vf);
        vf.close();
      } catch (e) {
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, gl.RGBA, gl.UNSIGNED_BYTE, videoEl);
      }
    } else {
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, gl.RGBA, gl.UNSIGNED_BYTE, videoEl);
    }
    this.srcMode = 1;
    const w = videoEl.videoWidth, h = videoEl.videoHeight;
    this.srcW = w; this.srcH = h;
    if (!this.fbos || this.fbos.graded.w !== w) this._buildFbos(w, h);
  }

  _pass(prog, target, setup){
    const gl = this.gl;
    gl.bindFramebuffer(gl.FRAMEBUFFER, target ? target.f : null);
    gl.viewport(0, 0, target ? target.w : this.canvas.width, target ? target.h : this.canvas.height);
    gl.useProgram(prog.p);
    setup(prog.u);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  }

  _bindTex(unit, tex, loc){
    const gl = this.gl;
    gl.activeTexture(gl.TEXTURE0 + unit);
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.uniform1i(loc, unit);
  }

  render(P, time, seed){
    const gl = this.gl, F = this.fbos;
    if (!F) return;
    const sceneKey = [seed, P.inputLutOn, P.lutOn, this.lutNA, this.lutNB, P.gain,
      P.pushPull, P.compImpact, P.compRange, P.shutter, P.resetHistory ? 1 : 0,
      P.yuvNorm, P.fullRange, P.halSpread, this.quality].join("|");
    const needScene = sceneKey !== this._sceneKey;
    if (needScene){
    this._sceneKey = sceneKey;
    // 1. grade (MRT graded+raw)
    if (!this.fbMRT){
      this.fbMRT = gl.createFramebuffer();
      gl.bindFramebuffer(gl.FRAMEBUFFER, this.fbMRT);
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, F.graded.t, 0);
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT1, gl.TEXTURE_2D, F.raw.t, 0);
      gl.drawBuffers([gl.COLOR_ATTACHMENT0, gl.COLOR_ATTACHMENT1]);
    }
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.fbMRT);
    gl.viewport(0, 0, F.graded.w, F.graded.h);
    gl.useProgram(this.pGrade.p);
    const u = this.pGrade.u;
    const bindT = (unit, tex, loc, is3D) => {
      gl.activeTexture(gl.TEXTURE0 + unit);
      gl.bindTexture(is3D ? gl.TEXTURE_3D : gl.TEXTURE_2D, tex);
      gl.uniform1i(loc, unit);
    };
    bindT(0, this.tY, u.uY); bindT(1, this.tU, u.uU); bindT(2, this.tV, u.uV);
    bindT(3, this.tVideo, u.uVideo);
    bindT(4, this.tLutA, u.uLutA, true);
    bindT(5, this.tLutB, u.uLutB, true);
    gl.uniform1i(u.uSrcMode, this.srcMode);
    gl.uniform1f(u.uYNorm, P.yuvNorm);
    gl.uniform1i(u.uFullRange, P.fullRange ? 1 : 0);
    gl.uniform1i(u.uLutNA, this.lutNA || 2);
    gl.uniform1i(u.uLutNB, this.lutNB || 2);
    gl.uniform1i(u.uLutAOn, (P.inputLutOn && this.lutNA) ? 1 : 0);
    gl.uniform1i(u.uLutBOn, (P.lutOn && this.lutNB) ? 1 : 0);
    gl.uniform1f(u.uGain, P.gain || 0);
    gl.uniform1f(u.uPushPull, P.pushPull || 0);
    gl.uniform1f(u.uCompress, P.compImpact || 0);
    gl.uniform1f(u.uCompressWP, P.compWP || 1.0);
    gl.uniform1f(u.uCompressRange, P.compRange ?? 0.5);
    gl.uniform2f(u.uSrcSize, this.srcW, this.srcH);
    gl.drawArrays(gl.TRIANGLES, 0, 3);

    // 2. slow shutter: acumulador temporal (ping-pong hA/hB)
    const reset = !this.histInit || P.resetHistory;
    this._pass(this.pAccum, F.hB, uu => {
      this._bindTex(0, F.graded.t, uu.uCurr);
      this._bindTex(1, F.hA.t, uu.uPrev);
      gl.uniform1f(uu.uFeedback, P.shutter || 0);
      gl.uniform1i(uu.uReset, reset ? 1 : 0);
    });
    [F.hA, F.hB] = [F.hB, F.hA];
    this.histInit = true;

    // 3. downs + blurs sobre la imagen acumulada
    const down = (src, dst) => this._pass(this.pDown, dst, uu => {
      this._bindTex(0, src.t, uu.uTex);
      gl.uniform2f(uu.uTexel, 1/src.w, 1/src.h);
    });
    down(F.hA, F.b0); down(F.b0, F.c0); down(F.c0, F.d0);
    const blur = (src, tmp, dst, rad) => {
      this._pass(this.pBlur, tmp, uu => {
        this._bindTex(0, src.t, uu.uTex);
        gl.uniform2f(uu.uDir, rad/src.w, 0); gl.uniform1f(uu.uRadius, 1);
      });
      this._pass(this.pBlur, dst, uu => {
        this._bindTex(0, tmp.t, uu.uTex);
        gl.uniform2f(uu.uDir, 0, rad/src.h); gl.uniform1f(uu.uRadius, 1);
      });
    };
    blur(F.b0, F.b1, F.b0, 7.0);
    blur(F.c0, F.c1, F.c0, 1.5 + P.halSpread * 2.0);
    blur(F.d0, F.d1, F.d0, 4.0 + P.halSpread * 6.0);

    // 3b. métrica de movimiento PRE-shutter (graded vs frame anterior)
    down(F.graded, F.gB); down(F.gB, F.gC); down(F.gC, F.gD);
    this._pass(this.pMotion, F.motion, uu => {
      this._bindTex(0, F.gD.t, uu.uA);
      this._bindTex(1, F.gPrev.t, uu.uB);
    });
    this._pass(this.pAccum, F.gPrev, uu => {   // gPrev := gD
      this._bindTex(0, F.gD.t, uu.uCurr);
      this._bindTex(1, F.gPrev.t, uu.uPrev);
      gl.uniform1f(uu.uFeedback, 0); gl.uniform1i(uu.uReset, 1);
    });
    }

    // 4. composite → canvas
    const wamp = (P.weave || 0) * 2.5;
    const wr = 0.4 + (P.weaveRate || 0.5) * 2.0;
    const weaveX = wamp * (Math.sin(time*wr*1.7) + 0.5*Math.sin(time*wr*3.1 + 1.3)) / 1.5;
    const weaveY = wamp * (Math.sin(time*wr*2.3 + 0.7) + 0.5*Math.sin(time*wr*4.3 + 2.1)) / 1.5;
    this._pass(this.pComp, null, uu => {
      this._bindTex(0, F.hA.t, uu.uBase);
      this._bindTex(1, F.raw.t, uu.uRaw);
      this._bindTex(2, F.b0.t, uu.uBlurB);
      this._bindTex(3, F.c0.t, uu.uBlurC);
      this._bindTex(4, F.d0.t, uu.uBlurD);
      // exposición → color/grano (negativo denso = rico; rascado = lechoso)
      const pp = P.pushPull || 0;
      const satMul = 1 + 0.15 * pp;
      const grainMul = Math.max(0.4, 1 - 0.25 * pp);
      if (this.tGrain){
        this._bindTex(5, this.tGrain, uu.uGrainTex);
        gl.uniform1f(uu.uPlateN, this.plateN || 1024);
      }
      gl.uniform2f(uu.uTexel, 1/F.hA.w, 1/F.hA.h);
      gl.uniform1f(uu.uTime, time);
      gl.uniform1f(uu.uSeed, seed % 997);
      gl.uniform2f(uu.uWeavePx, weaveX, weaveY);
      gl.uniform1f(uu.uHalAmount, P.halation);
      gl.uniform1f(uu.uHalHue, P.halHue);
      gl.uniform1f(uu.uHalSat, P.halSat);
      gl.uniform1f(uu.uHalThr, P.halThr);
      gl.uniform1f(uu.uHalSpread, P.halSpread);
      gl.uniform1f(uu.uHalWhite, P.halWhite);
      gl.uniform1f(uu.uBloomAmount, P.bloom);
      gl.uniform1f(uu.uBloomThr, P.bloomThr);
      gl.uniform1f(uu.uBloomWarm, P.bloomWarm);
      gl.uniform1f(uu.uSoftness, P.softness);
      gl.uniform1f(uu.uGrainAmount, P.grain * grainMul);
      gl.uniform1f(uu.uGrainSize, P.grainSize);
      gl.uniform1f(uu.uGrainRough, P.grainRough);
      gl.uniform1f(uu.uGrainChroma, P.grainChroma);
      gl.uniform1f(uu.uGrainDefocus, P.grainDefocus);
      gl.uniform1f(uu.uGrainS, P.grainShadows);
      gl.uniform1f(uu.uGrainM, P.grainMids);
      gl.uniform1f(uu.uGrainH, P.grainHighs);
      gl.uniform1f(uu.uGrainR, P.grainRed);
      gl.uniform1f(uu.uGrainB, P.grainBlue);
      gl.uniform1f(uu.uFilmRes, P.filmRes);
      gl.uniform1f(uu.uVigAmount, P.vignette);
      gl.uniform1f(uu.uVigSize, P.vigSize);
      gl.uniform1f(uu.uVigRound, P.vigRound);
      gl.uniform1f(uu.uVigCX, P.vigCX);
      gl.uniform1f(uu.uVigCY, P.vigCY);
      gl.uniform1f(uu.uCA, P.chroma);
      gl.uniform1f(uu.uDust, P.dust);
      gl.uniform1f(uu.uFlicker, P.flicker);
      gl.uniform1f(uu.uFlickerRate, 0.5);
      gl.uniform1f(uu.uBreath, P.breath || 0);
      gl.uniform1f(uu.uBreathRate, P.breathRate ?? 0.5);
      gl.uniform1f(uu.uWeaveRot, P.weaveRot ?? 0.3);
      gl.uniform1f(uu.uAcut, P.acutance || 0);
      gl.uniform1f(uu.uColorSep, P.colorSep || 0);
      gl.uniform1f(uu.uHueSkew, P.hueSkew ?? 1.0);
      gl.uniform1f(uu.uCrosstalk, P.crosstalk ?? 0.3);
      gl.uniform1f(uu.uSubtractive, P.subtractive ?? 0.6);
      gl.uniform1f(uu.uStockSat, (P.stockSat ?? 1.0) * satMul);
      gl.uniform1f(uu.uPrint, P.print ?? 0.5);
      gl.uniform1f(uu.uFrameInset, P.frameInset || 0);
      gl.uniform1f(uu.uFrameCorner, P.frameCorner ?? 40);
      gl.uniform1f(uu.uFrameWobble, P.frameWobble ?? 0.5);
      gl.uniform2f(uu.uRes, F.hA.w, F.hA.h);
      gl.uniform1f(uu.uWipe, P.wipe);
    });
  }
}
