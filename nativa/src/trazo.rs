//! El trazo a pulso — la primitiva universal del zine (NORTE §1.3).
//!
//! Toda línea de la interfaz pasa por aquí: separadores, subrayados, cajas,
//! flechas, círculos de rotulador. Jitter DETERMINISTA (semilla = id del
//! elemento) para que la tinta no «hierva» entre frames; grosor variable
//! (presión) y un punto de sangrado en los extremos.

use crate::ui::Dibujo;

fn hash(n: u32) -> f32 {
    let mut x = n.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 13;
    x = x.wrapping_mul(0xC2B2_AE35);
    x ^= x >> 16;
    (x & 0xFF_FFFF) as f32 / 16_777_216.0
}

/// ruido 1D suave en [-1, 1] (lattice + coseno)
fn ruido(semilla: u32, t: f32) -> f32 {
    let i = t.floor();
    let f = t - i;
    let a = hash(semilla.wrapping_add((i as i64 as u32).wrapping_mul(0x2C1B_3C6D)));
    let b = hash(semilla.wrapping_add(((i as i64 + 1) as u32).wrapping_mul(0x2C1B_3C6D)));
    let s = (1.0 - (f * std::f32::consts::PI).cos()) * 0.5;
    (a + (b - a) * s) * 2.0 - 1.0
}

/// semilla estable a partir de una posición (para líneas sin id propio)
pub fn semilla_de(x: f32, y: f32) -> u32 {
    (x as i32 as u32).wrapping_mul(73_856_093) ^ (y as i32 as u32).wrapping_mul(19_349_663)
}

/// polilínea a pulso: puntos → cinta de triángulos soldada, con jitter
/// perpendicular, presión variable y sangrado en los extremos
pub fn pulso(d: &mut Dibujo, pts: &[(f32, f32)], g: f32, c: [f32; 4], semilla: u32) {
    if pts.len() < 2 {
        return;
    }
    // longitud total → nº de muestras (una cada ~6 px)
    let mut largo = 0.0f32;
    for w in pts.windows(2) {
        largo += ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt();
    }
    let n = ((largo / 6.0) as usize).clamp(2, 260);
    let amp = (g * 0.9).clamp(0.7, 2.2); // amplitud del pulso
    let mut prev: Option<([f32; 2], [f32; 2])> = None;
    let punto_en = |t: f32| -> (f32, f32, f32, f32) {
        // t ∈ [0,1] sobre la polilínea → punto + tangente
        let objetivo = t * largo;
        let mut acc = 0.0;
        for w in pts.windows(2) {
            let l = ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt();
            if acc + l >= objetivo || l <= 0.0001 {
                let f = ((objetivo - acc) / l.max(0.0001)).clamp(0.0, 1.0);
                return (
                    w[0].0 + (w[1].0 - w[0].0) * f,
                    w[0].1 + (w[1].1 - w[0].1) * f,
                    (w[1].0 - w[0].0) / l.max(0.0001),
                    (w[1].1 - w[0].1) / l.max(0.0001),
                );
            }
            acc += l;
        }
        let w = [pts[pts.len() - 2], pts[pts.len() - 1]];
        let l = ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt().max(0.0001);
        (w[1].0, w[1].1, (w[1].0 - w[0].0) / l, (w[1].1 - w[0].1) / l)
    };
    for k in 0..=n {
        let t = k as f32 / n as f32;
        let (x, y, tx, ty) = punto_en(t);
        let (nx, ny) = (-ty, tx);
        // jitter perpendicular + presión (más fina a mitad de trazo)
        let woble = ruido(semilla, t * largo / 26.0) * amp;
        let presion = 0.78 + 0.5 * (ruido(semilla ^ 0x5bd1, t * largo / 40.0) * 0.5 + 0.5);
        // sangrado: los extremos un pelín más gordos (la tinta se posa)
        let punta = 1.0 + 0.65 * ((-((t * largo).min((1.0 - t) * largo)) / 5.0).exp());
        let gg = g * presion * punta * 0.5;
        let (px, py) = (x + nx * woble, y + ny * woble);
        let izq = [px + nx * gg, py + ny * gg];
        let der = [px - nx * gg, py - ny * gg];
        if let Some((pi, pd)) = prev {
            d.tri(pi, izq, der, c);
            d.tri(pi, der, pd, c);
        }
        prev = Some((izq, der));
    }
}

/// línea recta a pulso entre dos puntos
pub fn linea(d: &mut Dibujo, x0: f32, y0: f32, x1: f32, y1: f32, g: f32, c: [f32; 4], semilla: u32) {
    pulso(d, &[(x0, y0), (x1, y1)], g, c, semilla);
}

/// subrayado con caída (la mano baja al final)
pub fn subraya(d: &mut Dibujo, x0: f32, x1: f32, y: f32, g: f32, c: [f32; 4], semilla: u32) {
    pulso(d, &[(x0, y), ((x0 + x1) * 0.5, y + 1.0), (x1, y + 2.2)], g, c, semilla);
}

/// flecha a mano: cuerpo curvado + dos plumas
pub fn flecha(d: &mut Dibujo, x0: f32, y0: f32, x1: f32, y1: f32, g: f32, c: [f32; 4], semilla: u32) {
    let (mx, my) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
    let (dx, dy) = (x1 - x0, y1 - y0);
    let l = (dx * dx + dy * dy).sqrt().max(0.0001);
    let (nx, ny) = (-dy / l, dx / l);
    let curva = l * 0.12 * (hash(semilla) - 0.5) * 2.0;
    pulso(d, &[(x0, y0), (mx + nx * curva, my + ny * curva), (x1, y1)], g, c, semilla);
    let (ux, uy) = (dx / l, dy / l);
    let p = 9.0f32.min(l * 0.3);
    pulso(d, &[(x1 - ux * p + nx * p * 0.6, y1 - uy * p + ny * p * 0.6), (x1, y1)], g, c, semilla ^ 3);
    pulso(d, &[(x1 - ux * p - nx * p * 0.6, y1 - uy * p - ny * p * 0.6), (x1, y1)], g, c, semilla ^ 7);
}

/// elipse de rotulador: no cierra exacto, se solapa un poco (como al rodear)
pub fn circulo(d: &mut Dibujo, cx: f32, cy: f32, rx: f32, ry: f32, g: f32, c: [f32; 4], semilla: u32) {
    let n = 26;
    let mut pts = Vec::with_capacity(n + 4);
    let a0 = hash(semilla) * std::f32::consts::TAU;
    for k in 0..=(n + 2) {
        // 1.08 vueltas: el solape del rotulador
        let a = a0 + k as f32 / n as f32 * std::f32::consts::TAU * 1.08;
        let rr = 1.0 + 0.05 * ruido(semilla, k as f32 * 0.7);
        pts.push((cx + a.cos() * rx * rr, cy + a.sin() * ry * rr));
    }
    pulso(d, &pts, g, c, semilla);
}

/// caja a pulso: cuatro trazos con las esquinas pasadas de largo
pub fn caja(d: &mut Dibujo, x: f32, y: f32, w: f32, h: f32, g: f32, c: [f32; 4], semilla: u32) {
    let s = 2.0; // lo que se pasa el lápiz en cada esquina
    linea(d, x - s, y, x + w + s, y, g, c, semilla);
    linea(d, x + w, y - s, x + w, y + h + s, g, c, semilla ^ 11);
    linea(d, x + w + s, y + h, x - s, y + h, g, c, semilla ^ 23);
    linea(d, x, y + h + s, x, y - s, g, c, semilla ^ 37);
}

/// tachón: zigzag de rotulador sobre un tramo
pub fn tachon(d: &mut Dibujo, x0: f32, x1: f32, y: f32, alto: f32, c: [f32; 4], semilla: u32) {
    let n = (((x1 - x0) / 7.0) as usize).max(2);
    let mut pts = Vec::with_capacity(n);
    for k in 0..n {
        let t = k as f32 / (n - 1) as f32;
        pts.push((x0 + (x1 - x0) * t, y + if k % 2 == 0 { -alto * 0.5 } else { alto * 0.5 }));
    }
    pulso(d, &pts, 1.6, c, semilla);
}

/// llave « { » vertical (para agrupar)
pub fn llave(d: &mut Dibujo, x: f32, y0: f32, y1: f32, g: f32, c: [f32; 4], semilla: u32) {
    let m = (y0 + y1) * 0.5;
    pulso(d, &[(x + 4.0, y0), (x, y0 + 3.0), (x, m - 3.0), (x - 4.0, m), (x, m + 3.0), (x, y1 - 3.0), (x + 4.0, y1)], g, c, semilla);
}
