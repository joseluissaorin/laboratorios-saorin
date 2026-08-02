//! Cronómetro del proyector en proceso: índice, seek exacto y secuencia.
//! Uso: crono_cine <fichero.mp4> [t_seek]

use filmlook_core::cine::Cine;
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let ruta = std::path::PathBuf::from(args.next().expect("uso: crono_cine <mp4> [t]"));
    let t_seek: f64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(5.0);

    let m = Instant::now();
    let mut cine = Cine::abre(&ruta)?;
    let (w, h, fps, dur) = cine.info();
    println!("índice+sesión: {:.1} ms · {}x{} {:.2} fps {:.1} s", m.elapsed().as_secs_f64() * 1e3, w, h, fps, dur);

    let m = Instant::now();
    let fr = cine.frame_en(t_seek);
    match &fr {
        Some(f) => println!("seek {:.2}s → frame pts {:.3} ({}x{}): {:.1} ms",
                            t_seek, f.pts, f.w, f.h, m.elapsed().as_secs_f64() * 1e3),
        None => println!("seek {:.2}s → NADA en {:.1} ms", t_seek, m.elapsed().as_secs_f64() * 1e3),
    }

    let m = Instant::now();
    let mut n = 0;
    let mut ultimo = 0.0;
    while n < 120 {
        match cine.siguiente() {
            Some(f) => { ultimo = f.pts; n += 1; }
            None => break,
        }
    }
    let s = m.elapsed().as_secs_f64();
    println!("secuencia: {} frames en {:.1} ms ({:.1} fps) · último pts {:.3}", n, s * 1e3, n as f64 / s.max(1e-9), ultimo);

    // seeks aleatorios (el caso del scrub)
    let m = Instant::now();
    let mut peor = 0.0f64;
    for k in 0..20 {
        let t = (k as f64 * 0.37) % dur.max(0.1);
        let mi = Instant::now();
        let _ = cine.frame_en(t);
        peor = peor.max(mi.elapsed().as_secs_f64());
    }
    println!("20 seeks: media {:.1} ms · peor {:.1} ms",
             m.elapsed().as_secs_f64() * 1e3 / 20.0, peor * 1e3);
    Ok(())
}
