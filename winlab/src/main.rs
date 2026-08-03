//! winlab — el motor zero-copy de Windows (890M): MF decode → wgpu → encode.

mod amf;
mod chain;
mod d11;
mod interop;
mod blit;
mod mf_decode;
mod mf_encode;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::time::Instant;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Graphics::Dxgi::Common::*;

#[derive(Parser)]
#[command(name = "winlab", about = "film-look · motor GPU nativo Windows (MF+D3D11+wgpu)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// hito 1: decode HW puro (MF → P010 en GPU)
    BenchDecode {
        input: String,
        #[arg(long)]
        max_frames: Option<usize>,
    },
    /// hito 2: decode + cadena fílmica completa + pack (sin encoder)
    BenchRender {
        input: String,
        #[arg(long)]
        lut: Option<String>,
        #[arg(long)]
        lut_in: Option<String>,
        #[arg(long)]
        prefs: Option<String>,
        #[arg(long)]
        max_frames: Option<usize>,
    },
    /// techo del encoder MFT en solitario (un frame real repetido)
    BenchEncode {
        input: String,
        #[arg(long, default_value = "1000")]
        frames: usize,
    },
    /// el pipeline completo: decode → look → HEVC 10-bit → mp4 con audio
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
        bitrate: u32,
        #[arg(long)]
        max_frames: Option<usize>,
        /// EL CORTE, dentro del motor: segundo de la fuente por el que empezar.
        /// Sin esto había que cortar antes con ffmpeg, que decodifica y
        /// re-codifica el material entero — la mitad del tiempo del revelado
        /// y la razón de que la 890M se arrastrara (MOTOR §0).
        #[arg(long)]
        desde: Option<f64>,
        /// cuántos fotogramas servir desde ahí (la rejilla del proyecto)
        #[arg(long)]
        cuantos: Option<usize>,
        /// EL ENCUADRE, ocho números: conform + escala/giro/desplazamiento
        /// (a0,a1,a2,a3,b0,b1,b2,b3 — los mismos que calcula `plan::matriz`).
        /// Lo que cae fuera sale negro: el letterbox sin un filtro.
        #[arg(long)]
        enc: Option<String>,
        /// el lienzo del máster, si no es el de la fuente (WxH)
        #[arg(long)]
        lienzo: Option<String>,
        /// EL PASO DEL PROYECTO. Si no es el de la fuente, el motor sirve el
        /// fotograma que toca en cada instante del máster en vez de leer de
        /// corrido: sin esto una bobina a 25 con material a 59,94 salía
        /// estampada a 59,94 y con solo los primeros 18 s del clip.
        #[arg(long)]
        fps: Option<f64>,
    },
    /// LA BOBINA ENTERA de un tirón: sin corte, sin fase de fundidos y sin
    /// concatenación. El plan es el mismo JSON de timeline de siempre.
    Bobina {
        /// fichero con el plan (o «-» para leerlo de la entrada estándar)
        plan: String,
        /// dónde están las gelatinas (<taller>\luts\{entrada,color})
        #[arg(long)]
        luts: Option<String>,
        /// SOLO UN TRAMO: primer renglón. Es lo que permite recalcular un
        /// clip sin recalcular la bobina entera (MOTOR §7, caché fina).
        #[arg(long)]
        desde: Option<usize>,
        /// cuántos renglones desde ahí
        #[arg(long)]
        cuantos: Option<usize>,
        /// fotogramas de CARRERILLA antes del tramo: se revelan y se tiran,
        /// pero dejan la historia del obturador caliente
        #[arg(long, default_value = "8")]
        carrerilla: usize,
        /// adónde va este tramo (por defecto, lo que diga el plan)
        #[arg(long)]
        out: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::BenchDecode { input, max_frames } => bench_decode(&input, max_frames),
        Cmd::BenchRender { input, lut, lut_in, prefs, max_frames } =>
            run(&input, None, lut, lut_in, prefs, 0, max_frames, None, None, None, None, None),
        Cmd::BenchEncode { input, frames } => bench_encode(&input, frames),
        Cmd::Bobina { plan, luts, desde, cuantos, carrerilla, out } =>
            revela_bobina(&plan, luts.as_deref(), desde, cuantos, carrerilla, out),
        Cmd::Render { input, out, lut, lut_in, prefs, bitrate, max_frames,
                      desde, cuantos, enc, lienzo, fps } =>
            run(&input, Some(out), lut, lut_in, prefs, bitrate,
                cuantos.or(max_frames), desde, enc, lienzo, fps, None),
    }
}

fn frame_dev_check(tex: &ID3D11Texture2D, ours: &windows::Win32::Graphics::Direct3D11::ID3D11Device, label: &str) {
    use windows::core::Interface;
    unsafe {
        let dev: Option<windows::Win32::Graphics::Direct3D11::ID3D11Device> = tex.GetDevice().ok();
        if let Some(dv) = dev {
            eprintln!("   [{}] device: {:p} vs nuestro {:p} → {}", label,
                dv.as_raw(), ours.as_raw(),
                if dv.as_raw() == ours.as_raw() { "MISMO" } else { "¡DISTINTO!" });
        }
    }
}

fn bench_decode(input: &str, max_frames: Option<usize>) -> Result<()> {
    let d = d11::D11::new()?;
    let mut dec = mf_decode::MfDecoder::new(&d, input)?;
    eprintln!("🎞  {}x{} · {:.2} fps", dec.width, dec.height, dec.fps);
    let t0 = Instant::now();
    let mut n = 0usize;
    let total = max_frames.unwrap_or(usize::MAX);
    while let Some(f) = dec.next()? {
        drop(f);
        n += 1;
        if n >= total { break; }
    }
    let el = t0.elapsed().as_secs_f64();
    eprintln!("✅ decode puro: {} frames en {:.1}s = {:.1} fps", n, el, n as f64 / el);
    Ok(())
}

fn bench_encode(input: &str, frames: usize) -> Result<()> {
    let d = d11::D11::new()?;
    let mut dec = mf_decode::MfDecoder::new(&d, input)?;
    let (w, h) = (dec.width, if dec.height == 2176 { 2160 } else { dec.height });
    // un frame real → texturas compartidas → device del encoder
    let (y11, yh) = d.shared_tex(w, h, DXGI_FORMAT_R16_UNORM)?;
    let (uv11, uvh) = d.shared_tex(w / 2, h / 2, DXGI_FORMAT_R16G16_UNORM)?;
    let f = dec.next()?.ok_or_else(|| anyhow::anyhow!("sin frame"))?;
    let own = d.p010_tex(dec.width, dec.height)?;
    unsafe { d.ctx.CopyResource(&own, &f.tex) };
    let mut pb = blit::PlaneBlit::new(&d)?;
    pb.split(&d, &own, &y11, &uv11, w, h)?;
    d.flush_wait()?;
    drop(f);
    drop(dec);

    // anillo compartido con el frame real ensamblado en el slot 0
    let ring: Vec<_> = (0..8).map(|_| d.p010_shared_tex(w, h)).collect::<Result<Vec<_>>>()?;
    pb.merge(&d, &y11, &uv11, &ring[0].0, w, h)?;
    d.flush_wait()?;
    let ring_h: Vec<_> = ring.iter().map(|p| p.1).collect();
    let backend = std::env::var("WINLAB_ENC").unwrap_or_else(|_| "amf".into());
    eprintln!("backend: {}", backend);
    if backend == "mf" {
        let mut e = mf_encode::MfEncoder::new(w, h, 59.94, 40_000_000, &std::env::temp_dir().join("winlab_bench_enc.mp4").to_string_lossy(), input, &ring_h)?;
        let t0 = Instant::now();
        for i in 0..frames { e.submit_slot(0, i as i64)?; }
        e.finish()?;
        let el = t0.elapsed().as_secs_f64();
        eprintln!("✅ encoder solo (MFT): {} frames en {:.1}s = {:.1} fps", frames, el, frames as f64 / el);
        return Ok(());
    }
    let mut e = amf::AmfEncoder::new(&d, w, h, 59.94, 40_000_000, &std::env::temp_dir().join("winlab_bench_enc.mp4").to_string_lossy(), input, &ring_h)?;
    let t0 = Instant::now();
    for i in 0..frames {
        e.submit_slot(0, i as i64)?;
    }
    e.finish()?;
    let el = t0.elapsed().as_secs_f64();
    eprintln!("✅ encoder solo: {} frames en {:.1}s = {:.1} fps", frames, el, frames as f64 / el);
    let _ = y11; let _ = uv11;
    Ok(())
}

/// un slot del pool: texturas D3D11 compartidas + su cara wgpu
struct Slot {
    y11: ID3D11Texture2D,
    uv11: ID3D11Texture2D,
    y_view: wgpu::TextureView,
    uv_view: wgpu::TextureView,
    yh: HANDLE,
    uvh: HANDLE,
}

fn make_slot(d: &d11::D11, gpu: &interop::Gpu, w: u32, h: u32, out: bool) -> Result<Slot> {
    let (fy, fuv, wy, wuv) = if out {
        // TYPELESS: casteable a UINT para renderizar en wgpu Y copiable a los
        // planos UNORM del P010 (la copia typed UINT→UNORM se descarta en silencio)
        (DXGI_FORMAT_R16_TYPELESS, DXGI_FORMAT_R16G16_TYPELESS,
         wgpu::TextureFormat::R16Uint, wgpu::TextureFormat::Rg16Uint)
    } else {
        // UNORM tipado: los planos P010 son R16_UNORM-class; copia FULL-subresource
        (DXGI_FORMAT_R16_UNORM, DXGI_FORMAT_R16G16_UNORM,
         wgpu::TextureFormat::R16Unorm, wgpu::TextureFormat::Rg16Unorm)
    };
    let (y11, yh) = d.shared_tex(w, h, fy)?;
    let (uv11, uvh) = d.shared_tex(w / 2, h / 2, fuv)?;
    let usage = if out { wgpu::TextureUsages::RENDER_ATTACHMENT } else { wgpu::TextureUsages::TEXTURE_BINDING };
    let yt = interop::import_shared(gpu, yh, wy, w, h, usage)?;
    let uvt = interop::import_shared(gpu, uvh, wuv, w / 2, h / 2, usage)?;
    Ok(Slot {
        y11, uv11,
        y_view: yt.create_view(&Default::default()),
        uv_view: uvt.create_view(&Default::default()),
        yh, uvh,
    })
}

#[allow(clippy::too_many_arguments)]
fn run(
    input: &str,
    out: Option<String>,
    lut: Option<String>,
    lut_in: Option<String>,
    prefs_path: Option<String>,
    bitrate: u32,
    max_frames: Option<usize>,
    desde: Option<f64>,
    enc: Option<String>,
    lienzo: Option<String>,
    fps_salida: Option<f64>,
    // LA BOBINA. Cuando viene, el bucle deja de leer un clip de corrido y
    // recorre la tabla de renglones: cada fotograma sale de la fuente que diga
    // su renglón, con su receta y su encuadre, y las juntas se resuelven con un
    // segundo dibujo encima (MOTOR §5bis).
    mut bob: Option<PlanWin>,
) -> Result<()> {
    let prefs: serde_json::Value = match &prefs_path {
        Some(p) => serde_json::from_slice(&std::fs::read(p)?)?,
        None => serde_json::json!({}),
    };

    let d = d11::D11::new()?;
    let gpu = interop::init()?;
    // Con bobina las fuentes ya están abiertas y viven en `bob`; sin ella hay
    // un solo clip. El anillo de entrada se dimensiona con la PRIMERA fuente
    // con imagen: todas las de una bobina tienen que medir lo mismo (si no,
    // esto falla y el taller cae al camino de siempre).
    let (mut puestos, luts_cat) = match &bob {
        Some(b) => { let (p, c) = prepara_bobina(&d, &gpu, b)?; (p, c) }
        None => (Vec::new(), Vec::new()),
    };
    let mut dec: Option<mf_decode::MfDecoder> = match &bob {
        Some(_) => None,
        None => Some(mf_decode::MfDecoder::new(&d, input)?),
    };
    let (src_w, src_h, src_fps) = {
        let r = match (&dec, &bob) {
            (Some(x), _) => (x.width, x.height, x.fps),
            (_, Some(_)) => puestos.iter().filter_map(|p| p.dec.as_ref())
                .map(|x| (x.width, x.height, x.fps)).next()
                .ok_or_else(|| anyhow::anyhow!("la bobina no tiene ninguna fuente con imagen"))?,
            _ => anyhow::bail!("sin fuente"),
        };
        if bob.is_some() {
            for p in puestos.iter().filter_map(|p| p.dec.as_ref()) {
                anyhow::ensure!(p.width == r.0 && p.height == r.1,
                    "la bobina mezcla tamaños ({}x{} y {}x{}): aún no lo hace el motor",
                    r.0, r.1, p.width, p.height);
            }
        }
        r
    };
    // EL CORTE, aquí dentro: Media Foundation retrocede al fotograma clave
    // anterior y descarta el arranque, y esos fotogramas NO tocan la GPU del
    // look. Antes esto lo hacía ffmpeg re-codificando el material entero.
    // EL PASO DEL MÁSTER. La bobina puede ir a 25 con material a 59,94: el
    // motor sirve entonces el fotograma que cubre cada instante de salida,
    // que es la conversión de cadencia, y estampa el máster con ESE paso.
    let paso = bob.as_ref().map(|b| b.plan.fps)
        .or(fps_salida.filter(|f| *f > 0.5)).unwrap_or(src_fps);
    let remuestrea = (paso - src_fps).abs() > 0.001;
    if remuestrea {
        eprintln!("   cadencia: fuente {src_fps:.3} → máster {paso:.3} fps");
    }
    if let (Some(t), Some(dec)) = (desde, dec.as_mut()) {
        dec.busca(t)?;
        eprintln!("   corte en el motor: desde {t:.3} s");
        // el mux tiene que recortar el sonido por el mismo sitio
        std::env::set_var("FL_AUDIO_SS", format!("{t:.4}"));
        if let Some(n) = max_frames {
            std::env::set_var("FL_AUDIO_T", format!("{:.4}", n as f64 / paso.max(1.0)));
        }
    }
    // altura de display (el decoder alinea a 16/32)
    let (w, h) = (src_w, if src_h % 16 != 0 { src_h } else { src_h.min(2160.max(src_h / 16 * 16)) });
    let h = if src_h == 2176 { 2160 } else { h };
    // el alto VISIBLE de la fuente (el decoder alinea la superficie a 16/32):
    // es lo que hay que muestrear, no la superficie entera
    let src_visible = h;
    // el lienzo del máster puede no ser el de la fuente: ahí es donde el
    // encuadre hace de conform (y de letterbox) sin un solo filtro
    let (w, h) = match lienzo.as_deref().and_then(|s| {
        let (a, b) = s.split_once(['x', 'X'])?;
        Some((a.trim().parse::<u32>().ok()? & !1, b.trim().parse::<u32>().ok()? & !1))
    }) { Some(v) if v.0 >= 2 && v.1 >= 2 => v, _ => (w, h) };
    // el lienzo de la bobina manda sobre todo lo demás
    let (w, h) = bob.as_ref().map(|b| (b.plan.w & !1, b.plan.h & !1)).unwrap_or((w, h));
    eprintln!("🎞  {}x{} (superficie {}x{}) · {:.2} fps", w, h, src_w, src_h, paso);

    let grain = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/ui/assets/grain.bin");
    // 12 números: la afín (4+2), cuántas muestras por eje (2) y su paso (4)
    let matriz: Option<Vec<f32>> = enc.as_deref().and_then(|s| {
        let v: Vec<f32> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        (v.len() == 12).then_some(v)
    });
    let mut ch = chain::WinChain::new(
        &gpu.device, &gpu.queue, w, h, &prefs,
        lut_in.as_deref(), lut.as_deref(), &grain,
    )?;
    ch.grade_u.yuv_norm = src_visible as f32 / src_h as f32;   // padding del decoder
    // EL ENCUADRE del clip. Sin matriz, identidad: uv de lienzo = uv de
    // fuente. Con ella, el conform al lienzo del proyecto y el encuadre del
    // autor viajan en el shader, y lo que sale del cuadro se pinta negro —
    // que es el letterbox, y antes lo hacía un `scale`+`pad` de ffmpeg.
    if let Some(m) = matriz {
        ch.grade_u.enc_a = [m[0], m[1], m[2], m[3]];
        ch.grade_u.enc_b = [m[4], m[5], m[6], m[7]];
        ch.grade_u.paso = [m[8], m[9], m[10], m[11]];
        // el padding del decoder ya lo lleva la propia matriz por la V
        ch.grade_u.yuv_norm = src_visible as f32 / src_h as f32;
        eprintln!("   encuadre: [{:.4} {:.4} {:.4} {:.4}] giro [{:.4} {:.4}]",
                  m[0], m[1], m[2], m[3], m[4], m[5]);
    }

    const N: usize = 8;
    let ins: Vec<Slot> = (0..N).map(|_| make_slot(&d, &gpu, src_w, src_h, false)).collect::<Result<_>>()?;
    // P010 propios COMPARTIDOS: D3D11 recibe el frame (CopyResource) y la cola
    // D3D12 hace el split por plano (en D3D12 la copia planar sí funciona en AMD)
    let owns: Vec<_> = (0..N).map(|_| d.p010_shared_tex(src_w, src_h)).collect::<Result<Vec<_>>>()?;
    let mut held: std::collections::VecDeque<mf_decode::GpuFrame> = Default::default();
    let outs: Vec<Slot> = (0..N).map(|_| make_slot(&d, &gpu, w, h, true)).collect::<Result<_>>()?;
    // anillo P010 del encoder, también compartido (lo llena la cola D3D12)
    let encp: Vec<_> = (0..N).map(|_| d.p010_shared_tex(w, h)).collect::<Result<Vec<_>>>()?;

    // ── coreografía de fences (todo en GPU, cero esperas de CPU) ──
    // F_in : main-D3D11 → señala tras las copias de entrada; la cola D3D12 la espera
    // F_r  : cola D3D12 → señala tras el render; lo esperan encoder-D3D11 y main-D3D11
    // F_e  : encoder-D3D11 → señala tras sus copias; la cola D3D12 la espera (reuso out)
    let (f_in11, f_in_h) = d.create_shared_fence()?;
    let f_in12 = interop::open_fence(&gpu, f_in_h)?;
    let dev12 = interop::raw_device(&gpu)?;
    let queue12 = interop::raw_queue(&gpu)?;
    use windows::Win32::Graphics::Direct3D12::D3D12_FENCE_FLAG_SHARED;
    let f_r12: windows::Win32::Graphics::Direct3D12::ID3D12Fence =
        unsafe { dev12.CreateFence(0, D3D12_FENCE_FLAG_SHARED)? };
    let f_r_h = unsafe { dev12.CreateSharedHandle(&f_r12, None, windows::Win32::Foundation::GENERIC_ALL.0, None)? };
    let f_r11_main = d.open_shared_fence(f_r_h)?;
    let ctx4_main = d.ctx4()?;

    // ── listas D3D12 pregrabadas: split (own→in) y merge (out→encp) por slot ──
    use windows::core::Interface as _Iface;
    use windows::Win32::Graphics::Direct3D12::*;
    let loc12 = |r: &ID3D12Resource, sub: u32| D3D12_TEXTURE_COPY_LOCATION {
        pResource: std::mem::ManuallyDrop::new(Some(r.clone())),
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: sub },
    };
    let record = |pairs: &[(&ID3D12Resource, u32, &ID3D12Resource, u32)]| -> Result<ID3D12CommandList> {
        let alloc: ID3D12CommandAllocator = unsafe { dev12.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)? };
        let list: ID3D12GraphicsCommandList = unsafe { dev12.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &alloc, None)? };
        for (dst, dsub, src, ssub) in pairs {
            unsafe { list.CopyTextureRegion(&loc12(dst, *dsub), 0, 0, 0, &loc12(src, *ssub), None) };
        }
        unsafe { list.Close()? };
        std::mem::forget(alloc);   // el allocator vive lo que la lista (no se resetea)
        Ok(list.cast()?)
    };
    let mut pre_lists: Vec<Option<ID3D12CommandList>> = Vec::new();
    let mut post_lists: Vec<Option<ID3D12CommandList>> = Vec::new();
    for i in 0..N {
        let own12 = interop::open_resource12(&gpu, owns[i].1)?;
        let y12 = interop::open_resource12(&gpu, ins[i].yh)?;
        let uv12 = interop::open_resource12(&gpu, ins[i].uvh)?;
        pre_lists.push(Some(record(&[(&y12, 0, &own12, 0), (&uv12, 0, &own12, 1)])?));
        let oy12 = interop::open_resource12(&gpu, outs[i].yh)?;
        let ouv12 = interop::open_resource12(&gpu, outs[i].uvh)?;
        let ep12 = interop::open_resource12(&gpu, encp[i].1)?;
        post_lists.push(Some(record(&[(&ep12, 0, &oy12, 0), (&ep12, 1, &ouv12, 0)])?));
    }

    // hilo del encoder: recibe (slot, submission), espera la GPU, ensambla P010 y somete
    enum Enc { Mf(mf_encode::MfEncoder), Amf(amf::AmfEncoder) }
    impl Enc {
        fn d(&self) -> &d11::D11 {
            match self { Enc::Mf(e) => &e.d, Enc::Amf(e) => &e.d }
        }
        fn submit_slot(&mut self, slot: usize, idx: i64) -> Result<()> {
            match self { Enc::Mf(e) => e.submit_slot(slot, idx), Enc::Amf(e) => e.submit_slot(slot, idx) }
        }
        fn finish(self) -> Result<()> {
            match self { Enc::Mf(e) => e.finish(), Enc::Amf(e) => e.finish() }
        }
    }
    /// `idx` es el número del fotograma ESCRITO (la carrerilla ya descontada:
    /// es el pts y es el testigo que suelta el anillo); `revelado` es el del
    /// fotograma REVELADO, que es en lo que cuenta el testigo del render. No
    /// son lo mismo en cuanto hay carrerilla, y confundirlos hacía que el
    /// encoder leyera la casilla del anillo antes de que se revelara.
    struct EncMsg { slot: usize, sub: wgpu::SubmissionIndex, idx: i64, revelado: u64 }
    let out = if std::env::var("WINLAB_NOENC").is_ok() { None } else { out };
    let (enc_tx, enc_join, ack_rx) = if let Some(path) = &out {
        let backend = std::env::var("WINLAB_ENC").unwrap_or_else(|_| "amf".into());
        let ring_h: Vec<_> = encp.iter().map(|p| p.1).collect();
        let e = if backend == "mf" {
            Enc::Mf(mf_encode::MfEncoder::new(w, h, paso, bitrate, path, input, &ring_h)?)
        } else {
            Enc::Amf(amf::AmfEncoder::new(&d, w, h, paso, bitrate, path, input, &ring_h)?)
        };
        eprintln!("🎛  encoder: {} (fences GPU)", if backend == "mf" { "MFT" } else { "AMF" });
        // fences del encoder
        let f_r11_enc = e.d().open_shared_fence(f_r_h)?;
        let (f_e11, f_e_h) = e.d().create_shared_fence()?;
        let f_e12 = interop::open_fence(&gpu, f_e_h)?;
        let ctx4_enc = e.d().ctx4()?;
        let (tx, rx) = std::sync::mpsc::sync_channel::<EncMsg>(N - 1);
        let h = std::thread::spawn(move || -> Result<(f64, usize)> {
            let mut e = e;
            let mut wait_ms = 0.0f64;
            let mut count = 0usize;
            for msg in rx {
                let t = Instant::now();
                // espera CPU a que el render+merge D3D12 termine: garantiza que
                // el P010 está lleno antes de que el VCN lo lea. El testigo va
                // en fotogramas REVELADOS, no escritos (`revelado`, no `idx`).
                while unsafe { f_r11_enc.GetCompletedValue() } < msg.revelado {
                    std::hint::spin_loop();
                }
                e.submit_slot(msg.slot, msg.idx)?;
                // suelta el anillo para el merge de dentro de N frames
                unsafe { ctx4_enc.Signal(&f_e11, msg.idx as u64 + 1)? };
                wait_ms += t.elapsed().as_secs_f64() * 1e3;
                count += 1;
            }
            e.finish()?;
            Ok((wait_ms, count))
        });
        (Some(tx), Some(h), Some(f_e12))
    } else { (None, None, None) };

    if std::env::var("WINLAB_D12TEST").is_ok() && dec.is_some() {
        use windows::core::Interface;
        use windows::Win32::Graphics::Direct3D12::*;
        let dec = dec.as_mut().unwrap();
        let frame = dec.next()?.ok_or_else(|| anyhow::anyhow!("sin frame"))?;
        let (ownp, ownh) = d.p010_shared_tex(src_w, src_h)?;
        unsafe { d.ctx.CopyResource(&ownp, &frame.tex) };
        d.flush_wait()?;
        let own12 = interop::open_resource12(&gpu, ownh)?;
        let y12 = interop::open_resource12(&gpu, ins[0].yh)?;
        let uv12 = interop::open_resource12(&gpu, ins[0].uvh)?;
        let alloc: ID3D12CommandAllocator = unsafe { dev12.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)? };
        let list: ID3D12GraphicsCommandList = unsafe { dev12.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &alloc, None)? };
        let loc = |r: &ID3D12Resource, sub: u32| D3D12_TEXTURE_COPY_LOCATION {
            pResource: std::mem::ManuallyDrop::new(Some(r.clone())),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: sub },
        };
        unsafe {
            list.CopyTextureRegion(&loc(&y12, 0), 0, 0, 0, &loc(&own12, 0), None);
            list.CopyTextureRegion(&loc(&uv12, 0), 0, 0, 0, &loc(&own12, 1), None);
            list.Close()?;
            queue12.ExecuteCommandLists(&[Some(list.cast()?)]);
            queue12.Signal(&f_r12, 999_999)?;
        }
        while unsafe { f_r12.GetCompletedValue() } < 999_999 { std::hint::spin_loop(); }
        d.readback_stats(&ins[0].y11, "D12 plano Y")?;
        d.readback_stats(&ins[0].uv11, "D12 plano UV")?;
        return Ok(());
    }


    if std::env::var("WINLAB_D12TEST").is_ok() && dec.is_some() {
        use windows::core::Interface;
        use windows::Win32::Graphics::Direct3D12::*;
        let dec = dec.as_mut().unwrap();
        let frame = dec.next()?.ok_or_else(|| anyhow::anyhow!("sin frame"))?;
        let (ownp, ownh) = d.p010_shared_tex(src_w, src_h)?;
        unsafe { d.ctx.CopyResource(&ownp, &frame.tex) };
        d.flush_wait()?;
        let own12 = interop::open_resource12(&gpu, ownh)?;
        let y12 = interop::open_resource12(&gpu, ins[0].yh)?;
        let uv12 = interop::open_resource12(&gpu, ins[0].uvh)?;
        let alloc: ID3D12CommandAllocator = unsafe { dev12.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)? };
        let list: ID3D12GraphicsCommandList = unsafe { dev12.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &alloc, None)? };
        let loc = |r: &ID3D12Resource, sub: u32| D3D12_TEXTURE_COPY_LOCATION {
            pResource: std::mem::ManuallyDrop::new(Some(r.clone())),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: sub },
        };
        unsafe {
            list.CopyTextureRegion(&loc(&y12, 0), 0, 0, 0, &loc(&own12, 0), None);
            list.CopyTextureRegion(&loc(&uv12, 0), 0, 0, 0, &loc(&own12, 1), None);
            list.Close()?;
            queue12.ExecuteCommandLists(&[Some(list.cast()?)]);
            queue12.Signal(&f_r12, 999_999)?;
        }
        while unsafe { f_r12.GetCompletedValue() } < 999_999 { std::hint::spin_loop(); }
        d.readback_stats(&ins[0].y11, "D12 plano Y")?;
        d.readback_stats(&ins[0].uv11, "D12 plano UV")?;
        return Ok(());
    }

    let t0 = Instant::now();
    let mut n = 0usize;
    let total = max_frames.unwrap_or(usize::MAX);
    let mut acc_dec = 0.0f64;
    let mut acc_copy = 0.0f64;
    let mut acc_gpu = 0.0f64;
    let mut acc_enc = 0.0f64;
    let mut subs: std::collections::VecDeque<wgpu::SubmissionIndex> = Default::default();

    // LA CARRERILLA: los primeros renglones se revelan y se tiran, para que el
    // obturador llegue al tramo con su arrastre ya formado (MOTOR §7)
    let saltar: usize = std::env::var("FL_SALTAR").ok()
        .and_then(|v| v.parse().ok()).unwrap_or(0);
    if saltar > 0 { eprintln!("   carrerilla: {saltar} fotograma(s) que no se escriben"); }
    // cuántos fotogramas TIENE que escribir este tramo. Sólo en el camino de
    // bobina: en el de un solo clip el final lo marca el material.
    let cuantos_pedidos: Option<usize> = bob.as_ref()
        .map(|b| b.plan.renglones.len().saturating_sub(saltar));
    let t_cero = desde.unwrap_or(0.0);
    // el anillo del LADO B de una junta: durante un encadenado hacen falta dos
    // fuentes vivas a la vez. Es pequeño (los encadenados son cortos) y solo
    // se crea si la bobina tiene alguno.
    const M: usize = 4;
    let hay_junta = bob.as_ref().map(|b| b.plan.renglones.iter()
        .any(|r| r.fuente_b != filmlook_core::plan::NINGUNA)).unwrap_or(false);
    let (ins_b, owns_b, pre_b) = if hay_junta {
        let ib: Vec<Slot> = (0..M).map(|_| make_slot(&d, &gpu, src_w, src_h, false)).collect::<Result<_>>()?;
        let ob: Vec<_> = (0..M).map(|_| d.p010_shared_tex(src_w, src_h)).collect::<Result<Vec<_>>>()?;
        let mut pl: Vec<Option<ID3D12CommandList>> = Vec::new();
        for i in 0..M {
            let own12 = interop::open_resource12(&gpu, ob[i].1)?;
            let y12 = interop::open_resource12(&gpu, ib[i].yh)?;
            let uv12 = interop::open_resource12(&gpu, ib[i].uvh)?;
            pl.push(Some(record(&[(&y12, 0, &own12, 0), (&uv12, 0, &own12, 1)])?));
        }
        eprintln!("   carril B para las juntas: {M} hueco(s)");
        (ib, ob, pl)
    } else { (Vec::new(), Vec::new(), Vec::new()) };
    let mut held_b: std::collections::VecDeque<mf_decode::GpuFrame> = Default::default();
    // ── los carriles de LAS CAPAS (CAPAS §5): C y D, como el B ────────────
    // Sólo se crean si alguna capa es de vídeo; las de foto y rótulo van
    // residentes y no necesitan anillo.
    let es_video_capa = |b: &PlanWin, f: u32| f != filmlook_core::plan::NINGUNA
        && !b.plan.fuentes[f as usize].foto && !b.plan.fuentes[f as usize].hueco;
    let hay_capa_video = bob.as_ref().map(|b| b.plan.renglones.iter()
        .any(|r| es_video_capa(b, r.fuente_c) || es_video_capa(b, r.fuente_d)))
        .unwrap_or(false);
    let mut haz_carril = || -> Result<(Vec<Slot>, Vec<(ID3D11Texture2D, HANDLE)>,
                                       Vec<Option<ID3D12CommandList>>)> {
        let ib: Vec<Slot> = (0..M).map(|_| make_slot(&d, &gpu, src_w, src_h, false))
            .collect::<Result<_>>()?;
        let ob: Vec<_> = (0..M).map(|_| d.p010_shared_tex(src_w, src_h))
            .collect::<Result<Vec<_>>>()?;
        let mut pl: Vec<Option<ID3D12CommandList>> = Vec::new();
        for i in 0..M {
            let own12 = interop::open_resource12(&gpu, ob[i].1)?;
            let y12 = interop::open_resource12(&gpu, ib[i].yh)?;
            let uv12 = interop::open_resource12(&gpu, ib[i].uvh)?;
            pl.push(Some(record(&[(&y12, 0, &own12, 0), (&uv12, 0, &own12, 1)])?));
        }
        Ok((ib, ob, pl))
    };
    let (ins_c, owns_c, pre_c) = if hay_capa_video { haz_carril()? }
                                 else { (Vec::new(), Vec::new(), Vec::new()) };
    let (ins_d, owns_d, pre_d) = if hay_capa_video { haz_carril()? }
                                 else { (Vec::new(), Vec::new(), Vec::new()) };
    if hay_capa_video { eprintln!("   carriles C y D para capas de vídeo: {M} hueco(s)"); }
    let mut held_c: std::collections::VecDeque<mf_decode::GpuFrame> = Default::default();
    let mut held_d: std::collections::VecDeque<mf_decode::GpuFrame> = Default::default();

    loop {
        // EL RENGLÓN: de qué fuente sale este fotograma, en qué segundo y con
        // cuánto peso se encadena con el siguiente (MOTOR §5).
        let ren = bob.as_ref().map(|b| b.plan.renglones.get(n).copied());
        if matches!(ren, Some(None)) { break; }
        let ren = ren.flatten();

        let t = Instant::now();
        let siguiente = match &ren {
            Some(r) => {
                match puestos[r.fuente_a as usize].dec.as_mut() {
                    Some(dv) => dv.en(r.t_a)?,
                    None => None,           // hueco: negro, sin decodificar
                }
            }
            // un solo clip: de corrido si la cadencia coincide (lo más
            // rápido); por tiempo si hay que convertirla
            _ => {
                let dv = dec.as_mut().unwrap();
                if remuestrea { dv.en(t_cero + n as f64 / paso)? } else { dv.next()? }
            }
        };
        let hueco = ren.map(|r| puestos[r.fuente_a as usize].dec.is_none()).unwrap_or(false);
        // el HUECO de la bobina no decodifica nada: el fotograma se pinta
        // negro sacando el encuadre fuera de rango, y el hueco del anillo se
        // queda con lo que hubiera (que nadie muestrea)
        let frame = match siguiente {
            Some(f) => Some(f),
            None if hueco => None,
            None => break,
        };
        acc_dec += t.elapsed().as_secs_f64() * 1e3;

        let slot = &ins[n % N];
        let oslot = &outs[n % N];

        // copias de entrada: en GPU, esperando (EN GPU) a que el render viejo suelte el slot
        let t = Instant::now();
        if n >= N {
            unsafe { ctx4_main.Wait(&f_r11_main, (n - N) as u64 + 1)? };
        }
        if let Some(f) = &frame {
            unsafe { d.ctx.CopyResource(&owns[n % N].0, &f.tex) };
        }
        unsafe { ctx4_main.Signal(&f_in11, n as u64 + 1)? };
        unsafe { d.ctx.Flush() };
        // el sample vive en un anillo hasta que su split haya corrido en GPU
        // (MF recicla la textura del pool en cuanto lo soltamos)
        if let Some(f) = frame { held.push_back(f); }
        if held.len() > 4 {
            let m_old = (n - 4) as u64 + 1;
            while unsafe { f_in11.GetCompletedValue() } < m_old {
                std::hint::spin_loop();
            }
            held.pop_front();
        }
        acc_copy += t.elapsed().as_secs_f64() * 1e3;

        // cadena fílmica + pack, ordenada tras las copias (y tras el encoder si reusa out)
        let t = Instant::now();
        unsafe { queue12.Wait(&f_in12, n as u64 + 1)? };
        // el anillo del codificador cuenta fotogramas ESCRITOS, y los de
        // carrerilla no se escriben: hay que descontarlos o se espera por un
        // testigo que no va a llegar nunca (bloqueo al arrancar el tramo)
        if n >= N + saltar {
            if let Some(fe) = ack_rx.as_ref() {
                unsafe { queue12.Wait(fe, (n - saltar - N) as u64 + 1)? };
            }
        }
        unsafe { queue12.ExecuteCommandLists(&[pre_lists[n % N].clone()]) };
        // ── EL LADO B DE UNA JUNTA ────────────────────────────────────────
        // Se decodifica, se copia a su propio carril y se dibuja encima con su
        // peso. El pase caro corre UNA vez después, sobre la mezcla — que
        // además es como se revela de verdad una copia con doble exposición.
        let mut b_slot: Option<usize> = None;
        if let Some(r) = &ren {
            if r.fuente_b != filmlook_core::plan::NINGUNA && !ins_b.is_empty() {
                let ib = r.fuente_b as usize;
                if let Some(fb) = puestos[ib].dec.as_mut().and_then(|dv| dv.en(r.t_b).ok().flatten()) {
                    let k = n % M;
                    unsafe { d.ctx.CopyResource(&owns_b[k].0, &fb.tex) };
                    unsafe { ctx4_main.Signal(&f_in11, n as u64 + 1)? };
                    unsafe { d.ctx.Flush() };
                    held_b.push_back(fb);
                    if held_b.len() > M { held_b.pop_front(); }
                    unsafe { queue12.ExecuteCommandLists(&[pre_b[k].clone()]) };
                    b_slot = Some(k);
                }
            }
        }
        // ── LAS CAPAS DE VÍDEO: mismo trato que el lado B ────────────────
        let mut c_slot: Option<usize> = None;
        let mut d_slot: Option<usize> = None;
        if let Some(r) = &ren {
            if r.fuente_c != filmlook_core::plan::NINGUNA && !ins_c.is_empty() {
                let ic = r.fuente_c as usize;
                if let Some(fc) = puestos[ic].dec.as_mut()
                    .and_then(|dv| dv.en(r.t_c).ok().flatten()) {
                    let k = n % M;
                    unsafe { d.ctx.CopyResource(&owns_c[k].0, &fc.tex) };
                    unsafe { ctx4_main.Signal(&f_in11, n as u64 + 1)? };
                    unsafe { d.ctx.Flush() };
                    held_c.push_back(fc);
                    if held_c.len() > M { held_c.pop_front(); }
                    unsafe { queue12.ExecuteCommandLists(&[pre_c[k].clone()]) };
                    c_slot = Some(k);
                }
            }
            if r.fuente_d != filmlook_core::plan::NINGUNA && !ins_d.is_empty() {
                let id2 = r.fuente_d as usize;
                if let Some(fd) = puestos[id2].dec.as_mut()
                    .and_then(|dv| dv.en(r.t_d).ok().flatten()) {
                    let k = n % M;
                    unsafe { d.ctx.CopyResource(&owns_d[k].0, &fd.tex) };
                    unsafe { ctx4_main.Signal(&f_in11, n as u64 + 1)? };
                    unsafe { d.ctx.Flush() };
                    held_d.push_back(fd);
                    if held_d.len() > M { held_d.pop_front(); }
                    unsafe { queue12.ExecuteCommandLists(&[pre_d[k].clone()]) };
                    d_slot = Some(k);
                }
            }
        }
        let cmd = match &ren {
            Some(r) => {
                let ia = r.fuente_a as usize;
                let mut enc2 = gpu.device.create_command_encoder(&Default::default());
                // el lado A, con su receta y su encuadre
                let mut ga = puestos[ia].gu;
                ga.pad0 = puestos[ia].shutter;
                // EL ARRASTRE DEL OBTURADOR NO CRUZA EL EMPALME (`r.corte`):
                // un corte seco no tiene continuidad de luz, y sin esto el
                // primer fotograma del plano nuevo llevaba encima un 14 % del
                // anterior (el valor de la casa). En un encadenado sí sigue:
                // ahí las dos imágenes conviven de verdad.
                ga.pad1 = if n == 0 || r.corte { 1.0 } else { 0.0 };
                ga.peso = 1.0;
                if puestos[ia].dec.is_none() {
                    // hueco: todo fuera del cuadro → negro, sin decodificar
                    ga.enc_a = [1.0, 0.0, 0.0, 1.0];
                    ga.enc_b = [9.0, 9.0, 1.0, 1.0];
                }
                let (la, lb) = (puestos[ia].lut_a, puestos[ia].lut_b);
                let (va, vb) = (luts_cat[la].1.clone(), luts_cat[lb].1.clone());
                // LA FOTO NO SE DECODIFICA: su textura es la misma fotograma
                // tras fotograma, y entra por donde entraría el decodificador
                let (ya, uva) = match &puestos[ia].foto {
                    Some((vy, vuv)) => (vy, vuv),
                    None => (&slot.y_view, &slot.uv_view),
                };
                ch.revela_capa(&gpu.device, &gpu.queue, &mut enc2,
                               ya, uva, &ga, Some((&va, &vb)),
                               (n % N) as u64, 0, true, None, ia as u64);
                // el lado B: un dibujo más, con su peso. Eso es el fundido.
                let b_foto = (r.fuente_b != filmlook_core::plan::NINGUNA)
                    .then(|| puestos[r.fuente_b as usize].foto.is_some())
                    .unwrap_or(false);
                if b_slot.is_some() || b_foto {
                    let ib = r.fuente_b as usize;
                    let k = b_slot.unwrap_or(0);
                    let mut gb = puestos[ib].gu;
                    gb.pad0 = puestos[ib].shutter;
                    gb.pad1 = if n == 0 || r.corte { 1.0 } else { 0.0 };
                    gb.peso = r.peso_b;
                    let (lc, ld) = (puestos[ib].lut_a, puestos[ib].lut_b);
                    let (vc, vd) = (luts_cat[lc].1.clone(), luts_cat[ld].1.clone());
                    let (yb, uvb) = match &puestos[ib].foto {
                        Some((vy, vuv)) => (vy, vuv),
                        None => (&ins_b[k].y_view, &ins_b[k].uv_view),
                    };
                    ch.revela_capa(&gpu.device, &gpu.queue, &mut enc2,
                                   yb, uvb, &gb, Some((&vc, &vd)),
                                   k as u64, 1, true, None, ib as u64);
                }
                // ── LAS CAPAS: C y luego D, encima de todo (CAPAS §5) ──
                // `peso = alfa` de la capa; el alfa por píxel de un RGBA lo
                // multiplica el shader. pad1 = 1: un rótulo no arrastra
                // historia del obturador.
                for (ci, (fk, ak, kslot, ring)) in [
                    (r.fuente_c, r.alfa_c, c_slot, &ins_c),
                    (r.fuente_d, r.alfa_d, d_slot, &ins_d),
                ].into_iter().enumerate() {
                    if fk == filmlook_core::plan::NINGUNA { continue }
                    let ic = fk as usize;
                    let mut gc = puestos[ic].gu;
                    gc.pad0 = 0.0;
                    gc.pad1 = 1.0;
                    gc.peso = ak;
                    let (lc2, ld2) = (puestos[ic].lut_a, puestos[ic].lut_b);
                    let (vc2, vd2) = (luts_cat[lc2].1.clone(), luts_cat[ld2].1.clone());
                    let carril = 2 + ci as u64;
                    if let Some(v) = puestos[ic].capa_rgba.clone() {
                        ch.revela_capa(&gpu.device, &gpu.queue, &mut enc2,
                                       &slot.y_view, &slot.uv_view, &gc,
                                       Some((&vc2, &vd2)),
                                       (n % N) as u64, carril, true,
                                       Some(&v), ic as u64);
                    } else if let Some(k) = kslot {
                        ch.revela_capa(&gpu.device, &gpu.queue, &mut enc2,
                                       &ring[k].y_view, &ring[k].uv_view, &gc,
                                       Some((&vc2, &vd2)),
                                       k as u64, carril, true, None, ic as u64);
                    }
                }
                // la receta del pase caro es la del clip que manda ahora
                let manda = if r.fuente_b != filmlook_core::plan::NINGUNA && r.peso_b > 0.5 {
                    r.fuente_b as usize } else { ia };
                ch.comp_u = puestos[manda].comp;
                ch.weave = puestos[manda].weave;
                // el fundido a negro/blanco: una constante, sin segunda fuente
                ch.comp_u.fundido = r.nivel_color;
                ch.comp_u.fundido_color = r.color_fijo;
                ch.compone(&gpu.device, &gpu.queue, &mut enc2,
                           &oslot.y_view, &oslot.uv_view, n, paso);
                enc2.finish()
            }
            _ => ch.encode_frame(
                &gpu.device, &gpu.queue,
                &slot.y_view, &slot.uv_view,
                &oslot.y_view, &oslot.uv_view,
                n, paso,
            ),
        };
        let sub = gpu.queue.submit([cmd]);
        unsafe { queue12.ExecuteCommandLists(&[post_lists[n % N].clone()]) };
        unsafe { queue12.Signal(&f_r12, n as u64 + 1)? };
        acc_gpu += t.elapsed().as_secs_f64() * 1e3;

        if n == 5 && std::env::var("WINLAB_DEBUG").is_ok() {
            gpu.device.poll(wgpu::Maintain::Wait);
            d.readback_stats(&slot.y11, "entrada Y")?;
            d.readback_stats(&slot.uv11, "entrada UV")?;
            d.readback_stats(&outs[n % N].y11, "salida Y (pack)")?;
            d.readback_stats(&outs[n % N].uv11, "salida UV (pack)")?;
            d.readback_stats(&encp[n % N].0, "encoder P010 (merge D3D12)")?;
            d.dump_debug();
        }

        // el hilo del encoder recoge el testigo (espera en GPU, no aquí)
        if let Some(tx) = enc_tx.as_ref() {
            if n >= saltar {
                let t = Instant::now();
                tx.send(EncMsg { slot: n % N, sub: sub.clone(),
                                 idx: (n - saltar) as i64, revelado: n as u64 + 1 })
                    .map_err(|_| anyhow::anyhow!("hilo encoder caído"))?;
                acc_enc += t.elapsed().as_secs_f64() * 1e3;
            } else {
                // de carrerilla: la GPU trabaja (el obturador se calienta) y
                // el fotograma no se escribe. Se espera aquí para no adelantar
                // al anillo, que este fotograma no va a liberar nadie.
                gpu.device.poll(wgpu::Maintain::wait_for(sub.clone()));
            }
        }
        subs.push_back(sub);
        if subs.len() > 2 * N {
            let old = subs.pop_front().unwrap();
            gpu.device.poll(wgpu::Maintain::wait_for(old));   // límite de profundidad
        }

        n += 1;
        if n % 100 == 0 {
            eprint!("\r  {} frames · {:.1} fps  ", n, n as f64 / t0.elapsed().as_secs_f64());
        }
        if n >= total { break; }
    }
    gpu.device.poll(wgpu::Maintain::Wait);
    unsafe { d.ctx.Flush() };
    drop(enc_tx);
    let mut enc_stats = None;
    if let Some(h) = enc_join {
        enc_stats = Some(h.join().map_err(|_| anyhow::anyhow!("join encoder"))??);
    }
    drop(ack_rx);

    let el = t0.elapsed().as_secs_f64();
    let nf = n.max(1) as f64;
    eprintln!("\n✅ {} frames en {:.1}s = {:.1} fps e2e", n, el, n as f64 / el);
    eprintln!("   decode {:.2} · copia-planos {:.2} · gpu-submit {:.2} · envío-encoder {:.2} ms/frame",
              acc_dec / nf, acc_copy / nf, acc_gpu / nf, acc_enc / nf);
    if let Some((wait_ms, count)) = enc_stats {
        eprintln!("   hilo encoder: gpu-wait {:.2} ms/frame · {} frames", wait_ms / count.max(1) as f64, count);
    }
    // ── SI FALTAN FOTOGRAMAS, SE DICE ────────────────────────────────────
    // Un tramo que escribe menos de lo que se le pidió —o nada— salía por la
    // puerta con código 0 y el shell lo daba por bueno. El máster acababa
    // cortado en seco donde el `concat` se topaba con el fichero roto: al
    // autor le faltaba el último plano y no había ni un aviso en ninguna
    // parte. El motor del Mac ya comprobaba esto; éste no.
    if let (Some(esperados), Some((_, escritos))) = (cuantos_pedidos, enc_stats) {
        anyhow::ensure!(escritos == esperados,
                        "faltan fotogramas: se escribieron {escritos} de {esperados}");
    }
    Ok(())
}

// ── LA BOBINA ENTERA, DE UN TIRÓN (MOTOR §0 y §5) ─────────────────────────
//
// Antes, en Windows: ffmpeg cortaba cada pieza (decodificar + re-codificar el
// material entero), el motor le pasaba el look, otro ffmpeg re-codificaba el
// máster COMPLETO para meter los fundidos, y un tercero concatenaba.
//
// Ahora: se compila la bobina a una tabla de renglones (uno por fotograma de
// salida) y se recorre. Cada renglón dice de qué fuente sale el fotograma, en
// qué segundo, con qué receta y con cuánto peso se encadena con el siguiente.
// **El corte y los fundidos dejan de ser fases.**

/// UNA FOTO RESIDENTE (PENDIENTE §4bis.10): los dos planos de una imagen
/// subidos UNA vez, con el mismo empaquetado que entrega el decodificador
/// (código de 10 bits alineado arriba en 16). Devuelve (ancho, alto, Y, UV).
fn foto_residente(device: &wgpu::Device, queue: &wgpu::Queue, ruta: &str)
    -> Result<(u32, u32, wgpu::TextureView, wgpu::TextureView)>
{
    let (w, h, y, uv) = filmlook_core::foto::planos(std::path::Path::new(ruta))?;
    let sube = |fmt: wgpu::TextureFormat, tw: u32, th: u32, bytes: &[u8], fila: u32|
        -> wgpu::TextureView {
        let t = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("foto"),
            size: wgpu::Extent3d { width: tw, height: th, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: fmt,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &t, mip_level: 0,
                origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            bytes,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(fila),
                                          rows_per_image: Some(th) },
            wgpu::Extent3d { width: tw, height: th, depth_or_array_layers: 1 });
        t.create_view(&Default::default())
    };
    let vy = sube(wgpu::TextureFormat::R16Unorm, w, h,
                  bytemuck::cast_slice(&y), w * 2);
    let vuv = sube(wgpu::TextureFormat::Rg16Unorm, w / 2, h / 2,
                   bytemuck::cast_slice(&uv), (w / 2) * 4);
    Ok((w, h, vy, vuv))
}

/// una fuente de la bobina, ya preparada para revelar
pub struct Puesto {
    pub dec: Option<mf_decode::MfDecoder>,   // None = hueco (negro) o foto
    /// UNA FOTO O UN RÓTULO, subidos una vez y residentes (PENDIENTE
    /// §4bis.10). Sin esto, una bobina con una sola tarjeta de título caía al
    /// camino viejo entero (que funciona, pero es tres veces más lento).
    pub foto: Option<(wgpu::TextureView, wgpu::TextureView)>,
    /// la capa RGBA residente (CAPAS §5): rótulos y fotos CON su alfa
    pub capa_rgba: Option<wgpu::TextureView>,
    pub gu: filmlook_core::params::GradeU,
    pub comp: filmlook_core::params::CompU,
    pub shutter: f32,
    pub weave: f32,
    /// índices en el catálogo de gelatinas
    pub lut_a: usize,
    pub lut_b: usize,
}

/// lo que el bucle necesita saber de la bobina. Solo el PLAN: las fuentes y
/// las gelatinas se abren dentro del bucle, con **su** dispositivo. Abrirlas
/// fuera y pasarlas era un error de manual: los decodificadores cuelgan del
/// D3D11 con el que se crearon y las texturas del wgpu con el que se
/// subieron; el bucle abre los suyos, y copiar entre dispositivos distintos
/// no vale.
pub struct PlanWin {
    pub plan: filmlook_core::plan::Plan,
    pub luts_dir: Option<String>,
}

fn revela_bobina(ruta: &str, luts_dir: Option<&str>,
                 desde: Option<usize>, cuantos: Option<usize>,
                 carrerilla: usize, out: Option<String>) -> Result<()> {
    use filmlook_core::{params, plan as P};
    let cuerpo = if ruta == "-" {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut s)?;
        s
    } else {
        std::fs::read_to_string(ruta)?
    };
    let payload: serde_json::Value = serde_json::from_str(&cuerpo)?;
    let mut plan = P::compila(&payload).map_err(|e| anyhow::anyhow!(e))?;
    if let Some(o) = out { plan.salida = o; }
    // ── SOLO UN TRAMO (MOTOR §7) ──────────────────────────────────────
    // Recortar la tabla de renglones y ya está. La carrerilla son unos
    // cuantos de más por delante que se revelan y NO se escriben, para que el
    // obturador llegue a la primera imagen con su arrastre ya formado.
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
    eprintln!("🎞  bobina: {} fotograma(s) · {} fuente(s) · {}×{} @ {:.3} fps",
              plan.renglones.len(), plan.fuentes.len(), plan.w, plan.h, plan.fps);
    let salida = plan.salida.clone();
    let bitrate = plan.bitrate.clamp(1_000_000, 400_000_000) as u32;
    Ok(marcha(plan, luts_dir, salida, bitrate)?)
}

/// resuelve el nombre de una gelatina a su ruta dentro del taller
fn busca_lut(n: &str, ranura: &str, dir: Option<&str>) -> std::path::PathBuf {
    let p = std::path::Path::new(n);
    if p.is_file() { return p.to_path_buf(); }
    match dir {
        Some(d) => std::path::Path::new(d).join(ranura).join(
            p.file_name().unwrap_or_default()),
        None => p.to_path_buf(),
    }
}

/// Arranca el bucle con la bobina. Todo lo que necesita dispositivo (las
/// fuentes y las gelatinas) se abre dentro, en `prepara_bobina`.
fn marcha(plan: filmlook_core::plan::Plan, luts_dir: Option<&str>,
          salida: String, bitrate: u32) -> Result<()> {
    let bob = PlanWin { plan, luts_dir: luts_dir.map(String::from) };
    run("", Some(salida), None, None, None, bitrate,
        None, None, None, None, None, Some(bob))
}

/// Abre las fuentes y sube las gelatinas CON EL DISPOSITIVO DEL BUCLE.
fn prepara_bobina(d: &d11::D11, gpu: &interop::Gpu, b: &PlanWin)
        -> Result<(Vec<Puesto>, Vec<(wgpu::Texture, wgpu::TextureView)>)> {
    use filmlook_core::params;
    let mut catalogo: Vec<(wgpu::Texture, wgpu::TextureView)> = Vec::new();
    let mut indice: std::collections::HashMap<String, usize> = Default::default();
    let ident: Vec<f32> = vec![0.,0.,0., 1.,0.,0., 0.,1.,0., 1.,1.,0.,
                               0.,0.,1., 1.,0.,1., 0.,1.,1., 1.,1.,1.];
    let dir = b.luts_dir.as_deref();
    // EL CATÁLOGO DE GELATINAS, una vez y compartido: dos clips con la misma
    // receta no vuelven a subir la LUT (MOTOR §5).
    let mut pide = |n: Option<&str>, ranura: &str,
                    cat: &mut Vec<(wgpu::Texture, wgpu::TextureView)>,
                    idx: &mut std::collections::HashMap<String, usize>| -> (usize, u32) {
        let clave = format!("{ranura}:{}", n.unwrap_or(""));
        if let Some(&k) = idx.get(&clave) { return (k, cat[k].0.width()); }
        let (tam, datos) = match n {
            Some(nombre) => {
                let ruta = busca_lut(nombre, ranura, dir);
                match chain::parse_cube(&ruta.to_string_lossy()) {
                    Ok(v) => v,
                    Err(e) => { eprintln!("   ⚠ gelatina «{}»: {e} — sigo sin ella", ruta.display());
                                (2, ident.clone()) }
                }
            }
            None => (2, ident.clone()),
        };
        let t = filmlook_core::pipeline::make_3d_lut(&gpu.device, &gpu.queue, tam, &datos);
        cat.push(t);
        let k = cat.len() - 1;
        idx.insert(clave, k);
        (k, tam)
    };

    let (pw, ph) = (b.plan.w, b.plan.h);
    let mut puestos: Vec<Puesto> = Vec::new();
    for f in &b.plan.fuentes {
        let (ka, na) = pide(f.lut_in.as_deref(), "entrada", &mut catalogo, &mut indice);
        let (kb, nb) = pide(f.lut.as_deref(), "color", &mut catalogo, &mut indice);
        let (dec, foto, capa_rgba, dims_rgba) = if f.hueco {
            (None, None, None, None)
        } else if f.foto && f.capa {
            // UNA CAPA con foto o rótulo: RGBA residente CON su alfa
            let (w2, h2, datos) = filmlook_core::foto::rgba(
                std::path::Path::new(&f.fichero))
                .map_err(|e| anyhow::anyhow!("{}: {e}", f.fichero))?;
            let t = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("capa rgba"),
                size: wgpu::Extent3d { width: w2, height: h2, depth_or_array_layers: 1 },
                mip_level_count: 1, sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &t, mip_level: 0,
                    origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                &datos,
                wgpu::TexelCopyBufferLayout { offset: 0,
                    bytes_per_row: Some(w2 * 4), rows_per_image: None },
                wgpu::Extent3d { width: w2, height: h2, depth_or_array_layers: 1 });
            eprintln!("   capa RGBA residente: {} ({w2}×{h2})", f.fichero);
            let v = t.create_view(&Default::default());
            std::mem::forget(t);
            (None, None, Some(v), Some((w2, h2)))
        } else if f.foto {
            let (fw, fh, vy, vuv) = foto_residente(&gpu.device, &gpu.queue, &f.fichero)?;
            eprintln!("   foto residente: {} ({fw}×{fh})", f.fichero);
            (None, Some((fw, fh, vy, vuv)), None, None)
        } else {
            (Some(mf_decode::MfDecoder::new(d, &f.fichero)
                .map_err(|e| anyhow::anyhow!("{}: {e}", f.fichero))?), None, None, None)
        };
        let (sw, sh) = match (&dec, &foto, &dims_rgba) {
            (Some(x), _, _) => (x.width, x.height),
            (_, Some((fw, fh, _, _)), _) => (*fw, *fh),
            (_, _, Some((fw, fh))) => (*fw, *fh),
            _ => (pw, ph),
        };
        // el alto VISIBLE (el decoder alinea la superficie a 16/32). Una foto
        // no lleva relleno: lo que se sube es lo que hay.
        let visible = if foto.is_some() { sh } else if sh == 2176 { 2160 } else { sh };
        let (ma, mb, paso) = filmlook_core::plan::matriz_de(f, sw as f32, visible as f32,
                                                            pw as f32, ph as f32);
        let mut gu = params::grade_u(&f.prefs, sw, visible, na, nb,
                                     f.lut_in.is_some(), f.lut.is_some());
        gu.enc_a = ma;
        gu.enc_b = mb;
        gu.paso = paso;
        gu.yuv_norm = visible as f32 / sh as f32;
        // la semántica de capa (CAPAS §4): 2 = vídeo capa, 3 = RGBA capa
        if f.capa { gu.src_mode = if f.foto { 3 } else { 2 }; }
        puestos.push(Puesto {
            dec,
            foto: foto.map(|(_, _, vy, vuv)| (vy, vuv)),
            capa_rgba,
            gu,
            comp: params::comp_u(&f.prefs, pw, ph),
            shutter: f.prefs["shutter"].as_f64().unwrap_or(0.0) as f32,
            weave: f.prefs["weave"].as_f64().unwrap_or(0.0) as f32,
            lut_a: ka, lut_b: kb,
        });
    }
    Ok((puestos, catalogo))
}
