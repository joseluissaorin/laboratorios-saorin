//! El sonido del taller: decode AAC en proceso (symphonia) → anillo → cpal.
//! Sin ffplay, sin procesos: la voz sale sincronizada con el reloj del visor
//! (el anillo se llena desde el t exacto del seek) y cada orden nueva
//! resincroniza. Los proxies van sin audio: se lee SIEMPRE del máster.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::{FormatOptions, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

/// duración de una cinta de audio (solo cabecera, sin decodificar)
pub fn dur_de(ruta: &std::path::Path) -> Option<f64> {
    let f = std::fs::File::open(ruta).ok()?;
    let mss = MediaSourceStream::new(Box::new(f), Default::default());
    let mut hint = Hint::new();
    if let Some(e) = ruta.extension().and_then(|e| e.to_str()) { hint.with_extension(e); }
    let s = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default()).ok()?;
    let p = s.format.tracks().iter().find(|t| t.codec_params.sample_rate.is_some())?;
    let rate = p.codec_params.sample_rate? as f64;
    p.codec_params.n_frames.map(|n| n as f64 / rate)
}

pub enum OrdenAudio {
    /// suena [t0, t1) del fichero (tiempo de FUENTE); gain en dB; los
    /// fundidos son relativos a [borde_in, borde_out] de la FUENTE
    Toca { ruta: PathBuf, t0: f64, t1: f64, gain: f64,
           borde_in: f64, fade_in: f64, fade_out: f64,
           banda: Vec<(f64, f64)> },
    Para,
}

pub struct Sonido {
    tx: Sender<OrdenAudio>,
    tx_musica: Sender<OrdenAudio>,
    /// el anillo del FOLEY (los ruiditos del oficio, one-shot)
    taller: Arc<Anillo>,
    rate: u32,
    canales: usize,
    _stream: Option<cpal::Stream>,
}

struct Anillo {
    datos: Mutex<VecDeque<f32>>,
}

impl Sonido {
    pub fn nuevo() -> Sonido {
        let (tx, rx) = channel::<OrdenAudio>();
        let (tx_musica, rx_musica) = channel::<OrdenAudio>();
        let voz = Arc::new(Anillo { datos: Mutex::new(VecDeque::new()) });
        let musica = Arc::new(Anillo { datos: Mutex::new(VecDeque::new()) });
        let taller = Arc::new(Anillo { datos: Mutex::new(VecDeque::new()) });

        let stream = monta_stream(voz.clone(), musica.clone(), taller.clone());
        let (rate, canales) = match &stream {
            Some((_, r, c)) => (*r, *c),
            None => (48000, 2),
        };
        let a = voz.clone();
        std::thread::Builder::new().name("sonido".into())
            .spawn(move || hilo(rx, a, rate, canales)).expect("hilo de sonido");
        let b = musica.clone();
        std::thread::Builder::new().name("musica".into())
            .spawn(move || hilo(rx_musica, b, rate, canales)).expect("hilo de música");
        Sonido { tx, tx_musica, taller, rate, canales,
                 _stream: stream.map(|(s, _, _)| s) }
    }

    /// el FOLEY del taller: ruiditos sintetizados del oficio, al instante
    pub fn foley(&self, cual: Foley) {
        if !crate::prefs::FOLEY.load(std::sync::atomic::Ordering::Relaxed) { return; }
        let r = self.rate as f32;
        let mut d = self.taller.datos.lock().unwrap();
        if d.len() > (r as usize) { return; } // no acumular ruido
        let mete = |d: &mut VecDeque<f32>, muestras: &[f32], canales: usize| {
            for &m in muestras { for _ in 0..canales { d.push_back(m); } }
        };
        match cual {
            Foley::Corte => {
                // chasquido de empalmadora: ruido corto con caída rápida
                let n = (r * 0.05) as usize;
                let mut sem = 0x9e3779b9u32;
                let v: Vec<f32> = (0..n).map(|i| {
                    sem = sem.wrapping_mul(1664525).wrapping_add(1013904223);
                    let ruido = (sem >> 16) as f32 / 32768.0 - 1.0;
                    let env = (1.0 - i as f32 / n as f32).powi(3);
                    ruido * env * 0.35
                }).collect();
                mete(&mut d, &v, self.canales);
            }
            Foley::Lata => {
                // golpe sordo: seno grave con caída
                let n = (r * 0.09) as usize;
                let v: Vec<f32> = (0..n).map(|i| {
                    let t = i as f32 / r;
                    let env = (1.0 - i as f32 / n as f32).powi(2);
                    (t * 110.0 * std::f32::consts::TAU).sin() * env * 0.3
                }).collect();
                mete(&mut d, &v, self.canales);
            }
            Foley::Tick => {
                // tic de marca: seno agudo brevísimo
                let n = (r * 0.03) as usize;
                let v: Vec<f32> = (0..n).map(|i| {
                    let t = i as f32 / r;
                    let env = 1.0 - i as f32 / n as f32;
                    (t * 1320.0 * std::f32::consts::TAU).sin() * env * 0.18
                }).collect();
                mete(&mut d, &v, self.canales);
            }
        }
    }

    /// EL AMBIENTE DE LA SALA (NORTE §1.6): un hilo de fondo sintetizado que
    /// se rellena a demanda — la mesa tiene su reloj y su proyector lejano,
    /// el cuarto oscuro su goteo y su ventilador, el revelado su burbujeo.
    /// Se llama en cada vuelta; solo trabaja si al anillo le falta colchón.
    pub fn ambiente(&self, sala: Ambiente, reloj: f32) {
        if !crate::prefs::FOLEY.load(std::sync::atomic::Ordering::Relaxed)
            || matches!(sala, Ambiente::Ninguno) {
            return;
        }
        let r = self.rate as f32;
        let mut d = self.taller.datos.lock().unwrap();
        // colchón de ~0,25 s: ni se corta ni se acumula
        let objetivo = (r * 0.25) as usize * self.canales;
        if d.len() >= objetivo { return; }
        let n = (objetivo - d.len()) / self.canales.max(1);
        let mut sem = (reloj * 7919.0) as u32 | 1;
        let mut ruido = || {
            sem = sem.wrapping_mul(1664525).wrapping_add(1013904223);
            (sem >> 16) as f32 / 32768.0 - 1.0
        };
        let t0 = reloj;
        for i in 0..n {
            let t = t0 + i as f32 / r;
            let m = match sala {
                Ambiente::Mesa => {
                    // el proyector lejano (zumbido 48 Hz) + el tic-tac del reloj
                    let zumbido = (t * 48.0 * std::f32::consts::TAU).sin() * 0.006;
                    let fase = t % 1.0;
                    let tic = if fase < 0.012 {
                        (1.0 - fase / 0.012) * ruido() * 0.02
                    } else { 0.0 };
                    zumbido + tic + ruido() * 0.0016
                }
                Ambiente::Cuarto => {
                    // el ventilador (ruido rosa lento) y una gota cada ~3,7 s
                    let vent = ruido() * 0.005
                        + (t * 96.0 * std::f32::consts::TAU).sin() * 0.003;
                    let fase = t % 3.7;
                    let gota = if fase < 0.09 {
                        let e = (1.0 - fase / 0.09).powi(3);
                        ((t * 900.0 - fase * 400.0) * std::f32::consts::TAU).sin() * e * 0.05
                    } else { 0.0 };
                    vent + gota
                }
                Ambiente::Revelado => {
                    // el burbujeo de las cubetas: chasquidos aleatorios suaves
                    let base = ruido() * 0.004;
                    let burbuja = if ruido() > 0.9985 { ruido() * 0.06 } else { 0.0 };
                    base + burbuja
                }
                Ambiente::Ninguno => 0.0,
            };
            for _ in 0..self.canales { d.push_back(m); }
        }
    }

    pub fn manda(&self, o: OrdenAudio) {
        let _ = self.tx.send(o);
    }

    /// la pista de MÚSICA tiene su propio decodificador y su anillo
    pub fn manda_musica(&self, o: OrdenAudio) {
        let _ = self.tx_musica.send(o);
    }
}

#[derive(Clone, Copy)]
pub enum Foley { Corte, Lata, Tick }

/// el ambiente sonoro de cada sala del taller
#[derive(Clone, Copy, PartialEq)]
pub enum Ambiente { Ninguno, Mesa, Cuarto, Revelado }

/// el DUCKING de la música bajo la voz (conmutable en ajustes)
pub static DUCKING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

// ── LOS MANDOS DEL MARGEN (§1.6) y EL MEDIDOR (§4bis.11) ─────────────────
//
// El nivel vive aquí y no en cada orden porque hay que poder moverlo MIENTRAS
// suena: el hilo de audio los lee en cada bloque. En centésimas de dB, que es
// lo que cabe en un entero atómico sin cerrojos.
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering as Ord2};
pub static VOL_VOZ: AtomicI32 = AtomicI32::new(0);
pub static VOL_MUSICA: AtomicI32 = AtomicI32::new(0);
/// LAS PALANCAS DE SILENCIO, en el mezclador y no en la orden. Antes vivían
/// sólo en `arranca_toca`: bajar la palanca no hacía nada hasta que la
/// reproducción volvía a empezar, y con la bobina sonando no hacía nada nunca.
/// Aquí las lee el hilo de audio en cada bloque, como los niveles.
pub static MUDO_VOZ: AtomicBool = AtomicBool::new(false);
pub static MUDO_MUSICA: AtomicBool = AtomicBool::new(false);
/// el PICO del último bloque, en milésimas (0..1000) — lo leen los vúmetros
pub static PICO_VOZ: AtomicU32 = AtomicU32::new(0);
pub static PICO_MUSICA: AtomicU32 = AtomicU32::new(0);
/// EL PICO DE LA MEZCLA, POR CANAL. Los de arriba son por bus y suman los dos
/// canales en uno: sirven para ver si una banda está sonando, no para ver si
/// la mezcla se va por un lado. Éstos son lo que uno mira antes de entregar.
pub static PICO_L: AtomicU32 = AtomicU32::new(0);
pub static PICO_R: AtomicU32 = AtomicU32::new(0);

/// pon los niveles del margen (en dB)
pub fn pon_niveles(voz: f64, musica: f64) {
    VOL_VOZ.store((voz.clamp(-40.0, 12.0) * 100.0) as i32, Ord2::Relaxed);
    VOL_MUSICA.store((musica.clamp(-40.0, 12.0) * 100.0) as i32, Ord2::Relaxed);
}

/// baja o sube las dos palancas de silencio
pub fn pon_mudos(voz: bool, musica: bool) {
    MUDO_VOZ.store(voz, Ord2::Relaxed);
    MUDO_MUSICA.store(musica, Ord2::Relaxed);
}

/// lo que MIDEN los vúmetros ahora mismo: (voz, música) en 0..1
pub fn medidor() -> (f32, f32) {
    (PICO_VOZ.load(Ord2::Relaxed) as f32 / 1000.0,
     PICO_MUSICA.load(Ord2::Relaxed) as f32 / 1000.0)
}

/// la MEZCLA, canal a canal: (L, R) en 0..1
pub fn medidor_lr() -> (f32, f32) {
    (PICO_L.load(Ord2::Relaxed) as f32 / 1000.0,
     PICO_R.load(Ord2::Relaxed) as f32 / 1000.0)
}

fn lineal(centesimas: i32) -> f32 {
    if centesimas <= -4000 { return 0.0; }
    10f32.powf(centesimas as f32 / 2000.0)
}

fn monta_stream(voz: Arc<Anillo>, musica: Arc<Anillo>, taller: Arc<Anillo>)
    -> Option<(cpal::Stream, u32, usize)> {
    let host = cpal::default_host();
    let dev = host.default_output_device()?;
    let cfg = dev.default_output_config().ok()?;
    let rate = cfg.sample_rate().0;
    let canales = cfg.channels() as usize;
    let config: cpal::StreamConfig = cfg.into();
    let mut env = 0.0f32;   // la envolvente del sidechain (persiste entre bloques)
    let stream = dev
        .build_output_stream(
            &config,
            move |out: &mut [f32], _| {
                // la envolvente del sidechain vive entre llamadas
                // la MEZCLA del taller: voz + música, sumadas
                let mut a = voz.datos.lock().unwrap();
                let mut b = musica.datos.lock().unwrap();
                let mut c = taller.datos.lock().unwrap();
                // LOS MANDOS DEL MARGEN, leídos en cada bloque: así el nivel
                // se puede mover mientras suena (§1.6)
                let ga = if MUDO_VOZ.load(Ord2::Relaxed) { 0.0 }
                         else { lineal(VOL_VOZ.load(Ord2::Relaxed)) };
                let gb = if MUDO_MUSICA.load(Ord2::Relaxed) { 0.0 }
                         else { lineal(VOL_MUSICA.load(Ord2::Relaxed)) };
                let (mut pa, mut pb) = (0.0f32, 0.0f32);
                // la mezcla, canal a canal: `out` viene entrelazado
                let (mut pl, mut pr_) = (0.0f32, 0.0f32);
                for (n, muestra) in out.iter_mut().enumerate() {
                    let va = a.pop_front().unwrap_or(0.0) * ga;
                    let vb = b.pop_front().unwrap_or(0.0) * gb;
                    let vc = c.pop_front().unwrap_or(0.0);
                    pa = pa.max(va.abs());
                    pb = pb.max(vb.abs());
                    // DUCKING: la música se aparta cuando hay voz (sidechain).
                    // Ataque rápido (se quita del medio ya), recuperación lenta
                    // (no bombea entre palabras) — el gesto del oficio.
                    let nivel = va.abs();
                    if nivel > env {
                        env += (nivel - env) * 0.02;          // ~5 ms
                    } else {
                        env += (nivel - env) * 0.00025;       // ~400 ms
                    }
                    let duck = if DUCKING.load(std::sync::atomic::Ordering::Relaxed) {
                        // hasta −12 dB con voz plena
                        1.0 / (1.0 + env * 12.0)
                    } else { 1.0 };
                    let mez = (va + vb * duck + vc).clamp(-1.0, 1.0);
                    if n % canales == 0 { pl = pl.max(mez.abs()); }
                    else if n % canales == 1 { pr_ = pr_.max(mez.abs()); }
                    *muestra = mez;
                }
                // EL MEDIDOR: el pico del bloque, con caída suave para que la
                // aguja no dé saltos (los vúmetros del margen lo leen)
                let baja = |viejo: &AtomicU32, pico: f32| {
                    let v0 = viejo.load(Ord2::Relaxed) as f32 / 1000.0;
                    let v = if pico > v0 { pico } else { v0 * 0.82 };
                    viejo.store((v.clamp(0.0, 1.0) * 1000.0) as u32, Ord2::Relaxed);
                };
                baja(&PICO_VOZ, pa);
                baja(&PICO_MUSICA, pb);
                // con salida monofónica no hay R que medir: se copia la L
                baja(&PICO_L, pl);
                baja(&PICO_R, if canales > 1 { pr_ } else { pl });
            },
            |e| eprintln!("sonido: {e}"),
            None,
        )
        .ok()?;
    stream.play().ok()?;
    Some((stream, rate, canales))
}

/// vacía las órdenes pendientes quedándose con la última
fn ultima(rx: &Receiver<OrdenAudio>) -> Result<Option<OrdenAudio>, ()> {
    let mut o = None;
    loop {
        match rx.try_recv() {
            Ok(x) => o = Some(x),
            Err(TryRecvError::Empty) => return Ok(o),
            Err(TryRecvError::Disconnected) => return Err(()),
        }
    }
}

fn hilo(rx: Receiver<OrdenAudio>, anillo: Arc<Anillo>, rate: u32, canales: usize) {
    let mut pendiente: Option<OrdenAudio> = None;
    loop {
        let orden = match pendiente.take() {
            Some(o) => o,
            None => match rx.recv() { Ok(o) => o, Err(_) => return },
        };
        let orden = match ultima(&rx) { Ok(Some(o)) => o, Ok(None) => orden, Err(_) => return };
        match orden {
            OrdenAudio::Para => {
                anillo.datos.lock().unwrap().clear();
            }
            OrdenAudio::Toca { ruta, t0, t1, gain, borde_in, fade_in, fade_out, banda } => {
                anillo.datos.lock().unwrap().clear();
                if std::env::var("FL_CRONO").is_ok() {
                    eprintln!("  sonido: toca {:.2}–{:.2} de {} ({:+.1} dB)", t0, t1,
                              ruta.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
                              gain);
                }
                if let Some(o) = decodifica(&rx, &anillo, &ruta, t0, t1, rate, canales,
                                            gain, borde_in, fade_in, fade_out, &banda) {
                    pendiente = Some(o);
                }
            }
        }
    }
}

/// decodifica AAC desde t0 llenando el anillo; devuelve la orden que interrumpió
fn decodifica(
    rx: &Receiver<OrdenAudio>, anillo: &Anillo, ruta: &PathBuf,
    t0: f64, t1: f64, rate_out: u32, canales_out: usize, gain_db: f64,
    borde_in: f64, fade_in: f64, fade_out: f64, banda: &[(f64, f64)],
) -> Option<OrdenAudio> {
    let ganancia = 10f32.powf(gain_db as f32 / 20.0);
    // envolvente de fundidos (sobre el tiempo de FUENTE)
    let puntos: Vec<(f64, f64)> = banda.to_vec();
    let envuelve = move |t: f64| -> f32 {
        let mut f = 1.0f64;
        if fade_in > 0.005 { f = f.min(((t - borde_in) / fade_in).clamp(0.0, 1.0)); }
        if fade_out > 0.005 { f = f.min(((t1 - t) / fade_out).clamp(0.0, 1.0)); }
        // la banda elástica: interpolación lineal en dB entre puntos
        if !puntos.is_empty() {
            let db = if t <= puntos[0].0 { puntos[0].1 }
                else if t >= puntos[puntos.len() - 1].0 { puntos[puntos.len() - 1].1 }
                else {
                    let mut v = puntos[0].1;
                    for w in puntos.windows(2) {
                        if t >= w[0].0 && t <= w[1].0 {
                            let fr = (t - w[0].0) / (w[1].0 - w[0].0).max(1e-9);
                            v = w[0].1 + (w[1].1 - w[0].1) * fr;
                            break;
                        }
                    }
                    v
                };
            f *= 10f64.powf(db / 20.0);
        }
        f as f32
    };
    let f = std::fs::File::open(ruta).ok()?;
    let mss = MediaSourceStream::new(Box::new(f), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = ruta.extension().and_then(|e| e.to_str()) { hint.with_extension(ext); }
    let sondado = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .ok()?;
    let mut fmt = sondado.format;
    let pista = fmt.tracks().iter()
        .find(|t| t.codec_params.sample_rate.is_some())?.clone();
    let mut dec = symphonia::default::get_codecs()
        .make(&pista.codec_params, &DecoderOptions::default()).ok()?;
    let rate_in = pista.codec_params.sample_rate.unwrap_or(48000);
    let tb = pista.codec_params.time_base;

    let buscado = fmt.seek(SeekMode::Coarse,
        SeekTo::Time { time: Time::from(t0.max(0.0)), track_id: Some(pista.id) }).ok();
    // muestras de la fuente que hay que saltar para aterrizar en t0 exacto
    let ts_real = buscado.map(|s| s.actual_ts).unwrap_or(0);
    let t_real = tb.map(|tb| {
        let t = tb.calc_time(ts_real);
        t.seconds as f64 + t.frac
    }).unwrap_or(0.0);
    let mut salta = ((t0 - t_real).max(0.0) * rate_in as f64) as usize;

    let tope_anillo = (rate_out as usize) * canales_out; // ~1 s por delante
    let paso = rate_in as f64 / rate_out as f64;
    let mut fase = 0.0f64;
    let mut previa = [0.0f32; 2];
    let mut t_fuente = t0;

    loop {
        let paquete = match fmt.next_packet() { Ok(p) => p, Err(_) => return None };
        if paquete.track_id() != pista.id { continue; }
        let Ok(dcd) = dec.decode(&paquete) else { continue };
        let spec = *dcd.spec();
        let n_can = spec.channels.count().max(1);
        let mut sb = SampleBuffer::<f32>::new(dcd.capacity() as u64, spec);
        sb.copy_interleaved_ref(dcd);
        let mut muestras = sb.samples();
        if salta > 0 {
            let s = (salta * n_can).min(muestras.len());
            muestras = &muestras[s..];
            salta -= s / n_can;
        }
        if muestras.is_empty() { continue; }

        // remuestreo lineal + mezcla al layout del dispositivo
        let frames_in = muestras.len() / n_can;
        let mut sal: Vec<f32> = Vec::with_capacity((frames_in as f64 / paso) as usize * canales_out + 8);
        while fase < frames_in as f64 {
            let i = fase as usize;
            let fr = (fase - i as f64) as f32;
            let izq0 = muestras[i * n_can];
            let der0 = muestras[i * n_can + (n_can > 1) as usize];
            let (izq1, der1) = if i + 1 < frames_in {
                (muestras[(i + 1) * n_can], muestras[(i + 1) * n_can + (n_can > 1) as usize])
            } else { (izq0, der0) };
            let izq = izq0 + (izq1 - izq0) * fr;
            let der = der0 + (der1 - der0) * fr;
            previa = [izq, der];
            let t_s = t_fuente + fase / rate_in as f64;
            let env = envuelve(t_s);
            for c in 0..canales_out {
                sal.push(env * ganancia * if c == 0 { izq } else if c == 1 { der } else { 0.0 });
            }
            fase += paso;
        }
        fase -= frames_in as f64;
        let _ = previa;
        t_fuente += frames_in as f64 / rate_in as f64;

        // meter en el anillo sin dejar que crezca más de ~1 s
        let mut i = 0usize;
        while i < sal.len() {
            {
                let mut d = anillo.datos.lock().unwrap();
                let hueco = tope_anillo.saturating_sub(d.len());
                let n = hueco.min(sal.len() - i);
                d.extend(sal[i..i + n].iter().copied());
                i += n;
            }
            if i < sal.len() {
                match ultima(rx) {
                    Ok(Some(o)) => return Some(o),
                    Ok(None) => std::thread::sleep(std::time::Duration::from_millis(8)),
                    Err(_) => return None,
                }
            }
        }
        if t_fuente >= t1 { return None; }
        match ultima(rx) {
            Ok(Some(o)) => return Some(o),
            Ok(None) => {}
            Err(_) => return None,
        }
    }
}
