//! Las ondas de audio de las cintas: un hilo decodifica el AAC del máster
//! UNA vez (symphonia, en proceso) y entrega ~1000 picos normalizados por
//! cinta. La tira de la bobina los dibuja dentro, como el webview. Nada
//! bloquea: mientras se cuecen, la tira va sin onda.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub const CUBOS: usize = 1000;

pub struct Ondas {
    tx: Sender<(String, PathBuf)>,
    rx: Receiver<(String, Vec<f32>)>,
    /// media → picos 0..1 (CUBOS cubos sobre la duración entera)
    pub listas: HashMap<String, Vec<f32>>,
    pedidas: HashSet<String>,
}

impl Ondas {
    pub fn nuevas() -> Ondas {
        let (tx, prx) = channel::<(String, PathBuf)>();
        let (ptx, rx) = channel::<(String, Vec<f32>)>();
        std::thread::Builder::new()
            .name("ondas".into())
            .spawn(move || hilo(prx, ptx))
            .expect("hilo de ondas");
        Ondas { tx, rx, listas: HashMap::new(), pedidas: HashSet::new() }
    }

    /// pide (una vez) y devuelve los picos si ya están
    pub fn pide(&mut self, media: &str, ruta: &PathBuf) -> Option<&Vec<f32>> {
        if !self.listas.contains_key(media) && self.pedidas.insert(media.to_string()) {
            let _ = self.tx.send((media.to_string(), ruta.clone()));
        }
        self.listas.get(media)
    }

    pub fn recibe(&mut self) {
        while let Ok((media, picos)) = self.rx.try_recv() {
            self.pedidas.remove(&media);
            self.listas.insert(media, picos);
        }
    }
}

fn hilo(rx: Receiver<(String, PathBuf)>, tx: Sender<(String, Vec<f32>)>) {
    while let Ok((media, ruta)) = rx.recv() {
        let picos = calcula(&ruta).unwrap_or_default();
        if tx.send((media, picos)).is_err() { return; }
    }
}

/// máximo absoluto por cubo sobre la cinta entera (mono, normalizado)
fn calcula(ruta: &PathBuf) -> Option<Vec<f32>> {
    let f = std::fs::File::open(ruta).ok()?;
    let mss = MediaSourceStream::new(Box::new(f), Default::default());
    let mut hint = Hint::new();
    if let Some(e) = ruta.extension().and_then(|e| e.to_str()) { hint.with_extension(e); }
    let sondado = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default()).ok()?;
    let mut fmt = sondado.format;
    let pista = fmt.tracks().iter().find(|t| t.codec_params.sample_rate.is_some())?.clone();
    let mut dec = symphonia::default::get_codecs()
        .make(&pista.codec_params, &DecoderOptions::default()).ok()?;
    let rate = pista.codec_params.sample_rate? as usize;
    let total = pista.codec_params.n_frames
        .map(|n| n as usize)
        .unwrap_or(rate * 60);
    let por_cubo = (total / CUBOS).max(1);
    let mut picos = vec![0.0f32; CUBOS];
    let mut n = 0usize;
    while let Ok(paquete) = fmt.next_packet() {
        if paquete.track_id() != pista.id { continue; }
        let Ok(dcd) = dec.decode(&paquete) else { continue };
        let spec = *dcd.spec();
        let canales = spec.channels.count().max(1);
        let mut sb = SampleBuffer::<f32>::new(dcd.capacity() as u64, spec);
        sb.copy_interleaved_ref(dcd);
        for tr in sb.samples().chunks_exact(canales) {
            let v = tr[0].abs();
            let k = (n / por_cubo).min(CUBOS - 1);
            if v > picos[k] { picos[k] = v; }
            n += 1;
        }
    }
    let max = picos.iter().cloned().fold(0.0f32, f32::max).max(0.05);
    for p in &mut picos { *p /= max; }
    Some(picos)
}
