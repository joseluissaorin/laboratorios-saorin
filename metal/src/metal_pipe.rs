//! Pipeline Metal: device, pipelines, texturas, render de la cadena.

pub use metal::*;
use metal::MTLPixelFormat as MTLPixelFormatMetal_unused;
use std::mem;
use std::path::Path;

#[derive(Clone)]
pub struct Gpu {
    pub device: Device,
    pub queue: CommandQueue,
    pub sampler: SamplerState,
    pub sampler_rep: SamplerState,
    pub sampler_near: SamplerState,
}

#[derive(Clone)]
pub struct Pipelines {
    /// la cadena viene del WGSL del taller (cambia el reparto de texturas
    /// del revelado: el shader traducido no tiene el hueco `tVideo`)
    pub uno: bool,
    pub grade: RenderPipelineState,
    pub down: RenderPipelineState,
    pub blur: RenderPipelineState,
    pub accum: RenderPipelineState,
    pub comp: RenderPipelineState,
    pub pack_y: RenderPipelineState,
    pub pack_uv: RenderPipelineState,
}

fn mk_sampler(device: &Device, repeat: bool, filter: bool) -> SamplerState {
    let d = SamplerDescriptor::new();
    d.set_address_mode_s(if repeat { MTLSamplerAddressMode::Repeat } else { MTLSamplerAddressMode::ClampToEdge });
    d.set_address_mode_t(if repeat { MTLSamplerAddressMode::Repeat } else { MTLSamplerAddressMode::ClampToEdge });
    d.set_address_mode_r(if repeat { MTLSamplerAddressMode::Repeat } else { MTLSamplerAddressMode::ClampToEdge });
    d.set_mag_filter(if filter { MTLSamplerMinMagFilter::Linear } else { MTLSamplerMinMagFilter::Nearest });
    d.set_min_filter(if filter { MTLSamplerMinMagFilter::Linear } else { MTLSamplerMinMagFilter::Nearest });
    d.set_mip_filter(if filter { MTLSamplerMipFilter::Linear } else { MTLSamplerMipFilter::Nearest });
    device.new_sampler(&d)
}

impl Gpu {
    pub fn new() -> Self {
        let device = Device::system_default().expect("sin GPU Metal");
        let queue = device.new_command_queue();
        let sampler = mk_sampler(&device, false, true);
        let sampler_rep = mk_sampler(&device, true, true);
        let sampler_near = mk_sampler(&device, false, false);
        Gpu { device, queue, sampler, sampler_rep, sampler_near }
    }
}

fn pipe(device: &Device, lib: &Library, frag: &str, targets: &[MTLPixelFormat]) -> RenderPipelineState {
    pipe_mezcla(device, lib, frag, targets, false)
}

/// `mezcla` enciende el encadenado por alfa en el primer destino: el segundo
/// lado de una junta se dibuja encima con su peso y ya está. Es todo el coste
/// de un fundido en este motor.
fn pipe_mezcla(device: &Device, lib: &Library, frag: &str, targets: &[MTLPixelFormat],
               mezcla: bool) -> RenderPipelineState {
    let d = RenderPipelineDescriptor::new();
    // el vértice de la casa se llama `vs_full`; el que sale traducido del
    // WGSL, `vs_main` — cada biblioteca trae el suyo
    let vs = lib.get_function("vs_full", None)
        .or_else(|_| lib.get_function("vs_main", None))
        .expect("la biblioteca no trae vértice");
    d.set_vertex_function(Some(&vs));
    d.set_fragment_function(Some(&lib.get_function(frag, None).unwrap()));
    for (i, fmt) in targets.iter().enumerate() {
        let a = d.color_attachments().object_at(i as u64).unwrap();
        a.set_pixel_format(*fmt);
        if mezcla && i == 0 {
            a.set_blending_enabled(true);
            a.set_source_rgb_blend_factor(MTLBlendFactor::SourceAlpha);
            a.set_destination_rgb_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
            a.set_source_alpha_blend_factor(MTLBlendFactor::One);
            a.set_destination_alpha_blend_factor(MTLBlendFactor::Zero);
        }
    }
    device.new_render_pipeline_state(&d).unwrap()
}

impl Pipelines {
    pub fn new(device: &Device) -> Self {
        let src = include_str!("shaders/chain.metal");
        let opts = CompileOptions::new();
        let lib = device.new_library_with_source(src, &opts)
            .unwrap_or_else(|e| panic!("MSL compile: {e}"));

        // ── EL LOOK ÚNICO (MOTOR §8) ──────────────────────────────────
        // `build.rs` traduce con naga los shaders del taller —los mismos que
        // usan la preview y el motor de Windows— a Metal. Con `FL_LOOK=wgsl`
        // la cadena del máster del Mac sale de ESA fuente y no de la copia en
        // MSL. Cada shader va en su propia biblioteca porque los nombres de
        // entrada (`vs_main`, `fs_main`) se repiten.
        //
        // ES EL CAMINO POR DEFECTO desde que el autor eligió (1-ago-2026):
        // miró los dos granos al 100 % y se quedó con el del taller. Con eso,
        // preview, máster del Mac y máster de Windows pasan a ser la misma
        // imagen — antes se separaban 47 dB.
        //
        // `FL_LOOK=msl` recupera la copia vieja en Metal, por si hiciera
        // falta reproducir un máster antiguo.
        let uno = std::env::var("FL_LOOK").as_deref() != Ok("msl");
        let traducido = |nombre: &str, fuente: &str, fmts: &[MTLPixelFormat],
                         mezcla: bool| -> Option<RenderPipelineState> {
            if !uno || fuente.trim().is_empty() { return None; }
            match device.new_library_with_source(fuente, &opts) {
                Ok(l2) => Some(pipe_mezcla(device, &l2, "fs_main", fmts, mezcla)),
                Err(e) => {
                    eprintln!("   ⚠ «{nombre}» traducido no compila ({e}); sigo con el MSL de la casa");
                    None
                }
            }
        };
        let g_comp  = include_str!(concat!(env!("OUT_DIR"), "/look_comp.metal"));
        let g_down  = include_str!(concat!(env!("OUT_DIR"), "/look_down.metal"));
        let g_blur  = include_str!(concat!(env!("OUT_DIR"), "/look_blur.metal"));
        let g_grade = include_str!(concat!(env!("OUT_DIR"), "/look_grade_bi.metal"));
        let p_comp = traducido("comp", g_comp, &[FMT_INTERMEDIO], false)
            .unwrap_or_else(|| pipe(device, &lib, "fs_comp", &[FMT_INTERMEDIO]));
        let p_down = traducido("down", g_down, &[FMT_PIRAMIDE], false)
            .unwrap_or_else(|| pipe(device, &lib, "fs_down", &[FMT_PIRAMIDE]));
        let p_blur = traducido("blur", g_blur, &[FMT_PIRAMIDE], false)
            .unwrap_or_else(|| pipe(device, &lib, "fs_blur", &[FMT_PIRAMIDE]));
        let p_grade = traducido("grade", g_grade, &[FMT_INTERMEDIO], true)
            .unwrap_or_else(|| pipe_mezcla(device, &lib, "fs_grade_solo", &[FMT_INTERMEDIO], true));
        if uno {
            eprintln!("   look: el del taller (WGSL), el mismo que la preview y Windows");
        }
        Pipelines {
            uno,
            grade: p_grade,
            down: p_down,
            blur: p_blur,
            accum: pipe(device, &lib, "fs_accum", &[FMT_INTERMEDIO]),
            comp: p_comp,
            pack_y: pipe(device, &lib, "fs_pack_y", &[MTLPixelFormat::R16Unorm]),
            pack_uv: pipe(device, &lib, "fs_pack_uv", &[MTLPixelFormat::RG16Unorm]),
        }
    }
}

pub struct TargetSet {
    pub graded: Texture,
    pub h_a: Texture,
    pub h_b: Texture,
    pub b0: Texture,
    pub b1: Texture,
    pub c0: Texture,
    pub c1: Texture,
    pub d0: Texture,
    pub d1: Texture,
    pub out: Texture,
    pub w: u64,
    pub h: u64,
}

/// EL FORMATO DE LOS INTERMEDIOS, decidido midiendo (MOTOR §2).
///
/// `RGBA16Float` gasta 8 bytes por píxel y el alfa no lo usa nadie: en 4K son
/// 66 MB por búfer y ~465 MB de ida y vuelta a memoria por fotograma. La
/// dieta era obligada; lo que no estaba claro era a qué formato.
///
///   · `RG11B10Float` (4 bytes) — lo que proponía el plan. **No sirve**:
///     6 bits de mantisa son ~1,6 % de error relativo y el máster se separa
///     43 dB de la referencia (unos 16 valores de código sobre 1023). Ni
///     siquiera vale solo para la pirámide: con la halación de la casa (1.5)
///     se queda en 54 dB.
///   · `RGB10A2Unorm` (4 bytes) — **este**. Medido: 59,7 dB, ~1 valor de
///     código. Después de las LUT toda la señal vive en [0,1] y el grano del
///     comp ditherea lo que quede. Mitad de tráfico por 1 LSB.
///
/// Y no es una elección nueva: **el motor de Windows ya usaba 10 bits sin
/// signo** mientras el del Mac iba en 16F. O sea que los dos másteres NO eran
/// el mismo fotograma. Igualarlos aquí quita una divergencia en vez de
/// crearla (MOTOR §8: un solo look).
pub const FMT_INTERMEDIO: MTLPixelFormat = MTLPixelFormat::RGB10A2Unorm;

/// la pirámide de desenfoques, en el mismo formato
pub const FMT_PIRAMIDE: MTLPixelFormat = MTLPixelFormat::RGB10A2Unorm;

fn tex(device: &Device, w: u64, h: u64, fmt: MTLPixelFormat) -> Texture {
    let d = TextureDescriptor::new();
    d.set_width(w);
    d.set_height(h);
    d.set_pixel_format(fmt);
    d.set_usage(MTLTextureUsage::Unknown.union(MTLTextureUsage::RenderTarget).union(MTLTextureUsage::ShaderRead));
    d.set_storage_mode(MTLStorageMode::Private);
    device.new_texture(&d)
}

impl TargetSet {
    pub fn new(device: &Device, w: u64, h: u64, out_fmt: MTLPixelFormat) -> Self {
        let t = |dw: u64, dh: u64| tex(device, w / dw, h / dh, FMT_INTERMEDIO);
        let p = |dw: u64, dh: u64| tex(device, w / dw, h / dh, FMT_PIRAMIDE);
        TargetSet {
            graded: t(1, 1), h_a: t(1, 1), h_b: t(1, 1),
            b0: p(2, 2), b1: p(2, 2), c0: p(4, 4), c1: p(4, 4), d0: p(8, 8), d1: p(8, 8),
            out: tex(device, w, h, out_fmt),
            w, h,
        }
    }
}

pub fn make_3d_lut(device: &Device, n: u64, data: &[f32]) -> Texture {
    let d = TextureDescriptor::new();
    d.set_texture_type(MTLTextureType::D3);
    d.set_width(n);
    d.set_height(n);
    d.set_depth(n);
    d.set_pixel_format(MTLPixelFormat::RGBA32Float);
    d.set_usage(MTLTextureUsage::ShaderRead);
    d.set_storage_mode(MTLStorageMode::Shared);
    let tex = device.new_texture(&d);
    let mut rgba = Vec::with_capacity(data.len() / 3 * 4);
    for c in data.chunks(3) { rgba.extend_from_slice(&[c[0], c[1], c[2], 1.0]); }
    tex.replace_region_in_slice(
        MTLRegion::new_3d(0, 0, 0, n, n, n),
        0,
        0,
        rgba.as_ptr() as *const _,
        n * 16,
        n * n * 16,
    );
    tex
}

/// UNA FOTO RESIDENTE (PENDIENTE §4bis.10). Los dos planos de una imagen
/// subidos UNA vez: a partir de ahí el motor la trata como cualquier otra
/// fuente y la bobina con fotos o rótulos deja de caer al camino viejo (que
/// funcionaba, pero era tres veces más lento).
pub fn foto_residente(device: &Device, path: &Path) -> anyhow::Result<(Texture, Texture, u32, u32)> {
    let (w, h, y, uv) = crate::foto::planos(path)?;
    let mk = |fmt: MTLPixelFormat, tw: u64, th: u64| -> Texture {
        let d = TextureDescriptor::new();
        d.set_width(tw);
        d.set_height(th);
        d.set_pixel_format(fmt);
        d.set_usage(MTLTextureUsage::ShaderRead);
        d.set_storage_mode(MTLStorageMode::Shared);
        device.new_texture(&d)
    };
    let ty = mk(MTLPixelFormat::R16Unorm, w as u64, h as u64);
    ty.replace_region(MTLRegion::new_2d(0, 0, w as u64, h as u64), 0,
                      y.as_ptr() as *const _, w as u64 * 2);
    let tuv = mk(MTLPixelFormat::RG16Unorm, (w / 2) as u64, (h / 2) as u64);
    tuv.replace_region(MTLRegion::new_2d(0, 0, (w / 2) as u64, (h / 2) as u64), 0,
                       uv.as_ptr() as *const _, (w / 2) as u64 * 4);
    Ok((ty, tuv, w, h))
}

pub fn make_grain(gpu: &Gpu, path: &Path) -> Texture {
    let raw = std::fs::read(path).expect("grain.bin");
    let d = TextureDescriptor::new();
    d.set_width(1024);
    d.set_height(1024);
    d.set_pixel_format(MTLPixelFormat::R16Float);
    d.set_usage(MTLTextureUsage::ShaderRead.union(MTLTextureUsage::RenderTarget));
    d.set_storage_mode(MTLStorageMode::Shared);
    d.set_mipmap_level_count(5);
    let tex = gpu.device.new_texture(&d);
    tex.replace_region(MTLRegion::new_2d(0, 0, 1024, 1024), 0, raw.as_ptr() as *const _, 1024 * 2);
    // los mips se muestrean con level(lod) en fs_comp: hay que generarlos
    let cmd = gpu.queue.new_command_buffer();
    let blit = cmd.new_blit_command_encoder();
    blit.generate_mipmaps(&tex);
    blit.end_encoding();
    cmd.commit();
    cmd.wait_until_completed();
    tex
}

pub struct PassEncoder<'a> {
    pub enc: &'a RenderCommandEncoderRef,
}

/// Vuelca una textura a PPM/PGM 8-bit para depurar (lento, solo debug).
pub fn dump_texture(gpu: &Gpu, tex: &Texture, path: &str) {
    let (w, h) = (tex.width(), tex.height());
    let fmt = tex.pixel_format();
    let bpp: u64 = match fmt {
        MTLPixelFormat::R8Unorm => 1,
        MTLPixelFormat::RG8Unorm => 2,
        MTLPixelFormat::R16Unorm | MTLPixelFormat::RG16Unorm => 2,
        MTLPixelFormat::RGBA16Float => 8,
        MTLPixelFormat::RG11B10Float => 4,
        MTLPixelFormat::RGB10A2Unorm => 4,
        _ => { eprintln!("dump: formato no soportado {:?}", fmt); return; }
    };
    let bpr = w * bpp * if fmt == MTLPixelFormat::RG16Unorm { 2 } else { 1 };
    let buf = gpu.device.new_buffer(bpr * h, MTLResourceOptions::StorageModeShared);
    let cmd = gpu.queue.new_command_buffer();
    let blit = cmd.new_blit_command_encoder();
    blit.copy_from_texture_to_buffer(
        tex, 0, 0, MTLOrigin { x: 0, y: 0, z: 0 }, MTLSize { width: w, height: h, depth: 1 },
        &buf, 0, bpr, 0, MTLBlitOption::empty());
    blit.end_encoding();
    cmd.commit();
    cmd.wait_until_completed();
    let data = unsafe { std::slice::from_raw_parts(buf.contents() as *const u8, (bpr * h) as usize) };
    let f16 = |lo: u8, hi: u8| -> f32 {
        let bits = u16::from_le_bytes([lo, hi]);
        let s = if bits >> 15 == 1 { -1.0f32 } else { 1.0 };
        let e = ((bits >> 10) & 0x1f) as i32;
        let m = (bits & 0x3ff) as f32;
        if e == 0 { s * m / 1024.0 * (2.0f32).powi(-14) }
        else { s * (1.0 + m / 1024.0) * (2.0f32).powi(e - 15) }
    };
    let mut out = Vec::new();
    match fmt {
        // 10 bits sin signo empaquetados en un u32: R en los bits bajos
        MTLPixelFormat::RGB10A2Unorm => {
            let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
            for f in 0..h as usize {
                for c in 0..w as usize {
                    let o = f * bpr as usize + c * 4;
                    let v = u32::from_le_bytes([data[o], data[o+1], data[o+2], data[o+3]]);
                    for k in 0..3 {
                        let d10 = (v >> (10 * k)) & 0x3ff;
                        ppm.push((d10 as f32 / 1023.0 * 255.0).round().clamp(0.0, 255.0) as u8);
                    }
                }
            }
            let _ = std::fs::write(path, ppm);
            return;
        }
        MTLPixelFormat::RGBA16Float => {
            out.extend_from_slice(format!("P6\n{} {}\n255\n", w, h).as_bytes());
            for px in data.chunks(8) {
                for c in 0..3 {
                    let v = f16(px[c * 2], px[c * 2 + 1]).clamp(0.0, 1.0);
                    out.push((v * 255.0) as u8);
                }
            }
        }
        MTLPixelFormat::R16Unorm => {
            out.extend_from_slice(format!("P5\n{} {}\n255\n", w, h).as_bytes());
            for px in data.chunks(2) { out.push(px[1]); }
        }
        MTLPixelFormat::R8Unorm => {
            out.extend_from_slice(format!("P5\n{} {}\n255\n", w, h).as_bytes());
            out.extend_from_slice(data);
        }
        MTLPixelFormat::RG8Unorm | MTLPixelFormat::RG16Unorm => {
            out.extend_from_slice(format!("P6\n{} {}\n255\n", w, h).as_bytes());
            let step = if fmt == MTLPixelFormat::RG8Unorm { 2 } else { 4 };
            for px in data.chunks(step) {
                if fmt == MTLPixelFormat::RG8Unorm { out.extend_from_slice(&[px[0], px[1], 128]); }
                else { out.extend_from_slice(&[px[1], px[3], 128]); }
            }
        }
        _ => unreachable!(),
    }
    std::fs::write(path, out).unwrap();
    eprintln!("   dump {} ({}x{} {:?})", path, w, h, fmt);
}

pub fn render_pass<'a>(cmd: &'a CommandBufferRef, targets: &[&Texture]) -> &'a RenderCommandEncoderRef {
    let d = RenderPassDescriptor::new();
    for (i, t) in targets.iter().enumerate() {
        let a = d.color_attachments().object_at(i as u64).unwrap();
        a.set_texture(Some(t));
        a.set_load_action(MTLLoadAction::Load);
        a.set_store_action(MTLStoreAction::Store);
    }
    cmd.new_render_command_encoder(d)
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GradeParams {
    pub src_mode: u32, pub full_range: u32, pub lut_na: u32, pub lut_nb: u32,
    pub lut_a_on: u32, pub lut_b_on: u32,
    pub yuv_norm: f32, pub gain: f32, pub push_pull: f32, pub compress: f32,
    pub compress_wp: f32, pub compress_range: f32,
    pub src_w: f32, pub src_h: f32, pub pad0: f32, pub pad1: f32,
    /// EL ENCUADRE: la afín completa de uv-lienzo a uv-fuente (conform,
    /// cuartos de vuelta, escala por eje, posición, giro sobre el ancla y
    /// volteo), más cuántas muestras hay que tomar y su separación — el
    /// filtro de reducción (MOTOR §5, PENDIENTE §1.5). Lo que cae fuera sale
    /// negro: el letterbox gratis.
    pub enc_a: [f32; 4], pub enc_b: [f32; 4], pub paso: [f32; 4],
    /// cuánto pesa este pase sobre el destino. 1 = escribe; <1 = encadena
    /// con lo que ya hay. **Aquí está el fundido entero** (MOTOR §5bis).
    pub peso: f32,
    /// qué matriz YUV→RGB: 0 = BT.709, 1 = BT.2020, 2 = BT.601
    pub matriz: u32,
    /// dónde va sentado el croma respecto al luma, en téxeles de croma
    pub croma_x: f32, pub croma_y: f32,
    /// el filtro ND: fuerza, tinte plano, perfil de sombras, guarda de gris
    pub nd: [f32; 4],
}

impl Default for GradeParams {
    fn default() -> Self {
        GradeParams {
            src_mode: 0, full_range: 0, lut_na: 2, lut_nb: 2, lut_a_on: 0, lut_b_on: 0,
            yuv_norm: 1.0, gain: 0.0, push_pull: 0.0, compress: 0.0,
            compress_wp: 1.0, compress_range: 0.5, src_w: 0.0, src_h: 0.0,
            pad0: 0.0, pad1: 0.0,
            // identidad: uv de lienzo = uv de fuente, una muestra por píxel
            enc_a: [1.0, 0.0, 0.0, 1.0], enc_b: [0.0, 0.0, 1.0, 1.0],
            paso: [0.0; 4],
            peso: 1.0, matriz: 0, croma_x: -0.5, croma_y: 0.0, nd: [0.0; 4],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CompParams {
    pub time: f32, pub seed: f32, pub wipe: f32, pub res_w: f32,
    pub res_h: f32, pub texel_x: f32, pub texel_y: f32,
    pub weave_px_x: f32, pub weave_px_y: f32, pub weave_rot: f32,
    pub hal_amount: f32, pub hal_hue: f32, pub hal_sat: f32, pub hal_thr: f32,
    pub hal_spread: f32, pub hal_white: f32,
    pub bloom_amount: f32, pub bloom_thr: f32, pub bloom_warm: f32,
    pub softness: f32, pub acutance: f32, pub color_sep: f32,
    pub hue_skew: f32, pub crosstalk: f32, pub subtractive: f32, pub stock_sat: f32, pub print_: f32,
    pub grain_amount: f32, pub grain_size: f32, pub grain_rough: f32, pub grain_chroma: f32,
    pub grain_defocus: f32,
    pub grain_s: f32, pub grain_m: f32, pub grain_h: f32, pub grain_r: f32, pub grain_b: f32,
    pub film_res: f32, pub plate_n: f32,
    pub vig_amount: f32, pub vig_size: f32, pub vig_round: f32, pub vig_cx: f32, pub vig_cy: f32,
    pub ca: f32,
    pub dust: f32, pub flicker: f32, pub flicker_rate: f32, pub breath: f32, pub breath_rate: f32,
    pub frame_inset: f32, pub frame_corner: f32, pub frame_wobble: f32,
    /// la LUPA cuentahílos. Aquí no se usa (es de la preview), pero el hueco
    /// TIENE que estar: la estructura del taller la lleva, y si el Mac se la
    /// salta, todo lo que va detrás cae tres huecos antes y el shader
    /// traducido del WGSL lee basura (medido: 10 dB en vez de coincidir).
    pub lupa: f32, pub lupa_cx: f32, pub lupa_cy: f32,
    /// EL FUNDIDO A COLOR, al final de todo. Un fundido a negro se hace sobre
    /// la COPIA, no sobre el negativo: aquí oscurece la imagen ya revelada,
    /// con su grano y su halación, que es lo que pasa en un laboratorio. Y no
    /// necesita segunda fuente ni segundo decodificador — es una constante.
    pub fundido: f32, pub fundido_color: f32, pub pad_f: f32, pub pad_g: f32,
}

pub struct Renderer {
    pub gpu: Gpu,
    pub pipes: Pipelines,
    pub targets: TargetSet,
    pub grade_params: GradeParams,
    pub comp_params: CompParams,
    pub lut_a: Texture,
    pub lut_b: Texture,
    pub grain: Texture,
    pub shutter: f32,
    pub weave_amount: f32,
    /// el 1×1 blanco que ata el hueco de la capa RGBA cuando no hay capa
    pub blanco: Texture,
}

/// el 1×1 RGBA de repuesto (blanco opaco): Metal quiere todos los huecos
/// atados aunque el shader no los lea
pub fn tex_blanca(device: &Device) -> Texture {
    let d = TextureDescriptor::new();
    d.set_width(1);
    d.set_height(1);
    d.set_pixel_format(MTLPixelFormat::RGBA8Unorm);
    d.set_usage(MTLTextureUsage::ShaderRead);
    d.set_storage_mode(MTLStorageMode::Shared);
    let t = device.new_texture(&d);
    t.replace_region(MTLRegion::new_2d(0, 0, 1, 1), 0,
                     [255u8, 255, 255, 255].as_ptr() as *const _, 4);
    t
}

/// UNA CAPA RGBA RESIDENTE (CAPAS §5): la foto o el rótulo con su alfa,
/// subidos una vez
pub fn capa_rgba(device: &Device, ruta: &std::path::Path)
    -> anyhow::Result<Texture>
{
    // el crate del motor lleva `image` directo (plan.rs entra por ruta, no
    // como crate): la misma cuenta que foto::rgba, aquí mismo
    let img = image::open(ruta)?.to_rgba8();
    let (w, h) = (img.width(), img.height());
    let datos = img.into_raw();
    let d = TextureDescriptor::new();
    d.set_width(w as u64);
    d.set_height(h as u64);
    d.set_pixel_format(MTLPixelFormat::RGBA8Unorm);
    d.set_usage(MTLTextureUsage::ShaderRead);
    d.set_storage_mode(MTLStorageMode::Shared);
    let t = device.new_texture(&d);
    t.replace_region(MTLRegion::new_2d(0, 0, w as u64, h as u64), 0,
                     datos.as_ptr() as *const _, (w * 4) as u64);
    Ok(t)
}

impl Renderer {
    pub fn pack_into(&self, cmd: &CommandBufferRef, src: &Texture, y_plane: &Texture, uv_plane: &Texture) {
        {
            let enc = render_pass(cmd, &[y_plane]);
            enc.set_render_pipeline_state(&self.pipes.pack_y);
            enc.set_fragment_texture(0, Some(src));
            enc.set_fragment_sampler_state(0, Some(&self.gpu.sampler));
            enc.draw_primitives(MTLPrimitiveType::Triangle, 0, 3);
            enc.end_encoding();
        }
        {
            let enc = render_pass(cmd, &[uv_plane]);
            enc.set_render_pipeline_state(&self.pipes.pack_uv);
            enc.set_fragment_texture(0, Some(src));
            enc.set_fragment_sampler_state(0, Some(&self.gpu.sampler));
            enc.draw_primitives(MTLPrimitiveType::Triangle, 0, 3);
            enc.end_encoding();
        }
    }

    /// EL PASE DE REVELADO de UNA fuente sobre el lienzo. Con `peso = 1`
    /// escribe; con `peso < 1` se encadena sobre lo que ya haya. Llamarlo dos
    /// veces con dos fuentes y sus recetas ES el fundido entero (MOTOR §5bis):
    /// el pase caro —pirámide, halación, grano— corre UNA sola vez después,
    /// sobre la imagen ya mezclada, que además es como se revela de verdad
    /// una copia con doble exposición.
    pub fn revela_en(&self, cmd: &CommandBufferRef, t_y: &Texture, t_uv: &Texture,
                     params: &GradeParams, lut_a: &Texture, lut_b: &Texture) {
        self.revela_capa(cmd, t_y, t_uv, params, lut_a, lut_b, None)
    }

    /// el mismo pase, con la textura RGBA de una capa (CAPAS §5). El camino
    /// sin capa le ata un 1×1 de repuesto: el shader no la lee con
    /// src_mode < 3, pero Metal quiere TODOS los huecos atados.
    pub fn revela_capa(&self, cmd: &CommandBufferRef, t_y: &Texture, t_uv: &Texture,
                       params: &GradeParams, lut_a: &Texture, lut_b: &Texture,
                       rgba: Option<&Texture>) {
        let gpu = &self.gpu;
        // con obturador el revelado escribe DIRECTAMENTE la historia nueva
        // (h_b) leyendo la vieja (h_a): un pase menos y un búfer 4K menos de
        // ida y vuelta. Sin obturador, al lienzo de siempre.
        let usa = params.pad0 > 0.001;
        let destino = if usa { &self.targets.h_b } else { &self.targets.graded };
        let enc = render_pass(cmd, &[destino]);
        enc.set_render_pipeline_state(&self.pipes.grade);
        enc.set_fragment_bytes(0, mem::size_of::<GradeParams>() as u64,
                               params as *const _ as *const _);
        enc.set_fragment_texture(0, Some(t_y));
        enc.set_fragment_texture(1, Some(t_uv));
        let rgba = rgba.unwrap_or(&self.blanco);
        if self.pipes.uno {
            // el revelado traducido del WGSL es BIPLANAR puro: no tiene el
            // hueco `tVideo` que sí lleva el MSL de la casa, así que todo lo
            // que va detrás sube un puesto. Con la capa, el reparto por orden
            // de binding es tY, tUV, lutA, lutB, hist, RGBA.
            enc.set_fragment_texture(2, Some(lut_a));
            enc.set_fragment_texture(3, Some(lut_b));
            enc.set_fragment_texture(4, Some(&self.targets.h_a));
            enc.set_fragment_texture(5, Some(rgba));
        } else {
            enc.set_fragment_texture(2, Some(rgba)); // tVideo: la capa RGBA
            enc.set_fragment_texture(3, Some(lut_a));
            enc.set_fragment_texture(4, Some(lut_b));
            enc.set_fragment_texture(5, Some(&self.targets.h_a));
        }
        enc.set_fragment_sampler_state(0, Some(&gpu.sampler));
        enc.set_fragment_sampler_state(1, Some(&gpu.sampler_near));
        enc.draw_primitives(MTLPrimitiveType::Triangle, 0, 3);
        enc.end_encoding();
    }

    /// tras revelar (y encadenar, si había junta): la historia nueva pasa a
    /// ser la vieja. Va aparte porque el intercambio ocurre UNA vez por
    /// fotograma, no una por fuente.
    pub fn cierra_obturador(&mut self, usa: bool) {
        if usa { mem::swap(&mut self.targets.h_a, &mut self.targets.h_b); }
    }

    /// El resto de la cadena sobre lo que haya en `graded`: obturador,
    /// pirámide, composición y empaquetado a los planos del codificador.
    pub fn compone(&mut self, cmd: &CommandBufferRef, frame_idx: usize, fps: f64,
                   pack: Option<(&Texture, &Texture)>) {
        let gpu = &self.gpu;
        let hal_spread = self.comp_params.hal_spread;

        // el obturador ya no es un pase: viene fundido en el revelado
        let use_shutter = self.shutter > 0.001;
        let base = if use_shutter { self.targets.h_a.clone() } else { self.targets.graded.clone() };

        // downs
        let down_pass = |cmd: &CommandBufferRef, src: &Texture, dst: &Texture| {
            #[repr(C)]
            struct D { texel: [f32; 2], pad: [f32; 2] }
            let d = D { texel: [1.0 / src.width() as f32, 1.0 / src.height() as f32], pad: [0.0; 2] };
            let enc = render_pass(cmd, &[dst]);
            enc.set_render_pipeline_state(&self.pipes.down);
            enc.set_fragment_bytes(0, mem::size_of::<D>() as u64, &d as *const _ as *const _);
            enc.set_fragment_texture(0, Some(src));
            enc.set_fragment_sampler_state(0, Some(&gpu.sampler));
            enc.draw_primitives(MTLPrimitiveType::Triangle, 0, 3);
            enc.end_encoding();
        };
        down_pass(cmd, &base, &self.targets.b0);
        down_pass(cmd, &self.targets.b0.clone(), &self.targets.c0);
        down_pass(cmd, &self.targets.c0.clone(), &self.targets.d0);

        // blurs
        let blur_pass = |cmd: &CommandBufferRef, src: &Texture, dst: &Texture, rad: f32, horizontal: bool| {
            #[repr(C)]
            struct B { dir: [f32; 2], radius: f32, pad: f32 }
            let b = B {
                dir: if horizontal { [rad / src.width() as f32, 0.0] } else { [0.0, rad / src.height() as f32] },
                radius: 1.0, pad: 0.0,
            };
            let enc = render_pass(cmd, &[dst]);
            enc.set_render_pipeline_state(&self.pipes.blur);
            enc.set_fragment_bytes(0, mem::size_of::<B>() as u64, &b as *const _ as *const _);
            enc.set_fragment_texture(0, Some(src));
            enc.set_fragment_sampler_state(0, Some(&gpu.sampler));
            enc.draw_primitives(MTLPrimitiveType::Triangle, 0, 3);
            enc.end_encoding();
        };
        let blur2 = |cmd: &CommandBufferRef, a: &Texture, b: &Texture, rad: f32| {
            blur_pass(cmd, a, b, rad, true);
            blur_pass(cmd, b, a, rad, false);
        };
        blur2(cmd, &self.targets.b0.clone(), &self.targets.b1.clone(), 7.0);
        blur2(cmd, &self.targets.c0.clone(), &self.targets.c1.clone(), 1.5 + hal_spread * 2.0);
        blur2(cmd, &self.targets.d0.clone(), &self.targets.d1.clone(), 4.0 + hal_spread * 6.0);

        // comp → out
        {
            let time = frame_idx as f32 / fps as f32;
            let mut cp = self.comp_params;
            cp.time = time;
            cp.seed = (frame_idx % 997) as f32;
            cp.res_w = self.targets.w as f32;
            cp.res_h = self.targets.h as f32;
            cp.texel_x = 1.0 / self.targets.w as f32;
            cp.texel_y = 1.0 / self.targets.h as f32;
            let wamp = self.weave_amount * 2.5;
            let wr = 0.4 + 0.5 * 2.0;
            cp.weave_px_x = wamp * ((time * wr * 1.7).sin() + 0.5 * (time * wr * 3.1 + 1.3).sin()) / 1.5;
            cp.weave_px_y = wamp * ((time * wr * 2.3 + 0.7).sin() + 0.5 * (time * wr * 4.3 + 2.1).sin()) / 1.5;
            let enc = render_pass(cmd, &[&self.targets.out]);
            enc.set_render_pipeline_state(&self.pipes.comp);
            enc.set_fragment_bytes(0, mem::size_of::<CompParams>() as u64, &cp as *const _ as *const _);
            enc.set_fragment_texture(0, Some(&base));
            // tRaw: solo lo lee la cortinilla del comparador (wipe<1), que en el
            // máster no existe. Se ata `graded` para no tener un búfer entero de más.
            enc.set_fragment_texture(1, Some(&self.targets.graded));
            enc.set_fragment_texture(2, Some(&self.targets.b0));
            enc.set_fragment_texture(3, Some(&self.targets.c0));
            enc.set_fragment_texture(4, Some(&self.targets.d0));
            enc.set_fragment_texture(5, Some(&self.grain));
            enc.set_fragment_sampler_state(0, Some(&gpu.sampler));
            enc.set_fragment_sampler_state(1, Some(&gpu.sampler_rep));
            enc.draw_primitives(MTLPrimitiveType::Triangle, 0, 3);
            enc.end_encoding();
        }
        if let Some((py, puv)) = pack {
            self.pack_into(cmd, &self.targets.out.clone(), py, puv);
        }
    }

}
