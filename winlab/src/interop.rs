//! Interop D3D11 ↔ wgpu(D3D12): abrir handles compartidos NT como texturas wgpu.

use anyhow::{ensure, Result};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D12::{ID3D12CommandQueue, ID3D12Device, ID3D12Fence, ID3D12Resource};

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

pub fn init() -> Result<Gpu> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        ..Default::default()
    });
    let adapter = pollster_block(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .ok_or_else(|| anyhow::anyhow!("sin adaptador DX12"))?;
    let info = adapter.get_info();
    eprintln!("🎛  wgpu: {} ({:?})", info.name, info.backend);
    ensure!(info.name.to_lowercase().contains("amd") || info.name.contains("890M"),
            "el adaptador wgpu no es la 890M: {}", info.name);
    let (device, queue) = pollster_block(adapter.request_device(
        &wgpu::DeviceDescriptor {
            required_features: wgpu::Features::TEXTURE_FORMAT_16BIT_NORM | wgpu::Features::RG11B10UFLOAT_RENDERABLE,
            ..Default::default()
        },
        None,
    ))?;
    Ok(Gpu { device, queue })
}

fn pollster_block<F: std::future::Future>(f: F) -> F::Output {
    pollster::block_on(f)
}

/// la cola cruda D3D12 sobre la que wgpu somete (para Wait/Signal de fences)
pub fn raw_queue(gpu: &Gpu) -> Result<ID3D12CommandQueue> {
    unsafe {
        gpu.device.as_hal::<wgpu::hal::api::Dx12, _, _>(|d| {
            d.map(|dd| dd.raw_queue().clone())
        })
    }
    .ok_or_else(|| anyhow::anyhow!("sin cola hal dx12"))
}

pub fn raw_device(gpu: &Gpu) -> Result<ID3D12Device> {
    unsafe {
        gpu.device.as_hal::<wgpu::hal::api::Dx12, _, _>(|d| {
            d.map(|dd| dd.raw_device().clone())
        })
    }
    .ok_or_else(|| anyhow::anyhow!("sin device hal dx12"))
}

/// abre un handle compartido como recurso D3D12 crudo (alias de la misma memoria)
pub fn open_resource12(gpu: &Gpu, handle: HANDLE) -> Result<ID3D12Resource> {
    let dev = raw_device(gpu)?;
    let mut r: Option<ID3D12Resource> = None;
    unsafe { dev.OpenSharedHandle(handle, &mut r)? };
    r.ok_or_else(|| anyhow::anyhow!("recurso D3D12 nulo"))
}

/// abre un fence compartido de D3D11 en D3D12
pub fn open_fence(gpu: &Gpu, handle: HANDLE) -> Result<ID3D12Fence> {
    let dev = raw_device(gpu)?;
    let mut f: Option<ID3D12Fence> = None;
    unsafe { dev.OpenSharedHandle(handle, &mut f)? };
    f.ok_or_else(|| anyhow::anyhow!("fence nulo"))
}

/// abre un handle compartido de D3D11 como textura wgpu del formato dado
pub fn import_shared(
    gpu: &Gpu,
    handle: HANDLE,
    format: wgpu::TextureFormat,
    w: u32,
    h: u32,
    usage: wgpu::TextureUsages,
) -> Result<wgpu::Texture> {
    // ID3D12Device del backend de wgpu
    let raw12: ID3D12Device = unsafe {
        gpu.device.as_hal::<wgpu::hal::api::Dx12, _, _>(|d| {
            d.map(|dd| dd.raw_device().clone())
        })
    }
    .ok_or_else(|| anyhow::anyhow!("sin device hal dx12"))?;

    let mut resource: Option<ID3D12Resource> = None;
    unsafe { raw12.OpenSharedHandle(handle, &mut resource)? };
    let resource = resource.ok_or_else(|| anyhow::anyhow!("OpenSharedHandle nulo"))?;

    let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
    let hal_tex = unsafe {
        wgpu::hal::dx12::Device::texture_from_raw(
            resource,
            format,
            wgpu::TextureDimension::D2,
            size,
            1,
            1,
        )
    };
    let tex = unsafe {
        gpu.device.create_texture_from_hal::<wgpu::hal::api::Dx12>(
            hal_tex,
            &wgpu::TextureDescriptor {
                label: None,
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            },
        )
    };
    Ok(tex)
}
