//! El papel procedural (NORTE §1.1) — el material del taller, generado en GPU.
//!
//! Fibra anisótropa (fbm), grano fino, viñeta, manchas por semilla (cada
//! bobina tiene SU papel) y tres modos: hueso cálido (mesa), hueso frío
//! (revelado) y papel negro tiza (cuarto oscuro). Un pase fullscreen que
//! SUSTITUYE al clear + tile de grain: mismo coste, otro mundo.

use crate::ui::Gpu;

pub const MODO_MESA: f32 = 0.0;
pub const MODO_REVELADO: f32 = 1.0;
pub const MODO_TIZA: f32 = 2.0;

const WGSL: &str = r#"
struct U { tam: vec2<f32>, semilla: f32, modo: f32 };
@group(0) @binding(0) var<uniform> u: U;

fn hash2(p: vec2<f32>) -> f32 {
  let q = fract(p * vec2<f32>(0.1031, 0.1030) + u.semilla * 0.017);
  let r = q + dot(q, q.yx + 33.33);
  return fract((r.x + r.y) * r.x);
}
fn vnoise(p: vec2<f32>) -> f32 {
  let i = floor(p); let f = fract(p);
  let s = f * f * (3.0 - 2.0 * f);
  let a = hash2(i); let b = hash2(i + vec2<f32>(1.0, 0.0));
  let c = hash2(i + vec2<f32>(0.0, 1.0)); let d = hash2(i + vec2<f32>(1.0, 1.0));
  return mix(mix(a, b, s.x), mix(c, d, s.x), s.y);
}
fn fbm(p: vec2<f32>) -> f32 {
  var v = 0.0; var a = 0.5; var q = p;
  for (var k = 0; k < 3; k = k + 1) {
    v = v + a * vnoise(q);
    q = q * 2.13 + vec2<f32>(17.0, 9.2);
    a = a * 0.5;
  }
  return v;
}

struct VO { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) vi: u32) -> VO {
  var o: VO;
  let x = f32(i32(vi & 1u) * 4 - 1);
  let y = f32(i32(vi >> 1u) * 4 - 1);
  o.pos = vec4<f32>(x, y, 0.0, 1.0);
  o.uv = vec2<f32>(x, -y) * 0.5 + 0.5;
  return o;
}

@fragment fn fs(i: VO) -> @location(0) vec4<f32> {
  let px = i.uv * u.tam;
  // fibra: anisótropa (corre horizontal), dos escalas
  let fibra = fbm(px * vec2<f32>(0.012, 0.055)) - 0.5;
  let poro  = fbm(px * vec2<f32>(0.09, 0.11)) - 0.5;
  // grano fino de imprenta
  let grano = hash2(px) - 0.5;
  // viñeta hacia los bordes
  let d = i.uv - 0.5;
  let vin = dot(d, d) * 0.16;
  // manchas grandes por semilla (2 blobs + un cerco de café)
  var mancha = 0.0;
  for (var k = 0; k < 2; k = k + 1) {
    let fk = f32(k);
    let c = vec2<f32>(hash2(vec2<f32>(fk * 7.1, 3.3)), hash2(vec2<f32>(1.9, fk * 11.7)));
    let r = distance(i.uv, c);
    mancha = mancha + smoothstep(0.22, 0.0, r) * 0.030;
  }
  // cerco de taza: anillo fino en una esquina que depende de la semilla
  let cc = vec2<f32>(0.12 + 0.7 * hash2(vec2<f32>(4.2, 8.8)), 0.80 + 0.15 * hash2(vec2<f32>(9.1, 2.4)));
  let rr = distance(i.uv * vec2<f32>(u.tam.x / u.tam.y, 1.0), cc * vec2<f32>(u.tam.x / u.tam.y, 1.0));
  let cerco = smoothstep(0.012, 0.004, abs(rr - 0.055)) * 0.05;

  if (u.modo > 1.5) {
    // ── papel negro tiza: pizarra granulada, motas claras, polvo ──
    var g = 0.075 + fibra * 0.020 + poro * 0.028 + grano * 0.026 - vin * 0.35;
    // motas de tiza dispersas
    let mota = step(0.9965, hash2(floor(px * 0.5) * 2.0));
    g = g + mota * 0.10;
    let col = vec3<f32>(g * 1.02, g * 0.99, g * 0.94);
    return vec4<f32>(col, 1.0);
  }
  // ── hueso ──
  var base: vec3<f32>;
  if (u.modo < 0.5) {
    base = vec3<f32>(0.949, 0.933, 0.894);  // mesa: cálido
  } else {
    base = vec3<f32>(0.940, 0.936, 0.912);  // revelado: frío
  }
  var col = base + fibra * 0.024 + poro * 0.016 + grano * 0.014 - vin;
  col = col - mancha * vec3<f32>(0.9, 1.0, 1.1);
  col = col - cerco * vec3<f32>(0.15, 0.45, 0.65);
  return vec4<f32>(col, 1.0);
}
"#;

pub struct Papel {
    pipeline: wgpu::RenderPipeline,
    uni: wgpu::Buffer,
    bg: wgpu::BindGroup,
    pub semilla: f32,
    pub modo: f32,
}

impl Papel {
    pub fn new(g: &Gpu) -> Self {
        let sh = g.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("papel"),
            source: wgpu::ShaderSource::Wgsl(WGSL.into()),
        });
        let uni = g.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uni-papel"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bgl = g.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
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
            label: Some("papel"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &sh,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &sh,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: g.config.format,
                    blend: None,
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
        Papel { pipeline, uni, bg, semilla: 1.0, modo: MODO_MESA }
    }

    /// la semilla del papel ES el nombre del proyecto
    pub fn siembra(&mut self, nombre: &str) {
        let mut h = 0u32;
        for b in nombre.bytes() {
            h = h.wrapping_mul(31).wrapping_add(b as u32);
        }
        self.semilla = (h % 977) as f32 + 1.0;
    }

    pub fn sube(&self, g: &Gpu) {
        let (w, h) = g.alto_ancho();
        g.queue.write_buffer(&self.uni, 0, bytemuck::cast_slice(&[w, h, self.semilla, self.modo]));
    }

    pub fn pinta(&self, rp: &mut wgpu::RenderPass) {
        rp.set_pipeline(&self.pipeline);
        rp.set_bind_group(0, &self.bg, &[]);
        rp.draw(0..3, 0..1);
    }
}
