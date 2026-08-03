//! UNA FOTO FIJA COMO FUENTE DEL MOTOR (PENDIENTE §4bis.10 y §6).
//!
//! Si la bobina llevaba una foto o un rótulo, el revelado **caía al camino
//! viejo**: cortar con ffmpeg, pasar el look pieza a pieza y concatenar. Eso
//! funciona, pero es tres veces más lento, y una bobina con una sola tarjeta
//! de título ya pagaba el precio entero.
//!
//! Aquí una imagen se convierte UNA vez a los mismos dos planos que entrega el
//! decodificador por hardware —Y en 16 bits y UV entrelazado en 16+16, con el
//! código de 10 bits alineado arriba, que es justo lo que leen `grade_bi.wgsl`
//! y `chain.metal`—. A partir de ahí el motor la trata como cualquier otra
//! fuente: una textura residente que no se vuelve a tocar en todo el revelado.
//!
//! Los planos salen **BT.709 limited**, la misma convención que el resto del
//! taller: `Y = (código − 64) / 876` y `U,V = (código − 512) / 896`.

use std::path::Path;

/// ¿es esto una foto (o un rótulo, que es un PNG)? La decisión vive en
/// `plan.rs`, que es el fichero que comparten todos los motores: si aquí
/// dijera otra cosa, el plan y el motor no estarían de acuerdo.
pub use crate::plan::es_foto;

/// los dos planos de una imagen, listos para subirlos a la GPU:
/// `(ancho, alto, Y, UV)` con Y de `w*h` muestras y UV de `(w/2)*(h/2)` pares.
///
/// El valor guardado es el código de 10 bits **desplazado seis bits a la
/// izquierda**: las texturas son `R16Unorm`/`RG16Unorm` y el shader recupera
/// el código multiplicando por 1023,98. Si se guardara el código a secas, la
/// imagen saldría negra.
pub fn planos(ruta: &Path) -> anyhow::Result<(u32, u32, Vec<u16>, Vec<u16>)> {
    let img = image::open(ruta)?;
    let rgb = img.to_rgb8();
    // el 4:2:0 pide lados pares, y el conform ya se encarga del resto
    let (w, h) = (rgb.width() as usize & !1, rgb.height() as usize & !1);
    anyhow::ensure!(w >= 2 && h >= 2, "la imagen es demasiado pequeña");
    let (cw, ch) = (w / 2, h / 2);
    let mut y = vec![0u16; w * h];
    let mut uv = vec![0u16; cw * ch * 2];
    let luma = |r: f32, g: f32, b: f32| 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let px = |x: usize, fila: usize| -> (f32, f32, f32) {
        let p = rgb.get_pixel(x as u32, fila as u32);
        (p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0)
    };
    let codigo = |v: f32| -> u16 { ((v.clamp(0.0, 1023.0) as u16) & 0x3ff) << 6 };
    for fila in 0..h {
        for col in 0..w {
            let (r, g, b) = px(col, fila);
            y[fila * w + col] = codigo(luma(r, g, b) * 876.0 + 64.0);
        }
    }
    // el croma se promedia en el bloque 2×2: muestrear solo la esquina deja
    // bordes de color en las diagonales
    for fila in 0..ch {
        for col in 0..cw {
            let (mut su, mut sv) = (0.0f32, 0.0f32);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (r, g, b) = px(col * 2 + dx, fila * 2 + dy);
                let yl = luma(r, g, b);
                su += (b - yl) / 1.8556;
                sv += (r - yl) / 1.5748;
            }
            let k = (fila * cw + col) * 2;
            uv[k] = codigo(su / 4.0 * 896.0 + 512.0);
            uv[k + 1] = codigo(sv / 4.0 * 896.0 + 512.0);
        }
    }
    Ok((w as u32, h as u32, y, uv))
}

#[cfg(test)]
mod pruebas {
    use super::*;

    /// escribe un PNG de un solo color y comprueba el código que sale
    fn png_de(r: u8, g: u8, b: u8) -> std::path::PathBuf {
        let mut img = image::RgbImage::new(4, 4);
        for p in img.pixels_mut() { *p = image::Rgb([r, g, b]); }
        let ruta = std::env::temp_dir().join(format!("fl_foto_{r}_{g}_{b}.png"));
        img.save(&ruta).unwrap();
        ruta
    }

    #[test]
    fn el_negro_cae_en_64_y_el_blanco_en_940() {
        let (w, h, y, uv) = planos(&png_de(0, 0, 0)).unwrap();
        assert_eq!((w, h), (4, 4));
        // 64 << 6 = 4096
        assert_eq!(y[0] >> 6, 64);
        // sin croma: 512
        assert_eq!(uv[0] >> 6, 512);
        assert_eq!(uv[1] >> 6, 512);
        let (_, _, y, _) = planos(&png_de(255, 255, 255)).unwrap();
        assert_eq!(y[0] >> 6, 940);
    }

    #[test]
    fn el_rojo_tira_del_croma_v() {
        let (_, _, _, uv) = planos(&png_de(255, 0, 0)).unwrap();
        // los valores de manual para el rojo BT.709 limited:
        //   V = (1 − 0,2126) / 1,5748 · 896 + 512 = 960
        //   U = (0 − 0,2126) / 1,8556 · 896 + 512 = 409
        assert_eq!(uv[1] >> 6, 960, "V");
        assert_eq!(uv[0] >> 6, 409, "U");
    }

    #[test]
    fn los_rotulos_tambien_son_fotos() {
        assert!(es_foto(Path::new("titulo_hola.png")));
        assert!(es_foto(Path::new("FOTO.JPG")));
        assert!(!es_foto(Path::new("clip.mp4")));
    }
}

/// LA FOTO DE UNA CAPA, en RGBA de 8 bits con su alfa (CAPAS §5). La versión
/// planar de arriba tira el alfa —el camino base no lo necesita—; una capa
/// vive de él: es lo que deja ver el fotograma de abajo.
pub fn rgba(ruta: &std::path::Path) -> anyhow::Result<(u32, u32, Vec<u8>)> {
    let img = image::open(ruta)?.to_rgba8();
    let (w, h) = (img.width(), img.height());
    Ok((w, h, img.into_raw()))
}
