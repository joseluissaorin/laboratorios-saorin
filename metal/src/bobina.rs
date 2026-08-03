//! EL REVELADO DE UNA BOBINA ENTERA, de un tirón (MOTOR §0).
//!
//! Antes: cinco procesos pasándose ficheros comprimidos. ffmpeg cortaba cada
//! pieza (decodificar + re-codificar el material entero), el motor le pasaba
//! el look, otro ffmpeg re-codificaba el máster COMPLETO para meter los
//! fundidos, y un tercero concatenaba. El material se comprimía tres veces y
//! la mitad del tiempo se iba en la fase de corte.
//!
//! Ahora: se compila la bobina a una tabla de renglones (uno por fotograma de
//! salida) y se recorre. Cada renglón dice de qué fuente sale el fotograma,
//! en qué segundo, con qué receta y con cuánto peso se encadena con el
//! siguiente. **El corte y los fundidos dejan de ser fases.**

use crate::encode_vt::VtEncoder;
use crate::fuente::{Cuadro, Fuente};
use crate::metal_pipe::*;
use crate::plan::{self, Plan, NINGUNA};
use crate::vt_ffi::*;
use anyhow::Result;
use objc::{msg_send, sel, sel_impl};
use std::collections::HashMap;
use std::time::Instant;

/// lo que hace falta saber de cada fuente para revelarla
struct Puesto {
    cine: Option<Fuente>,          // None = hueco (negro) o foto
    /// UNA FOTO O UN RÓTULO, subido una vez y residente (PENDIENTE §4bis.10):
    /// no hay nada que decodificar, así que la bobina con fotos deja de caer
    /// al camino viejo (que funcionaba, pero era tres veces más lento).
    foto: Option<(metal::Texture, metal::Texture)>,
    /// LA CAPA RGBA residente (CAPAS §5): rótulos y fotos de la capa, con su
    /// alfa. Sólo la llevan las fuentes con `capa` y `foto`.
    capa_rgba: Option<metal::Texture>,
    grade: GradeParams,
    comp: CompParams,
    shutter: f32,
    weave: f32,
    lut_a: metal::Texture,
    lut_b: metal::Texture,
}

pub fn revela(plan: &Plan, luts: &mut dyn FnMut(Option<&str>, &str) -> (u64, Vec<f32>),
              prefs_f: &dyn Fn(&serde_json::Value, &str, f64) -> f32) -> Result<()> {
    let t0 = Instant::now();
    let gpu = Gpu::new();
    let pipes = Pipelines::new(&gpu.device);
    let (pw, ph) = (plan.w, plan.h);
    eprintln!("🎞  bobina: {} fotograma(s) · {} fuente(s) · {pw}×{ph} @ {:.3} fps",
              plan.renglones.len(), plan.fuentes.len(), plan.fps);

    // las gelatinas, UNA VEZ y compartidas: dos clips con la misma receta no
    // vuelven a subir la LUT (MOTOR §5 — en la preview esto costaba 750 ms
    // por fotograma cuando se recompilaba)
    let mut cache_lut: HashMap<String, metal::Texture> = HashMap::new();
    let mut pide_lut = |n: Option<&str>, ranura: &str, gpu: &Gpu| -> metal::Texture {
        let clave = format!("{ranura}:{}", n.unwrap_or(""));
        if let Some(t) = cache_lut.get(&clave) { return t.clone(); }
        let (k, datos) = luts(n, ranura);
        let t = make_3d_lut(&gpu.device, k, &datos);
        cache_lut.insert(clave, t.clone());
        t
    };

    let grain = make_grain(&gpu, std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../app/ui/assets/grain.bin").as_path());

    // abrir cada fuente y preparar su receta
    let mut puestos: Vec<Puesto> = Vec::new();
    for f in &plan.fuentes {
        let lut_a = pide_lut(f.lut_in.as_deref(), "entrada", &gpu);
        let lut_b = pide_lut(f.lut.as_deref(), "color", &gpu);
        let (cine, foto, capa_rgba) = if f.hueco {
            (None, None, None)
        } else if f.foto && f.capa {
            // UNA CAPA con foto o rótulo: RGBA residente, CON su alfa —
            // que es de lo que vive una capa (CAPAS §5)
            let t = crate::metal_pipe::capa_rgba(&gpu.device,
                                                 std::path::Path::new(&f.fichero))
                .map_err(|e| anyhow::anyhow!("{}: {e}", f.fichero))?;
            eprintln!("   capa RGBA residente: {} ({}×{})",
                      f.fichero, t.width(), t.height());
            (None, None, Some(t))
        } else if f.foto {
            let (ty, tuv, fw, fh) = foto_residente(&gpu.device,
                                                   std::path::Path::new(&f.fichero))
                .map_err(|e| anyhow::anyhow!("{}: {e}", f.fichero))?;
            eprintln!("   foto residente: {} ({fw}×{fh})", f.fichero);
            (None, Some((ty, tuv)), None)
        } else {
            (Some(Fuente::abre(std::path::Path::new(&f.fichero))
                .map_err(|e| anyhow::anyhow!("{}: {e}", f.fichero))?), None, None)
        };
        let (sw, sh) = match (&cine, &foto, &capa_rgba) {
            (Some(c), _, _) => { let i = c.info(); (i.0 as f32, i.1 as f32) }
            (_, Some((ty, _)), _) => (ty.width() as f32, ty.height() as f32),
            (_, _, Some(t)) => (t.width() as f32, t.height() as f32),
            _ => (pw as f32, ph as f32),
        };
        let (a, b, paso) = plan::matriz_de(f, sw, sh, pw as f32, ph as f32);
        let p = &f.prefs;
        let mut g = GradeParams {
            src_w: sw, src_h: sh, enc_a: a, enc_b: b, paso,
            lut_a_on: f.lut_in.is_some() as u32, lut_b_on: f.lut.is_some() as u32,
            gain: prefs_f(p, "gain", 0.0), push_pull: prefs_f(p, "pushPull", 0.0),
            compress: prefs_f(p, "compImpact", 0.0),
            compress_wp: prefs_f(p, "compWP", 1.0),
            compress_range: prefs_f(p, "compRange", 0.5),
            // EL FILTRO ND, deshecho antes de la gelatina de entrada
            nd: [prefs_f(p, "ndFix", 0.0).clamp(0.0, 1.5),
                 prefs_f(p, "ndTint", 0.0).clamp(-1.0, 1.0),
                 prefs_f(p, "ndShadow", 2.2).clamp(0.2, 8.0),
                 prefs_f(p, "ndGuard", 0.42).clamp(0.02, 1.0)],
            matriz: match p["matriz"].as_str().unwrap_or("709") {
                "2020" | "bt2020" => 1, "601" | "bt601" => 2, _ => 0 },
            // la semántica de capa (CAPAS §4): 2 = vídeo capa, 3 = RGBA capa
            src_mode: if f.capa { if f.foto { 3 } else { 2 } } else { 0 },
            ..Default::default()
        };
        g.lut_na = lut_a.width() as u32;
        g.lut_nb = lut_b.width() as u32;
        puestos.push(Puesto {
            cine, foto, capa_rgba, grade: g, comp: comp_de(p, prefs_f),
            shutter: prefs_f(p, "shutter", 0.0), weave: prefs_f(p, "weave", 0.0),
            lut_a, lut_b,
        });
    }

    // el codificador, al lienzo del proyecto. El sonido NO viene de aquí: la
    // bobina puede tener veinte fuentes y su mezcla la hornea el taller con
    // ffmpeg en una sola pasada al final (MOTOR §11).
    let enc = VtEncoder::new(pw, ph, &plan.salida, "", plan.fps, plan.bitrate, 0.0)?;
    let pool = enc.pool;
    let tex_cache = cache_de_texturas(&gpu);
    let sin_imagen = vacia(&gpu);   // una vez, no por fotograma

    let mut r = Renderer {
        gpu: gpu.clone(), pipes: pipes.clone(),
        targets: TargetSet::new(&gpu.device, pw as u64, ph as u64, FMT_INTERMEDIO),
        grade_params: puestos[0].grade, comp_params: puestos[0].comp,
        lut_a: puestos[0].lut_a.clone(), lut_b: puestos[0].lut_b.clone(),
        grain: grain.clone(), shutter: puestos[0].shutter,
        weave_amount: puestos[0].weave,
        blanco: crate::metal_pipe::tex_blanca(&gpu.device),
    };

    // el hilo del codificador: la GPU y VideoToolbox trabajan a la vez
    struct Trabajo(metal::CommandBuffer, CVPixelBufferRef, i64, Vec<Cuadro>,
                   Vec<(metal::Texture, metal::Texture)>);
    unsafe impl Send for Trabajo {}
    // LA PROFUNDIDAD DEL ANILLO, en bytes y no en fotogramas. Cuánto puede ir
    // por delante el decodificador antes de que la memoria mande. Medido sobre
    // una bobina 4K60 con encadenado:
    //
    //    6 fotogramas → 157 fps      24 → 162      48 → 176
    //   12            → 155          32 → 174      64 → 176 (satura)
    //
    // Cuanto más hondo, menos espera el revelado al codificador… y más espera a
    // las fuentes: a partir de 48 el decodificador es el único límite. Pero un
    // fotograma 4K en x420 son 25 MB, así que 48 serían más de un giga. Se fija
    // un presupuesto y salen los fotogramas que quepan — en 1080p sale hondo
    // gratis, en 4K sale prudente.
    const PRESUPUESTO: usize = 1_500_000_000;
    let por_cuadro = (pw as usize * ph as usize * 3).max(1);
    let hondo: usize = std::env::var("FL_ANILLO").ok().and_then(|v| v.parse().ok())
        .unwrap_or_else(|| (PRESUPUESTO / (por_cuadro * 3)).clamp(4, 48));
    eprintln!("   anillo: {hondo} fotograma(s) por delante ({} MB)",
              hondo * por_cuadro * 3 / 1_000_000);
    let (tx, rx) = std::sync::mpsc::sync_channel::<Trabajo>(hondo);
    let fps = plan.fps;
    let hilo = std::thread::spawn(move || {
        let mut n = 0i64;
        for Trabajo(cmd, pb, idx, cuadros, texturas) in rx {
            cmd.wait_until_completed();
            // los fotogramas de entrada se sueltan AQUÍ: antes de que la GPU
            // termine, VideoToolbox recicla el búfer y el look acaba revelando
            // otra imagen (el fallo que tenía el motor de un solo clip)
            drop(texturas); drop(cuadros);
            enc.encode(pb, idx, fps);
            unsafe { CFRelease(pb as *mut std::ffi::c_void) };
            n += 1;
        }
        enc.finish(n);
        n
    });

    // ── EL HILO DE LAS FUENTES (MOTOR §6) ──────────────────────────────
    // Decodificar y componer se turnaban: la GPU esperaba al decodificador y
    // el decodificador a la GPU. Aquí los fotogramas se van sacando POR
    // DELANTE, en su propio hilo, y llegan por una cola acotada — que es
    // además la contrapresión: si el revelado se retrasa, el decodificador
    // se para solo en vez de comerse la memoria.
    //
    // Y es lo que hace baratas las juntas: durante un encadenado hacen falta
    // dos fuentes a la vez, pero el hilo lleva ventaja acumulada del clip que
    // se está yendo, así que el pico se amortiza en vez de dar un tirón.
    struct Servido { ren: crate::plan::Renglon, a: Option<Cuadro>, b: Option<Cuadro>,
                     c: Option<Cuadro>, d: Option<Cuadro> }
    unsafe impl Send for Servido {}
    let total = plan.renglones.len();
    let renglones = plan.renglones.clone();
    let (ftx, frx) = std::sync::mpsc::sync_channel::<Servido>(hondo * 2);
    let mut cines: Vec<Option<Fuente>> = puestos.iter_mut()
        .map(|p| p.cine.take()).collect();
    let fuentes = std::thread::spawn(move || {
        for ren in renglones.iter() {
            let a = cines[ren.fuente_a as usize].as_mut().and_then(|c| c.en(ren.t_a));
            let b = if ren.fuente_b != NINGUNA {
                cines[ren.fuente_b as usize].as_mut().and_then(|c| c.en(ren.t_b))
            } else { None };
            // LA CAPA: si es vídeo, su fotograma; una foto no decodifica nada
            let c = if ren.fuente_c != NINGUNA {
                cines[ren.fuente_c as usize].as_mut().and_then(|x| x.en(ren.t_c))
            } else { None };
            let d = if ren.fuente_d != NINGUNA {
                cines[ren.fuente_d as usize].as_mut().and_then(|x| x.en(ren.t_d))
            } else { None };
            if ftx.send(Servido { ren: *ren, a, b, c, d }).is_err() { break; }
        }
    });

    // LA CARRERILLA: los primeros renglones se revelan y se tiran. Dejan la
    // historia del obturador con su arrastre ya formado, para que el tramo no
    // empiece con un escalón (MOTOR §7).
    let saltar: usize = std::env::var("FL_SALTAR").ok()
        .and_then(|v| v.parse().ok()).unwrap_or(0);
    if saltar > 0 { eprintln!("   carrerilla: {saltar} fotograma(s) que no se escriben"); }
    let mut hechos = 0usize;
    // el desglose que dice quién manda (MOTOR §9, contrato de medición)
    let (mut t_esp, mut t_gpu, mut t_env) = (0.0f64, 0.0f64, 0.0f64);
    let mut reloj = Instant::now();
    let mut frx = frx.into_iter();
    while let Some(servido) = { let r = frx.next(); t_esp += reloj.elapsed().as_secs_f64(); r } {
        let t = hechos;
        reloj = Instant::now();
        let ren = &servido.ren;
        let cola = gpu.queue.clone();
        let cmd = cola.new_command_buffer();
        let mut vivos: Vec<Cuadro> = Vec::new();
        let mut texs: Vec<(metal::Texture, metal::Texture)> = Vec::new();

        // ── el lado A ──
        let ia = ren.fuente_a as usize;
        // EL ARRASTRE DEL OBTURADOR NO CRUZA EL EMPALME (`ren.corte`). Un
        // corte seco no tiene continuidad de luz: el fotograma que entra se
        // expone solo. Sin esto, el primero del plano nuevo llevaba encima un
        // 14 % del que se iba —el valor de la casa— y se veía el plano
        // anterior uno o dos fotogramas después de haber cortado. Peor aún:
        // el arrastre SÍ se cortaba en el primer fotograma de cada tramo, así
        // que el máster salía distinto según dónde cayeran los troceados de la
        // caché. En un encadenado no se toca: ahí las dos imágenes conviven.
        let mut hay = false;
        if let Some((ty, tuv)) = puestos[ia].foto.clone() {
            // LA FOTO NO SE DECODIFICA: la misma textura, fotograma tras
            // fotograma, con su encuadre y su receta como cualquier otra
            let mut g = puestos[ia].grade;
            g.peso = 1.0;
            g.pad0 = puestos[ia].shutter;
            g.pad1 = if t == 0 || ren.corte { 1.0 } else { 0.0 };
            r.revela_en(cmd, &ty, &tuv, &g, &puestos[ia].lut_a.clone(),
                        &puestos[ia].lut_b.clone());
            hay = true;
        } else if let Some(c) = servido.a {
            if let Some((ty, tuv)) = importa(&tex_cache, c.pb) {
                let mut g = puestos[ia].grade;
                g.peso = 1.0;
                g.pad0 = puestos[ia].shutter;          // el obturador, fundido
                g.pad1 = if t == 0 || ren.corte { 1.0 } else { 0.0 };
                r.revela_en(cmd, &ty, &tuv, &g, &puestos[ia].lut_a.clone(),
                            &puestos[ia].lut_b.clone());
                texs.push((ty, tuv));
                hay = true;
            }
            vivos.push(c);
        }
        if !hay {
            // hueco (o fuente agotada): negro. Un `peso` de 1 con la fuente
            // fuera de encuadre pinta negro sin decodificar nada.
            let mut g = puestos[ia].grade;
            g.peso = 1.0;
            g.pad0 = puestos[ia].shutter;
            g.pad1 = if t == 0 || ren.corte { 1.0 } else { 0.0 };
            // todo fuera del cuadro → letterbox total, sin decodificar nada
            g.enc_a = [1.0, 0.0, 0.0, 1.0];
            g.enc_b = [9.0, 9.0, 1.0, 1.0];
            r.revela_en(cmd, &sin_imagen.0, &sin_imagen.1, &g,
                        &puestos[ia].lut_a.clone(), &puestos[ia].lut_b.clone());
        }

        // ── el lado B: LA JUNTA. Un dibujo más con su peso, y ya está ──
        let manda = if ren.fuente_b != NINGUNA && ren.peso_b > 0.5 {
            ren.fuente_b as usize
        } else { ia };
        if ren.fuente_b != NINGUNA {
            let ib = ren.fuente_b as usize;
            if let Some((ty, tuv)) = puestos[ib].foto.clone() {
                let mut g = puestos[ib].grade;
                g.peso = ren.peso_b;
                g.pad0 = puestos[ib].shutter;
                g.pad1 = if t == 0 || ren.corte { 1.0 } else { 0.0 };
                r.revela_en(cmd, &ty, &tuv, &g, &puestos[ib].lut_a.clone(),
                            &puestos[ib].lut_b.clone());
            } else if let Some(c) = servido.b {
                if let Some((ty, tuv)) = importa(&tex_cache, c.pb) {
                    let mut g = puestos[ib].grade;
                    g.peso = ren.peso_b;
                    g.pad0 = puestos[ib].shutter;
                    g.pad1 = if t == 0 || ren.corte { 1.0 } else { 0.0 };
                    r.revela_en(cmd, &ty, &tuv, &g, &puestos[ib].lut_a.clone(),
                                &puestos[ib].lut_b.clone());
                    texs.push((ty, tuv));
                }
                vivos.push(c);
            }
        }

        // ── LA CAPA (CAPAS §5): un dibujo más, encima de A y B ───────
        // `peso = alfa_c` (los fundidos de la capa); si es RGBA el alfa por
        // píxel lo multiplica el shader. La trampa del destino: con
        // obturador, A y B escriben en h_b y la capa tiene que caer AHÍ
        // MISMO — así que lleva el pad0 del que manda (elige destino) con
        // pad1 = 1 (no arrastra historia: un rótulo no es luz de escena).
        for (fk, tk, ak, cua) in [
            (ren.fuente_c, ren.t_c, ren.alfa_c, servido.c),
            (ren.fuente_d, ren.t_d, ren.alfa_d, servido.d),
        ] {
            let _ = tk;
            if fk == NINGUNA { continue }
            let ic = fk as usize;
            let mut g = puestos[ic].grade;
            g.peso = ak;
            g.pad0 = puestos[manda].shutter;
            g.pad1 = 1.0;
            if let Some(t_rgba) = puestos[ic].capa_rgba.clone() {
                r.revela_capa(cmd, &sin_imagen.0, &sin_imagen.1, &g,
                              &puestos[ic].lut_a.clone(),
                              &puestos[ic].lut_b.clone(), Some(&t_rgba));
            } else if let Some(cua) = cua {
                if let Some((ty, tuv)) = importa(&tex_cache, cua.pb) {
                    r.revela_en(cmd, &ty, &tuv, &g,
                                &puestos[ic].lut_a.clone(),
                                &puestos[ic].lut_b.clone());
                    texs.push((ty, tuv));
                }
                vivos.push(cua);
            }
        }

        // la historia nueva pasa a ser la vieja: UNA vez por fotograma, ya
        // reveladas las dos fuentes de la junta
        r.cierra_obturador(puestos[manda].shutter > 0.001);
        // la receta del pase caro es la del clip que manda en este fotograma
        r.comp_params = puestos[manda].comp;
        r.shutter = puestos[manda].shutter;
        r.weave_amount = puestos[manda].weave;
        // el fundido a negro/blanco de la bobina: una constante, sin segunda
        // fuente y sin segundo decodificador
        r.comp_params.fundido = ren.nivel_color;
        r.comp_params.fundido_color = ren.color_fijo;

        let Some(pb) = crate::encode_vt::alloc_from_pool(pool) else {
            anyhow::bail!("el pool del codificador se quedó sin búferes");
        };
        let Some((py, puv)) = importa(&tex_cache, pb) else {
            anyhow::bail!("no pude importar el fotograma de salida");
        };
        r.compone(cmd, t, plan.fps, Some((&py, &puv)));
        cmd.commit();
        texs.push((py, puv));
        t_gpu += reloj.elapsed().as_secs_f64();
        reloj = Instant::now();
        if t >= saltar {
            tx.send(Trabajo(cmd.to_owned(), pb, (t - saltar) as i64, vivos, texs))
              .map_err(|_| anyhow::anyhow!("el hilo del codificador se cayó"))?;
        } else {
            // de carrerilla: la GPU sí trabaja (el obturador se calienta),
            // pero el fotograma no llega al codificador
            cmd.wait_until_completed();
            drop(texs); drop(vivos);
            unsafe { CFRelease(pb as *mut std::ffi::c_void) };
        }
        t_env += reloj.elapsed().as_secs_f64();
        reloj = Instant::now();
        hechos += 1;
        if hechos % 50 == 0 {
            eprint!("\r  {hechos}/{total} · {:.1} fps  ", hechos as f64 / t0.elapsed().as_secs_f64());
        }
    }
    drop(tx);
    let _ = fuentes.join();
    let escritos = hilo.join().map_err(|_| anyhow::anyhow!("hilo del codificador"))?;
    let el = t0.elapsed().as_secs_f64();
    let n = escritos.max(1) as f64;
    eprintln!("\n✅ bobina revelada: {escritos} fotogramas en {el:.1}s = {:.1} fps",
              escritos as f64 / el.max(0.001));
    eprintln!("   esperando fuentes {:.2} · componer {:.2} · esperando al codificador {:.2} ms/fotograma",
              t_esp * 1e3 / n, t_gpu * 1e3 / n, t_env * 1e3 / n);
    anyhow::ensure!(escritos as usize == total - saltar,
                    "faltan fotogramas: {escritos} de {}", total - saltar);
    Ok(())
}

fn comp_de(p: &serde_json::Value,
           f: &dyn Fn(&serde_json::Value, &str, f64) -> f32) -> CompParams {
    CompParams {
        hal_amount: f(p, "halation", 0.0), hal_hue: f(p, "halHue", 1.0),
        hal_sat: f(p, "halSat", 0.9), hal_thr: f(p, "halThr", 0.5),
        hal_spread: f(p, "halSpread", 0.7), hal_white: f(p, "halWhite", 0.0),
        bloom_amount: f(p, "bloom", 0.0), bloom_thr: f(p, "bloomThr", 0.72),
        bloom_warm: f(p, "bloomWarm", 0.15),
        softness: f(p, "softness", 0.0), acutance: f(p, "acutance", 0.0),
        color_sep: f(p, "colorSep", 0.0),
        hue_skew: f(p, "hueSkew", 1.0), crosstalk: f(p, "crosstalk", 0.3),
        subtractive: f(p, "subtractive", 0.6), stock_sat: f(p, "stockSat", 1.0),
        print_: f(p, "print", 0.5),
        grain_amount: f(p, "grain", 0.0), grain_size: f(p, "grainSize", 2.6),
        grain_rough: f(p, "grainRough", 0.35), grain_chroma: f(p, "grainChroma", 0.25),
        grain_defocus: f(p, "grainDefocus", 0.55),
        grain_s: f(p, "grainShadows", 0.8), grain_m: f(p, "grainMids", 1.0),
        grain_h: f(p, "grainHighs", 0.5), grain_r: f(p, "grainRed", 1.0),
        grain_b: f(p, "grainBlue", 1.25), film_res: f(p, "filmRes", 0.5),
        plate_n: 1024.0,
        vig_amount: f(p, "vignette", 0.0), vig_size: f(p, "vigSize", 0.55),
        vig_round: f(p, "vigRound", 1.0), vig_cx: f(p, "vigCX", 0.5),
        vig_cy: f(p, "vigCY", 0.5), ca: f(p, "chroma", 0.0),
        dust: f(p, "dust", 0.0), flicker: f(p, "flicker", 0.0), flicker_rate: 0.5,
        breath: f(p, "breath", 0.0), breath_rate: f(p, "breathRate", 0.5),
        frame_inset: f(p, "frameInset", 0.0), frame_corner: f(p, "frameCorner", 40.0),
        frame_wobble: f(p, "frameWobble", 0.5),
        wipe: 1.0, weave_rot: f(p, "weaveRot", 0.3),
        ..Default::default()
    }
}

fn cache_de_texturas(gpu: &Gpu) -> CVMetalTextureCacheRef {
    let mut c: CVMetalTextureCacheRef = std::ptr::null_mut();
    let dev: *mut std::ffi::c_void = unsafe { msg_send![&*gpu.device, self] };
    unsafe { CVMetalTextureCacheCreate(std::ptr::null(), std::ptr::null(), dev,
                                       std::ptr::null(), &mut c) };
    c
}

/// los dos planos de un CVPixelBuffer como texturas, sin copiar un byte
fn importa(cache: &CVMetalTextureCacheRef, pb: CVPixelBufferRef)
           -> Option<(metal::Texture, metal::Texture)> {
    let w = unsafe { CVPixelBufferGetWidth(pb) };
    let h = unsafe { CVPixelBufferGetHeight(pb) };
    let uno = |plano: usize, fmt: u64, pw: usize, ph: usize| -> Option<metal::Texture> {
        let mut ct: CVMetalTextureRef = std::ptr::null_mut();
        let st = unsafe {
            CVMetalTextureCacheCreateTextureFromImage(std::ptr::null(), *cache, pb,
                                                      std::ptr::null(), fmt, pw, ph, plano, &mut ct)
        };
        if st != 0 || ct.is_null() { return None; }
        let raw = unsafe { CVMetalTextureGetTexture(ct) };
        if raw.is_null() { unsafe { CFRelease(ct as *mut std::ffi::c_void) }; return None; }
        unsafe { core_foundation::base::CFRetain(raw as *const std::ffi::c_void) };
        let t: metal::Texture = unsafe { metal::foreign_types::ForeignType::from_ptr(raw as *mut _) };
        unsafe { CFRelease(ct as *mut std::ffi::c_void) };
        Some(t)
    };
    match (uno(0, metal::MTLPixelFormat::R16Unorm as u64, w, h),
           uno(1, metal::MTLPixelFormat::RG16Unorm as u64, w / 2, h / 2)) {
        (Some(a), Some(b)) => Some((a, b)),
        _ => None,
    }
}

/// un par de texturas mínimas para los renglones que no leen imagen (huecos)
fn vacia(gpu: &Gpu) -> (metal::Texture, metal::Texture) {
    use metal::{MTLPixelFormat, MTLTextureUsage, MTLStorageMode, TextureDescriptor};
    let mk = |fmt: MTLPixelFormat| {
        let d = TextureDescriptor::new();
        d.set_width(2); d.set_height(2);
        d.set_pixel_format(fmt);
        d.set_usage(MTLTextureUsage::ShaderRead);
        d.set_storage_mode(MTLStorageMode::Private);
        gpu.device.new_texture(&d)
    };
    (mk(MTLPixelFormat::R16Unorm), mk(MTLPixelFormat::RG16Unorm))
}
