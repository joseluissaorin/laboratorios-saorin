//! La cadena fílmica en wgpu para Windows: grade biplanar (P010) → shutter →
//! pirámide → comp (reutiliza los WGSL de core) → pack a planos P010.

use anyhow::Result;
use filmlook_core::params::{self, CompU, GradeU};
use filmlook_core::pipeline::*;

// vs común (idéntico a core: cada pase preserva orientación)
/// el vértice y el revelado biplanar viven en el taller
/// (`core/src/shaders/grade_bi.wgsl`): los usan las DOS máquinas
const GRADE_BI: &str = include_str!("../../core/src/shaders/grade_bi.wgsl");
/// el vértice suelto, para los pases que no traen el suyo
const VS: &str = include_str!("../../core/src/shaders/vs_comun.wgsl");

// grade biplanar: P010 (Y R16Unorm + UV RG16Unorm, video-range 10-bit MSB)


// pack RGB → planos P010 (video-range 10-bit, MSB en unorm16)
const FS_PACK_Y: &str = r#"
@group(0) @binding(0) var tIn: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<u32> {
  let c = textureSample(tIn, samp, in.uv).rgb;
  let code = clamp(round(64.0 + 876.0 * dot(c, vec3(0.2126, 0.7152, 0.0722))), 0.0, 1023.0);
  let v = u32(code) << 6u;    // P010: 10 bits en los altos del u16
  return vec4(v, v, v, 65535u);
}
"#;

const FS_PACK_UV: &str = r#"
@group(0) @binding(0) var tIn: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<u32> {
  let c = textureSample(tIn, samp, in.uv).rgb;
  let cu = clamp(round(512.0 + 896.0 * dot(c, vec3(-0.1146, -0.3854, 0.5))), 0.0, 1023.0);
  let cv = clamp(round(512.0 + 896.0 * dot(c, vec3(0.5, -0.4542, -0.0458))), 0.0, 1023.0);
  return vec4(u32(cu) << 6u, u32(cv) << 6u, 0u, 65535u);
}
"#;

fn mk_target(device: &wgpu::Device, w: u32, h: u32, fmt: wgpu::TextureFormat) -> Target {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: fmt,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = tex.create_view(&Default::default());
    Target { tex, view, w, h }
}

fn ct(fmt: wgpu::TextureFormat) -> Option<wgpu::ColorTargetState> {
    Some(wgpu::ColorTargetState {
        format: fmt,
        blend: Some(wgpu::BlendState::REPLACE),
        write_mask: wgpu::ColorWrites::ALL,
    })
}

/// el destino del REVELADO, con encadenado por alfa: el segundo lado de una
/// junta se dibuja encima con su peso. Es todo el coste de un fundido en este
/// motor (MOTOR §5bis). Con peso 1 se comporta como REPLACE.
fn ct_mezcla(fmt: wgpu::TextureFormat) -> Option<wgpu::ColorTargetState> {
    Some(wgpu::ColorTargetState {
        format: fmt,
        blend: Some(wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        }),
        write_mask: wgpu::ColorWrites::ALL,
    })
}

// la dieta de ancho de banda: intermedios en RG11B10 (la mitad de tráfico);
// graded y la salida RGB se quedan en f16 (precisión pre-grano)
// post-LUT todo vive en display [0,1] y entregamos 10 bits: 10-bit basta
// (el grano de comp además ditherea); la mitad de ancho de banda que 16F
const LIGHT: wgpu::TextureFormat = wgpu::TextureFormat::Rgb10a2Unorm;
const HEAVY: wgpu::TextureFormat = wgpu::TextureFormat::Rgb10a2Unorm;
// 10 bits exactos para historia/salida: la mitad de tráfico que f16
const TEN: wgpu::TextureFormat = wgpu::TextureFormat::Rgb10a2Unorm;

pub struct WinChain {
    pub grade: Pass,
    pub down: Pass,
    pub blur: Pass,
    pub accum: Pass,
    pub comp: Pass,
    pub pack_y: Pass,
    pub pack_uv: Pass,
    pub targets: TargetSet,
    pub out_rgb: Target,
    pub samp: wgpu::Sampler,
    pub samp_rep: wgpu::Sampler,
    pub lut_a: (wgpu::Texture, wgpu::TextureView),
    pub lut_b: (wgpu::Texture, wgpu::TextureView),
    pub grain_view: wgpu::TextureView,
    pub grade_u: GradeU,
    pub comp_u: CompU,
    pub shutter: f32,
    pub weave: f32,
    pub grade_buf: wgpu::Buffer,
    pub comp_buf: wgpu::Buffer,
    pub small_u: wgpu::Buffer,
    // bind groups cacheados: clave = which<<16 | slot<<1 | paridad
    bg_cache: std::collections::HashMap<u64, wgpu::BindGroup>,
    pub w: u32,
    pub h: u32,
}

pub fn parse_cube(path: &str) -> Result<(u32, Vec<f32>)> {
    let text = std::fs::read_to_string(path)?;
    let mut n = 0u32;
    let mut vals = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') { continue; }
        if let Some(rest) = l.strip_prefix("LUT_3D_SIZE") { n = rest.trim().parse()?; continue; }
        if l.chars().next().unwrap().is_alphabetic() { continue; }
        for tok in l.split_whitespace() { vals.push(tok.parse::<f32>()?); }
    }
    anyhow::ensure!(n > 0 && vals.len() == (n * n * n * 3) as usize, "cube inválido");
    Ok((n, vals))
}

impl WinChain {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        w: u32,
        h: u32,
        prefs: &serde_json::Value,
        lut_in: Option<&str>,
        lut: Option<&str>,
        grain_path: &std::path::Path,
    ) -> Result<Self> {
        let grade = make_pass(device, GRADE_BI, &[
uniform_entry(0, params::bytes_uniforme::<params::GradeU>()),
            tex_filter_entry(1), tex_filter_entry(2),
            tex3d_entry(3), tex3d_entry(4),
            sampler_entry(5),
            tex_filter_entry(6),
        ], &[ct_mezcla(TEN)]);
        let down = make_pass(device, include_str!("../../core/src/shaders/down.wgsl"), &[
            uniform_entry(0, 16), tex_filter_entry(1), sampler_entry(2),
        ], &[ct(LIGHT)]);
        let blur = make_pass(device, include_str!("../../core/src/shaders/blur.wgsl"), &[
            uniform_entry(0, 16), tex_filter_entry(1), sampler_entry(2),
        ], &[ct(LIGHT)]);
        let accum = make_pass(device, include_str!("../../core/src/shaders/accum.wgsl"), &[
            uniform_entry(0, 16), tex_filter_entry(1), tex_filter_entry(2), sampler_entry(3),
        ], &[ct(HEAVY)]);
        // ── EL PARCHE DEL COMPUESTO ──────────────────────────────────
        // Windows escribe además el plano Y en un segundo destino, así que
        // aquí el punto de entrada del comp devuelve una ESTRUCTURA y no un
        // color. Se hace parcheando por texto el shader del taller, que es la
        // única fuente de verdad — y eso es frágil: al añadir el tramado, la
        // línea del `return` dejó de casar. La firma sí se cambió y el
        // `return` no, y el módulo salió con los dos tipos peleados. El error
        // que da naga —«el valor devuelto no casa con el de la función»—
        // apunta a la línea del shader compartido y no dice ni una palabra de
        // que aquí hay un parche, así que cuesta media tarde encontrarlo.
        //
        // Por eso ahora **se comprueba que los dos anclajes existan** y, si
        // no, se para aquí con un mensaje que dice exactamente qué pasó.
        const FIRMA: &str = "fn fs_main(in: VsOut) -> @location(0) vec4<f32> {";
        const RETORNO: &str =
            "return vec4<f32>(clamp(col + tram, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);";
        let bruto = include_str!("../../core/src/shaders/comp.wgsl");
        assert!(bruto.contains(FIRMA),
                "el comp.wgsl del taller ha cambiado de FIRMA: el parche de Windows \
                 (que le añade la salida del plano Y) ya no encaja. Mira chain.rs.");
        assert!(bruto.contains(RETORNO),
                "el comp.wgsl del taller ha cambiado su RETURN: el parche de Windows \
                 (que le añade la salida del plano Y) ya no encaja. Mira chain.rs.");
        let comp_src = bruto
            .replace(FIRMA, "fn fs_main(in: VsOut) -> CompOut {")
            .replace(RETORNO,
                     "var o: CompOut;\n  let cf = clamp(col + tram, vec3<f32>(0.0), vec3<f32>(1.0));\n  o.rgb = vec4<f32>(cf, 1.0);\n  let code = clamp(round(64.0 + 876.0 * dot(cf, vec3<f32>(0.2126, 0.7152, 0.0722))), 0.0, 1023.0);\n  o.y = u32(code) << 6u;\n  return o;")
            + "\nstruct CompOut { @location(0) rgb: vec4<f32>, @location(1) y: u32 };\n";
        let comp = make_pass(device, &comp_src, &[
uniform_entry(0, params::bytes_uniforme::<params::CompU>()),
            tex_filter_entry(1), tex_filter_entry(2), tex_filter_entry(3),
            tex_filter_entry(4), tex_filter_entry(5), tex_filter_entry(6),
            sampler_entry(7), sampler_entry(8),
        ], &[ct(TEN), Some(wgpu::ColorTargetState {
            format: wgpu::TextureFormat::R16Uint,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })]);
        let pack_y = make_pass(device, &format!("{VS}{FS_PACK_Y}"), &[
            tex_filter_entry(0), sampler_entry(1),
        ], &[Some(wgpu::ColorTargetState {
            format: wgpu::TextureFormat::R16Uint,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })]);
        let pack_uv = make_pass(device, &format!("{VS}{FS_PACK_UV}"), &[
            tex_filter_entry(0), sampler_entry(1),
        ], &[Some(wgpu::ColorTargetState {
            format: wgpu::TextureFormat::Rg16Uint,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })]);

        let targets = TargetSet {
            graded: mk_target(device, w, h, HEAVY),
            raw: mk_target(device, w, h, LIGHT),
            h_a: mk_target(device, w, h, TEN),
            h_b: mk_target(device, w, h, TEN),
            b0: mk_target(device, w / 2, h / 2, LIGHT),
            b1: mk_target(device, w / 2, h / 2, LIGHT),
            c0: mk_target(device, w / 4, h / 4, LIGHT),
            c1: mk_target(device, w / 4, h / 4, LIGHT),
            d0: mk_target(device, w / 8, h / 8, LIGHT),
            d1: mk_target(device, w / 8, h / 8, LIGHT),
        };
        let out_rgb = mk_target(device, w, h, TEN);
        let samp = make_sampler(device);
        let samp_rep = make_repeat_sampler(device);

        let ident: Vec<f32> = vec![0.,0.,0., 1.,0.,0., 0.,1.,0., 1.,1.,0., 0.,0.,1., 1.,0.,1., 0.,1.,1., 1.,1.,1.];
        let a = lut_in.map(parse_cube).transpose()?;
        let b = lut.map(parse_cube).transpose()?;
        let (na, da) = a.clone().unwrap_or((2, ident.clone()));
        let (nb, db) = b.clone().unwrap_or((2, ident));
        let lut_a = make_3d_lut(device, queue, na, &da);
        let lut_b = make_3d_lut(device, queue, nb, &db);

        // grano con la placa de la casa
        let grain_raw = std::fs::read(grain_path)?;
        let t_grain = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: 1024, height: 1024, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &t_grain, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &grain_raw,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(1024 * 2), rows_per_image: Some(1024) },
            wgpu::Extent3d { width: 1024, height: 1024, depth_or_array_layers: 1 },
        );
        let grain_view = t_grain.create_view(&Default::default());
        std::mem::forget(t_grain);

        let grade_u = params::grade_u(prefs, w, h, na, nb, a.is_some(), b.is_some());
        let comp_u = params::comp_u(prefs, w, h);
        let shutter = params::f(prefs, "shutter", 0.0);
        let weave = params::f(prefs, "weave", 0.0);
        let grade_buf = uniform_buffer(device, bytemuck::bytes_of(&grade_u));
        let comp_buf = uniform_buffer(device, bytemuck::bytes_of(&comp_u));
        let small_u = uniform_buffer(device, &[0u8; 16]);

        Ok(WinChain {
            grade, down, blur, accum, comp, pack_y, pack_uv,
            targets, out_rgb, samp, samp_rep, lut_a, lut_b, grain_view,
            grade_u, comp_u, shutter, weave,
            grade_buf, comp_buf, small_u, w, h,
            bg_cache: Default::default(),
        })
    }

    /// codifica el frame: entra Y/UV importados, sale en los planos out_y/out_uv
    /// EL PASE DE REVELADO de UNA fuente sobre el lienzo, con su receta y su
    /// peso. Con `peso = 1` escribe; con `peso < 1` se encadena sobre lo que
    /// ya haya. Llamarlo dos veces con dos fuentes ES el fundido entero
    /// (MOTOR §5bis): el pase caro corre UNA vez después, sobre la mezcla.
    ///
    /// `carril` distingue el lado A del B de una junta para que cada uno
    /// tenga su propio grupo de enlace (si no, el segundo pisaría al primero).
    #[allow(clippy::too_many_arguments)]
    pub fn revela_en(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        t_y: &wgpu::TextureView,
        t_uv: &wgpu::TextureView,
        gu: &GradeU,
        luts: Option<(&wgpu::TextureView, &wgpu::TextureView)>,
        slot: u64,
        carril: u64,
        primero: bool,
    ) {
        queue.write_buffer(&self.grade_buf, 0, bytemuck::bytes_of(gu));
        let par = if primero { 0u64 } else { 1u64 };
        let _ = par;
        let (la, lb) = luts.unwrap_or((&self.lut_a.1, &self.lut_b.1));
        // la clave lleva el carril: dos fuentes vivas a la vez en una junta
        let gkey = (0x20u64 << 16) | (slot << 4) | (carril << 1)
                   | if primero { 1 } else { 0 };
        if !self.bg_cache.contains_key(&gkey) {
            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.grade.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.grade_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(t_y) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(t_uv) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(la) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(lb) },
                    wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.samp) },
                    wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(
                        if primero { &self.targets.h_a.view } else { &self.targets.h_b.view }) },
                ],
            });
            self.bg_cache.insert(gkey, bg);
        }
        let bg = self.bg_cache[&gkey].clone();
        // el lado A limpia el lienzo (peso 1 = sustituye); el B se mezcla
        run_pass(enc, &self.grade, &bg, &[&self.targets.h_b.view]);
    }

    /// El resto de la cadena sobre lo que haya en la historia: pirámide,
    /// composición y empaquetado a los planos del codificador.
    pub fn compone(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        out_y: &wgpu::TextureView,
        out_uv: &wgpu::TextureView,
        frame_idx: usize,
        fps: f64,
    ) {
        std::mem::swap(&mut self.targets.h_a, &mut self.targets.h_b);
        self.cadena(device, queue, enc, out_y, out_uv, frame_idx, fps);
    }

    pub fn encode_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        t_y: &wgpu::TextureView,
        t_uv: &wgpu::TextureView,
        out_y: &wgpu::TextureView,
        out_uv: &wgpu::TextureView,
        frame_idx: usize,
        fps: f64,
    ) -> wgpu::CommandBuffer {
        let mut enc = device.create_command_encoder(&Default::default());

        // grade + obturador fusionados: lee h_a, escribe h_b (una escritura 4K)
        let mut gu = self.grade_u;
        gu.pad0 = self.shutter;
        gu.pad1 = if frame_idx == 0 { 1.0 } else { 0.0 };
        queue.write_buffer(&self.grade_buf, 0, bytemuck::bytes_of(&gu));
        let slot = (frame_idx % 8) as u64;        // N=8 slots en main.rs
        let par = (frame_idx & 1) as u64;         // paridad del swap h_a/h_b
        let gkey = (1u64 << 16) | (slot << 1) | par;
        if !self.bg_cache.contains_key(&gkey) {
            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.grade.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.grade_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(t_y) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(t_uv) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&self.lut_a.1) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&self.lut_b.1) },
                    wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.samp) },
                    wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&self.targets.h_a.view) },
                ],
            });
            self.bg_cache.insert(gkey, bg);
        }
        let grade_bg = self.bg_cache[&gkey].clone();
        run_pass(&mut enc, &self.grade, &grade_bg, &[&self.targets.h_b.view]);
        std::mem::swap(&mut self.targets.h_a, &mut self.targets.h_b);
        self.cadena(device, queue, &mut enc, out_y, out_uv, frame_idx, fps);
        enc.finish()
    }

    /// LA CADENA CARA, de la historia al plano del codificador: pirámide,
    /// halación, grano y empaquetado. Corre UNA vez por fotograma, también
    /// cuando hay junta — sobre la imagen ya mezclada, que es como se revela
    /// de verdad una copia con doble exposición (MOTOR §5bis).
    #[allow(clippy::too_many_arguments)]
    fn cadena(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        out_y: &wgpu::TextureView,
        out_uv: &wgpu::TextureView,
        frame_idx: usize,
        fps: f64,
    ) {
        let par = (frame_idx & 1) as u64;
        let base = &self.targets.h_a;

        // pirámide
        let small_bg = |pass: &Pass, buf: &wgpu::Buffer, v: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pass.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(v) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.samp) },
                ],
            })
        };
        let down_bg = |v: &wgpu::TextureView, tw: u32, th: u32| {
            let mut u = [0u8; 16];
            u[0..4].copy_from_slice(&(1.0f32 / tw as f32).to_le_bytes());
            u[4..8].copy_from_slice(&(1.0f32 / th as f32).to_le_bytes());
            small_bg(&self.down, &uniform_buffer(device, &u), v)
        };
        let _ = &down_bg;
        let dkey = (2u64 << 16) | par;
        if !self.bg_cache.contains_key(&dkey) {
            self.bg_cache.insert(dkey, down_bg(&base.view, base.w, base.h));
            self.bg_cache.insert((3u64 << 16), down_bg(&self.targets.b0.view, self.targets.b0.w, self.targets.b0.h));
            self.bg_cache.insert((4u64 << 16), down_bg(&self.targets.c0.view, self.targets.c0.w, self.targets.c0.h));
        }
        run_pass(&mut *enc, &self.down, &self.bg_cache[&dkey].clone(), &[&self.targets.b0.view]);
        run_pass(&mut *enc, &self.down, &self.bg_cache[&(3u64 << 16)].clone(), &[&self.targets.c0.view]);
        run_pass(&mut *enc, &self.down, &self.bg_cache[&(4u64 << 16)].clone(), &[&self.targets.d0.view]);
        let hal_spread = self.comp_u.hal_spread;
        let blur_bg = |v: &wgpu::TextureView, dir: [f32; 2]| {
            let mut u = [0u8; 16];
            u[0..4].copy_from_slice(&dir[0].to_le_bytes());
            u[4..8].copy_from_slice(&dir[1].to_le_bytes());
            u[8..12].copy_from_slice(&1.0f32.to_le_bytes());
            small_bg(&self.blur, &uniform_buffer(device, &u), v)
        };
        {
            if !self.bg_cache.contains_key(&(10u64 << 16)) {
                let pairs: [(&Target, &Target, f32); 3] = [
                    (&self.targets.b0, &self.targets.b1, 7.0),
                    (&self.targets.c0, &self.targets.c1, 1.5 + hal_spread * 2.0),
                    (&self.targets.d0, &self.targets.d1, 4.0 + hal_spread * 6.0),
                ];
                for (i, (a, b, rad)) in pairs.iter().enumerate() {
                    self.bg_cache.insert(((10 + 2 * i as u64) << 16), blur_bg(&a.view, [*rad / a.w as f32, 0.0]));
                    self.bg_cache.insert(((11 + 2 * i as u64) << 16), blur_bg(&b.view, [0.0, *rad / a.h as f32]));
                }
            }
            let pairs: [(&Target, &Target); 3] = [
                (&self.targets.b0, &self.targets.b1),
                (&self.targets.c0, &self.targets.c1),
                (&self.targets.d0, &self.targets.d1),
            ];
            for (i, (a, b)) in pairs.iter().enumerate() {
                run_pass(&mut *enc, &self.blur, &self.bg_cache[&((10 + 2 * i as u64) << 16)].clone(), &[&b.view]);
                run_pass(&mut *enc, &self.blur, &self.bg_cache[&((11 + 2 * i as u64) << 16)].clone(), &[&a.view]);
            }
        }

        // comp → RGB
        let time = frame_idx as f32 / fps as f32;
        let wamp = self.weave * 2.5;
        let wr = 0.4 + 0.5 * 2.0;
        let mut cu = self.comp_u;
        cu.time = time;
        cu.seed = (frame_idx % 997) as f32;
        cu.weave_px_x = wamp * ((time * wr * 1.7).sin() + 0.5 * (time * wr * 3.1 + 1.3).sin()) / 1.5;
        cu.weave_px_y = wamp * ((time * wr * 2.3 + 0.7).sin() + 0.5 * (time * wr * 4.3 + 2.1).sin()) / 1.5;
        queue.write_buffer(&self.comp_buf, 0, bytemuck::bytes_of(&cu));
        let ckey = (5u64 << 16) | par;
        if !self.bg_cache.contains_key(&ckey) {
            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.comp.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.comp_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&base.view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.targets.h_b.view) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&self.targets.b0.view) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&self.targets.c0.view) },
                    wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&self.targets.d0.view) },
                    wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&self.grain_view) },
                    wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::Sampler(&self.samp) },
                    wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::Sampler(&self.samp_rep) },
                ],
            });
            self.bg_cache.insert(ckey, bg);
        }
        let comp_bg = self.bg_cache[&ckey].clone();
        // comp con MRT: RGB + plano Y de una vez
        run_pass(&mut *enc, &self.comp, &comp_bg, &[&self.out_rgb.view, out_y]);

        // el plano UV (a media resolución, del RGB final)
        let pkey = 6u64 << 16;
        if !self.bg_cache.contains_key(&pkey) {
            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.pack_uv.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&self.out_rgb.view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.samp) },
                ],
            });
            self.bg_cache.insert(pkey, bg);
        }
        let pk_bg = self.bg_cache[&pkey].clone();
        run_pass(&mut *enc, &self.pack_uv, &pk_bg, &[out_uv]);
    }
}
