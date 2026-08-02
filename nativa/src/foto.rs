//! Fotos fijas como clips: un JPEG/PNG se convierte UNA vez a un
//! `Fotograma` (códigos YUV de 10 bits, BT.709 limited — lo que espera la
//! cadena) y el visor lo sirve sin decoder. La cámara del taller también
//! saca fotos: que entren como ciudadanas de primera.

use filmlook_core::cine::Fotograma;
use std::path::Path;

pub fn es_foto(ruta: &Path) -> bool {
    matches!(ruta.extension().and_then(|e| e.to_str())
                 .map(|e| e.to_lowercase()).as_deref(),
             Some("jpg") | Some("jpeg") | Some("png"))
}

/// carga y convierte (limitando el lado mayor a ~2000 px: es una preview)
pub fn carga(ruta: &Path) -> Option<Fotograma> {
    let img = image::open(ruta).ok()?;
    let img = if img.width().max(img.height()) > 2000 {
        img.resize(2000, 2000, image::imageops::FilterType::Triangle)
    } else { img };
    let rgb = img.to_rgb8();
    let (w, h) = (rgb.width() as usize & !1, rgb.height() as usize & !1);
    if w < 2 || h < 2 { return None; }
    let (cw, ch) = (w / 2, h / 2);
    let mut y = vec![0u16; w * h];
    let mut u = vec![0u16; cw * ch];
    let mut v = vec![0u16; cw * ch];
    let px = |x: usize, yy: usize| -> (f32, f32, f32) {
        let p = rgb.get_pixel(x as u32, yy as u32);
        (p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0)
    };
    for fila in 0..h {
        for col in 0..w {
            let (r, g, b) = px(col, fila);
            let yl = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            y[fila * w + col] = ((yl * 876.0 + 64.0).clamp(0.0, 1023.0)) as u16;
        }
    }
    for fila in 0..ch {
        for col in 0..cw {
            let (r, g, b) = px(col * 2, fila * 2);
            let yl = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            let ul = (b - yl) / 1.8556;
            let vl = (r - yl) / 1.5748;
            u[fila * cw + col] = ((ul * 896.0 + 512.0).clamp(0.0, 1023.0)) as u16;
            v[fila * cw + col] = ((vl * 896.0 + 512.0).clamp(0.0, 1023.0)) as u16;
        }
    }
    Some(Fotograma { y, u, v, w: w as u32, h: h as u32, pts: 0.0 })
}
