//! El atlas de objetos del taller (NORTE §1.4) — coordenadas de
//! assets/doodles.png. DUPLICADAS en tools/hornea_doodles.py: si cambia el
//! layout allí, cambia aquí.

pub type R = (f32, f32, f32, f32);

pub const LATA: R = (0.0, 0.0, 512.0, 512.0);
pub const FOTO_LAB: R = (512.0, 0.0, 336.0, 272.0);
pub const CELO: R = (848.0, 0.0, 176.0, 80.0);
pub const CHINCHETA_ROJA: R = (848.0, 80.0, 88.0, 88.0);
pub const GRAPA: R = (936.0, 80.0, 88.0, 88.0);
pub const CHINCHETA_TINTA: R = (848.0, 168.0, 88.0, 88.0);
pub const CHINCHETA_AMBAR: R = (936.0, 168.0, 88.0, 88.0);
pub const BOTELLA: R = (512.0, 272.0, 168.0, 304.0);
pub const CAJA: R = (680.0, 272.0, 344.0, 168.0);
pub const CUBETA: R = (680.0, 440.0, 344.0, 200.0);
pub const PINZA: R = (512.0, 576.0, 96.0, 208.0);
pub const WASHI: [R; 4] = [
    (0.0, 512.0, 256.0, 56.0),
    (0.0, 568.0, 256.0, 56.0),
    (0.0, 624.0, 256.0, 56.0),
    (0.0, 680.0, 256.0, 56.0),
];

pub fn uv(r: R) -> [f32; 4] {
    [r.0 / 1024.0, r.1 / 1024.0, (r.0 + r.2) / 1024.0, (r.1 + r.3) / 1024.0]
}
