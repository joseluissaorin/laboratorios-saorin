use anyhow::Result;
use clap::{Parser, Subcommand};
use filmlook_core::{pipeline::*, profile, video};
use std::time::Instant;

#[derive(Parser)]
#[command(name = "filmlook", about = "film-look lab · motor GPU nativo")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Renderiza un vídeo con LUT(s) + film emulation
    Render {
        input: String,
        #[arg(short, long)]
        out: String,
        #[arg(long)]
        lut: Option<String>,
        #[arg(long)]
        lut_in: Option<String>,
        #[arg(long)]
        prefs: Option<String>,
        #[arg(long)]
        fps: Option<f64>,
        #[arg(long, default_value = "prores_ks")]
        codec: String,
        #[arg(long)]
        scale: Option<f32>,
        #[arg(long)]
        max_frames: Option<usize>,
        #[arg(long)]
        bench: bool,
    },
}

fn parse_cube(path: &str) -> Result<(u32, Vec<f32>)> {
    let text = std::fs::read_to_string(path)?;
    let mut n = 0u32;
    let mut vals = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') { continue; }
        if let Some(rest) = l.strip_prefix("LUT_3D_SIZE") {
            n = rest.trim().parse()?;
            continue;
        }
        if l.chars().next().unwrap().is_alphabetic() { continue; }
        for tok in l.split_whitespace() {
            vals.push(tok.parse::<f32>()?);
        }
    }
    anyhow::ensure!(n > 0 && vals.len() == (n * n * n * 3) as usize, "cube inválido");
    Ok((n, vals))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Render { input, out, lut, lut_in, prefs, fps, codec, scale, max_frames, bench } => {
            render(&input, &out, lut, lut_in, prefs, fps, &codec, scale, max_frames, bench)
        }
    }
}

fn render(
    input: &str, out: &str, lut: Option<String>, lut_in: Option<String>,
    prefs_path: Option<String>, fps_opt: Option<f64>, codec: &str,
    scale_opt: Option<f32>, max_frames: Option<usize>, bench: bool,
) -> Result<()> {
    // prefs (valores del lab)
    let prefs: serde_json::Value = match &prefs_path {
        Some(p) => serde_json::from_slice(&std::fs::read(p)?)?,
        None => std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../app/ui/assets/prefs_default.json"))
            .ok()
            .and_then(|b: Vec<u8>| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .unwrap_or(serde_json::json!({})),
    };
    let f = |k: &str, d: f64| prefs[k].as_f64().unwrap_or(d) as f32;

    let (w0, h0, native_fps, duration) = video::probe(input)?;
    let fps = fps_opt.unwrap_or(native_fps);
    let total = max_frames.unwrap_or((duration * fps) as usize);

    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    })).ok_or_else(|| anyhow::anyhow!("sin adaptador GPU"))?;
    let info = adapter.get_info();
    let prof = profile::detect(&info);
    let scale = scale_opt.unwrap_or(prof.default_scale);
    eprintln!("🎛  {} ({} · {:?}) · tier ×{:.2} · inflight {}",
              info.name, prof.vendor, info.backend, scale, prof.inflight);

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor::default(),
        None,
    ))?;

    let w = ((w0 as f32) * scale).round() as u32;
    let h = ((h0 as f32) * scale).round() as u32;
    let mut targets = make_target_set(&device, w, h);
    let samp = make_sampler(&device);
    let samp_rep = make_repeat_sampler(&device);

    // LUTs
    let lut_a_data = lut_in.map(|p| parse_cube(&p)).transpose()?;
    let lut_b_data = lut.map(|p| parse_cube(&p)).transpose()?;
    let (_ta, lut_a_view) = match &lut_a_data {
        Some((n, d)) => make_3d_lut(&device, &queue, *n, d),
        None => make_3d_lut(&device, &queue, 2, &[0.,0.,0., 1.,0.,0., 0.,1.,0., 1.,1.,0., 0.,0.,1., 1.,0.,1., 0.,1.,1., 1.,1.,1.]),
    };
    let (_tb, lut_b_view) = match &lut_b_data {
        Some((n, d)) => make_3d_lut(&device, &queue, *n, d),
        None => make_3d_lut(&device, &queue, 2, &[0.,0.,0., 1.,0.,0., 0.,1.,0., 1.,1.,0., 0.,0.,1., 1.,0.,1., 0.,1.,1., 1.,1.,1.]),
    };

    // texturas fuente: YUV uint + dummy rgba
    let mk_plane = |pw: u32, ph: u32| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: pw, height: ph, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R16Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    };
    let t_y = mk_plane(w0, h0);
    let t_u = mk_plane(w0 / 2, h0 / 2);
    let t_v = mk_plane(w0 / 2, h0 / 2);
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

    // grain plate
    let grain_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/ui/assets/grain.bin");
    let grain_raw = std::fs::read(&grain_dir)?;
    let grain: Vec<u16> = bytemuck::cast_slice(&grain_raw).to_vec();
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
        bytemuck::cast_slice(&grain),
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(1024 * 2), rows_per_image: Some(1024) },
        wgpu::Extent3d { width: 1024, height: 1024, depth_or_array_layers: 1 },
    );
    let view_grain = t_grain.create_view(&Default::default());

    // pases
    let grade = make_pass(&device, include_str!("shaders/grade.wgsl"), &[
        uniform_entry(0, filmlook_core::params::bytes_uniforme::<filmlook_core::params::GradeU>()),
        tex_uint_entry(1), tex_uint_entry(2), tex_uint_entry(3),
        tex_filter_entry(4),
        tex3d_entry(5), tex3d_entry(6),
        sampler_entry(7),
    ], &[color_target(), color_target()]);
    let down = make_pass(&device, include_str!("shaders/down.wgsl"), &[
        uniform_entry(0, 16), tex_filter_entry(1), sampler_entry(2),
    ], &[color_target()]);
    let blur = make_pass(&device, include_str!("shaders/blur.wgsl"), &[
        uniform_entry(0, 16), tex_filter_entry(1), sampler_entry(2),
    ], &[color_target()]);
    let accum = make_pass(&device, include_str!("shaders/accum.wgsl"), &[
        uniform_entry(0, 16), tex_filter_entry(1), tex_filter_entry(2), sampler_entry(3),
    ], &[color_target()]);
    let comp = make_pass(&device, include_str!("shaders/comp.wgsl"), &[
        uniform_entry(0, filmlook_core::params::bytes_uniforme::<filmlook_core::params::CompU>()),
        tex_filter_entry(1), tex_filter_entry(2), tex_filter_entry(3),
        tex_filter_entry(4), tex_filter_entry(5), tex_filter_entry(6),
        sampler_entry(7), sampler_entry(8),
    ], &[color_target()]);

    // LOS UNIFORMES, los del taller y no una copia. Aquí vivía un `GradeU`
    // local de 64 bytes mientras `grade.wgsl` pedía 80: este CLI llevaba
    // tiempo **sin arrancar** («Binding size 64 … less than minimum 80») y
    // nadie se enteró porque el trabajo pasa por la app. Es exactamente la
    // divergencia que MOTOR §8 quiere quitar: la misma estructura escrita
    // tres veces (aquí, en la preview y en el motor de Windows).
    let grade_u = filmlook_core::params::grade_u(
        &prefs, w0, h0,
        lut_a_data.as_ref().map(|(n, _)| *n).unwrap_or(2),
        lut_b_data.as_ref().map(|(n, _)| *n).unwrap_or(2),
        lut_a_data.is_some(), lut_b_data.is_some());
    let grade_buf = uniform_buffer(&device, bytemuck::bytes_of(&grade_u));

    // ídem con la receta del comp: la del taller, no otra copia
    let mut comp_u = filmlook_core::params::comp_u(&prefs, w, h);
    comp_u.wipe = 1.0;
    let comp_buf = uniform_buffer(&device, bytemuck::bytes_of(&comp_u));

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

    let mk_tex_pass_bg = |pass: &Pass, ubuf: &wgpu::Buffer, v: &wgpu::TextureView| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pass.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(v) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&samp) },
            ],
        })
    };
    let small_u = uniform_buffer(&device, &[0u8; 16]);

    let accum_bg = |curr: &wgpu::TextureView, prev: &wgpu::TextureView, ubuf: &wgpu::Buffer| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &accum.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(curr) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(prev) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&samp) },
            ],
        })
    };

    // salida final rgba8 + staging ring
    let out_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let out_view = out_tex.create_view(&Default::default());
    let out_pass = make_pass(&device, include_str!("shaders/comp.wgsl"), &[
        uniform_entry(0, filmlook_core::params::bytes_uniforme::<filmlook_core::params::CompU>()),
        tex_filter_entry(1), tex_filter_entry(2), tex_filter_entry(3),
        tex_filter_entry(4), tex_filter_entry(5), tex_filter_entry(6),
        sampler_entry(7), sampler_entry(8),
    ], &[Some(wgpu::ColorTargetState {
        format: wgpu::TextureFormat::Rgba8Unorm,
        blend: Some(wgpu::BlendState::REPLACE),
        write_mask: wgpu::ColorWrites::ALL,
    })]);

    let padded = (w * 4 + 255) / 256 * 256;
    let stage_size = (padded * h) as u64;
    let staging: Vec<wgpu::Buffer> = (0..prof.staging_ring).map(|_| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: stage_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }).collect();

    let mut dec = video::FfmpegDecoder::open(input)?;
    let mut enc = if bench { None } else { Some(video::FfmpegEncoder::new(out, w, h, fps, input, codec)?) };

    let shutter = f("shutter", 0.0);
    let t_start = Instant::now();
    let mut acc_decode = std::time::Duration::ZERO;
    let mut acc_render = std::time::Duration::ZERO;
    let mut acc_read = std::time::Duration::ZERO;
    let mut acc_write = std::time::Duration::ZERO;
    let mut frame_idx = 0usize;
    let mut staged: Vec<(usize, wgpu::Buffer)> = Vec::new();
    let mut staging_it = staging.iter().cycle();

    while frame_idx < total {
        let t = Instant::now();
        let Some((y, u, v)) = dec.next_frame() else { break };
        // subir planos
        let upload = |tex: &wgpu::Texture, data: &[u16], pw: u32, ph: u32| {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                bytemuck::cast_slice(data),
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(pw * 2), rows_per_image: Some(ph) },
                wgpu::Extent3d { width: pw, height: ph, depth_or_array_layers: 1 },
            );
        };
        upload(&t_y, y, w0, h0);
        upload(&t_u, u, w0 / 2, h0 / 2);
        upload(&t_v, v, w0 / 2, h0 / 2);
        acc_decode += t.elapsed();

        let t = Instant::now();
        let mut encoder = device.create_command_encoder(&Default::default());
        run_pass(&mut encoder, &grade, &grade_bg, &[&targets.graded.view, &targets.raw.view]);
        let use_shutter = shutter > 0.001;
        let base_is_hist = use_shutter;
        if use_shutter {
            let reset = if frame_idx == 0 { 1u32 } else { 0u32 };
            let mut u = [0u8; 16];
            u[0..4].copy_from_slice(&shutter.to_le_bytes());
            u[4..8].copy_from_slice(&reset.to_le_bytes());
            queue.write_buffer(&small_u, 0, &u);
            let bg = accum_bg(&targets.graded.view, &targets.h_a.view, &small_u);
            run_pass(&mut encoder, &accum, &bg, &[&targets.h_b.view]);
            std::mem::swap(&mut targets.h_a, &mut targets.h_b);
        }
        let base = if base_is_hist { &targets.h_a } else { &targets.graded };
        let down_bg = |v: &wgpu::TextureView, tw: u32, th: u32| {
            let mut u = [0u8; 16];
            u[0..4].copy_from_slice(&(1.0f32 / tw as f32).to_le_bytes());
            u[4..8].copy_from_slice(&(1.0f32 / th as f32).to_le_bytes());
            let buf = uniform_buffer(&device, &u);
            mk_tex_pass_bg(&down, &buf, v)
        };
        run_pass(&mut encoder, &down, &down_bg(&base.view, base.w, base.h), &[&targets.b0.view]);
        run_pass(&mut encoder, &down, &down_bg(&targets.b0.view, targets.b0.w, targets.b0.h), &[&targets.c0.view]);
        run_pass(&mut encoder, &down, &down_bg(&targets.c0.view, targets.c0.w, targets.c0.h), &[&targets.d0.view]);
        let blur_bg = |v: &wgpu::TextureView, dir: [f32; 2], rad: f32| {
            let mut u = [0u8; 16];
            u[0..4].copy_from_slice(&dir[0].to_le_bytes());
            u[4..8].copy_from_slice(&dir[1].to_le_bytes());
            u[8..12].copy_from_slice(&rad.to_le_bytes());
            let buf = uniform_buffer(&device, &u);
            mk_tex_pass_bg(&blur, &buf, v)
        };
        let blur2 = |enc: &mut wgpu::CommandEncoder, a: &Target, b: &Target, rad: f32| {
            run_pass(enc, &blur, &blur_bg(&a.view, [rad / a.w as f32, 0.0], 1.0), &[&b.view]);
            run_pass(enc, &blur, &blur_bg(&b.view, [0.0, rad / a.h as f32], 1.0), &[&a.view]);
        };
        blur2(&mut encoder, &targets.b0, &targets.b1, 7.0);
        blur2(&mut encoder, &targets.c0, &targets.c1, 1.5 + f("halSpread", 0.7) * 2.0);
        blur2(&mut encoder, &targets.d0, &targets.d1, 4.0 + f("halSpread", 0.7) * 6.0);

        let time = frame_idx as f32 / fps as f32;
        let wamp = f("weave", 0.0) * 2.5;
        let wr = 0.4 + 0.5 * 2.0;
        let weave_x = wamp * ((time * wr * 1.7).sin() + 0.5 * (time * wr * 3.1 + 1.3).sin()) / 1.5;
        let weave_y = wamp * ((time * wr * 2.3 + 0.7).sin() + 0.5 * (time * wr * 4.3 + 2.1).sin()) / 1.5;
        let mut cu = comp_u;
        cu.time = time;
        cu.seed = (frame_idx % 997) as f32;
        cu.weave_px_x = weave_x;
        cu.weave_px_y = weave_y;
        queue.write_buffer(&comp_buf, 0, bytemuck::bytes_of(&cu));
        let comp_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &out_pass.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: comp_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&base.view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&targets.raw.view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&targets.b0.view) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&targets.c0.view) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&targets.d0.view) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&view_grain) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::Sampler(&samp) },
                wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::Sampler(&samp_rep) },
            ],
        });
        run_pass(&mut encoder, &out_pass, &comp_bg, &[&out_view]);

        // copia a staging
        let sbuf = staging_it.next().unwrap().clone();
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &out_tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &sbuf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        queue.submit([encoder.finish()]);
        acc_render += t.elapsed();

        // drena staged (el más viejo ya terminó en la mayoría de los casos)
        staged.push((frame_idx, sbuf));
        while staged.len() >= prof.staging_ring {
            let (idx, buf) = staged.remove(0);
            let t2 = Instant::now();
            let slice = buf.slice(..);
            slice.map_async(wgpu::MapMode::Read, |_| {});
            device.poll(wgpu::Maintain::Wait);
            let data = slice.get_mapped_range();
            let direct = padded == w * 4;
            let mut rgba_owned;
            let rgba: &[u8] = if direct {
                &data[..(w * h * 4) as usize]
            } else {
                rgba_owned = vec![0u8; (w * h * 4) as usize];
                for row in 0..h as usize {
                    rgba_owned[row * (w * 4) as usize..(row + 1) * (w * 4) as usize]
                        .copy_from_slice(&data[row * padded as usize..row * padded as usize + (w * 4) as usize]);
                }
                &rgba_owned
            };
            let t3 = Instant::now();
            if let Some(e) = enc.as_mut() { e.write_frame(rgba); }
            acc_write += t3.elapsed();
            drop(data);
            buf.unmap();
            acc_read += t2.elapsed();
            let _ = idx;
        }

        frame_idx += 1;
        if frame_idx % 24 == 0 {
            let rate = frame_idx as f64 / t_start.elapsed().as_secs_f64();
            eprint!("\r  {}/{} · {:.1} fps   ", frame_idx, total, rate);
        }
    }
    // drena el resto
    for (_, buf) in staged.drain(..) {
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::Maintain::Wait);
        {
            let data = slice.get_mapped_range();
            let mut rgba = vec![0u8; (w * h * 4) as usize];
            for row in 0..h as usize {
                rgba[row * (w * 4) as usize..(row + 1) * (w * 4) as usize]
                    .copy_from_slice(&data[row * padded as usize..row * padded as usize + (w * 4) as usize]);
            }
            if let Some(e) = enc.as_mut() { e.write_frame(&rgba); }
        }
        buf.unmap();
    }
    if let Some(e) = enc { e.finish(); }
    let el = t_start.elapsed().as_secs_f64();
    let n = frame_idx.max(1) as f64;
    eprintln!("\n✅ {}/{} frames en {:.1}s = {:.1} fps", frame_idx, total, el, frame_idx as f64 / el);
    eprintln!("   decode+upload {:.1} · render {:.1} · readback {:.1} · encode {:.1} ms/frame",
              acc_decode.as_secs_f64() * 1e3 / n, acc_render.as_secs_f64() * 1e3 / n,
              acc_read.as_secs_f64() * 1e3 / n, acc_write.as_secs_f64() * 1e3 / n);
    Ok(())
}
