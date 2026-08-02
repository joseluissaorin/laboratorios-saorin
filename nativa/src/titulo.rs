//! Títulos de la casa: el texto se RASTERIZA a un PNG (Space Grotesk
//! sobre negro, ámbar Saorín opcional) y entra en la bobina como clip-foto
//! por el camino ya probado — preview = export, sin excepciones.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use std::path::{Path, PathBuf};

pub fn crea(base: &Path, texto: &str, w: u32, h: u32) -> Option<PathBuf> {
    let fuente = FontRef::try_from_slice(
        include_bytes!("../assets/fonts/SpaceGrotesk-Bold.ttf")).ok()?;
    let mut img = image::RgbImage::new(w, h);
    // negro película, no negro puro
    for p in img.pixels_mut() { *p = image::Rgb([10, 9, 8]); }

    let lineas: Vec<&str> = texto.lines().filter(|l| !l.trim().is_empty()).collect();
    if lineas.is_empty() { return None; }
    let escala = PxScale::from((h as f32 / 9.0).min(w as f32 * 1.6 / lineas.iter()
        .map(|l| l.chars().count()).max().unwrap_or(1) as f32));
    let sf = fuente.as_scaled(escala);
    let alto_linea = sf.height() * 1.25;
    let y0 = h as f32 / 2.0 - alto_linea * lineas.len() as f32 / 2.0;

    for (li, linea) in lineas.iter().enumerate() {
        // medir el ancho
        let mut ancho = 0.0f32;
        let mut previa: Option<ab_glyph::GlyphId> = None;
        for ch in linea.chars() {
            let id = fuente.glyph_id(ch);
            if let Some(p) = previa { ancho += sf.kern(p, id); }
            ancho += sf.h_advance(id);
            previa = Some(id);
        }
        let mut x = (w as f32 - ancho) / 2.0;
        let y = y0 + li as f32 * alto_linea + sf.ascent();
        let mut previa: Option<ab_glyph::GlyphId> = None;
        for ch in linea.chars() {
            let id = fuente.glyph_id(ch);
            if let Some(p) = previa { x += sf.kern(p, id); }
            let glyph = id.with_scale_and_position(escala, ab_glyph::point(x, y));
            if let Some(og) = fuente.outline_glyph(glyph) {
                let bb = og.px_bounds();
                og.draw(|gx, gy, cov| {
                    let (px, py) = (bb.min.x as i32 + gx as i32, bb.min.y as i32 + gy as i32);
                    if px >= 0 && py >= 0 && (px as u32) < w && (py as u32) < h {
                        let v = (cov * 242.0) as u8;
                        let p = img.get_pixel_mut(px as u32, py as u32);
                        // hueso del zine sobre negro
                        p[0] = p[0].max(v);
                        p[1] = p[1].max((cov * 238.0) as u8);
                        p[2] = p[2].max((cov * 228.0) as u8);
                    }
                });
            }
            x += sf.h_advance(id);
            previa = Some(id);
        }
    }

    let dir = base.join("titulos");
    std::fs::create_dir_all(&dir).ok()?;
    let slug: String = texto.chars().filter(|c| c.is_alphanumeric() || *c == ' ')
        .take(24).collect::<String>().replace(' ', "_");
    let mut ruta = dir.join(format!("titulo_{slug}.png"));
    let mut k = 2;
    while ruta.exists() {
        ruta = dir.join(format!("titulo_{slug}_{k}.png"));
        k += 1;
    }
    img.save(&ruta).ok()?;
    Some(ruta)
}
