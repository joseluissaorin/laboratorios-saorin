// grade_bi.wgsl — EL REVELADO, con la fuente BIPLANAR que entregan los
// decodificadores por hardware (P010: Y en R16Unorm + UV en RG16Unorm).
//
// Vive aquí, en el taller, y no dentro del motor de Windows, porque lo usan
// LOS DOS: winlab lo incluye tal cual y el motor del Mac lo traduce a Metal
// en el build (MOTOR §8). Una sola fuente de verdad para la parte de la
// cadena que más se toca.
//
// El otro `grade.wgsl` es el de fuente PLANAR (tres texturas de enteros), que
// es lo que sube la preview desde la CPU. Son dos entradas distintas al mismo
// revelado, no dos revelados.

struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
  var p = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
  var o: VsOut;
  o.pos = vec4(p[vi], 0.0, 1.0);
  o.uv = vec2(p[vi].x, -p[vi].y) * 0.5 + 0.5;
  return o;
}

struct GradeU {
  src_mode: u32, full_range: u32, lut_na: u32, lut_nb: u32,
  lut_a_on: u32, lut_b_on: u32, yuv_norm: f32, gain: f32,
  push_pull: f32, compress: f32, compress_wp: f32, compress_range: f32,
  src_w: f32, src_h: f32, pad0: f32, pad1: f32,
  // el encuadre: afín completa (uv lienzo → uv fuente) + cuántas muestras
  enc_a: vec4<f32>, enc_b: vec4<f32>, paso: vec4<f32>,
  peso: f32, matriz: u32, croma_x: f32, croma_y: f32,
  // el filtro ND: fuerza, tinte plano, perfil de sombras y guarda de gris
  nd: vec4<f32>,
};
@group(0) @binding(0) var<uniform> P: GradeU;
@group(0) @binding(1) var tY: texture_2d<f32>;
@group(0) @binding(2) var tUV: texture_2d<f32>;
@group(0) @binding(3) var tLutA: texture_3d<f32>;
@group(0) @binding(4) var tLutB: texture_3d<f32>;
@group(0) @binding(5) var samp: sampler;
@group(0) @binding(6) var tHist: texture_2d<f32>;   // historia del obturador

/// LA GELATINA, POR TETRAEDROS y no por el cubo entero.
///
/// La interpolación trilineal mezcla las ocho esquinas del cubo; la
/// tetraédrica parte el cubo en seis tetraedros y usa sólo los cuatro
/// vértices del que toca. Son las mismas ocho lecturas y algo más de cuenta,
/// pero **respeta la diagonal de grises**: con una LUT de mucha curvatura
/// cerca del neutro —que es justo donde el ojo mira— la trilineal se sale del
/// eje y tuerce los grises. Es lo que usa el mundo del etalonaje, y por eso
/// una .cube se ve aquí como se ve en Resolve.
fn lut3(t: texture_3d<f32>, n: u32, cin: vec3<f32>) -> vec3<f32> {
  // GUARDA: con `n` a 0 o 1, `n - 1u` se da la vuelta a 4 294 967 295 y la
  // gelatina lee basura. Pasa si una .cube no se pudo leer y el tamaño se
  // quedó a cero mientras la ranura seguía encendida.
  if (n < 2u) { return clamp(cin, vec3(0.0), vec3(1.0)); }
  let mx = vec3<u32>(n - 1u);
  let p = clamp(cin, vec3(0.0), vec3(1.0)) * f32(n - 1u);
  let b = floor(p);
  let f = p - b;
  let i0 = vec3<u32>(b);
  let c000 = textureLoad(t, min(i0, mx), 0).rgb;
  let c111 = textureLoad(t, min(i0 + vec3<u32>(1u,1u,1u), mx), 0).rgb;
  var r: vec3<f32>;
  if (f.r >= f.g) {
    if (f.g >= f.b) {          // r ≥ g ≥ b
      let c100 = textureLoad(t, min(i0 + vec3<u32>(1u,0u,0u), mx), 0).rgb;
      let c110 = textureLoad(t, min(i0 + vec3<u32>(1u,1u,0u), mx), 0).rgb;
      r = c000 + f.r * (c100 - c000) + f.g * (c110 - c100) + f.b * (c111 - c110);
    } else if (f.r >= f.b) {   // r ≥ b > g
      let c100 = textureLoad(t, min(i0 + vec3<u32>(1u,0u,0u), mx), 0).rgb;
      let c101 = textureLoad(t, min(i0 + vec3<u32>(1u,0u,1u), mx), 0).rgb;
      r = c000 + f.r * (c100 - c000) + f.b * (c101 - c100) + f.g * (c111 - c101);
    } else {                   // b > r ≥ g
      let c001 = textureLoad(t, min(i0 + vec3<u32>(0u,0u,1u), mx), 0).rgb;
      let c101 = textureLoad(t, min(i0 + vec3<u32>(1u,0u,1u), mx), 0).rgb;
      r = c000 + f.b * (c001 - c000) + f.r * (c101 - c001) + f.g * (c111 - c101);
    }
  } else {
    if (f.b >= f.g) {          // b ≥ g > r
      let c001 = textureLoad(t, min(i0 + vec3<u32>(0u,0u,1u), mx), 0).rgb;
      let c011 = textureLoad(t, min(i0 + vec3<u32>(0u,1u,1u), mx), 0).rgb;
      r = c000 + f.b * (c001 - c000) + f.g * (c011 - c001) + f.r * (c111 - c011);
    } else if (f.b >= f.r) {   // g > b ≥ r
      let c010 = textureLoad(t, min(i0 + vec3<u32>(0u,1u,0u), mx), 0).rgb;
      let c011 = textureLoad(t, min(i0 + vec3<u32>(0u,1u,1u), mx), 0).rgb;
      r = c000 + f.g * (c010 - c000) + f.b * (c011 - c010) + f.r * (c111 - c011);
    } else {                   // g > r > b
      let c010 = textureLoad(t, min(i0 + vec3<u32>(0u,1u,0u), mx), 0).rgb;
      let c110 = textureLoad(t, min(i0 + vec3<u32>(1u,1u,0u), mx), 0).rgb;
      r = c000 + f.g * (c010 - c000) + f.r * (c110 - c010) + f.b * (c111 - c110);
    }
  }
  return r;
}

/// uv del LIENZO → uv de la FUENTE. Una afín y ya: el conform, los cuartos de
/// vuelta, la escala de cada eje, la posición, el giro sobre el ancla y el
/// volteo vienen compuestos de la CPU. Devuelve w<0 cuando cae fuera: eso es
/// el letterbox, y no cuesta ni un filtro.
fn encuadra(uv: vec2<f32>) -> vec3<f32> {
  let f = vec2(P.enc_a.x * uv.x + P.enc_a.y * uv.y + P.enc_b.x,
               P.enc_a.z * uv.x + P.enc_a.w * uv.y + P.enc_b.y);
  let dentro = f.x >= 0.0 && f.y >= 0.0 && f.x <= 1.0 && f.y <= 1.0;
  return vec3(f, select(-1.0, 1.0, dentro));
}

/// un tap, ya en YUV **normalizado a 0..1** (no en códigos).
///
/// `textureSampleLevel` y no `textureSample`: el segundo necesita derivadas
/// («gradient instruction») y eso, dentro de un bucle, obliga al compilador de
/// D3D a desenrollarlo. Con un tope dinámico no puede y **el motor de Windows
/// no arranca** (X3570 → X3511). Estas texturas no tienen mipmaps: el nivel es
/// siempre el 0, así que no se pierde nada.
///
/// EL CROMA VA SENTADO donde diga `croma_x/croma_y`: el 4:2:0 de cámara está
/// sentado a la IZQUIERDA, no centrado, y media muestra de croma se ve como
/// una franja de color en los bordes verticales duros.
fn tap(uvs: vec2<f32>) -> vec3<f32> {
  let y = textureSampleLevel(tY, samp, uvs, 0.0).r;
  // el desplazamiento va en téxeles de CROMA, que miden dos de luma
  let dc = vec2(P.croma_x * 2.0 / max(P.src_w, 1.0),
                P.croma_y * 2.0 / max(P.src_h, 1.0));
  let c = textureSampleLevel(tUV, samp, uvs + dc, 0.0).rg;
  return vec3(y, c.r, c.g);
}

/// EL FILTRO DE REDUCCIÓN. Al agrandar basta el bilineal del muestreador; al
/// reducir —y se reduce SIEMPRE, porque conformar 4K a 1080 ya es reducir a la
/// mitad— un solo tap se salta píxeles. Se promedia una rejilla de nx×ny
/// muestras repartidas por la huella real del píxel de salida, y cuántas hay
/// que tomar lo dice la CPU, que conoce la matriz exacta (§1.5).
///
/// EL TOPE ES CONSTANTE (6) y las muestras de más se saltan. Con el tope
/// dinámico, D3D intenta desenrollar «874 iteraciones» y se rinde. Seis y no
/// cuatro porque cuatro topaba en 4:1 y un 8K a 1080 es 7,4:1 — ahí volvía el
/// hormigueo. Por encima de 6:1 sigue faltando, y la respuesta de verdad no es
/// ensanchar más la rejilla sino un pase de prefiltrado a media resolución.
/// No está hecho, y se dice.
fn muestrea(f: vec2<f32>) -> vec3<f32> {
  let nx = i32(P.enc_b.z);
  let ny = i32(P.enc_b.w);
  if (nx <= 1 && ny <= 1) {
    return tap(vec2(f.x, f.y * P.yuv_norm));
  }
  let px = P.paso.xy;
  let py = P.paso.zw;
  var acc = vec3(0.0);
  var n = 0.0;
  for (var i = 0; i < 6; i = i + 1) {
    if (i >= nx) { continue; }
    let ox = (f32(i) + 0.5) / f32(nx) - 0.5;
    for (var j = 0; j < 6; j = j + 1) {
      if (j >= ny) { continue; }
      let oy = (f32(j) + 0.5) / f32(ny) - 0.5;
      let u = f + px * ox + py * oy;
      acc = acc + tap(vec2(u.x, u.y * P.yuv_norm));
      n = n + 1.0;
    }
  }
  return acc / max(n, 1.0);
}

/// EL HOMBRO DE LAS ALTAS LUCES.
///
/// Exponencial: tangente a la recta en el umbral (así no se ve la juntura) y
/// **con el blanco donde toca**. La versión de antes tenía por techo
/// `thr + range/(1 + c·range/wp)`, estrictamente menor que 1: encender la
/// compresión bajaba el blanco a gris en vez de recuperar detalle.
fn hombro(o: f32, span: f32, dureza: f32) -> f32 {
  return span / dureza * (1.0 - exp(-dureza * o / span));
}

/// EL FILTRO ND, DESHECHO.
///
/// Un ND no es gris del todo, y ensucia de dos maneras que piden dos curas:
///
/// 1. **El tinte plano.** Un ND variable son dos polarizadores cruzados y su
///    extinción depende de la longitud de onda: sale una dominante —casi
///    siempre magenta— parecida en toda la escala. Se quita con una ganancia
///    por canal, como un balance suave.
///
/// 2. **La contaminación de infrarrojos**, que es la fea. El cristal ND corta
///    el visible pero deja pasar el infrarrojo cercano (700–1000 nm), y el
///    sensor sí lo ve; el filtro rojo de la matriz de color es el que más lo
///    deja entrar. Como el término es **aditivo**, sólo se nota donde hay poca
///    luz visible: los negros se van a granate y las telas oscuras sintéticas
///    salen marrones. Cuanto más denso el ND, peor.
///
/// La cura del (2) tiene que quitar el rojo **que sobra** sin tocar el rojo
/// que hay. De ahí las dos guardas:
///
/// - **la de gris**: la contaminación tiñe lo que está cerca del neutro; un
///   rojo de verdad está muy saturado y no se toca;
/// - **la de sombras**: el término es aditivo, así que pesa donde el visible
///   es débil y desaparece en las luces.
///
/// Se aplica sobre la señal de cámara, ANTES de la gelatina de entrada,
/// porque ahí es donde ocurrió la suciedad.
fn corrige_nd(c: vec3<f32>) -> vec3<f32> {
  let fuerza = P.nd.x;
  let tinte = P.nd.y;
  if (fuerza <= 0.001 && abs(tinte) <= 0.001) { return c; }
  var v = c;
  if (abs(tinte) > 0.001) {
    // magenta = rojo y azul de más y verde de menos: se deshace en esa proporción
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
  // el verde es el canal menos contaminado: sirve de referencia
  let sobra_r = max(v.r - v.g, 0.0);
  let sobra_b = max(v.b - v.g, 0.0);
  return vec3(v.r - k * sobra_r, v.g, v.b - k * 0.35 * sobra_b);
}

/// YUV → RGB con la matriz que toque. Estaba clavada a BT.709 y el material
/// BT.2020 —el HDR de cualquier móvil de hoy— salía con los colores torcidos.
fn a_rgb(Y: f32, U: f32, V: f32) -> vec3<f32> {
  if (P.matriz == 1u) {          // BT.2020 no constante
    return vec3(Y + 1.4746 * V, Y - 0.16455 * U - 0.57135 * V, Y + 1.8814 * U);
  }
  if (P.matriz == 2u) {          // BT.601
    return vec3(Y + 1.402 * V, Y - 0.344136 * U - 0.714136 * V, Y + 1.772 * U);
  }
  return vec3(Y + 1.5748 * V, Y - 0.1873 * U - 0.4681 * V, Y + 1.8556 * U);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
  let enc = encuadra(in.uv);
  if (enc.z < 0.0) { return vec4(0.0, 0.0, 0.0, P.peso); }
  let yuv = muestrea(enc.xy);
  // ── DE CÓDIGO A SEÑAL, sin depender de la profundidad de bits ────────
  // La cuenta de antes era `(código − 64) / 876`, que sólo vale si la fuente
  // es de 10 bits: con 8 los desplazamientos no significan nada. En forma
  // normalizada los números son LOS MISMOS a cualquier profundidad.
  var Y: f32; var U: f32; var V: f32;
  if (P.full_range == 1u) {
    Y = yuv.x; U = yuv.y - 0.5; V = yuv.z - 0.5;
  } else {
    Y = (yuv.x - 16.0 / 255.0) / (219.0 / 255.0);
    U = (yuv.y - 128.0 / 255.0) / (224.0 / 255.0);
    V = (yuv.z - 128.0 / 255.0) / (224.0 / 255.0);
  }
  // SIN RECORTAR EL LUMA: de 941 a 1023 hay superblanco legal, y es
  // exactamente el material que el hombro está para recuperar. Recortarlo
  // aquí era tirarlo antes incluso de la matriz.
  var raw = a_rgb(Y, U, V);
  raw = corrige_nd(raw);
  // ── LA DISCIPLINA DE RECORTES ────────────────────────────────────────
  // Sólo por abajo hasta el final. Antes se recortaba a [0,1] al salir del
  // muestreo, otra vez tras la ganancia y otra dentro de cada rama del
  // push/pull: cuando la señal llegaba al compresor ya estaba plana en 1,0 y
  // el hombro no tenía nada que doblar — se limitaba a bajar lo que ya había.
  // Ahora se deja correr por encima de 1 y se recorta UNA vez, justo antes de
  // la gelatina.
  raw = max(raw, vec3(0.0));
  raw = raw * exp2(P.gain);
  let pp = P.push_pull;
  if (abs(pp) > 0.001) {
    raw = max(raw * exp2(pp * 0.7), vec3(0.0));
    raw = pow(raw, vec3(1.0 + pp * 0.10));
    if (pp > 0.0) { raw = mix(raw, vec3(1.0), 0.04 * pp); }
    else { raw = pow(raw, vec3(1.0 - pp * 0.06)); }
  }
  // EL HOMBRO SÓLO TIENE SENTIDO SI HAY ALGO QUE METER. `compress_wp` es
  // ahora **el nivel de entrada que acaba en blanco**: con 1,6 se recoge un
  // stop y medio de superblanco y se deja en 1,0. Con 1,0 no hay margen
  // ninguno, y un hombro sin margen no recorta nada — sólo levanta las luces
  // altas para que el blanco siga siendo blanco, que es un cambio de
  // contraste disfrazado. Ahí se apaga y se dice.
  let margen = P.compress_wp;
  if (P.compress > 0.001 && margen > 1.001) {
    let thr = clamp(1.0 - P.compress_range, 0.0, 0.999);
    let span = max(1.0 - thr, 1e-4);
    let d = max(P.compress, 1e-4);
    // la cabeza de entrada que queremos ver acabar exactamente en blanco
    let cab = max(margen - thr, 1e-4);
    let k = span / max(hombro(cab, span, d), 1e-6);
    let sobra = max(raw - vec3(thr), vec3(0.0));
    raw = min(raw, vec3(thr))
        + vec3(hombro(sobra.r, span, d), hombro(sobra.g, span, d),
               hombro(sobra.b, span, d)) * k;
  }
  raw = clamp(raw, vec3(0.0), vec3(1.0));   // el ÚNICO recorte por arriba
  var graded = raw;
  if (P.lut_a_on == 1u) { graded = clamp(lut3(tLutA, P.lut_na, graded), vec3(0.0), vec3(1.0)); }
  if (P.lut_b_on == 1u) { graded = clamp(lut3(tLutB, P.lut_nb, graded), vec3(0.0), vec3(1.0)); }
  // obturador IIR fusionado: pad0 = feedback, pad1 = reset (1.0 en el frame 0)
  if (P.pad0 > 0.001 && P.pad1 < 0.5) {
    let hist = textureSample(tHist, samp, in.uv).rgb;
    graded = mix(graded, hist, P.pad0);
  }
  // el alfa ES el peso: con mezcla src-alpha, peso 1 sustituye y peso p
  // encadena. No hay «pase de fundido» (MOTOR §5bis).
  return vec4(graded, P.peso);
}
