pub mod vt_ffi;
pub mod metal_pipe;
pub mod decode_vt;
pub mod encode_vt;
pub mod fuente;
pub mod bobina;

/// El índice MP4 del taller, tal cual. Se incluye por ruta en vez de depender
/// del crate `filmlook-core` entero porque ese arrastra wgpu y winit, que aquí
/// no pintan nada: `indice.rs` no usa más que std y anyhow.
#[path = "../../core/src/indice.rs"]
pub mod indice;

/// las fotos y los rótulos como fuente del motor: los mismos planos que
/// entrega el decodificador, calculados una vez (PENDIENTE §4bis.10)
#[path = "../../core/src/foto.rs"]
pub mod foto;

/// el plan de bobina, compartido con el motor de Windows: la compilación de
/// la bobina a renglones es la MISMA en las dos máquinas, o los dos másteres
/// no serían el mismo montaje (MOTOR §8).
#[path = "../../core/src/plan.rs"]
pub mod plan;
