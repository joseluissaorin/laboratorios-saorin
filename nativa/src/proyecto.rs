//! El proyecto: se lee del MISMO sitio que el resto del taller (~/filmlab),
//! así la app nativa y la herramienta de línea de comandos comparten bobina.

use anyhow::Result;
use std::path::{Path, PathBuf};

pub use filmlook_core::plan::{Encaje, Encuadre};

/// cuántas pistas de música puede apilar la bobina (§2)
pub const PISTAS_MUSICA: usize = 3;

/// UNA MARCA DE LA BOBINA. Era un número suelto y nada más; una marca sirve
/// para acordarse de algo, así que lleva su nota y su color (§4bis.1).
#[derive(Clone, PartialEq, Debug)]
pub struct Marca {
    pub t: f64,
    pub nota: String,
    /// 0..3 — el mismo juego de chinchetas que la washi
    pub color: u8,
}

impl Marca {
    pub fn nueva(t: f64) -> Marca { Marca { t, nota: String::new(), color: 0 } }
}

#[derive(Clone)]
pub struct Clip {
    pub media: String,
    pub ruta: PathBuf,
    pub t_in: f64,
    pub t_out: f64,
    pub hueco: bool,
    /// fundido en la CABEZA del clip (desde el anterior o desde negro), en s
    pub fade: f64,
    /// velocidad de reproducción. 1 = normal · negativa = MARCHA ATRÁS ·
    /// 0 = CONGELADO (el mismo fotograma durante todo el clip, §4bis.3)
    pub speed: f64,
    /// EL ENCUADRE del clip, en el único modelo que hay (§1.5)
    pub enc: Encuadre,
    /// la orientación que trae el FICHERO (cuartos de vuelta del contenedor):
    /// es de donde sale `enc.cuartos` al importar, y adonde vuelve el
    /// «encuadre a cero»
    pub cuartos_fichero: u8,
    /// el cuarto oscuro DE ESTE CLIP (los ajustes del proyecto con las
    /// diferencias propias encima) y sus gelatinas
    pub prefs: serde_json::Value,
    pub lut_in: Option<PathBuf>,
    pub lut_color: Option<PathBuf>,
    /// silenciar el sonido del vídeo de ESTE clip (la palanca de la ficha)
    pub mute: bool,
    /// DESPLAZAR EL SONIDO respecto a su vídeo, en segundos (sincronía fina,
    /// §4bis.11). Positivo = el sonido llega más tarde.
    pub desfase: f64,
    /// la cinta washi de color (0..3) que organiza de un vistazo
    pub washi: Option<u8>,
    /// la nota manuscrita pegada al clip
    pub nota: String,
    /// la grapa: los clips con el mismo número van juntos (NORTE §3.5)
    pub grupo: Option<u32>,
    /// el fichero no aparece: el clip se conserva y se avisa (§4)
    pub ausente: bool,
    /// UNA BOBINA DENTRO DE OTRA (CAPAS §2): la clave de la hija. El clip es
    /// una VENTANA sobre ella: `t_in`/`t_out` recortan en su línea de tiempo,
    /// y por dentro la hija sigue siendo suya (editarla y volver refresca).
    pub anidada: Option<String>,
}

impl Clip {
    /// lo que dura EN LA BOBINA
    pub fn dur(&self) -> f64 {
        let v = self.speed.abs();
        // congelado: el tramo dura lo que se le haya estirado, no lo que dure
        // el trozo de fuente (que es un solo fotograma)
        if v < 0.02 { return (self.t_out - self.t_in).max(0.04); }
        ((self.t_out - self.t_in) / v).max(0.04)
    }

    /// ¿qué segundo de la FUENTE toca a los `d` segundos de haber entrado?
    pub fn fuente_en(&self, d: f64) -> f64 {
        let v = self.speed.abs();
        if v < 0.02 { return self.t_in; }
        if self.speed < 0.0 { (self.t_out - d.max(0.0) * v).max(self.t_in) }
        else { self.t_in + d.max(0.0) * v }
    }

    pub fn congelado(&self) -> bool { self.speed.abs() < 0.02 }
}

/// un clip de la pista de MÚSICA (audio[] del webview)
#[derive(Clone)]
pub struct ClipAudio {
    pub media: String,
    pub ruta: PathBuf,
    pub t_in: f64,
    pub t_out: f64,
    /// dónde empieza en la BOBINA
    pub start: f64,
    pub gain: f64,
    pub fade_in: f64,
    pub fade_out: f64,
    /// banda elástica: puntos (t de FUENTE, ganancia dB) ordenados
    pub banda: Vec<(f64, f64)>,
    /// esta pista, callada (sin tocar las demás)
    pub mute: bool,
    /// EN QUÉ CARRIL va (0..PISTAS_MUSICA-1). Antes todas compartían carril y
    /// con dos canciones se solapaban visualmente (§2).
    pub pista: u8,
    /// desplazamiento fino, en segundos (se suma a `start`)
    pub desfase: f64,
}

impl ClipAudio {
    pub fn dur(&self) -> f64 { (self.t_out - self.t_in).max(0.05) }
    /// dónde empieza de verdad en la bobina (con el desfase puesto)
    pub fn entra(&self) -> f64 { (self.start + self.desfase).max(0.0) }
}

/// UNA CAPA (CAPAS §2): un clip COLOCADO encima de la bobina, no un eslabón
/// de ella. Reutiliza `Clip` entero —encuadre, receta, gelatinas, velocidad,
/// silencio— y añade lo que una capa necesita: dónde entra y sus fundidos.
#[derive(Clone)]
pub struct Capa {
    pub c: Clip,
    /// en qué segundo de la bobina entra
    pub start: f64,
    /// fundidos de ALFA de la propia capa (no a negro: a transparente)
    pub fundido_in: f64,
    pub fundido_out: f64,
}

impl Capa {
    pub fn dur(&self) -> f64 { self.c.dur() }
    pub fn fin(&self) -> f64 { self.start + self.dur() }
    /// el alfa de la capa en el segundo `t` de la bobina (0 = no se ve)
    pub fn alfa_en(&self, t: f64) -> f32 {
        if t < self.start || t >= self.fin() { return 0.0 }
        let dentro = t - self.start;
        let mut a = 1.0f64;
        if self.fundido_in > 0.001 { a = a.min(dentro / self.fundido_in); }
        if self.fundido_out > 0.001 { a = a.min((self.dur() - dentro) / self.fundido_out); }
        a.clamp(0.0, 1.0) as f32
    }
}

/// UNA BOBINA HIJA ya cargada (CAPAS §2): lo que hace falta para resolver la
/// preview y para aplanar el payload
pub struct SubBobina {
    pub clips: Vec<Clip>,
    pub capas: Vec<Capa>,
    pub audio: Vec<ClipAudio>,
    pub w: u32,
    pub h: u32,
    pub fps: f64,
    pub dur: f64,
}

/// el FORMATO del proyecto: la decisión creativa de primer orden.
/// None = «del primer clip» (auto).
#[derive(Clone, Debug, PartialEq)]
pub struct Formato {
    pub w: u32,
    pub h: u32,
    /// etiqueta del preset («16:9», «9:16», «1:1», «4:5», «2.39:1», «4:3»)
    pub aspecto: String,
}

/// presets POR DESTINO, no por número (HERRAMIENTA §2)
pub const FORMATOS: [(&str, &str, u32, u32); 6] = [
    ("16:9", "apaisado · YouTube", 1920, 1080),
    ("9:16", "vertical · Reels", 1080, 1920),
    ("1:1", "cuadrado", 1080, 1080),
    ("4:5", "retrato · feed", 1080, 1350),
    ("2.39:1", "cine", 1920, 804),
    ("4:3", "clásico", 1440, 1080),
];

pub const FPS_OPCIONES: [f64; 6] = [0.0, 24.0, 25.0, 29.97, 30.0, 60.0]; // 0 = del clip

pub struct Proyecto {
    pub base: PathBuf,
    pub nombre: String,
    pub clips: Vec<Clip>,
    pub prefs: serde_json::Value,
    pub lut_in: Option<PathBuf>,
    pub lut_color: Option<PathBuf>,
    pub fps: f64,
    pub formato: Option<Formato>,
    /// marcas persistentes de la bobina
    pub marcas: Vec<Marca>,
    /// la pista de música (bajo el vídeo)
    pub audio: Vec<ClipAudio>,
    /// LA CAPA de vídeo (encima): clips colocados con su alfa (CAPAS §2). El
    /// orden de la lista es el apilado: la última, encima.
    pub capas: Vec<Capa>,
    /// las bobinas hijas de los clips anidados, cargadas al abrir
    pub subbobinas: std::collections::HashMap<String, SubBobina>,
    /// las palancas del margen: silenciar el sonido del vídeo / la música
    pub mudo_voz: bool,
    pub mudo_musica: bool,
    /// EL RANGO de la bobina (entrada/salida). El JSON ya traía el campo y no
    /// lo leía ni lo escribía nadie; con él vienen el bucle, revelar solo el
    /// tramo marcado y exportar un trozo (§4bis.2).
    pub rango: Option<(f64, f64)>,
    /// LOS NIVELES del margen, en dB: el sonido del vídeo y la música (§1.6)
    pub vol_voz: f64,
    pub vol_musica: f64,
}

fn casa() -> PathBuf {
    std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."))
}

/// DÓNDE ESTÁ DE VERDAD ESTE MATERIAL.
///
/// Se busca en tres sitios y en este orden: la carpeta `media/` del taller,
/// el registro DE ESTA BOBINA y el registro global.
///
/// Los dos últimos son la corrección de un fallo que borraba trabajo: el
/// material se importa por referencia y se apunta en `registros/<bobina>.json`,
/// pero aquí solo se miraba `media.json`. Resultado: un clip cuyo fichero solo
/// estaba en el registro de su bobina se veía en la estantería, se editaba y
/// se reproducía sin problema… y **desaparecía al reabrir el proyecto**,
/// porque su ruta no se resolvía. Y el revelado fallaba por lo mismo.
fn resolver(base: &Path, nombre: &str) -> PathBuf {
    let local = base.join("media").join(nombre);
    if local.is_file() { return local; }
    let mira = |p: PathBuf| -> Option<PathBuf> {
        let b = std::fs::read(p).ok()?;
        let v: serde_json::Value = serde_json::from_slice(&b).ok()?;
        Some(PathBuf::from(v.get(nombre)?.as_str()?))
    };
    // el registro de la bobina abierta
    let actual = std::fs::read_to_string(base.join("current.txt")).unwrap_or_default();
    let actual = actual.trim();
    if !actual.is_empty() {
        if let Some(p) = mira(base.join("registros").join(format!("{actual}.json"))) {
            return p;
        }
    }
    // y el del taller entero
    if let Some(p) = mira(base.join("media.json")) { return p; }
    local
}

/// encuentra un .cube por nombre siendo tolerante: macOS guarda los acentos
/// descompuestos (NFD) y el JSON los trae compuestos (NFC), así que la ruta
/// exacta falla aunque el fichero esté ahí
fn busca_lut(dir: &Path, nombre: &str) -> Option<PathBuf> {
    let exacta = dir.join(nombre);
    if exacta.is_file() { return Some(exacta); }
    let clave = |s: &str| -> String {
        s.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect()
    };
    let k = clave(nombre);
    let mut solo: Option<PathBuf> = None;
    let mut n = 0;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let f = e.file_name().to_string_lossy().to_string();
        if !f.to_lowercase().ends_with(".cube") { continue; }
        n += 1;
        solo = Some(e.path());
        if clave(&f) == k { return Some(e.path()); }
    }
    // si solo hay una gelatina en el cajón, esa es
    if n == 1 { solo } else { None }
}

#[derive(Clone)]
pub struct Cinta {
    pub nombre: String,
    pub ruta: PathBuf,
    pub dur: f64,
    pub w: u32,
    pub h: u32,
    pub fps: f64,
    /// a qué BALDA pertenece (None = el material suelto del taller)
    pub balda: Option<String>,
}

/// una bobina de la portada (tarjeta de proyecto reciente)
pub struct BobinaInfo {
    pub nombre: String,
    /// "" = la bobina clásica (project.json); si no, el nombre en projects/
    pub clave: String,
    pub clips: usize,
    pub dur: f64,
    pub formato: String,
    /// primera cinta (para la miniatura de la tarjeta)
    pub primera: Option<(String, PathBuf)>,
    pub modificada: Option<std::time::SystemTime>,
}

fn resumen_bobina(base: &Path, clave: &str, ruta: &Path) -> Option<BobinaInfo> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(ruta).ok()?).ok()?;
    let clips = v["clips"].as_array().cloned().unwrap_or_default();
    let mut dur = 0.0;
    let mut primera = None;
    for c in &clips {
        if c["gap"].as_bool().unwrap_or(false) {
            dur += c["dur"].as_f64().unwrap_or(0.0);
            continue;
        }
        dur += (c["out"].as_f64().unwrap_or(0.0) - c["in"].as_f64().unwrap_or(0.0)).max(0.0);
        if primera.is_none() {
            if let Some(m) = c["media"].as_str() {
                let r = resolver(base, m);
                if r.is_file() { primera = Some((m.to_string(), r)); }
            }
        }
    }
    let formato = v["project"]["aspect"].as_str()
        .filter(|a| *a != "auto").unwrap_or("auto").to_string();
    let nombre = if clave.is_empty() { "bobina clásica".to_string() } else { clave.to_string() };
    let modificada = std::fs::metadata(ruta).ok().and_then(|m| m.modified().ok());
    Some(BobinaInfo { nombre, clave: clave.to_string(), clips: clips.len(), dur, formato, primera, modificada })
}

/// todas las bobinas del taller, la más reciente primero
pub fn bobinas(base: &Path) -> Vec<BobinaInfo> {
    let mut v = Vec::new();
    let clasica = base.join("project.json");
    if clasica.is_file() {
        if let Some(b) = resumen_bobina(base, "", &clasica) { v.push(b); }
    }
    if let Ok(rd) = std::fs::read_dir(base.join("projects")) {
        for e in rd.flatten() {
            let p = e.path();
            let nombre = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if p.extension().map(|x| x == "json").unwrap_or(false) && !nombre.starts_with('.')
                && !nombre.ends_with(".media") {
                if let Some(b) = resumen_bobina(base, &nombre, &p) { v.push(b); }
            }
        }
    }
    v.sort_by(|a, b| b.modificada.cmp(&a.modificada));
    v
}

/// activa una bobina (escribe current.txt); "" = la clásica
pub fn activa(base: &Path, clave: &str) -> anyhow::Result<()> {
    std::fs::write(base.join("current.txt"), clave)?;
    Ok(())
}

/// el nombre de una bobina, limpio de lo que no puede ir en un fichero
fn limpia_nombre(n: &str) -> String {
    n.trim().chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':' | '"' | '<' | '>' | '|' | '?' | '*'))
        .collect()
}

// ── LAS BOBINAS SE MANEJAN DESDE LA PORTADA (§4bis.8) ────────────────────
// Renombrar, duplicar (para probar dos montajes del mismo material) y borrar.
// Antes había que ir al disco a mano, que es justo lo que un taller no debería
// pedirte. La estantería (el registro de material) viaja con la bobina.

/// renombra una bobina y su registro de material
pub fn renombra_bobina(base: &Path, clave: &str, nuevo: &str) -> anyhow::Result<String> {
    anyhow::ensure!(!clave.is_empty(), "la bobina clásica no se renombra");
    let nuevo = limpia_nombre(nuevo);
    anyhow::ensure!(!nuevo.is_empty(), "sin nombre");
    if nuevo == clave { return Ok(nuevo); }
    let dir = base.join("projects");
    let destino = dir.join(format!("{nuevo}.json"));
    anyhow::ensure!(!destino.exists(), "ya existe una bobina con ese nombre");
    std::fs::rename(dir.join(format!("{clave}.json")), &destino)?;
    let reg = base.join("registros");
    let _ = std::fs::rename(reg.join(format!("{clave}.json")), reg.join(format!("{nuevo}.json")));
    // si era la abierta, sigue siéndolo con su nombre nuevo
    let actual = std::fs::read_to_string(base.join("current.txt")).unwrap_or_default();
    if actual.trim() == clave { activa(base, &nuevo)?; }
    Ok(nuevo)
}

/// duplica una bobina (con su estantería) y devuelve la clave de la copia
pub fn duplica_bobina(base: &Path, clave: &str) -> anyhow::Result<String> {
    let origen = if clave.is_empty() { base.join("project.json") }
                 else { base.join("projects").join(format!("{clave}.json")) };
    anyhow::ensure!(origen.is_file(), "esa bobina no está en el disco");
    let raiz = if clave.is_empty() { "bobina clásica" } else { clave };
    let dir = base.join("projects");
    std::fs::create_dir_all(&dir)?;
    let mut nombre = format!("{raiz} (copia)");
    let mut k = 2;
    while dir.join(format!("{nombre}.json")).exists() {
        nombre = format!("{raiz} (copia {k})");
        k += 1;
    }
    std::fs::copy(&origen, dir.join(format!("{nombre}.json")))?;
    let reg = base.join("registros");
    std::fs::create_dir_all(&reg)?;
    let reg_origen = if clave.is_empty() { base.join("media.json") }
                     else { reg.join(format!("{clave}.json")) };
    if reg_origen.is_file() {
        let _ = std::fs::copy(&reg_origen, reg.join(format!("{nombre}.json")));
    } else {
        let _ = std::fs::write(reg.join(format!("{nombre}.json")), b"{}");
    }
    Ok(nombre)
}

/// borra una bobina. El MATERIAL no se toca jamás: solo el montaje, y con una
/// copia en `backups/` por si acaso.
pub fn borra_bobina(base: &Path, clave: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!clave.is_empty(), "la bobina clásica no se borra");
    let ruta = base.join("projects").join(format!("{clave}.json"));
    anyhow::ensure!(ruta.is_file(), "esa bobina no está en el disco");
    let copias = base.join("backups");
    let _ = std::fs::create_dir_all(&copias);
    let sello = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs()).unwrap_or(0);
    let _ = std::fs::copy(&ruta, copias.join(format!("{}-borrada-{sello}.json",
                                                     clave.replace(' ', "_"))));
    std::fs::remove_file(&ruta)?;
    let _ = std::fs::remove_file(base.join("registros").join(format!("{clave}.json")));
    let actual = std::fs::read_to_string(base.join("current.txt")).unwrap_or_default();
    if actual.trim() == clave { activa(base, "")?; }
    Ok(())
}

/// renombra una cinta (la CLAVE lógica de media.json y sus usos en la
/// bobina) — el fichero en disco NO se toca jamás
pub fn renombra_cinta(base: &Path, pr: &mut Proyecto, viejo: &str, nuevo: &str) -> bool {
    let nuevo = nuevo.trim();
    if nuevo.is_empty() || nuevo == viejo { return false; }
    let mj = base.join("media.json");
    let mut m: serde_json::Map<String, serde_json::Value> = std::fs::read(&mj).ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    // solo renombrables las referenciadas (las de media/ llevan su nombre físico)
    let Some(v) = m.remove(viejo) else { return false };
    if m.contains_key(nuevo) { m.insert(viejo.to_string(), v); return false; }
    m.insert(nuevo.to_string(), v);
    let _ = std::fs::write(&mj, serde_json::to_vec(&serde_json::Value::Object(m)).unwrap_or_default());
    for c in &mut pr.clips { if c.media == viejo { c.media = nuevo.to_string(); } }
    for a in &mut pr.audio { if a.media == viejo { a.media = nuevo.to_string(); } }
    let _ = pr.guarda();
    true
}

/// quita una cinta del REGISTRO (media.json); el fichero queda intacto
pub fn quita_cinta(base: &Path, nombre: &str) -> bool {
    let mj = base.join("media.json");
    let mut m: serde_json::Map<String, serde_json::Value> = std::fs::read(&mj).ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    if m.remove(nombre).is_none() { return false; }
    let _ = std::fs::write(&mj, serde_json::to_vec(&serde_json::Value::Object(m)).unwrap_or_default());
    true
}

const EXT_VIDEO: [&str; 12] = ["mp4", "mov", "m4v", "mkv", "webm", "jpg", "jpeg", "png", "wav", "mp3", "flac", "m4a"];

/// importa ficheros POR REFERENCIA a media.json (cero copias — regla de la
/// casa). Carpetas → recursivo. Devuelve (importados, saltados).
/// una CARPETA arrastrada = una BALDA enchufada a ella (NORTE §2bis.2).
/// Volver a llamarla con la misma carpeta = «volver a mirar» (rescan).
pub fn importa_carpeta(base: &Path, mj: &Path, dir: &Path) -> (String, usize) {
    importa_carpeta_como(base, mj, dir, None)
}

/// como importa_carpeta pero respetando un nombre de balda ya existente
/// (el «volver a mirar» no debe rebautizar la balda)
pub fn importa_carpeta_como(base: &Path, mj: &Path, dir: &Path,
                            nombre: Option<&str>) -> (String, usize) {
    let nombre_balda = nombre.map(String::from)
        .or_else(|| dir.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "balda".into());
    let claves = |mj: &Path| -> std::collections::HashSet<String> {
        std::fs::read(mj).ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|j| j.as_object().map(|o| {
                o.iter().filter(|(_, v)| v.is_string()).map(|(k, _)| k.clone()).collect()
            })).unwrap_or_default()
    };
    let mj = mj.to_path_buf();
    let antes = claves(&mj);
    let (n, _) = importa_en(base, &mj, &[dir.to_path_buf()]);
    let despues = claves(&mj);
    let nuevas: Vec<String> = despues.difference(&antes).cloned().collect();
    // registrar la balda (mezclando con lo que ya tuviera)
    let mut j: serde_json::Value = std::fs::read(&mj).ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(serde_json::json!({}));
    let entrada = &mut j["_baldas"][&nombre_balda];
    let mut lista: Vec<String> = entrada["cintas"].as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();
    for nva in &nuevas {
        if !lista.contains(nva) { lista.push(nva.clone()); }
    }
    lista.sort();
    *entrada = serde_json::json!({
        "carpeta": dir.to_string_lossy(),
        "cintas": lista,
    });
    let _ = std::fs::write(&mj, serde_json::to_vec_pretty(&j).unwrap_or_default());
    (nombre_balda, n)
}

pub fn importa(base: &Path, rutas: &[PathBuf]) -> (usize, usize) {
    importa_en(base, &base.join("media.json"), rutas)
}

/// importa al registro que se le diga (el de la bobina activa)
pub fn importa_en(base: &Path, mj: &Path, rutas: &[PathBuf]) -> (usize, usize) {
    let mj = mj.to_path_buf();
    let mut m: serde_json::Map<String, serde_json::Value> = std::fs::read(&mj).ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let (mut hechos, mut saltados) = (0usize, 0usize);
    let mut cola: Vec<PathBuf> = rutas.to_vec();
    while let Some(r) = cola.pop() {
        if r.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&r) {
                for e in rd.flatten() { cola.push(e.path()); }
            }
            continue;
        }
        let ext = r.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if !EXT_VIDEO.contains(&ext.as_str()) { saltados += 1; continue; }
        let Some(nombre0) = r.file_name().map(|n| n.to_string_lossy().to_string()) else { continue };
        let ruta_abs = r.to_string_lossy().to_string();
        // ya registrada con la misma ruta → nada; mismo nombre y otra ruta → sufijo
        if m.get(&nombre0).and_then(|v| v.as_str()) == Some(ruta_abs.as_str())
            || base.join("media").join(&nombre0).is_file() && base.join("media").join(&nombre0) == r {
            saltados += 1;
            continue;
        }
        let mut nombre = nombre0.clone();
        let mut k = 2;
        while m.contains_key(&nombre) || base.join("media").join(&nombre).is_file() {
            if m.get(&nombre).and_then(|v| v.as_str()) == Some(ruta_abs.as_str()) { break; }
            let (tallo, ext) = nombre0.rsplit_once('.').unwrap_or((nombre0.as_str(), ""));
            nombre = format!("{tallo} ({k}).{ext}");
            k += 1;
        }
        if m.get(&nombre).and_then(|v| v.as_str()) == Some(ruta_abs.as_str()) {
            saltados += 1;
            continue;
        }
        m.insert(nombre, serde_json::Value::String(ruta_abs));
        hechos += 1;
    }
    if hechos > 0 {
        let _ = std::fs::write(&mj, serde_json::to_vec(&serde_json::Value::Object(m)).unwrap_or_default());
    }
    (hechos, saltados)
}

/// crea una bobina nueva heredando el cuarto oscuro del proyecto actual
/// las RESOLUCIONES que ofrece el taller (altura del lienzo; el ancho sale
/// del aspecto elegido). 0 = «del primer clip».
pub const ALTURAS: [(u32, &str); 5] = [
    (0, "del clip"),
    (720, "720p"),
    (1080, "1080p"),
    (1440, "1440p"),
    (2160, "4K"),
];

pub fn crea_bobina(base: &Path, nombre: &str, aspecto: &str, fps: f64, alto: u32,
                   prefs: &serde_json::Value, lut_in: &str, lut_color: &str) -> anyhow::Result<()> {
    let nombre = limpia_nombre(nombre);
    anyhow::ensure!(!nombre.is_empty(), "sin nombre");
    let dir = base.join("projects");
    std::fs::create_dir_all(&dir)?;
    let ruta = dir.join(format!("{nombre}.json"));
    anyhow::ensure!(!ruta.exists(), "ya existe una bobina con ese nombre");
    // la resolución elegida manda: el aspecto da la PROPORCIÓN y la altura
    // el tamaño (el ancho se redondea a par, que los códecs lo exigen)
    let (w, h) = match FORMATOS.iter().find(|f| f.0 == aspecto) {
        Some(f) if alto > 0 => {
            let prop = f.2 as f64 / f.3 as f64;
            let w = ((alto as f64 * prop / 2.0).round() * 2.0) as u32;
            (w, alto)
        }
        Some(f) => (f.2, f.3),
        None => (0, 0),
    };
    // EL CAMPO `bin` NO ESTÁ AQUÍ a propósito. Se guardaba y no lo leía
    // nadie, y un campo que no hace nada es una promesa incumplida en el
    // formato (§4bis.9). Lo que hacía falta —una bandeja donde apartar
    // trozos— ya existe y es el cubo de recortes.
    let v = serde_json::json!({
        "v": 1,
        "clips": [],
        "audio": [],
        "prefs": prefs,
        "lutEntrada": lut_in,
        "lutColor": lut_color,
        "project": { "aspect": if aspecto.is_empty() { "auto" } else { aspecto },
                     "fps": fps, "w": w, "h": h },
        "markers": [],
        "range": null,
        "nextId": 1,
    });
    std::fs::write(&ruta, serde_json::to_vec(&v)?)?;

    // LA ESTANTERÍA NACE VACÍA. Antes heredaba el registro del taller entero
    // y una bobina recién cortada aparecía con el material de todas las
    // demás — lo que uno espera de un proyecto nuevo es una mesa limpia.
    //
    // Es el mismo lío que hacía desaparecer clips al reabrir: `media.json`
    // hacía de tres cosas a la vez (registro de la bobina clásica, semilla de
    // las nuevas y sitio donde resolver rutas). Ahora la regla es una:
    //
    //   · `registros/<bobina>.json` es la ESTANTERÍA de esa bobina, y lo
    //     único que se enseña en la mesa;
    //   · `media.json` es el registro de la bobina clásica, y además el
    //     ÚLTIMO sitio donde se busca una ruta que no aparezca en el de la
    //     bobina (para que lo viejo siga abriendo).
    let dir_reg = base.join("registros");
    std::fs::create_dir_all(&dir_reg)?;
    std::fs::write(dir_reg.join(format!("{nombre}.json")), b"{}")?;

    activa(base, &nombre)?;
    Ok(())
}

/// EL PAYLOAD de una bobina hija (CAPAS §8): lo que el aplanador del plan
/// necesita para sustituir el clip anidado por los clips reales. Los nombres
/// de gelatina van como ficheros (el shell los resuelve con su catálogo).
pub fn payload_de_sub(sb: &SubBobina) -> serde_json::Value {
    let nombre_lut = |p: &Option<PathBuf>| -> serde_json::Value {
        match p.as_ref().and_then(|x| x.file_name()) {
            Some(n) => serde_json::json!(n.to_string_lossy()),
            None => serde_json::Value::Null,
        }
    };
    let clip_json = |c: &Clip| -> serde_json::Value {
        let mut o = serde_json::json!({
            "file": c.ruta.to_string_lossy(),
            "in": c.t_in, "out": c.t_out,
            "fade": c.fade, "speed": c.speed,
            "cuartos": c.cuartos_fichero,
            "prefs": c.prefs.clone(),
            "mute": c.mute,
        });
        if c.hueco { o["gap"] = serde_json::json!(true); }
        if !c.enc.es_limpio(c.cuartos_fichero) { o["tf"] = c.enc.json(); }
        o["lut_in"] = nombre_lut(&c.lut_in);
        o["lut"] = nombre_lut(&c.lut_color);
        if let Some(a) = &c.anidada { o["anidada"] = serde_json::json!(a); }
        o
    };
    serde_json::json!({
        "project": {"w": sb.w, "h": sb.h, "fps": sb.fps},
        "clips": sb.clips.iter().map(clip_json).collect::<Vec<_>>(),
        "clips2": sb.capas.iter().map(|cp| {
            let mut o = clip_json(&cp.c);
            o["start"] = serde_json::json!(cp.start);
            if cp.fundido_in > 0.001 { o["fadeIn"] = serde_json::json!(cp.fundido_in); }
            if cp.fundido_out > 0.001 { o["fadeOut"] = serde_json::json!(cp.fundido_out); }
            o
        }).collect::<Vec<_>>(),
        "audio": sb.audio.iter().map(|a| serde_json::json!({
            "file": a.ruta.to_string_lossy(), "in": a.t_in, "out": a.t_out,
            "start": a.entra(), "gain": a.gain,
            "fadeIn": a.fade_in, "fadeOut": a.fade_out,
        })).collect::<Vec<_>>(),
    })
}

/// UNA BOBINA HIJA, cargada con el mismo lector que la madre. Un nivel para
/// la preview; el máster aplana hasta tres (plan::aplana_anidadas).
fn carga_sub(base: &Path, clave: &str, dir_in: &Path, dir_col: &Path)
    -> Option<SubBobina>
{
    let pp = base.join("projects").join(format!("{clave}.json"));
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&pp).ok()?).ok()?;
    let prefs_proy = v.get("prefs").cloned().unwrap_or(serde_json::json!({}));
    let nom_in = v["lutEntrada"].as_str().unwrap_or("Directo · sin transformar.cube").to_string();
    let nom_c = v["lutColor"].as_str().unwrap_or("Saorín · 65 puntos.cube").to_string();
    let mut aus = Vec::new();
    let clips = carga_clips_de(&v["clips"].as_array().cloned().unwrap_or_default(),
                               base, dir_in, dir_col, &prefs_proy, &nom_in, &nom_c,
                               &mut aus);
    if clips.is_empty() { return None }
    let capas_json = v["capas"].as_array().cloned().unwrap_or_default();
    let capas_clips = carga_clips_de(&capas_json, base, dir_in, dir_col,
                                     &prefs_proy, &nom_in, &nom_c, &mut aus);
    let capas = capas_clips.into_iter().zip(capas_json.iter())
        .map(|(c, j)| Capa {
            c,
            start: j["start"].as_f64().unwrap_or(0.0).max(0.0),
            fundido_in: j["fadeIn"].as_f64().unwrap_or(0.0).max(0.0),
            fundido_out: j["fadeOut"].as_f64().unwrap_or(0.0).max(0.0),
        }).collect();
    let mut audio = Vec::new();
    for a in v["audio"].as_array().cloned().unwrap_or_default() {
        let media = a["media"].as_str().unwrap_or("").to_string();
        let ruta = resolver(base, &media);
        if !ruta.is_file() { continue }
        audio.push(ClipAudio {
            media, ruta,
            t_in: a["in"].as_f64().unwrap_or(0.0),
            t_out: a["out"].as_f64().unwrap_or(0.0),
            start: a["start"].as_f64().unwrap_or(0.0),
            gain: a["gain"].as_f64().unwrap_or(0.0),
            fade_in: a["fadeIn"].as_f64().unwrap_or(0.0),
            fade_out: a["fadeOut"].as_f64().unwrap_or(0.0),
            banda: Vec::new(),
            mute: a["mute"].as_bool().unwrap_or(false),
            pista: a["pista"].as_u64().unwrap_or(0).min(PISTAS_MUSICA as u64 - 1) as u8,
            desfase: a["desfase"].as_f64().unwrap_or(0.0),
        });
    }
    let dur: f64 = clips.iter().map(|c| c.dur()).sum();
    let (w, h) = (v["project"]["w"].as_u64().unwrap_or(1920) as u32,
                  v["project"]["h"].as_u64().unwrap_or(1080) as u32);
    let fps = v["project"]["fps"].as_f64().filter(|f| *f > 1.0).unwrap_or(25.0);
    Some(SubBobina { clips, capas, audio, w, h, fps, dur })
}

/// EL LECTOR DE CLIPS, uno solo (CAPAS §2): lo usan la bobina, las capas y
/// las bobinas hijas. Antes este bucle vivía dentro de `cargar` y cualquier
/// segundo consumidor habría tenido que copiarlo.
#[allow(clippy::too_many_arguments)]
fn carga_clips_de(arr: &[serde_json::Value], base: &Path,
                  dir_in: &Path, dir_col: &Path,
                  prefs_proy: &serde_json::Value,
                  nom_in_proy: &str, nom_c_proy: &str,
                  ausentes: &mut Vec<String>) -> Vec<Clip> {
        let mut clips = Vec::new();
        let mut ausentes: Vec<String> = Vec::new();
        for c in arr.iter().cloned() {
            let hueco = c["gap"].as_bool().unwrap_or(false);
            let media = c["media"].as_str().unwrap_or("").to_string();
            let (t_in, t_out) = if hueco {
                (0.0, c["dur"].as_f64().unwrap_or(2.0))
            } else {
                (c["in"].as_f64().unwrap_or(0.0), c["out"].as_f64().unwrap_or(0.0))
            };
            let ruta = if hueco { PathBuf::new() } else { resolver(base, &media) };
            // UN CLIP NO SE TIRA NUNCA. Antes, si su fichero no se resolvía,
            // aquí había un `continue`: el clip desaparecía del montaje sin
            // decir nada, y el autor perdía el trabajo sin enterarse. Ahora se
            // conserva —con su corte, su receta y su sitio— y se marca como
            // material ausente para que se pueda volver a enlazar.
            let es_anidada = c["anidada"].as_str().is_some();
            if !hueco && !es_anidada && !ruta.is_file() {
                eprintln!("⚠ material ausente: «{media}» (el clip se conserva; \
                           vuelve a importarlo para recuperar la imagen)");
                ausentes.push(media.clone());
            }
            // el cuarto oscuro del clip: los del proyecto + lo suyo encima
            let mut prefs = prefs_proy.clone();
            if let (Some(base_obj), Some(propias)) = (prefs.as_object_mut(), c["prefs"].as_object()) {
                for (k, val) in propias { base_obj.insert(k.clone(), val.clone()); }
            }
            let nom_in = c["lutEntrada"].as_str().unwrap_or(nom_in_proy);
            let nom_c = c["lutColor"].as_str().unwrap_or(nom_c_proy);
            // LA ORIENTACIÓN DEL FICHERO. Se guarda con el clip para no volver
            // a abrir el contenedor en cada carga, pero si no está (bobinas de
            // antes) se lee ahora: es lo que endereza el material de móvil.
            let cuartos_fichero = c["cuartos"].as_u64().map(|q| (q % 4) as u8)
                .unwrap_or_else(|| if hueco || es_anidada { 0 } else {
                    filmlook_core::indice::sondea_orientado(&ruta).map(|x| x.4).unwrap_or(0)
                });
            // EL ENCUADRE: `tf` es el modelo de hoy; `transform` era el de
            // antes (zoom + centro) y se convierte una vez al abrir.
            let enc = if c["tf"].is_object() {
                Encuadre::de_json(&c["tf"], cuartos_fichero)
            } else if c["transform"].is_object() {
                let z = c["transform"]["zoom"].as_f64().unwrap_or(1.0).max(0.05);
                let cx = c["transform"]["cx"].as_f64().unwrap_or(0.5);
                let cy = c["transform"]["cy"].as_f64().unwrap_or(0.5);
                let mut e = Encuadre::limpio(cuartos_fichero);
                e.escala = (z as f32, z as f32);
                e.pos = (((0.5 - cx) * (z - 1.0)) as f32, ((0.5 - cy) * (z - 1.0)) as f32);
                e
            } else {
                Encuadre::limpio(cuartos_fichero)
            };
            let ausente = !hueco && !es_anidada && !ruta.is_file();
            clips.push(Clip {
                media, ruta, t_in, t_out, hueco,
                fade: c["fade"].as_f64().unwrap_or(0.0),
                speed: c["speed"].as_f64().map(|v| v.clamp(-8.0, 8.0)).unwrap_or(1.0),
                enc, cuartos_fichero,
                prefs,
                lut_in: busca_lut(&dir_in, nom_in),
                lut_color: busca_lut(&dir_col, nom_c),
                mute: c["mute"].as_bool().unwrap_or(false),
                desfase: c["desfase"].as_f64().unwrap_or(0.0),
                washi: c["washi"].as_u64().map(|w| (w % 4) as u8),
                nota: c["nota"].as_str().unwrap_or("").to_string(),
                grupo: c["grupo"].as_u64().map(|g| g as u32),
                ausente,
                anidada: c["anidada"].as_str().map(String::from),
            });
        }

    clips
}

impl Proyecto {
    /// la estantería: lo que hay en media/ + lo referenciado en media.json
    /// el registro de material DE ESTA BOBINA: projects/<nombre>.media.json
    /// (la bobina clásica usa el media.json del taller, como siempre)
    pub fn media_json(&self) -> PathBuf {
        let actual = std::fs::read_to_string(self.base.join("current.txt")).unwrap_or_default();
        let actual = actual.trim().to_string();
        if actual.is_empty() {
            // la bobina clásica sigue con el media.json del taller
            self.base.join("media.json")
        } else {
            // CARPETA APARTE: en projects/ solo viven bobinas — un registro
            // ahí se confundiría con una (bobinas() lista projects/*.json)
            let dir = self.base.join("registros");
            let _ = std::fs::create_dir_all(&dir);
            dir.join(format!("{actual}.json"))
        }
    }

    pub fn estanteria(&self) -> Vec<Cinta> {
        let mut v: Vec<Cinta> = Vec::new();
        let mut mete = |nombre: String, ruta: PathBuf| {
            if !ruta.is_file() || v.iter().any(|c| c.nombre == nombre) { return; }
            let ext = nombre.rsplit('.').next().unwrap_or("").to_lowercase();
            // las FOTOS son ciudadanas de primera (4 s por defecto)
            if ["jpg", "jpeg", "png"].contains(&ext.as_str()) {
                v.push(Cinta { nombre, ruta, dur: 4.0, w: 0, h: 0, fps: 0.0, balda: None });
                return;
            }
            // las CINTAS DE AUDIO van a la pista de música (fps=-1 las marca)
            if ["wav", "mp3", "flac", "m4a"].contains(&ext.as_str()) {
                v.push(Cinta { nombre, ruta, dur: 0.0, w: 0, h: 0, fps: -1.0, balda: None });
                return;
            }
            if !["mp4", "mov", "m4v", "mkv", "webm"].contains(&ext.as_str()) { return; }
            // sondeo nativo (leer el moov); ffprobe solo para contenedores no-BMFF
            let (w, h, fps, dur) = filmlook_core::indice::sondea(&ruta)
                .or_else(|_| filmlook_core::video::probe(ruta.to_str().unwrap_or("")))
                .unwrap_or((0, 0, 0.0, 0.0));
            v.push(Cinta { nombre, ruta, dur, w, h, fps, balda: None });
        };
        if let Ok(rd) = std::fs::read_dir(self.base.join("media")) {
            for e in rd.flatten() {
                mete(e.file_name().to_string_lossy().to_string(), e.path());
            }
        }
        // el registro DE LA BOBINA (cada bobina tiene su estantería); si es
        // la clásica, este camino ES el media.json del taller
        let mj = self.media_json();
        if let Ok(b) = std::fs::read(&mj) {
            if let Ok(j) = serde_json::from_slice::<serde_json::Value>(&b) {
                if let Some(o) = j.as_object() {
                    for (n, p) in o {
                        if n.starts_with('_') { continue; }   // _baldas y demás
                        if let Some(p) = p.as_str() { mete(n.clone(), PathBuf::from(p)); }
                    }
                }
            }
        }
        // asignar cada cinta a su BALDA según el registro (_baldas)
        if let Ok(b) = std::fs::read(&mj) {
            if let Ok(j) = serde_json::from_slice::<serde_json::Value>(&b) {
                if let Some(baldas) = j["_baldas"].as_object() {
                    for (nombre_b, def) in baldas {
                        if let Some(lista) = def["cintas"].as_array() {
                            for n in lista.iter().filter_map(|x| x.as_str()) {
                                if let Some(c) = v.iter_mut().find(|c| c.nombre == n) {
                                    c.balda = Some(nombre_b.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
        v.sort_by(|a, b| a.nombre.cmp(&b.nombre));
        v
    }

    /// las baldas del registro: (nombre, carpeta enchufada), ordenadas
    pub fn baldas(&self) -> Vec<(String, Option<PathBuf>)> {
        let mut v: Vec<(String, Option<PathBuf>)> = std::fs::read(self.media_json()).ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|j| j["_baldas"].as_object().map(|o| {
                o.iter().map(|(n, d)| {
                    (n.clone(), d["carpeta"].as_str().map(PathBuf::from))
                }).collect()
            })).unwrap_or_default();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }

    /// UN CLIP NUEVO a partir de una cinta de la estantería, con el cuarto
    /// oscuro del proyecto ENTERO puesto (prefs y las dos gelatinas): un clip
    /// nuevo jamás entra «en crudo». La orientación sale del contenedor, así
    /// que un vídeo de móvil entra derecho.
    pub fn clip_de(&self, c: &Cinta) -> Clip {
        let cuartos = filmlook_core::indice::sondea_orientado(&c.ruta)
            .map(|x| x.4).unwrap_or(0);
        Clip {
            media: c.nombre.clone(),
            ruta: c.ruta.clone(),
            t_in: 0.0,
            t_out: if c.dur > 0.1 { c.dur } else { 4.0 },
            hueco: false,
            fade: 0.0,
            speed: 1.0,
            enc: Encuadre::limpio(cuartos),
            cuartos_fichero: cuartos,
            prefs: self.prefs.clone(),
            lut_in: self.lut_in.clone(),
            lut_color: self.lut_color.clone(),
            mute: false, desfase: 0.0,
            washi: None, nota: String::new(), grupo: None, ausente: false,
            anidada: None,
        }
    }

    /// UN HUECO: negro con silencio, de `dur` segundos
    pub fn hueco_de(&self, dur: f64) -> Clip {
        Clip {
            media: String::new(), ruta: PathBuf::new(),
            t_in: 0.0, t_out: dur.max(0.04), hueco: true,
            fade: 0.0, speed: 1.0,
            enc: Encuadre::limpio(0), cuartos_fichero: 0,
            prefs: self.prefs.clone(), lut_in: None, lut_color: None,
            mute: false, desfase: 0.0,
            washi: None, nota: String::new(), grupo: None, ausente: false,
            anidada: None,
        }
    }

    /// añade una cinta al final de la bobina
    pub fn anade(&mut self, c: &Cinta) {
        let nuevo = self.clip_de(c);
        self.clips.push(nuevo);
    }

    /// TODO corte cae en la rejilla de frames (frame-accurate, QoL (E))
    pub fn cuantiza(&mut self) {
        let fps = self.fps.max(1.0);
        for c in &mut self.clips {
            if c.hueco { continue; }
            c.t_in = (c.t_in * fps).round() / fps;
            c.t_out = ((c.t_out * fps).round() / fps).max(c.t_in + 1.0 / fps);
        }
    }

    /// parte el clip que hay bajo t. Devuelve true si cortó de verdad.
    pub fn corta(&mut self, t: f64) -> bool {
        let Some((i, src_t)) = self.en(t) else { return false };
        let fps = self.fps.max(1.0);
        let src_t = (src_t * fps).round() / fps;
        let c = &self.clips[i];
        if src_t - c.t_in < 0.08 || c.t_out - src_t < 0.08 { return false; }
        let mut nuevo = c.clone();
        nuevo.t_in = src_t;
        nuevo.fade = 0.0;
        nuevo.nota = String::new();
        // un clip CONGELADO no se corta por el tiempo de fuente (siempre es el
        // mismo fotograma): se parte por la mitad que toque de su duración
        if c.congelado() {
            let ini = self.inicios().get(i).copied().unwrap_or(0.0);
            let dentro = (t - ini).clamp(0.04, c.dur() - 0.04);
            nuevo.t_in = c.t_in;
            nuevo.t_out = c.t_out - dentro;
            self.clips[i].t_out = c.t_in + dentro;
            self.clips.insert(i + 1, nuevo);
            return true;
        }
        self.clips[i].t_out = src_t;
        self.clips.insert(i + 1, nuevo);
        true
    }

    /// EL SONIDO DE UN CLIP, SUELTO EN SU PROPIA PISTA (§7).
    ///
    /// El sonido de un vídeo viajaba pegado a su clip y no había forma de
    /// tratarlo como material: ni moverlo, ni cortarlo aparte, ni dejarlo
    /// sonando por debajo del plano siguiente. Esto lo baja a una pista de
    /// audio —el mismo fichero, el mismo trozo, en el mismo segundo— y calla
    /// el del clip, para que no suene dos veces.
    ///
    /// El clip NO se toca por lo demás: si te arrepientes, se vuelve a subir
    /// la palanca y se borra la pista.
    pub fn desacopla(&mut self, i: usize) -> Option<usize> {
        let ini = *self.inicios().get(i)?;
        let c = self.clips.get(i)?;
        if c.hueco || c.mute || crate::foto::es_foto(&c.ruta) { return None }
        let (media, ruta, t_in, t_out) = (c.media.clone(), c.ruta.clone(), c.t_in, c.t_out);
        // el primer carril libre en ese tramo, para no tapar otra música
        let dur = (t_out - t_in).max(0.05);
        let libre = (0..PISTAS_MUSICA as u8).find(|k| {
            !self.audio.iter().any(|a| a.pista == *k
                && a.entra() < ini + dur - 1e-6 && ini < a.entra() + a.dur() - 1e-6)
        }).unwrap_or(0);
        self.audio.push(ClipAudio {
            media, ruta, t_in, t_out, start: ini, gain: 0.0,
            fade_in: 0.0, fade_out: 0.0, banda: Vec::new(), mute: false,
            pista: libre, desfase: 0.0,
        });
        self.clips[i].mute = true;
        Some(self.audio.len() - 1)
    }

    pub fn quita(&mut self, i: usize) {
        if i < self.clips.len() { self.clips.remove(i); }
    }

    /// LA CUCHILLA, TAMBIÉN EN LA MÚSICA. Parte la pista `ia` por el segundo
    /// `t` de la bobina y deja las dos mitades pegadas, con la misma receta.
    /// Devuelve el índice de la mitad nueva.
    ///
    /// Los fundidos NO se heredan a medias: el trozo de la izquierda conserva
    /// su entrada y pierde su salida, el de la derecha al revés. Si no, un
    /// corte por la mitad de una canción con fundido de salida deja las dos
    /// mitades bajando de volumen.
    pub fn corta_audio(&mut self, ia: usize, t: f64) -> Option<usize> {
        let a = self.audio.get(ia)?;
        let dentro = t - a.entra();
        if dentro < 0.08 || a.dur() - dentro < 0.08 { return None }
        let mut nuevo = a.clone();
        nuevo.t_in = a.t_in + dentro;
        nuevo.start = a.entra() + dentro;
        nuevo.desfase = 0.0;
        nuevo.fade_in = 0.0;
        let a = self.audio.get_mut(ia)?;
        a.t_out = a.t_in + dentro;
        a.fade_out = 0.0;
        self.audio.insert(ia + 1, nuevo);
        Some(ia + 1)
    }

    /// LOS PUNTOS A LOS QUE SE PEGA EL IMÁN: el principio y el final de cada
    /// plano, y cada marca. Es lo que hace que cortar o colocar música «con
    /// la imagen» sea un gesto y no una puntería.
    pub fn imanes(&self) -> Vec<f64> {
        let mut v: Vec<f64> = vec![0.0];
        let mut acc = 0.0;
        for c in &self.clips { acc += c.dur(); v.push(acc); }
        v.extend(self.marcas.iter().map(|m| m.t));
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        v
    }

    /// el imán más cercano a `t` dentro de `radio` segundos (None si ninguno)
    pub fn iman_cerca(&self, t: f64, radio: f64) -> Option<f64> {
        self.imanes().into_iter()
            .filter(|x| (x - t).abs() <= radio)
            .min_by(|a, b| (a - t).abs().partial_cmp(&(b - t).abs())
                    .unwrap_or(std::cmp::Ordering::Equal))
    }

    /// guarda la bobina donde la lee el resto del taller
    /// EL FICHERO DE ESTA BOBINA. Estaba calculado a mano en `guarda()` y en
    /// `cargar()`; que lo diga un solo sitio es lo que permite enseñárselo al
    /// autor sin miedo a mentir.
    pub fn ruta_json(&self) -> PathBuf {
        let actual = std::fs::read_to_string(self.base.join("current.txt")).unwrap_or_default();
        let actual = actual.trim().to_string();
        if actual.is_empty() { self.base.join("project.json") }
        else { self.base.join("projects").join(format!("{actual}.json")) }
    }

    pub fn guarda(&self) -> anyhow::Result<()> {
        let actual = std::fs::read_to_string(self.base.join("current.txt")).unwrap_or_default();
        let actual = actual.trim().to_string();
        let pp = if actual.is_empty() { self.base.join("project.json") }
                 else { self.base.join("projects").join(format!("{actual}.json")) };
        let mut v: serde_json::Value = std::fs::read(&pp).ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(serde_json::json!({}));
        let clips: Vec<serde_json::Value> = self.clips.iter().enumerate().map(|(i, c)| {
            let mut o = serde_json::json!({
                "id": i + 1, "media": c.media, "in": c.t_in, "out": c.t_out
            });
            if c.hueco { o["gap"] = serde_json::json!(true); o["dur"] = serde_json::json!(c.t_out); }
            if c.fade > 0.001 { o["fade"] = serde_json::json!(c.fade); }
            if (c.speed - 1.0).abs() > 0.001 { o["speed"] = serde_json::json!(c.speed); }
            // EL ENCUADRE se guarda TAL CUAL, sin traducir a nada
            if !c.enc.es_limpio(c.cuartos_fichero) { o["tf"] = c.enc.json(); }
            if c.cuartos_fichero != 0 { o["cuartos"] = serde_json::json!(c.cuartos_fichero); }
            if c.prefs != self.prefs { o["prefs"] = c.prefs.clone(); }
            if c.mute { o["mute"] = serde_json::json!(true); }
            if c.desfase.abs() > 1e-4 { o["desfase"] = serde_json::json!(c.desfase); }
            if let Some(w) = c.washi { o["washi"] = serde_json::json!(w); }
            if !c.nota.is_empty() { o["nota"] = serde_json::json!(c.nota); }
            if let Some(g) = c.grupo { o["grupo"] = serde_json::json!(g); }
            if let Some(a) = &c.anidada { o["anidada"] = serde_json::json!(a); }
            o
        }).collect();
        v["clips"] = serde_json::json!(clips);
        // LAS CAPAS (CAPAS §2): los mismos campos que un clip + colocación
        let capas: Vec<serde_json::Value> = self.capas.iter().map(|cp| {
            let c = &cp.c;
            let mut o = serde_json::json!({
                "media": c.media, "in": c.t_in, "out": c.t_out,
                "start": cp.start,
            });
            if cp.fundido_in > 0.001 { o["fadeIn"] = serde_json::json!(cp.fundido_in); }
            if cp.fundido_out > 0.001 { o["fadeOut"] = serde_json::json!(cp.fundido_out); }
            if (c.speed - 1.0).abs() > 0.001 { o["speed"] = serde_json::json!(c.speed); }
            if !c.enc.es_limpio(c.cuartos_fichero) { o["tf"] = c.enc.json(); }
            if c.cuartos_fichero != 0 { o["cuartos"] = serde_json::json!(c.cuartos_fichero); }
            if c.prefs != self.prefs { o["prefs"] = c.prefs.clone(); }
            if c.mute { o["mute"] = serde_json::json!(true); }
            o
        }).collect();
        v["capas"] = serde_json::json!(capas);
        let audio: Vec<serde_json::Value> = self.audio.iter().enumerate().map(|(i, a)| {
            serde_json::json!({ "id": i + 1, "media": a.media, "in": a.t_in,
                "out": a.t_out, "start": a.start, "gain": a.gain,
                "mute": a.mute, "fadeIn": a.fade_in, "fadeOut": a.fade_out,
                "pista": a.pista, "desfase": a.desfase,
                "banda": a.banda.iter().map(|(t, g)| serde_json::json!({"t": t, "g": g}))
                    .collect::<Vec<_>>() })
        }).collect();
        v["audio"] = serde_json::json!(audio);
        v["markers"] = serde_json::json!(self.marcas.iter().map(|m| {
            serde_json::json!({"t": m.t, "nota": m.nota, "color": m.color})
        }).collect::<Vec<_>>());
        v["range"] = match self.rango {
            Some((a, b)) => serde_json::json!({"in": a, "out": b}),
            None => serde_json::Value::Null,
        };
        v["mudo"] = serde_json::json!({"voz": self.mudo_voz, "musica": self.mudo_musica});
        v["vol"] = serde_json::json!({"voz": self.vol_voz, "musica": self.vol_musica});
        v["prefs"] = self.prefs.clone();
        if let Some(f) = &self.formato {
            v["project"] = serde_json::json!({
                "aspect": f.aspecto, "fps": self.fps, "w": f.w, "h": f.h });
        }
        v["v"] = serde_json::json!(1);
        let tmp = pp.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(&v)?)?;
        std::fs::rename(&tmp, &pp)?;
        Ok(())
    }

    pub fn cargar() -> Result<Self> {
        let base = std::env::var("FL_MEDIA").ok()
            .map(|m| PathBuf::from(m).parent().unwrap_or(Path::new(".")).to_path_buf())
            .unwrap_or_else(|| casa().join("filmlab"));
        // la bobina abierta (multi-proyecto) o la clásica
        let actual = std::fs::read_to_string(base.join("current.txt")).unwrap_or_default();
        let actual = actual.trim();
        let pp = if actual.is_empty() {
            base.join("project.json")
        } else {
            base.join("projects").join(format!("{actual}.json"))
        };
        let v: serde_json::Value = std::fs::read(&pp).ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(serde_json::json!({}));

        // las dos carpetas de gelatinas, configurables (por defecto, las de
        // siempre: <taller>/luts/entrada y <taller>/luts/color)
        let dir_in = crate::prefs::dir_luts(&base, "entrada");
        let dir_col = crate::prefs::dir_luts(&base, "color");
        let prefs_proy = v.get("prefs").cloned().unwrap_or(serde_json::json!({}));
        let nom_in_proy = v["lutEntrada"].as_str().unwrap_or("Directo · sin transformar.cube").to_string();
        let nom_c_proy = v["lutColor"].as_str().unwrap_or("Saorín · 65 puntos.cube").to_string();

        let mut ausentes: Vec<String> = Vec::new();
        let clips = carga_clips_de(
            &v["clips"].as_array().cloned().unwrap_or_default(), &base,
            &dir_in, &dir_col, &prefs_proy, &nom_in_proy, &nom_c_proy,
            &mut ausentes);

        // ── LAS CAPAS (CAPAS §2): clips colocados, leídos por el mismo lector ──
        let capas_json = v["capas"].as_array().cloned().unwrap_or_default();
        let capas_clips = carga_clips_de(&capas_json, &base, &dir_in, &dir_col,
                                         &prefs_proy, &nom_in_proy, &nom_c_proy,
                                         &mut ausentes);
        let capas: Vec<Capa> = capas_clips.into_iter().zip(capas_json.iter())
            .map(|(c, j)| Capa {
                c,
                start: j["start"].as_f64().unwrap_or(0.0).max(0.0),
                fundido_in: j["fadeIn"].as_f64().unwrap_or(0.0).max(0.0),
                fundido_out: j["fadeOut"].as_f64().unwrap_or(0.0).max(0.0),
            }).collect();

        let lut_in = busca_lut(&dir_in, &nom_in_proy);
        let lut_color = busca_lut(&dir_col, &nom_c_proy);

        let fps = v["project"]["fps"].as_f64().filter(|f| *f > 1.0)
            .or_else(|| clips.first().and_then(|c| {
                filmlook_core::indice::sondea(&c.ruta).ok().map(|x| x.2)
                    .or_else(|| filmlook_core::video::probe(c.ruta.to_str().unwrap_or("")).ok().map(|x| x.2))
            }))
            .unwrap_or(25.0);

        // el formato del proyecto («auto» = del primer clip → None)
        let formato = v["project"]["aspect"].as_str()
            .filter(|a| *a != "auto" && !a.is_empty())
            .map(|a| {
                let (mut w, mut h) = (v["project"]["w"].as_u64().unwrap_or(0) as u32,
                                      v["project"]["h"].as_u64().unwrap_or(0) as u32);
                if w == 0 || h == 0 {
                    if let Some(f) = FORMATOS.iter().find(|f| f.0 == a) { w = f.2; h = f.3; }
                }
                Formato { w: w.max(16), h: h.max(16), aspecto: a.to_string() }
            });

        let mut audio = Vec::new();
        for a in v["audio"].as_array().cloned().unwrap_or_default() {
            let media = a["media"].as_str().unwrap_or("").to_string();
            let ruta = resolver(&base, &media);
            if !ruta.is_file() { continue; }
            audio.push(ClipAudio {
                media, ruta,
                t_in: a["in"].as_f64().unwrap_or(0.0),
                t_out: a["out"].as_f64().unwrap_or(0.0),
                start: a["start"].as_f64().unwrap_or(0.0),
                gain: a["gain"].as_f64().unwrap_or(0.0),
                fade_in: a["fadeIn"].as_f64().unwrap_or(0.0),
                fade_out: a["fadeOut"].as_f64().unwrap_or(0.0),
                banda: a["banda"].as_array().cloned().unwrap_or_default().iter()
                    .filter_map(|p| Some((p["t"].as_f64()?, p["g"].as_f64()?)))
                    .collect(),
                mute: a["mute"].as_bool().unwrap_or(false),
                pista: a["pista"].as_u64().unwrap_or(0)
                    .min(PISTAS_MUSICA as u64 - 1) as u8,
                desfase: a["desfase"].as_f64().unwrap_or(0.0),
            });
        }
        // las marcas: objetos con nota y color, pero sin perder las que se
        // guardaron como números sueltos
        let mut marcas: Vec<Marca> = v["markers"].as_array().cloned().unwrap_or_default()
            .iter().filter_map(|m| {
                if let Some(t) = m.as_f64() { return Some(Marca::nueva(t)); }
                Some(Marca {
                    t: m["t"].as_f64()?,
                    nota: m["nota"].as_str().unwrap_or("").to_string(),
                    color: m["color"].as_u64().unwrap_or(0).min(3) as u8,
                })
            }).collect();
        marcas.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
        let rango = match (v["range"]["in"].as_f64(), v["range"]["out"].as_f64()) {
            (Some(a), Some(b)) if b > a + 0.01 => Some((a, b)),
            _ => None,
        };
        // ── LAS BOBINAS HIJAS de los clips anidados (CAPAS §2) ────────────
        // Se cargan al abrir (y al volver de editarlas, porque volver ES
        // volver a cargar): así la anidada está viva sin más maquinaria.
        let mut subbobinas = std::collections::HashMap::new();
        let claves: std::collections::HashSet<String> =
            clips.iter().filter_map(|c| c.anidada.clone()).collect();
        for clave in claves {
            match carga_sub(&base, &clave, &dir_in, &dir_col) {
                Some(sb) => { subbobinas.insert(clave, sb); }
                None => eprintln!("⚠ bobina anidada «{clave}»: no se pudo cargar                                    (el clip se conserva y se ve negro)"),
            }
        }

        let nombre = if actual.is_empty() { "bobina clásica".to_string() } else { actual.to_string() };
        Ok(Proyecto {
            base,
            nombre,
            clips,
            prefs: prefs_proy,
            lut_in,
            lut_color,
            fps,
            formato,
            marcas,
            audio,
            capas,
            subbobinas,
            mudo_voz: v["mudo"]["voz"].as_bool().unwrap_or(false),
            mudo_musica: v["mudo"]["musica"].as_bool().unwrap_or(false),
            rango,
            vol_voz: v["vol"]["voz"].as_f64().unwrap_or(0.0).clamp(-40.0, 12.0),
            vol_musica: v["vol"]["musica"].as_f64().unwrap_or(0.0).clamp(-40.0, 12.0),
        })
    }

    /// proporción del lienzo del proyecto (o la del primer clip si es auto)
    pub fn proporcion(&self) -> f32 {
        if let Some(f) = &self.formato {
            return f.w as f32 / f.h.max(1) as f32;
        }
        for c in &self.clips {
            if c.hueco { continue; }
            if let Ok((w, h, _, _)) = filmlook_core::indice::sondea(&c.ruta) {
                return w as f32 / h.max(1) as f32;
            }
        }
        16.0 / 9.0
    }

    /// «1080p25 · 9:16» — el formato a la vista, siempre
    pub fn rotulo_formato(&self) -> String {
        let fps = if (self.fps - self.fps.round()).abs() < 0.02 {
            format!("{:.0}", self.fps)
        } else {
            format!("{:.2}", self.fps)
        };
        match &self.formato {
            Some(f) => format!("{}p{} · {}", f.h.min(f.w), fps, f.aspecto),
            None => format!("auto · {fps} fps"),
        }
    }

    /// RESOLVER UN CLIP para la preview (CAPAS §6): si es una anidada,
    /// devuelve el clip REAL de la hija que suena en `src_t` (tiempo en la
    /// línea de la hija) y su tiempo de fuente. Un nivel: el máster aplana
    /// hasta tres, la preview enseña el primero (anotado en CAPAS.md).
    pub fn resuelve(&self, i: usize, src_t: f64) -> Option<(&Clip, f64)> {
        let c = self.clips.get(i)?;
        let Some(clave) = &c.anidada else { return Some((c, src_t)) };
        let sb = self.subbobinas.get(clave)?;
        let mut acc = 0.0;
        for hc in &sb.clips {
            let fin = acc + hc.dur();
            if src_t < fin || std::ptr::eq(hc, sb.clips.last().unwrap()) {
                return Some((hc, hc.fuente_en(src_t - acc)));
            }
            acc = fin;
        }
        None
    }

    /// LAS CAPAS VISIBLES en el segundo `t`: hasta dos, de abajo arriba, con
    /// su tiempo de fuente y su alfa (CAPAS §3)
    pub fn capas_en(&self, t: f64) -> Vec<(usize, f64, f32)> {
        let mut v: Vec<(usize, f64, f32)> = self.capas.iter().enumerate()
            .filter(|(_, cp)| t >= cp.start - 1e-9 && t < cp.fin() - 1e-9)
            .map(|(k, cp)| (k, cp.c.fuente_en(t - cp.start), cp.alfa_en(t)))
            .collect();
        let n = v.len();
        if n > 2 { v.drain(..n - 2); }
        v
    }

    /// RECARGAR LAS BOBINAS HIJAS (CAPAS §2): al insertar una anidada nueva
    /// o al volver de editar una hija
    pub fn recarga_subbobinas(&mut self) {
        let dir_in = crate::prefs::dir_luts(&self.base, "entrada");
        let dir_col = crate::prefs::dir_luts(&self.base, "color");
        self.subbobinas.clear();
        let mut cola: Vec<(String, usize)> = self.clips.iter()
            .filter_map(|c| c.anidada.clone()).map(|k| (k, 0)).collect();
        while let Some((clave, hondo)) = cola.pop() {
            if self.subbobinas.contains_key(&clave) || clave == self.nombre { continue }
            if hondo >= 3 { continue }
            if let Some(sb) = carga_sub(&self.base, &clave, &dir_in, &dir_col) {
                cola.extend(sb.clips.iter().filter_map(|c| c.anidada.clone())
                    .map(|k| (k, hondo + 1)));
                self.subbobinas.insert(clave, sb);
            }
        }
    }

    pub fn duracion(&self) -> f64 {
        self.clips.iter().map(|c| c.dur()).sum()
    }

    /// (índice del clip, tiempo dentro de su fuente) para un tiempo de bobina
    pub fn en(&self, t: f64) -> Option<(usize, f64)> {
        let mut acc = 0.0;
        for (i, c) in self.clips.iter().enumerate() {
            let fin = acc + c.dur();
            if t < fin || i + 1 == self.clips.len() {
                return Some((i, c.fuente_en(t - acc)));
            }
            acc = fin;
        }
        None
    }

    /// EL RANGO efectivo de la bobina: el marcado, o toda ella
    pub fn tramo(&self) -> (f64, f64) {
        let fin = self.duracion();
        match self.rango {
            Some((a, b)) => (a.clamp(0.0, fin), b.clamp(0.0, fin)),
            None => (0.0, fin),
        }
    }

    /// inicio de cada clip en la bobina
    pub fn inicios(&self) -> Vec<f64> {
        let mut v = Vec::with_capacity(self.clips.len());
        let mut acc = 0.0;
        for c in &self.clips {
            v.push(acc);
            acc += c.dur();
        }
        v
    }
}
