// grade.wgsl — fuente (YUV planar o RGBA) → RGB → gain → push/pull →
// hombro de altas luces → LUT A (log→709) → LUT B (grade). Dos salidas MRT.
//
// Es la MISMA cadena que `grade_bi.wgsl`, con otra entrada: allí la fuente
// llega biplanar desde el decodificador por hardware y aquí llega en tres
// texturas de enteros que sube la CPU. Todo lo que se toque en una hay que
// tocarlo en la otra, o la preview deja de decir la verdad.

struct GradeParams {
  src_mode: u32, full_range: u32, lut_na: u32, lut_nb: u32,
  lut_a_on: u32, lut_b_on: u32, yuv_norm: f32, gain: f32,
  push_pull: f32, compress: f32, compress_wp: f32, compress_range: f32,
  src_w: f32, src_h: f32, pad0: f32, pad1: f32,
  // EL ENCUADRE, el MISMO que el del máster (§1.5): afín completa de uv de
  // lienzo a uv de fuente en `enc_a`+`enc_b.xy`, cuántas muestras por eje en
  // `enc_b.zw` y su separación en `paso`. Aquí se acabó la traducción al
  // vuelo entre el modelo del visor y el del revelado.
  enc_a: vec4<f32>, enc_b: vec4<f32>, paso: vec4<f32>,
  // `peso` no lo usa este pase (la preview no encadena dos fuentes), pero el
  // hueco existe porque LA ESTRUCTURA ES LA MISMA en los dos shaders y en los
  // dos motores: divergir el reparto de bytes se paga carísimo.
  peso: f32, matriz: u32, croma_x: f32, croma_y: f32,
  nd: vec4<f32>,
};

@group(0) @binding(0) var<uniform> P: GradeParams;
@group(0) @binding(1) var tY: texture_2d<u32>;
@group(0) @binding(2) var tU: texture_2d<u32>;
@group(0) @binding(3) var tV: texture_2d<u32>;
@group(0) @binding(4) var tVideo: texture_2d<f32>;
@group(0) @binding(5) var tLutA: texture_3d<f32>;
@group(0) @binding(6) var tLutB: texture_3d<f32>;
@group(0) @binding(7) var samp: sampler;

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

/// bilineal A MANO: estas texturas son de ENTEROS y no se pueden muestrear
fn bilin(t: texture_2d<u32>, size: vec2<f32>, uv: vec2<f32>) -> f32 {
  let pos = clamp(uv, vec2(0.0), vec2(1.0)) * size - 0.5;
  let fl = floor(pos);
  let f = pos - fl;
  let i0 = vec2<i32>(fl);
  // EL TAMAÑO REAL DEL PLANO, redondeado hacia ARRIBA. Con `vec2<i32>(size)`
  // un plano de croma de dimensión impar (1921/2 = 960,5) se quedaba un
  // téxel corto y la última columna se clavaba en la penúltima.
  let mx = vec2<i32>(ceil(size)) - vec2<i32>(1, 1);
  let c00 = f32(textureLoad(t, clamp(i0, vec2(0, 0), mx), 0).r);
  let c10 = f32(textureLoad(t, clamp(i0 + vec2(1, 0), vec2(0, 0), mx), 0).r);
  let c01 = f32(textureLoad(t, clamp(i0 + vec2(0, 1), vec2(0, 0), mx), 0).r);
  let c11 = f32(textureLoad(t, clamp(i0 + vec2(1, 1), vec2(0, 0), mx), 0).r);
  return mix(mix(c00, c10, f.x), mix(c01, c11, f.x), f.y);
}

/// uv del LIENZO → uv de la FUENTE. La misma afín que el máster; w<0 = fuera
/// del material, que es el letterbox.
fn encuadra(uv: vec2<f32>) -> vec3<f32> {
  let f = vec2(P.enc_a.x * uv.x + P.enc_a.y * uv.y + P.enc_b.x,
               P.enc_a.z * uv.x + P.enc_a.w * uv.y + P.enc_b.y);
  let dentro = f.x >= 0.0 && f.y >= 0.0 && f.x <= 1.0 && f.y <= 1.0;
  return vec3(f, select(-1.0, 1.0, dentro));
}

/// un tap: YUV **normalizado a 0..1** (o RGB si la fuente es una foto).
///
/// El croma va sentado donde diga `croma_x/croma_y`: el 4:2:0 de cámara está
/// sentado a la IZQUIERDA y media muestra se ve como franja de color en los
/// bordes verticales duros.
fn tap(uv: vec2<f32>) -> vec3<f32> {
  if P.src_mode == 1u {
    return textureSampleLevel(tVideo, samp, uv, 0.0).rgb;
  }
  let norm = max(P.yuv_norm, 1.0);
  let y = bilin(tY, vec2(P.src_w, P.src_h), uv) / norm;
  // el plano de croma mide la MITAD, redondeando hacia arriba
  let cs = ceil(vec2(P.src_w, P.src_h) * 0.5);
  let dc = vec2(P.croma_x / max(cs.x, 1.0), P.croma_y / max(cs.y, 1.0));
  let u = bilin(tU, cs, uv + dc) / norm;
  let v = bilin(tV, cs, uv + dc) / norm;
  return vec3(y, u, v);
}

/// EL FILTRO DE REDUCCIÓN (§1.5, segunda parte): una rejilla de muestras
/// repartida por la huella real del píxel de salida. Con un solo tap el
/// conform de 4K a 1080 se salta la mitad de los píxeles y aparece el
/// hormigueo en barandillas, rejas y pelo.
///
/// Tope CONSTANTE de 4 aquí (y 6 en el del máster): cada tap de este camino
/// cuesta doce `textureLoad` porque el bilineal va a mano, y esto se dibuja a
/// 60 Hz. La preview se queda en 4:1; el máster llega a 6:1.
fn sample_src(enc: vec2<f32>) -> vec3<f32> {
  let nx = i32(P.enc_b.z);
  let ny = i32(P.enc_b.w);
  if nx <= 1 && ny <= 1 { return tap(enc); }
  let px = P.paso.xy;
  let py = P.paso.zw;
  var acc = vec3(0.0);
  var n = 0.0;
  // tope CONSTANTE: con un tope dinámico el compilador de D3D no sabe
  // desenrollar y el pase no compila (X3570 → X3511)
  for (var i = 0; i < 4; i = i + 1) {
    if (i >= nx) { continue; }
    let ox = (f32(i) + 0.5) / f32(nx) - 0.5;
    for (var j = 0; j < 4; j = j + 1) {
      if (j >= ny) { continue; }
      let oy = (f32(j) + 0.5) / f32(ny) - 0.5;
      acc = acc + tap(enc + px * ox + py * oy);
      n = n + 1.0;
    }
  }
  return acc / max(n, 1.0);
}

/// la gelatina por TETRAEDROS (el porqué está en `grade_bi.wgsl`). Con guarda
/// para `n < 2`, que en enteros sin signo se daba la vuelta a 4 294 967 295.
fn lut3(t: texture_3d<f32>, n: u32, c_in: vec3<f32>) -> vec3<f32> {
  if (n < 2u) { return clamp(c_in, vec3(0.0), vec3(1.0)); }
  let mx = vec3<i32>(i32(n) - 1);
  let cero = vec3<i32>(0, 0, 0);
  let p = clamp(c_in, vec3(0.0), vec3(1.0)) * f32(n - 1u);
  let b = floor(p);
  let f = p - b;
  let i0 = vec3<i32>(b);
  let c000 = textureLoad(t, clamp(i0, cero, mx), 0).rgb;
  let c111 = textureLoad(t, clamp(i0 + vec3(1, 1, 1), cero, mx), 0).rgb;
  var r: vec3<f32>;
  if (f.r >= f.g) {
    if (f.g >= f.b) {
      let c100 = textureLoad(t, clamp(i0 + vec3(1, 0, 0), cero, mx), 0).rgb;
      let c110 = textureLoad(t, clamp(i0 + vec3(1, 1, 0), cero, mx), 0).rgb;
      r = c000 + f.r * (c100 - c000) + f.g * (c110 - c100) + f.b * (c111 - c110);
    } else if (f.r >= f.b) {
      let c100 = textureLoad(t, clamp(i0 + vec3(1, 0, 0), cero, mx), 0).rgb;
      let c101 = textureLoad(t, clamp(i0 + vec3(1, 0, 1), cero, mx), 0).rgb;
      r = c000 + f.r * (c100 - c000) + f.b * (c101 - c100) + f.g * (c111 - c101);
    } else {
      let c001 = textureLoad(t, clamp(i0 + vec3(0, 0, 1), cero, mx), 0).rgb;
      let c101 = textureLoad(t, clamp(i0 + vec3(1, 0, 1), cero, mx), 0).rgb;
      r = c000 + f.b * (c001 - c000) + f.r * (c101 - c001) + f.g * (c111 - c101);
    }
  } else {
    if (f.b >= f.g) {
      let c001 = textureLoad(t, clamp(i0 + vec3(0, 0, 1), cero, mx), 0).rgb;
      let c011 = textureLoad(t, clamp(i0 + vec3(0, 1, 1), cero, mx), 0).rgb;
      r = c000 + f.b * (c001 - c000) + f.g * (c011 - c001) + f.r * (c111 - c011);
    } else if (f.b >= f.r) {
      let c010 = textureLoad(t, clamp(i0 + vec3(0, 1, 0), cero, mx), 0).rgb;
      let c011 = textureLoad(t, clamp(i0 + vec3(0, 1, 1), cero, mx), 0).rgb;
      r = c000 + f.g * (c010 - c000) + f.b * (c011 - c010) + f.r * (c111 - c011);
    } else {
      let c010 = textureLoad(t, clamp(i0 + vec3(0, 1, 0), cero, mx), 0).rgb;
      let c110 = textureLoad(t, clamp(i0 + vec3(1, 1, 0), cero, mx), 0).rgb;
      r = c000 + f.g * (c010 - c000) + f.r * (c110 - c010) + f.b * (c111 - c110);
    }
  }
  return r;
}

/// el hombro de las altas luces (el porqué, en `grade_bi.wgsl`)
fn hombro(o: f32, span: f32, dureza: f32) -> f32 {
  return span / dureza * (1.0 - exp(-dureza * o / span));
}

/// EL FILTRO ND, DESHECHO — el mismo de `grade_bi.wgsl`, donde está contado
/// por qué son dos suciedades distintas y por qué la del infrarrojo lleva
/// guarda de gris y guarda de sombras.
fn corrige_nd(c: vec3<f32>) -> vec3<f32> {
  let fuerza = P.nd.x;
  let tinte = P.nd.y;
  if (fuerza <= 0.001 && abs(tinte) <= 0.001) { return c; }
  var v = c;
  if (abs(tinte) > 0.001) {
    v = vec3(v.r * (1.0 - 0.18 * tinte),
             v.g * (1.0 + 0.05 * tinte),
             v.b * (1.0 - 0.08 * tinte));
  }
  if (fuerza <= 0.001) { return v; }
  let luz = dot(max(v, vec3(0.0)), vec3(0.2126, 0.7152, 0.0722));
  let mx = max(v.r, max(v.g, v.b));
  let mn = min(v.r, min(v.g, v.b));
  let sat = (mx - mn) / max(mx, 1e-4);
  let gris = 1.0 - smoothstep(P.nd.w * 0.5, P.nd.w, sat);
  let sombra = pow(clamp(1.0 - luz, 0.0, 1.0), P.nd.z);
  let k = fuerza * gris * sombra;
  return vec3(v.r - k * max(v.r - v.g, 0.0), v.g,
              v.b - k * 0.35 * max(v.b - v.g, 0.0));
}

/// YUV → RGB con la matriz que toque (709 · 2020 · 601)
fn a_rgb(Y: f32, U: f32, V: f32) -> vec3<f32> {
  if (P.matriz == 1u) {
    return vec3(Y + 1.4746 * V, Y - 0.16455 * U - 0.57135 * V, Y + 1.8814 * U);
  }
  if (P.matriz == 2u) {
    return vec3(Y + 1.402 * V, Y - 0.344136 * U - 0.714136 * V, Y + 1.772 * U);
  }
  return vec3(Y + 1.5748 * V, Y - 0.1873 * U - 0.4681 * V, Y + 1.8556 * U);
}

struct FragOut {
  @location(0) graded: vec4<f32>,
  @location(1) raw: vec4<f32>,
};

@fragment
fn fs_main(in: VsOut) -> FragOut {
  // el encuadre se calcula UNA vez: antes se hacía aquí para el letterbox y
  // otra vez dentro de `sample_src`
  let enc = encuadra(in.uv);
  // FUERA DEL MATERIAL = NEGRO, y negro de verdad: si el letterbox pasara por
  // las gelatinas saldría del color al que la LUT lleve el cero, que no tiene
  // por qué ser negro
  if enc.z < 0.0 {
    var n: FragOut;
    n.raw = vec4(0.0, 0.0, 0.0, 1.0);
    n.graded = vec4(0.0, 0.0, 0.0, 1.0);
    return n;
  }
  let m = sample_src(enc.xy);
  var raw: vec3<f32>;
  if P.src_mode == 1u {
    raw = m;                                  // una foto ya viene en RGB
  } else {
    var Y: f32; var U: f32; var V: f32;
    if P.full_range == 1u {
      Y = m.x; U = m.y - 0.5; V = m.z - 0.5;
    } else {
      // forma SIN profundidad de bits: los mismos números a 8, 10 o 12. La de
      // antes, `(y·1023 − 64)/876`, sólo valía si la fuente era de 10.
      Y = (m.x - 16.0 / 255.0) / (219.0 / 255.0);
      U = (m.y - 128.0 / 255.0) / (224.0 / 255.0);
      V = (m.z - 128.0 / 255.0) / (224.0 / 255.0);
    }
    // sin recortar el luma: el superblanco legal (941–1023) es exactamente el
    // material que el hombro está para recuperar
    raw = a_rgb(Y, U, V);
  }
  raw = corrige_nd(raw);
  // LA DISCIPLINA DE RECORTES: sólo por abajo hasta el final (el comentario
  // largo está en `grade_bi.wgsl`). Recortar por arriba antes del hombro
  // dejaba al compresor sin nada que doblar.
  raw = max(raw, vec3(0.0));
  raw = raw * exp2(P.gain);

  let pp = P.push_pull;
  if abs(pp) > 0.001 {
    raw = max(raw * exp2(pp * 0.7), vec3(0.0));
    raw = pow(raw, vec3(1.0 + pp * 0.10));
    if pp > 0.0 {
      raw = mix(raw, vec3(1.0), 0.04 * pp);
    } else {
      raw = pow(raw, vec3(1.0 - pp * 0.06));
    }
  }

  // el hombro sólo tiene sentido si hay margen por encima de 1 (ver el
  // comentario en `grade_bi.wgsl`)
  if P.compress > 0.001 && P.compress_wp > 1.001 {
    let thr = clamp(1.0 - P.compress_range, 0.0, 0.999);
    let span = max(1.0 - thr, 1e-4);
    let d = max(P.compress, 1e-4);
    let cab = max(P.compress_wp - thr, 1e-4);
    let k = span / max(hombro(cab, span, d), 1e-6);
    let sobra = max(raw - vec3(thr), vec3(0.0));
    raw = min(raw, vec3(thr))
        + vec3(hombro(sobra.r, span, d), hombro(sobra.g, span, d),
               hombro(sobra.b, span, d)) * k;
  }
  raw = clamp(raw, vec3(0.0), vec3(1.0));   // el ÚNICO recorte por arriba

  var graded = raw;
  if P.lut_a_on == 1u { graded = clamp(lut3(tLutA, P.lut_na, graded), vec3(0.0), vec3(1.0)); }
  if P.lut_b_on == 1u { graded = clamp(lut3(tLutB, P.lut_nb, graded), vec3(0.0), vec3(1.0)); }

  var o: FragOut;
  o.raw = vec4(raw, 1.0);
  o.graded = vec4(graded, 1.0);
  return o;
}
