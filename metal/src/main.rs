use anyhow::Result;
use clap::{Parser, Subcommand};
use filmlook_metal::decode_vt::{parse_annexb, VtDecoder};
use filmlook_metal::encode_vt::VtEncoder;
use filmlook_metal::metal_pipe::*;
use filmlook_metal::vt_ffi::*;
use objc::{msg_send, sel, sel_impl};
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::Instant;
use std::collections::VecDeque;

#[derive(Parser)]
#[command(name = "filmlook-metal", about = "film-look · render GPU nativo (Metal+VT, zero-copy)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
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
        #[arg(long, default_value = "40000000")]
        bitrate: i64,
        /// hevc | prores4444 | prores422hq
        #[arg(long, default_value = "hevc")]
        codec: String,
        #[arg(long)]
        max_frames: Option<usize>,
        #[arg(long)]
        bench: bool,
    },
    /// LA BOBINA ENTERA de un tirón: sin corte, sin fase de fundidos y sin
    /// concatenación. El plan es el mismo JSON de timeline de siempre.
    Bobina {
        /// fichero con el plan (o «-» para leerlo de la entrada estándar)
        plan: String,
        /// dónde están las gelatinas (<taller>/luts/{entrada,color})
        #[arg(long)]
        luts: Option<String>,
        /// SOLO UN TRAMO de la bobina: primer renglón a revelar. Es lo que
        /// permite recalcular un clip sin recalcular la bobina entera
        /// (MOTOR §7, caché fina).
        #[arg(long)]
        desde: Option<usize>,
        /// cuántos renglones desde ahí
        #[arg(long)]
        cuantos: Option<usize>,
        /// fotogramas de CARRERILLA antes del tramo: se revelan y se tiran,
        /// pero dejan la historia del obturador caliente. Sin esto, cada
        /// tramo empezaría con el arrastre a cero y se vería el escalón en
        /// la juntura.
        #[arg(long, default_value = "8")]
        carrerilla: usize,
        /// adónde va este tramo (por defecto, lo que diga el plan)
        #[arg(long)]
        out: Option<String>,
    },
}

fn parse_cube(path: &str) -> Result<(u64, Vec<f32>)> {
    let text = std::fs::read_to_string(path)?;
    let mut n = 0u64;
    let mut vals = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') { continue; }
        if let Some(rest) = l.strip_prefix("LUT_3D_SIZE") { n = rest.trim().parse()?; continue; }
        if l.starts_with(|c: char| c.is_alphabetic()) { continue; }
        for tok in l.split_whitespace() { vals.push(tok.parse::<f32>()?); }
    }
    anyhow::ensure!(n > 0 && vals.len() == (n * n * n * 3) as usize, "cube inválido");
    Ok((n, vals))
}

/// (fps, ancho, alto) del stream de vídeo — el encoder se crea al tamaño de la
/// FUENTE (antes estaba clavado a 3840×2160 y estiraba el aspecto)
fn probe_src(path: &str) -> (f64, u32, u32) {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0",
               "-show_entries", "stream=r_frame_rate,width,height",
               "-of", "csv=p=0", path])
        .output().ok();
    out.and_then(|o| {
        let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
        // csv: width,height,r_frame_rate
        let mut it = s.split(',');
        let w: u32 = it.next()?.parse().ok()?;
        let h: u32 = it.next()?.parse().ok()?;
        let rate = it.next()?;
        let (n, d) = rate.split_once('/').unwrap_or((rate, "1"));
        let fps = n.parse::<f64>().ok()? / d.parse::<f64>().unwrap_or(1.0).max(1.0);
        Some((fps, w & !1, h & !1))   // dims pares (4:2:0)
    }).unwrap_or((24.0, 3840, 2160))
}

fn OUT_DEBUG(b: bool) -> bool { b }

fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Cmd::Bobina { plan, luts, desde, cuantos, carrerilla, out } = &cli.cmd {
        return revela_bobina(plan, luts.as_deref(), *desde, *cuantos, *carrerilla,
                             out.as_deref());
    }
    let Cmd::Render { input, out, lut, lut_in, prefs, bitrate, codec, max_frames, bench } = cli.cmd
        else { unreachable!() };
    if codec != "hevc" { std::env::set_var("FL_CODEC", &codec); }

    let prefs: serde_json::Value = match &prefs {
        Some(p) => serde_json::from_slice(&std::fs::read(p)?)?,
        None => serde_json::json!({}),
    };
    let f = |k: &str, d: f64| prefs[k].as_f64().unwrap_or(d) as f32;
    let (fps, src_w, src_h) = probe_src(&input);

    // 1. demux: ffmpeg remux a Annex-B (rápido, sin decode)
    eprintln!("📦 demuxing…");
    let t0 = Instant::now();
    let mut demux = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-i", &input,
               "-map", "0:v:0", "-c:v", "copy", "-bsf:v", "hevc_mp4toannexb", "-f", "hevc", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stream = Vec::new();
    demux.stdout.take().unwrap().read_to_end(&mut stream)?;
    let _ = demux.wait();
    let mut a = parse_annexb(stream);
    eprintln!("   {} NALs · {} parameter sets en {:.1}s", a.pending.len(),
              [!a.vps.is_empty(), !a.sps.is_empty(), !a.pps.is_empty()].iter().filter(|x| **x).count(),
              t0.elapsed().as_secs_f64());
    anyhow::ensure!(!a.vps.is_empty() && !a.sps.is_empty() && !a.pps.is_empty(),
                    "no se encontraron VPS/SPS/PPS");

    // 2. GPU + VT
    let gpu = Gpu::new();
    let dec = VtDecoder::new(&gpu.device, &a.vps, &a.sps, &a.pps)?;
    let pipes = Pipelines::new(&gpu.device);

    let lut_a_data = lut_in.map(|p| parse_cube(&p)).transpose()?;
    let lut_b_data = lut.map(|p| parse_cube(&p)).transpose()?;
    let ident: Vec<f32> = vec![0.,0.,0., 1.,0.,0., 0.,1.,0., 1.,1.,0., 0.,0.,1., 1.,0.,1., 0.,1.,1., 1.,1.,1.];
    let (na, lut_a) = lut_a_data.as_ref().map(|(n, d)| (*n, d.clone())).unwrap_or((2, ident.clone()));
    let (nb, lut_b) = lut_b_data.as_ref().map(|(n, d)| (*n, d.clone())).unwrap_or((2, ident));
    let lut_a_tex = make_3d_lut(&gpu.device, na, &lut_a);
    let lut_b_tex = make_3d_lut(&gpu.device, nb, &lut_b);
    let grain = make_grain(&gpu, std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../app/ui/assets/grain.bin").as_path());

    let total = max_frames.unwrap_or(a.pending.len());
    let audio_limit = if max_frames.is_some() { total as f64 / fps } else { 0.0 };
    let enc = if bench { None } else { Some(VtEncoder::new(src_w, src_h, &out, &input, fps, bitrate, audio_limit)?) };

    // espera el primer frame para conocer el tamaño real
    let mut renderer: Option<Renderer> = None;
    let mut frame_idx = 0usize;
    let t_start = Instant::now();
    let mut acc_decode = 0.0f64;
    let mut acc_render = 0.0f64;
    let mut acc_encode = 0.0f64;
    let mut inflight = 0usize;
    let acc_alloc = std::cell::Cell::new(0.0f64);

    // hilo de encode: espera la GPU y somete a VT fuera del bucle principal;
    // el canal acotado (8 frames en vuelo ≈ 200 MB x420) da la contrapresión
    /// (comando, pixelbuffer de SALIDA, índice, pixelbuffer de ENTRADA, sus
    /// texturas). Los dos últimos no se usan aquí: viajan para que **no se
    /// liberen antes de que la GPU haya leído el fotograma**. Soltarlos en el
    /// bucle devolvía el búfer al pool de VideoToolbox, que lo reciclaba para
    /// un fotograma posterior mientras el look aún estaba leyéndolo: a partir
    /// del séptimo fotograma el máster salía con imágenes de otro sitio, y
    /// distinto en cada revelado (dos ejecuciones idénticas daban PSNR 24 dB).
    struct EncJob(metal::CommandBuffer, CVPixelBufferRef, i64,
                  CVPixelBufferRef, (metal::Texture, metal::Texture));
    unsafe impl Send for EncJob {}
    let enc_pool = enc.as_ref().map(|e| e.pool);
    let (etx, enc_join) = if let Some(e) = enc {
        let (tx, rx) = std::sync::mpsc::sync_channel::<EncJob>(8);
        let h = std::thread::spawn(move || {
            let (mut wait_ms, mut enc_ms, mut count) = (0.0f64, 0.0f64, 0i64);
            for EncJob(cmd, pb, idx, entrada, texturas) in rx {
                let t = Instant::now();
                cmd.wait_until_completed();
                wait_ms += t.elapsed().as_secs_f64() * 1e3;
                // AQUÍ, y no antes: la GPU ya ha leído el fotograma de entrada
                drop(texturas);
                unsafe { CFRelease(entrada as *mut std::ffi::c_void) };
                let t = Instant::now();
                e.encode(pb, idx, fps);
                enc_ms += t.elapsed().as_secs_f64() * 1e3;
                unsafe { CFRelease(pb as *mut std::ffi::c_void) };
                count += 1;
            }
            e.finish(count);
            (wait_ms, enc_ms, count)
        });
        (Some(tx), Some(h))
    } else { (None, None) };
    // en bench (sin encoder) mantenemos una cola corta para no esperar la GPU en serie
    let mut pending_gpu: VecDeque<(metal::CommandBuffer, CVPixelBufferRef,
                                  (metal::Texture, metal::Texture))> = VecDeque::new();

    let tex_cache = {
        let mut c: CVMetalTextureCacheRef = std::ptr::null_mut();
        let dev_ptr: *mut std::ffi::c_void = unsafe { objc::msg_send![&*gpu.device, self] };
        unsafe {
            CVMetalTextureCacheCreate(std::ptr::null(), std::ptr::null(), dev_ptr, std::ptr::null(), &mut c)
        };
        c
    };

    let mut nal_iter = a.pending.iter_mut();
    let mut submitted = 0usize;
    let mut flushed = false;
    let mut acc_iter = 0.0f64;
    'frames: loop {
        if frame_idx >= total { break; }
        let t_iter = Instant::now();
        // consigue el siguiente frame decodificado, alimentando NALs bajo demanda
        let t = Instant::now();
        let frame = loop {
            if let Some(f) = dec.pop() { break f; }
            match nal_iter.next() {
                Some(nal) => {
                    dec.decode_nal(nal, CMTime { value: submitted as i64 * 1000,
                        timescale: (fps * 1000.0).round().max(1.0) as i32, flags: 1, epoch: 0 });
                    submitted += 1;
                }
                None => {
                    if !flushed { dec.flush(); flushed = true; continue; }
                    break 'frames;
                }
            }
        };
        {
            let Some((t_y, t_uv, w, h)) = dec.import_planes(frame.pixel_buffer) else {
                unsafe { CFRelease(frame.pixel_buffer as *mut std::ffi::c_void) };
                continue;
            };
            acc_decode += t.elapsed().as_secs_f64() * 1e3;

            if renderer.is_none() {
                let targets = TargetSet::new(&gpu.device, w as u64, h as u64,
                                             filmlook_metal::metal_pipe::FMT_INTERMEDIO);
                renderer = Some(Renderer {
                    grade_params: GradeParams {
                        src_mode: 0, full_range: 0,
                        lut_na: na as u32, lut_nb: nb as u32,
                        lut_a_on: lut_a_data.is_some() as u32, lut_b_on: lut_b_data.is_some() as u32,
                        yuv_norm: 1.0, gain: f("gain", 0.0),
                        push_pull: f("pushPull", 0.0), compress: f("compImpact", 0.0),
                        compress_wp: f("compWP", 1.0), compress_range: f("compRange", 0.5),
                        src_w: w as f32, src_h: h as f32,
                        ..Default::default()
                    },
                    comp_params: CompParams {
                        hal_amount: f("halation", 0.0), hal_hue: f("halHue", 1.0),
                        hal_sat: f("halSat", 0.9), hal_thr: f("halThr", 0.5),
                        hal_spread: f("halSpread", 0.7), hal_white: f("halWhite", 0.0),
                        bloom_amount: f("bloom", 0.0), bloom_thr: f("bloomThr", 0.72),
                        bloom_warm: f("bloomWarm", 0.15),
                        softness: f("softness", 0.0), acutance: f("acutance", 0.0),
                        color_sep: f("colorSep", 0.0),
                        hue_skew: f("hueSkew", 1.0), crosstalk: f("crosstalk", 0.3),
                        subtractive: f("subtractive", 0.6), stock_sat: f("stockSat", 1.0),
                        print_: f("print", 0.5),
                        grain_amount: f("grain", 0.0), grain_size: f("grainSize", 2.6),
                        grain_rough: f("grainRough", 0.35), grain_chroma: f("grainChroma", 0.25),
                        grain_defocus: f("grainDefocus", 0.55),
                        grain_s: f("grainShadows", 0.8), grain_m: f("grainMids", 1.0),
                        grain_h: f("grainHighs", 0.5), grain_r: f("grainRed", 1.0),
                        grain_b: f("grainBlue", 1.25), film_res: f("filmRes", 0.5),
                        plate_n: 1024.0,
                        vig_amount: f("vignette", 0.0), vig_size: f("vigSize", 0.55),
                        vig_round: f("vigRound", 1.0), vig_cx: f("vigCX", 0.5),
                        vig_cy: f("vigCY", 0.5), ca: f("chroma", 0.0),
                        dust: f("dust", 0.0), flicker: f("flicker", 0.0), flicker_rate: 0.5,
                        breath: f("breath", 0.0), breath_rate: f("breathRate", 0.5),
                        frame_inset: f("frameInset", 0.0), frame_corner: f("frameCorner", 40.0),
                        frame_wobble: f("frameWobble", 0.5),
                        wipe: 1.0, weave_rot: f("weaveRot", 0.3),
                        ..Default::default()
                    },
                    shutter: f("shutter", 0.0),
                    weave_amount: f("weave", 0.0),
                    gpu: gpu.clone(),
                    pipes: pipes.clone(),
                    targets,
                    lut_a: lut_a_tex.clone(),
                    lut_b: lut_b_tex.clone(),
                    grain: grain.clone(),
                });
            }
            let r = renderer.as_mut().unwrap();

            // textura de salida: CVPixelBuffer del pool del encoder (o dummy en bench)
            let t = Instant::now();
            let pb = enc_pool.and_then(filmlook_metal::encode_vt::alloc_from_pool);
            acc_alloc.set(acc_alloc.get() + t.elapsed().as_secs_f64() * 1e3);
            if frame_idx == 0 && pb.is_some() {
                let pf = unsafe { CVPixelBufferGetPixelFormatType(pb.unwrap()) };
                eprintln!("   pool pixel format: 0x{:08x} ({}x{})", pf,
                    unsafe { CVPixelBufferGetWidth(pb.unwrap()) },
                    unsafe { CVPixelBufferGetHeight(pb.unwrap()) });
            }
            let t_imp = Instant::now();
            let planes = if let Some(pb) = pb {
                let import = |plane: usize, fmt: u64, pw: usize, ph: usize| -> Option<metal::Texture> {
                    let mut ct: CVMetalTextureRef = std::ptr::null_mut();
                    let st = unsafe {
                        CVMetalTextureCacheCreateTextureFromImage(
                            std::ptr::null(), tex_cache, pb, std::ptr::null(),
                            fmt, pw, ph, plane, &mut ct)
                    };
                    if st != 0 { eprintln!("   import plano {} fmt 0x{:x} falló st={}", plane, fmt, st); return None; }
                    let raw = unsafe { CVMetalTextureGetTexture(ct) };
                    if raw.is_null() { unsafe { CFRelease(ct as *mut std::ffi::c_void) }; return None; }
                    unsafe { core_foundation::base::CFRetain(raw as *const std::ffi::c_void) };
                    let t: metal::Texture = unsafe { metal::foreign_types::ForeignType::from_ptr(raw as *mut _) };
                    unsafe { CFRelease(ct as *mut std::ffi::c_void) };
                    Some(t)
                };
                match (import(0, MTLPixelFormat::R16Unorm as u64, w, h),
                       import(1, MTLPixelFormat::RG16Unorm as u64, w / 2, h / 2)) {
                    (Some(a), Some(b)) => Some((a, b)),
                    _ => None,
                }
            } else { None };
            acc_alloc.set(acc_alloc.get() + t_imp.elapsed().as_secs_f64() * 1e3);

            let dbg_texs = if frame_idx == 2 && std::env::var("FL_DEBUG_DUMP").is_ok() {
                Some((r.targets.graded.clone(), r.targets.out.clone()))
            } else { None };
            let t = Instant::now();
            // el comando lo crea quien manda, no el Renderer: así revelar y
            // componer son dos pasos separables (los necesita la bobina)
            let cola = gpu.queue.clone();
            let cmd = cola.new_command_buffer();
            let (mut gp, la, lb) = (r.grade_params, r.lut_a.clone(), r.lut_b.clone());
            gp.pad0 = r.shutter;                     // el obturador va fundido
            gp.pad1 = if frame_idx == 0 { 1.0 } else { 0.0 };
            r.revela_en(cmd, &t_y, &t_uv, &gp, &la, &lb);
            r.cierra_obturador(r.shutter > 0.001);
            match &planes {
                Some((py, puv)) => r.compone(cmd, frame_idx, fps, Some((py, puv))),
                None => r.compone(cmd, frame_idx, fps, None),
            }
            cmd.commit();
            acc_render += t.elapsed().as_secs_f64() * 1e3;
            inflight += 1;
            let _ = inflight;

            if frame_idx == 2 && pb.is_some() {
                cmd.wait_until_completed();
                if let Some((dbg_graded, dbg_out)) = &dbg_texs {
                    unsafe {
                        let pf = CVPixelBufferGetPixelFormatType(frame.pixel_buffer);
                        let b = pf.to_be_bytes();
                        eprintln!("   decode pixfmt 0x{:08x} '{}' · plano0 bpr={} · plano1 bpr={}",
                            pf, String::from_utf8_lossy(&b),
                            CVPixelBufferGetBytesPerRowOfPlane(frame.pixel_buffer, 0),
                            CVPixelBufferGetBytesPerRowOfPlane(frame.pixel_buffer, 1));
                    }
                    dump_texture(&gpu, &t_y, "/tmp/fl_dbg_y.pgm");
                    dump_texture(&gpu, &t_uv, "/tmp/fl_dbg_uv.ppm");
                    dump_texture(&gpu, dbg_graded, "/tmp/fl_dbg_graded.ppm");
                    dump_texture(&gpu, dbg_out, "/tmp/fl_dbg_out.ppm");
                }
                let pb2 = pb.unwrap();
                unsafe {
                    CVPixelBufferLockBaseAddress(pb2, 1);
                    for plane in 0..2usize {
                        let base = CVPixelBufferGetBaseAddressOfPlane(pb2, plane);
                        let bpr = CVPixelBufferGetBytesPerRowOfPlane(pb2, plane);
                        let pw = CVPixelBufferGetWidthOfPlane(pb2, plane);
                        let ph = CVPixelBufferGetHeightOfPlane(pb2, plane);
                        if !base.is_null() {
                            let px = std::slice::from_raw_parts(base as *const u8, 16);
                            eprintln!("   plano {}: {}x{} bpr={} datos={:?}", plane, pw, ph, bpr, px);
                        } else {
                            eprintln!("   plano {}: NULL ({}x{} bpr={})", plane, pw, ph, bpr);
                        }
                    }
                    CVPixelBufferUnlockBaseAddress(pb2, 1);
                }
            }
            let t = Instant::now();
            // el fotograma de entrada y sus texturas viajan CON el trabajo: se
            // sueltan cuando la GPU termina, no ahora (ver EncJob)
            if let (Some(tx), Some(pb)) = (etx.as_ref(), pb) {
                let _ = tx.send(EncJob(cmd.to_owned(), pb, frame_idx as i64,
                                       frame.pixel_buffer, (t_y, t_uv)));
            } else {
                pending_gpu.push_back((cmd.to_owned(), frame.pixel_buffer, (t_y, t_uv)));
                while pending_gpu.len() >= 4 {
                    let (c, entrada, texturas) = pending_gpu.pop_front().unwrap();
                    c.wait_until_completed();
                    drop(texturas);
                    unsafe { CFRelease(entrada as *mut std::ffi::c_void) };
                }
            }
            acc_encode += t.elapsed().as_secs_f64() * 1e3;
            acc_iter += t_iter.elapsed().as_secs_f64() * 1e3;
            frame_idx += 1;
            if frame_idx % 50 == 0 {
                eprint!("\r  {} frames · {:.1} fps   ", frame_idx, frame_idx as f64 / t_start.elapsed().as_secs_f64());
            }
            if frame_idx >= total { break; }
        }
    }
    drop(etx);
    let enc_stats = enc_join.map(|h| h.join().expect("hilo de encode"));
    while let Some((cmd, entrada, texturas)) = pending_gpu.pop_front() {
        cmd.wait_until_completed();
        drop(texturas);
        unsafe { CFRelease(entrada as *mut std::ffi::c_void) };
    }
    // libera lo que quede en la cola del decoder (p.ej. con --max-frames)
    dec.flush();
    while let Some(frame) = dec.pop() {
        unsafe { CFRelease(frame.pixel_buffer as *mut std::ffi::c_void) };
    }
    let el = t_start.elapsed().as_secs_f64();
    let n = frame_idx.max(1) as f64;
    eprintln!("\n✅ {} frames en {:.1}s = {:.1} fps e2e", frame_idx, el, frame_idx as f64 / el);
    eprintln!("   iter {:.2} | import/decode-out {:.1} · render {:.1} · backpressure-envío {:.1} · alloc+import-pb {:.2} ms/frame",
              acc_iter / n, acc_decode / n, acc_render / n, acc_encode / n, acc_alloc.get() / n);
    if let Some((wait_ms, enc_ms, count)) = enc_stats {
        let c = count.max(1) as f64;
        eprintln!("   hilo encode: gpu-wait {:.2} · vt-submit {:.2} ms/frame ({} frames)",
                  wait_ms / c, enc_ms / c, count);
    }
    // un render que no decodificó nada NO es un éxito: antes salía 0 con un
    // fichero degenerado y el shell lo concatenaba tan contento
    if frame_idx == 0 {
        eprintln!("❌ 0 frames decodificados — perfil no soportado o stream corrupto");
        std::process::exit(1);
    }
    if let Some((_, _, count)) = enc_stats {
        if count == 0 {
            eprintln!("❌ el encoder no emitió ningún frame");
            std::process::exit(1);
        }
    }
    // …y el fichero final tiene que existir con contenido (el mux puede fallar
    // aunque el encoder haya contado frames)
    if !bench {
        let final_path = if std::path::Path::new(&out).is_file() {
            out.clone()
        } else {
            format!("{}.mov", out.trim_end_matches(".mp4"))   // ProRes fuerza .mov
        };
        let sz = std::fs::metadata(&final_path).map(|m| m.len()).unwrap_or(0);
        if sz < 1024 {
            eprintln!("❌ salida vacía o inexistente: {final_path}");
            std::process::exit(1);
        }
    }
    // salida directa: los drops de teardown abortan (heap pisado por los shims
    // objc_msgSend de CMTime — malloc report en drop de AnnexB). El trabajo ya
    // está hecho y validado; el SO recupera todo. FIXME: cazar el scribble.
    std::process::exit(0);
}

/// EL REVELADO DE UNA BOBINA: leer el plan, compilarlo y recorrerlo.
fn revela_bobina(ruta: &str, luts_dir: Option<&str>,
                 desde: Option<usize>, cuantos: Option<usize>,
                 carrerilla: usize, out: Option<&str>) -> Result<()> {
    let cuerpo = if ruta == "-" {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s)?;
        s
    } else {
        std::fs::read_to_string(ruta)?
    };
    let payload: serde_json::Value = serde_json::from_str(&cuerpo)?;
    let mut plan = filmlook_metal::plan::compila(&payload).map_err(|e| anyhow::anyhow!(e))?;
    if plan.codec != "hevc" { std::env::set_var("FL_CODEC", &plan.codec); }

    // ── SOLO UN TRAMO (MOTOR §7) ──────────────────────────────────────
    // Recortar la tabla de renglones y ya está: el resto del motor no se
    // entera. La carrerilla son unos cuantos renglones de más por delante
    // que se revelan y no se escriben, para que el obturador llegue a la
    // primera imagen del tramo con su arrastre ya formado.
    if let Some(o) = out { plan.salida = o.to_string(); }
    let mut saltar = 0usize;
    if let Some(d) = desde {
        let d = d.min(plan.renglones.len());
        saltar = carrerilla.min(d);
        let ini = d - saltar;
        let fin = cuantos.map(|c| (d + c).min(plan.renglones.len()))
                         .unwrap_or(plan.renglones.len());
        plan.renglones = plan.renglones[ini..fin.max(ini)].to_vec();
        eprintln!("   tramo: renglones {d}..{fin} (+{saltar} de carrerilla)");
    }
    std::env::set_var("FL_SALTAR", saltar.to_string());

    // la identidad: sin gelatina, la señal pasa tal cual
    let ident: Vec<f32> = vec![0.,0.,0., 1.,0.,0., 0.,1.,0., 1.,1.,0.,
                               0.,0.,1., 1.,0.,1., 0.,1.,1., 1.,1.,1.];
    let dir = luts_dir.map(std::path::PathBuf::from);
    let mut pide = move |n: Option<&str>, ranura: &str| -> (u64, Vec<f32>) {
        let Some(n) = n else { return (2, ident.clone()) };
        // el nombre puede venir suelto o con ruta: se busca en su ranura
        let p = std::path::Path::new(n);
        let cand = if p.is_file() { p.to_path_buf() } else {
            match &dir {
                Some(d) => d.join(ranura).join(p.file_name().unwrap_or_default()),
                None => p.to_path_buf(),
            }
        };
        match parse_cube(&cand.to_string_lossy()) {
            Ok(v) => v,
            Err(e) => { eprintln!("   ⚠ gelatina «{}»: {e} — sigo sin ella", cand.display());
                        (2, ident.clone()) }
        }
    };
    let f = |p: &serde_json::Value, k: &str, d: f64| p[k].as_f64().unwrap_or(d) as f32;
    filmlook_metal::bobina::revela(&plan, &mut pide, &f)?;
    // los drops de teardown abortan con los shims de objc (ya documentado):
    // el trabajo está hecho y validado, el SO recupera lo demás
    std::process::exit(0);
}
