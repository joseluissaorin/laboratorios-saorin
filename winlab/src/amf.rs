//! AMF (Advanced Media Framework) por FFI directo: vtables C transcritos de
//! los headers públicos de AMD (GPUOpen AMF, public/include). El encoder HEVC
//! nativo aguanta la compartición del VCN mucho mejor que el MFT.

#![allow(non_snake_case, dead_code)]

use anyhow::{anyhow, ensure, Result};

#[allow(dead_code)]
fn winquiet<S: AsRef<std::ffi::OsStr>>(program: S) -> std::process::Command {
    #[allow(unused_mut)]
    let mut c = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c
}
use std::ffi::c_void;
use std::io::Write;
use std::process::{Child, ChildStdin, Command, Stdio};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;

use crate::d11::D11;

type AmfResult = i32;
const AMF_OK: AmfResult = 0;

// AMFVariantStruct: { AMF_VARIANT_TYPE type; union(16 bytes) }  → 24 bytes
#[repr(C)]
#[derive(Clone, Copy)]
struct AmfVariant {
    ty: i32,
    _pad: i32,
    val: [u64; 2],
}

impl AmfVariant {
    fn int64(v: i64) -> Self { AmfVariant { ty: 2, _pad: 0, val: [v as u64, 0] } }
    fn bool_(v: bool) -> Self { AmfVariant { ty: 1, _pad: 0, val: [v as u64, 0] } }
    fn size(w: i32, h: i32) -> Self {
        AmfVariant { ty: 5, _pad: 0, val: [(w as u32 as u64) | ((h as u32 as u64) << 32), 0] }
    }
    fn rate(num: i32, den: i32) -> Self {
        AmfVariant { ty: 7, _pad: 0, val: [(num as u32 as u64) | ((den as u32 as u64) << 32), 0] }
    }
}

// ── vtables (orden EXACTO de los headers C) ────────────────────────────────

#[repr(C)]
struct FactoryVtbl {
    CreateContext: unsafe extern "C" fn(*mut Factory, *mut *mut Ctx) -> AmfResult,
    CreateComponent: unsafe extern "C" fn(*mut Factory, *mut Ctx, *const u16, *mut *mut Comp) -> AmfResult,
    SetCacheFolder: usize,
    GetCacheFolder: usize,
    GetDebug: usize,
    GetTrace: usize,
    GetPrograms: usize,
}
#[repr(C)]
struct Factory { vtbl: *const FactoryVtbl }

#[repr(C)]
struct CtxVtbl {
    Acquire: unsafe extern "C" fn(*mut Ctx) -> i32,
    Release: unsafe extern "C" fn(*mut Ctx) -> i32,
    QueryInterface: usize,
    SetProperty: usize, GetProperty: usize, HasProperty: usize,
    GetPropertyCount: usize, GetPropertyAt: usize, Clear: usize,
    AddTo: usize, CopyTo: usize, AddObserver: usize, RemoveObserver: usize,
    Terminate: unsafe extern "C" fn(*mut Ctx) -> AmfResult,
    InitDX9: usize, GetDX9Device: usize, LockDX9: usize, UnlockDX9: usize,
    InitDX11: unsafe extern "C" fn(*mut Ctx, *mut c_void, i32) -> AmfResult,
    GetDX11Device: usize, LockDX11: usize, UnlockDX11: usize,
    InitOpenCL: usize, GetOpenCLContext: usize, GetOpenCLCommandQueue: usize,
    GetOpenCLDeviceID: usize, GetOpenCLComputeFactory: usize, InitOpenCLEx: usize,
    LockOpenCL: usize, UnlockOpenCL: usize,
    InitOpenGL: usize, GetOpenGLContext: usize, GetOpenGLDrawable: usize,
    LockOpenGL: usize, UnlockOpenGL: usize,
    InitXV: usize, GetXVDevice: usize, LockXV: usize, UnlockXV: usize,
    InitGralloc: usize, GetGrallocDevice: usize, LockGralloc: usize, UnlockGralloc: usize,
    AllocBuffer: usize, AllocSurface: usize, AllocAudioBuffer: usize,
    CreateBufferFromHostNative: usize, CreateSurfaceFromHostNative: usize,
    CreateSurfaceFromDX9Native: usize,
    CreateSurfaceFromDX11Native: unsafe extern "C" fn(*mut Ctx, *mut c_void, *mut *mut Surface, *mut c_void) -> AmfResult,
}
#[repr(C)]
struct Ctx { vtbl: *const CtxVtbl }

#[repr(C)]
struct CompVtbl {
    Acquire: unsafe extern "C" fn(*mut Comp) -> i32,
    Release: unsafe extern "C" fn(*mut Comp) -> i32,
    QueryInterface: usize,
    SetProperty: unsafe extern "C" fn(*mut Comp, *const u16, AmfVariant) -> AmfResult,
    GetProperty: usize, HasProperty: usize, GetPropertyCount: usize,
    GetPropertyAt: usize, Clear: usize, AddTo: usize, CopyTo: usize,
    AddObserver: usize, RemoveObserver: usize,
    // AMFPropertyStorageEx
    GetPropertiesInfoCount: usize, GetPropertyInfoAt: usize,
    GetPropertyInfo: usize, ValidateProperty: usize,
    // AMFComponent
    Init: unsafe extern "C" fn(*mut Comp, i32, i32, i32) -> AmfResult,
    ReInit: usize,
    Terminate: unsafe extern "C" fn(*mut Comp) -> AmfResult,
    Drain: unsafe extern "C" fn(*mut Comp) -> AmfResult,
    Flush: usize,
    SubmitInput: unsafe extern "C" fn(*mut Comp, *mut Data) -> AmfResult,
    QueryOutput: unsafe extern "C" fn(*mut Comp, *mut *mut Data) -> AmfResult,
    GetContext: usize, SetOutputDataAllocatorCB: usize, GetCaps: usize, Optimize: usize,
}
#[repr(C)]
struct Comp { vtbl: *const CompVtbl }

// AMFData / AMFBuffer / AMFSurface comparten los primeros 23 slots
#[repr(C)]
struct DataVtbl {
    Acquire: unsafe extern "C" fn(*mut Data) -> i32,
    Release: unsafe extern "C" fn(*mut Data) -> i32,
    QueryInterface: usize,
    SetProperty: usize, GetProperty: usize, HasProperty: usize,
    GetPropertyCount: usize, GetPropertyAt: usize, Clear: usize,
    AddTo: usize, CopyTo: usize, AddObserver: usize, RemoveObserver: usize,
    GetMemoryType: usize,
    Duplicate: usize, Convert: usize, Interop: usize,
    GetDataType: usize, IsReusable: usize,
    SetPts: unsafe extern "C" fn(*mut Data, i64),
    GetPts: usize,
    SetDuration: unsafe extern "C" fn(*mut Data, i64),
    GetDuration: usize,
    // AMFBuffer a partir de aquí
    SetSize: usize,
    GetSize: unsafe extern "C" fn(*mut Data) -> usize,
    GetNative: unsafe extern "C" fn(*mut Data) -> *mut c_void,
}
#[repr(C)]
struct Data { vtbl: *const DataVtbl }
type Surface = Data;   // usamos solo la porción AMFData del surface

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub struct AmfEncoder {
    pub d: D11,
    _lib: usize,
    ctx: *mut Ctx,
    comp: *mut Comp,
    p010: Vec<ID3D11Texture2D>,
    surfs: Vec<*mut Surface>,      // superficies AMF cacheadas (una por slot)
    next_slot: usize,
    fps: f64,
    mux: Option<ChildStdin>,
    child: Option<Child>,
    pub frames_out: usize,
}

use crate::mf_encode::recorte_audio;

unsafe impl Send for AmfEncoder {}

const RING: usize = 8;

impl AmfEncoder {
    pub fn new(_dev: &D11, width: u32, height: u32, fps: f64, bitrate: u32,
               out_path: &str, audio_src: &str,
               ring_h: &[windows::Win32::Foundation::HANDLE]) -> Result<Self> {
        // device PROPIO: compartir el contexto inmediato entre hilos serializa
        // (probado: 64 fps vs 85); el cruce se paga con fences compartidos
        let d = D11::new()?;

        // mux ffmpeg (idéntico al del MFT)
        let ffmpeg = if std::path::Path::new(r"C:\ProgramData\chocolatey\bin\ffmpeg.exe").exists() {
            r"C:\ProgramData\chocolatey\bin\ffmpeg.exe"
        } else { "ffmpeg" };
        let codec = std::env::var("WINLAB_CODEC").unwrap_or_else(|_| "hevc".into());
        let raw_fmt = if codec == "av1" { "obu" } else { "hevc" };
        let mut child = winquiet(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y",
                   "-f", raw_fmt, "-r", &format!("{:.3}", fps), "-i", "-"])
            // el recorte del sonido va PEGADO a su entrada, antes del `-i`:
            // ffmpeg es posicional y un `-b:a` suelto delante de un fichero
            // se toma como opción de ESA entrada («cannot be applied to input
            // url … Move this option before the file it belongs to»)
            .args(recorte_audio(audio_src))
            .args(if audio_src.is_empty() { vec!["-map", "0:v"] }
                  else { vec!["-map", "0:v", "-map", "1:a?"] })
            .args([
                   "-c:v", "copy", "-c:a", "aac", "-b:a", "256k",
                   "-bsf:v",
                   // EL COLOR, ESCRITO DENTRO DEL BITSTREAM.
                   //
                   // Con `-c:v copy` ffmpeg NO puede tocar el VUI que va dentro
                   // del HEVC: `-colorspace bt709` solo escribe la etiqueta del
                   // contenedor y el reproductor hace caso a la de dentro. El
                   // codificador de AMD estaba marcando el máster como
                   // **BT.2020 + PQ (HDR)** cuando los datos son Rec.709 SDR,
                   // así que cualquier reproductor le aplicaba un mapeo de tonos
                   // de HDR: quemados los altos, aplastados los bajos, y de
                   // regalo más contraste y más saturación que la preview.
                   //
                   // 1 = BT.709 en las tres · rango limitado (que es lo que
                   // produce el empaquetado: 64..940).
                   "hevc_metadata=colour_primaries=1:transfer_characteristics=1\
:matrix_coefficients=1:video_full_range_flag=0",
                   "-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709",
                   "-color_range", "tv",
                   out_path])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;
        let mux = child.stdin.take();

        // amfrt64.dll → AMFInit
        use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
        let lib = unsafe { LoadLibraryW(windows::core::PCWSTR(w("amfrt64.dll").as_ptr()))? };
        let init = unsafe { GetProcAddress(lib, windows::core::PCSTR(b"AMFInit\0".as_ptr())) }
            .ok_or_else(|| anyhow!("sin AMFInit"))?;
        type InitFn = unsafe extern "C" fn(u64, *mut *mut Factory) -> AmfResult;
        let init: InitFn = unsafe { std::mem::transmute(init) };

        // versión 1.4.x (major.minor en los 32 altos)
        let version: u64 = (1u64 << 48) | (4u64 << 32);
        let mut factory: *mut Factory = std::ptr::null_mut();
        ensure!(unsafe { init(version, &mut factory) } == AMF_OK && !factory.is_null(), "AMFInit falló");

        let mut ctx: *mut Ctx = std::ptr::null_mut();
        ensure!(unsafe { ((*(*factory).vtbl).CreateContext)(factory, &mut ctx) } == AMF_OK, "CreateContext");
        let dev_ptr = d.device.as_raw();
        ensure!(unsafe { ((*(*ctx).vtbl).InitDX11)(ctx, dev_ptr, 110) } == AMF_OK, "InitDX11");

        let comp_id = if codec == "av1" { "AMFVideoEncoderHW_AV1" } else { "AMFVideoEncoderHW_HEVC" };
        let mut comp: *mut Comp = std::ptr::null_mut();
        ensure!(unsafe {
            ((*(*factory).vtbl).CreateComponent)(factory, ctx, w(comp_id).as_ptr(), &mut comp)
        } == AMF_OK, "CreateComponent");

        let set = |name: &str, v: AmfVariant| -> AmfResult {
            unsafe { ((*(*comp).vtbl).SetProperty)(comp, w(name).as_ptr(), v) }
        };
        if codec == "av1" {
            set("Av1Usage", AmfVariant::int64(0));               // transcoding
            set("Av1QualityPreset", AmfVariant::int64(100));     // SPEED
            set("Av1Profile", AmfVariant::int64(1));             // main
            set("Av1ColorBitDepth", AmfVariant::int64(10));
            set("Av1TargetBitrate", AmfVariant::int64(bitrate as i64));
            let rc: i64 = std::env::var("WINLAB_RC").ok().and_then(|v| v.parse().ok()).unwrap_or(3);
            set("Av1RateControlMethod", AmfVariant::int64(rc));  // 3 = CBR (CQP=0 vía WINLAB_RC; no cambia la velocidad)
            if rc == 0 {
                let q: i64 = std::env::var("WINLAB_QP").ok().and_then(|v| v.parse().ok()).unwrap_or(96);
                set("Av1QIndexIntra", AmfVariant::int64(q));
                set("Av1QIndexInter", AmfVariant::int64(q));
            }
            // pre-análisis fuera pase lo que pase (roba tiempo de 3D a la cadena)
            set("Av1RateControlPreAnalysisEnable", AmfVariant::bool_(false));
            set("Av1PreEncodeEnable", AmfVariant::bool_(false));
            set("Av1GOPSize", AmfVariant::int64(300));
            set("Av1FrameSize", AmfVariant::size(width as i32, height as i32));
            set("Av1FrameRate", AmfVariant::rate((fps * 1000.0).round() as i32, 1000));
            let tiles: i64 = std::env::var("WINLAB_TILES").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
            set("Av1NumTilesPerFrame", AmfVariant::int64(tiles));
            set("Av1ScreenContentTools", AmfVariant::bool_(false));
            // señalización BT.709 en la cabecera de secuencia (si no, PQ/BT.2020)
            set("Av1OutputColorPrimaries", AmfVariant::int64(1));
            set("Av1OutputTransferCharacteristic", AmfVariant::int64(1));
            set("Av1OutputColorMatrix", AmfVariant::int64(1));
        } else {
            let usage: i64 = std::env::var("WINLAB_USAGE").ok()
                .and_then(|v| v.parse().ok()).unwrap_or(0);
            set("HevcUsage", AmfVariant::int64(usage));
            set("HevcQualityPreset", AmfVariant::int64(10));     // SPEED
            set("HevcProfile", AmfVariant::int64(2));            // Main10
            set("HevcColorBitDepth", AmfVariant::int64(10));
            set("HevcTargetBitrate", AmfVariant::int64(bitrate as i64));
            let rc: i64 = std::env::var("WINLAB_RC").ok().and_then(|v| v.parse().ok()).unwrap_or(3);
            set("HevcRateControlMethod", AmfVariant::int64(rc));
            if rc == 0 {
                let qp: i64 = std::env::var("WINLAB_QP").ok().and_then(|v| v.parse().ok()).unwrap_or(24);
                set("HevcQP_I", AmfVariant::int64(qp));
                set("HevcQP_P", AmfVariant::int64(qp));
            }
            set("HevcRateControlPreanalysisEnable", AmfVariant::bool_(false));
            set("HevcGOPSize", AmfVariant::int64(300));
            set("HevcFrameSize", AmfVariant::size(width as i32, height as i32));
            set("HevcFrameRate", AmfVariant::rate((fps * 1000.0).round() as i32, 1000));
        }

        // AMF_SURFACE_P010 = 10
        ensure!(unsafe { ((*(*comp).vtbl).Init)(comp, 10, width as i32, height as i32) } == AMF_OK,
                "encoder Init(P010)");

        // anillo P010 COMPARTIDO: lo llena la cola D3D12 con copias por plano;
        // aquí solo lo abrimos y lo envolvemos en superficies AMF
        let p010 = ring_h.iter().map(|&h| d.open_shared(h)).collect::<Result<Vec<_>>>()?;
        // superficies AMF creadas UNA vez por slot (no por frame)
        let mut surfs = Vec::with_capacity(RING);
        for t in &p010 {
            let mut s: *mut Surface = std::ptr::null_mut();
            ensure!(unsafe {
                ((*(*ctx).vtbl).CreateSurfaceFromDX11Native)(ctx, t.as_raw(), &mut s, std::ptr::null_mut())
            } == AMF_OK, "CreateSurfaceFromDX11Native");
            surfs.push(s);
        }

        Ok(AmfEncoder {
            d, _lib: lib.0 as usize, ctx, comp, p010, surfs, next_slot: 0, fps,
            mux, child: Some(child), frames_out: 0,
        })
    }

    fn poll_outputs(&mut self) -> Result<()> {
        loop {
            let mut data: *mut Data = std::ptr::null_mut();
            let r = unsafe { ((*(*self.comp).vtbl).QueryOutput)(self.comp, &mut data) };
            if r != AMF_OK || data.is_null() { break; }
            unsafe {
                let size = ((*(*data).vtbl).GetSize)(data);
                let ptr = ((*(*data).vtbl).GetNative)(data);
                if !ptr.is_null() && size > 0 {
                    let bytes = std::slice::from_raw_parts(ptr as *const u8, size);
                    if let Some(m) = self.mux.as_mut() { let _ = m.write_all(bytes); }
                    self.frames_out += 1;
                }
                ((*(*data).vtbl).Release)(data);
            }
        }
        Ok(())
    }

    /// el P010 del slot ya viene lleno (copias por plano en la cola D3D12)
    pub fn submit_slot(&mut self, slot: usize, frame_idx: i64) -> Result<()> {
        self.next_slot += 1;
        let surf = self.surfs[slot];
        unsafe {
            let pts = (frame_idx as f64 * 10_000_000.0 / self.fps).round() as i64;
            ((*(*surf).vtbl).SetPts)(surf, pts);
            ((*(*surf).vtbl).SetDuration)(surf, (10_000_000.0 / self.fps).round() as i64);
        }
        // somete; si la cola está llena, drena salidas y reintenta
        loop {
            let r = unsafe { ((*(*self.comp).vtbl).SubmitInput)(self.comp, surf) };
            if r == AMF_OK { break; }
            self.poll_outputs()?;
            std::thread::sleep(std::time::Duration::from_micros(200));
        }
        self.poll_outputs()?;
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        unsafe { ((*(*self.comp).vtbl).Drain)(self.comp) };
        // apura la cola con paciencia
        for _ in 0..600 {
            let before = self.frames_out;
            self.poll_outputs()?;
            if self.frames_out == before {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            if self.frames_out >= self.next_slot { break; }
        }
        unsafe {
            ((*(*self.comp).vtbl).Terminate)(self.comp);
            ((*(*self.comp).vtbl).Release)(self.comp);
            ((*(*self.ctx).vtbl).Terminate)(self.ctx);
            ((*(*self.ctx).vtbl).Release)(self.ctx);
        }
        if let Some(m) = self.mux.take() { drop(m); }
        if let Some(mut c) = self.child.take() {
            let out = c.wait_with_output()?;
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.trim().is_empty() { eprintln!("ffmpeg mux: {}", &err[..err.len().min(400)]); }
        }
        eprintln!("   encoder AMF: {} frames escritos", self.frames_out);
        Ok(())
    }
}
