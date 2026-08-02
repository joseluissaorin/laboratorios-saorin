//! EL LOOK, TRADUCIDO EN VEZ DE COPIADO (MOTOR §8, plan B).
//!
//! La cadena fílmica estaba escrita dos veces: en WGSL (la preview y el motor
//! de Windows) y en MSL (el motor del Mac). Dos sitios donde arreglar el mismo
//! fallo y dos sitios donde divergir — y **habían divergido**: medido, 47 dB
//! entre el máster del Mac y el mismo fotograma por la cadena WGSL. O sea que
//! la promesa de la casa, «lo que ves es lo que sale», no se cumplía.
//!
//! Aquí `naga` (que ya viene con wgpu) traduce el WGSL a Metal en el build.
//! Una sola fuente de verdad; el Mac deja de tener su copia.
//!
//! El resultado se compila junto al `chain.metal` de siempre y se elige con
//! `FL_LOOK=wgsl`: cambiar el look del máster es decisión del autor, no del
//! compilador, así que el camino viejo sigue siendo el de por defecto hasta
//! que él vea las dos imágenes.

use std::path::Path;

fn main() {
    let raiz = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/src/shaders");
    // FICHERO A FICHERO, no la carpeta. En macOS tocar un fichero NO cambia
    // la fecha del directorio que lo contiene: con `rerun-if-changed` sobre
    // la carpeta, cargo daba por bueno el Metal traducido de la vez anterior
    // y el motor seguía revelando con el shader viejo. Dos tardes de
    // mediciones contradictorias salieron de aquí.
    println!("cargo:rerun-if-changed={}", raiz.display());
    for e in std::fs::read_dir(&raiz).into_iter().flatten().flatten() {
        println!("cargo:rerun-if-changed={}", e.path().display());
    }
    let salida = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());

    // Cada shader del taller trae su propio vértice, así que la entrada del
    // fragmento y la salida del vértice casan por construcción y se traducen
    // sueltos. Uno por biblioteca: los nombres de entrada (`vs_main`,
    // `fs_main`) se repiten y no pueden convivir en el mismo Metal.
    for nombre in ["comp", "down", "blur", "accum", "grade_bi"] {
        let f = raiz.join(format!("{nombre}.wgsl"));
        let fuente = std::fs::read_to_string(&f).unwrap_or_default();
        let msl = if fuente.is_empty() {
            println!("cargo:warning=falta {}", f.display());
            String::new()
        } else {
            match traduce(&fuente) {
                Ok(s) => s,
                Err(e) => {
                    println!("cargo:warning=«{nombre}» no se pudo traducir a Metal: {e}");
                    String::new()
                }
            }
        };
        std::fs::write(salida.join(format!("look_{nombre}.metal")), msl).unwrap();
    }
}

fn traduce(fuente: &str) -> Result<String, String> {
    use naga::back::msl;
    let modulo = naga::front::wgsl::parse_str(fuente)
        .map_err(|e| e.emit_to_string(fuente))?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&modulo).map_err(|e| format!("{e:?}"))?;

    // EL REPARTO SE DEDUCE DEL PROPIO SHADER, no se fija a mano: cada pase
    // usa lo suyo (el `down` tiene un muestreador donde el `comp` tiene una
    // textura) y con un mapa fijo naga se salta en silencio los puntos de
    // entrada que no le cuadran — el motor compila y luego no encuentra
    // `fs_main`. Se recorren los globales en orden de binding y se numeran
    // por clase, que es justo el convenio de `metal_pipe`: búfer 0, texturas
    // 0,1,2… y muestreadores 0,1…
    let mut globales: Vec<(u32, &naga::TypeInner)> = modulo.global_variables
        .iter()
        .filter_map(|(_, v)| {
            let b = v.binding.as_ref()?;
            Some((b.binding, &modulo.types[v.ty].inner))
        })
        .collect();
    globales.sort_by_key(|(b, _)| *b);

    let mut rec = msl::EntryPointResources::default();
    let (mut n_tex, mut n_samp, mut n_buf) = (0u8, 0u8, 0u8);
    for (b, inner) in globales {
        let mut t = msl::BindTarget::default();
        match inner {
            naga::TypeInner::Image { .. } => { t.texture = Some(n_tex); n_tex += 1; }
            naga::TypeInner::Sampler { .. } => {
                t.sampler = Some(msl::BindSamplerTarget::Resource(n_samp)); n_samp += 1;
            }
            _ => { t.buffer = Some(n_buf); n_buf += 1; }
        }
        rec.resources.insert(naga::ResourceBinding { group: 0, binding: b }, t);
    }

    let mut mapa = msl::EntryPointResourceMap::default();
    mapa.insert("fs_main".into(), rec.clone());
    mapa.insert("vs_main".into(), rec);

    let opts = msl::Options {
        lang_version: (2, 2),
        per_entry_point_map: mapa,
        inline_samplers: vec![],
        spirv_cross_compatibility: false,
        fake_missing_bindings: false,
        bounds_check_policies: Default::default(),
        zero_initialize_workgroup_memory: false,
        force_loop_bounding: false,
    };
    let (s, _) = msl::write_string(&modulo, &info, &opts, &msl::PipelineOptions::default())
        .map_err(|e| format!("{e:?}"))?;
    Ok(s)
}
