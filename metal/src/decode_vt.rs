//! Decode zero-copy: VTDecompressionSession → CVPixelBuffer (P010) →
//! CVMetalTextureCache → MTLTexture Y/CbCr. Sin ninguna copia a CPU.

use crate::vt_ffi::*;
use metal::{Device, MTLPixelFormat, Texture};
use metal::foreign_types::ForeignType;
use objc::{msg_send, sel, sel_impl};
use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::Mutex;

pub struct DecodedFrame {
    pub pixel_buffer: CVPixelBufferRef,
    pub pts: CMTime,
}

struct SendPb(CVPixelBufferRef, CMTime);
unsafe impl Send for SendPb {}
static OUT_QUEUE: Mutex<Option<VecDeque<SendPb>>> = Mutex::new(None);

unsafe extern "C" fn on_frame(
    _refcon: *mut c_void,
    _source: *mut c_void,
    status: OSStatus,
    _flags: u32,
    image: CVPixelBufferRef,
    pts: CMTime,
    _dur: CMTime,
) {
    if status == 0 && !image.is_null() {
        core_foundation::base::CFRetain(image as *const c_void);
        let mut q = OUT_QUEUE.lock().unwrap();
        if std::env::var("FL_ORDEN").is_ok() {
            eprintln!("   ← callback pts={} ts={}", pts.value, pts.timescale);
        }
        q.as_mut().unwrap().push_back(SendPb(image, pts));
    }
}

/// el incremento del sello de tiempo entre fotogramas consecutivos
pub const PASO_PTS: i64 = 1000;
/// cuántos fotogramas se acumulan antes de dar por perdido el que falta
const VENTANA: usize = 12;

pub struct VtDecoder {
    session: VTDecompressionSessionRef,
    cache: CVMetalTextureCacheRef,
    fmt: CMVideoFormatDescriptionRef,
    /// el sello del fotograma que toca entregar (el búfer de reordenación)
    siguiente: std::cell::Cell<i64>,
}

impl VtDecoder {
    /// Crea la sesión a partir de los parameter sets HEVC (VPS/SPS/PPS).
    pub fn new(device: &Device, vps: &[u8], sps: &[u8], pps: &[u8]) -> anyhow::Result<Self> {
        let dev_ptr: *mut c_void = unsafe { objc::msg_send![&**device, self] };
        let ptrs: Vec<*const u8> = [vps.as_ptr(), sps.as_ptr(), pps.as_ptr()].to_vec();
        let sizes: Vec<usize> = [vps.len(), sps.len(), pps.len()].to_vec();
        let mut fmt: CMVideoFormatDescriptionRef = std::ptr::null_mut();
        let st = unsafe {
            CMVideoFormatDescriptionCreateFromHEVCParameterSets(
                std::ptr::null(), 3, ptrs.as_ptr(), sizes.as_ptr(), 4,
                std::ptr::null(), &mut fmt,
            )
        };
        anyhow::ensure!(st == 0, "format description falló: {st}");

        let mut cache: CVMetalTextureCacheRef = std::ptr::null_mut();
        let st = unsafe {
            CVMetalTextureCacheCreate(std::ptr::null(), std::ptr::null(),
                                      dev_ptr, std::ptr::null(), &mut cache)
        };
        anyhow::ensure!(st == 0, "texture cache falló: {st}");

        let cb = VTDecompressionOutputCallbackRecord {
            decompression_output_callback: on_frame,
            decompression_output_ref_con: std::ptr::null_mut(),
        };
        *OUT_QUEUE.lock().unwrap() = Some(VecDeque::new());
        // pide EXPLÍCITAMENTE x420 (10-bit biplanar MSB-aligned): sin esto VT emite
        // 'p420' empaquetado (10 bits/px, bpr 5120) y la importación R16/RG16 lee basura
        let pf = KCVPIXELFORMATTYPE_420YPCBCR10BIPLANARVIDEORANGE.to_le_bytes();
        let pf_num = unsafe { CFNumberCreate(std::ptr::null(), 3, pf.as_ptr() as *const c_void) };
        let dest_attrs = cfdict(&[
            (unsafe { kCVPixelBufferPixelFormatTypeKey }, pf_num as *const c_void),
            (unsafe { kCVPixelBufferIOSurfacePropertiesKey }, cfdict(&[]) as *const c_void),
            (unsafe { kCVPixelBufferMetalCompatibilityKey }, unsafe { kCFBooleanTrue as *const c_void }),
        ]);
        let mut session: VTDecompressionSessionRef = std::ptr::null_mut();
        let st = unsafe {
            VTDecompressionSessionCreate(
                std::ptr::null(), fmt, std::ptr::null(), dest_attrs as *const c_void, &cb, &mut session,
            )
        };
        anyhow::ensure!(st == 0, "decompression session falló: {st}");
        Ok(VtDecoder { session, cache, fmt, siguiente: std::cell::Cell::new(0) })
    }

    /// Alimenta un NAL unit (AVCC: 4 bytes longitud big-endian + payload).
    pub fn decode_nal(&self, data: &mut [u8], pts: CMTime) {
        let mut bb: CMBlockBufferRef = std::ptr::null_mut();
        let st = unsafe {
            CMBlockBufferCreateWithMemoryBlock(
                std::ptr::null(),
                data.as_mut_ptr() as *mut c_void,
                data.len(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                data.len(),
                0,
                &mut bb,
            )
        };
        if st != 0 { return; }
        let size = data.len();
        let mut sb: CMSampleBufferRef = std::ptr::null_mut();
        // EL SELLO VIAJA CON LA MUESTRA. Antes se pasaban 0 entradas de tiempo
        // y el `pts` de este método se quedaba sin usar: el callback recibía
        // un tiempo inventado y no había forma de reordenar la salida.
        let timing = CMSampleTimingInfo {
            duration: CMTime { value: PASO_PTS, timescale: pts.timescale, flags: 1, epoch: 0 },
            presentation_ts: pts,
            decode_ts: CMTIME_INVALIDO,
        };
        let st = unsafe {
            CMSampleBufferCreateReady(
                std::ptr::null(), bb, self.fmt, 1, 1,
                &timing as *const CMSampleTimingInfo as *const c_void, 1, &size, &mut sb,
            )
        };
        if st == 0 {
            // 1 = kVTDecodeFrame_EnableAsynchronousDecompression: sin esto el decode
            // es SÍNCRONO y bloquea el hilo ~3.6 ms/frame
            unsafe { VTDecompressionSessionDecodeFrame(self.session, sb, 1, std::ptr::null_mut(), std::ptr::null_mut()) };
            unsafe { CFRelease(sb as *mut c_void) };
        }
        unsafe { CFRelease(bb as *mut c_void) };
    }

    /// EL BÚFER DE REORDENACIÓN. Con
    /// `kVTDecodeFrame_EnableAsynchronousDecompression` VideoToolbox llama
    /// al callback **en cuanto termina cada fotograma**, que NO es el orden
    /// en que se los dimos: la cola de salida salía barajada y el máster
    /// llevaba los fotogramas cambiados de sitio a partir del octavo. Peor:
    /// barajados de forma distinta en cada ejecución (medido con dos
    /// revelados idénticos: PSNR 28 dB entre ellos, cuando tenía que ser
    /// infinito).
    ///
    /// Aquí solo sale el fotograma que toca. `paso` es el incremento del
    /// sello que pone `decode_nal` (`submitted * 1000`).
    pub fn pop(&self) -> Option<DecodedFrame> {
        let mut g = OUT_QUEUE.lock().unwrap();
        let q = g.as_mut().unwrap();
        if q.is_empty() { return None; }
        let esperado = self.siguiente.get();
        // ¿está el que toca?
        if let Some(k) = q.iter().position(|s| s.1.value == esperado) {
            let s = q.remove(k).unwrap();
            self.siguiente.set(esperado + PASO_PTS);
            return Some(DecodedFrame { pixel_buffer: s.0, pts: s.1 });
        }
        // no está: o aún no ha salido del decodificador, o se perdió. Si hay
        // material de sobra esperando, el que faltaba no va a venir (fotograma
        // corrupto): se avanza al más antiguo que haya y se sigue.
        if q.len() < VENTANA { return None; }
        let k = (0..q.len()).min_by_key(|&i| q[i].1.value).unwrap();
        let s = q.remove(k).unwrap();
        eprintln!("   ⚠ falta el fotograma pts={esperado}: sigo por pts={}", s.1.value);
        self.siguiente.set(s.1.value + PASO_PTS);
        Some(DecodedFrame { pixel_buffer: s.0, pts: s.1 })
    }

    pub fn flush(&self) {
        unsafe { VTDecompressionSessionWaitForAsynchronousFrames(self.session) };
    }

    /// Importa los planos Y (R16Unorm) y UV (RG16Unorm) como MTLTextures.
    pub fn import_planes(&self, pb: CVPixelBufferRef) -> Option<(Texture, Texture, usize, usize)> {
        let w = unsafe { CVPixelBufferGetWidth(pb) };
        let h = unsafe { CVPixelBufferGetHeight(pb) };
        let mut make = |plane: usize, fmt: MTLPixelFormat, pw: usize, ph: usize| -> Option<Texture> {
            let mut ct: CVMetalTextureRef = std::ptr::null_mut();
            let st = unsafe {
                CVMetalTextureCacheCreateTextureFromImage(
                    std::ptr::null(), self.cache, pb, std::ptr::null(),
                    fmt as u64, pw, ph, plane, &mut ct,
                )
            };
            if st != 0 || ct.is_null() { return None; }
            let raw = unsafe { CVMetalTextureGetTexture(ct) };
            if raw.is_null() { unsafe { CFRelease(ct as *mut c_void) }; return None; }
            unsafe { core_foundation::base::CFRetain(raw as *const c_void) };
            let out = unsafe { Texture::from_ptr(raw as *mut _) };
            unsafe { CFRelease(ct as *mut c_void) };
            Some(out)
        };
        let y = make(0, MTLPixelFormat::R16Unorm, w, h)?;
        let uv = make(1, MTLPixelFormat::RG16Unorm, w / 2, h / 2)?;
        Some((y, uv, w, h))
    }
}

impl Drop for VtDecoder {
    fn drop(&mut self) {
        unsafe {
            VTDecompressionSessionInvalidate(self.session);
            CFRelease(self.session as *mut c_void);
            CFRelease(self.cache as *mut c_void);
            CFRelease(self.fmt as *mut c_void);
        }
    }
}

/// Parseo de un stream Annex-B (ffmpeg -bsf hevc_mp4toannexb): separa NALs,
/// extrae VPS/SPS/PPS y convierte a AVCC por demanda.
pub struct AnnexB {
  
    pub vps: Vec<u8>,
    pub sps: Vec<u8>,
    pub pps: Vec<u8>,
    pub pending: Vec<Vec<u8>>,
}

pub fn parse_annexb(mut data: Vec<u8>) -> AnnexB {
    // start codes respetando emulation prevention: 00 00 03 xx es DATO, no borde
    let mut starts = Vec::new();      // (inicio payload, len prefijo)
    let mut i = 0usize;
    while i + 3 < data.len() {
        if data[i] == 0 && data[i + 1] == 0 {
            if data[i + 2] == 3 { i += 3; continue; }              // EPB
            if data[i + 2] == 1 { starts.push((i, 3)); i += 3; continue; }
            if data[i + 2] == 0 && data[i + 3] == 1 { starts.push((i, 4)); i += 4; continue; }
        }
        i += 1;
    }
    let mut a = AnnexB { vps: vec![], sps: vec![], pps: vec![], pending: vec![] };
    // UNIDADES DE ACCESO, no NALs sueltas. Un fotograma puede venir en varias
    // slices, y entre medias hay SEI y delimitadores que NO son imagen. Si
    // cada NAL se manda como si fuese un fotograma, el sello de tiempo deja
    // de contar fotogramas y el búfer de reordenación se queda esperando
    // sellos que no existen. Se corta por la primera slice de cada imagen
    // (`first_slice_segment_in_pic_flag`, el primer bit de la cabecera de
    // slice) y todo lo que va delante viaja pegado a ella.
    let mut au: Vec<u8> = Vec::new();          // la unidad de acceso en curso
    let mut tiene_imagen = false;
    for (k, &(sc, prefix)) in starts.iter().enumerate() {
        let s = sc + prefix;
        let mut e = if k + 1 < starts.len() { starts[k + 1].0 } else { data.len() };
        while e > s && data[e - 1] == 0 { e -= 1; }                // trailing zeros
        let nal = &data[s..e];
        if nal.len() < 3 { continue; }
        let nal_type = (nal[0] >> 1) & 0x3f;
        match nal_type {
            32 => { a.vps = nal.to_vec(); continue; }
            33 => { a.sps = nal.to_vec(); continue; }
            34 => { a.pps = nal.to_vec(); continue; }
            _ => {}
        }
        let vcl = nal_type <= 31;
        let primera = vcl && (nal[2] >> 7) & 1 == 1;
        if primera && tiene_imagen {
            a.pending.push(std::mem::take(&mut au));
            tiene_imagen = false;
        }
        au.extend_from_slice(&(nal.len() as u32).to_be_bytes());
        au.extend_from_slice(nal);
        tiene_imagen |= vcl;
    }
    if tiene_imagen { a.pending.push(au); }
    a
}
