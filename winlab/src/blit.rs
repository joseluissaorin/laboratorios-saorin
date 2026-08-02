//! Split/merge de planos P010 con pixel shader.
//!
//! El driver AMD descarta EN SILENCIO cualquier CopySubresourceRegion entre un
//! plano de P010 y una textura R16/RG16 (probado en todas las combinaciones:
//! typed, typeless, shared, plain, con y sin caja). El único camino que el
//! hardware garantiza es el de los reproductores: un SRV R16_UNORM sobre la
//! textura P010 expone el plano Y y uno R16G16_UNORM el plano UV, y un RTV
//! con esos mismos formatos escribe en cada plano. Aquí: VS de triángulo
//! fullscreen + PS passthrough con Load (sin sampler, 1:1 exacto).

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use windows::core::{Interface, PCSTR};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::d11::D11;

const HLSL: &str = r#"
struct VOut { float4 pos : SV_Position; };
VOut vs(uint vi : SV_VertexID) {
    float2 p = float2((vi << 1) & 2, vi & 2);
    VOut o; o.pos = float4(p * 2.0 - 1.0, 0.0, 1.0);
    return o;
}
Texture2D<float>  tY  : register(t0);
Texture2D<float2> tUV : register(t0);
float  ps_y (VOut i) : SV_Target { return tY.Load(int3(i.pos.xy, 0)); }
float2 ps_uv(VOut i) : SV_Target { return tUV.Load(int3(i.pos.xy, 0)); }
"#;

fn compile(entry: &str, target: &str) -> Result<Vec<u8>> {
    let entry_c = std::ffi::CString::new(entry)?;
    let target_c = std::ffi::CString::new(target)?;
    let mut blob = None;
    let mut err = None;
    let hr = unsafe {
        D3DCompile(
            HLSL.as_ptr() as *const _, HLSL.len(), None, None, None,
            PCSTR(entry_c.as_ptr() as *const u8),
            PCSTR(target_c.as_ptr() as *const u8),
            0, 0, &mut blob, Some(&mut err),
        )
    };
    if hr.is_err() {
        let msg = err
            .and_then(|b: ID3DBlob| unsafe {
                let p = b.GetBufferPointer() as *const u8;
                let n = b.GetBufferSize();
                Some(String::from_utf8_lossy(std::slice::from_raw_parts(p, n)).to_string())
            })
            .unwrap_or_default();
        return Err(anyhow!("D3DCompile {entry}: {msg}"));
    }
    let blob = blob.unwrap();
    let p = unsafe { blob.GetBufferPointer() } as *const u8;
    let n = unsafe { blob.GetBufferSize() };
    Ok(unsafe { std::slice::from_raw_parts(p, n) }.to_vec())
}

pub struct PlaneBlit {
    vs: ID3D11VertexShader,
    ps_y: ID3D11PixelShader,
    ps_uv: ID3D11PixelShader,
    rs: ID3D11RasterizerState,
    srvs: HashMap<(usize, u32), ID3D11ShaderResourceView>,
    rtvs: HashMap<(usize, u32), ID3D11RenderTargetView>,
}

unsafe impl Send for PlaneBlit {}

impl PlaneBlit {
    pub fn new(d: &D11) -> Result<Self> {
        let vsb = compile("vs", "vs_5_0")?;
        let pyb = compile("ps_y", "ps_5_0")?;
        let puvb = compile("ps_uv", "ps_5_0")?;
        let mut vs = None;
        let mut ps_y = None;
        let mut ps_uv = None;
        unsafe {
            d.device.CreateVertexShader(&vsb, None, Some(&mut vs))?;
            d.device.CreatePixelShader(&pyb, None, Some(&mut ps_y))?;
            d.device.CreatePixelShader(&puvb, None, Some(&mut ps_uv))?;
        }
        let rdesc = D3D11_RASTERIZER_DESC {
            FillMode: D3D11_FILL_SOLID,
            CullMode: D3D11_CULL_NONE,
            DepthClipEnable: true.into(),
            ..Default::default()
        };
        let mut rs = None;
        unsafe { d.device.CreateRasterizerState(&rdesc, Some(&mut rs))? };
        Ok(PlaneBlit {
            vs: vs.unwrap(), ps_y: ps_y.unwrap(), ps_uv: ps_uv.unwrap(),
            rs: rs.unwrap(),
            srvs: HashMap::new(), rtvs: HashMap::new(),
        })
    }

    fn srv(&mut self, d: &D11, tex: &ID3D11Texture2D, fmt: DXGI_FORMAT) -> Result<ID3D11ShaderResourceView> {
        self.srv_slice(d, tex, fmt, u32::MAX)
    }

    fn srv_slice(&mut self, d: &D11, tex: &ID3D11Texture2D, fmt: DXGI_FORMAT, slice: u32) -> Result<ID3D11ShaderResourceView> {
        let key = (tex.as_raw() as usize, fmt.0 as u32 ^ (slice.wrapping_mul(0x9e37)) );
        if let Some(v) = self.srvs.get(&key) { return Ok(v.clone()); }
        let desc = if slice == u32::MAX {
            D3D11_SHADER_RESOURCE_VIEW_DESC {
                Format: fmt,
                ViewDimension: D3D11_SRV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                    Texture2D: D3D11_TEX2D_SRV { MostDetailedMip: 0, MipLevels: 1 },
                },
            }
        } else {
            D3D11_SHADER_RESOURCE_VIEW_DESC {
                Format: fmt,
                ViewDimension: D3D11_SRV_DIMENSION_TEXTURE2DARRAY,
                Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                    Texture2DArray: D3D11_TEX2D_ARRAY_SRV {
                        MostDetailedMip: 0, MipLevels: 1,
                        FirstArraySlice: slice, ArraySize: 1,
                    },
                },
            }
        };
        let mut v = None;
        unsafe { d.device.CreateShaderResourceView(tex, Some(&desc), Some(&mut v))? };
        let v = v.unwrap();
        self.srvs.insert(key, v.clone());
        Ok(v)
    }

    fn rtv(&mut self, d: &D11, tex: &ID3D11Texture2D, fmt: DXGI_FORMAT) -> Result<ID3D11RenderTargetView> {
        let key = (tex.as_raw() as usize, fmt.0 as u32);
        if let Some(v) = self.rtvs.get(&key) { return Ok(v.clone()); }
        let desc = D3D11_RENDER_TARGET_VIEW_DESC {
            Format: fmt,
            ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_RENDER_TARGET_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_RTV { MipSlice: 0 },
            },
        };
        let mut v = None;
        unsafe { d.device.CreateRenderTargetView(tex, Some(&desc), Some(&mut v))? };
        let v = v.unwrap();
        self.rtvs.insert(key, v.clone());
        Ok(v)
    }

    fn draw(&self, d: &D11, ps: &ID3D11PixelShader, srv: &ID3D11ShaderResourceView,
            rtv: &ID3D11RenderTargetView, w: u32, h: u32) {
        let vp = D3D11_VIEWPORT {
            TopLeftX: 0.0, TopLeftY: 0.0,
            Width: w as f32, Height: h as f32,
            MinDepth: 0.0, MaxDepth: 1.0,
        };
        unsafe {
            d.ctx.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
            d.ctx.RSSetViewports(Some(&[vp]));
            d.ctx.RSSetState(&self.rs);
            d.ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            d.ctx.VSSetShader(&self.vs, None);
            d.ctx.PSSetShader(ps, None);
            d.ctx.PSSetShaderResources(0, Some(&[Some(srv.clone())]));
            d.ctx.Draw(3, 0);
            // desengancha para que la textura pueda volver a ser destino de copia
            d.ctx.PSSetShaderResources(0, Some(&[None]));
            d.ctx.OMSetRenderTargets(None, None);
        }
    }

    /// P010 → (R16, RG16): saca los dos planos a texturas sueltas
    pub fn split(&mut self, d: &D11, src: &ID3D11Texture2D,
                 dst_y: &ID3D11Texture2D, dst_uv: &ID3D11Texture2D,
                 w: u32, h: u32) -> Result<()> {
        self.split_slice(d, src, u32::MAX, dst_y, dst_uv, w, h)
    }

    /// como split, pero desde un slice concreto de un Texture2DArray (pool del decoder)
    pub fn split_slice(&mut self, d: &D11, src: &ID3D11Texture2D, slice: u32,
                 dst_y: &ID3D11Texture2D, dst_uv: &ID3D11Texture2D,
                 w: u32, h: u32) -> Result<()> {
        let sy = self.srv_slice(d, src, DXGI_FORMAT_R16_UNORM, slice)?;
        let suv = self.srv_slice(d, src, DXGI_FORMAT_R16G16_UNORM, slice)?;
        let ry = self.rtv(d, dst_y, DXGI_FORMAT_R16_UNORM)?;
        let ruv = self.rtv(d, dst_uv, DXGI_FORMAT_R16G16_UNORM)?;
        self.draw(d, &self.ps_y.clone(), &sy, &ry, w, h);
        self.draw(d, &self.ps_uv.clone(), &suv, &ruv, w / 2, h / 2);
        Ok(())
    }

    /// (R16, RG16) → P010: ensambla la entrada del encoder
    pub fn merge(&mut self, d: &D11, src_y: &ID3D11Texture2D, src_uv: &ID3D11Texture2D,
                 dst: &ID3D11Texture2D, w: u32, h: u32) -> Result<()> {
        let sy = self.srv(d, src_y, DXGI_FORMAT_R16_UNORM)?;
        let suv = self.srv(d, src_uv, DXGI_FORMAT_R16G16_UNORM)?;
        let ry = self.rtv(d, dst, DXGI_FORMAT_R16_UNORM)?;
        let ruv = self.rtv(d, dst, DXGI_FORMAT_R16G16_UNORM)?;
        self.draw(d, &self.ps_y.clone(), &sy, &ry, w, h);
        self.draw(d, &self.ps_uv.clone(), &suv, &ruv, w / 2, h / 2);
        Ok(())
    }
}
