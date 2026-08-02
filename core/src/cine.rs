//! El proyector en proceso: decode por hardware SIN procesos ni tuberías en
//! el camino interactivo. En Mac, VideoToolbox alimentado por el índice del
//! contenedor (seek O(1) al keyframe); en otras plataformas cae al camino
//! ffmpeg mientras no exista su backend nativo (MF ya está escrito en winlab).
//!
//! Contrato de frontera: un `Fotograma` son planos Y/U/V u16 listos para
//! `write_texture` (una copia por frame, el mínimo universal; el fast path
//! zero-copy se negocia después sin cambiar esta frontera).

use crate::indice::{Codec, Indice};
use anyhow::Result;
use std::path::Path;

#[derive(Clone)]
pub struct Fotograma {
    pub y: Vec<u16>,
    pub u: Vec<u16>,
    pub v: Vec<u16>,
    pub w: u32,
    pub h: u32,
    /// tiempo de presentación en segundos de la FUENTE
    pub pts: f64,
}

#[cfg(target_os = "macos")]
pub use mac::Cine;
#[cfg(target_os = "windows")]
pub use win::Cine;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub use generico::Cine;

// ═══════════════════════════════════════════════ macOS: VideoToolbox ═══

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use std::ffi::c_void;
    use std::fs::File;
    use std::io::{Read, Seek, SeekFrom};
    use std::sync::Mutex;

    // ── FFI mínima (C puro: VideoToolbox/CoreMedia/CoreVideo/CoreFoundation) ──
    #[allow(non_snake_case, non_camel_case_types)]
    mod ffi {
        use std::ffi::c_void;
        pub type OSStatus = i32;
        #[repr(C)]
        #[derive(Clone, Copy)]
        pub struct CMTime { pub value: i64, pub timescale: i32, pub flags: u32, pub epoch: i64 }
        pub type Ref = *mut c_void;
        pub type CRef = *const c_void;

        pub type VTDecompressionOutputCallback = unsafe extern "C" fn(
            refcon: *mut c_void, source: *mut c_void, status: OSStatus, flags: u32,
            image: Ref, pts: CMTime, dur: CMTime);
        #[repr(C)]
        pub struct CallbackRecord {
            pub cb: VTDecompressionOutputCallback,
            pub refcon: *mut c_void,
        }

        pub const FMT_X420: u32 = u32::from_be_bytes(*b"x420"); // 10-bit biplanar
        pub const FMT_420V: u32 = u32::from_be_bytes(*b"420v"); // NV12 8-bit
        pub const FMT_P010_FULL: u32 = u32::from_be_bytes(*b"xf20");
        pub const FMT_420F: u32 = u32::from_be_bytes(*b"420f");

        #[link(name = "VideoToolbox", kind = "framework")]
        #[link(name = "CoreVideo", kind = "framework")]
        #[link(name = "CoreMedia", kind = "framework")]
        #[link(name = "CoreFoundation", kind = "framework")]
        extern "C" {
            pub fn CFRelease(r: CRef);
            pub fn CFDictionaryCreateMutable(a: CRef, cap: isize, kcb: CRef, vcb: CRef) -> Ref;
            pub fn CFDictionarySetValue(d: Ref, k: CRef, v: CRef);
            pub fn CFNumberCreate(a: CRef, tipo: isize, valor: *const c_void) -> Ref;
            pub static kCFTypeDictionaryKeyCallBacks: [u8; 0];
            pub static kCFTypeDictionaryValueCallBacks: [u8; 0];
            pub static kCVPixelBufferPixelFormatTypeKey: CRef;
            /// allocator "no toques esta memoria": imprescindible para que el
            /// block buffer NO libere los Vec de Rust (doble free si es NULL)
            pub static kCFAllocatorNull: CRef;

            pub fn CMVideoFormatDescriptionCreateFromHEVCParameterSets(
                a: CRef, n: usize, ptrs: *const *const u8, tams: *const usize,
                nal_len: i32, ext: CRef, out: *mut Ref) -> OSStatus;
            pub fn CMVideoFormatDescriptionCreateFromH264ParameterSets(
                a: CRef, n: usize, ptrs: *const *const u8, tams: *const usize,
                nal_len: i32, out: *mut Ref) -> OSStatus;
            pub fn CMBlockBufferCreateWithMemoryBlock(
                a: CRef, mem: *mut c_void, len: usize, ba: CRef, src: CRef,
                off: usize, dlen: usize, flags: u32, out: *mut Ref) -> OSStatus;
            pub fn CMSampleBufferCreateReady(
                a: CRef, data: Ref, fmt: Ref, ns: isize, nt: isize, timing: CRef,
                nsz: isize, tams: *const usize, out: *mut Ref) -> OSStatus;
            pub fn VTDecompressionSessionCreate(
                a: CRef, fmt: Ref, spec: CRef, dest: CRef,
                cb: *const CallbackRecord, out: *mut Ref) -> OSStatus;
            pub fn VTDecompressionSessionDecodeFrame(
                s: Ref, sb: Ref, flags: u32, refcon: *mut c_void, outflags: *mut u32) -> OSStatus;
            pub fn VTDecompressionSessionWaitForAsynchronousFrames(s: Ref) -> OSStatus;
            pub fn VTDecompressionSessionInvalidate(s: Ref);
            pub fn CVPixelBufferGetPixelFormatType(pb: Ref) -> u32;
            pub fn CVPixelBufferLockBaseAddress(pb: Ref, f: u64) -> OSStatus;
            pub fn CVPixelBufferUnlockBaseAddress(pb: Ref, f: u64) -> OSStatus;
            pub fn CVPixelBufferGetBytesPerRowOfPlane(pb: Ref, p: usize) -> usize;
            pub fn CVPixelBufferGetWidthOfPlane(pb: Ref, p: usize) -> usize;
            pub fn CVPixelBufferGetHeightOfPlane(pb: Ref, p: usize) -> usize;
            pub fn CVPixelBufferGetBaseAddressOfPlane(pb: Ref, p: usize) -> *mut c_void;
        }
    }

    /// la salida de la sesión: el callback (síncrono) deja aquí el pixel buffer
    struct Salida(Mutex<Vec<(ffi::Ref, i64)>>);

    unsafe extern "C" fn al_frame(
        refcon: *mut c_void, source: *mut c_void, status: ffi::OSStatus, _flags: u32,
        image: ffi::Ref, _pts: ffi::CMTime, _dur: ffi::CMTime,
    ) {
        if status != 0 || image.is_null() { return; }
        // el pts real viaja en `source` (el índice de muestra que alimentamos)
        let idx = source as i64;
        unsafe {
            extern "C" { fn CFRetain(r: *const c_void) -> *const c_void; }
            CFRetain(image as *const c_void);
        }
        let salida = &*(refcon as *const Salida);
        salida.0.lock().unwrap().push((image, idx));
    }

    struct Sesion {
        s: ffi::Ref,
        fmt: ffi::Ref,
        salida: Box<Salida>,
    }
    unsafe impl Send for Sesion {}

    impl Sesion {
        fn nueva(codec: &Codec) -> Result<Self> {
            let mut fmt: ffi::Ref = std::ptr::null_mut();
            let (st, quiere_fmt) = unsafe {
                match codec {
                    Codec::Hevc { vps, sps, pps, nal_len } => {
                        let ptrs = [vps.as_ptr(), sps.as_ptr(), pps.as_ptr()];
                        let tams = [vps.len(), sps.len(), pps.len()];
                        (ffi::CMVideoFormatDescriptionCreateFromHEVCParameterSets(
                            std::ptr::null(), 3, ptrs.as_ptr(), tams.as_ptr(),
                            *nal_len as i32, std::ptr::null(), &mut fmt), ffi::FMT_X420)
                    }
                    Codec::H264 { sps, pps, nal_len } => {
                        let ptrs = [sps.as_ptr(), pps.as_ptr()];
                        let tams = [sps.len(), pps.len()];
                        (ffi::CMVideoFormatDescriptionCreateFromH264ParameterSets(
                            std::ptr::null(), 2, ptrs.as_ptr(), tams.as_ptr(),
                            *nal_len as i32, &mut fmt), ffi::FMT_420V)
                    }
                }
            };
            anyhow::ensure!(st == 0, "format description falló: {st}");

            let salida = Box::new(Salida(Mutex::new(Vec::new())));
            let cb = ffi::CallbackRecord {
                cb: al_frame,
                refcon: &*salida as *const Salida as *mut c_void,
            };
            // pedir EXPLÍCITAMENTE el formato biplanar esperado (la trampa del
            // 'p420' empaquetado está documentada en el motor de render)
            let dest = unsafe {
                let d = ffi::CFDictionaryCreateMutable(
                    std::ptr::null(), 2,
                    &ffi::kCFTypeDictionaryKeyCallBacks as *const _ as ffi::CRef,
                    &ffi::kCFTypeDictionaryValueCallBacks as *const _ as ffi::CRef);
                let pf = quiere_fmt.to_le_bytes();
                let num = ffi::CFNumberCreate(std::ptr::null(), 3, pf.as_ptr() as *const c_void);
                ffi::CFDictionarySetValue(d, ffi::kCVPixelBufferPixelFormatTypeKey, num as ffi::CRef);
                ffi::CFRelease(num as ffi::CRef);
                d
            };
            let mut s: ffi::Ref = std::ptr::null_mut();
            let st = unsafe {
                ffi::VTDecompressionSessionCreate(
                    std::ptr::null(), fmt, std::ptr::null(), dest as ffi::CRef, &cb, &mut s)
            };
            unsafe { ffi::CFRelease(dest as ffi::CRef) };
            anyhow::ensure!(st == 0, "decompression session falló: {st}");
            Ok(Sesion { s, fmt, salida })
        }

        /// decode SÍNCRONO de una muestra AVCC; devuelve los buffers emitidos
        fn decodifica(&self, datos: &mut [u8], idx: usize) -> Vec<(ffi::Ref, i64)> {
            unsafe {
                let mut bb: ffi::Ref = std::ptr::null_mut();
                let st = ffi::CMBlockBufferCreateWithMemoryBlock(
                    std::ptr::null(), datos.as_mut_ptr() as *mut c_void, datos.len(),
                    ffi::kCFAllocatorNull, std::ptr::null(), 0, datos.len(), 0, &mut bb);
                if st != 0 { return Vec::new(); }
                let tam = datos.len();
                let mut sb: ffi::Ref = std::ptr::null_mut();
                let st = ffi::CMSampleBufferCreateReady(
                    std::ptr::null(), bb, self.fmt, 1, 0, std::ptr::null(), 1, &tam, &mut sb);
                if st == 0 {
                    ffi::VTDecompressionSessionDecodeFrame(
                        self.s, sb, 0, idx as *mut c_void, std::ptr::null_mut());
                    ffi::CFRelease(sb as ffi::CRef);
                }
                ffi::CFRelease(bb as ffi::CRef);
            }
            std::mem::take(&mut *self.salida.0.lock().unwrap())
        }

        fn purga(&self) {
            unsafe { ffi::VTDecompressionSessionWaitForAsynchronousFrames(self.s) };
            for (pb, _) in std::mem::take(&mut *self.salida.0.lock().unwrap()) {
                unsafe { ffi::CFRelease(pb as ffi::CRef) };
            }
        }
    }

    impl Drop for Sesion {
        fn drop(&mut self) {
            self.purga();
            unsafe {
                ffi::VTDecompressionSessionInvalidate(self.s);
                ffi::CFRelease(self.s as ffi::CRef);
                ffi::CFRelease(self.fmt as ffi::CRef);
            }
        }
    }

    /// planos u16 desde el pixel buffer (una copia; deinterleave del croma).
    /// `paso` = 2 decima a media resolución DURANTE la copia (la preview no
    /// necesita 4K completo: 4× menos bytes que mover y subir).
    fn a_planos(pb: ffi::Ref, pts: f64, paso: usize) -> Option<Fotograma> {
        unsafe {
            if ffi::CVPixelBufferLockBaseAddress(pb, 1) != 0 { return None; }
            let fmt = ffi::CVPixelBufferGetPixelFormatType(pb);
            let w = ffi::CVPixelBufferGetWidthOfPlane(pb, 0);
            let h = ffi::CVPixelBufferGetHeightOfPlane(pb, 0);
            let cw = ffi::CVPixelBufferGetWidthOfPlane(pb, 1);
            let ch = ffi::CVPixelBufferGetHeightOfPlane(pb, 1);
            let bpr0 = ffi::CVPixelBufferGetBytesPerRowOfPlane(pb, 0);
            let bpr1 = ffi::CVPixelBufferGetBytesPerRowOfPlane(pb, 1);
            let p0 = ffi::CVPixelBufferGetBaseAddressOfPlane(pb, 0) as *const u8;
            let p1 = ffi::CVPixelBufferGetBaseAddressOfPlane(pb, 1) as *const u8;
            if p0.is_null() || p1.is_null() {
                ffi::CVPixelBufferUnlockBaseAddress(pb, 1);
                return None;
            }
            let diez_bits = fmt == ffi::FMT_X420 || fmt == ffi::FMT_P010_FULL;
            let (ow, oh) = ((w / paso) & !1, (h / paso) & !1);
            let (ocw, och) = (ow / 2, oh / 2);
            let mut y = vec![0u16; ow * oh];
            let mut u = vec![0u16; ocw * och];
            let mut v = vec![0u16; ocw * och];
            // la cadena espera códigos de 10 bits (yuv_norm = 1023, como el
            // camino yuv420p10le): P010 viene MSB-aligned (>>6) y el 8-bit sube (<<2)
            if diez_bits {
                for fila in 0..oh {
                    let src = std::slice::from_raw_parts(p0.add((fila * paso).min(h - 1) * bpr0) as *const u16, w);
                    let dst = &mut y[fila * ow..(fila + 1) * ow];
                    for c in 0..ow { dst[c] = src[c * paso] >> 6; }
                }
                for fila in 0..och {
                    let src = std::slice::from_raw_parts(p1.add((fila * paso).min(ch - 1) * bpr1) as *const u16, cw * 2);
                    let (du, dv) = (&mut u[fila * ocw..], &mut v[fila * ocw..]);
                    for c in 0..ocw {
                        let s = (c * paso).min(cw - 1);
                        du[c] = src[s * 2] >> 6;
                        dv[c] = src[s * 2 + 1] >> 6;
                    }
                }
            } else {
                for fila in 0..oh {
                    let src = std::slice::from_raw_parts(p0.add((fila * paso).min(h - 1) * bpr0), w);
                    let dst = &mut y[fila * ow..(fila + 1) * ow];
                    for c in 0..ow { dst[c] = (src[c * paso] as u16) << 2; }
                }
                for fila in 0..och {
                    let src = std::slice::from_raw_parts(p1.add((fila * paso).min(ch - 1) * bpr1), cw * 2);
                    let (du, dv) = (&mut u[fila * ocw..], &mut v[fila * ocw..]);
                    for c in 0..ocw {
                        let s = (c * paso).min(cw - 1);
                        du[c] = (src[s * 2] as u16) << 2;
                        dv[c] = (src[s * 2 + 1] as u16) << 2;
                    }
                }
            }
            ffi::CVPixelBufferUnlockBaseAddress(pb, 1);
            Some(Fotograma { y, u, v, w: ow as u32, h: oh as u32, pts })
        }
    }

    pub struct Cine {
        pub ind: Indice,
        f: File,
        sesion: Sesion,
        /// decima a media resolución en la copia (la preview no necesita 4K)
        pub mitad: bool,
        /// siguiente muestra a alimentar (orden decode)
        pos: usize,
        /// posición en el orden de PANTALLA (índice en orden_pts)
        sig_pantalla: usize,
        /// frames decodificados aún no entregados (reordenación B-frames)
        buf: Vec<(usize, Fotograma)>,
        lee_buf: Vec<u8>,
        /// el último fotograma servido: pedir el mismo punto dos veces
        /// (refinado en pausa → play) no repaga el GOP entero
        cacheado: Option<(usize, Fotograma)>,
    }

    impl Cine {
        pub fn abre(ruta: &Path) -> Result<Self> {
            let ind = Indice::abre(ruta)?;
            let f = File::open(ruta)?;
            let sesion = Sesion::nueva(&ind.codec)?;
            Ok(Cine { ind, f, sesion, mitad: false, pos: 0, sig_pantalla: 0,
                      buf: Vec::new(), lee_buf: Vec::new(), cacheado: None })
        }

        pub fn info(&self) -> (u32, u32, f64, f64) {
            (self.ind.w, self.ind.h, self.ind.fps, self.ind.dur)
        }

        fn paso(&self) -> usize {
            if self.mitad && self.ind.w > 2200 { 2 } else { 1 }
        }

        fn alimenta(&mut self, idx: usize, convierte: bool) {
            let Some(m) = self.ind.muestras.get(idx).copied() else { return };
            self.lee_buf.resize(m.tam as usize, 0);
            if self.f.seek(SeekFrom::Start(m.off)).is_err() { return; }
            if self.f.read_exact(&mut self.lee_buf).is_err() { return; }
            let mut datos = std::mem::take(&mut self.lee_buf);
            let salidas = self.sesion.decodifica(&mut datos, idx);
            self.lee_buf = datos;
            let paso = self.paso();
            for (pb, sidx) in salidas {
                let sidx = sidx as usize;
                if convierte {
                    let pts = self.ind.pts_s(sidx);
                    if let Some(fr) = a_planos(pb, pts, paso) {
                        self.buf.push((sidx, fr));
                    }
                }
                unsafe { ffi::CFRelease(pb as ffi::CRef) };
            }
        }

        /// seek exacto: decodifica del keyframe al objetivo y devuelve SOLO ese
        pub fn frame_en(&mut self, t: f64) -> Option<Fotograma> {
            let i = self.ind.muestra_en(t);
            // ¿es el punto recién servido y el decoder sigue ahí? (pausa→play):
            // de la caché, gratis — no se repaga el GOP
            if let Some((ci, fr)) = &self.cacheado {
                if *ci == i && self.pos == i + 1 { return Some(fr.clone()); }
            }
            let k = self.ind.keyframe_para(i);
            // si venimos decodificando este mismo GOP, continuar sin reabrir
            let desde = if self.pos > k && self.pos <= i { self.pos } else { k };
            if desde == k { self.buf.clear(); }
            // EL ORDEN DE PANTALLA DECIDE QUÉ SE GUARDA. Antes se convertía
            // solo el objetivo y después se vaciaba el búfer entero: con
            // fotogramas B eso tira trabajo que hacía falta —en un IBBBP el P
            // se decodifica ANTES que las tres B que van delante suyo en
            // pantalla—, así que al reproducir tras un salto faltaban
            // fotogramas y la imagen daba tirones. Se convierte y se guarda lo
            // que caiga DESPUÉS del objetivo en pantalla, que en la práctica
            // son los dos o tres de la reordenación, no el GOP.
            let pantalla_obj = self.ind.pantalla_de(i);
            for idx in desde..=i {
                let hace_falta = idx == i || self.ind.pantalla_de(idx) > pantalla_obj;
                self.alimenta(idx, hace_falta);
            }
            self.pos = i + 1;
            // dejar el cursor de pantalla apuntando al frame siguiente al objetivo
            self.sig_pantalla = pantalla_obj + 1;
            let obj = self.buf.iter().position(|(idx, _)| *idx == i)?;
            let (_, fr) = self.buf.swap_remove(obj);
            // lo que ya no se va a ver, fuera; lo de después, se queda
            let ind = &self.ind;
            let sig = self.sig_pantalla;
            self.buf.retain(|(idx, _)| ind.pantalla_de(*idx) >= sig);
            self.cacheado = Some((i, fr.clone()));
            Some(fr)
        }

        /// prepara reproducción desde t (decodifica hasta tener el primer frame listo)
        pub fn arranca_en(&mut self, t: f64) -> Option<Fotograma> {
            self.frame_en(t)
        }

        /// SOLO el fotograma clave anterior a t: un decode, cero catch-up.
        /// Es el fotograma del scrub sin proxy (el exacto llega con el refinado).
        pub fn frame_clave(&mut self, t: f64) -> Option<Fotograma> {
            let i = self.ind.muestra_en(t);
            let k = self.ind.keyframe_para(i);
            self.buf.clear();
            self.alimenta(k, true);
            self.pos = k + 1;
            self.sig_pantalla = self.ind.pantalla_de(k) + 1;
            let obj = self.buf.iter().position(|(idx, _)| *idx == k)?;
            let (_, fr) = self.buf.swap_remove(obj);
            self.buf.clear();
            self.cacheado = Some((k, fr.clone()));
            Some(fr)
        }

        /// el fotograma del SCRUB: exacto si es barato (mismo GOP, pocos
        /// pasos), el keyframe si el catch-up saldría caro
        pub fn frame_scrub(&mut self, t: f64) -> Option<Fotograma> {
            let i = self.ind.muestra_en(t);
            let k = self.ind.keyframe_para(i);
            let desde = if self.pos > k && self.pos <= i { self.pos } else { k };
            if i + 1 - desde <= 10 { self.frame_en(t) } else { self.frame_clave(t) }
        }

        /// siguiente fotograma en orden de pantalla; None = fin del stream
        pub fn siguiente(&mut self) -> Option<Fotograma> {
            let esperado = *self.ind.orden_pts.get(self.sig_pantalla)? as usize;
            // decodificar hasta que el esperado esté en el buffer (reorden B-frames)
            let mut intentos = 0;
            while !self.buf.iter().any(|(i, _)| *i == esperado) {
                if self.pos >= self.ind.muestras.len() || intentos > 16 { break; }
                let idx = self.pos;
                self.pos += 1;
                self.alimenta(idx, true);
                intentos += 1;
            }
            let p = self.buf.iter().position(|(i, _)| *i == esperado)?;
            let (_, fr) = self.buf.swap_remove(p);
            self.sig_pantalla += 1;
            // no dejar crecer el buffer de reorden
            if self.buf.len() > 8 { self.buf.drain(..self.buf.len() - 8); }
            Some(fr)
        }
    }
}

// ══════════════════════════════ Windows: Media Foundation (SourceReader) ═══

#[cfg(target_os = "windows")]
mod win {
    use super::*;
    use windows::core::{Interface, GUID, PROPVARIANT};
    use windows::Win32::Graphics::Direct3D::*;
    use windows::Win32::Graphics::Direct3D11::*;
    use windows::Win32::Media::MediaFoundation::*;

    /// decode por hardware (VCN) vía SourceReader con nuestro dispositivo
    /// D3D11; la lectura a CPU la hace MF al bloquear el buffer (una copia,
    /// el contrato universal). Seek: SetCurrentPosition → keyframe previo →
    /// descartar hasta el objetivo (mismo esquema que VideoToolbox).
    pub struct Cine {
        reader: IMFSourceReader,
        w: u32, h: u32, fps: f64, dur: f64,
        diez_bits: bool,
        /// decima a media resolución en la copia (la preview no necesita 4K)
        pub mitad: bool,
        /// pts del último frame servido (para decidir si hace falta seek)
        ultimo_pts: f64,
        leido: bool,
        /// último fotograma servido por un seek: pausa→play no repaga el GOP
        cacheado: Option<Fotograma>,
        // se retienen para que vivan lo que viva el reader
        _dev: ID3D11Device,
        _mgr: IMFDXGIDeviceManager,
    }
    unsafe impl Send for Cine {}

    fn arranca_mf() {
        static UNA: std::sync::Once = std::sync::Once::new();
        UNA.call_once(|| unsafe {
            let _ = MFStartup(MF_VERSION, MFSTARTUP_FULL);
        });
    }

    impl Cine {
        pub fn abre(ruta: &Path) -> Result<Self> {
            // el Source Resolver usa COM: los HILOS de trabajo (cabina,
            // miniaturas) no lo traen inicializado — sin esto, abre/lee
            // fallan EN SILENCIO fuera del hilo principal
            unsafe {
                use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            arranca_mf();
            // dispositivo D3D11 con vídeo + protección multihilo (lo exige MF)
            let (dev, _ctx) = unsafe {
                let mut d: Option<ID3D11Device> = None;
                let mut c: Option<ID3D11DeviceContext> = None;
                D3D11CreateDevice(
                    None, D3D_DRIVER_TYPE_HARDWARE, None,
                    D3D11_CREATE_DEVICE_VIDEO_SUPPORT | D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    None, D3D11_SDK_VERSION, Some(&mut d), None, Some(&mut c))?;
                (d.unwrap(), c.unwrap())
            };
            let mt: ID3D11Multithread = dev.cast()?;
            unsafe { mt.SetMultithreadProtected(true) };
            let mut token = 0u32;
            let mut mgr: Option<IMFDXGIDeviceManager> = None;
            unsafe { MFCreateDXGIDeviceManager(&mut token, &mut mgr)? };
            let mgr = mgr.unwrap();
            unsafe { mgr.ResetDevice(&dev, token)? };

            let mut attrs: Option<IMFAttributes> = None;
            unsafe { MFCreateAttributes(&mut attrs, 3)? };
            let attrs = attrs.unwrap();
            unsafe {
                attrs.SetUnknown(&MF_SOURCE_READER_D3D_MANAGER, &mgr)?;
                attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
            }
            let ancho: Vec<u16> = ruta.to_string_lossy().encode_utf16()
                .chain(std::iter::once(0)).collect();
            let reader = unsafe {
                MFCreateSourceReaderFromURL(windows::core::PCWSTR(ancho.as_ptr()), &attrs)?
            };
            unsafe {
                reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
                reader.SetStreamSelection(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, true)?;
            }
            // P010 (10-bit) si el stream lo da; NV12 como salida universal
            let mut diez_bits = false;
            unsafe {
                let ty = MFCreateMediaType()?;
                ty.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
                ty.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_P010)?;
                if reader.SetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, None, &ty).is_ok() {
                    diez_bits = true;
                } else {
                    let ty = MFCreateMediaType()?;
                    ty.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
                    ty.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
                    reader.SetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, None, &ty)?;
                }
            }
            let cur = unsafe { reader.GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)? };
            let tam = unsafe { cur.GetUINT64(&MF_MT_FRAME_SIZE)? };
            let (w, h) = ((tam >> 32) as u32, (tam & 0xffff_ffff) as u32);
            let rate = unsafe { cur.GetUINT64(&MF_MT_FRAME_RATE).unwrap_or((30 << 32) | 1) };
            let fps = (rate >> 32) as f64 / ((rate & 0xffff_ffff) as f64).max(1.0);
            let dur = unsafe {
                reader.GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
                    .ok()
                    .and_then(|pv| u64::try_from(&pv).ok())
                    .map(|d| d as f64 / 1e7)
                    .unwrap_or(0.0)
            };
            anyhow::ensure!(w > 0 && h > 0, "sin dimensiones de vídeo");
            Ok(Cine { reader, w, h, fps, dur, diez_bits, mitad: false,
                      ultimo_pts: -1.0, leido: false, cacheado: None,
                      _dev: dev, _mgr: mgr })
        }

        pub fn info(&self) -> (u32, u32, f64, f64) { (self.w, self.h, self.fps, self.dur) }

        fn paso(&self) -> usize {
            if self.mitad && self.w > 2200 { 2 } else { 1 }
        }

        fn lee(&mut self) -> Option<(f64, IMFSample)> {
            loop {
                let mut flags = 0u32;
                let mut pts = 0i64;
                let mut sample: Option<IMFSample> = None;
                unsafe {
                    self.reader.ReadSample(
                        MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, 0,
                        None, Some(&mut flags), Some(&mut pts), Some(&mut sample),
                    ).ok()?;
                }
                if flags & (MF_SOURCE_READERF_ENDOFSTREAM.0 as u32) != 0 { return None; }
                if let Some(s) = sample {
                    return Some((pts as f64 / 1e7, s));
                }
            }
        }

        fn convierte(&self, pts: f64, sample: &IMFSample) -> Option<Fotograma> {
            unsafe {
                let buf = sample.ConvertToContiguousBuffer().ok()?;
                // 2D si se puede (pitch real); si no, contiguo con stride = w
                let (base, pitch, total): (*const u8, usize, usize) =
                    if let Ok(b2) = buf.cast::<IMF2DBuffer2>() {
                        let mut scan0 = std::ptr::null_mut();
                        let mut pitch = 0i32;
                        let mut start = std::ptr::null_mut();
                        let mut len = 0u32;
                        b2.Lock2DSize(MF2DBuffer_LockFlags_Read, &mut scan0, &mut pitch,
                                      &mut start, &mut len).ok()?;
                        if pitch < 0 { b2.Unlock2D().ok(); return None; }
                        (scan0 as *const u8, pitch as usize, len as usize)
                    } else {
                        let mut p = std::ptr::null_mut();
                        let mut len = 0u32;
                        buf.Lock(&mut p, None, Some(&mut len)).ok()?;
                        let bpp = if self.diez_bits { 2 } else { 1 };
                        (p as *const u8, self.w as usize * bpp, len as usize)
                    };
                let (w, h) = (self.w as usize, self.h as usize);
                let (cw, ch) = (w / 2, h / 2);
                // el plano UV empieza tras la altura EMPADRONADA del buffer
                let h_pad = (total * 2 / (3 * pitch)).max(h);
                let p_y = base;
                let p_uv = base.add(pitch * h_pad);
                // decimación 2× durante la copia si la preview no pide 4K
                let paso = self.paso();
                let (ow, oh) = ((w / paso) & !1, (h / paso) & !1);
                let (ocw, och) = (ow / 2, oh / 2);
                let mut y = vec![0u16; ow * oh];
                let mut u = vec![0u16; ocw * och];
                let mut v = vec![0u16; ocw * och];
                if self.diez_bits {
                    for fila in 0..oh {
                        let src = std::slice::from_raw_parts(p_y.add((fila * paso).min(h - 1) * pitch) as *const u16, w);
                        let dst = &mut y[fila * ow..(fila + 1) * ow];
                        for c in 0..ow { dst[c] = src[c * paso] >> 6; }
                    }
                    for fila in 0..och {
                        let src = std::slice::from_raw_parts(p_uv.add((fila * paso).min(ch - 1) * pitch) as *const u16, cw * 2);
                        let (du, dv) = (&mut u[fila * ocw..], &mut v[fila * ocw..]);
                        for c in 0..ocw {
                            let s = (c * paso).min(cw - 1);
                            du[c] = src[s * 2] >> 6;
                            dv[c] = src[s * 2 + 1] >> 6;
                        }
                    }
                } else {
                    for fila in 0..oh {
                        let src = std::slice::from_raw_parts(p_y.add((fila * paso).min(h - 1) * pitch), w);
                        let dst = &mut y[fila * ow..(fila + 1) * ow];
                        for c in 0..ow { dst[c] = (src[c * paso] as u16) << 2; }
                    }
                    for fila in 0..och {
                        let src = std::slice::from_raw_parts(p_uv.add((fila * paso).min(ch - 1) * pitch), cw * 2);
                        let (du, dv) = (&mut u[fila * ocw..], &mut v[fila * ocw..]);
                        for c in 0..ocw {
                            let s = (c * paso).min(cw - 1);
                            du[c] = (src[s * 2] as u16) << 2;
                            dv[c] = (src[s * 2 + 1] as u16) << 2;
                        }
                    }
                }
                if let Ok(b2) = buf.cast::<IMF2DBuffer2>() { let _ = b2.Unlock2D(); }
                else { let _ = buf.Unlock(); }
                Some(Fotograma { y, u, v, w: ow as u32, h: oh as u32, pts })
            }
        }

        pub fn frame_en(&mut self, t: f64) -> Option<Fotograma> {
            let t = t.max(0.0);
            let medio = 0.5 / self.fps.max(1.0);
            // ¿el punto recién servido y el reader sigue ahí? de la caché
            if let Some(fr) = &self.cacheado {
                if self.leido && (fr.pts - self.ultimo_pts).abs() < 1e-9
                    && fr.pts + medio >= t && fr.pts <= t + medio {
                    return Some(fr.clone());
                }
            }
            if !self.leido || t < self.ultimo_pts - 0.001 || t > self.ultimo_pts + 1.5 {
                let pos = PROPVARIANT::from((t * 1e7) as i64);
                unsafe { self.reader.SetCurrentPosition(&GUID::zeroed(), &pos).ok()? };
                self.leido = true;
            }
            let mut ultima: Option<(f64, IMFSample)> = None;
            loop {
                match self.lee() {
                    Some((pts, s)) => {
                        let listo = pts + medio >= t;
                        ultima = Some((pts, s));
                        if listo { break; }
                    }
                    None => break,
                }
            }
            let (pts, s) = ultima?;
            self.ultimo_pts = pts;
            let fr = self.convierte(pts, &s);
            self.cacheado = fr.clone();
            fr
        }

        pub fn arranca_en(&mut self, t: f64) -> Option<Fotograma> { self.frame_en(t) }

        pub fn siguiente(&mut self) -> Option<Fotograma> {
            let (pts, s) = self.lee()?;
            self.ultimo_pts = pts;
            self.convierte(pts, &s)
        }

        /// SOLO el fotograma clave anterior a t (el primero tras el seek):
        /// un decode, cero catch-up — el scrub sin proxy
        pub fn frame_clave(&mut self, t: f64) -> Option<Fotograma> {
            let pos = PROPVARIANT::from((t.max(0.0) * 1e7) as i64);
            unsafe { self.reader.SetCurrentPosition(&GUID::zeroed(), &pos).ok()? };
            self.leido = true;
            let (pts, s) = self.lee()?;
            self.ultimo_pts = pts;
            let fr = self.convierte(pts, &s);
            self.cacheado = fr.clone();
            fr
        }

        /// el fotograma del SCRUB: exacto si venimos leyendo hacia delante
        /// (catch-up corto), el keyframe si el salto es grande
        pub fn frame_scrub(&mut self, t: f64) -> Option<Fotograma> {
            if self.leido && t >= self.ultimo_pts - 0.001 && t - self.ultimo_pts < 0.25 {
                self.frame_en(t)
            } else {
                self.frame_clave(t)
            }
        }
    }
}

// ══════════════════════════════════ resto de plataformas: camino ffmpeg ═══

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod generico {
    use super::*;
    use crate::video::FfmpegDecoder;

    /// mientras el backend Media Foundation no esté enchufado aquí, el camino
    /// interactivo en Windows/Linux usa ffmpeg (lento pero universal)
    pub struct Cine {
        ruta: std::path::PathBuf,
        dec: Option<FfmpegDecoder>,
        pub mitad: bool,
        w: u32, h: u32, fps: f64, dur: f64,
        t: f64,
    }

    impl Cine {
        pub fn abre(ruta: &Path) -> Result<Self> {
            let (w, h, fps, dur) = crate::indice::sondea(ruta)
                .or_else(|_| crate::video::probe(ruta.to_str().unwrap_or("")))?;
            Ok(Cine { ruta: ruta.to_path_buf(), dec: None, mitad: false, w, h, fps, dur, t: 0.0 })
        }

        pub fn frame_clave(&mut self, t: f64) -> Option<Fotograma> { self.frame_en(t) }
        pub fn frame_scrub(&mut self, t: f64) -> Option<Fotograma> { self.frame_en(t) }

        pub fn info(&self) -> (u32, u32, f64, f64) { (self.w, self.h, self.fps, self.dur) }

        pub fn frame_en(&mut self, t: f64) -> Option<Fotograma> {
            let d = FfmpegDecoder::open_at(self.ruta.to_str()?, t, self.w, self.h).ok()?;
            self.dec = Some(d);
            self.t = t;
            self.siguiente()
        }

        pub fn arranca_en(&mut self, t: f64) -> Option<Fotograma> { self.frame_en(t) }

        pub fn siguiente(&mut self) -> Option<Fotograma> {
            let d = self.dec.as_mut()?;
            let (y, u, v) = d.next_frame()?;
            let fr = Fotograma {
                y: y.to_vec(), u: u.to_vec(), v: v.to_vec(),
                w: self.w, h: self.h, pts: self.t,
            };
            self.t += 1.0 / self.fps.max(1.0);
            Some(fr)
        }
    }
}
