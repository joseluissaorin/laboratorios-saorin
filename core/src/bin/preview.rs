//! preview — ventana interactiva del film-look lab (wgpu + winit).
//!
//! Reproduce el vídeo con la cadena fílmica completa en vivo.
//! Teclas: Espacio play/pausa · ←/→ seek ∓/±5 s · W wipe A/B · R recarga prefs
//! · Esc salir. Las prefs también se recargan solas al guardar el JSON.

use anyhow::Result;
use clap::Parser;
use filmlook_core::{params, pipeline::*, video};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

#[derive(Parser)]
#[command(name = "filmlook-preview", about = "film-look lab · preview interactiva")]
struct Cli {
    input: String,
    #[arg(long)]
    lut: Option<String>,
    #[arg(long)]
    lut_in: Option<String>,
    #[arg(long)]
    prefs: Option<String>,
    /// escala de la cadena respecto al vídeo (0.5 = 1080p desde 4K)
    #[arg(long, default_value = "0.5")]
    scale: f32,
    /// modo esclavo: órdenes JSON por stdin ({"clip","t","play","prefs"})
    #[arg(long)]
    ipc: bool,
}

/// buzón de la última orden recibida (coalescente: manda la más nueva)
type Buzon = std::sync::Arc<std::sync::Mutex<Option<serde_json::Value>>>;

fn arranca_ipc() -> Buzon {
    let buzon: Buzon = std::sync::Arc::new(std::sync::Mutex::new(None));
    let b2 = buzon.clone();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        for linea in stdin.lock().lines() {
            let Ok(l) = linea else { break };
            let l = l.trim().to_string();
            if l.is_empty() { continue; }
            if l == "salir" { std::process::exit(0); }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&l) {
                *b2.lock().unwrap() = Some(v);
            }
        }
    });
    buzon
}

fn parse_cube(path: &str) -> Result<(u32, Vec<f32>)> {
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

fn read_prefs(path: &Option<String>) -> serde_json::Value {
    path.as_ref()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(serde_json::json!({}))
}

fn prefs_mtime(path: &Option<String>) -> Option<std::time::SystemTime> {
    path.as_ref().and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok())
}

struct State {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,

    dec: video::FfmpegDecoder,
    input: String,
    fps: f64,
    duration: f64,
    pos_s: f64,

    // cadena
    targets: TargetSet,
    grade: Pass, down: Pass, blur: Pass, accum: Pass, present: Pass,
    samp: wgpu::Sampler, samp_rep: wgpu::Sampler,
    t_y: wgpu::Texture, t_u: wgpu::Texture, t_v: wgpu::Texture,
    grade_bg: wgpu::BindGroup,
    view_grain: wgpu::TextureView,
    grade_buf: wgpu::Buffer,
    comp_buf: wgpu::Buffer,
    small_u: wgpu::Buffer,
    w: u32, h: u32, dw: u32, dh: u32,

    // prefs vivas
    prefs_path: Option<String>,
    prefs: serde_json::Value,
    prefs_seen: Option<std::time::SystemTime>,
    lut_meta: (u32, u32, bool, bool),
    grade_u: params::GradeU,
    comp_u: params::CompU,
    shutter: f32,

    // reproducción
    playing: bool,
    needs_frame: bool,
    wipe_on: bool,
    frame_idx: usize,
    last_frame: Instant,
    fps_avg: f64,
}

impl State {
    fn new(window: Arc<Window>, cli: &Cli) -> Result<Self> {
        let (w0, h0, fps, duration) = video::probe(&cli.input)?;
        let dw = ((w0 as f32 * cli.scale).round() as u32) & !1;
        let dh = ((h0 as f32 * cli.scale).round() as u32) & !1;
        let dec = video::FfmpegDecoder::open_at(&cli.input, 0.0, dw, dh)?;

        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        })).ok_or_else(|| anyhow::anyhow!("sin adaptador GPU"))?;
        let info = adapter.get_info();
        eprintln!("🎛  {} ({:?})", info.name, info.backend);
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor::default(), None))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats.iter().copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(64),
            height: size.height.max(64),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let (w, h) = (dw, dh);
        let targets = make_target_set(&device, w, h);
        let samp = make_sampler(&device);
        let samp_rep = make_repeat_sampler(&device);

        let lut_a_data = cli.lut_in.as_ref().map(|p| parse_cube(p)).transpose()?;
        let lut_b_data = cli.lut.as_ref().map(|p| parse_cube(p)).transpose()?;
        let ident: Vec<f32> = vec![0.,0.,0., 1.,0.,0., 0.,1.,0., 1.,1.,0., 0.,0.,1., 1.,0.,1., 0.,1.,1., 1.,1.,1.];
        let (_ta, lut_a_view) = match &lut_a_data {
            Some((n, d)) => make_3d_lut(&device, &queue, *n, d),
            None => make_3d_lut(&device, &queue, 2, &ident),
        };
        let (_tb, lut_b_view) = match &lut_b_data {
            Some((n, d)) => make_3d_lut(&device, &queue, *n, d),
            None => make_3d_lut(&device, &queue, 2, &ident),
        };
        let lut_meta = (
            lut_a_data.as_ref().map(|(n, _)| *n).unwrap_or(2),
            lut_b_data.as_ref().map(|(n, _)| *n).unwrap_or(2),
            lut_a_data.is_some(), lut_b_data.is_some(),
        );

        let mk_plane = |pw: u32, ph: u32| device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: pw, height: ph, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R16Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let t_y = mk_plane(dw, dh);
        let t_u = mk_plane(dw / 2, dh / 2);
        let t_v = mk_plane(dw / 2, dh / 2);
        let t_video = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view_y = t_y.create_view(&Default::default());
        let view_u = t_u.create_view(&Default::default());
        let view_v = t_v.create_view(&Default::default());
        let view_video = t_video.create_view(&Default::default());

        let grain_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../app/ui/assets/grain.bin");
        let grain_raw = std::fs::read(&grain_path)?;
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
        let view_grain = t_grain.create_view(&Default::default());

        let grade = make_pass(&device, include_str!("../shaders/grade.wgsl"), &[
            uniform_entry(0, params::bytes_uniforme::<params::GradeU>()),
            tex_uint_entry(1), tex_uint_entry(2), tex_uint_entry(3),
            tex_filter_entry(4),
            tex3d_entry(5), tex3d_entry(6),
            sampler_entry(7),
        ], &[color_target(), color_target()]);
        let down = make_pass(&device, include_str!("../shaders/down.wgsl"), &[
            uniform_entry(0, 16), tex_filter_entry(1), sampler_entry(2),
        ], &[color_target()]);
        let blur = make_pass(&device, include_str!("../shaders/blur.wgsl"), &[
            uniform_entry(0, 16), tex_filter_entry(1), sampler_entry(2),
        ], &[color_target()]);
        let accum = make_pass(&device, include_str!("../shaders/accum.wgsl"), &[
            uniform_entry(0, 16), tex_filter_entry(1), tex_filter_entry(2), sampler_entry(3),
        ], &[color_target()]);
        // comp directo al swapchain (formato de la superficie)
        let present = make_pass(&device, include_str!("../shaders/comp.wgsl"), &[
            uniform_entry(0, params::bytes_uniforme::<params::CompU>()),
            tex_filter_entry(1), tex_filter_entry(2), tex_filter_entry(3),
            tex_filter_entry(4), tex_filter_entry(5), tex_filter_entry(6),
            sampler_entry(7),
        ], &[Some(wgpu::ColorTargetState {
            format,
            blend: Some(wgpu::BlendState::REPLACE),
            write_mask: wgpu::ColorWrites::ALL,
        })]);

        let prefs = read_prefs(&cli.prefs);
        let grade_u = params::grade_u(&prefs, dw, dh, lut_meta.0, lut_meta.1, lut_meta.2, lut_meta.3);
        let comp_u = params::comp_u(&prefs, w, h);
        let shutter = params::f(&prefs, "shutter", 0.0);
        let grade_buf = uniform_buffer(&device, bytemuck::bytes_of(&grade_u));
        let comp_buf = uniform_buffer(&device, bytemuck::bytes_of(&comp_u));
        let small_u = uniform_buffer(&device, &[0u8; 16]);

        let grade_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &grade.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: grade_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view_y) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&view_u) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&view_v) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&view_video) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&lut_a_view) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&lut_b_view) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::Sampler(&samp) },
            ],
        });

        let prefs_seen = prefs_mtime(&cli.prefs);
        Ok(State {
            window, surface, config, device, queue,
            dec, input: cli.input.clone(), fps, duration, pos_s: 0.0,
            targets, grade, down, blur, accum, present,
            samp, samp_rep, t_y, t_u, t_v, grade_bg, view_grain,
            grade_buf, comp_buf, small_u,
            w, h, dw, dh,
            prefs_path: cli.prefs.clone(), prefs, prefs_seen, lut_meta,
            grade_u, comp_u, shutter,
            playing: true, needs_frame: true, wipe_on: false, frame_idx: 0,
            last_frame: Instant::now(), fps_avg: 0.0,
        })
    }

    fn reload_prefs(&mut self) {
        self.prefs = read_prefs(&self.prefs_path);
        self.apply_prefs();
        eprintln!("🔄 prefs recargadas");
    }

    fn apply_prefs(&mut self) {
        self.grade_u = params::grade_u(&self.prefs, self.dw, self.dh,
            self.lut_meta.0, self.lut_meta.1, self.lut_meta.2, self.lut_meta.3);
        self.comp_u = params::comp_u(&self.prefs, self.w, self.h);
        self.shutter = params::f(&self.prefs, "shutter", 0.0);
        self.queue.write_buffer(&self.grade_buf, 0, bytemuck::bytes_of(&self.grade_u));
    }

    /// salta a un tiempo ABSOLUTO del clip actual
    fn seek_abs(&mut self, t: f64) {
        let target = t.clamp(0.0, (self.duration - 0.05).max(0.0));
        if let Ok(d) = video::FfmpegDecoder::open_at(&self.input, target, self.dw, self.dh) {
            self.dec = d;
            self.pos_s = target;
            self.frame_idx = 0;
            self.needs_frame = true;   // en pausa hay que pintar el frame nuevo
        }
    }

    /// una orden del editor: clip / posición / play / prefs
    fn apply_cmd(&mut self, v: &serde_json::Value) {
        if let Some(pr) = v.get("prefs") {
            if pr.is_object() {
                self.prefs = pr.clone();
                self.apply_prefs();
            }
        }
        let t = v["t"].as_f64();
        if let Some(clip) = v["clip"].as_str() {
            if clip != self.input {
                if let Ok((_w, _h, fps, dur)) = video::probe(clip) {
                    self.input = clip.to_string();
                    self.fps = fps;
                    self.duration = dur;
                    self.seek_abs(t.unwrap_or(0.0));
                    let nombre = std::path::Path::new(clip)
                        .file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    self.window.set_title(&format!("LABORATORIOS SAORÍN · {nombre}"));
                    return;
                }
            }
        }
        if let Some(t) = t {
            if (t - self.pos_s).abs() > 0.08 { self.seek_abs(t); }
        }
        if let Some(p) = v["play"].as_bool() { self.playing = p; }
    }

    fn seek(&mut self, delta_s: f64) {
        let target = (self.pos_s + delta_s).clamp(0.0, (self.duration - 0.5).max(0.0));
        if let Ok(d) = video::FfmpegDecoder::open_at(&self.input, target, self.dw, self.dh) {
            self.dec = d;
            self.pos_s = target;
            self.frame_idx = 0; // resetea el acumulador del shutter
            self.needs_frame = true;
        }
    }

    fn frame(&mut self) {
        // recarga de prefs por mtime (una vez por segundo aprox)
        if self.frame_idx % 30 == 0 {
            let m = prefs_mtime(&self.prefs_path);
            if m.is_some() && m != self.prefs_seen {
                self.prefs_seen = m;
                self.reload_prefs();
            }
        }

        if self.playing || self.needs_frame {
            self.needs_frame = false;
            let Some((y, u, v)) = self.dec.next_frame().map(|(y, u, v)| {
                (y.to_vec(), u.to_vec(), v.to_vec())
            }) else {
                // fin del clip → loop
                self.seek(-self.pos_s);
                self.window.request_redraw();
                return;
            };
            let upload = |tex: &wgpu::Texture, data: &[u16], pw: u32, ph: u32| {
                self.queue.write_texture(
                    wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    bytemuck::cast_slice(data),
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(pw * 2), rows_per_image: Some(ph) },
                    wgpu::Extent3d { width: pw, height: ph, depth_or_array_layers: 1 },
                );
            };
            upload(&self.t_y, &y, self.dw, self.dh);
            upload(&self.t_u, &u, self.dw / 2, self.dh / 2);
            upload(&self.t_v, &v, self.dw / 2, self.dh / 2);
            self.pos_s += 1.0 / self.fps;
            self.frame_idx += 1;
        }

        let Ok(surf_tex) = self.surface.get_current_texture() else {
            self.surface.configure(&self.device, &self.config);
            return;
        };
        let surf_view = surf_tex.texture.create_view(&Default::default());

        let mut encoder = self.device.create_command_encoder(&Default::default());
        run_pass(&mut encoder, &self.grade, &self.grade_bg,
                 &[&self.targets.graded.view, &self.targets.raw.view]);

        let use_shutter = self.shutter > 0.001;
        if use_shutter {
            let reset = if self.frame_idx <= 1 { 1u32 } else { 0u32 };
            let mut ub = [0u8; 16];
            ub[0..4].copy_from_slice(&self.shutter.to_le_bytes());
            ub[4..8].copy_from_slice(&reset.to_le_bytes());
            self.queue.write_buffer(&self.small_u, 0, &ub);
            let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.accum.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.small_u.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.targets.graded.view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.targets.h_a.view) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.samp) },
                ],
            });
            run_pass(&mut encoder, &self.accum, &bg, &[&self.targets.h_b.view]);
            std::mem::swap(&mut self.targets.h_a, &mut self.targets.h_b);
        }
        let base: &Target = if use_shutter { &self.targets.h_a } else { &self.targets.graded };

        let mk_small_bg = |pass: &Pass, buf: &wgpu::Buffer, v: &wgpu::TextureView| {
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
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
            let mut ub = [0u8; 16];
            ub[0..4].copy_from_slice(&(1.0f32 / tw as f32).to_le_bytes());
            ub[4..8].copy_from_slice(&(1.0f32 / th as f32).to_le_bytes());
            mk_small_bg(&self.down, &uniform_buffer(&self.device, &ub), v)
        };
        run_pass(&mut encoder, &self.down, &down_bg(&base.view, base.w, base.h), &[&self.targets.b0.view]);
        run_pass(&mut encoder, &self.down, &down_bg(&self.targets.b0.view, self.targets.b0.w, self.targets.b0.h), &[&self.targets.c0.view]);
        run_pass(&mut encoder, &self.down, &down_bg(&self.targets.c0.view, self.targets.c0.w, self.targets.c0.h), &[&self.targets.d0.view]);

        let hal_spread = self.comp_u.hal_spread;
        let blur_bg = |v: &wgpu::TextureView, dir: [f32; 2]| {
            let mut ub = [0u8; 16];
            ub[0..4].copy_from_slice(&dir[0].to_le_bytes());
            ub[4..8].copy_from_slice(&dir[1].to_le_bytes());
            ub[8..12].copy_from_slice(&1.0f32.to_le_bytes());
            mk_small_bg(&self.blur, &uniform_buffer(&self.device, &ub), v)
        };
        let mut blur2 = |enc: &mut wgpu::CommandEncoder, a: &Target, b: &Target, rad: f32| {
            run_pass(enc, &self.blur, &blur_bg(&a.view, [rad / a.w as f32, 0.0]), &[&b.view]);
            run_pass(enc, &self.blur, &blur_bg(&b.view, [0.0, rad / a.h as f32]), &[&a.view]);
        };
        blur2(&mut encoder, &self.targets.b0, &self.targets.b1, 7.0);
        blur2(&mut encoder, &self.targets.c0, &self.targets.c1, 1.5 + hal_spread * 2.0);
        blur2(&mut encoder, &self.targets.d0, &self.targets.d1, 4.0 + hal_spread * 6.0);

        let time = self.pos_s as f32;
        let wamp = params::f(&self.prefs, "weave", 0.0) * 2.5;
        let wr = 0.4 + 0.5 * 2.0;
        let mut cu = self.comp_u;
        cu.time = time;
        cu.seed = (self.frame_idx % 997) as f32;
        cu.wipe = if self.wipe_on { 0.5 } else { 1.0 };
        cu.weave_px_x = wamp * ((time * wr * 1.7).sin() + 0.5 * (time * wr * 3.1 + 1.3).sin()) / 1.5;
        cu.weave_px_y = wamp * ((time * wr * 2.3 + 0.7).sin() + 0.5 * (time * wr * 4.3 + 2.1).sin()) / 1.5;
        self.queue.write_buffer(&self.comp_buf, 0, bytemuck::bytes_of(&cu));
        let comp_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.present.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.comp_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&base.view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.targets.raw.view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&self.targets.b0.view) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&self.targets.c0.view) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&self.targets.d0.view) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&self.view_grain) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::Sampler(&self.samp_rep) },
            ],
        });
        run_pass(&mut encoder, &self.present, &comp_bg, &[&surf_view]);
        self.queue.submit([encoder.finish()]);
        surf_tex.present();

        // ritmo: al fps del vídeo
        let target = Duration::from_secs_f64(1.0 / self.fps);
        let el = self.last_frame.elapsed();
        if self.playing && el < target { std::thread::sleep(target - el); }
        let dt = self.last_frame.elapsed().as_secs_f64();
        self.last_frame = Instant::now();
        self.fps_avg = if self.fps_avg == 0.0 { 1.0 / dt } else { self.fps_avg * 0.95 + 0.05 / dt };

        if self.frame_idx % 15 == 0 {
            self.window.set_title(&format!(
                "filmlook · {} · {:02}:{:04.1} / {:02}:{:04.1} · {:.0} fps{}{}",
                if self.playing { "▶" } else { "⏸" },
                (self.pos_s / 60.0) as u32, self.pos_s % 60.0,
                (self.duration / 60.0) as u32, self.duration % 60.0,
                self.fps_avg,
                if self.wipe_on { " · WIPE" } else { "" },
                if self.prefs_path.is_some() { " · prefs vivas" } else { "" },
            ));
        }
    }
}

struct App {
    cli: Cli,
    state: Option<State>,
    buzon: Option<Buzon>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.state.is_some() { return; }
        let attrs = Window::default_attributes()
            .with_title("filmlook · cargando…")
            .with_inner_size(winit::dpi::LogicalSize::new(1600.0, 900.0));
        let window = Arc::new(el.create_window(attrs).expect("ventana"));
        match State::new(window, &self.cli) {
            Ok(mut s) => {
                if self.cli.ipc { s.playing = false; }   // la manda el editor
                self.state = Some(s);
            }
            Err(e) => { eprintln!("❌ {e:#}"); el.exit(); }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(s) = self.state.as_mut() else { return };
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(size) => {
                s.config.width = size.width.max(64);
                s.config.height = size.height.max(64);
                s.surface.configure(&s.device, &s.config);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed { return; }
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::Escape) => el.exit(),
                    PhysicalKey::Code(KeyCode::Space) => s.playing = !s.playing,
                    PhysicalKey::Code(KeyCode::ArrowLeft) => s.seek(-5.0),
                    PhysicalKey::Code(KeyCode::ArrowRight) => s.seek(5.0),
                    PhysicalKey::Code(KeyCode::KeyW) => s.wipe_on = !s.wipe_on,
                    PhysicalKey::Code(KeyCode::KeyR) => s.reload_prefs(),
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => s.frame(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let (Some(s), Some(b)) = (self.state.as_mut(), self.buzon.as_ref()) {
            let orden = b.lock().unwrap().take();
            if let Some(v) = orden { s.apply_cmd(&v); }
        }
        if let Some(s) = self.state.as_ref() { s.window.request_redraw(); }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let el = EventLoop::new()?;
    el.set_control_flow(winit::event_loop::ControlFlow::Poll);
    let buzon = if cli.ipc { Some(arranca_ipc()) } else { None };
    let mut app = App { cli, state: None, buzon };
    el.run_app(&mut app)?;
    Ok(())
}
