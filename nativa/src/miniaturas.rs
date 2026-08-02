//! Las miniaturas del taller, INSTANTÁNEAS: un hilo con sus propios
//! decodificadores de proxy (all-intra: un fotograma = un decode de 1–3 ms)
//! que sirve fotogramas reales para la tira de la bobina y las latas de la
//! estantería. La UI pide por clave; lo que no está se decodifica en
//! milisegundos y aparece en el siguiente redraw. Nada bloquea jamás.

use crate::ui::{Atlas, Gpu, MINI_H, MINI_W};
use filmlook_core::cine::{Cine, Fotograma};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

/// (nombre de la cinta, tiempo en centésimas de segundo)
pub type Clave = (String, u32);

pub struct Miniaturas {
    tx: Sender<(Clave, PathBuf, f64)>,
    rx: Receiver<(Clave, Vec<u8>)>,
    cache: HashMap<Clave, u32>,
    pedidas: HashSet<Clave>,
    /// claves que fallaron: se reintentan tras un enfriamiento (no cada frame)
    fallidas: HashMap<Clave, std::time::Instant>,
    uso: HashMap<Clave, u64>,
    tick: u64,
}

impl Miniaturas {
    pub fn nueva() -> Miniaturas {
        let (tx, prx) = channel::<(Clave, PathBuf, f64)>();
        let (ptx, rx) = channel::<(Clave, Vec<u8>)>();
        std::thread::Builder::new()
            .name("miniaturas".into())
            .spawn(move || hilo(prx, ptx))
            .expect("hilo de miniaturas");
        Miniaturas {
            tx, rx,
            cache: HashMap::new(),
            pedidas: HashSet::new(),
            fallidas: HashMap::new(),
            uso: HashMap::new(),
            tick: 0,
        }
    }

    pub fn tic(&mut self) { self.tick += 1; }

    /// slot del atlas para la clave; si no está, la encarga (y devuelve None)
    pub fn pide(&mut self, clave: Clave, ruta: &PathBuf, t: f64) -> Option<u32> {
        if let Some(&slot) = self.cache.get(&clave) {
            self.uso.insert(clave, self.tick);
            return Some(slot);
        }
        if let Some(cuando) = self.fallidas.get(&clave) {
            if cuando.elapsed().as_secs_f64() < 5.0 { return None; }
            self.fallidas.remove(&clave);
        }
        if self.pedidas.len() < 64 && self.pedidas.insert(clave.clone()) {
            let _ = self.tx.send((clave, ruta.clone(), t));
        }
        None
    }

    /// recoge lo decodificado y lo sube al atlas (con desalojo LRU si está lleno)
    pub fn recibe(&mut self, g: &Gpu, atlas: &mut Atlas) -> bool {
        let mut hay = false;
        while let Ok((clave, rgba)) = self.rx.try_recv() {
            self.pedidas.remove(&clave);
            if rgba.is_empty() {
                // el hilo no pudo: reintento con enfriamiento
                self.fallidas.insert(clave, std::time::Instant::now());
                continue;
            }
            let slot = match atlas.toma() {
                Some(s) => s,
                None => {
                    // desalojar la miniatura más olvidada
                    let Some(vieja) = self.cache.iter()
                        .min_by_key(|(k, _)| self.uso.get(*k).copied().unwrap_or(0))
                        .map(|(k, _)| k.clone()) else { continue };
                    let s = self.cache.remove(&vieja).unwrap();
                    self.uso.remove(&vieja);
                    atlas.suelta(s);
                    match atlas.toma() { Some(s2) => s2, None => { atlas.suelta(s); continue } }
                }
            };
            atlas.sube_slot(g, slot, &rgba);
            self.uso.insert(clave.clone(), self.tick);
            self.cache.insert(clave, slot);
            hay = true;
        }
        hay
    }
}

fn hilo(pedidos: Receiver<(Clave, PathBuf, f64)>, listas: Sender<(Clave, Vec<u8>)>) {
    let mut cines: HashMap<PathBuf, Cine> = HashMap::new();
    while let Ok((clave, ruta, t)) = pedidos.recv() {
        if crate::foto::es_foto(&ruta) {
            match crate::foto::carga(&ruta) {
                Some(fr) => { if listas.send((clave, a_rgba(&fr))).is_err() { return; } }
                None => { if listas.send((clave, Vec::new())).is_err() { return; } }
            }
            continue;
        }
        if !cines.contains_key(&ruta) {
            if cines.len() >= 4 {
                if let Some(k) = cines.keys().next().cloned() { cines.remove(&k); }
            }
            match Cine::abre(&ruta) {
                Ok(mut c) => { c.mitad = true; cines.insert(ruta.clone(), c); }
                Err(e) => {
                    eprintln!("miniaturas: {}: {e:#}", ruta.display());
                    if listas.send((clave, Vec::new())).is_err() { return; }
                    continue;
                }
            }
        }
        if crate::foto::es_foto(&ruta) {
            match crate::foto::carga(&ruta) {
                Some(fr) => { if listas.send((clave, a_rgba(&fr))).is_err() { return; } }
                None => { if listas.send((clave, Vec::new())).is_err() { return; } }
            }
            continue;
        }
        let Some(cine) = cines.get_mut(&ruta) else { continue };
        // el keyframe más cercano basta para una miniatura (con máster sin
        // proxy, el catch-up exacto costaría un GOP entero por lata)
        let Some(fr) = cine.frame_clave(t) else {
            eprintln!("miniaturas: {} t={t:.2}: sin frame", ruta.display());
            if listas.send((clave, Vec::new())).is_err() { return; }
            continue;
        };
        if listas.send((clave, a_rgba(&fr))).is_err() { return; }
    }
}

/// fotograma (códigos YUV de 10 bits) → miniatura RGBA MINI_W×MINI_H
/// (BT.709). El contenido ENCAJA (fit) en el lienzo 160×90 con bandas
/// oscuras: un vertical jamás sale espachurrado.
fn a_rgba(f: &Fotograma) -> Vec<u8> {
    let (w, h) = (f.w as usize, f.h as usize);
    let (cw, ch) = (w / 2, h / 2);
    let mut out = vec![0u8; (MINI_W * MINI_H * 4) as usize];
    // fondo: casi negro película
    for px in out.chunks_exact_mut(4) { px[0] = 18; px[1] = 17; px[2] = 14; px[3] = 255; }
    // rect interior con la proporción de la FUENTE
    let prop = w as f32 / h.max(1) as f32;
    let (mut iw, mut ih) = (MINI_W as f32, MINI_W as f32 / prop);
    if ih > MINI_H as f32 { ih = MINI_H as f32; iw = ih * prop; }
    let (iw, ih) = (iw as usize, ih as usize);
    let (ox, oy) = ((MINI_W as usize - iw) / 2, (MINI_H as usize - ih) / 2);
    for fila in 0..ih {
        let sy = (fila * h / ih.max(1)).min(h - 1);
        for col in 0..iw {
            let sx = (col * w / iw.max(1)).min(w - 1);
            let y = f.y[sy * w + sx] as f32;
            let (ux, uy) = ((sx / 2).min(cw - 1), (sy / 2).min(ch.max(1) - 1));
            let u = f.u[uy * cw + ux] as f32;
            let v = f.v[uy * cw + ux] as f32;
            let yl = (y - 64.0) / 876.0;
            let ul = (u - 512.0) / 896.0;
            let vl = (v - 512.0) / 896.0;
            let r = yl + 1.5748 * vl;
            let g = yl - 0.1873 * ul - 0.4681 * vl;
            let b = yl + 1.8556 * ul;
            let o = ((fila + oy) * MINI_W as usize + col + ox) * 4;
            out[o] = (r.clamp(0.0, 1.0) * 255.0) as u8;
            out[o + 1] = (g.clamp(0.0, 1.0) * 255.0) as u8;
            out[o + 2] = (b.clamp(0.0, 1.0) * 255.0) as u8;
            out[o + 3] = 255;
        }
    }
    out
}
