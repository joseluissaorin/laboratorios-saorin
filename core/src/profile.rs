//! Perfiles por vendedor: cada GPU tiene un punto óptimo distinto.
//!
//! - Apple Silicon (Metal, memoria unificada): readbacks casi gratis, 16F en
//!   todo, 3 frames en vuelo, readback ring grande.
//! - NVIDIA: VRAM dedicada — minimizar readbacks, async compute si estuviera,
//!   staging buffers moderados.
//! - AMD: similar; en iGPUs antiguas el filtrado 16F es lento → tier rápido.
//! - Intel iGPU: memoria compartida pero ancho de banda justo → tier rápido
//!   por defecto y blurs a 8 bits.

use wgpu::{AdapterInfo, Backend, DeviceType};

#[derive(Clone, Debug)]
pub struct Profile {
    pub vendor: &'static str,
    /// escala interna por defecto (1.0 ultra, 0.5 rápida, 0.35 patata)
    pub default_scale: f32,
    /// frames en vuelo (command buffers sin sincronizar)
    pub inflight: usize,
    /// buffers de staging para readback
    pub staging_ring: usize,
    /// true si la memoria es unificada/compartida (readbacks baratos)
    pub unified_memory: bool,
    /// blurs/intermedios a 8 bits (iGPUs con 16F lento)
    pub low_precision_blurs: bool,
}

pub fn detect(info: &AdapterInfo) -> Profile {
    let apple = info.vendor == 0x106B || (info.backend == Backend::Metal && info.name.contains("Apple"));
    let nvidia = info.vendor == 0x10DE || info.name.contains("NVIDIA");
    let amd = info.vendor == 0x1002 || info.name.contains("AMD") || info.name.contains("Radeon");
    let intel = info.vendor == 0x8086 || info.name.contains("Intel");
    let integrated = matches!(info.device_type, DeviceType::IntegratedGpu);

    if apple {
        Profile {
            vendor: "apple",
            default_scale: 1.0,
            inflight: 3,
            staging_ring: 4,
            unified_memory: true,
            low_precision_blurs: false,
        }
    } else if nvidia {
        Profile {
            vendor: "nvidia",
            default_scale: 1.0,
            inflight: 2,
            staging_ring: 2,
            unified_memory: false,
            low_precision_blurs: false,
        }
    } else if amd {
        Profile {
            vendor: "amd",
            default_scale: if integrated { 0.5 } else { 1.0 },
            inflight: 2,
            staging_ring: 3,
            unified_memory: integrated,
            low_precision_blurs: integrated,
        }
    } else if intel {
        Profile {
            vendor: "intel",
            default_scale: 0.5,
            inflight: 2,
            staging_ring: 3,
            unified_memory: true,
            low_precision_blurs: integrated,
        }
    } else {
        Profile {
            vendor: "generic",
            default_scale: 0.5,
            inflight: 2,
            staging_ring: 2,
            unified_memory: false,
            low_precision_blurs: false,
        }
    }
}
