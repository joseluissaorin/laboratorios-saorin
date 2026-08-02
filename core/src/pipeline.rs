//! Pipeline wgpu: texturas, pases y readback. Espejo del pipeline WebGL del lab.

use std::num::NonZeroU32;
use wgpu::util::DeviceExt;

pub const TEX_FMT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

pub struct Target {
    pub tex: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub w: u32,
    pub h: u32,
}

impl Target {
    pub fn new(device: &wgpu::Device, w: u32, h: u32) -> Self {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TEX_FMT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        Target { tex, view, w, h }
    }
}

pub fn make_target_set(device: &wgpu::Device, w: u32, h: u32) -> TargetSet {
    let t = |dw: u32, dh: u32| Target::new(device, w / dw, h / dh);
    TargetSet {
        graded: t(1, 1),
        raw: t(1, 1),
        h_a: t(1, 1),
        h_b: t(1, 1),
        b0: t(2, 2),
        b1: t(2, 2),
        c0: t(4, 4),
        c1: t(4, 4),
        d0: t(8, 8),
        d1: t(8, 8),
    }
}

pub struct TargetSet {
    pub graded: Target,
    pub raw: Target,
    pub h_a: Target,
    pub h_b: Target,
    pub b0: Target,
    pub b1: Target,
    pub c0: Target,
    pub c1: Target,
    pub d0: Target,
    pub d1: Target,
}

pub struct Pass {
    pub pipeline: wgpu::RenderPipeline,
    pub layout: wgpu::BindGroupLayout,
}

fn shader(device: &wgpu::Device, src: &str) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(src.into()),
    })
}

pub fn make_pass(
    device: &wgpu::Device,
    src: &str,
    entries: &[wgpu::BindGroupLayoutEntry],
    targets: &[Option<wgpu::ColorTargetState>],
) -> Pass {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries,
    });
    let module = shader(device, src);
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[&layout],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&pl),
        vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_main"), buffers: &[], compilation_options: Default::default() },
        fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs_main"), targets, compilation_options: Default::default() }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });
    Pass { pipeline, layout }
}

pub fn tex_filter_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

pub fn tex_uint_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Uint,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

pub fn tex3d_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D3,
            multisampled: false,
        },
        count: None,
    }
}

pub fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

pub fn uniform_entry(binding: u32, size: u64) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: wgpu::BufferSize::new(size),
        },
        count: None,
    }
}

pub fn color_target() -> Option<wgpu::ColorTargetState> {
    Some(wgpu::ColorTargetState {
        format: TEX_FMT,
        blend: Some(wgpu::BlendState::REPLACE),
        write_mask: wgpu::ColorWrites::ALL,
    })
}

pub fn make_sampler(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: None,
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    })
}

pub fn make_repeat_sampler(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: None,
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        address_mode_w: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    })
}

pub fn uniform_buffer(device: &wgpu::Device, data: &[u8]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: data,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    })
}

pub fn make_3d_lut(device: &wgpu::Device, queue: &wgpu::Queue, n: u32, data: &[f32]) -> (wgpu::Texture, wgpu::TextureView) {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: n, height: n, depth_or_array_layers: n },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    // empaquetar RGB → RGBA
    let mut rgba = Vec::with_capacity(data.len() / 3 * 4);
    for c in data.chunks(3) {
        rgba.extend_from_slice(&[c[0], c[1], c[2], 1.0]);
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytemuck::cast_slice(&rgba),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(n * 16),
            rows_per_image: Some(n),
        },
        wgpu::Extent3d { width: n, height: n, depth_or_array_layers: n },
    );
    let view = tex.create_view(&Default::default());
    (tex, view)
}

pub fn run_pass(
    encoder: &mut wgpu::CommandEncoder,
    pass: &Pass,
    bind: &wgpu::BindGroup,
    targets: &[&wgpu::TextureView],
) {
    let attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = targets
        .iter()
        .map(|v| Some(wgpu::RenderPassColorAttachment {
            view: v,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
        }))
        .collect();
    let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &attachments,
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    rp.set_pipeline(&pass.pipeline);
    rp.set_bind_group(0, bind, &[]);
    rp.draw(0..3, 0..1);
}
