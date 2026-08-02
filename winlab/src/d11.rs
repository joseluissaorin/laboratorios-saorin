//! D3D11: dispositivo compartido con Media Foundation, texturas compartibles
//! con D3D12/wgpu, y las copias de planos P010 (subrecursos por plano).

use anyhow::Result;
use windows::core::Interface;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;

#[derive(Clone)]
pub struct D11 {
    pub device: ID3D11Device,
    pub ctx: ID3D11DeviceContext,
}

unsafe impl Send for D11 {}

impl D11 {
    pub fn new() -> Result<Self> {
        let mut device: Option<ID3D11Device> = None;
        let mut ctx: Option<ID3D11DeviceContext> = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                if std::env::var("WINLAB_D3DDEBUG").is_ok() {
                    D3D11_CREATE_DEVICE_VIDEO_SUPPORT | D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_DEBUG
                } else {
                    D3D11_CREATE_DEVICE_VIDEO_SUPPORT | D3D11_CREATE_DEVICE_BGRA_SUPPORT
                },
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut ctx),
            )?;
        }
        let device = device.unwrap();
        let ctx = ctx.unwrap();
        // imprescindible para compartir el dispositivo con MF (hilos del decoder)
        let mt: ID3D11Multithread = device.cast()?;
        unsafe { mt.SetMultithreadProtected(true) };
        Ok(D11 { device, ctx })
    }

    /// textura compartible (NT handle) para importar en wgpu/D3D12
    pub fn shared_tex(&self, w: u32, h: u32, fmt: DXGI_FORMAT) -> Result<(ID3D11Texture2D, HANDLE)> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: fmt,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: (D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0 | D3D11_RESOURCE_MISC_SHARED.0) as u32,
        };
        let mut tex: Option<ID3D11Texture2D> = None;
        unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut tex))? };
        let tex = tex.unwrap();
        let res: IDXGIResource1 = tex.cast()?;
        let handle = unsafe {
            res.CreateSharedHandle(None, (DXGI_SHARED_RESOURCE_READ | DXGI_SHARED_RESOURCE_WRITE).0, None)?
        };
        Ok((tex, handle))
    }

    /// P010 compartible (NT handle) — para abrirlo también en D3D12
    pub fn p010_shared_tex(&self, w: u32, h: u32) -> Result<(ID3D11Texture2D, HANDLE)> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_P010,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: (D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0 | D3D11_RESOURCE_MISC_SHARED.0) as u32,
        };
        let mut tex: Option<ID3D11Texture2D> = None;
        unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut tex))? };
        let tex = tex.unwrap();
        let res: IDXGIResource1 = tex.cast()?;
        let handle = unsafe {
            res.CreateSharedHandle(None, (DXGI_SHARED_RESOURCE_READ | DXGI_SHARED_RESOURCE_WRITE).0, None)?
        };
        Ok((tex, handle))
    }

    /// P010 corriente (para ensamblar la entrada del encoder)
    pub fn p010_tex(&self, w: u32, h: u32) -> Result<ID3D11Texture2D> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_P010,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut tex: Option<ID3D11Texture2D> = None;
        unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut tex))? };
        Ok(tex.unwrap())
    }

    /// copia un plano de una textura planar (P010): los planos son subrecursos
    /// copia de PLANO COMPLETO (los formatos de vídeo no admiten cajas parciales:
    /// una copia parcial se descarta EN SILENCIO)
    pub fn copy_plane(
        &self,
        dst: &ID3D11Texture2D, dst_subres: u32,
        src: &ID3D11Texture2D, src_subres: u32,
        _w: u32, _h: u32,
    ) {
        unsafe {
            self.ctx.CopySubresourceRegion(
                dst, dst_subres, 0, 0, 0,
                src, src_subres, None,
            );
        }
    }

    /// fence compartido creado en ESTE device D3D11 (para que lo espere D3D12 u otro 11)
    pub fn create_shared_fence(&self) -> Result<(ID3D11Fence, HANDLE)> {
        let dev5: ID3D11Device5 = self.device.cast()?;
        let mut fence: Option<ID3D11Fence> = None;
        unsafe { dev5.CreateFence(0, D3D11_FENCE_FLAG_SHARED, &mut fence)? };
        let fence = fence.ok_or_else(|| anyhow::anyhow!("CreateFence nulo"))?;
        let handle = unsafe {
            fence.CreateSharedHandle(None, windows::Win32::Foundation::GENERIC_ALL.0, None)?
        };
        Ok((fence, handle))
    }

    /// abre un fence compartido creado en otro sitio (D3D12 u otro D3D11)
    pub fn open_shared_fence(&self, handle: HANDLE) -> Result<ID3D11Fence> {
        let dev5: ID3D11Device5 = self.device.cast()?;
        let mut fence: Option<ID3D11Fence> = None;
        unsafe { dev5.OpenSharedFence(handle, &mut fence)? };
        fence.ok_or_else(|| anyhow::anyhow!("OpenSharedFence nulo"))
    }

    pub fn ctx4(&self) -> Result<ID3D11DeviceContext4> {
        Ok(self.ctx.cast()?)
    }

    /// abre una textura compartida (NT handle) creada en otro dispositivo
    pub fn open_shared(&self, handle: HANDLE) -> Result<ID3D11Texture2D> {
        let dev1: ID3D11Device1 = self.device.cast()?;
        let tex: ID3D11Texture2D = unsafe { dev1.OpenSharedResource1(handle)? };
        Ok(tex)
    }

    /// vuelca los mensajes del debug layer (si está activo)
    pub fn dump_debug(&self) {
        if let Ok(iq) = self.device.cast::<ID3D11InfoQueue>() {
            let n = unsafe { iq.GetNumStoredMessages() };
            for i in 0..n.min(20) {
                let mut len = 0usize;
                unsafe { let _ = iq.GetMessage(i, None, &mut len); }
                if len == 0 { continue; }
                let mut buf = vec![0u8; len];
                let msg = buf.as_mut_ptr() as *mut D3D11_MESSAGE;
                if unsafe { iq.GetMessage(i, Some(msg), &mut len) }.is_ok() {
                    let m = unsafe { &*msg };
                    let desc = unsafe {
                        std::slice::from_raw_parts(m.pDescription as *const u8, m.DescriptionByteLength)
                    };
                    eprintln!("   [d3d11] {}", String::from_utf8_lossy(desc));
                }
            }
            unsafe { iq.ClearStoredMessages() };
        }
    }

    /// lee de vuelta una textura (staging) y devuelve stats de los primeros u16
    pub fn readback_stats(&self, tex: &ID3D11Texture2D, label: &str) -> Result<()> {
        let mut desc = Default::default();
        unsafe { tex.GetDesc(&mut desc) };
        let sdesc = D3D11_TEXTURE2D_DESC {
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
            ..desc
        };
        let mut st: Option<ID3D11Texture2D> = None;
        unsafe { self.device.CreateTexture2D(&sdesc, None, Some(&mut st))? };
        let st = st.unwrap();
        unsafe { self.ctx.CopyResource(&st, tex) };
        self.flush_wait()?;
        let mut mapped = Default::default();
        unsafe { self.ctx.Map(&st, 0, D3D11_MAP_READ, 0, Some(&mut mapped))? };
        let p = mapped.pData as *const u16;
        let n = 4096usize;
        let s = unsafe { std::slice::from_raw_parts(p, n) };
        let mn = s.iter().min().unwrap();
        let mx = s.iter().max().unwrap();
        let avg: u64 = s.iter().map(|&v| v as u64).sum::<u64>() / n as u64;
        eprintln!("   [{}] {}x{} fmt={:?} · min={} max={} avg={}", label, desc.Width, desc.Height, desc.Format, mn, mx, avg);
        unsafe { self.ctx.Unmap(&st, 0) };
        Ok(())
    }

    /// espera CPU a que termine todo lo encolado (barrera entre APIs)
    pub fn flush_wait(&self) -> Result<()> {
        let qdesc = D3D11_QUERY_DESC { Query: D3D11_QUERY_EVENT, MiscFlags: 0 };
        let mut q: Option<ID3D11Query> = None;
        unsafe { self.device.CreateQuery(&qdesc, Some(&mut q))? };
        let q = q.unwrap();
        unsafe {
            self.ctx.End(&q);
            self.ctx.Flush();
            let mut done: u32 = 0;
            loop {
                let hr = self.ctx.GetData(
                    &q,
                    Some(&mut done as *mut u32 as *mut _),
                    std::mem::size_of::<u32>() as u32,
                    0,
                );
                if hr.is_ok() && done != 0 { break; }
                std::hint::spin_loop();
            }
        }
        Ok(())
    }
}
