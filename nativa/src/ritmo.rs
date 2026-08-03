//! EL PULSO DE LA MÚSICA: dónde caen los golpes, para sembrar la bobina de
//! marcas y montar al compás.
//!
//! El método es el clásico de verdad (Ellis 2007, el mismo que lleva
//! librosa), no un truco de umbral:
//!
//!   1. la **envolvente de ataques**: STFT corta y flujo espectral positivo
//!      sobre log-magnitud — sube cuando ENTRA energía nueva (un golpe),
//!      no cuando simplemente hay volumen;
//!   2. el **tempo** por autocorrelación de esa envolvente, con un prior
//!      log-normal alrededor de 120 BPM (que un vals no se lea al doble);
//!   3. los **golpes** por programación dinámica: cada cuadro elige su
//!      antecesor ideal a ~un periodo de distancia, premiando caer donde la
//!      envolvente pega y castigando estirar o encoger el compás. Aguanta
//!      rubato y silencios sin perder el hilo, que es lo que un peine de
//!      marcas cada 60/BPM segundos no hace.
//!
//! Todo en proceso (symphonia + una FFT de casa): cero dependencias nuevas,
//! compila igual en el Mac y en Windows.

use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// el compás de una cinta: sus golpes (en segundos DEL FICHERO) y el tempo
pub struct Compas {
    pub bpm: f64,
    pub golpes: Vec<f64>,
}

const VENTANA: usize = 1024;
const SALTO: usize = 256;

/// analiza una cinta entera. None = no se pudo decodificar o no hay pulso
/// que valga (habla, silencio, ambiente).
pub fn analiza(ruta: &Path) -> Option<Compas> {
    let (mono, rate) = decodifica_mono(ruta)?;
    analiza_desde(&mono, rate)
}

/// el análisis a partir del PCM mono (ya diezmado): separado para poder
/// probarlo con señales fabricadas, sin ficheros de por medio
fn analiza_desde(mono: &[f32], rate: usize) -> Option<Compas> {
    if mono.len() < rate * 2 {
        return None; // menos de dos segundos: ahí no hay compás que medir
    }
    let fs = rate as f64 / SALTO as f64; // cuadros de envolvente por segundo

    // ── 1 · la envolvente de ataques ────────────────────────────────────
    let hann: Vec<f32> = (0..VENTANA)
        .map(|i| {
            let x = std::f32::consts::PI * i as f32 / VENTANA as f32;
            x.sin() * x.sin()
        })
        .collect();
    let fft = Fft::nueva(VENTANA);
    let mut re = vec![0.0f32; VENTANA];
    let mut im = vec![0.0f32; VENTANA];
    let mut previa = vec![0.0f32; VENTANA / 2];
    let mut env: Vec<f32> = Vec::with_capacity(mono.len() / SALTO);
    let mut i = 0;
    while i + VENTANA <= mono.len() {
        for k in 0..VENTANA {
            re[k] = mono[i + k] * hann[k];
            im[k] = 0.0;
        }
        fft.transforma(&mut re, &mut im);
        let mut flujo = 0.0f32;
        for k in 1..VENTANA / 2 {
            let m = (re[k] * re[k] + im[k] * im[k]).sqrt();
            let l = (1.0 + 10.0 * m).ln();
            let d = l - previa[k];
            if d > 0.0 {
                flujo += d;
            }
            previa[k] = l;
        }
        env.push(flujo);
        i += SALTO;
    }
    if env.len() < 32 {
        return None;
    }

    // quitarle la media local (±0.35 s): que cuente el GOLPE, no el volumen
    let w = (fs * 0.35).round() as usize;
    let n = env.len();
    let mut acum = vec![0.0f64; n + 1];
    for k in 0..n {
        acum[k + 1] = acum[k] + env[k] as f64;
    }
    let mut limpia = vec![0.0f32; n];
    let mut centrada = vec![0.0f32; n]; // sin rectificar: para la guarda
    for k in 0..n {
        let a = k.saturating_sub(w);
        let b = (k + w + 1).min(n);
        let media = ((acum[b] - acum[a]) / (b - a) as f64) as f32;
        centrada[k] = env[k] - media;
        limpia[k] = centrada[k].max(0.0);
    }
    let tope = limpia.iter().cloned().fold(0.0f32, f32::max);
    if tope <= 1e-6 {
        return None;
    }
    for v in &mut limpia {
        *v /= tope;
    }

    // ── 2 · el tempo, por autocorrelación con prior ─────────────────────
    // lags de 240 BPM (0.25 s) a 40 BPM (1.5 s)
    let lag_min = (fs * 60.0 / 240.0).round() as usize;
    let lag_max = ((fs * 60.0 / 40.0).round() as usize).min(n / 2);
    if lag_min + 2 >= lag_max {
        return None;
    }
    let mut mejor = (0.0f64, 0usize);
    for lag in lag_min..=lag_max {
        let mut r = 0.0f64;
        for k in 0..n - lag {
            r += limpia[k] as f64 * limpia[k + lag] as f64;
        }
        // prior log-normal centrado en 120 BPM (periodo 0.5 s), σ = 1 octava
        let periodo = lag as f64 / fs;
        let prior = (-0.5 * ((periodo / 0.5).log2()).powi(2)).exp();
        let v = r * prior;
        if v > mejor.0 {
            mejor = (v, lag);
        }
    }
    let p = mejor.1;
    if p == 0 {
        return None;
    }
    // ¿HAY pulso siquiera? La medida honesta es la autocorrelación de
    // Pearson de la envolvente SIN rectificar: la versión rectificada
    // hereda un lóbulo del propio filtro de media local (medido: ruido
    // blanco daba contraste 5.6 por ese artefacto) — sin rectificar, el
    // ruido queda en ~0 y la música asoma sin dudas.
    let energia: f64 = centrada.iter().map(|&v| (v as f64) * (v as f64)).sum();
    let mut rho = 0.0f64;
    for k in 0..n - p {
        rho += centrada[k] as f64 * centrada[k + p] as f64;
    }
    rho /= energia.max(1e-9);
    if std::env::var("FL_RITMO_DEBUG").is_ok() {
        eprintln!("p={p} rho={rho:.3} fs={fs:.1} n={n}");
    }
    if rho < 0.12 {
        return None; // nada se repite: eso no es música con compás
    }

    // ── 3 · los golpes, por programación dinámica (Ellis) ───────────────
    // la envolvente, apenas suavizada (σ = P/32) para que el pico no baile
    let sigma = (p as f32 / 32.0).max(1.0);
    let radio = (2.0 * sigma).ceil() as usize;
    let nucleo: Vec<f32> = (0..=2 * radio)
        .map(|j| {
            let d = j as f32 - radio as f32;
            (-0.5 * (d / sigma) * (d / sigma)).exp()
        })
        .collect();
    let suma_n: f32 = nucleo.iter().sum();
    let mut local = vec![0.0f32; n];
    for k in 0..n {
        let mut s = 0.0f32;
        for (j, &g) in nucleo.iter().enumerate() {
            let idx = k as isize + j as isize - radio as isize;
            if idx >= 0 && (idx as usize) < n {
                s += limpia[idx as usize] * g;
            }
        }
        local[k] = s / suma_n;
    }

    let lo = ((p as f64) / 2.0).round().max(1.0) as usize;
    let hi = ((p as f64) * 2.0).round() as usize;
    let apriete = 100.0f64; // cuánto castiga desviarse del periodo
    let mut cum = vec![0.0f64; n];
    let mut atras = vec![-1isize; n];
    for k in 0..n {
        let mut mejor_s = 0.0f64;
        let mut mejor_j = -1isize;
        if k >= lo {
            let desde = k.saturating_sub(hi);
            for j in desde..=k - lo {
                let d = (k - j) as f64;
                let s = cum[j] - apriete * (d / p as f64).ln().powi(2);
                if s > mejor_s {
                    mejor_s = s;
                    mejor_j = j as isize;
                }
            }
        }
        cum[k] = local[k] as f64 + mejor_s;
        atras[k] = mejor_j;
    }

    // el último golpe: el mejor final en los dos últimos periodos
    let cola = n.saturating_sub(hi.max(1));
    let mut fin = cola;
    for k in cola..n {
        if cum[k] > cum[fin] {
            fin = k;
        }
    }
    let mut cuadros = vec![fin];
    let mut k = fin;
    while atras[k] >= 0 {
        k = atras[k] as usize;
        cuadros.push(k);
    }
    cuadros.reverse();

    // recortar los extremos flojos: un golpe donde la música aún no ha
    // empezado (o ya acabó) es mentira
    let mut fuerzas: Vec<f32> = cuadros.iter().map(|&c| local[c]).collect();
    fuerzas.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mediana = fuerzas.get(fuerzas.len() / 2).copied().unwrap_or(0.0);
    let umbral = mediana * 0.2;
    let ini = cuadros.iter().position(|&c| local[c] >= umbral).unwrap_or(0);
    let fin2 = cuadros.iter().rposition(|&c| local[c] >= umbral).unwrap_or(0);
    if fin2 <= ini + 3 {
        return None; // tres golpes no son un compás
    }
    let golpes: Vec<f64> = cuadros[ini..=fin2]
        .iter()
        .map(|&c| (c * SALTO) as f64 / rate as f64)
        .collect();

    // el tempo dicho con los golpes de verdad (mediana del intervalo)
    let mut inter: Vec<f64> = golpes.windows(2).map(|w| w[1] - w[0]).collect();
    inter.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let paso = inter.get(inter.len() / 2).copied().unwrap_or(0.5);
    Some(Compas { bpm: 60.0 / paso.max(1e-3), golpes })
}

/// la cinta entera en mono, diezmada ×2 (el pulso no vive por encima de
/// 6 kHz y la FFT se hace a mitad de precio)
fn decodifica_mono(ruta: &Path) -> Option<(Vec<f32>, usize)> {
    let f = std::fs::File::open(ruta).ok()?;
    let mss = MediaSourceStream::new(Box::new(f), Default::default());
    let mut hint = Hint::new();
    if let Some(e) = ruta.extension().and_then(|e| e.to_str()) {
        hint.with_extension(e);
    }
    let sondado = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .ok()?;
    let mut fmt = sondado.format;
    let pista = fmt
        .tracks()
        .iter()
        .find(|t| t.codec_params.sample_rate.is_some())?
        .clone();
    let mut dec = symphonia::default::get_codecs()
        .make(&pista.codec_params, &DecoderOptions::default())
        .ok()?;
    let rate = pista.codec_params.sample_rate? as usize;
    let mut mono: Vec<f32> = Vec::new();
    let mut resto: Option<f32> = None;
    while let Ok(paquete) = fmt.next_packet() {
        if paquete.track_id() != pista.id {
            continue;
        }
        let Ok(dcd) = dec.decode(&paquete) else { continue };
        let spec = *dcd.spec();
        let canales = spec.channels.count().max(1);
        let mut sb = SampleBuffer::<f32>::new(dcd.capacity() as u64, spec);
        sb.copy_interleaved_ref(dcd);
        for tr in sb.samples().chunks_exact(canales) {
            let v = tr.iter().sum::<f32>() / canales as f32;
            match resto.take() {
                None => resto = Some(v),
                Some(a) => mono.push((a + v) * 0.5), // diezmar ×2 con media
            }
        }
    }
    if mono.is_empty() {
        return None;
    }
    Some((mono, rate / 2))
}

/// UNA FFT DE CASA: radix-2 iterativa con giros precomputados. No hace falta
/// más — 1024 puntos, magnitud para el flujo espectral — y nos ahorra una
/// dependencia entera.
struct Fft {
    n: usize,
    rev: Vec<u32>,
    giros: Vec<(f32, f32)>,
}

impl Fft {
    fn nueva(n: usize) -> Fft {
        debug_assert!(n.is_power_of_two());
        let bits = n.trailing_zeros();
        let rev = (0..n as u32).map(|i| i.reverse_bits() >> (32 - bits)).collect();
        let giros = (0..n / 2)
            .map(|k| {
                let a = -2.0 * std::f32::consts::PI * k as f32 / n as f32;
                (a.cos(), a.sin())
            })
            .collect();
        Fft { n, rev, giros }
    }

    fn transforma(&self, re: &mut [f32], im: &mut [f32]) {
        for i in 0..self.n {
            let j = self.rev[i] as usize;
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut tramo = 2;
        while tramo <= self.n {
            let salto = self.n / tramo;
            let mitad = tramo / 2;
            for base in (0..self.n).step_by(tramo) {
                for k in 0..mitad {
                    let (wr, wi) = self.giros[k * salto];
                    let (i0, i1) = (base + k, base + k + mitad);
                    let tr = re[i1] * wr - im[i1] * wi;
                    let ti = re[i1] * wi + im[i1] * wr;
                    re[i1] = re[i0] - tr;
                    im[i1] = im[i0] - ti;
                    re[i0] += tr;
                    im[i0] += ti;
                }
            }
            tramo *= 2;
        }
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    /// un clic seco cada 0.5 s (120 BPM) tiene que salir clavado
    #[test]
    fn el_pulso_de_un_metronomo_sale_clavado() {
        let rate = 22050usize; // como llega de producción: ya diezmado
        let dur = 12.0f64;
        let mut mono = vec![0.0f32; (rate as f64 * dur) as usize];
        let paso = 0.5f64;
        let mut t = 0.25f64;
        while t < dur {
            let i0 = (t * rate as f64) as usize;
            for j in 0..220 {
                // un golpe percusivo: ruido que decae en 10 ms
                let x = j as f32 / 220.0;
                let ruido = ((j * 2654435761usize) % 65536) as f32 / 32768.0 - 1.0;
                if i0 + j < mono.len() {
                    mono[i0 + j] += ruido * (1.0 - x);
                }
            }
            t += paso;
        }
        let c = analiza_desde(&mono, rate).expect("tiene pulso");
        assert!((c.bpm - 120.0).abs() < 3.0, "bpm {}", c.bpm);
        // cada golpe detectado cae a menos de 45 ms de un clic de verdad
        let mut peor = 0.0f64;
        for &g in &c.golpes {
            let cerca = ((g - 0.25) / paso).round() * paso + 0.25;
            peor = peor.max((g - cerca).abs());
        }
        assert!(peor < 0.045, "peor desvío {peor}");
        assert!(c.golpes.len() >= 18, "solo {} golpes", c.golpes.len());
    }

    /// el ruido plano no tiene compás y hay que decirlo, no inventarlo
    #[test]
    fn al_ruido_no_se_le_inventa_un_compas() {
        let rate = 22050usize;
        // xorshift de verdad: una congruencia lineal sin estado tiene
        // estructura periódica y el detector la encontraba — con razón
        let mut x = 0x12345678u32;
        let mono: Vec<f32> = (0..rate * 8)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x as f32 / u32::MAX as f32) - 0.5
            })
            .collect();
        let c = analiza_desde(&mono, rate);
        assert!(c.is_none(), "inventó {} golpes a {:.1} BPM",
                c.as_ref().unwrap().golpes.len(), c.as_ref().unwrap().bpm);
    }

    /// contra un fichero de verdad: FL_RITMO_FICHERO=… cargo test -- --ignored
    #[test]
    #[ignore]
    fn una_cinta_de_verdad() {
        let ruta = std::env::var("FL_RITMO_FICHERO").expect("FL_RITMO_FICHERO");
        let t0 = std::time::Instant::now();
        match analiza(Path::new(&ruta)) {
            Some(c) => {
                let d = c.golpes.last().copied().unwrap_or(0.0);
                eprintln!("♩ {:.1} BPM · {} golpes en {:.0} s · analizado en {:.0} ms",
                          c.bpm, c.golpes.len(), d, t0.elapsed().as_secs_f64() * 1e3);
                eprintln!("  primeros: {:?}",
                          &c.golpes[..c.golpes.len().min(8)].iter()
                              .map(|g| (g * 100.0).round() / 100.0).collect::<Vec<_>>());
            }
            None => eprintln!("✗ sin pulso ({:.0} ms)", t0.elapsed().as_secs_f64() * 1e3),
        }
    }

    /// y un tempo que no es el del prior (84 BPM) también se encuentra
    #[test]
    fn un_tempo_lento_no_se_dobla() {
        let rate = 22050usize;
        let dur = 16.0f64;
        let mut mono = vec![0.0f32; (rate as f64 * dur) as usize];
        let paso = 60.0 / 84.0;
        let mut t = 0.3f64;
        while t < dur {
            let i0 = (t * rate as f64) as usize;
            for j in 0..220 {
                let x = j as f32 / 220.0;
                let ruido = ((j * 40503usize) % 65536) as f32 / 32768.0 - 1.0;
                if i0 + j < mono.len() {
                    mono[i0 + j] += ruido * (1.0 - x);
                }
            }
            t += paso;
        }
        let c = analiza_desde(&mono, rate).expect("tiene pulso");
        assert!((c.bpm - 84.0).abs() < 4.0, "bpm {}", c.bpm);
    }
}
