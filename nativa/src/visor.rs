//! El visor NATIVO: la cadena fílmica completa (la MISMA del render) pintando
//! el fotograma de la bobina dentro de la ventana. Sin webview, sin WebCodecs,
//! sin procesos ffmpeg: la cabina (VideoToolbox en proceso) sirve fotogramas
//! por el índice del contenedor. Política del taller: el proxy all-intra es el
//! caballo de batalla (scrub y reproducción al instante); el máster a
//! resolución completa entra solo para el frame exacto en pausa.

use crate::cabina::{Cabina, Listo, Orden, Tier};
use crate::proyecto::Proyecto;
use crate::sonido::{OrdenAudio, Sonido};
use crate::ui::Gpu;
use filmlook_core::cine::Fotograma;
use filmlook_core::{params, pipeline::*};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

fn cube(p: &Option<std::path::PathBuf>) -> Option<(u32, Vec<f32>)> {
    let p = p.as_ref()?;
    let texto = std::fs::read_to_string(p).ok()?;
    let (mut n, mut vals) = (0u32, Vec::new());
    for l in texto.lines() {
        let l = l.trim();
        if l.is_empty() || l.starts_with('#') { continue; }
        if let Some(r) = l.strip_prefix("LUT_3D_SIZE") { n = r.trim().parse().ok()?; continue; }
        if l.starts_with(|c: char| c.is_alphabetic()) { continue; }
        for t in l.split_whitespace() { vals.push(t.parse::<f32>().ok()?); }
    }
    if n > 0 && vals.len() == (n * n * n * 3) as usize { Some((n, vals)) } else { None }
}

pub struct Visor {
    cabina: Cabina,
    sonido: Sonido,
    gen: u64,
    clip_activo: Option<usize>,
    pub t: f64,
    pub tocando: bool,
    reloj: Instant,
    t_reloj: f64,
    ultimo: Instant,
    ultimo_frame: Instant,
    /// frames de reproducción esperando su momento (pts de FUENTE)
    cola: VecDeque<Fotograma>,
    /// monitor de FUENTE: cuando Some, el transporte va sobre esta cinta
    /// (ruta del máster, duración) y no sobre la bobina
    pub fuente: Option<(PathBuf, f64)>,
    /// refinado a máster pendiente: (src_t, clip, desde cuándo)
    refina: Option<(f64, usize, Instant)>,
    marca_orden: Option<Instant>,
    crono: bool,
    /// fotos fijas ya convertidas (una vez por ruta)
    fotos: std::collections::HashMap<PathBuf, Fotograma>,
    /// decoders de scrub EN el hilo de eventos: el proxy all-intra decodifica
    /// en 1–3 ms, así que el fotograma del seek se sirve síncrono y el
    /// siguiente redraw ya lo pinta (sin viaje por la cabina)
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    escrutinio: std::collections::HashMap<PathBuf, filmlook_core::cine::Cine>,

    // la cadena
    targets: TargetSet,
    grade: Pass, down: Pass, blur: Pass, accum: Pass, present: Pass,
    /// el pase de LA CAPA (CAPAS §6): el mismo revelado, compuesto por alfa
    grade_capa: Pass,
    /// texturas RGBA residentes de las capas foto/rótulo, por ruta
    capa_rgba: std::collections::HashMap<PathBuf, (wgpu::Texture, wgpu::TextureView)>,
    /// los uniformes de los dos huecos de capa
    capa_bufs: [wgpu::Buffer; 2],
    samp: wgpu::Sampler, samp_rep: wgpu::Sampler,
    t_y: wgpu::Texture, t_u: wgpu::Texture, t_v: wgpu::Texture,
    grade_bg: wgpu::BindGroup,
    view_grain: wgpu::TextureView,
    grade_buf: wgpu::Buffer,
    comp_buf: wgpu::Buffer,
    small_u: wgpu::Buffer,
    comp_bg: Option<wgpu::BindGroup>,
    /// el bind group de LA LUPA: mismas texturas, uniform con el aumento
    lupa_buf: wgpu::Buffer,
    lupa_bg: Option<wgpu::BindGroup>,
    /// dónde apunta el cuentahílos, en píxeles de pantalla
    pub lupa_centro: (f32, f32),
    /// SCRUB AUDIBLE: cuándo sonó la última chispa (una cada ~70 ms)
    ultima_chispa: Instant,

    /// resolución de la CADENA = **el lienzo del proyecto**. Antes era la de
    /// la fuente y el conform no existía en la preview: un 9:16 dentro de una
    /// bobina 16:9 se veía a sangre y salía con bandas en el máster. Ahora el
    /// visor pinta el mismo lienzo que el revelado (§1.5).
    pub w: u32,
    pub h: u32,
    /// resolución de las texturas de FUENTE ahora mismo (proxy o máster)
    src_w: u32,
    src_h: u32,
    aspecto: f32,
    comp_u: params::CompU,
    shutter: f32,
    pub frames: u64,
    frame_idx: usize,
    pub fps_medido: f64,
    fps_cuenta: u32,
    pub rect_pantalla: [f32; 4],
    pub wipe: bool,
    hay_imagen: bool,
    // el cuarto oscuro POR CLIP: gelatinas y ajustes cambian en cada junta
    vistas_yuv: (wgpu::TextureView, wgpu::TextureView, wgpu::TextureView),
    vista_video: wgpu::TextureView,
    cache_lut: std::collections::HashMap<String, (wgpu::Texture, wgpu::TextureView)>,
    tam_cache: std::collections::HashMap<String, u32>,
    lut_puestas: (String, String),
    bg_sucio: bool,
    /// el encuadre del clip activo — vive en los uniformes de la cadena
    pub encuadre: crate::proyecto::Encuadre,
    pendiente: Option<Fotograma>,
    cuarto_pendiente: Option<usize>,
    /// la receta que YA está subida a la GPU (huella): evita recompilar el
    /// cuarto oscuro en cada fotograma (la cadena costaba 750 ms por frame)
    receta_puesta: Option<(usize, String, Option<PathBuf>, Option<PathBuf>,
                           crate::proyecto::Encuadre)>,
    /// aquí empieza un plano: el acumulador del obturador arranca de cero
    corte_pendiente: bool,
    weave_amp: f32,
}

fn plano(dev: &wgpu::Device, pw: u32, ph: u32) -> wgpu::Texture {
    dev.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: pw.max(2), height: ph.max(2), depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R16Uint,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// EL LIENZO DE LA PREVIEW: el del proyecto, con la misma proporción y —si la
/// bobina es de 4K— la mitad de lado, que es lo que el taller ya hacía con la
/// fuente. La proporción NO se toca: es la del máster.
fn lienzo_de(pr: &Proyecto) -> (u32, u32) {
    let (mut w, mut h) = match &pr.formato {
        Some(f) => (f.w, f.h),
        None => pr.clips.iter().filter(|c| !c.hueco).find_map(|c| {
            filmlook_core::indice::sondea(&c.ruta).ok()
                .or_else(|| filmlook_core::video::probe(c.ruta.to_str().unwrap_or("")).ok())
                .map(|(cw, ch, _, _)| (cw, ch))
        }).unwrap_or((1920, 1080)),
    };
    if w.max(h) > 2200 { w /= 2; h /= 2; }
    ((w.max(16)) & !1, (h.max(16)) & !1)
}

impl Visor {
    pub fn new(g: &Gpu, pr: &Proyecto) -> anyhow::Result<Self> {
        let (w, h) = lienzo_de(pr);
        let aspecto = w as f32 / h.max(1) as f32;
        let dev = &g.device;
        let targets = make_target_set(dev, w, h);
        let samp = make_sampler(dev);
        let samp_rep = make_repeat_sampler(dev);

        let ident: Vec<f32> = vec![0.,0.,0., 1.,0.,0., 0.,1.,0., 1.,1.,0., 0.,0.,1., 1.,0.,1., 0.,1.,1., 1.,1.,1.];
        let la = cube(&pr.lut_in);
        let lb = cube(&pr.lut_color);
        let (_ta, va) = match &la { Some((n, d)) => make_3d_lut(dev, &g.queue, *n, d),
                                    None => make_3d_lut(dev, &g.queue, 2, &ident) };
        let (_tb, vb) = match &lb { Some((n, d)) => make_3d_lut(dev, &g.queue, *n, d),
                                    None => make_3d_lut(dev, &g.queue, 2, &ident) };
        let meta = (la.as_ref().map(|x| x.0).unwrap_or(2), lb.as_ref().map(|x| x.0).unwrap_or(2),
                    la.is_some(), lb.is_some());

        let t_y = plano(dev, w, h);
        let t_u = plano(dev, w / 2, h / 2);
        let t_v = plano(dev, w / 2, h / 2);
        let t_video = dev.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let (vy, vu, vv) = (t_y.create_view(&Default::default()),
                            t_u.create_view(&Default::default()),
                            t_v.create_view(&Default::default()));
        let vvideo = t_video.create_view(&Default::default());

        // la placa de grano (la misma del laboratorio)
        let grano_ruta = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../app/ui/assets/grain.bin");
        let grano = std::fs::read(&grano_ruta).unwrap_or_else(|_| vec![0u8; 1024 * 1024 * 2]);
        let t_grain = dev.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: 1024, height: 1024, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        g.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &t_grain, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &grano,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(1024 * 2), rows_per_image: Some(1024) },
            wgpu::Extent3d { width: 1024, height: 1024, depth_or_array_layers: 1 },
        );
        let view_grain = t_grain.create_view(&Default::default());

        let grade = make_pass(dev, include_str!("../../core/src/shaders/grade.wgsl"), &[
            uniform_entry(0, filmlook_core::params::bytes_uniforme::<filmlook_core::params::GradeU>()),
            tex_uint_entry(1), tex_uint_entry(2), tex_uint_entry(3),
            tex_filter_entry(4), tex3d_entry(5), tex3d_entry(6), sampler_entry(7),
        ], &[color_target(), color_target()]);
        // el MISMO pase, con mezcla por alfa: es lo que compone una capa
        // encima sin sustituir lo revelado (CAPAS §6)
        let grade_capa = make_pass(dev, include_str!("../../core/src/shaders/grade.wgsl"), &[
            uniform_entry(0, filmlook_core::params::bytes_uniforme::<filmlook_core::params::GradeU>()),
            tex_uint_entry(1), tex_uint_entry(2), tex_uint_entry(3),
            tex_filter_entry(4), tex3d_entry(5), tex3d_entry(6), sampler_entry(7),
        ], &[filmlook_core::pipeline::color_target_blend(filmlook_core::pipeline::TEX_FMT),
             filmlook_core::pipeline::color_target_blend(filmlook_core::pipeline::TEX_FMT)]);
        let down = make_pass(dev, include_str!("../../core/src/shaders/down.wgsl"), &[
            uniform_entry(0, 16), tex_filter_entry(1), sampler_entry(2),
        ], &[color_target()]);
        let blur = make_pass(dev, include_str!("../../core/src/shaders/blur.wgsl"), &[
            uniform_entry(0, 16), tex_filter_entry(1), sampler_entry(2),
        ], &[color_target()]);
        let accum = make_pass(dev, include_str!("../../core/src/shaders/accum.wgsl"), &[
            uniform_entry(0, 16), tex_filter_entry(1), tex_filter_entry(2), sampler_entry(3),
        ], &[color_target()]);
        // el comp entra DIRECTO en la ventana (sobre el papel ya pintado)
        let present = make_pass(dev, include_str!("../../core/src/shaders/comp.wgsl"), &[
            uniform_entry(0, filmlook_core::params::bytes_uniforme::<filmlook_core::params::CompU>()),
            tex_filter_entry(1), tex_filter_entry(2), tex_filter_entry(3),
            tex_filter_entry(4), tex_filter_entry(5), tex_filter_entry(6),
            sampler_entry(7), sampler_entry(8),
        ], &[Some(wgpu::ColorTargetState {
            format: g.config.format,
            blend: Some(wgpu::BlendState::REPLACE),
            write_mask: wgpu::ColorWrites::ALL,
        })]);

        let grade_u = params::grade_u(&pr.prefs, w, h, meta.0, meta.1, meta.2, meta.3);
        let comp_u = params::comp_u(&pr.prefs, w, h);
        let shutter = params::f(&pr.prefs, "shutter", 0.0);
        let grade_buf = uniform_buffer(dev, bytemuck::bytes_of(&grade_u));
        let comp_buf = uniform_buffer(dev, bytemuck::bytes_of(&comp_u));
        let lupa_buf = uniform_buffer(dev, bytemuck::bytes_of(&comp_u));
        let small_u = uniform_buffer(dev, &[0u8; 16]);

        let grade_bg = dev.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &grade.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: grade_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&vy) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&vu) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&vv) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&vvideo) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&va) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&vb) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::Sampler(&samp) },
            ],
        });

        Ok(Visor {
            cabina: Cabina::nueva(),
            sonido: Sonido::nuevo(),
            gen: 0,
            clip_activo: None,
            t: 0.0, tocando: false,
            reloj: Instant::now(), t_reloj: 0.0, ultimo: Instant::now(),
            ultimo_frame: Instant::now(),
            cola: VecDeque::new(),
            fuente: None,
            refina: None,
            marca_orden: None,
            crono: std::env::var("FL_CRONO").is_ok(),
            fotos: std::collections::HashMap::new(),
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            escrutinio: std::collections::HashMap::new(),
            targets, grade, grade_capa, down, blur, accum, present, samp, samp_rep,
            t_y, t_u, t_v, grade_bg, view_grain, grade_buf, comp_buf, small_u,
            comp_bg: None,
            lupa_buf,
            lupa_bg: None,
            lupa_centro: (0.0, 0.0),
            ultima_chispa: Instant::now(),
            w, h, src_w: w, src_h: h, aspecto, comp_u, shutter,
            frames: 0, frame_idx: 0, fps_medido: 0.0, fps_cuenta: 0,
            rect_pantalla: [0.0; 4], wipe: false, hay_imagen: false,
            vistas_yuv: (vy, vu, vv), vista_video: vvideo,
            cache_lut: std::collections::HashMap::new(),
            tam_cache: std::collections::HashMap::new(),
            capa_rgba: std::collections::HashMap::new(),
            capa_bufs: [
                g.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("capa 0"),
                    size: params::bytes_uniforme::<params::GradeU>(),
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                g.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("capa 1"),
                    size: params::bytes_uniforme::<params::GradeU>(),
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
            ],
            lut_puestas: (String::new(), String::new()),
            bg_sucio: false,
            encuadre: crate::proyecto::Encuadre::limpio(0),
            pendiente: None,
            cuarto_pendiente: None,
            receta_puesta: None,
            corte_pendiente: true,
            weave_amp: params::f(&pr.prefs, "weave", 0.0) * 2.5,
        })
    }

    /// el proxy del taller si está cocido; si no, el máster
    /// LA FUENTE REAL de un clip para la preview (CAPAS §6): si es una
    /// anidada, el clip hijo que suena en ese instante. Devuelve también el
    /// tiempo de fuente ya traducido.
    fn fuente_real(pr: &Proyecto, i: usize, src_t: f64) -> (PathBuf, f64, f64) {
        match pr.resuelve(i, src_t) {
            Some((c, t)) => {
                // el proxy del hijo, si existe
                let p = pr.base.join(".proxies").join(&c.media);
                let ruta = if p.is_file() { p } else { c.ruta.clone() };
                (ruta, t, c.t_out)
            }
            None => (PathBuf::new(), src_t, src_t),
        }
    }

    fn ruta_proxy(pr: &Proyecto, i: usize) -> PathBuf {
        let c = &pr.clips[i];
        let p = pr.base.join(".proxies").join(&c.media);
        if p.is_file() { p } else { c.ruta.clone() }
    }

    /// precalienta TODOS los decoders de la bobina en los ratos muertos de la
    /// cabina: primero los proxies (el instante), luego los másters (el refinado)
    pub fn precalienta(&mut self, pr: &Proyecto) {
        let mut rutas: Vec<(PathBuf, f64)> = Vec::new();
        // LAS FOTOS Y LOS RÓTULOS NO PASAN POR LA CABINA: no tienen `moov` que
        // abrir, y precalentarlos solo servía para escupir «sin moov» dos veces
        // por cada tarjeta de título
        for (i, c) in pr.clips.iter().enumerate() {
            if c.hueco || crate::foto::es_foto(&c.ruta) { continue; }
            rutas.push((Self::ruta_proxy(pr, i), c.t_in));
        }
        for c in pr.clips.iter() {
            if c.hueco || crate::foto::es_foto(&c.ruta) { continue; }
            rutas.push((c.ruta.clone(), c.t_in));
        }
        rutas.dedup();
        self.cabina.manda(Orden::Precalienta { rutas });
    }

    /// drenar la cabina FUERA del redraw: la imagen nueva pide el redraw, no al revés
    pub fn drena(&mut self) -> bool {
        let mut hay = false;
        while let Ok(Listo { gen, tier, fr }) = self.cabina.rx.try_recv() {
            if gen != self.gen { continue; }
            if self.tocando && tier == Tier::Proxy {
                self.cola.push_back(fr);
            } else {
                self.pendiente = Some(fr);
            }
            hay = true;
            if let Some(m) = self.marca_orden.take() {
                if self.crono {
                    eprintln!("⏱ orden→frame: {:.1} ms", m.elapsed().as_secs_f64() * 1000.0);
                }
            }
        }
        hay
    }

    /// aplica el cuarto oscuro DEL CLIP: sus gelatinas y sus ajustes
    fn aplica_cuarto(&mut self, g: &Gpu, prefs: &serde_json::Value,
                     lut_in: &Option<std::path::PathBuf>,
                     lut_color: &Option<std::path::PathBuf>) {
        let clave = |p: &Option<std::path::PathBuf>| -> String {
            p.as_ref().map(|x| x.to_string_lossy().to_string()).unwrap_or_default()
        };
        let (ka, kb) = (clave(lut_in), clave(lut_color));

        // uniformes del clip (siempre: pueden cambiar sin cambiar de gelatina)
        let na = self.tam_lut(&ka);
        let nb = self.tam_lut(&kb);
        let gu = params::grade_u_enc(prefs, self.src_w, self.src_h, na, nb,
                                     !ka.is_empty(), !kb.is_empty(),
                                     &self.encuadre, self.w, self.h);
        g.queue.write_buffer(&self.grade_buf, 0, bytemuck::bytes_of(&gu));
        self.comp_u = params::comp_u(prefs, self.w, self.h);
        self.shutter = params::f(prefs, "shutter", 0.0);
        self.weave_amp = params::f(prefs, "weave", 0.0) * 2.5;

        if !self.bg_sucio && (ka.clone(), kb.clone()) == self.lut_puestas { return; }
        // gelatinas nuevas o texturas recreadas: rehacer el bind group
        self.carga_lut(g, &ka);
        self.carga_lut(g, &kb);
        let ident_a = self.cache_lut.get(&ka);
        let ident_b = self.cache_lut.get(&kb);
        let (Some((_, va)), Some((_, vb))) = (ident_a, ident_b) else { return };
        self.grade_bg = g.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None, layout: &self.grade.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.grade_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.vistas_yuv.0) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.vistas_yuv.1) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&self.vistas_yuv.2) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&self.vista_video) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(va) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(vb) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::Sampler(&self.samp) },
            ],
        });
        self.lut_puestas = (ka, kb);
        self.bg_sucio = false;
    }

    /// cuántos puntos tiene una gelatina. Se memoriza porque `cube()` LEE Y
    /// PARSEA el fichero entero —una de 65 puntos son 274 625 líneas— y esto
    /// ahora se pregunta en cada fotograma, no una vez por receta.
    fn tam_lut(&mut self, clave: &str) -> u32 {
        if clave.is_empty() { return 2; }
        if let Some(&n) = self.tam_cache.get(clave) { return n; }
        let n = cube(&Some(std::path::PathBuf::from(clave))).map(|x| x.0).unwrap_or(2);
        self.tam_cache.insert(clave.to_string(), n);
        n
    }

    fn carga_lut(&mut self, g: &Gpu, clave: &str) {
        if self.cache_lut.contains_key(clave) { return; }
        let ident: Vec<f32> = vec![0.,0.,0., 1.,0.,0., 0.,1.,0., 1.,1.,0., 0.,0.,1., 1.,0.,1., 0.,1.,1., 1.,1.,1.];
        let (n, datos) = if clave.is_empty() {
            (2, ident)
        } else {
            cube(&Some(std::path::PathBuf::from(clave))).unwrap_or((2, ident))
        };
        let t = make_3d_lut(&g.device, &g.queue, n, &datos);
        self.cache_lut.insert(clave.to_string(), t);
    }

    /// Fuerza a releer el cuarto oscuro. El índice es sólo una PISTA de qué se
    /// ha tocado: quién manda de verdad lo decide `cadena()` mirando la aguja.
    ///
    /// Antes esto ELEGÍA el clip cuya receta se aplicaba, y por eso
    /// seleccionar un clip mientras veías otro te pintaba la imagen de uno con
    /// los ajustes del otro: la preview mentía sobre lo que enseñaba.
    pub fn marca_cuarto(&mut self, i: usize) { self.cuarto_pendiente = Some(i); }

    /// EMPIEZA UN PLANO: el arrastre del obturador no cruza el empalme, igual
    /// que en el máster (lo que ves es lo que sale)
    fn marca_corte(&mut self, i: usize) {
        if self.clip_activo != Some(i) { self.corte_pendiente = true; }
    }

    /// el foley del taller pasa por el mezclador del visor
    pub fn foley(&self, cual: crate::sonido::Foley) { self.sonido.foley(cual); }

    /// LOS MANDOS DEL MARGEN (§1.6): el nivel de la voz y el de la música
    pub fn pon_niveles(&self, voz: f64, musica: f64) {
        crate::sonido::pon_niveles(voz, musica);
    }

    /// LAS PALANCAS DEL MARGEN, al mezclador. Se llama al cargar la bobina y
    /// cada vez que se tocan, no sólo al arrancar la reproducción.
    pub fn pon_mudos(&self, voz: bool, musica: bool) {
        crate::sonido::pon_mudos(voz, musica);
    }

    /// los dos mandos y las dos palancas de una vez, leídos del proyecto
    pub fn manda_mezcla(&self, pr: &Proyecto) {
        crate::sonido::pon_niveles(pr.vol_voz, pr.vol_musica);
        crate::sonido::pon_mudos(pr.mudo_voz, pr.mudo_musica);
    }
    /// SCRUB AUDIBLE (NORTE §6): al arrastrar la aguja se oye un mordisco
    /// del sonido que hay debajo — la moviola de toda la vida. Se limita a
    /// una chispa cada 70 ms para no ahogar al hilo de audio.
    pub fn chispa(&mut self, pr: &Proyecto) {
        if !crate::prefs::SCRUB_AUDIBLE.load(std::sync::atomic::Ordering::Relaxed)
            || self.tocando {
            return;
        }
        if self.ultima_chispa.elapsed().as_millis() < 70 {
            return;
        }
        // ¿qué suena bajo la aguja? la voz del clip (si no está muda)
        let (ruta, t) = if let Some((ruta, _)) = self.fuente.clone() {
            (ruta, self.t)
        } else {
            let Some((i, src_t)) = pr.en(self.t) else { return };
            let Some(c) = pr.clips.get(i) else { return };
            if c.hueco || c.mute || pr.mudo_voz || crate::foto::es_foto(&c.ruta) {
                return;
            }
            (c.ruta.clone(), src_t)
        };
        self.ultima_chispa = Instant::now();
        if self.crono { eprintln!("  chispa en t={t:.2}"); }
        // un mordisco de 90 ms con sus fundidos de 20 ms: se oye el material,
        // no un chasquido
        self.sonido.manda(OrdenAudio::Toca {
            ruta, t0: t, t1: t + 0.09, gain: -3.0,
            borde_in: t, fade_in: 0.02, fade_out: 0.02, banda: Vec::new(),
        });
    }

    /// el ambiente sonoro de la sala (lo rellena el bucle en cada vuelta)
    pub fn ambiente(&self, cual: crate::sonido::Ambiente, reloj: f32) {
        self.sonido.ambiente(cual, reloj);
    }

    /// cambio de bobina en caliente: estado a cero y primer fotograma YA
    pub fn recarga(&mut self, g: &Gpu, pr: &Proyecto) {
        self.manda_mezcla(pr);
        self.gen += 1;
        self.cola.clear();
        self.pendiente = None;
        self.comp_bg = None;
        self.clip_activo = None;
        self.cuarto_pendiente = None;
        self.refina = None;
        self.tocando = false;
        self.hay_imagen = false;
        self.t = 0.0;
        self.frame_idx = 0;
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        self.escrutinio.clear();
        self.sonido.manda(OrdenAudio::Para);
        // el lienzo del PROYECTO (el visor pinta lo mismo que sale al máster)
        let (w, h) = lienzo_de(pr);
        self.aspecto = w as f32 / h.max(1) as f32;
        if (w, h) != (self.w, self.h) {
            self.w = w;
            self.h = h;
            self.targets = make_target_set(&g.device, w, h);
            self.comp_bg = None;
            self.lupa_bg = None;
            self.receta_puesta = None;
        }
        if !pr.clips.is_empty() {
            self.busca(pr, 0.0);
        }
        self.precalienta(pr);
    }

    pub fn encaje(&self, max_w: f32, max_h: f32) -> (f32, f32) {
        let mut w = max_w;
        let mut h = w / self.aspecto;
        if h > max_h { h = max_h; w = h * self.aspecto; }
        (w.max(64.0), h.max(36.0))
    }

    /// arranca la secuencia de reproducción (vídeo + sonido) desde self.t
    fn arranca_toca(&mut self, pr: &Proyecto) {
        let Some((i, src_t)) = pr.en(self.t) else { return };
        self.marca_corte(i);
        self.clip_activo = Some(i);
        self.cuarto_pendiente = Some(i);
        let c = &pr.clips[i];
        if c.hueco {
            // el hueco es NEGRO con SILENCIO: nada que decodificar
            self.sonido.manda(OrdenAudio::Para);
            return;
        }
        if crate::foto::es_foto(&c.ruta) {
            let ruta = c.ruta.clone();
            if !self.fotos.contains_key(&ruta) {
                if let Some(fr) = crate::foto::carga(&ruta) {
                    self.fotos.insert(ruta.clone(), fr);
                }
            }
            if let Some(fr) = self.fotos.get(&ruta) {
                self.pendiente = Some(fr.clone());
            }
            self.sonido.manda(OrdenAudio::Para);
            return;
        }
        if c.anidada.is_some() {
            // la ANIDADA (CAPAS §6): se proyecta el clip hijo que toque; el
            // sonido de la hija no viaja en la preview (anotado en CAPAS.md)
            let (ruta, t0, t1) = Self::fuente_real(pr, i, src_t);
            if ruta.as_os_str().is_empty() {
                self.sonido.manda(OrdenAudio::Para);
                return;
            }
            self.cabina.manda(Orden::Toca {
                gen: self.gen, ruta, t0, t1, tier: Tier::Proxy,
            });
            self.sonido.manda(OrdenAudio::Para);
            return;
        }
        self.cabina.manda(Orden::Toca {
            gen: self.gen, ruta: Self::ruta_proxy(pr, i),
            t0: src_t, t1: c.t_out, tier: Tier::Proxy,
        });
        if (c.speed - 1.0).abs() < 0.01 && !c.mute && !pr.mudo_voz {
            self.sonido.manda(OrdenAudio::Toca { ruta: c.ruta.clone(), t0: src_t, t1: c.t_out, gain: 0.0, borde_in: c.t_in, fade_in: 0.0, fade_out: 0.0, banda: Vec::new() });
        } else {
            // clip retimado o silenciado: mudo en preview
            self.sonido.manda(OrdenAudio::Para);
        }
        // ¿hay MÚSICA bajo este punto de la bobina? pues que suene
        // (salvo que la palanca de la pista esté bajada)
        let t_bobina = self.t;
        match pr.audio.iter().filter(|_| !pr.mudo_musica).find(|a| {
            t_bobina >= a.start - 0.01 && t_bobina < a.start + a.dur()
        }) {
            Some(a) => {
                let t0 = a.t_in + (t_bobina - a.start).max(0.0);
                self.sonido.manda_musica(OrdenAudio::Toca {
                    ruta: a.ruta.clone(), t0, t1: a.t_out, gain: a.gain,
                    borde_in: a.t_in, fade_in: a.fade_in, fade_out: a.fade_out,
                    banda: a.banda.clone(),
                });
            }
            None => self.sonido.manda_musica(OrdenAudio::Para),
        }
    }

    pub fn play_pausa(&mut self, pr: &Proyecto) {
        if let Some((ruta, dur)) = self.fuente.clone() {
            if !self.tocando && self.t >= dur - 0.05 { self.t = 0.0; }
            self.tocando = !self.tocando;
            self.reloj = Instant::now();
            self.t_reloj = self.t;
            self.gen += 1;
            self.cola.clear();
            if self.tocando {
                self.cabina.manda(Orden::Toca {
                    gen: self.gen, ruta: ruta.clone(), t0: self.t, t1: dur, tier: Tier::Proxy,
                });
                self.sonido.manda(OrdenAudio::Toca { ruta, t0: self.t, t1: dur, gain: 0.0, borde_in: 0.0, fade_in: 0.0, fade_out: 0.0, banda: Vec::new() });
            } else {
                self.sonido.manda(OrdenAudio::Para);
            }
            return;
        }
        // espacio al final de la bobina = proyectar desde el principio
        if !self.tocando && self.t >= pr.duracion() - 0.05 {
            self.busca(pr, 0.0);
        }
        self.tocando = !self.tocando;
        self.reloj = Instant::now();
        self.t_reloj = self.t;
        self.gen += 1;
        self.cola.clear();
        self.marca_orden = Some(Instant::now());
        if self.tocando {
            self.arranca_toca(pr);
        } else {
            self.sonido.manda(OrdenAudio::Para);
            self.sonido.manda_musica(OrdenAudio::Para);
            // en pausa: la imagen en pantalla YA es el fotograma correcto —
            // pedir otro al decoder de scrub (parado GOPs atrás) enseñaba un
            // keyframe viejo un instante. No se busca NADA: solo el refinado
            // a máster, y casi inmediato (no hay scrub del que protegerse).
            if let Some((i, src_t)) = pr.en(self.t) {
                if !pr.clips[i].hueco {
                    self.refina = Some((src_t, i,
                        Instant::now() - std::time::Duration::from_millis(320)));
                }
            }
        }
    }

    pub fn busca(&mut self, pr: &Proyecto, t: f64) {
        if let Some((ruta, dur)) = self.fuente.clone() {
            self.t = t.clamp(0.0, (dur - 0.02).max(0.0));
            self.t_reloj = self.t;
            self.reloj = Instant::now();
            self.gen += 1;
            self.cola.clear();
            if self.tocando {
                self.cabina.manda(Orden::Toca {
                    gen: self.gen, ruta: ruta.clone(), t0: self.t, t1: dur, tier: Tier::Proxy,
                });
                self.sonido.manda(OrdenAudio::Toca { ruta, t0: self.t, t1: dur, gain: 0.0, borde_in: 0.0, fade_in: 0.0, fade_out: 0.0, banda: Vec::new() });
            } else if !self.fuente_sincrono(&ruta, self.t) {
                self.cabina.manda(Orden::Frame {
                    gen: self.gen, ruta, t: self.t, tier: Tier::Proxy,
                });
            }
            return;
        }
        self.t = t.clamp(0.0, (pr.duracion() - 0.02).max(0.0));
        self.t_reloj = self.t;
        self.reloj = Instant::now();
        self.gen += 1;
        self.cola.clear();
        self.marca_orden = Some(Instant::now());
        let Some((i, src_t)) = pr.en(self.t) else { return };
        self.marca_corte(i);
        self.clip_activo = Some(i);
        self.cuarto_pendiente = Some(i);
        if pr.clips[i].hueco { return; }
        if crate::foto::es_foto(&pr.clips[i].ruta) {
            let ruta = pr.clips[i].ruta.clone();
            if !self.fotos.contains_key(&ruta) {
                if let Some(fr) = crate::foto::carga(&ruta) {
                    self.fotos.insert(ruta.clone(), fr);
                }
            }
            if let Some(fr) = self.fotos.get(&ruta) {
                self.pendiente = Some(fr.clone());
            }
            return;
        }
        if self.tocando {
            self.arranca_toca(pr);
        } else {
            if !self.busca_sincrono(pr, i, src_t) {
                self.cabina.manda(Orden::Frame {
                    gen: self.gen, ruta: Self::ruta_proxy(pr, i), t: src_t, tier: Tier::Proxy,
                });
            }
            self.refina = Some((src_t, i, Instant::now()));
        }
    }

    /// decode síncrono del proxy en el hilo de eventos (1–3 ms): el fotograma
    /// del scrub no espera ni una vuelta del bucle. Devuelve false si no pudo.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn busca_sincrono(&mut self, pr: &Proyecto, i: usize, src_t: f64) -> bool {
        // la anidada resuelve al clip hijo (CAPAS §6)
        let (ruta, src_t) = if pr.clips.get(i).map(|c| c.anidada.is_some()).unwrap_or(false) {
            let (r, t, _) = Self::fuente_real(pr, i, src_t);
            if r.as_os_str().is_empty() { return false }
            (r, t)
        } else {
            (Self::ruta_proxy(pr, i), src_t)
        };
        if !self.escrutinio.contains_key(&ruta) {
            if self.escrutinio.len() >= 4 {
                if let Some(k) = self.escrutinio.keys().next().cloned() {
                    self.escrutinio.remove(&k);
                }
            }
            match filmlook_core::cine::Cine::abre(&ruta) {
                Ok(mut c) => { c.mitad = crate::prefs::PREVIEW_MEDIA.load(std::sync::atomic::Ordering::Relaxed); self.escrutinio.insert(ruta.clone(), c); }
                Err(e) => {
                    if self.crono { eprintln!("  escrutinio: {} NO abre: {e:#}", ruta.display()); }
                    return false;
                }
            }
        }
        let Some(cine) = self.escrutinio.get_mut(&ruta) else { return false };
        let m = Instant::now();
        // exacto si es barato; el keyframe si el catch-up saldría caro
        // (el frame EXACTO a resolución completa llega con el refinado)
        let Some(fr) = cine.frame_scrub(src_t) else { return false };
        if self.crono {
            eprintln!("⏱ scrub síncrono t={src_t:.2}: {:.1} ms", m.elapsed().as_secs_f64() * 1e3);
        }
        self.marca_orden = None;
        self.pendiente = Some(fr);
        true
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn busca_sincrono(&mut self, _pr: &Proyecto, _i: usize, _src_t: f64) -> bool {
        // sin backend nativo en proceso, el decode síncrono bloquearía el hilo
        // de eventos (ffmpeg = proceso): que lo sirva la cabina
        false
    }

    /// scrub síncrono sobre la cinta de la FUENTE
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn fuente_sincrono(&mut self, ruta: &PathBuf, t: f64) -> bool {
        if !self.escrutinio.contains_key(ruta) {
            if self.escrutinio.len() >= 4 {
                if let Some(k) = self.escrutinio.keys().next().cloned() {
                    self.escrutinio.remove(&k);
                }
            }
            match filmlook_core::cine::Cine::abre(ruta) {
                Ok(mut c) => { c.mitad = crate::prefs::PREVIEW_MEDIA.load(std::sync::atomic::Ordering::Relaxed); self.escrutinio.insert(ruta.clone(), c); }
                Err(_) => return false,
            }
        }
        let Some(cine) = self.escrutinio.get_mut(ruta) else { return false };
        let Some(fr) = cine.frame_scrub(t) else { return false };
        self.pendiente = Some(fr);
        true
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn fuente_sincrono(&mut self, _ruta: &PathBuf, _t: f64) -> bool { false }

    /// una vuelta del proyector: recoger frames, reloj, ritmo y refinado
    pub fn avanza(&mut self, pr: &Proyecto) {
        // ── recoger lo que la cabina haya servido ──
        self.drena();

        if self.tocando {
            if let Some((_, dur)) = self.fuente {
                let t = self.t_reloj + self.reloj.elapsed().as_secs_f64();
                if t >= dur {
                    self.tocando = false;
                    self.sonido.manda(OrdenAudio::Para);
                    self.t = dur;
                    return;
                }
                self.t = t;
                let margen = 0.5 / pr.fps.max(1.0);
                let mut mostrado = None;
                while self.cola.front().map(|f| f.pts <= t + margen).unwrap_or(false) {
                    mostrado = self.cola.pop_front();
                }
                if let Some(f) = mostrado { self.pendiente = Some(f); }
                return;
            }
            let t = self.t_reloj + self.reloj.elapsed().as_secs_f64();
            if t >= pr.duracion() {
                self.tocando = false;
                self.sonido.manda(OrdenAudio::Para);
                self.sonido.manda_musica(OrdenAudio::Para);
                self.t = pr.duracion();
                return;
            }
            self.t = t;
            // ¿cruce de junta? → nueva secuencia para el clip entrante
            if let Some((i, src_t)) = pr.en(t) {
                if self.crono && self.ultimo.elapsed().as_secs_f64() > 1.0 && self.cola.is_empty() {
                    eprintln!("  visor: cola VACÍA en src_t={src_t:.2} (¿la cabina no sirve?)");
                    self.ultimo = Instant::now();
                }
                if self.clip_activo != Some(i) {
                    self.gen += 1;
                    self.cola.clear();
                    self.arranca_toca(pr);
                }
                // ── el ritmo: mostrar el último frame cuyo pts ya tocaba ──
                let margen = 0.5 / pr.fps.max(1.0);
                let mut mostrado = None;
                while self.cola.front().map(|f| f.pts <= src_t + margen).unwrap_or(false) {
                    mostrado = self.cola.pop_front();
                }
                if let Some(f) = mostrado {
                    self.pendiente = Some(f);
                    self.ultimo_frame = Instant::now();
                    // fps honesto: fotogramas mostrados por ventana rodante
                    self.fps_cuenta += 1;
                    let v = self.ultimo.elapsed().as_secs_f64();
                    if v >= 0.5 {
                        self.fps_medido = self.fps_cuenta as f64 / v;
                        self.fps_cuenta = 0;
                        self.ultimo = Instant::now();
                    }
                } else if self.cola.is_empty()
                    && self.ultimo_frame.elapsed().as_secs_f64() > 0.15
                    && !pr.clips.get(i).map(|c| c.hueco || crate::foto::es_foto(&c.ruta)).unwrap_or(false)
                {
                    // el reloj espera al fotograma: no dejar la imagen atrás
                    self.t_reloj = self.t;
                    self.reloj = Instant::now();
                }
            }
        } else if let Some((src_t, i, desde)) = self.refina {
            // en pausa y quieto un momento: el máster a resolución completa
            // (debounce generoso: un máster a destiempo bloquea la cabina
            // y le roba el instante al scrub)
            if desde.elapsed().as_secs_f64() > 0.4 {
                self.refina = None;
                if let Some(c) = pr.clips.get(i) {
                    if !c.hueco {
                        self.cabina.manda(Orden::Frame {
                            gen: self.gen, ruta: c.ruta.clone(), t: src_t, tier: Tier::Master,
                        });
                    }
                }
            }
        }
    }

    /// sube el fotograma pendiente y encadena TODOS los pases del look
    pub fn cadena(&mut self, g: &Gpu, pr: &Proyecto, enc: &mut wgpu::CommandEncoder, tiempo: f64) {
        // ¿cambia la resolución de la fuente (proxy ↔ máster)? → texturas nuevas
        if let Some(f) = self.pendiente.as_ref() {
            if (f.w, f.h) != (self.src_w, self.src_h) {
                self.src_w = f.w;
                self.src_h = f.h;
                self.t_y = plano(&g.device, f.w, f.h);
                self.t_u = plano(&g.device, f.w / 2, f.h / 2);
                self.t_v = plano(&g.device, f.w / 2, f.h / 2);
                self.vistas_yuv = (self.t_y.create_view(&Default::default()),
                                   self.t_u.create_view(&Default::default()),
                                   self.t_v.create_view(&Default::default()));
                self.bg_sucio = true;
                // las vistas YUV son OTRAS: el bind group del grade apunta a
                // texturas muertas. Invalidar la huella o la caché de la
                // receta se salta el aplica_cuarto que lo reconstruye y la
                // imagen se queda congelada (bug de la ronda 43, visto en
                // Windows: sin proxies el tamaño cambia en cada refinado).
                self.receta_puesta = None;
                if self.cuarto_pendiente.is_none() {
                    self.cuarto_pendiente = self.clip_activo
                        .or_else(|| pr.en(self.t).map(|x| x.0));
                }
            }
        }
        // ── LA RECETA ES DEL CLIP QUE SE ESTÁ VIENDO ──────────────────────
        // Manda la aguja, no la selección. Y se comprueba EN CADA FOTOGRAMA,
        // no sólo cuando alguien avisa: así el encuadre se ve moverse mientras
        // se arrastra (está en la huella) y cruzar un empalme cambia la receta
        // aunque nadie haya tocado nada.
        self.cuarto_pendiente.take();
        let manda = self.clip_activo.or_else(|| pr.en(self.t).map(|x| x.0));
        if let Some(i) = manda {
            if let Some(c) = pr.clips.get(i) {
                // la huella: si es la MISMA que está puesta, no se toca nada
                // (subir gelatinas 3D cuesta ~750 ms). Lleva el índice del
                // clip y su encuadre, que es lo que faltaba.
                let huella = (i, c.prefs.to_string(), c.lut_in.clone(),
                              c.lut_color.clone(), c.enc);
                if self.receta_puesta.as_ref() != Some(&huella) {
                    self.encuadre = c.enc;
                    self.aplica_cuarto(g, &c.prefs, &c.lut_in, &c.lut_color);
                    self.receta_puesta = Some(huella);
                }
            }
        }
        if let Some(f) = self.pendiente.take() {
            sube(&g.queue, &self.t_y, &f.y, f.w, f.h);
            sube(&g.queue, &self.t_u, &f.u, f.w / 2, f.h / 2);
            sube(&g.queue, &self.t_v, &f.v, f.w / 2, f.h / 2);
            self.frames += 1;
            self.frame_idx += 1;
            self.hay_imagen = true;
        }
        if !self.hay_imagen { return; }

        run_pass(enc, &self.grade, &self.grade_bg,
                 &[&self.targets.graded.view, &self.targets.raw.view]);

        // ── LAS CAPAS (CAPAS §6): hasta dos, compuestas por alfa encima ──
        // Fotos y rótulos van residentes en RGBA; una capa de vídeo se
        // decodifica síncrona (el mismo camino del scrub) y sube como RGBA
        // convertida — la preview dice la verdad también con capas.
        if !pr.capas.is_empty() {
            // la gelatina identidad tiene que existir antes de atar el grupo
            self.carga_lut(g, "");
        }
        for (hueco, (k, t_f, alfa)) in pr.capas_en(self.t).into_iter().enumerate() {
            if alfa <= 0.001 { continue }
            let Some(cp) = pr.capas.get(k) else { continue };
            let vista = if crate::foto::es_foto(&cp.c.ruta) {
                if !self.capa_rgba.contains_key(&cp.c.ruta) {
                    if let Ok((fw, fh, datos)) = filmlook_core::foto::rgba(&cp.c.ruta) {
                        let t = g.device.create_texture(&wgpu::TextureDescriptor {
                            label: Some("capa rgba"),
                            size: wgpu::Extent3d { width: fw, height: fh,
                                                   depth_or_array_layers: 1 },
                            mip_level_count: 1, sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: wgpu::TextureFormat::Rgba8UnormSrgb,
                            usage: wgpu::TextureUsages::TEXTURE_BINDING
                                 | wgpu::TextureUsages::COPY_DST,
                            view_formats: &[],
                        });
                        g.queue.write_texture(
                            wgpu::TexelCopyTextureInfo { texture: &t, mip_level: 0,
                                origin: wgpu::Origin3d::ZERO,
                                aspect: wgpu::TextureAspect::All },
                            &datos,
                            wgpu::TexelCopyBufferLayout { offset: 0,
                                bytes_per_row: Some(fw * 4), rows_per_image: None },
                            wgpu::Extent3d { width: fw, height: fh,
                                             depth_or_array_layers: 1 });
                        let v = t.create_view(&Default::default());
                        self.capa_rgba.insert(cp.c.ruta.clone(), (t, v));
                    }
                }
                self.capa_rgba.get(&cp.c.ruta).map(|(_, v)| v)
            } else {
                // capa de VÍDEO: el fotograma síncrono convertido a RGBA una
                // vez por redraw (proxy o máster; en pausa es exacto)
                let ruta = cp.c.ruta.clone();
                let fr = {
                    if !self.escrutinio.contains_key(&ruta) {
                        if let Ok(mut c2) = filmlook_core::cine::Cine::abre(&ruta) {
                            c2.mitad = crate::prefs::PREVIEW_MEDIA
                                .load(std::sync::atomic::Ordering::Relaxed);
                            self.escrutinio.insert(ruta.clone(), c2);
                        }
                    }
                    self.escrutinio.get_mut(&ruta).and_then(|c2| c2.frame_en(t_f))
                };
                if let Some(f) = fr {
                    let clave_t = PathBuf::from(format!("{}·video", ruta.display()));
                    let rehacer = self.capa_rgba.get(&clave_t)
                        .map(|(t, _)| t.width() != f.w || t.height() != f.h)
                        .unwrap_or(true);
                    if rehacer {
                        let t = g.device.create_texture(&wgpu::TextureDescriptor {
                            label: Some("capa video"),
                            size: wgpu::Extent3d { width: f.w, height: f.h,
                                                   depth_or_array_layers: 1 },
                            mip_level_count: 1, sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: wgpu::TextureFormat::Rgba8Unorm,
                            usage: wgpu::TextureUsages::TEXTURE_BINDING
                                 | wgpu::TextureUsages::COPY_DST,
                            view_formats: &[],
                        });
                        let v = t.create_view(&Default::default());
                        self.capa_rgba.insert(clave_t.clone(), (t, v));
                    }
                    let (t, _) = &self.capa_rgba[&clave_t];
                    // YUV → RGBA de 8 bits en CPU: suficiente para la capa de
                    // la preview (el máster va por su camino de 10 bits)
                    let mut rgba = vec![0u8; (f.w * f.h * 4) as usize];
                    for y in 0..f.h as usize {
                        for x in 0..f.w as usize {
                            let yy = f.y[y * f.w as usize + x] as f32;
                            let cu = f.u[(y / 2) * (f.w as usize / 2) + x / 2] as f32;
                            let cv = f.v[(y / 2) * (f.w as usize / 2) + x / 2] as f32;
                            let yl = ((yy / 1023.0 * 255.0) - 16.0) / 219.0 * 255.0;
                            let ub = (cu / 1023.0 * 255.0) - 128.0;
                            let vb = (cv / 1023.0 * 255.0) - 128.0;
                            let r = (yl + 1.5748 * vb).clamp(0.0, 255.0) as u8;
                            let gg = (yl - 0.1873 * ub - 0.4681 * vb).clamp(0.0, 255.0) as u8;
                            let b = (yl + 1.8556 * ub).clamp(0.0, 255.0) as u8;
                            let o = (y * f.w as usize + x) * 4;
                            rgba[o] = r; rgba[o + 1] = gg; rgba[o + 2] = b; rgba[o + 3] = 255;
                        }
                    }
                    g.queue.write_texture(
                        wgpu::TexelCopyTextureInfo { texture: t, mip_level: 0,
                            origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                        &rgba,
                        wgpu::TexelCopyBufferLayout { offset: 0,
                            bytes_per_row: Some(f.w * 4), rows_per_image: None },
                        wgpu::Extent3d { width: f.w, height: f.h,
                                         depth_or_array_layers: 1 });
                    self.capa_rgba.get(&clave_t).map(|(_, v)| v)
                } else { None }
            };
            let Some(vista) = vista else { continue };
            // el uniforme de esta capa: src_mode 3 (RGBA con alfa), su
            // encuadre sobre el lienzo del proyecto y su alfa como peso
            let (fw, fh) = (vista as *const wgpu::TextureView, ());
            let _ = (fw, fh);
            let dims = self.capa_rgba.iter()
                .find(|(_, (_, vv)) | std::ptr::eq(vv, vista))
                .map(|(_, (t, _))| (t.width(), t.height())).unwrap_or((2, 2));
            let mut gu = params::grade_u_enc(&cp.c.prefs, dims.0, dims.1, 2, 2,
                                             false, false, &cp.c.enc,
                                             self.w, self.h);
            gu.src_mode = 3;
            gu.peso = alfa;
            g.queue.write_buffer(&self.capa_bufs[hueco.min(1)], 0,
                                 bytemuck::bytes_of(&gu));
            let bg = g.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None, layout: &self.grade_capa.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0,
                        resource: self.capa_bufs[hueco.min(1)].as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1,
                        resource: wgpu::BindingResource::TextureView(&self.vistas_yuv.0) },
                    wgpu::BindGroupEntry { binding: 2,
                        resource: wgpu::BindingResource::TextureView(&self.vistas_yuv.1) },
                    wgpu::BindGroupEntry { binding: 3,
                        resource: wgpu::BindingResource::TextureView(&self.vistas_yuv.2) },
                    wgpu::BindGroupEntry { binding: 4,
                        resource: wgpu::BindingResource::TextureView(vista) },
                    wgpu::BindGroupEntry { binding: 5,
                        resource: wgpu::BindingResource::TextureView(
                            &self.cache_lut.get("").map(|(_, v)| v.clone())
                                .unwrap_or_else(|| self.cache_lut.values().next()
                                    .map(|(_, v)| v.clone()).unwrap()) ) },
                    wgpu::BindGroupEntry { binding: 6,
                        resource: wgpu::BindingResource::TextureView(
                            &self.cache_lut.get("").map(|(_, v)| v.clone())
                                .unwrap_or_else(|| self.cache_lut.values().next()
                                    .map(|(_, v)| v.clone()).unwrap()) ) },
                    wgpu::BindGroupEntry { binding: 7,
                        resource: wgpu::BindingResource::Sampler(&self.samp) },
                ],
            });
            run_pass(enc, &self.grade_capa, &bg,
                     &[&self.targets.graded.view, &self.targets.raw.view]);
        }

        let usa_shutter = self.shutter > 0.001;
        if usa_shutter {
            // el arrastre arranca de cero en el primer fotograma Y EN CADA
            // CORTE: sin esto, el plano que se va asomaba en el que entra
            let reset = if self.frame_idx <= 1 || self.corte_pendiente { 1u32 } else { 0u32 };
            self.corte_pendiente = false;
            let mut ub = [0u8; 16];
            ub[0..4].copy_from_slice(&self.shutter.to_le_bytes());
            ub[4..8].copy_from_slice(&reset.to_le_bytes());
            g.queue.write_buffer(&self.small_u, 0, &ub);
            let bg = g.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None, layout: &self.accum.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.small_u.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.targets.graded.view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.targets.h_a.view) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.samp) },
                ],
            });
            run_pass(enc, &self.accum, &bg, &[&self.targets.h_b.view]);
            std::mem::swap(&mut self.targets.h_a, &mut self.targets.h_b);
        }
        let base_es_h = usa_shutter;

        let peque = |pass: &Pass, buf: wgpu::Buffer, v: &wgpu::TextureView| {
            g.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None, layout: &pass.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(v) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.samp) },
                ],
            })
        };
        let bg_down = |v: &wgpu::TextureView, tw: u32, th: u32| {
            let mut ub = [0u8; 16];
            ub[0..4].copy_from_slice(&(1.0f32 / tw as f32).to_le_bytes());
            ub[4..8].copy_from_slice(&(1.0f32 / th as f32).to_le_bytes());
            peque(&self.down, uniform_buffer(&g.device, &ub), v)
        };
        {
            let base: &Target = if base_es_h { &self.targets.h_a } else { &self.targets.graded };
            let bg = bg_down(&base.view, base.w, base.h);
            run_pass(enc, &self.down, &bg, &[&self.targets.b0.view]);
        }
        let bg = bg_down(&self.targets.b0.view, self.targets.b0.w, self.targets.b0.h);
        run_pass(enc, &self.down, &bg, &[&self.targets.c0.view]);
        let bg = bg_down(&self.targets.c0.view, self.targets.c0.w, self.targets.c0.h);
        run_pass(enc, &self.down, &bg, &[&self.targets.d0.view]);

        let spread = self.comp_u.hal_spread;
        let bg_blur = |v: &wgpu::TextureView, dir: [f32; 2]| {
            let mut ub = [0u8; 16];
            ub[0..4].copy_from_slice(&dir[0].to_le_bytes());
            ub[4..8].copy_from_slice(&dir[1].to_le_bytes());
            ub[8..12].copy_from_slice(&1.0f32.to_le_bytes());
            peque(&self.blur, uniform_buffer(&g.device, &ub), v)
        };
        let mut difumina = |enc: &mut wgpu::CommandEncoder, a: &Target, b: &Target, rad: f32| {
            let bg1 = bg_blur(&a.view, [rad / a.w as f32, 0.0]);
            run_pass(enc, &self.blur, &bg1, &[&b.view]);
            let bg2 = bg_blur(&b.view, [0.0, rad / a.h as f32]);
            run_pass(enc, &self.blur, &bg2, &[&a.view]);
        };
        difumina(enc, &self.targets.b0, &self.targets.b1, 7.0);
        difumina(enc, &self.targets.c0, &self.targets.c1, 1.5 + spread * 2.0);
        difumina(enc, &self.targets.d0, &self.targets.d1, 4.0 + spread * 6.0);

        // los uniformes vivos del comp (tiempo, semilla, gate weave, wipe)
        let time = tiempo as f32;
        let mut cu = self.comp_u;
        cu.time = time;
        cu.seed = (self.frame_idx % 997) as f32;
        cu.wipe = if self.wipe { 0.5 } else { 1.0 };
        let wr = 1.4f32;
        let w0 = self.weave_amp;
        cu.weave_px_x = w0 * ((time * wr * 1.7).sin() + 0.5 * (time * wr * 3.1 + 1.3).sin()) / 1.5;
        cu.weave_px_y = w0 * ((time * wr * 2.3 + 0.7).sin() + 0.5 * (time * wr * 4.3 + 2.1).sin()) / 1.5;
        g.queue.write_buffer(&self.comp_buf, 0, bytemuck::bytes_of(&cu));

        let base: &Target = if base_es_h { &self.targets.h_a } else { &self.targets.graded };
        self.comp_bg = Some(g.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None, layout: &self.present.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.comp_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&base.view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.targets.raw.view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&self.targets.b0.view) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&self.targets.c0.view) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&self.targets.d0.view) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&self.view_grain) },
                // el 7 muestrea la IMAGEN (recorte) y el 8 la PLACA DE GRANO
                // (repetición): la placa está hecha por FFT para teselar sin
                // costura y antes se muestreaba todo con el mismo
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::Sampler(&self.samp) },
                wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::Sampler(&self.samp_rep) },
            ],
        }));
        self.lupa_bg = Some(g.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lupa"), layout: &self.present.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.lupa_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&base.view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.targets.raw.view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&self.targets.b0.view) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&self.targets.c0.view) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&self.targets.d0.view) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&self.view_grain) },
                // el 7 muestrea la IMAGEN (recorte) y el 8 la PLACA DE GRANO
                // (repetición): la placa está hecha por FFT para teselar sin
                // costura y antes se muestreaba todo con el mismo
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::Sampler(&self.samp) },
                wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::Sampler(&self.samp_rep) },
            ],
        }));
        // el uniform de la lupa: el mismo comp con el aumento puesto
        let mut lu = cu;
        lu.lupa = 4.0;
        let [vx, vy, vw, vh] = self.rect_pantalla;
        let (cx2, cy2) = self.lupa_centro;
        lu.lupa_cx = ((cx2 - vx) / vw.max(0.001)).clamp(0.0, 1.0);
        lu.lupa_cy = ((cy2 - vy) / vh.max(0.001)).clamp(0.0, 1.0);
        g.queue.write_buffer(&self.lupa_buf, 0, bytemuck::bytes_of(&lu));
    }

    /// el vidrio: el comp entra en el rectángulo del visor (viewport)
    pub fn pinta(&self, rp: &mut wgpu::RenderPass, escala: f32) {
        let r = self.rect_pantalla;
        self.pinta_en(rp, escala, r);
    }

    /// EL MISMO VIDRIO EN OTRO SITIO. Lo usa la ventana del VIGÍA (§3): el
    /// visor suelto para el segundo monitor no repite la cadena, dibuja el
    /// mismo resultado en otra superficie del mismo dispositivo.
    pub fn pinta_en(&self, rp: &mut wgpu::RenderPass, escala: f32, rect: [f32; 4]) {
        let Some(bg) = self.comp_bg.as_ref() else { return };
        let [x, y, w, h] = rect;
        if w < 1.0 || h < 1.0 { return; }
        rp.set_viewport(x * escala, y * escala, w * escala, h * escala, 0.0, 1.0);
        rp.set_pipeline(&self.present.pipeline);
        rp.set_bind_group(0, bg, &[]);
        rp.draw(0..3, 0..1);
    }

    /// la proporción del lienzo (la usa el vigía para encajar su ventana)
    pub fn proporcion(&self) -> f32 { self.aspecto }

    /// LA LUPA CUENTAHÍLOS: la misma imagen ampliada ×n dentro de un recuadro
    /// (viewport escalado + tijera al recuadro: cero pases extra de cadena)
    pub fn pinta_lupa(&self, rp: &mut wgpu::RenderPass, escala: f32,
                      lx: f32, ly: f32, lado: f32, fis_w: f32, fis_h: f32) {
        let Some(bg) = self.lupa_bg.as_ref() else { return };
        let [x, y, w, h] = self.rect_pantalla;
        // el recuadro del cuentahílos, en píxeles físicos y dentro del vidrio
        let x0 = (lx - lado / 2.0).max(x);
        let y0 = (ly - lado / 2.0).max(y);
        let x1 = (lx + lado / 2.0).min(x + w);
        let y1 = (ly + lado / 2.0).min(y + h);
        if x1 <= x0 + 2.0 || y1 <= y0 + 2.0 { return; }
        let (rx, ry) = ((x0 * escala) as u32, (y0 * escala) as u32);
        let rw = (((x1 - x0) * escala) as u32).min(fis_w as u32 - rx);
        let rh = (((y1 - y0) * escala) as u32).min(fis_h as u32 - ry);
        if rw == 0 || rh == 0 { return; }
        // el aumento va en el UNIFORM (el viewport no puede salirse del target)
        rp.set_scissor_rect(rx, ry, rw, rh);
        rp.set_viewport(x * escala, y * escala, w * escala, h * escala, 0.0, 1.0);
        rp.set_pipeline(&self.present.pipeline);
        rp.set_bind_group(0, bg, &[]);
        rp.draw(0..3, 0..1);
        rp.set_scissor_rect(0, 0, fis_w as u32, fis_h as u32);
    }
}

fn sube(q: &wgpu::Queue, t: &wgpu::Texture, d: &[u16], w: u32, h: u32) {
    q.write_texture(
        wgpu::TexelCopyTextureInfo { texture: t, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        bytemuck::cast_slice(d),
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 2), rows_per_image: Some(h) },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
}
