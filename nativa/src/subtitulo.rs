//! EL PIE: la pista de subtítulos.
//!
//! Un subtítulo no es un rótulo suelto: es texto con su entrada y su salida,
//! que se escribe, se corrige y se re-estila EN BLOQUE. Por eso tiene pista
//! propia y no vive en las capas.
//!
//! Pero para el REVELADO sí es una capa: cada línea se rasteriza a un PNG con
//! su alfa y entra por el camino ya probado de las capas (CAPAS §4), así que
//! los dos motores y la preview lo dibujan sin enterarse de que es un
//! subtítulo. **Preview = export, sin excepciones** (la regla de `titulo.rs`).
//!
//! El PNG se recorta al tamaño del texto (no al del lienzo): un lienzo entero
//! por línea serían 8 MB de textura por subtítulo y una bobina con doscientos
//! no cabría en la GPU.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use std::path::{Path, PathBuf};

/// UN SUBTÍTULO: cuándo entra, cuándo sale y qué dice.
#[derive(Clone, PartialEq, Debug)]
pub struct Sub {
    pub t0: f64,
    pub t1: f64,
    pub texto: String,
}

impl Sub {
    pub fn dur(&self) -> f64 { (self.t1 - self.t0).max(0.05) }
}

/// LA LETRA: las tres familias que trae el taller.
pub const FAMILIAS: [(&str, &[u8]); 3] = [
    ("Fraunces · serif", include_bytes!("../assets/fonts/Fraunces-Text.ttf")),
    ("Space Grotesk", include_bytes!("../assets/fonts/SpaceGrotesk-Regular.ttf")),
    ("Space Grotesk negra", include_bytes!("../assets/fonts/SpaceGrotesk-Bold.ttf")),
];

/// LOS COLORES DE LA LETRA. Nada de blanco puro: sobre película quema y se
/// separa de todo lo demás. El primero es el hueso del zine.
pub const TINTAS: [(&str, [f32; 3]); 4] = [
    ("hueso", [0.949, 0.933, 0.894]),
    ("ámbar", [0.969, 0.792, 0.475]),
    ("blanco", [1.0, 1.0, 1.0]),
    ("tinta", [0.106, 0.098, 0.086]),
];

/// LOS IDIOMAS que el oído entiende de una lista corta. El código es el que
/// espera whisper; `""` es «que lo adivine él», que sirve cuando la bobina
/// mezcla lenguas pero acierta menos en frases sueltas.
pub const IDIOMAS: [(&str, &str); 8] = [
    ("español", "es"),
    ("inglés", "en"),
    ("gallego", "gl"),
    ("catalán", "ca"),
    ("portugués", "pt"),
    ("francés", "fr"),
    ("italiano", "it"),
    ("lo adivina", ""),
];

/// EL ESTILO DEL PIE, para toda la pista.
///
/// El de casa es **clásico y moderno a la vez** a propósito: letra con
/// historia (Fraunces es un old-style de corte contemporáneo), centrada y
/// abajo como se ha hecho siempre, en hueso y no en blanco… y ni caja ni
/// contorno duro, que es lo que envejece un subtítulo: sólo una sombra
/// difuminada que lo despega del fondo sin que se note que está.
#[derive(Clone, PartialEq, Debug)]
pub struct Estilo {
    pub familia: u8,
    /// cuerpo de la letra, en fracción del ALTO del lienzo
    pub cuerpo: f32,
    pub tinta: u8,
    /// cuánta sombra (0 = ninguna)
    pub sombra: f32,
    /// contorno duro, en fracción del cuerpo (0 = ninguno)
    pub borde: f32,
    /// caja detrás (0 = ninguna, 1 = negra opaca)
    pub caja: f32,
    /// dónde se apoya la línea de abajo, en fracción del alto desde ABAJO
    pub margen: f32,
    pub mayusculas: bool,
    /// cuántos caracteres por línea antes de partir
    pub ancho_linea: u32,
    /// EN QUÉ LENGUA se escucha (índice de `IDIOMAS`). Va con el estilo
    /// porque es de la pista entera, no de cada línea.
    pub idioma: u8,
}

impl Default for Estilo {
    fn default() -> Self {
        Estilo {
            familia: 0,
            cuerpo: 0.046,
            tinta: 0,
            sombra: 0.75,
            borde: 0.0,
            caja: 0.0,
            margen: 0.085,
            mayusculas: false,
            ancho_linea: 40,
            idioma: 0,
        }
    }
}

impl Estilo {
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "familia": self.familia, "cuerpo": self.cuerpo, "tinta": self.tinta,
            "sombra": self.sombra, "borde": self.borde, "caja": self.caja,
            "margen": self.margen, "mayusculas": self.mayusculas,
            "ancho_linea": self.ancho_linea, "idioma": self.idioma,
        })
    }

    pub fn de_json(v: &serde_json::Value) -> Estilo {
        let d = Estilo::default();
        if !v.is_object() { return d; }
        let f = |k: &str, x: f32| v[k].as_f64().map(|n| n as f32).unwrap_or(x);
        Estilo {
            familia: v["familia"].as_u64().unwrap_or(d.familia as u64).min(2) as u8,
            cuerpo: f("cuerpo", d.cuerpo).clamp(0.02, 0.12),
            tinta: v["tinta"].as_u64().unwrap_or(d.tinta as u64).min(3) as u8,
            sombra: f("sombra", d.sombra).clamp(0.0, 1.0),
            borde: f("borde", d.borde).clamp(0.0, 0.2),
            caja: f("caja", d.caja).clamp(0.0, 1.0),
            margen: f("margen", d.margen).clamp(0.0, 0.6),
            mayusculas: v["mayusculas"].as_bool().unwrap_or(d.mayusculas),
            ancho_linea: v["ancho_linea"].as_u64().unwrap_or(d.ancho_linea as u64)
                .clamp(16, 80) as u32,
            idioma: v["idioma"].as_u64().unwrap_or(d.idioma as u64)
                .min(IDIOMAS.len() as u64 - 1) as u8,
        }
    }

    /// la huella del estilo + el texto: el nombre del PNG en la caché
    fn huella(&self, texto: &str, ph: u32) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        texto.hash(&mut h);
        self.familia.hash(&mut h);
        self.tinta.hash(&mut h);
        self.mayusculas.hash(&mut h);
        self.ancho_linea.hash(&mut h);
        ph.hash(&mut h);
        for v in [self.cuerpo, self.sombra, self.borde, self.caja] {
            ((v * 10000.0) as i64).hash(&mut h);
        }
        h.finish()
    }
}

/// PARTIR EL TEXTO EN LÍNEAS por palabras, como manda el oficio: nunca se
/// corta una palabra y se prefiere una línea equilibrada a una llena y otra
/// con dos sílabas.
pub fn parte(texto: &str, ancho: usize) -> Vec<String> {
    let limpio = texto.split_whitespace().collect::<Vec<_>>().join(" ");
    if limpio.is_empty() { return Vec::new(); }
    if limpio.chars().count() <= ancho { return vec![limpio]; }
    // dos líneas equilibradas: se busca el corte más cerca de la mitad
    let palabras: Vec<&str> = limpio.split(' ').collect();
    let total: usize = limpio.chars().count();
    let mut mejor = (usize::MAX, 1usize);
    for k in 1..palabras.len() {
        let izq: usize = palabras[..k].join(" ").chars().count();
        let der: usize = palabras[k..].join(" ").chars().count();
        if izq > ancho { break; }
        // el desequilibrio, y un castigo si la segunda tampoco cabe
        let mal = izq.abs_diff(total / 2) + if der > ancho { 100 } else { 0 };
        if mal < mejor.0 { mejor = (mal, k); }
    }
    let (izq, der) = (palabras[..mejor.1].join(" "), palabras[mejor.1..].join(" "));
    if der.chars().count() > ancho {
        // tres líneas o más: se sigue partiendo, pero eso ya es un subtítulo
        // demasiado largo y el autor debería cortarlo
        let mut v = vec![izq];
        v.extend(parte(&der, ancho));
        v
    } else {
        vec![izq, der]
    }
}

/// EL PIE RASTERIZADO: PNG con alfa, recortado al texto.
///
/// Devuelve (ruta, ancho, alto) en píxeles. El lienzo del máster manda el
/// cuerpo de la letra: el mismo estilo en una bobina 4K sale del mismo
/// tamaño relativo que en 1080.
pub fn rasteriza(base: &Path, e: &Estilo, texto: &str, pw: u32, ph: u32)
                 -> Option<(PathBuf, u32, u32)> {
    let texto = if e.mayusculas { texto.to_uppercase() } else { texto.to_string() };
    let lineas = parte(&texto, e.ancho_linea as usize);
    if lineas.is_empty() { return None; }

    let dir = base.join(".subs");
    std::fs::create_dir_all(&dir).ok()?;
    let ruta = dir.join(format!("{:016x}.png", e.huella(&texto, ph)));

    let fuente = FontRef::try_from_slice(FAMILIAS[(e.familia as usize).min(2)].1).ok()?;
    let px = (e.cuerpo * ph as f32).max(8.0);
    let escala = PxScale::from(px);
    let sf = fuente.as_scaled(escala);
    let alto_linea = px * 1.28;

    // medir
    let mide = |l: &str| -> f32 {
        let mut w = 0.0;
        let mut prev: Option<ab_glyph::GlyphId> = None;
        for ch in l.chars() {
            let id = fuente.glyph_id(ch);
            if let Some(p) = prev { w += sf.kern(p, id); }
            w += sf.h_advance(id);
            prev = Some(id);
        }
        w
    };
    let anchos: Vec<f32> = lineas.iter().map(|l| mide(l)).collect();
    let ancho_max = anchos.iter().cloned().fold(0.0f32, f32::max);
    // el margen: la sombra difuminada necesita sitio, y el contorno también
    // LA SOMBRA, ANCHA Y BLANDA. Ceñida (0.10 del cuerpo) se veía preciosa
    // sobre cielo oscuro y desaparecía sobre un parabrisas al sol — medido
    // mirando el mismo plano. Un halo de 0.22 del cuerpo despega el texto de
    // CUALQUIER fondo y sigue sin parecer una caja, que es lo que envejece.
    let radio = (px * 0.22 * e.sombra).max(if e.borde > 0.0 { px * e.borde } else { 0.0 });
    let pad = (radio * 2.0 + px * 0.18).ceil();
    let iw = ((ancho_max + pad * 2.0).ceil() as u32).clamp(8, pw.max(8) * 2);
    let ih = ((alto_linea * lineas.len() as f32 + pad * 2.0).ceil() as u32).max(8);

    if ruta.is_file() {
        return Some((ruta, iw, ih));
    }

    // ── la máscara del texto ────────────────────────────────────────────
    let mut mask = vec![0.0f32; (iw * ih) as usize];
    for (li, linea) in lineas.iter().enumerate() {
        let mut x = (iw as f32 - anchos[li]) / 2.0;
        let y = pad + li as f32 * alto_linea + sf.ascent();
        let mut prev: Option<ab_glyph::GlyphId> = None;
        for ch in linea.chars() {
            let id = fuente.glyph_id(ch);
            if let Some(p) = prev { x += sf.kern(p, id); }
            let g = id.with_scale_and_position(escala, ab_glyph::point(x, y));
            if let Some(og) = fuente.outline_glyph(g) {
                let bb = og.px_bounds();
                og.draw(|gx, gy, cov| {
                    let (px2, py2) = (bb.min.x as i32 + gx as i32, bb.min.y as i32 + gy as i32);
                    if px2 >= 0 && py2 >= 0 && (px2 as u32) < iw && (py2 as u32) < ih {
                        let i = (py2 as u32 * iw + px2 as u32) as usize;
                        mask[i] = mask[i].max(cov);
                    }
                });
            }
            x += sf.h_advance(id);
            prev = Some(id);
        }
    }

    // ── la sombra: la máscara difuminada y bajada un pelo ────────────────
    let sombra = if e.sombra > 0.001 {
        let mut s = desplaza(&mask, iw, ih, 0, (px * 0.045).round() as i32);
        let r = (radio.max(1.0)) as u32;
        s = difumina(&s, iw, ih, r);
        s = difumina(&s, iw, ih, r);   // dos pasadas de caja ≈ una gaussiana
        Some(s)
    } else { None };

    // ── el contorno duro, si se pide ─────────────────────────────────────
    let borde = if e.borde > 0.001 {
        Some(dilata(&mask, iw, ih, (px * e.borde).max(1.0) as u32))
    } else { None };

    // ── componer ────────────────────────────────────────────────────────
    let tinta = TINTAS[(e.tinta as usize).min(3)].1;
    let mut img = image::RgbaImage::new(iw, ih);
    for y in 0..ih {
        for x in 0..iw {
            let i = (y * iw + x) as usize;
            let mut r = 0.0f32; let mut g = 0.0; let mut b = 0.0; let mut a = 0.0f32;
            // la caja
            if e.caja > 0.001 { a = e.caja * 0.72; }
            // la sombra (negra, por debajo de todo)
            if let Some(s) = &sombra {
                // el halo se satura un poco (×1.6) antes de recortar: si no,
                // difuminar tanto lo deja translúcido y no separa nada
                let sa = (s[i] * 1.6 * e.sombra).clamp(0.0, 1.0);
                a = a + sa * (1.0 - a);
            }
            // el contorno (tinta oscura)
            if let Some(bd) = &borde {
                let ba = bd[i].clamp(0.0, 1.0);
                let (br, bg, bb) = (0.04, 0.035, 0.03);
                r = r * (1.0 - ba) + br * ba;
                g = g * (1.0 - ba) + bg * ba;
                b = b * (1.0 - ba) + bb * ba;
                a = a + ba * (1.0 - a);
            }
            // la letra
            let m = mask[i].clamp(0.0, 1.0);
            r = r * (1.0 - m) + tinta[0] * m;
            g = g * (1.0 - m) + tinta[1] * m;
            b = b * (1.0 - m) + tinta[2] * m;
            a = a + m * (1.0 - a);
            if a <= 0.002 { continue; }
            img.put_pixel(x, y, image::Rgba([
                (r.clamp(0.0, 1.0) * 255.0) as u8,
                (g.clamp(0.0, 1.0) * 255.0) as u8,
                (b.clamp(0.0, 1.0) * 255.0) as u8,
                (a.clamp(0.0, 1.0) * 255.0) as u8,
            ]));
        }
    }
    img.save(&ruta).ok()?;
    Some((ruta, iw, ih))
}

/// mueve una máscara (para la sombra)
fn desplaza(m: &[f32], w: u32, h: u32, dx: i32, dy: i32) -> Vec<f32> {
    let mut o = vec![0.0f32; m.len()];
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let (sx, sy) = (x - dx, y - dy);
            if sx >= 0 && sy >= 0 && sx < w as i32 && sy < h as i32 {
                o[(y as u32 * w + x as u32) as usize] = m[(sy as u32 * w + sx as u32) as usize];
            }
        }
    }
    o
}

/// desenfoque de caja separable (dos pasadas ≈ gaussiana)
fn difumina(m: &[f32], w: u32, h: u32, r: u32) -> Vec<f32> {
    if r == 0 { return m.to_vec(); }
    let mut tmp = vec![0.0f32; m.len()];
    let n = (2 * r + 1) as f32;
    for y in 0..h {
        for x in 0..w {
            let mut s = 0.0;
            for k in -(r as i32)..=(r as i32) {
                let xx = (x as i32 + k).clamp(0, w as i32 - 1) as u32;
                s += m[(y * w + xx) as usize];
            }
            tmp[(y * w + x) as usize] = s / n;
        }
    }
    let mut o = vec![0.0f32; m.len()];
    for y in 0..h {
        for x in 0..w {
            let mut s = 0.0;
            for k in -(r as i32)..=(r as i32) {
                let yy = (y as i32 + k).clamp(0, h as i32 - 1) as u32;
                s += tmp[(yy * w + x) as usize];
            }
            o[(y * w + x) as usize] = s / n;
        }
    }
    o
}

/// engorda la máscara (el contorno duro)
fn dilata(m: &[f32], w: u32, h: u32, r: u32) -> Vec<f32> {
    if r == 0 { return m.to_vec(); }
    let mut o = vec![0.0f32; m.len()];
    let r2 = (r * r) as i32;
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let mut v = 0.0f32;
            for dy in -(r as i32)..=(r as i32) {
                for dx in -(r as i32)..=(r as i32) {
                    if dx * dx + dy * dy > r2 { continue; }
                    let (xx, yy) = (x + dx, y + dy);
                    if xx >= 0 && yy >= 0 && xx < w as i32 && yy < h as i32 {
                        v = v.max(m[(yy as u32 * w + xx as u32) as usize]);
                    }
                }
            }
            o[(y as u32 * w + x as u32) as usize] = v;
        }
    }
    o
}

/// LEER UN .srt (lo que escupe el oído, y lo que trae cualquier otro sitio)
pub fn de_srt(texto: &str) -> Vec<Sub> {
    let mut subs = Vec::new();
    let reloj = |s: &str| -> Option<f64> {
        let s = s.trim().replace(',', ".");
        let p: Vec<&str> = s.split(':').collect();
        if p.len() != 3 { return None; }
        Some(p[0].parse::<f64>().ok()? * 3600.0
             + p[1].parse::<f64>().ok()? * 60.0
             + p[2].parse::<f64>().ok()?)
    };
    let mut lineas = texto.lines().peekable();
    while let Some(l) = lineas.next() {
        let l = l.trim();
        if !l.contains("-->") { continue; }
        let (a, b) = l.split_once("-->").unwrap();
        let (Some(t0), Some(t1)) = (reloj(a), reloj(b)) else { continue };
        let mut cuerpo = Vec::new();
        while let Some(sig) = lineas.peek() {
            if sig.trim().is_empty() { break; }
            cuerpo.push(lineas.next().unwrap().trim().to_string());
        }
        let texto = cuerpo.join(" ");
        if !texto.is_empty() { subs.push(Sub { t0, t1: t1.max(t0 + 0.05), texto }); }
    }
    subs
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn parte_por_palabras_y_equilibra() {
        let frase = "esto es una frase bastante larga que no cabe";
        let v = parte(frase, 30);
        assert_eq!(v.len(), 2, "{v:?}");
        assert!(v.iter().all(|l| l.chars().count() <= 30), "{v:?}");
        // ninguna palabra partida
        assert_eq!(v.join(" ").split_whitespace().count(),
                   frase.split_whitespace().count());
        // y equilibradas: no una llena y otra con dos letras
        let d = v[0].chars().count().abs_diff(v[1].chars().count());
        assert!(d < 10, "desequilibrio {d}: {v:?}");
    }

    /// más de dos líneas es mal subtítulo, pero si el autor lo escribe hay
    /// que repartirlo bien igualmente — nunca cortando una palabra
    #[test]
    fn tres_lineas_tambien_se_reparten() {
        let frase = "esto es una frase bastante larga que no cabe en una sola línea";
        let v = parte(frase, 30);
        assert!(v.len() >= 3, "{v:?}");
        assert!(v.iter().all(|l| l.chars().count() <= 30), "{v:?}");
        assert_eq!(v.join(" ").split_whitespace().count(),
                   frase.split_whitespace().count());
    }

    #[test]
    fn una_frase_corta_no_se_parte() {
        assert_eq!(parte("hola qué tal", 40), vec!["hola qué tal"]);
    }

    #[test]
    fn el_srt_se_lee_con_sus_tiempos() {
        let s = "1\n00:00:01,500 --> 00:00:03,250\nhola\n\n2\n00:01:00,000 --> 00:01:02,000\ndos\nlíneas\n";
        let v = de_srt(s);
        assert_eq!(v.len(), 2);
        assert!((v[0].t0 - 1.5).abs() < 1e-6 && (v[0].t1 - 3.25).abs() < 1e-6);
        assert_eq!(v[0].texto, "hola");
        assert!((v[1].t0 - 60.0).abs() < 1e-6);
        assert_eq!(v[1].texto, "dos líneas");
    }
}
