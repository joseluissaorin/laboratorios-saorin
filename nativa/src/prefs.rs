//! Preferencias de la APP (no del proyecto): persisten en
//! <taller>/prefs.json y viven como atómicas para que los hilos
//! (sonido, cabina, escrutinio) las lean sin cerrojos.

use std::sync::atomic::{AtomicBool, Ordering};

/// el sonido del taller (foley)
pub static FOLEY: AtomicBool = AtomicBool::new(true);
/// preview a media resolución (apagado = 4K también en movimiento)
pub static PREVIEW_MEDIA: AtomicBool = AtomicBool::new(true);
/// normalizar la sonoridad del máster (loudnorm). Es la ÚNICA opción de
/// revelado que sobrevive a la poda: no toca el vídeo ni cuesta velocidad.
pub static NORMALIZA: AtomicBool = AtomicBool::new(true);
/// EL IMÁN de la bobina: si los clips se pegan al soltarlos o van libres.
/// Encendido por defecto (una bobina de película no tiene huecos), pero se
/// apaga para separar un plano del siguiente.
pub static IMAN: AtomicBool = AtomicBool::new(true);

pub fn carga(base: &std::path::Path) {
    if let Ok(b) = std::fs::read(base.join("prefs.json")) {
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&b) {
            FOLEY.store(v["foley"].as_bool().unwrap_or(true), Ordering::Relaxed);
            PREVIEW_MEDIA.store(v["preview_media"].as_bool().unwrap_or(true), Ordering::Relaxed);
            SCRUB_AUDIBLE.store(v["scrub_audible"].as_bool().unwrap_or(true), Ordering::Relaxed);
            NORMALIZA.store(v["normaliza"].as_bool().unwrap_or(true), Ordering::Relaxed);
            IMAN.store(v["iman"].as_bool().unwrap_or(true), Ordering::Relaxed);
            crate::sonido::DUCKING.store(v["ducking"].as_bool().unwrap_or(true), Ordering::Relaxed);
        }
    }
}

pub fn guarda(base: &std::path::Path) {
    // mezclar, no pisar: el destino del máster vive en el mismo fichero
    let p = base.join("prefs.json");
    let mut v: serde_json::Value = std::fs::read(&p).ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(serde_json::json!({}));
    v["foley"] = serde_json::json!(FOLEY.load(Ordering::Relaxed));
    v["preview_media"] = serde_json::json!(PREVIEW_MEDIA.load(Ordering::Relaxed));
    v["scrub_audible"] = serde_json::json!(SCRUB_AUDIBLE.load(Ordering::Relaxed));
    v["normaliza"] = serde_json::json!(NORMALIZA.load(Ordering::Relaxed));
    v["iman"] = serde_json::json!(IMAN.load(Ordering::Relaxed));
    v["ducking"] = serde_json::json!(crate::sonido::DUCKING.load(Ordering::Relaxed));
    let _ = std::fs::write(&p, serde_json::to_vec_pretty(&v).unwrap_or_default());
}

/// dónde va el máster (lo elige el autor en la sala de revelado)
pub fn destino_guardado(base: &std::path::Path) -> Option<std::path::PathBuf> {
    let v: serde_json::Value = std::fs::read(base.join("prefs.json")).ok()
        .and_then(|b| serde_json::from_slice(&b).ok())?;
    v["destino"].as_str().map(std::path::PathBuf::from).filter(|p| p.is_dir())
}

pub fn guarda_destino(base: &std::path::Path, dir: Option<&std::path::Path>) {
    let p = base.join("prefs.json");
    let mut v: serde_json::Value = std::fs::read(&p).ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(serde_json::json!({}));
    match dir {
        Some(d) => v["destino"] = serde_json::json!(d.to_string_lossy()),
        None => { if let Some(o) = v.as_object_mut() { o.remove("destino"); } }
    }
    let _ = std::fs::write(&p, serde_json::to_vec_pretty(&v).unwrap_or_default());
}

// ── EL CAJÓN DEL MÁSTER ──────────────────────────────────────────────────
//
// Lo que sale por la puerta: a qué tamaño, revelado a qué escala, con qué
// códec y con cuánto caudal. Vive en el taller y no en la bobina porque es una
// decisión de ENTREGA, no de montaje: la misma bobina se saca hoy en 1080 para
// mandarla por correo y mañana en 8K para subirla.

/// (alto de salida · 0 = el de la bobina), factor de revelado, códec, caudal
/// base en Mb/s (para 1080) y filtro de escalado
#[derive(Clone, Debug, PartialEq)]
pub struct Master {
    pub alto: u32,
    pub sup: f64,
    pub codec: String,
    pub mbps: u32,
    pub filtro: String,
    /// cadencia del máster (0 = la de la bobina)
    pub fps: f64,
    /// LA AMPLIADORA (el cuarto oscuro): a qué tamaño sale la copia
    /// (0 = el lienzo, 1 = ×2, 2 = ×4) y en qué papel (0 = PNG 16 bits,
    /// 1 = PNG 8, 2 = JPEG)
    pub copia_tam: u32,
    pub copia_papel: u32,
}

impl Default for Master {
    fn default() -> Self {
        // el camino de siempre: al lienzo de la bobina, sin escalar y con el
        // códec que mastica el chip. Cero pases de más.
        Master { alto: 0, sup: 1.0, codec: "hevc".into(), mbps: 60,
                 filtro: "".into(), fps: 0.0, copia_tam: 0, copia_papel: 0 }
    }
}

/// LOS TAMAÑOS que ofrece el cajón (0 = el de la bobina)
pub const ALTURAS_MASTER: [(u32, &str); 6] = [
    (0, "del lienzo"), (720, "720p"), (1080, "1080p"),
    (1440, "1440p"), (2160, "4K"), (4320, "8K"),
];

/// A QUÉ ESCALA SE REVELA respecto a lo que sale
pub const REVELADOS: [(f64, &str, &str); 4] = [
    (0.5, "×0,5", "se revela más pequeño y se agranda: el grano del original, más gordo"),
    (1.0, "×1", "directo, sin escalar — el camino rápido de siempre"),
    (1.5, "×1,5", "supermuestreo suave: bordes y grano sin escalones"),
    (2.0, "×2", "supermuestreo entero: lo más limpio, y lo que más tarda"),
];

/// LOS CÓDECS, con la verdad de cada uno delante
pub const CODECS_MASTER: [(&str, &str, &str); 5] = [
    ("hevc", "HEVC 10 bits", "el motor del chip · el camino de la casa"),
    ("h264", "H.264 8 bits", "compatible con todo · pierde el 10-bit del look"),
    ("prores422hq", "ProRes 422 HQ", if cfg!(target_os = "macos")
        { "dos motores en el Mac · va aún más rápido" } else { "SOFTWARE aquí: lento" }),
    ("prores4444", "ProRes 4444", if cfg!(target_os = "macos")
        { "con alfa · el archivo de verdad" } else { "SOFTWARE aquí: lento" }),
    ("hevc_soft", "HEVC x265", "por software, lo más apretado · muy lento"),
];

pub const CAUDALES: [u32; 5] = [20, 40, 60, 150, 400];

pub const FILTROS_ESCALA: [(&str, &str); 3] = [
    ("", "el que toque"), ("lanczos", "nítido"), ("area", "suave"),
];

/// LA CADENCIA DEL MÁSTER (0 = la de la bobina). Que la bobina vaya a una y
/// el máster salga a otra es normal —se monta a 30 y se entrega a 24— y
/// además ahora se puede hacer sin tirón: cuando el máster cae entre dos
/// fotogramas de la fuente, el revelado los interpola (plan.rs).
pub const CADENCIAS_MASTER: [(f64, &str); 9] = [
    (0.0, "la bobina"), (23.976, "23,976"), (24.0, "24"), (25.0, "25"),
    (29.97, "29,97"), (30.0, "30"), (50.0, "50"), (59.94, "59,94"), (60.0, "60"),
];

pub fn master_guardado(base: &std::path::Path) -> Master {
    let d = Master::default();
    let Some(v) = std::fs::read(base.join("prefs.json")).ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        else { return d };
    let m = &v["master"];
    Master {
        alto: m["alto"].as_u64().unwrap_or(d.alto as u64) as u32,
        sup: m["super"].as_f64().unwrap_or(d.sup).clamp(0.25, 4.0),
        codec: m["codec"].as_str().unwrap_or(&d.codec).to_string(),
        mbps: m["mbps"].as_u64().unwrap_or(d.mbps as u64).clamp(5, 2000) as u32,
        filtro: m["filtro"].as_str().unwrap_or(&d.filtro).to_string(),
        fps: m["fps"].as_f64().unwrap_or(d.fps).clamp(0.0, 240.0),
        copia_tam: m["copia_tam"].as_u64().unwrap_or(d.copia_tam as u64).min(2) as u32,
        copia_papel: m["copia_papel"].as_u64().unwrap_or(d.copia_papel as u64).min(2) as u32,
    }
}

pub fn guarda_master(base: &std::path::Path, m: &Master) {
    let p = base.join("prefs.json");
    let mut v: serde_json::Value = std::fs::read(&p).ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(serde_json::json!({}));
    v["master"] = serde_json::json!({
        "alto": m.alto, "super": m.sup, "codec": m.codec,
        "mbps": m.mbps, "filtro": m.filtro, "fps": m.fps,
        "copia_tam": m.copia_tam, "copia_papel": m.copia_papel,
    });
    let _ = std::fs::write(&p, serde_json::to_vec_pretty(&v).unwrap_or_default());
}

// ── LA GEOMETRÍA DE LAS VENTANAS (§3 · 5 y §5) ───────────────────────────
//
// La principal se abría siempre a 1500×940 en el centro, y las secundarias no
// existían. Ahora cada ventana recuerda dónde estaba y con qué tamaño; si el
// sitio guardado ya no existe (un monitor que se fue), winit la coloca donde
// pueda y la siguiente vez se apunta la nueva.

/// (x, y, ancho, alto) en píxeles LÓGICOS
pub fn geometria(base: &std::path::Path, clave: &str) -> Option<(f64, f64, f64, f64)> {
    let v: serde_json::Value = std::fs::read(base.join("prefs.json")).ok()
        .and_then(|b| serde_json::from_slice(&b).ok())?;
    let g = &v["ventanas"][clave];
    let (w, h) = (g["w"].as_f64()?, g["h"].as_f64()?);
    if w < 200.0 || h < 150.0 { return None; }
    Some((g["x"].as_f64().unwrap_or(0.0), g["y"].as_f64().unwrap_or(0.0), w, h))
}

pub fn guarda_geometria(base: &std::path::Path, clave: &str,
                        x: f64, y: f64, w: f64, h: f64) {
    if w < 200.0 || h < 150.0 { return; }
    let p = base.join("prefs.json");
    let mut v: serde_json::Value = std::fs::read(&p).ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(serde_json::json!({}));
    v["ventanas"][clave] = serde_json::json!({"x": x, "y": y, "w": w, "h": h});
    let _ = std::fs::write(&p, serde_json::to_vec_pretty(&v).unwrap_or_default());
}

/// el SCRUB AUDIBLE: oír el material al arrastrar la aguja (la moviola)
pub static SCRUB_AUDIBLE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

// ── LAS CARPETAS DE GELATINAS ────────────────────────────────────────────
//
// Estaban clavadas en <taller>/luts/entrada y <taller>/luts/color. Quien tiene
// su colección de .cube en otro sitio —o compartida entre proyectos— tenía que
// copiarlas. Ahora se eligen, y si no se elige nada sigue siendo lo de antes.

pub fn dir_luts(base: &std::path::Path, ranura: &str) -> std::path::PathBuf {
    let v: Option<serde_json::Value> = std::fs::read(base.join("prefs.json")).ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    v.as_ref()
        .and_then(|v| v["luts"][ranura].as_str().map(std::path::PathBuf::from))
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| base.join("luts").join(ranura))
}

pub fn guarda_dir_luts(base: &std::path::Path, ranura: &str, dir: &std::path::Path) {
    let p = base.join("prefs.json");
    let mut v: serde_json::Value = std::fs::read(&p).ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(serde_json::json!({}));
    if !v["luts"].is_object() { v["luts"] = serde_json::json!({}); }
    v["luts"][ranura] = serde_json::json!(dir.to_string_lossy());
    let _ = std::fs::write(&p, serde_json::to_vec_pretty(&v).unwrap_or_default());
}

/// las gelatinas que hay en una ranura, por nombre y en orden
pub fn gelatinas(base: &std::path::Path, ranura: &str) -> Vec<std::path::PathBuf> {
    let mut v: Vec<std::path::PathBuf> = std::fs::read_dir(dir_luts(base, ranura))
        .into_iter().flatten().flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x.eq_ignore_ascii_case("cube")).unwrap_or(false))
        .collect();
    v.sort();
    v
}
