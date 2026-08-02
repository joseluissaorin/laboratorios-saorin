//! filmlook-core — motor GPU nativo (wgpu) del film-look lab.
//! Cadena: YUV 10-bit → Rec709 → LUT A/B → shutter → blurs → composite
//! (film color + grain + halation + …). Vídeo entra/sale por pipes ffmpeg.

pub mod cine;
/// una foto (o un rótulo) como fuente del motor, sin caer al camino viejo
pub mod foto;
pub mod indice;
pub mod params;
pub mod pipeline;
/// el plan de bobina compilado: lo comparten los dos motores (MOTOR §5)
pub mod plan;
pub mod profile;
pub mod video;
