//! Uniforms del pipeline construidos desde el JSON de prefs del laboratorio.
//! Los usa el binario `preview`; `main.rs` mantiene su copia inline (unificar
//! cuando toque el camino de render).

use serde_json::Value;

/// EL TAMAÑO MÍNIMO DEL BÚFER de un uniforme, tal y como lo pide WGSL: el
/// tamaño de la estructura redondeado a múltiplo de 16.
///
/// Esto existe porque el número se escribía **a mano en cinco sitios** (la
/// preview, el motor de Windows y los dos del core) y en la misma sesión
/// mordió dos veces: `GradeU` creció a 128 y seguía declarado 112, `CompU` a
/// 240 y seguía en 224. wgpu no avisa suave — rechaza el pipeline entero y el
/// motor no arranca por ningún camino. Ahora lo cuenta el compilador.
pub const fn bytes_uniforme<T>() -> u64 {
    ((std::mem::size_of::<T>() + 15) / 16 * 16) as u64
}

pub fn f(prefs: &Value, k: &str, d: f64) -> f32 {
    prefs[k].as_f64().unwrap_or(d) as f32
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GradeU {
    pub src_mode: u32, pub full_range: u32, pub lut_na: u32, pub lut_nb: u32,
    pub lut_a_on: u32, pub lut_b_on: u32, pub yuv_norm: f32, pub gain: f32,
    pub push_pull: f32, pub compress: f32, pub compress_wp: f32, pub compress_range: f32,
    pub src_w: f32, pub src_h: f32, pub pad0: f32, pub pad1: f32,
    /// EL ENCUADRE (MOTOR §5, PENDIENTE §1.5): la afín completa que lleva de
    /// uv de LIENZO a uv de FUENTE — conform, cuartos de vuelta, escala en
    /// cada eje, posición, giro sobre el ancla y volteo, todo compuesto en la
    /// CPU. `enc_a = [m00, m01, m10, m11]`, `enc_b.xy = [m02, m12]`.
    /// Lo que cae fuera sale negro, que es el letterbox sin un solo filtro.
    /// Identidad = [1,0,0,1] y [0,0,1,1].
    pub enc_a: [f32; 4],
    /// `.xy` la traslación de la afín · `.zw` CUÁNTAS MUESTRAS por eje (1..4)
    pub enc_b: [f32; 4],
    /// EL FILTRO DE REDUCCIÓN: separación entre muestras, en uv de fuente por
    /// píxel de salida (`[dx.u, dx.v, dy.u, dy.v]`). Conformar 4K a un lienzo
    /// 1080 ya es reducir a la mitad, y un solo tap bilineal se salta píxeles:
    /// eso es el hormigueo de las barandillas, las rejas y el pelo.
    pub paso: [f32; 4],
    /// cuánto pesa este pase sobre el destino: 1 escribe, <1 encadena con lo
    /// que ya haya. **El fundido entero está aquí** (MOTOR §5bis).
    pub peso: f32,
    /// QUÉ MATRIZ YUV→RGB: 0 = BT.709, 1 = BT.2020, 2 = BT.601. Estaba
    /// clavada a 709 y el material 2020 (HDR de móvil, que es justo lo que
    /// graba la casa) salía con los colores torcidos.
    pub matriz: u32,
    /// DÓNDE ESTÁ SENTADO EL CROMA respecto al luma, en téxeles de croma.
    /// El 4:2:0 de difusión va sentado a la IZQUIERDA (`-0.5, 0`); el de
    /// JPEG/MPEG-1 va centrado (`0, 0`). Media muestra de croma se ve como
    /// franja de color en los bordes verticales duros.
    pub croma_x: f32, pub croma_y: f32,
    /// EL FILTRO ND, en cuatro números (ver `nd_u`): fuerza, tinte plano,
    /// perfil de sombras y hasta qué saturación se considera «casi gris».
    pub nd: [f32; 4],
}

/// LOS CUATRO NÚMEROS DEL ND, desde las prefs del cuarto oscuro.
pub fn nd_u(prefs: &Value) -> [f32; 4] {
    [f(prefs, "ndFix", 0.0).clamp(0.0, 1.5),
     f(prefs, "ndTint", 0.0).clamp(-1.0, 1.0),
     f(prefs, "ndShadow", 2.2).clamp(0.2, 8.0),
     f(prefs, "ndGuard", 0.42).clamp(0.02, 1.0)]
}

pub fn grade_u(prefs: &Value, src_w: u32, src_h: u32, lut_na: u32, lut_nb: u32,
               lut_a_on: bool, lut_b_on: bool) -> GradeU {
    GradeU {
        src_mode: 0, full_range: 0,
        lut_na, lut_nb,
        lut_a_on: lut_a_on as u32, lut_b_on: lut_b_on as u32,
        yuv_norm: 1023.0, gain: f(prefs, "gain", 0.0),
        push_pull: f(prefs, "pushPull", 0.0), compress: f(prefs, "compImpact", 0.0),
        compress_wp: f(prefs, "compWP", 1.0), compress_range: f(prefs, "compRange", 0.5),
        src_w: src_w as f32, src_h: src_h as f32, pad0: 0.0, pad1: 0.0,
        // identidad: uv de lienzo = uv de fuente, una muestra por píxel
        enc_a: [1.0, 0.0, 0.0, 1.0], enc_b: [0.0, 0.0, 1.0, 1.0],
        paso: [0.0; 4],
        peso: 1.0,
        matriz: match prefs["matriz"].as_str().unwrap_or("709") {
            "2020" | "bt2020" => 1,
            "601" | "bt601" => 2,
            _ => 0,
        },
        // por defecto, el croma sentado a la izquierda: es lo que traen el
        // H.264 y el HEVC de cámara, que es todo el material de la casa
        croma_x: -0.5, croma_y: 0.0,
        nd: nd_u(prefs),
    }
}

/// grade_u con el ENCUADRE del clip puesto: el conform al lienzo del proyecto
/// y el reencuadre del autor, en la misma matriz que usa el revelado
pub fn grade_u_enc(prefs: &Value, src_w: u32, src_h: u32, lut_na: u32, lut_nb: u32,
                   lut_a_on: bool, lut_b_on: bool,
                   enc: &crate::plan::Encuadre, lienzo_w: u32, lienzo_h: u32) -> GradeU {
    let mut g = grade_u(prefs, src_w, src_h, lut_na, lut_nb, lut_a_on, lut_b_on);
    let (a, b, paso) = crate::plan::matriz(enc, src_w as f32, src_h as f32,
                                           lienzo_w as f32, lienzo_h as f32);
    g.enc_a = a;
    g.enc_b = b;
    g.paso = paso;
    g
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CompU {
    pub time: f32, pub seed: f32, pub wipe: f32, pub res_w: f32,
    pub res_h: f32, pub texel_x: f32, pub texel_y: f32, pub weave_px_x: f32,
    pub weave_px_y: f32, pub weave_rot: f32,
    pub hal_amount: f32, pub hal_hue: f32, pub hal_sat: f32, pub hal_thr: f32,
    pub hal_spread: f32, pub hal_white: f32,
    pub bloom_amount: f32, pub bloom_thr: f32, pub bloom_warm: f32,
    pub softness: f32, pub acutance: f32, pub color_sep: f32,
    pub hue_skew: f32, pub crosstalk: f32, pub subtractive: f32, pub stock_sat: f32, pub print: f32,
    pub grain_amount: f32, pub grain_size: f32, pub grain_rough: f32, pub grain_chroma: f32,
    pub grain_defocus: f32,
    pub grain_s: f32, pub grain_m: f32, pub grain_h: f32, pub grain_r: f32, pub grain_b: f32,
    pub film_res: f32,
    pub plate_n: f32,
    pub vig_amount: f32, pub vig_size: f32, pub vig_round: f32, pub vig_cx: f32, pub vig_cy: f32,
    pub ca: f32,
    pub dust: f32, pub flicker: f32, pub flicker_rate: f32, pub breath: f32, pub breath_rate: f32,
    pub frame_inset: f32, pub frame_corner: f32, pub frame_wobble: f32,
    /// la LUPA cuentahílos: aumento (0 = sin lupa) y su centro en uv
    pub lupa: f32, pub lupa_cx: f32, pub lupa_cy: f32,
    /// EL FUNDIDO A COLOR, al final de todo. Un fundido a negro se hace sobre
    /// la COPIA, no sobre el negativo: aquí oscurece la imagen ya revelada,
    /// con su grano y su halación. No necesita segunda fuente (MOTOR §5bis).
    /// WGSL redondea el tamaño de la estructura a múltiplo de 16, así que el
    /// BÚFER tiene que llegar a 240 bytes: con 236 wgpu rechaza el grupo de
    /// enlace («Binding size 236 … less than minimum 240») y no arranca ni el
    /// camino nuevo ni el viejo. 60 f32 exactos.
    pub fundido: f32, pub fundido_color: f32, pub pad_f: f32, pub pad_g: f32,
}

pub fn comp_u(prefs: &Value, w: u32, h: u32) -> CompU {
    CompU {
        time: 0.0, seed: 0.0, wipe: 1.0, res_w: w as f32,
        res_h: h as f32, texel_x: 1.0 / w as f32, texel_y: 1.0 / h as f32, weave_px_x: 0.0,
        weave_px_y: 0.0, weave_rot: f(prefs, "weaveRot", 0.3),
        hal_amount: f(prefs, "halation", 0.0), hal_hue: f(prefs, "halHue", 1.0),
        hal_sat: f(prefs, "halSat", 0.9), hal_thr: f(prefs, "halThr", 0.5),
        hal_spread: f(prefs, "halSpread", 0.7), hal_white: f(prefs, "halWhite", 0.0),
        bloom_amount: f(prefs, "bloom", 0.0), bloom_thr: f(prefs, "bloomThr", 0.72),
        bloom_warm: f(prefs, "bloomWarm", 0.15),
        softness: f(prefs, "softness", 0.0), acutance: f(prefs, "acutance", 0.0),
        color_sep: f(prefs, "colorSep", 0.0),
        hue_skew: f(prefs, "hueSkew", 1.0), crosstalk: f(prefs, "crosstalk", 0.3),
        subtractive: f(prefs, "subtractive", 0.6), stock_sat: f(prefs, "stockSat", 1.0),
        print: f(prefs, "print", 0.5),
        grain_amount: f(prefs, "grain", 0.0), grain_size: f(prefs, "grainSize", 2.6),
        grain_rough: f(prefs, "grainRough", 0.35), grain_chroma: f(prefs, "grainChroma", 0.25),
        grain_defocus: f(prefs, "grainDefocus", 0.55),
        grain_s: f(prefs, "grainShadows", 0.8), grain_m: f(prefs, "grainMids", 1.0),
        grain_h: f(prefs, "grainHighs", 0.5), grain_r: f(prefs, "grainRed", 1.0),
        grain_b: f(prefs, "grainBlue", 1.25), film_res: f(prefs, "filmRes", 0.5),
        plate_n: 1024.0,
        vig_amount: f(prefs, "vignette", 0.0), vig_size: f(prefs, "vigSize", 0.55),
        vig_round: f(prefs, "vigRound", 1.0), vig_cx: f(prefs, "vigCX", 0.5),
        vig_cy: f(prefs, "vigCY", 0.5), ca: f(prefs, "chroma", 0.0),
        dust: f(prefs, "dust", 0.0), flicker: f(prefs, "flicker", 0.0), flicker_rate: 0.5,
        breath: f(prefs, "breath", 0.0), breath_rate: f(prefs, "breathRate", 0.5),
        frame_inset: f(prefs, "frameInset", 0.0), frame_corner: f(prefs, "frameCorner", 40.0),
        frame_wobble: f(prefs, "frameWobble", 0.5), lupa: 0.0, lupa_cx: 0.5, lupa_cy: 0.5,
        fundido: 0.0, fundido_color: 0.0, pad_f: 0.0, pad_g: 0.0,
    }
}
