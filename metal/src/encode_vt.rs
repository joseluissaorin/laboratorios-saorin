//! Encode zero-copy: render → CVPixelBuffer x420 (IOSurface) → N sesiones VT
//! en paralelo por segmentos-GOP (el M4 Max tiene 2 motores de encode; una
//! sesión satura a ~250 fps) → bitstream HEVC reordenado → mux por pipe ffmpeg.
//! Sin readbacks.

use crate::vt_ffi::*;
use objc::runtime::{Object, BOOL, NO};
use objc::{class, msg_send, sel, sel_impl};
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::io::Write;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {
    static AVMediaTypeVideo: *const c_void;
    static AVMediaTypeAudio: *const c_void;
    static AVFileTypeQuickTimeMovie: *const c_void;
}
extern "C" {
    fn objc_msgSend();
}

/// mux ProRes: AVAssetWriter en passthrough (append de CMSampleBuffers ya
/// comprimidos, sin re-encode) + reordenación por PTS (2 sesiones → llegada
/// desordenada). El audio se re-muxea con ffmpeg al final.
struct ProresMux {
    writer: *mut Object,
    input: *mut Object,
    next: i64,
    pending: BTreeMap<i64, CMSampleBufferRef>,
    out_path: String,
    audio_src: String,
    /// duración máxima del audio (s) — con --max-frames el vídeo es más corto que el origen
    audio_limit_s: f64,
}
unsafe impl Send for ProresMux {}
static PRMUX: Mutex<Option<ProresMux>> = Mutex::new(None);
static AUDIO_PUMP: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

/// prepara el track de audio passthrough (hay que añadir el input ANTES de startWriting)
unsafe fn prepare_audio(writer: *mut Object, audio_src: &str) -> Option<(usize, usize, usize)> {
    unsafe {
        let url: *mut Object = msg_send![class!(NSURL), fileURLWithPath: cfstr(audio_src)];
        let asset: *mut Object = msg_send![class!(AVURLAsset), URLAssetWithURL: url options: std::ptr::null_mut::<Object>()];
        let tracks: *mut Object = msg_send![asset, tracksWithMediaType: AVMediaTypeAudio];
        let count: usize = msg_send![tracks, count];
        if count == 0 { return None; }
        let track: *mut Object = msg_send![tracks, objectAtIndex: 0usize];
        let fmts: *mut Object = msg_send![track, formatDescriptions];
        let fmt: *mut Object = msg_send![fmts, objectAtIndex: 0usize];
        let mut err: *mut Object = std::ptr::null_mut();
        let reader: *mut Object = msg_send![class!(AVAssetReader), alloc];
        let reader: *mut Object = msg_send![reader, initWithAsset: asset error: &mut err];
        if reader.is_null() { return None; }
        let out: *mut Object = msg_send![class!(AVAssetReaderTrackOutput), alloc];
        let out: *mut Object = msg_send![out, initWithTrack: track outputSettings: std::ptr::null_mut::<Object>()];
        let _: () = msg_send![reader, addOutput: out];
        let ainput: *mut Object = msg_send![class!(AVAssetWriterInput), alloc];
        let ainput: *mut Object = msg_send![ainput, initWithMediaType: AVMediaTypeAudio
                                                    outputSettings: std::ptr::null_mut::<Object>()
                                                    sourceFormatHint: fmt];
        let _: () = msg_send![ainput, setExpectsMediaDataInRealTime: NO];
        let _: () = msg_send![writer, addInput: ainput];
        Some((reader as usize, out as usize, ainput as usize))
    }
}

/// bombea el audio comprimido (passthrough) al writer en su propio hilo — sin remux final
fn start_audio_pump(parts: (usize, usize, usize), limit_s: f64) {
    let (reader_u, out_u, ainput_u) = parts;
    let h = std::thread::spawn(move || unsafe {
        let (reader, out, ainput) = (reader_u as *mut Object, out_u as *mut Object, ainput_u as *mut Object);
        let ok: BOOL = msg_send![reader, startReading];
        if ok == NO { return; }
        loop {
            let s: CMSampleBufferRef = msg_send![out, copyNextSampleBuffer];
            if s.is_null() { break; }
            let pts = CMSampleBufferGetPresentationTimeStamp(s);
            if limit_s > 0.0 && pts.timescale > 0
                && pts.value as f64 / pts.timescale as f64 > limit_s {
                CFRelease(s as *mut c_void);
                break;
            }
            loop {
                let ready: BOOL = msg_send![ainput, isReadyForMoreMediaData];
                if ready != NO { break; }
                std::thread::sleep(std::time::Duration::from_micros(500));
            }
            let ok: BOOL = msg_send![ainput, appendSampleBuffer: s];
            CFRelease(s as *mut c_void);
            if ok == NO { break; }
        }
        let _: () = msg_send![ainput, markAsFinished];
        let _: () = msg_send![reader, cancelReading];
    });
    *AUDIO_PUMP.lock().unwrap() = Some(h);
}

unsafe fn prores_on_packet(sample: CMSampleBufferRef) {
    let pts = unsafe { CMSampleBufferGetPresentationTimeStamp(sample) };
    unsafe { core_foundation::base::CFRetain(sample as *const c_void) };
    let mut g = PRMUX.lock().unwrap();
    let m = g.as_mut().unwrap();
    if m.writer.is_null() {
        // perezoso: el writer necesita el format description del primer sample
        let fmt = unsafe { CMSampleBufferGetFormatDescription(sample) };
        unsafe {
            let _ = std::fs::remove_file(&m.out_path);
            let path = cfstr(&m.out_path);
            let url: *mut Object = msg_send![class!(NSURL), fileURLWithPath: path];
            let mut err: *mut Object = std::ptr::null_mut();
            let w: *mut Object = msg_send![class!(AVAssetWriter), alloc];
            let w: *mut Object = msg_send![w, initWithURL: url fileType: AVFileTypeQuickTimeMovie error: &mut err];
            assert!(!w.is_null(), "AVAssetWriter init falló");
            let inp: *mut Object = msg_send![class!(AVAssetWriterInput), alloc];
            let inp: *mut Object = msg_send![inp, initWithMediaType: AVMediaTypeVideo
                                                 outputSettings: std::ptr::null_mut::<Object>()
                                                 sourceFormatHint: fmt];
            let _: () = msg_send![inp, setExpectsMediaDataInRealTime: NO];
            let _: () = msg_send![w, addInput: inp];
            let audio = prepare_audio(w, &m.audio_src);   // antes de startWriting
            let ok: BOOL = msg_send![w, startWriting];
            if ok == NO { eprintln!("   AVAssetWriter startWriting falló"); }
            // startSessionAtSourceTime: recibe CMTime por valor → objc_msgSend crudo
            let f: extern "C" fn(*mut Object, objc::runtime::Sel, CMTime) =
                std::mem::transmute(objc_msgSend as *const c_void);
            f(w, sel!(startSessionAtSourceTime:), CMTime { value: 0, timescale: pts.timescale, flags: 1, epoch: 0 });
            if let Some(parts) = audio { start_audio_pump(parts, m.audio_limit_s); }
            m.writer = w;
            m.input = inp;
        }
    }
    // pts.value = idx*1000 (timescale racional ×1000) → la clave del reorden
    // vuelve a ser el índice de frame
    m.pending.insert(pts.value / 1000, sample);
    while let Some(s) = m.pending.remove(&m.next) {
        unsafe {
            loop {
                let ready: BOOL = msg_send![m.input, isReadyForMoreMediaData];
                if ready != NO { break; }
                std::thread::sleep(std::time::Duration::from_micros(200));
            }
            let ok: BOOL = msg_send![m.input, appendSampleBuffer: s];
            if ok == NO { eprintln!("   appendSampleBuffer falló en frame {}", m.next); }
            CFRelease(s as *mut c_void);
        }
        m.next += 1;
    }
}

/// frames por segmento = GOP cerrado (IDR forzado al inicio). 60 @ 60fps = 1s.
const SEG_LEN: i64 = 60;

/// ProRes es all-intra → round-robin por frame entre los 2 motores ProRes del
/// M4 Max (verificado: 2 procesos paralelos no se estorban). HEVC solo tiene
/// un motor utilizable → 1 sesión.
/// CUÁNTOS MOTORES DE VÍDEO SE USAN A LA VEZ.
///
/// ProRes: dos, porque el chip tiene dos motores ProRes de verdad y cada
/// fotograma es independiente. HEVC: **uno**, y esto está medido, no supuesto.
/// El troceado por segmentos ya existía y parecía que HEVC podría repartirse
/// igual; probado sobre la misma bobina de 4K:
///
///   1 motor → 153 fps (esperando al codificador 3,6 ms/fotograma)
///   2       → 110 fps (6,4 ms)
///   3       → 106 fps (6,9 ms)
///
/// Las sesiones HEVC se pelean por el mismo bloque del chip y encima obligan
/// al mux a guardar segmentos desordenados. Más motores, más lento.
fn n_sessions() -> usize {
    let d = if prores_codec().is_some() { 2 } else { 1 };
    std::env::var("FL_MOTORES").ok().and_then(|v| v.parse().ok()).unwrap_or(d)
}

struct Segment {
    data: Vec<u8>,
    frames: i64,
}

struct MuxState {
    sink: Option<ChildStdin>,
    segments: BTreeMap<i64, Segment>,
    next: i64,
    /// nº total de frames (fijado en finish) para saber cuándo está completo el último segmento
    total: Option<i64>,
}

static MSTATE: Mutex<Option<MuxState>> = Mutex::new(None);
static MUX_CHILD: Mutex<Option<Child>> = Mutex::new(None);

fn expected_frames(seg: i64, total: Option<i64>) -> i64 {
    match total {
        Some(t) => (t - seg * SEG_LEN).clamp(0, SEG_LEN),
        None => SEG_LEN,
    }
}

fn flush_ready(st: &mut MuxState) {
    loop {
        let done = match st.segments.get(&st.next) {
            Some(s) => s.frames >= expected_frames(st.next, st.total),
            None => st.total.map_or(false, |t| st.next * SEG_LEN >= t),
        };
        if !done { break; }
        if let Some(seg) = st.segments.remove(&st.next) {
            if let Some(sink) = st.sink.as_mut() {
                if let Err(e) = sink.write_all(&seg.data) { eprintln!("   mux write: {}", e); }
            }
        }
        st.next += 1;
        if st.total.map_or(false, |t| st.next * SEG_LEN >= t) && st.segments.is_empty() { break; }
    }
}

unsafe extern "C" fn on_packet(
    _refcon: *mut c_void,
    _source: *mut c_void,
    status: OSStatus,
    _flags: u32,
    sample: CMSampleBufferRef,
) {
    if status != 0 || sample.is_null() { return; }
    if prores_codec().is_some() { unsafe { prores_on_packet(sample) }; return; }
    let bb = unsafe { CMSampleBufferGetDataBuffer(sample) };
    if bb.is_null() { return; }
    let total = unsafe { CMBlockBufferGetDataLength(bb) };
    let mut ptr: *mut i8 = std::ptr::null_mut();
    let mut len_at = 0usize;
    let mut total2 = 0usize;
    let st = unsafe { CMBlockBufferGetDataPointer(bb, 0, &mut len_at, &mut total2, &mut ptr) };
    if st != 0 || ptr.is_null() { return; }
    let pts = unsafe { CMSampleBufferGetPresentationTimeStamp(sample) };
    let seg_id = pts.value / SEG_LEN;

    let data = unsafe { std::slice::from_raw_parts(ptr as *const u8, total.min(total2)) };
    // AVCC → Annex-B (ffmpeg raw hevc espera start codes)
    let mut out = Vec::with_capacity(data.len() + 256);
    // primer frame del segmento (IDR): antepone los parameter sets DE ESTA SESIÓN
    // (no los del origen) — segmentos autocontenidos, mux trivial
    if pts.value % SEG_LEN == 0 {
        let fmt = unsafe { CMSampleBufferGetFormatDescription(sample) };
        if !fmt.is_null() {
            for idx in 0..3usize {
                let mut p: *const u8 = std::ptr::null();
                let mut sz = 0usize;
                let st = unsafe {
                    CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(
                        fmt, idx, &mut p, &mut sz, std::ptr::null_mut(), std::ptr::null_mut())
                };
                if st == 0 && !p.is_null() && sz > 0 {
                    out.extend_from_slice(&[0, 0, 0, 1]);
                    out.extend_from_slice(unsafe { std::slice::from_raw_parts(p, sz) });
                }
            }
        }
    }
    let mut i = 0usize;
    while i + 4 <= data.len() {
        let n = u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
        if n == 0 || i + 4 + n > data.len() { break; }
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(&data[i + 4..i + 4 + n]);
        i += 4 + n;
    }

    let mut g = MSTATE.lock().unwrap();
    let st = g.as_mut().unwrap();
    let seg = st.segments.entry(seg_id).or_insert_with(|| Segment { data: Vec::new(), frames: 0 });
    seg.data.extend_from_slice(&out);
    seg.frames += 1;
    flush_ready(st);
}

pub struct VtEncoder {
    sessions: Vec<VTCompressionSessionRef>,
    pub pool: CVPixelBufferPoolRef,
    force_key: CFMutableDictionaryRef,
    out_path: String,
    audio_src: String,
}

// las sesiones VT y el pool son thread-safe; el encoder vive en su propio hilo
unsafe impl Send for VtEncoder {}

/// alloc desde el pool sin necesitar el VtEncoder (que vive en el hilo de encode)
pub fn alloc_from_pool(pool: CVPixelBufferPoolRef) -> Option<CVPixelBufferRef> {
    let mut pb: CVPixelBufferRef = std::ptr::null_mut();
    let st = unsafe { CVPixelBufferPoolCreatePixelBuffer(std::ptr::null(), pool, &mut pb) };
    if st == 0 && !pb.is_null() { Some(pb) } else { None }
}

fn prores_codec() -> Option<FourCharCode> {
    match std::env::var("FL_CODEC").as_deref() {
        Ok("prores4444") => Some(KVTVIDEOTYPE_PRORES4444),
        Ok("prores422hq") => Some(KVTVIDEOTYPE_PRORES422HQ),
        _ => None,
    }
}

fn make_session(w: u32, h: u32, bitrate: i64) -> anyhow::Result<VTCompressionSessionRef> {
    let spec = cfdict(&[(
        unsafe { kVTVideoEncoderSpecification_EnableHardwareAcceleratedVideoEncoder },
        unsafe { kCFBooleanTrue as *const c_void },
    )]);
    let codec = prores_codec().unwrap_or(KVTVIDEOTYPE_HEVC);
    let mut session: VTCompressionSessionRef = std::ptr::null_mut();
    let st = unsafe {
        VTCompressionSessionCreate(
            std::ptr::null(), w as i32, h as i32, codec,
            spec, std::ptr::null(), std::ptr::null(),
            on_packet, std::ptr::null_mut(), &mut session,
        )
    };
    anyhow::ensure!(st == 0, "compression session falló: {st}");
    unsafe {
        // offline: RealTime=false deja al encoder usar toda su cola/paralelismo
        VTSessionSetProperty(session, kVTCompressionPropertyKey_RealTime, kCFBooleanFalse as *const c_void);
        VTSessionSetProperty(session, kVTCompressionPropertyKey_MaximizePowerEfficiency, kCFBooleanFalse as *const c_void);
        let p709 = cfstr("ITU_R_709_2");
        VTSessionSetProperty(session, kVTCompressionPropertyKey_YCbCrMatrix, p709 as *const c_void);
        if prores_codec().is_none() {
            let br = bitrate.to_le_bytes();
            let brnum = CFNumberCreate(std::ptr::null(), 4, br.as_ptr() as *const c_void);
            VTSessionSetProperty(session, kVTCompressionPropertyKey_AverageBitRate, brnum as *const c_void);
            VTSessionSetProperty(session, cfstr("PrioritizeEncodingSpeedOverQuality") as CFStringRef, kCFBooleanTrue as *const c_void);
            VTSessionSetProperty(session, kVTCompressionPropertyKey_AllowFrameReordering, kCFBooleanFalse as *const c_void);
            VTSessionSetProperty(session, kVTCompressionPropertyKey_ProfileLevel, cfstr("HEVC_Main10_AutoLevel") as *const c_void);
        }
    }
    Ok(session)
}

impl VtEncoder {
    pub fn new(w: u32, h: u32, out_path: &str, audio_src: &str, fps: f64, bitrate: i64,
               audio_limit_s: f64) -> anyhow::Result<Self> {
        if prores_codec().is_some() {
            // ProRes → AVAssetWriter passthrough directo al .mov final (audio en vivo)
            let final_path = if out_path.to_lowercase().ends_with(".mov") {
                out_path.to_string()
            } else {
                let p = format!("{}.mov", out_path.trim_end_matches(".mp4"));
                eprintln!("   ProRes → contenedor .mov: {}", p);
                p
            };
            *PRMUX.lock().unwrap() = Some(ProresMux {
                writer: std::ptr::null_mut(),
                input: std::ptr::null_mut(),
                next: 0,
                pending: BTreeMap::new(),
                out_path: final_path,
                audio_src: audio_src.to_string(),
                audio_limit_s: audio_limit_s,
            });
        } else {
            // HEVC → mux: ffmpeg -f hevc -i - → mp4 (+ audio). Con --max-frames
            // el audio se limita con -t sobre SU input (-shortest no sirve aquí:
            // con el pipe HEVC crudo sin timestamps descarta el audio entero)
            let mut cmd = Command::new("ffmpeg");
            cmd.args(["-hide_banner", "-loglevel", "error", "-y",
                      "-f", "hevc", "-r", &format!("{:.3}", fps), "-i", "-"]);
            if audio_limit_s > 0.0 {
                cmd.args(["-t", &format!("{:.3}", audio_limit_s)]);
            }
            // SIN FUENTE DE SONIDO no hay que darle un `-i` vacío: la bobina
            // puede tener veinte clips y su mezcla la hornea el taller aparte,
            // así que el motor entrega vídeo mudo y el mux se hace fuera.
            if !audio_src.is_empty() {
                cmd.args(["-i", audio_src, "-map", "0:v", "-map", "1:a?",
                          "-c:a", "aac", "-b:a", "256k"]);
            } else {
                cmd.args(["-map", "0:v"]);
            }
            cmd.args(["-c:v", "copy",
                      // el VUI, DENTRO del bitstream: con `-c:v copy` las
                      // etiquetas del contenedor no bastan (en el Mac la curva
                      // y los primarios salían «unknown»; en Windows era peor,
                      // AMF los marcaba como HDR)
                      "-bsf:v", "hevc_metadata=colour_primaries=1\
:transfer_characteristics=1:matrix_coefficients=1:video_full_range_flag=0",
                      "-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709",
                      "-color_range", "tv", out_path]);
            let mut child = cmd
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()?;
            *MSTATE.lock().unwrap() = Some(MuxState {
                sink: Some(child.stdin.take().unwrap()),
                segments: BTreeMap::new(),
                next: 0,
                total: None,
            });
            *MUX_CHILD.lock().unwrap() = Some(child);
        }

        let sessions = (0..n_sessions()).map(|_| make_session(w, h, bitrate))
            .collect::<anyhow::Result<Vec<_>>>()?;

        // pool x420 compartido: 10-bit biplanar video-range (formato nativo del encoder)
        let pool_attrs = cfdict(&[]);
        let pixfmt = KCVPIXELFORMATTYPE_420YPCBCR10BIPLANARVIDEORANGE.to_le_bytes();
        let fmt = unsafe { CFNumberCreate(std::ptr::null(), 3, pixfmt.as_ptr() as *const c_void) };
        let wn = unsafe { CFNumberCreate(std::ptr::null(), 3, (w as i32).to_le_bytes().as_ptr() as *const c_void) };
        let hn = unsafe { CFNumberCreate(std::ptr::null(), 3, (h as i32).to_le_bytes().as_ptr() as *const c_void) };
        let pb_attrs = cfdict(&[
            (unsafe { kCVPixelBufferPixelFormatTypeKey }, fmt as *const c_void),
            (unsafe { kCVPixelBufferWidthKey }, wn as *const c_void),
            (unsafe { kCVPixelBufferHeightKey }, hn as *const c_void),
            (unsafe { kCVPixelBufferIOSurfacePropertiesKey }, cfdict(&[]) as *const c_void),
            (unsafe { kCVPixelBufferMetalCompatibilityKey }, unsafe { kCFBooleanTrue as *const c_void }),
            (unsafe { kCVImageBufferColorPrimariesKey }, cfstr("ITU_R_709_2") as *const c_void),
            (unsafe { kCVImageBufferYCbCrMatrixKey }, cfstr("ITU_R_709_2") as *const c_void),
            (unsafe { kCVImageBufferTransferFunctionKey }, cfstr("ITU_R_709_2") as *const c_void),
        ]);
        let mut pool: CVPixelBufferPoolRef = std::ptr::null_mut();
        let st = unsafe { CVPixelBufferPoolCreate(std::ptr::null(), pool_attrs, pb_attrs, &mut pool) };
        anyhow::ensure!(st == 0, "pool falló: {st}");
        let force_key = cfdict(&[(
            unsafe { kVTEncodeFrameOptionKey_ForceKeyFrame },
            unsafe { kCFBooleanTrue as *const c_void },
        )]);
        Ok(VtEncoder { sessions, pool, force_key,
                       out_path: out_path.to_string(), audio_src: audio_src.to_string() })
    }

    pub fn alloc_pixel_buffer(&self) -> Option<CVPixelBufferRef> {
        let mut pb: CVPixelBufferRef = std::ptr::null_mut();
        let st = unsafe { CVPixelBufferPoolCreatePixelBuffer(std::ptr::null(), self.pool, &mut pb) };
        if st == 0 && !pb.is_null() { Some(pb) } else { None }
    }

    pub fn encode(&self, pb: CVPixelBufferRef, frame_idx: i64, fps: f64) {
        let (pts, dur, session, props) = if prores_codec().is_some() {
            // ProRes/AVAssetWriter: los CMTime van al .mov tal cual → timescale
            // racional ×1000 (59.94 → 59940; `fps as i32` truncaba a 59).
            // all-intra: round-robin por frame entre los motores ProRes.
            let ts = (fps * 1000.0).round().max(1.0) as i32;
            (CMTime { value: frame_idx * 1000, timescale: ts, flags: 1, epoch: 0 },
             CMTime { value: 1000, timescale: ts, flags: 1, epoch: 0 },
             self.sessions[(frame_idx as usize) % self.sessions.len()],
             std::ptr::null())
        } else {
            // HEVC: el PTS solo ordena los segmentos (el mux re-estampa con -r);
            // el reorden exige valores consecutivos 0,1,2… — NO escalar aquí
            let ts = fps.round().max(1.0) as i32;
            let seg = frame_idx / SEG_LEN;
            let props = if frame_idx % SEG_LEN == 0 { self.force_key as CFDictionaryRef } else { std::ptr::null() };
            (CMTime { value: frame_idx, timescale: ts, flags: 1, epoch: 0 },
             CMTime { value: 1, timescale: ts, flags: 1, epoch: 0 },
             self.sessions[(seg as usize) % self.sessions.len()],
             props)
        };
        let st = unsafe {
            VTCompressionSessionEncodeFrame(session, pb, pts, dur, props, std::ptr::null_mut(), std::ptr::null_mut())
        };
        if st != 0 { eprintln!("   encode st={}", st); }
    }

    pub fn finish(self, total_frames: i64) {
        unsafe {
            for &s in &self.sessions {
                VTCompressionSessionCompleteFrames(s, CMTime { value: i64::MAX, timescale: 1, flags: 1, epoch: 0 });
            }
            for &s in &self.sessions {
                VTCompressionSessionInvalidate(s);
                CFRelease(s as *mut c_void);
            }
            CFRelease(self.pool as *mut c_void);
        }
        if prores_codec().is_some() {
            let (writer, input, dropped) = {
                let mut g = PRMUX.lock().unwrap();
                let m = g.take().unwrap();
                (m.writer as usize, m.input as usize, m.pending.len())
            };
            if dropped > 0 { eprintln!("   ⚠️ {} frames ProRes sin volcar", dropped); }
            if let Some(h) = AUDIO_PUMP.lock().unwrap().take() { let _ = h.join(); }
            unsafe {
                let input = input as *mut Object;
                let writer = writer as *mut Object;
                if !writer.is_null() {
                    let _: () = msg_send![input, markAsFinished];
                    let ok: BOOL = msg_send![writer, finishWriting];
                    if ok == NO { eprintln!("   AVAssetWriter finishWriting falló"); }
                }
            }
            return;
        }
        {
            let mut g = MSTATE.lock().unwrap();
            let st = g.as_mut().unwrap();
            st.total = Some(total_frames);
            flush_ready(st);
            if !st.segments.is_empty() {
                eprintln!("   ⚠️ {} segmentos sin volcar (frames perdidos)", st.segments.len());
            }
            if let Some(mut s) = st.sink.take() { let _ = s.flush(); drop(s); }
            *g = None;
        }
        if let Some(mut c) = MUX_CHILD.lock().unwrap().take() {
            let out = c.wait_with_output();
            if let Ok(o) = out {
                let err = String::from_utf8_lossy(&o.stderr);
                if !err.is_empty() { eprintln!("ffmpeg mux: {}", &err[..err.len().min(500)]); }
            }
        }
    }
}
