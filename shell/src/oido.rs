//! EL OÍDO: la transcripción, en casa y sin red.
//!
//! whisper.cpp enlazado estático (whisper-rs). En el Mac corre por **Metal**;
//! en Windows por CPU, que en el HX 370 son doce núcleos Zen 5 con AVX-512 y
//! ggml los usa todos — no es el camino pobre, es el que rinde ahí.
//!
//! El modelo vive en `<taller>/modelos/` y se baja UNA vez. Nada sale de la
//! máquina: ni el audio, ni el texto, ni el nombre del fichero.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// LOS MODELOS, del más ligero al mejor.
pub const MODELOS: [(&str, &str, u64); 3] = [
    ("ligero",
     "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin",
     190_000_000),
    ("el de casa",
     "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
     574_000_000),
    ("el mejor",
     "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-q5_0.bin",
     1_080_000_000),
];

/// EL MODELO DE ESTA MÁQUINA cuando nadie elige.
///
/// **Medido, no supuesto**: el turbo por Metal en el M4 Max va a 21× tiempo
/// real; el mismo modelo por CPU en el HX 370 tardó más de diez minutos en
/// 34 segundos de sonido, o sea 0,06×. En CPU el que sirve es el ligero.
/// Quien quiera el bueno en Windows puede pedirlo y esperar.
pub fn el_de_esta_maquina() -> usize {
    if cfg!(target_os = "macos") { 1 } else { 0 }
}

fn nombre_modelo(url: &str) -> &str {
    url.rsplit('/').next().unwrap_or("modelo.bin")
}

/// El fichero del modelo, bajándolo si hace falta. `curl` está en las dos
/// máquinas (Windows 10+ lo trae de serie) y sabe reanudar.
pub fn modelo(taller: &Path, cual: usize, aviso: &dyn Fn(&str)) -> Result<PathBuf, String> {
    let (nombre, url, tam) = MODELOS[cual.min(MODELOS.len() - 1)];
    let dir = taller.join("modelos");
    std::fs::create_dir_all(&dir).map_err(|e| format!("no pude crear modelos/: {e}"))?;
    let ruta = dir.join(nombre_modelo(url));
    // «ya está» = existe Y tiene un tamaño creíble (una descarga a medias no
    // vale: whisper.cpp casca con un modelo truncado)
    if ruta.is_file() {
        let n = std::fs::metadata(&ruta).map(|m| m.len()).unwrap_or(0);
        if n > tam / 2 { return Ok(ruta); }
        aviso(&format!("el modelo estaba a medias ({} MB): lo vuelvo a bajar", n / 1_000_000));
    }
    aviso(&format!("bajando el modelo «{nombre}» ({} MB) — sólo esta vez", tam / 1_000_000));
    // EL PARCIAL ES DE ESTE PROCESO Y DE NADIE MÁS. Con un nombre compartido,
    // dos revelados a la vez escriben el mismo fichero, la suma sale del
    // tamaño correcto… y whisper.cpp casca al abrir un modelo corrupto.
    // (Pasó: media hora buscando un SIGSEGV que era esto.)
    let tmp = ruta.with_extension(format!("parcial{}", std::process::id()));
    let salida = std::process::Command::new("curl")
        .args(["-L", "--fail", "--retry", "3", "-o"])
        .arg(&tmp)
        .arg(url)
        .status()
        .map_err(|e| format!("no pude lanzar curl: {e}"))?;
    if !salida.success() {
        return Err(format!("no pude bajar el modelo «{nombre}» (curl {salida})"));
    }
    // LA CARRERA: dos revelados a la vez se bajan el mismo modelo y el
    // segundo se encuentra el `.parcial` ya renombrado por el primero. Si el
    // fichero bueno está y mide lo que debe, no hay nada que arreglar.
    if let Err(e) = std::fs::rename(&tmp, &ruta) {
        let n = std::fs::metadata(&ruta).map(|m| m.len()).unwrap_or(0);
        if n < tam / 2 {
            return Err(format!("no pude dejar el modelo: {e}"));
        }
        aviso("el modelo ya lo había dejado otro: sigo con ése");
    }
    Ok(ruta)
}

/// UN TROZO DE HABLA con sus tiempos, en segundos de la bobina
pub struct Trozo {
    pub t0: f64,
    pub t1: f64,
    pub texto: String,
}

/// UN PLANO QUE ESCUCHAR: qué fichero, qué trozo, dónde cae en la bobina y a
/// qué velocidad va. Con esto el oído devuelve tiempos DE LA BOBINA, que es
/// lo que necesita el pie.
pub struct Trabajo {
    pub fichero: PathBuf,
    pub t_in: f64,
    pub t_out: f64,
    /// dónde empieza este plano en la bobina
    pub desde: f64,
    /// velocidad del clip (el tiempo de la bobina va más despacio si v < 1)
    pub velocidad: f64,
}

/// EL AUDIO, como lo quiere whisper: mono 16 kHz en f32. Lo saca ffmpeg, que
/// ya está en el camino de todo lo demás.
fn pcm16k(ffmpeg: &str, media: &Path) -> Result<Vec<f32>, String> {
    pcm16k_trozo(ffmpeg, media, 0.0, 0.0)
}

/// el mismo audio pero SOLO un trozo (`dur` = 0 → hasta el final)
fn pcm16k_trozo(ffmpeg: &str, media: &Path, desde: f64, dur: f64)
                -> Result<Vec<f32>, String> {
    let mut c = std::process::Command::new(ffmpeg);
    c.args(["-v", "error"]);
    if desde > 0.001 { c.args(["-ss", &format!("{desde:.3}")]); }
    c.arg("-i").arg(media);
    if dur > 0.001 { c.args(["-t", &format!("{dur:.3}")]); }
    let o = c
        .args(["-vn", "-ac", "1", "-ar", "16000", "-f", "f32le", "-"])
        .output()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    if !o.status.success() {
        return Err(format!("no pude sacar el audio de {}: {}",
                           media.display(), String::from_utf8_lossy(&o.stderr)));
    }
    if o.stdout.len() < 4 { return Err("ese fichero no trae sonido".into()); }
    Ok(o.stdout.chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect())
}

/// CUÁNTOS HILOS — **medido, no razonado**.
///
/// La teoría dice que ggml quiere núcleos FÍSICOS (con SMT los dos hermanos
/// se pelean por la misma unidad vectorial). Se probó en el HX 370, que tiene
/// doce núcleos y veinticuatro hilos, sobre los mismos 34 s de sonido:
///
///     16 hilos lógicos → 153,6 s        12 físicos → 191,5 s
///
/// O sea que la teoría se equivocaba AQUÍ: este chip mezcla cuatro Zen 5 con
/// ocho Zen 5c, y dejarle al sistema más hilos que colocar le sale mejor que
/// atarle las manos. Se queda lo que midió mejor. Si alguien vuelve a
/// «arreglarlo» dividiendo por dos, que lo mida antes.
fn cuantos_hilos() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(16)
}

/// CÓMO SE BUSCA LA FRASE, según quién trabaje.
///
/// Con **Metal** (el Mac) el haz de cinco sale casi gratis —21× tiempo real
/// medido— y acierta más justo donde molesta: nombres propios y finales de
/// frase. **En CPU** ese mismo haz multiplica el trabajo por cinco, así que
/// allí se busca con haz corto: sigue siendo mejor que el muestreo voraz y
/// no convierte un minuto de bobina en diez de espera.
fn como_buscar() -> whisper_rs::SamplingStrategy {
    let haz = if cfg!(target_os = "macos") { 5 } else { 2 };
    whisper_rs::SamplingStrategy::BeamSearch { beam_size: haz, patience: 1.0 }
}

/// TRANSCRIBIR. `idioma` vacío = que lo detecte él.
pub fn escucha(modelo: &Path, ffmpeg: &str, media: &Path, idioma: &str,
               aviso: &dyn Fn(&str)) -> Result<Vec<Trozo>, String> {
    use whisper_rs::{FullParams, WhisperContext, WhisperContextParameters};
    let pcm = pcm16k(ffmpeg, media)?;
    let segundos = pcm.len() as f64 / 16000.0;
    aviso(&format!("{:.0} s de sonido · modelo {}", segundos,
                   modelo.file_name().unwrap_or_default().to_string_lossy()));

    whisper_rs::install_logging_hooks();
    let ctx = WhisperContext::new_with_params(
        modelo, WhisperContextParameters::default())
        .map_err(|e| format!("no pude abrir el modelo: {e}"))?;
    let mut est = ctx.create_state().map_err(|e| format!("whisper: {e}"))?;

    // BEAM SEARCH y no muestreo voraz: en habla real la diferencia se nota
    // justo donde molesta (nombres propios, finales de frase). Cuesta ~30 %
    // más de tiempo y lo vale para un subtítulo, que se lee entero.
    let mut p = FullParams::new(como_buscar());
    if !idioma.is_empty() { p.set_language(Some(idioma)); }
    p.set_translate(false);
    p.set_print_special(false);
    p.set_print_progress(false);
    p.set_print_realtime(false);
    p.set_print_timestamps(false);
    let hilos = cuantos_hilos();
    p.set_n_threads(hilos as i32);
    // que no se invente frases en los silencios (la plaga de whisper)
    p.set_no_speech_thold(0.6);
    p.set_suppress_blank(true);

    let t0 = std::time::Instant::now();
    est.full(p, &pcm).map_err(|e| format!("transcribiendo: {e}"))?;
    let mut trozos = Vec::new();
    for seg in est.as_iter() {
        let texto = seg.to_str_lossy().unwrap_or_default().to_string();
        let t = texto.trim();
        // los corchetes son los ruidos que se inventa («[Música]»)
        if t.is_empty() || t.starts_with('[') { continue; }
        let a = seg.start_timestamp() as f64 / 100.0;
        let b = seg.end_timestamp() as f64 / 100.0;
        trozos.push(Trozo { t0: a, t1: b.max(a + 0.2), texto: t.to_string() });
    }
    let el = t0.elapsed().as_secs_f64();
    aviso(&format!("{} trozo(s) en {:.1} s · {:.1}× tiempo real · {hilos} hilos",
                   trozos.len(), el, segundos / el.max(0.001)));
    Ok(trozos)
}

/// ESCUCHAR LA BOBINA ENTERA: todos los planos con una sola carga del modelo
/// (que es lo caro) y los tiempos ya puestos en la línea de tiempo.
pub fn escucha_bobina(modelo: &Path, ffmpeg: &str, trabajos: &[Trabajo], idioma: &str,
                      aviso: &dyn Fn(&str)) -> Result<Vec<Trozo>, String> {
    use whisper_rs::{FullParams, WhisperContext, WhisperContextParameters};
    whisper_rs::install_logging_hooks();
    let ctx = WhisperContext::new_with_params(
        modelo, WhisperContextParameters::default())
        .map_err(|e| format!("no pude abrir el modelo: {e}"))?;
    let hilos = cuantos_hilos();
    let mut todos: Vec<Trozo> = Vec::new();
    let t00 = std::time::Instant::now();
    let mut total_s = 0.0f64;
    for (k, t) in trabajos.iter().enumerate() {
        let dur = (t.t_out - t.t_in).max(0.0);
        if dur < 0.15 { continue; }
        aviso(&format!("el oído: plano {} de {} ({:.0} s)", k + 1, trabajos.len(), dur));
        let pcm = match pcm16k_trozo(ffmpeg, &t.fichero, t.t_in, dur) {
            Ok(p) => p,
            Err(e) => { aviso(&format!("   ⚠ {e}")); continue; }
        };
        if pcm.len() < 16000 / 4 { continue; }
        total_s += pcm.len() as f64 / 16000.0;
        let mut est = ctx.create_state().map_err(|e| format!("whisper: {e}"))?;
        let mut p = FullParams::new(como_buscar());
        if !idioma.is_empty() { p.set_language(Some(idioma)); }
        p.set_translate(false);
        p.set_print_special(false);
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_timestamps(false);
        p.set_n_threads(hilos as i32);
        p.set_no_speech_thold(0.6);
        p.set_suppress_blank(true);
        est.full(p, &pcm).map_err(|e| format!("transcribiendo: {e}"))?;
        // EL TIEMPO DE LA FUENTE AL DE LA BOBINA: un plano a media velocidad
        // dura el doble, así que lo que se oye en su segundo 3 cae en el 6
        let v = t.velocidad.abs().max(0.02);
        for seg in est.as_iter() {
            let texto = seg.to_str_lossy().unwrap_or_default().to_string();
            let x = texto.trim();
            if x.is_empty() || x.starts_with('[') { continue; }
            let a = t.desde + seg.start_timestamp() as f64 / 100.0 / v;
            let b = t.desde + seg.end_timestamp() as f64 / 100.0 / v;
            todos.push(Trozo { t0: a, t1: b.max(a + 0.25), texto: x.to_string() });
        }
    }
    todos.sort_by(|a, b| a.t0.partial_cmp(&b.t0).unwrap_or(std::cmp::Ordering::Equal));
    let el = t00.elapsed().as_secs_f64();
    aviso(&format!("{} trozo(s) en {:.1} s · {:.1}× tiempo real · {hilos} hilos",
                   todos.len(), el, total_s / el.max(0.001)));
    Ok(todos)
}

/// EL .srt, que es el formato que entiende todo el mundo (y el que lee la app)
pub fn srt(trozos: &[Trozo]) -> String {
    let reloj = |t: f64| {
        let t = t.max(0.0);
        let (h, m, s) = ((t / 3600.0) as u64, ((t % 3600.0) / 60.0) as u64, t % 60.0);
        format!("{h:02}:{m:02}:{:02},{:03}", s as u64, ((s - s.floor()) * 1000.0) as u64)
    };
    let mut o = String::new();
    for (i, t) in trozos.iter().enumerate() {
        let _ = writeln!(o, "{}", i + 1);
        let _ = writeln!(o, "{} --> {}", reloj(t.t0), reloj(t.t1));
        let _ = writeln!(o, "{}\n", t.texto);
    }
    o
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn el_srt_sale_con_el_reloj_bien() {
        let v = vec![Trozo { t0: 1.5, t1: 3.25, texto: "hola".into() },
                     Trozo { t0: 61.0, t1: 62.5, texto: "adiós".into() }];
        let s = srt(&v);
        assert!(s.contains("00:00:01,500 --> 00:00:03,250"), "{s}");
        assert!(s.contains("00:01:01,000 --> 00:01:02,500"), "{s}");
        assert!(s.contains("hola") && s.contains("adiós"));
    }
}
