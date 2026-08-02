//! El lienzo: la interfaz del taller dibujada con la MISMA pipeline wgpu que
//! revela el vídeo. Rectángulos de tinta sobre papel — el vocabulario del
//! zine (más adelante: texturas de papel/grano, tipografía y SDF).

use crate::proyecto::Proyecto;
use std::sync::Arc;
use wgpu::util::DeviceExt;
use winit::window::Window;

/// UN DISPOSITIVO, MUCHAS SUPERFICIES (PENDIENTE §3).
///
/// `Gpu::new` creaba instancia, adaptador, dispositivo **y** superficie de una
/// vez, así que solo cabía una ventana. Ahora la instancia y el adaptador
/// viajan dentro: el dispositivo se crea una vez y cada ventana nueva pide su
/// superficie con `secundaria`, que reusa `device` y `queue` (son punteros
/// contados, no copias). No hace falta ni otro contexto de GPU ni otro hilo.
pub struct Gpu {
    pub superficie: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub escala: f32,
    instancia: wgpu::Instance,
    adaptador: std::sync::Arc<wgpu::Adapter>,
}

impl Gpu {
    pub async fn new(v: Arc<Window>) -> anyhow::Result<Self> {
        let escala = v.scale_factor() as f32;
        let inst = wgpu::Instance::default();
        let superficie = inst.create_surface(v.clone())?;
        let adap = inst
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&superficie),
                ..Default::default()
            })
            .await
            .ok_or_else(|| anyhow::anyhow!("sin adaptador GPU"))?;
        eprintln!("🎛  {} ({:?})", adap.get_info().name, adap.get_info().backend);
        let (device, queue) = adap
            .request_device(&wgpu::DeviceDescriptor::default(), None)
            .await?;
        let caps = superficie.get_capabilities(&adap);
        let formato = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
        let t = v.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: formato,
            width: t.width.max(64),
            height: t.height.max(64),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            // 2 en vuelo: con 1, get_current_texture serializa el bucle al
            // vsync (18 ms bloqueado, redraw a 50 Hz). El scrub ya es síncrono
            // (aguja e imagen en el MISMO frame), así que aquí manda el ritmo
            // de reproducción: 60 Hz limpios.
            desired_maximum_frame_latency: 2,
        };
        superficie.configure(&device, &config);
        Ok(Gpu { superficie, config, device, queue, escala,
                 instancia: inst, adaptador: std::sync::Arc::new(adap) })
    }

    /// OTRA VENTANA sobre el MISMO dispositivo. El formato se hereda del
    /// cristal principal a propósito: así las pipelines (el lienzo, los tipos
    /// y el pase de presentación del visor) valen tal cual en las dos.
    pub fn secundaria(&self, v: Arc<Window>) -> anyhow::Result<Gpu> {
        let superficie = self.instancia.create_surface(v.clone())?;
        let caps = superficie.get_capabilities(&self.adaptador);
        anyhow::ensure!(caps.formats.contains(&self.config.format),
                        "esa pantalla no admite el formato del taller");
        let t = v.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: self.config.format,
            width: t.width.max(64),
            height: t.height.max(64),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        superficie.configure(&self.device, &config);
        Ok(Gpu {
            superficie, config,
            device: self.device.clone(), queue: self.queue.clone(),
            escala: v.scale_factor() as f32,
            instancia: self.instancia.clone(),
            adaptador: self.adaptador.clone(),
        })
    }

    /// LA DENSIDAD DE LA PANTALLA puede cambiar al mover la ventana a otro
    /// monitor (§5 · HiDPI): las coordenadas del taller son lógicas, así que
    /// basta con que la escala esté al día.
    pub fn pon_escala(&mut self, e: f32) {
        if (e - self.escala).abs() > 0.001 { self.escala = e.max(0.5); }
    }

    pub fn redimensiona(&mut self, w: u32, h: u32) {
        self.config.width = w.max(64);
        self.config.height = h.max(64);
        self.superficie.configure(&self.device, &self.config);
    }

    /// tamaño en píxeles LÓGICOS (los del ratón)
    pub fn alto_ancho(&self) -> (f32, f32) {
        (self.config.width as f32 / self.escala, self.config.height as f32 / self.escala)
    }
    pub fn alto_logico(&self) -> f32 { self.alto_ancho().1 }

    pub fn encoder(&self) -> wgpu::CommandEncoder {
        self.device.create_command_encoder(&Default::default())
    }

    pub fn pinta(&self, enc: wgpu::CommandEncoder, f: impl FnOnce(&mut wgpu::RenderPass)) {
        // el HUESO del zine: el grano se estampa encima
        self.pinta_sobre(enc, wgpu::Color { r: 0.949, g: 0.933, b: 0.894, a: 1.0 }, f)
    }

    /// pintar sobre un fondo cualquiera (el vigía quiere negro, no papel)
    pub fn pinta_sobre(&self, mut enc: wgpu::CommandEncoder, fondo: wgpu::Color,
                       f: impl FnOnce(&mut wgpu::RenderPass)) {
        let Ok(tex) = self.superficie.get_current_texture() else { return };
        let vista = tex.texture.create_view(&Default::default());
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("taller"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &vista,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(fondo),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            f(&mut rp);
        }
        self.queue.submit(Some(enc.finish()));
        tex.present();
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vert {
    pos: [f32; 2],
    color: [f32; 4],
}

/// las voces tipográficas del zine (lab.css: --grot / --mono / --hand / serif)
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Familia {
    /// Courier — datos, timecode, cuerpo (la voz por defecto del taller)
    Mono,
    /// Space Grotesk Bold — rótulos y títulos
    Grot,
    /// Caveat — manuscrita (nombres de bobina, susurros)
    Mano,
    /// Fraunces — serif de portada
    Serif,
}

/// una lista de rectángulos en coordenadas LÓGICAS de pantalla
pub struct Dibujo {
    verts: Vec<Vert>,
    pub textos: Vec<(f32, f32, String, f32, [f32; 4], Familia)>,
    /// UN DESPLAZAMIENTO PARA TODO LO QUE VENGA. Sirve para mover un bloque
    /// entero sin tocar sus coordenadas — lo usa la cabecera de la sala, que
    /// tiene que bajar lo que ocupa la barra de menú.
    pub desplaza_y: f32,
}

impl Dibujo {
    pub fn nuevo() -> Self {
        Dibujo { verts: Vec::with_capacity(8192), textos: Vec::new(), desplaza_y: 0.0 }
    }

    /// texto: (x, y de la línea base superior, contenido, tamaño, color)
    pub fn texto(&mut self, x: f32, y: f32, t: &str, tam: f32, c: [f32; 4]) {
        let y = y + self.desplaza_y;
        self.textos.push((x, y, t.to_string(), tam, c, Familia::Mono));
    }

    /// texto con voz tipográfica explícita
    pub fn texto_f(&mut self, f: Familia, x: f32, y: f32, t: &str, tam: f32, c: [f32; 4]) {
        let y = y + self.desplaza_y;
        self.textos.push((x, y, t.to_string(), tam, c, f));
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: [f32; 4]) {
        let y = y + self.desplaza_y;
        let (x0, y0, x1, y1) = (x, y, x + w, y + h);
        for p in [[x0, y0], [x1, y0], [x1, y1], [x0, y0], [x1, y1], [x0, y1]] {
            self.verts.push(Vert { pos: p, color: c });
        }
    }

    /// rectángulo RECORTADO por la izquierda (la bobina desplazada no debe
    /// invadir el margen de la estantería)
    pub fn rect_rec(&mut self, x_min: f32, x: f32, y: f32, w: f32, h: f32, c: [f32; 4]) {
        let x0 = x.max(x_min);
        let w2 = (x + w - x0).min(w);
        if w2 > 0.2 {
            self.rect(x0, y, w2, h, c);
        }
    }

    /// triángulo crudo (lo usa el trazo a pulso)
    pub fn tri(&mut self, a: [f32; 2], b: [f32; 2], c2: [f32; 2], c: [f32; 4]) {
        let dy = self.desplaza_y;
        self.tri_crudo([a[0], a[1] + dy], [b[0], b[1] + dy], [c2[0], c2[1] + dy], c);
    }

    fn tri_crudo(&mut self, a: [f32; 2], b: [f32; 2], c2: [f32; 2], c: [f32; 4]) {
        self.verts.push(Vert { pos: a, color: c });
        self.verts.push(Vert { pos: b, color: c });
        self.verts.push(Vert { pos: c2, color: c });
    }

    /// rectángulo ROTADO alrededor de su centro (ang en radianes) — los
    /// objetos pegados del taller nunca están perfectamente rectos
    pub fn rect_rot(&mut self, x: f32, y: f32, w: f32, h: f32, ang: f32, c: [f32; 4]) {
        let y = y + self.desplaza_y;
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let (s, co) = ang.sin_cos();
        let gira = |px: f32, py: f32| -> [f32; 2] {
            let (dx, dy) = (px - cx, py - cy);
            [cx + dx * co - dy * s, cy + dx * s + dy * co]
        };
        let p = [gira(x, y), gira(x + w, y), gira(x + w, y + h), gira(x, y + h)];
        self.tri_crudo(p[0], p[1], p[2], c);
        self.tri_crudo(p[0], p[2], p[3], c);
    }

    /// línea recta de grosor g (para reglas, agujas y subrayados)
    pub fn linea(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, g: f32, c: [f32; 4]) {
        let (dx, dy) = (x1 - x0, y1 - y0);
        let l = (dx * dx + dy * dy).sqrt().max(0.0001);
        let (nx, ny) = (-dy / l * g / 2.0, dx / l * g / 2.0);
        let p = [
            [x0 + nx, y0 + ny], [x1 + nx, y1 + ny], [x1 - nx, y1 - ny],
            [x0 + nx, y0 + ny], [x1 - nx, y1 - ny], [x0 - nx, y0 - ny],
        ];
        for q in p { self.verts.push(Vert { pos: q, color: c }); }
    }
}

pub struct Lienzo {
    pipeline: wgpu::RenderPipeline,
    buffer: wgpu::Buffer,
    n: u32,
    uni: wgpu::Buffer,
    bg: wgpu::BindGroup,
}

const WGSL: &str = r#"
struct U { tam: vec2<f32>, _p: vec2<f32> };
@group(0) @binding(0) var<uniform> u: U;
struct VO { @builtin(position) pos: vec4<f32>, @location(0) col: vec4<f32> };
@vertex fn vs(@location(0) p: vec2<f32>, @location(1) c: vec4<f32>) -> VO {
  var o: VO;
  let n = vec2<f32>(p.x / u.tam.x * 2.0 - 1.0, 1.0 - p.y / u.tam.y * 2.0);
  o.pos = vec4<f32>(n, 0.0, 1.0);
  o.col = c;
  return o;
}
@fragment fn fs(i: VO) -> @location(0) vec4<f32> { return i.col; }
"#;

impl Lienzo {
    pub fn new(g: &Gpu) -> Self {
        let sh = g.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lienzo"),
            source: wgpu::ShaderSource::Wgsl(WGSL.into()),
        });
        let uni = g.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uni"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bgl = g.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bg = g.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uni.as_entire_binding() }],
        });
        let pl = g.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let pipeline = g.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lienzo"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &sh,
                entry_point: Some("vs"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vert>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &sh,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: g.config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let buffer = g.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("verts"),
            size: 64 * 1024 * std::mem::size_of::<Vert>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Lienzo { pipeline, buffer, n: 0, uni, bg }
    }

    pub fn sube(&mut self, g: &Gpu, d: &Dibujo) {
        let (w, h) = g.alto_ancho();
        g.queue.write_buffer(&self.uni, 0, bytemuck::cast_slice(&[w, h, 0.0f32, 0.0f32]));
        let datos: &[u8] = bytemuck::cast_slice(&d.verts);
        let max = 64 * 1024 * std::mem::size_of::<Vert>();
        let datos = &datos[..datos.len().min(max)];
        g.queue.write_buffer(&self.buffer, 0, datos);
        self.n = (datos.len() / std::mem::size_of::<Vert>()) as u32;
    }

    pub fn pinta(&self, rp: &mut wgpu::RenderPass) {
        if self.n == 0 { return; }
        rp.set_pipeline(&self.pipeline);
        rp.set_bind_group(0, &self.bg, &[]);
        rp.set_vertex_buffer(0, self.buffer.slice(..));
        rp.draw(0..self.n, 0..1);
    }
}

// ═══════════════════════ el atlas de miniaturas (quads texturizados) ═══

pub const MINI_W: u32 = 160;
pub const MINI_H: u32 = 90;
const ATLAS_TAM: u32 = 2048;
const ATLAS_COLS: u32 = ATLAS_TAM / MINI_W;   // 12
const ATLAS_FILAS: u32 = ATLAS_TAM / MINI_H;  // 22

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct VertT {
    pos: [f32; 2],
    uv: [f32; 2],
}

/// quads con imagen: se dibujan entre el lienzo y la tipografía
pub struct DibujoTex {
    verts: Vec<VertT>,
}

impl DibujoTex {
    pub fn nuevo() -> Self { DibujoTex { verts: Vec::with_capacity(2048) } }

    /// quad texturizado; `frac` recorta por la derecha (miniatura parcial)
    pub fn quad(&mut self, x: f32, y: f32, w: f32, h: f32, slot: u32, frac: f32) {
        let (cx, fy) = ((slot % ATLAS_COLS) as f32, (slot / ATLAS_COLS) as f32);
        let u0 = cx * MINI_W as f32 / ATLAS_TAM as f32;
        let v0 = fy * MINI_H as f32 / ATLAS_TAM as f32;
        let u1 = u0 + MINI_W as f32 / ATLAS_TAM as f32 * frac.clamp(0.02, 1.0);
        let v1 = v0 + MINI_H as f32 / ATLAS_TAM as f32;
        let (x0, y0, x1, y1) = (x, y, x + w, y + h);
        for (p, t) in [([x0, y0], [u0, v0]), ([x1, y0], [u1, v0]), ([x1, y1], [u1, v1]),
                       ([x0, y0], [u0, v0]), ([x1, y1], [u1, v1]), ([x0, y1], [u0, v1])] {
            self.verts.push(VertT { pos: p, uv: t });
        }
    }
}

impl DibujoTex {
    /// quad del atlas recortado por la IZQUIERDA (subrango de u)
    pub fn quad_rec(&mut self, x_min: f32, x: f32, y: f32, w: f32, h: f32, slot: u32, frac: f32) {
        let x0 = x.max(x_min);
        let w2 = (x + w - x0).min(w);
        if w2 <= 0.5 {
            return;
        }
        let corte = (x0 - x) / w.max(0.001);      // cuánto se comió por la izquierda
        let (cx, fy) = ((slot % ATLAS_COLS) as f32, (slot / ATLAS_COLS) as f32);
        let uw = MINI_W as f32 / ATLAS_TAM as f32;
        let u0 = cx * uw + uw * corte * frac.clamp(0.02, 1.0);
        let u1 = cx * uw + uw * frac.clamp(0.02, 1.0);
        let v0 = fy * MINI_H as f32 / ATLAS_TAM as f32;
        let v1 = v0 + MINI_H as f32 / ATLAS_TAM as f32;
        let (x1, y0, y1) = (x0 + w2, y, y + h);
        for (p, t) in [([x0, y0], [u0, v0]), ([x1, y0], [u1, v0]), ([x1, y1], [u1, v1]),
                       ([x0, y0], [u0, v0]), ([x1, y1], [u1, v1]), ([x0, y1], [u0, v1])] {
            self.verts.push(VertT { pos: p, uv: t });
        }
    }
}

const WGSL_TEX: &str = r#"
struct U { tam: vec2<f32>, _p: vec2<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var t: texture_2d<f32>;
@group(0) @binding(2) var s: sampler;
struct VO { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@location(0) p: vec2<f32>, @location(1) uv: vec2<f32>) -> VO {
  var o: VO;
  let n = vec2<f32>(p.x / u.tam.x * 2.0 - 1.0, 1.0 - p.y / u.tam.y * 2.0);
  o.pos = vec4<f32>(n, 0.0, 1.0);
  o.uv = uv;
  return o;
}
@fragment fn fs(i: VO) -> @location(0) vec4<f32> {
  return textureSample(t, s, i.uv);
}
"#;

pub struct Atlas {
    pub tex: wgpu::Texture,
    pipeline: wgpu::RenderPipeline,
    buffer: wgpu::Buffer,
    uni: wgpu::Buffer,
    bg: wgpu::BindGroup,
    n: u32,
    buffer2: wgpu::Buffer,
    n2: u32,
    libres: Vec<u32>,
}

impl Atlas {
    pub fn new(g: &Gpu) -> Self {
        let tex = g.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("miniaturas"),
            size: wgpu::Extent3d { width: ATLAS_TAM, height: ATLAS_TAM, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let vista = tex.create_view(&Default::default());
        let samp = g.device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let sh = g.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("atlas"),
            source: wgpu::ShaderSource::Wgsl(WGSL_TEX.into()),
        });
        let uni = g.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uni-atlas"), size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bgl = g.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false, min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1, visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2, visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bg = g.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None, layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uni.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&vista) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&samp) },
            ],
        });
        let pl = g.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None, bind_group_layouts: &[&bgl], push_constant_ranges: &[],
        });
        let pipeline = g.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("atlas"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &sh, entry_point: Some("vs"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<VertT>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &sh, entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: g.config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let buffer = g.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("verts-atlas"),
            size: 16 * 1024 * std::mem::size_of::<VertT>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let libres = (0..ATLAS_COLS * ATLAS_FILAS).rev().collect();
        let buffer2 = g.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("verts-atlas-2"),
            size: 4096 * std::mem::size_of::<VertT>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Atlas { tex, pipeline, buffer, uni, bg, n: 0, buffer2, n2: 0, libres }
    }

    /// la capa ALTA de miniaturas (por encima de los modales: hoja de contactos)
    pub fn sube2(&mut self, g: &Gpu, d: &DibujoTex) {
        let datos: &[u8] = bytemuck::cast_slice(&d.verts);
        let max = 4096 * std::mem::size_of::<VertT>();
        let datos = &datos[..datos.len().min(max)];
        g.queue.write_buffer(&self.buffer2, 0, datos);
        self.n2 = (datos.len() / std::mem::size_of::<VertT>()) as u32;
    }

    pub fn pinta2(&self, rp: &mut wgpu::RenderPass) {
        if self.n2 == 0 { return; }
        rp.set_pipeline(&self.pipeline);
        rp.set_bind_group(0, &self.bg, &[]);
        rp.set_vertex_buffer(0, self.buffer2.slice(..));
        rp.draw(0..self.n2, 0..1);
    }

    pub fn toma(&mut self) -> Option<u32> { self.libres.pop() }
    pub fn suelta(&mut self, slot: u32) { self.libres.push(slot); }

    /// sube una miniatura RGBA (MINI_W×MINI_H) a su hueco del atlas
    pub fn sube_slot(&self, g: &Gpu, slot: u32, rgba: &[u8]) {
        let (cx, fy) = (slot % ATLAS_COLS, slot / ATLAS_COLS);
        g.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.tex, mip_level: 0,
                origin: wgpu::Origin3d { x: cx * MINI_W, y: fy * MINI_H, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0, bytes_per_row: Some(MINI_W * 4), rows_per_image: Some(MINI_H),
            },
            wgpu::Extent3d { width: MINI_W, height: MINI_H, depth_or_array_layers: 1 },
        );
    }

    pub fn sube(&mut self, g: &Gpu, d: &DibujoTex) {
        let (w, h) = g.alto_ancho();
        g.queue.write_buffer(&self.uni, 0, bytemuck::cast_slice(&[w, h, 0.0f32, 0.0f32]));
        let datos: &[u8] = bytemuck::cast_slice(&d.verts);
        let max = 16 * 1024 * std::mem::size_of::<VertT>();
        let datos = &datos[..datos.len().min(max)];
        g.queue.write_buffer(&self.buffer, 0, datos);
        self.n = (datos.len() / std::mem::size_of::<VertT>()) as u32;
    }

    pub fn pinta(&self, rp: &mut wgpu::RenderPass) {
        if self.n == 0 { return; }
        rp.set_pipeline(&self.pipeline);
        rp.set_bind_group(0, &self.bg, &[]);
        rp.set_vertex_buffer(0, self.buffer.slice(..));
        rp.draw(0..self.n, 0..1);
    }
}

// ═══════════════ estampas: texturas del taller (papel, cinta, doodles) ═══

/// una textura PNG con su pipeline de quads (uv arbitrarios, repeat)
pub struct Estampa {
    pipeline: wgpu::RenderPipeline,
    buffer: wgpu::Buffer,
    uni: wgpu::Buffer,
    bg: wgpu::BindGroup,
    n: u32,
    pub tw: u32,
    pub th: u32,
    verts: Vec<VertT>,
}

impl Estampa {
    pub fn new(g: &Gpu, png: &[u8], repite: bool) -> Self {
        let img = image::load_from_memory(png).expect("png").to_rgba8();
        let (tw, th) = img.dimensions();
        let tex = g.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("estampa"),
            size: wgpu::Extent3d { width: tw, height: th, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        g.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &img,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(tw * 4), rows_per_image: Some(th) },
            wgpu::Extent3d { width: tw, height: th, depth_or_array_layers: 1 },
        );
        let vista = tex.create_view(&Default::default());
        let modo = if repite { wgpu::AddressMode::Repeat } else { wgpu::AddressMode::ClampToEdge };
        let samp = g.device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: modo, address_mode_v: modo,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let sh = g.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("estampa"),
            source: wgpu::ShaderSource::Wgsl(WGSL_TEX.into()),
        });
        let uni = g.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uni-estampa"), size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bgl = g.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false, min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1, visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2, visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bg = g.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None, layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uni.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&vista) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&samp) },
            ],
        });
        let pl = g.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None, bind_group_layouts: &[&bgl], push_constant_ranges: &[],
        });
        let pipeline = g.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("estampa"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &sh, entry_point: Some("vs"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<VertT>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &sh, entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: g.config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let buffer = g.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("verts-estampa"),
            size: 4096 * std::mem::size_of::<VertT>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Estampa { pipeline, buffer, uni, bg, n: 0, tw, th, verts: Vec::new() }
    }

    pub fn limpia(&mut self) { self.verts.clear(); }

    /// quad con uv explícitos (u0,v0,u1,v1 — >1 repite si la estampa repite)
    pub fn quad_uv(&mut self, x: f32, y: f32, w: f32, h: f32, uv: [f32; 4]) {
        self.quad_uv_rot(x, y, w, h, uv, 0.0);
    }

    /// quad texturizado ROTADO alrededor de su centro
    pub fn quad_uv_rot(&mut self, x: f32, y: f32, w: f32, h: f32, uv: [f32; 4], ang: f32) {
        let [u0, v0, u1, v1] = uv;
        if ang == 0.0 {
            let (x0, y0, x1, y1) = (x, y, x + w, y + h);
            for (p, t) in [([x0, y0], [u0, v0]), ([x1, y0], [u1, v0]), ([x1, y1], [u1, v1]),
                           ([x0, y0], [u0, v0]), ([x1, y1], [u1, v1]), ([x0, y1], [u0, v1])] {
                self.verts.push(VertT { pos: p, uv: t });
            }
            return;
        }
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let (s, co) = ang.sin_cos();
        let gira = |px: f32, py: f32| -> [f32; 2] {
            let (dx, dy) = (px - cx, py - cy);
            [cx + dx * co - dy * s, cy + dx * s + dy * co]
        };
        let p = [gira(x, y), gira(x + w, y), gira(x + w, y + h), gira(x, y + h)];
        let t = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
        for (pp, tt) in [(p[0], t[0]), (p[1], t[1]), (p[2], t[2]),
                         (p[0], t[0]), (p[2], t[2]), (p[3], t[3])] {
            self.verts.push(VertT { pos: pp, uv: tt });
        }
    }

    /// papel: cubre el rect tileando la textura a escala natural
    pub fn tapiza(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.quad_uv(x, y, w, h, [0.0, 0.0, w / self.tw as f32, h / self.th as f32]);
    }

    pub fn sube(&mut self, g: &Gpu) {
        let (w, h) = g.alto_ancho();
        g.queue.write_buffer(&self.uni, 0, bytemuck::cast_slice(&[w, h, 0.0f32, 0.0f32]));
        let datos: &[u8] = bytemuck::cast_slice(&self.verts);
        let max = 4096 * std::mem::size_of::<VertT>();
        let datos = &datos[..datos.len().min(max)];
        g.queue.write_buffer(&self.buffer, 0, datos);
        self.n = (datos.len() / std::mem::size_of::<VertT>()) as u32;
    }

    pub fn pinta(&self, rp: &mut wgpu::RenderPass) {
        if self.n == 0 { return; }
        rp.set_pipeline(&self.pipeline);
        rp.set_bind_group(0, &self.bg, &[]);
        rp.set_vertex_buffer(0, self.buffer.slice(..));
        rp.draw(0..self.n, 0..1);
    }
}

/// el taller de tipografía: una sola fuente, tres tamaños, la tinta del zine
pub struct Tipos {
    fuente: glyphon::FontSystem,
    cache: glyphon::SwashCache,
    atlas: glyphon::TextAtlas,
    viewport: glyphon::Viewport,
    render: glyphon::TextRenderer,
    puestos: Vec<((String, u32, Familia), f32, f32, [f32; 4])>,
    /// shapear texto es CARO: se cachea por (texto, tamaño, familia) y solo
    /// se re-shapea lo que cambia (el timecode), no toda la interfaz
    formas: std::collections::HashMap<(String, u32, Familia), glyphon::Buffer>,
}

fn attrs_de(f: Familia) -> glyphon::Attrs<'static> {
    use glyphon::{Attrs, Family, Weight};
    match f {
        Familia::Mono => Attrs::new().family(Family::Name("Courier New"))
            .weight(Weight::BOLD),
        Familia::Grot => Attrs::new().family(Family::Name("Space Grotesk"))
            .weight(Weight::BOLD),
        Familia::Mano => Attrs::new().family(Family::Name("Caveat"))
            .weight(Weight(500)),
        Familia::Serif => Attrs::new().family(Family::Name("Fraunces")),
    }
}

impl Tipos {
    pub fn new(g: &Gpu) -> Self {
        let mut fuente = glyphon::FontSystem::new();
        // las tipografías del zine viajan DENTRO del binario
        for datos in [
            &include_bytes!("../assets/fonts/SpaceGrotesk-Bold.ttf")[..],
            &include_bytes!("../assets/fonts/SpaceGrotesk-Regular.ttf")[..],
            // instancias ESTÁTICAS: las variables confunden al matcher
            &include_bytes!("../assets/fonts/Caveat-Medium.ttf")[..],
            &include_bytes!("../assets/fonts/Fraunces-Text.ttf")[..],
        ] {
            fuente.db_mut().load_font_data(datos.to_vec());
        }
        let cache = glyphon::SwashCache::new();
        let cacheg = glyphon::Cache::new(&g.device);
        let mut atlas = glyphon::TextAtlas::new(&g.device, &g.queue, &cacheg, g.config.format);
        let viewport = glyphon::Viewport::new(&g.device, &cacheg);
        let render = glyphon::TextRenderer::new(&mut atlas, &g.device, Default::default(), None);
        Tipos { fuente, cache, atlas, viewport, render, puestos: Vec::new(),
                formas: std::collections::HashMap::new() }
    }

    pub fn prepara(&mut self, g: &Gpu, d: &Dibujo) {
        self.viewport.update(&g.queue, glyphon::Resolution {
            width: g.config.width, height: g.config.height,
        });
        self.puestos.clear();
        let mut usadas: std::collections::HashSet<(String, u32, Familia)> =
            std::collections::HashSet::with_capacity(d.textos.len());
        for (x, y, t, tam, c, fam) in &d.textos {
            let clave = (t.clone(), (*tam * 10.0) as u32, *fam);
            usadas.insert(clave.clone());
            let fuente = &mut self.fuente;
            self.formas.entry(clave.clone()).or_insert_with(|| {
                let mut b = glyphon::Buffer::new(fuente, glyphon::Metrics::new(*tam, *tam * 1.25));
                b.set_size(fuente, Some(2000.0), Some(tam * 1.6));
                b.set_text(fuente, t, attrs_de(*fam), glyphon::Shaping::Advanced);
                b.shape_until_scroll(fuente, false);
                b
            });
            self.puestos.push((clave, *x, *y, *c));
        }
        // no dejar crecer la caché con timecodes viejos
        if self.formas.len() > 512 {
            self.formas.retain(|k, _| usadas.contains(k));
        }
        let esc = g.escala;
        let formas = &self.formas;
        let areas: Vec<glyphon::TextArea> = self.puestos.iter()
            .filter_map(|(k, x, y, c)| formas.get(k).map(|b| (b, x, y, c)))
            .map(|(b, x, y, c)| glyphon::TextArea {
            buffer: b,
            left: x * esc,
            top: y * esc,
            scale: esc,
            bounds: glyphon::TextBounds {
                left: 0, top: 0,
                right: g.config.width as i32, bottom: g.config.height as i32,
            },
            default_color: glyphon::Color::rgba(
                (c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8, (c[3] * 255.0) as u8),
            custom_glyphs: &[],
        }).collect();
        let _ = self.render.prepare(&g.device, &g.queue, &mut self.fuente, &mut self.atlas,
                                    &self.viewport, areas, &mut self.cache);
    }

    pub fn pinta(&self, rp: &mut wgpu::RenderPass) {
        let _ = self.render.render(&self.atlas, &self.viewport, rp);
    }
}

/// la bobina sobre el banco: tiras de película, cintas de empalme y la aguja
pub fn bobina(d: &mut Dibujo, pr: &Proyecto, y: f32, t: f64, pxs: f32) {
    use crate::paleta::*;
    let x0 = 12.0;
    let alto = 84.0;
    // la regla
    d.linea(0.0, y - 14.0, 4000.0, y - 14.0, 1.4, TINTA);
    let mut acc = 0.0f64;
    for c in &pr.clips {
        let x = x0 + (acc as f32) * pxs;
        let w = (c.dur() as f32 * pxs).max(6.0);
        if c.hueco {
            d.rect(x, y, w, alto, [0.08, 0.07, 0.05, 1.0]);
        } else {
            d.rect(x, y, w, alto, PELICULA);
            // perforaciones arriba y abajo
            let mut px = x + 5.0;
            while px < x + w - 7.0 {
                d.rect(px, y + 4.0, 6.5, 5.0, HUESO);
                d.rect(px, y + alto - 9.0, 6.5, 5.0, HUESO);
                px += 12.0;
            }
        }
        // cinta de empalme en la junta
        d.rect(x + w - 3.0, y - 6.0, 6.0, alto + 12.0, AMBAR);
        acc += c.dur();
    }
    // la aguja
    let ax = x0 + (t as f32) * pxs;
    d.linea(ax, y - 24.0, ax, y + alto + 10.0, 1.8, ROJO);
    d.rect(ax - 6.0, y - 26.0, 13.0, 9.0, ROJO);
}
