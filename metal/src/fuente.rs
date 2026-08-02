//! UNA FUENTE DE LA BOBINA: un fichero de vídeo del que se pide *el
//! fotograma que toca*, en la GPU y sin pasar por la CPU.
//!
//! Es la unión de las dos mitades que ya existían en el taller y nunca se
//! habían juntado:
//!
//! - `core::cine` sabe **buscar**: tiene el índice del MP4 (`indice.rs`),
//!   salta al fotograma clave anterior, decodifica el arranque y lo tira, y
//!   entrega en orden de PANTALLA (reordenando los fotogramas B). Pero
//!   convierte a planos en la CPU — 25 MB por fotograma de copia, imposible
//!   a la velocidad del máster.
//! - `metal::decode_vt` sabe ir **rápido**: deja el fotograma en un
//!   CVPixelBuffer que se importa como textura sin copiar un byte. Pero lee
//!   el fichero entero de una vez y no sabe empezar por la mitad.
//!
//! Aquí están las dos: índice + búsqueda + orden de pantalla + zero-copy.
//! Con esto el motor puede cortar él solo y desaparece la fase de corte con
//! ffmpeg, que era la mitad del tiempo del revelado.

use crate::indice::{Codec, Indice};
use crate::vt_ffi::*;
use anyhow::Result;
use std::collections::VecDeque;
use std::ffi::c_void;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// un fotograma decodificado, todavía en la GPU
pub struct Cuadro {
    pub pb: CVPixelBufferRef,
    /// índice de muestra (orden de decodificación) del que salió
    pub muestra: usize,
    /// tiempo de presentación en segundos DE LA FUENTE
    pub pts: f64,
}

impl Drop for Cuadro {
    fn drop(&mut self) {
        if !self.pb.is_null() { unsafe { CFRelease(self.pb as *mut c_void) }; }
    }
}
unsafe impl Send for Cuadro {}

/// la cola por la que el callback de VideoToolbox devuelve lo decodificado.
/// Cada sesión tiene la suya: durante un encadenado hay dos fuentes vivas y
/// una cola global las mezclaría (el fallo que tenía `decode_vt`).
struct Buzon(Mutex<Vec<(CVPixelBufferRef, i64)>>);
unsafe impl Send for Buzon {}
unsafe impl Sync for Buzon {}

unsafe extern "C" fn al_cuadro(
    refcon: *mut c_void, source: *mut c_void, status: OSStatus, _flags: u32,
    image: CVPixelBufferRef, _pts: CMTime, _dur: CMTime,
) {
    if status != 0 || image.is_null() || refcon.is_null() { return; }
    core_foundation::base::CFRetain(image as *const c_void);
    // el índice de muestra viaja en `sourceFrameRefCon`, que VideoToolbox
    // devuelve intacto: más fiable que el sello de tiempo
    let buzon = &*(refcon as *const Buzon);
    buzon.0.lock().unwrap().push((image, source as i64));
}

pub struct Fuente {
    pub ind: Indice,
    f: File,
    sesion: VTDecompressionSessionRef,
    fmt: CMVideoFormatDescriptionRef,
    buzon: Arc<Buzon>,
    /// lo decodificado y aún no servido, por índice de muestra
    listos: VecDeque<(usize, CVPixelBufferRef)>,
    /// siguiente muestra a alimentar (orden de decodificación)
    pos: usize,
    /// posición en `orden_pts`: el siguiente fotograma en orden de PANTALLA
    sig: usize,
    lee: Vec<u8>,
    pub ruta: std::path::PathBuf,
}

impl Fuente {
    pub fn abre(ruta: &Path) -> Result<Self> {
        let ind = Indice::abre(ruta)?;
        let f = File::open(ruta)?;
        let mut fmt: CMVideoFormatDescriptionRef = std::ptr::null_mut();
        let st = unsafe {
            match &ind.codec {
                Codec::Hevc { vps, sps, pps, nal_len } => {
                    let ptrs = [vps.as_ptr(), sps.as_ptr(), pps.as_ptr()];
                    let tams = [vps.len(), sps.len(), pps.len()];
                    CMVideoFormatDescriptionCreateFromHEVCParameterSets(
                        std::ptr::null(), 3, ptrs.as_ptr(), tams.as_ptr(),
                        *nal_len as i32, std::ptr::null(), &mut fmt)
                }
                Codec::H264 { sps, pps, nal_len } => {
                    let ptrs = [sps.as_ptr(), pps.as_ptr()];
                    let tams = [sps.len(), pps.len()];
                    CMVideoFormatDescriptionCreateFromH264ParameterSets(
                        std::ptr::null(), 2, ptrs.as_ptr(), tams.as_ptr(),
                        *nal_len as i32, &mut fmt)
                }
            }
        };
        anyhow::ensure!(st == 0, "format description de {}: {st}", ruta.display());

        let buzon = Arc::new(Buzon(Mutex::new(Vec::new())));
        let cb = VTDecompressionOutputCallbackRecord {
            decompression_output_callback: al_cuadro,
            decompression_output_ref_con: Arc::as_ptr(&buzon) as *mut c_void,
        };
        // x420 para HEVC 10 bits, 420v para H.264 8 bits: si no se pide
        // explícitamente, VideoToolbox devuelve 'p420' empaquetado y la
        // importación R16/RG16 lee basura (trampa ya documentada)
        let quiere: u32 = match &ind.codec {
            Codec::Hevc { .. } => KCVPIXELFORMATTYPE_420YPCBCR10BIPLANARVIDEORANGE,
            Codec::H264 { .. } => u32::from_be_bytes(*b"420v"),
        };
        let pf = quiere.to_le_bytes();
        let num = unsafe { CFNumberCreate(std::ptr::null(), 3, pf.as_ptr() as *const c_void) };
        let dest = cfdict(&[
            (unsafe { kCVPixelBufferPixelFormatTypeKey }, num as *const c_void),
            (unsafe { kCVPixelBufferIOSurfacePropertiesKey }, cfdict(&[]) as *const c_void),
            (unsafe { kCVPixelBufferMetalCompatibilityKey }, unsafe { kCFBooleanTrue as *const c_void }),
        ]);
        let mut sesion: VTDecompressionSessionRef = std::ptr::null_mut();
        let st = unsafe {
            VTDecompressionSessionCreate(std::ptr::null(), fmt, std::ptr::null(),
                                         dest as *const c_void, &cb, &mut sesion)
        };
        anyhow::ensure!(st == 0, "sesión de decodificación de {}: {st}", ruta.display());
        Ok(Fuente { ind, f, sesion, fmt, buzon, listos: VecDeque::new(),
                    pos: 0, sig: 0, lee: Vec::new(), ruta: ruta.to_path_buf() })
    }

    pub fn info(&self) -> (u32, u32, f64, f64) {
        (self.ind.w, self.ind.h, self.ind.fps, self.ind.dur)
    }

    /// Alimenta una muestra al decodificador. **Síncrono**: sin
    /// `EnableAsynchronousDecompression` el callback ya ha devuelto el
    /// fotograma cuando esto retorna, y no hay nada que reordenar por
    /// carreras (el reordenamiento que sí hace falta, el de los fotogramas B,
    /// lo lleva `orden_pts` del índice). El coste que esto tenía en la
    /// preview —bloquear el hilo— aquí no importa: el motor solapa con hilos
    /// propios, no con el decodificador.
    fn alimenta(&mut self, idx: usize) {
        let Some(m) = self.ind.muestras.get(idx).copied() else { return };
        self.lee.resize(m.tam as usize, 0);
        if self.f.seek(SeekFrom::Start(m.off)).is_err() { return; }
        if self.f.read_exact(&mut self.lee).is_err() { return; }
        let mut bb: CMBlockBufferRef = std::ptr::null_mut();
        let st = unsafe {
            CMBlockBufferCreateWithMemoryBlock(
                std::ptr::null(), self.lee.as_mut_ptr() as *mut c_void, self.lee.len(),
                kCFAllocatorNull, std::ptr::null(), 0, self.lee.len(), 0, &mut bb)
        };
        if st != 0 { return; }
        let tam = self.lee.len();
        let mut sb: CMSampleBufferRef = std::ptr::null_mut();
        let st = unsafe {
            CMSampleBufferCreateReady(std::ptr::null(), bb, self.fmt, 1, 0,
                                      std::ptr::null(), 1, &tam, &mut sb)
        };
        if st == 0 {
            unsafe {
                VTDecompressionSessionDecodeFrame(self.sesion, sb, 0,
                                                  idx as *mut c_void, std::ptr::null_mut());
                CFRelease(sb as *mut c_void);
            }
        }
        unsafe { CFRelease(bb as *mut c_void) };
        // recoger lo que el callback haya dejado
        let mut b = self.buzon.0.lock().unwrap();
        for (pb, i) in b.drain(..) { self.listos.push_back((i as usize, pb)); }
    }

    /// EL SALTO. Coloca el cursor en el fotograma que contiene `t`: busca su
    /// fotograma clave anterior y decodifica el arranque **tirándolo** — esos
    /// fotogramas no llegan a tocar la GPU del look. Es el coste real de
    /// empezar un clip por la mitad, y por eso conviene solaparlo con el clip
    /// que se está yendo (MOTOR §5bis).
    pub fn busca(&mut self, t: f64) -> usize {
        let i = self.ind.muestra_en(t);
        let k = self.ind.keyframe_para(i);
        // si ya venimos decodificando este mismo GOP, seguir sin repagarlo
        let desde = if self.pos > k && self.pos <= i { self.pos } else {
            self.suelta_listos();
            unsafe { VTDecompressionSessionWaitForAsynchronousFrames(self.sesion) };
            self.buzon.0.lock().unwrap().drain(..).for_each(|(pb, _)|
                unsafe { CFRelease(pb as *mut c_void) });
            k
        };
        // DÓNDE EMPIEZA LA PANTALLA, antes de decodificar nada: hace falta
        // para saber qué se puede tirar y qué no
        let sig = self.ind.pantalla_de(i);
        for idx in desde..i {
            self.alimenta(idx);
            // el arranque del GOP, a la basura — PERO SOLO LO QUE YA NO SE VA
            // A VER. Aquí se tiraba todo, y con fotogramas B eso es tirar
            // trabajo hecho: en un IBBBP el fotograma P se decodifica ANTES
            // que las tres B que van delante suyo en pantalla, así que al
            // saltar a una B se soltaban las que venían después y el motor se
            // quedaba esperando a un fotograma que ya no iba a volver —
            // repetidos y negros en el máster. No se ve con el material de la
            // casa (una Insta360 sin fotogramas B) y se ve a la primera con
            // cualquier vídeo de móvil.
            self.suelta_anteriores(sig);
        }
        self.alimenta(i);
        self.suelta_anteriores(sig);
        self.pos = i + 1;
        self.sig = sig;
        i
    }

    fn suelta_listos(&mut self) {
        for (_, pb) in self.listos.drain(..) { unsafe { CFRelease(pb as *mut c_void) }; }
    }

    /// suelta lo decodificado que cae ANTES de `sig` en orden de pantalla: eso
    /// es lo único que ya no se va a ver. Lo que queda es la profundidad de
    /// reordenación del códec —unos pocos fotogramas—, no el GOP entero.
    fn suelta_anteriores(&mut self, sig: usize) {
        let ind = &self.ind;
        let mut quedan = VecDeque::with_capacity(self.listos.len());
        for (m, pb) in self.listos.drain(..) {
            if ind.pantalla_de(m) < sig { unsafe { CFRelease(pb as *mut c_void) }; }
            else { quedan.push_back((m, pb)); }
        }
        self.listos = quedan;
    }

    /// El siguiente fotograma **en orden de pantalla**. None = fin.
    pub fn siguiente(&mut self) -> Option<Cuadro> {
        let esperado = *self.ind.orden_pts.get(self.sig)? as usize;
        let mut vueltas = 0;
        while !self.listos.iter().any(|(i, _)| *i == esperado) {
            if self.pos >= self.ind.muestras.len() || vueltas > 24 { break; }
            let idx = self.pos;
            self.pos += 1;
            self.alimenta(idx);
            vueltas += 1;
        }
        let p = self.listos.iter().position(|(i, _)| *i == esperado)?;
        let (m, pb) = self.listos.remove(p).unwrap();
        self.sig += 1;
        Some(Cuadro { pb, muestra: m, pts: self.ind.pts_s(m) })
    }

    /// el fotograma cuyo tiempo de presentación cubre `t`, avanzando desde
    /// donde estemos. Devuelve None al acabarse la fuente.
    ///
    /// EL ORDEN DE PANTALLA MANDA, y esto se comparaba en orden de
    /// DECODIFICACIÓN: `c.muestra >= objetivo` con dos índices de decode
    /// mientras se recorre la pantalla. Con fotogramas B los dos órdenes no
    /// coinciden —un IBBP se decodifica 0,2,3,1 y se ve 0,1,2,3— así que el
    /// bucle paraba en un fotograma **anterior** al pedido y el máster daba un
    /// tirón en los primeros fotogramas de cada plano, justo después de cada
    /// corte. El motor de Windows ya comparaba por PTS; el del Mac era el que
    /// se salía del camino.
    pub fn en(&mut self, t: f64) -> Option<Cuadro> {
        let objetivo = self.ind.muestra_en(t);
        let pts_objetivo = self.ind.pts_s(objetivo);
        // ¿hay que retroceder o dar un salto largo? entonces buscar de nuevo,
        // y la cuenta es en POSICIONES DE PANTALLA, que es como avanzamos
        let pos = self.ind.pantalla_de(objetivo);
        if pos < self.sig || pos > self.sig + 8 { self.busca(t); }
        // medio fotograma de holgura: los pts vienen del contenedor y no
        // caen exactamente donde los pone la rejilla del proyecto
        let medio = 0.5 / self.ind.fps.max(1.0);
        loop {
            let c = self.siguiente()?;
            if c.pts + medio >= pts_objetivo { return Some(c); }
        }
    }
}

/// La `Fuente` puede mudarse a otro hilo: sus punteros de VideoToolbox y su
/// fichero son suyos y de nadie más — cada fuente tiene su propia sesión y su
/// propio buzón (por eso el buzón dejó de ser global). Lo que NO se puede es
/// tocar la misma fuente desde dos hilos, y por eso no es `Sync`.
unsafe impl Send for Fuente {}

impl Drop for Fuente {
    fn drop(&mut self) {
        self.suelta_listos();
        unsafe {
            VTDecompressionSessionInvalidate(self.sesion);
            CFRelease(self.sesion as *mut c_void);
            CFRelease(self.fmt as *mut c_void);
        }
    }
}
