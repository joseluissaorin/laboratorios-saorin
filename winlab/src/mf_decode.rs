//! Decode zero-copy con Media Foundation: SourceReader con el dispositivo
//! D3D11 → HEVC 10-bit por hardware (VCN) → texturas P010 en GPU.

use anyhow::{ensure, Result};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Media::MediaFoundation::*;

use crate::d11::D11;

pub struct MfDecoder {
    reader: IMFSourceReader,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration_s: f64,
    /// tamaño del array de texturas del pool del decoder (para subrecursos de plano)
    pub array_size: u32,
    /// el segundo del último fotograma servido (−1 = aún no se ha leído nada)
    ultimo: f64,
}

unsafe impl Send for MfDecoder {}

pub struct GpuFrame {
    pub tex: ID3D11Texture2D,
    pub subres: u32,          // slice del array (plano 0); plano 1 = array_size + slice
    pub pts_100ns: i64,
    pub sample: IMFSample,    // retiene la textura viva mientras se usa
}

unsafe impl Send for GpuFrame {}

impl MfDecoder {
    pub fn new(d11: &D11, path: &str) -> Result<Self> {
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL)? };

        // device manager de DXGI apuntando a NUESTRO dispositivo
        let mut token = 0u32;
        let mut manager: Option<IMFDXGIDeviceManager> = None;
        unsafe { MFCreateDXGIDeviceManager(&mut token, &mut manager)? };
        let manager = manager.unwrap();
        unsafe { manager.ResetDevice(&d11.device, token)? };

        let mut attrs: Option<IMFAttributes> = None;
        unsafe { MFCreateAttributes(&mut attrs, 4)? };
        let attrs = attrs.unwrap();
        unsafe {
            attrs.SetUnknown(&MF_SOURCE_READER_D3D_MANAGER, &manager)?;
            attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
            // pool del reader con BIND_SHADER_RESOURCE: permite SRVs por plano
            // directamente sobre la textura del decoder (split sin copia previa)
            attrs.SetUINT32(&MF_SA_D3D11_BINDFLAGS,
                windows::Win32::Graphics::Direct3D11::D3D11_BIND_SHADER_RESOURCE.0 as u32)?;
            attrs.SetUINT32(&MF_SOURCE_READER_DISABLE_DXVA, 0)?;
        }

        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let reader = unsafe {
            MFCreateSourceReaderFromURL(windows::core::PCWSTR(wide.as_ptr()), &attrs)?
        };

        // solo vídeo
        unsafe {
            reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
            reader.SetStreamSelection(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, true)?;
        }

        // salida P010 (10-bit biplanar en GPU)
        let out_ty = unsafe { MFCreateMediaType()? };
        unsafe {
            out_ty.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            out_ty.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_P010)?;
            reader.SetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, None, &out_ty)?;
        }

        // dimensiones y fps reales
        let cur = unsafe { reader.GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)? };
        let size = unsafe { cur.GetUINT64(&MF_MT_FRAME_SIZE)? };
        let (width, height) = ((size >> 32) as u32, (size & 0xffff_ffff) as u32);
        let rate = unsafe { cur.GetUINT64(&MF_MT_FRAME_RATE).unwrap_or((30 << 32) | 1) };
        let fps = (rate >> 32) as f64 / ((rate & 0xffff_ffff) as f64).max(1.0);

        let duration_s = 0.0;   // (si hiciera falta, ffprobe fuera)

        ensure!(width > 0 && height > 0, "sin dimensiones de vídeo");
        Ok(MfDecoder { reader, width, height, fps, duration_s, array_size: 0, ultimo: -1.0 })
    }

    /// siguiente frame decodificado (bloqueante); None al acabar
    pub fn next(&mut self) -> Result<Option<GpuFrame>> {
        loop {
            let mut stream = 0u32;
            let mut flags = 0u32;
            let mut pts = 0i64;
            let mut sample: Option<IMFSample> = None;
            unsafe {
                self.reader.ReadSample(
                    MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                    0,
                    Some(&mut stream),
                    Some(&mut flags),
                    Some(&mut pts),
                    Some(&mut sample),
                )?;
            }
            if flags & (MF_SOURCE_READERF_ENDOFSTREAM.0 as u32) != 0 {
                return Ok(None);
            }
            let Some(sample) = sample else { continue };
            let buf = unsafe { sample.GetBufferByIndex(0)? };
            let dxgi: IMFDXGIBuffer = buf.cast()?;
            let mut tex: Option<ID3D11Texture2D> = None;
            unsafe {
                dxgi.GetResource(&ID3D11Texture2D::IID, &mut tex as *mut _ as *mut *mut std::ffi::c_void)?
            };
            let tex = tex.unwrap();
            let subres = unsafe { dxgi.GetSubresourceIndex()? };
            if self.array_size == 0 {
                let mut desc = Default::default();
                unsafe { tex.GetDesc(&mut desc) };
                self.array_size = desc.ArraySize;
            }
            self.ultimo = pts as f64 / 1e7;
            return Ok(Some(GpuFrame { tex, subres, pts_100ns: pts, sample }));
        }
    }

    /// EL SALTO. Media Foundation se encarga de retroceder al fotograma clave
    /// anterior y de descartar el arranque: aquí solo se le dice el segundo.
    /// Es lo que permite que el motor corte él solo y que desaparezca la fase
    /// de corte con ffmpeg (MOTOR §5bis).
    pub fn busca(&mut self, t: f64) -> Result<()> {
        use windows::core::PROPVARIANT;
        let pos = PROPVARIANT::from((t.max(0.0) * 1e7) as i64);
        unsafe { self.reader.SetCurrentPosition(&windows::core::GUID::zeroed(), &pos)? };
        self.ultimo = -1.0;
        Ok(())
    }

    /// El fotograma que cubre el segundo `t`, avanzando desde donde estemos.
    /// Solo salta hacia atrás o cuando el salto es largo: dentro de un clip se
    /// lee de corrido, que es como va rápido.
    pub fn en(&mut self, t: f64) -> Result<Option<GpuFrame>> {
        let medio = 0.5 / self.fps.max(1.0);
        if self.ultimo < 0.0 || t < self.ultimo - 0.001 || t > self.ultimo + 1.5 {
            self.busca(t)?;
        }
        // ── MEDIA FOUNDATION DICE «SE ACABÓ» ANTES DE TIEMPO ────────────
        //
        // Medido: en un fichero de 11,411 s (684 fotogramas), tras un salto a
        // 6,1 s el lector daba fin de flujo en el segundo **10,9** — treinta
        // fotogramas antes del final de verdad. El motor entendía que el
        // material se había acabado, cortaba el tramo, y el máster salía sin
        // el último plano: 52,4 s de una bobina de 71. Con el mismo fichero
        // leído de corrido (sin salto) llegaba entero, y ffmpeg también.
        //
        // Así que un fin de flujo **por debajo de la duración declarada** no
        // se cree a la primera: se vuelve a buscar y se reintenta. Si a la
        // segunda insiste, entonces sí se ha acabado.
        for reintento in 0..2 {
            loop {
                match self.next()? {
                    Some(f) => {
                        if f.pts_100ns as f64 / 1e7 + medio >= t { return Ok(Some(f)); }
                    }
                    None => break,
                }
            }
            // SIN CONDICIONAR A LA DURACIÓN DECLARADA: es justo la que
            // miente. En el fichero medido, MF daba fin de flujo en 10,9 s y
            // declaraba una duración acorde, mientras que el contenedor tiene
            // 684 fotogramas hasta 11,411 y ffmpeg los lee todos. Fiarse de
            // su duración era preguntarle al mentiroso si miente.
            //
            // El reintento cuesta un salto, y sólo ocurre UNA vez por fuente
            // y tramo: cuando el material se acaba de verdad, la segunda
            // vuelta devuelve `None` y el motor sigue su camino.
            if reintento == 1 { return Ok(None); }
            eprintln!("   ⚠ fin de flujo en {t:.3} s (MF declara {:.3} s): vuelvo a buscar",
                      self.duration_s);
            self.busca(t)?;
        }
        Ok(None)
    }
}
