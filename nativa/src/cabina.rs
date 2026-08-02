//! La cabina de proyección: un hilo decodificador con órdenes de la sala.
//! Política del taller (la misma del webview, Sprint 3): NADA bloquea jamás —
//! el proxy all-intra es el caballo de batalla (scrub y reproducción, un
//! frame = un decode) y el máster a resolución completa entra solo para el
//! frame exacto en pausa. La última orden SIEMPRE gana: un seek nuevo
//! abandona lo que hubiera en marcha.

use filmlook_core::cine::{Cine, Fotograma};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender, SyncSender, TryRecvError};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tier { Proxy, Master }

pub struct Listo {
    pub gen: u64,
    pub tier: Tier,
    pub fr: Fotograma,
}

pub enum Orden {
    /// un frame exacto (scrub / pausa / refinado a máster)
    Frame { gen: u64, ruta: PathBuf, t: f64, tier: Tier },
    /// secuencia [t0, t1) para reproducción
    Toca { gen: u64, ruta: PathBuf, t0: f64, t1: f64, tier: Tier },
    /// precalentar decoders en los ratos muertos (índice + sesión + primer GOP)
    Precalienta { rutas: Vec<(PathBuf, f64)> },
}

pub struct Cabina {
    tx: Sender<Orden>,
    pub rx: Receiver<Listo>,
}

impl Cabina {
    pub fn nueva() -> Cabina {
        let (tx, orx) = channel::<Orden>();
        let (ftx, rx) = sync_channel::<Listo>(3);
        std::thread::Builder::new()
            .name("cabina".into())
            .spawn(move || hilo(orx, ftx))
            .expect("hilo de cabina");
        Cabina { tx, rx }
    }

    pub fn manda(&self, o: Orden) {
        let _ = self.tx.send(o);
    }
}

/// vacía la cola de órdenes y se queda con la ÚLTIMA REAL (la que manda).
/// Las Precalienta no compiten: van a su lista de ratos muertos — si no,
/// una Precalienta en cola se TRAGA la orden de imagen anterior (carrera
/// real cazada en el GPD: el frame inicial nunca llegaba).
fn ultima(ordenes: &Receiver<Orden>, calentar: &mut Vec<(PathBuf, f64)>) -> Result<Option<Orden>, ()> {
    let mut o = None;
    loop {
        match ordenes.try_recv() {
            Ok(Orden::Precalienta { rutas }) => calentar.extend(rutas),
            Ok(x) => o = Some(x),
            Err(TryRecvError::Empty) => return Ok(o),
            Err(TryRecvError::Disconnected) => return Err(()),
        }
    }
}

fn cine<'a>(m: &'a mut HashMap<PathBuf, Cine>, ruta: &PathBuf) -> Option<&'a mut Cine> {
    if !m.contains_key(ruta) {
        // jaula pequeña: los índices son baratos pero las sesiones no son gratis
        if m.len() >= 8 {
            if let Some(k) = m.keys().next().cloned() {
                m.remove(&k);
            }
        }
        match Cine::abre(ruta) {
            Ok(c) => { m.insert(ruta.clone(), c); }
            Err(e) => { eprintln!("cabina: no pude abrir {}: {e:#}", ruta.display()); return None; }
        }
    }
    m.get_mut(ruta)
}

fn crono() -> bool { std::env::var("FL_CRONO").is_ok() }

fn hilo(ordenes: Receiver<Orden>, salida: SyncSender<Listo>) {
    let mut cines: HashMap<PathBuf, Cine> = HashMap::new();
    let mut pendiente: Option<Orden> = None;
    // rutas por precalentar en los ratos muertos (una por vuelta: cualquier
    // orden de verdad SIEMPRE pasa por delante)
    let mut por_calentar: Vec<(PathBuf, f64)> = Vec::new();
    loop {
        let orden = match pendiente.take() {
            Some(o) => o,
            None if por_calentar.is_empty() => {
                match ordenes.recv() { Ok(o) => o, Err(_) => return }
            }
            None => match ordenes.try_recv() {
                Ok(o) => o,
                Err(TryRecvError::Empty) => {
                    let (ruta, t) = por_calentar.remove(0);
                    if !cines.contains_key(&ruta) {
                        let m = std::time::Instant::now();
                        if let Some(c) = cine(&mut cines, &ruta) {
                            c.mitad = true;
                            let _ = c.frame_en(t);
                            if crono() {
                                eprintln!("  cabina precalienta {}: {:.0} ms",
                                          ruta.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
                                          m.elapsed().as_secs_f64() * 1e3);
                            }
                        }
                    }
                    continue;
                }
                Err(TryRecvError::Disconnected) => return,
            },
        };
        // si mientras tanto llegaron más, la última REAL gana
        let orden = match orden {
            Orden::Precalienta { rutas } => { por_calentar.extend(rutas); continue; }
            o => o,
        };
        let orden = match ultima(&ordenes, &mut por_calentar) {
            Ok(Some(o)) => o, Ok(None) => orden, Err(_) => return,
        };

        match orden {
            Orden::Frame { gen, ruta, t, tier } => {
                let m = std::time::Instant::now();
                if let Some(c) = cine(&mut cines, &ruta) {
                    // la preview va a media resolución; solo el refinado a
                    // máster en pausa entrega la resolución completa
                    c.mitad = tier == Tier::Proxy
                    && crate::prefs::PREVIEW_MEDIA.load(std::sync::atomic::Ordering::Relaxed);
                    if let Some(fr) = c.frame_en(t) {
                        if crono() {
                            eprintln!("  cabina {tier:?} t={t:.2}: decode {:.1} ms", m.elapsed().as_secs_f64() * 1e3);
                        }
                        if salida.send(Listo { gen, tier, fr }).is_err() { return; }
                    } else if crono() {
                        eprintln!("  cabina {tier:?} t={t:.2}: SIN FRAME ({:.1} ms)", m.elapsed().as_secs_f64() * 1e3);
                    }
                }
            }
            Orden::Precalienta { rutas } => {
                por_calentar.extend(rutas);
            }
            Orden::Toca { gen, ruta, t0, t1, tier } => {
                if crono() { eprintln!("  cabina toca RECIBIDA t0={t0:.2}"); }
                let Some(c) = cine(&mut cines, &ruta) else {
                    if crono() { eprintln!("  cabina toca: no pude abrir {}", ruta.display()); }
                    continue;
                };
                c.mitad = crate::prefs::PREVIEW_MEDIA.load(std::sync::atomic::Ordering::Relaxed);
                let m = std::time::Instant::now();
                let mut fr = c.arranca_en(t0);
                if crono() {
                    eprintln!("  cabina toca t0={t0:.2}: primer frame {} ({:.1} ms)",
                              if fr.is_some() { "OK" } else { "NINGUNO" },
                              m.elapsed().as_secs_f64() * 1e3);
                }
                let mut enviados = 0u32;
                loop {
                    let Some(f) = fr else { break };
                    let fin = f.pts >= t1;
                    if salida.send(Listo { gen, tier, fr: f }).is_err() { return; }
                    enviados += 1;
                    if fin { break; }
                    match ultima(&ordenes, &mut por_calentar) {
                        Ok(Some(o)) => { pendiente = Some(o); break; }
                        Ok(None) => {}
                        Err(_) => return,
                    }
                    fr = c.siguiente();
                }
                if crono() {
                    eprintln!("  cabina toca fin: {enviados} frames enviados");
                }
            }
        }
    }
}
