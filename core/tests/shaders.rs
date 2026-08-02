//! TODOS LOS SHADERS DEL TALLER PASAN EL VALIDADOR.
//!
//! No basta con que naga los traduzca: el motor del Mac compila Metal y se
//! salta la validación de wgpu, así que un shader podía salir bien aquí y
//! tumbar el motor de Windows al crear el módulo. Pasó exactamente eso, y
//! costó la vuelta entera —sincronizar, compilar dos minutos, revelar— para
//! leer un error de tipos. Esto lo dice en un segundo.

#[test]
fn los_shaders_pasan_la_validacion() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/shaders");
    let mut vistos = 0;
    for e in std::fs::read_dir(&dir).expect("carpeta de shaders").flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("wgsl") { continue }
        let fuente = std::fs::read_to_string(&p).expect("leer el shader");
        let nombre = p.file_name().unwrap().to_string_lossy().to_string();
        let modulo = match naga::front::wgsl::parse_str(&fuente) {
            Ok(m) => m,
            Err(err) => panic!("«{nombre}» no compila:\n{}", err.emit_to_string(&fuente)),
        };
        if let Err(err) = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
            .validate(&modulo) {
            panic!("«{nombre}» no pasa la validación:\n{err:?}");
        }
        vistos += 1;
    }
    assert!(vistos >= 8, "sólo se han mirado {vistos} shaders");
}
