//! Encode HEVC 10-bit por hardware (VCN) vía Media Foundation Transform
//! asíncrono, con entrada P010 en GPU y salida Annex-B → mux ffmpeg (con audio).

use anyhow::{anyhow, ensure, Result};

#[allow(dead_code)]
fn winquiet<S: AsRef<std::ffi::OsStr>>(program: S) -> std::process::Command {
    #[allow(unused_mut)]
    let mut c = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c
}
use std::io::Write;
use std::process::{Child, ChildStdin, Command, Stdio};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Media::MediaFoundation::*;

use crate::d11::D11;

const RING: usize = 8;

pub struct MfEncoder {
    pub d: D11,                      // dispositivo PROPIO (sin contención con el decoder)
    mft: IMFTransform,
    events: IMFMediaEventGenerator,
    p010: Vec<ID3D11Texture2D>,
    next_slot: usize,
    need_input: usize,
    pending: std::collections::VecDeque<(usize, i64)>,   // p010 listos → MFT
    fps: f64,
    mux: Option<ChildStdin>,
    child: Option<Child>,
    pub frames_out: usize,
}

unsafe impl Send for MfEncoder {}

/// EL SONIDO DEL TRAMO, no el del fichero entero. Cuando el corte lo hace el
/// propio motor (`--desde`/`--cuantos`), el mux tiene que recortar el audio en
/// el mismo sitio; si no, la pieza saldría con la banda sonora completa pegada
/// al principio. `FL_AUDIO_SS`/`FL_AUDIO_T` los pone `run()` con los mismos
/// números del corte.
pub fn recorte_audio(audio_src: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    // SIN FUENTE DE SONIDO no hay que darle un `-i` vacío: la bobina puede
    // tener veinte clips y su mezcla la hornea el taller aparte, así que el
    // motor entrega vídeo mudo y el mux se hace fuera.
    if audio_src.is_empty() { return v; }
    if let Ok(ss) = std::env::var("FL_AUDIO_SS") {
        v.push("-ss".into()); v.push(ss);
    }
    if let Ok(t) = std::env::var("FL_AUDIO_T") {
        v.push("-t".into()); v.push(t);
    }
    v.push("-i".into()); v.push(audio_src.to_string());
    v
}

impl MfEncoder {
    pub fn new(w: u32, h: u32, fps: f64, bitrate: u32,
               out_path: &str, audio_src: &str,
               ring_h: &[windows::Win32::Foundation::HANDLE]) -> Result<Self> {
        let d = D11::new()?;   // dispositivo dedicado del encoder
        let d = &d.clone();
        // mux: ffmpeg -f hevc → mp4 (+ audio del origen)
        let ffmpeg = if std::path::Path::new(r"C:\ProgramData\chocolatey\bin\ffmpeg.exe").exists() {
            r"C:\ProgramData\chocolatey\bin\ffmpeg.exe"
        } else { "ffmpeg" };
        let mut child = winquiet(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y",
                   "-f", "hevc", "-r", &format!("{:.3}", fps), "-i", "-"])
            // el recorte del sonido va PEGADO a su entrada, antes del `-i`:
            // ffmpeg es posicional y un `-b:a` suelto delante de un fichero
            // se toma como opción de ESA entrada («cannot be applied to input
            // url … Move this option before the file it belongs to»)
            .args(recorte_audio(audio_src))
            .args(if audio_src.is_empty() { vec!["-map", "0:v"] }
                  else { vec!["-map", "0:v", "-map", "1:a?"] })
            .args([
                   "-c:v", "copy", "-c:a", "aac", "-b:a", "256k",
                   "-bsf:v",
                   // EL COLOR, ESCRITO DENTRO DEL BITSTREAM.
                   //
                   // Con `-c:v copy` ffmpeg NO puede tocar el VUI que va dentro
                   // del HEVC: `-colorspace bt709` solo escribe la etiqueta del
                   // contenedor y el reproductor hace caso a la de dentro. El
                   // codificador de AMD estaba marcando el máster como
                   // **BT.2020 + PQ (HDR)** cuando los datos son Rec.709 SDR,
                   // así que cualquier reproductor le aplicaba un mapeo de tonos
                   // de HDR: quemados los altos, aplastados los bajos, y de
                   // regalo más contraste y más saturación que la preview.
                   //
                   // 1 = BT.709 en las tres · rango limitado (que es lo que
                   // produce el empaquetado: 64..940).
                   "hevc_metadata=colour_primaries=1:transfer_characteristics=1\
:matrix_coefficients=1:video_full_range_flag=0",
                   "-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709",
                   "-color_range", "tv",
                   out_path])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;
        let mux = child.stdin.take();

        // MFT de encoder HEVC por hardware
        let reg = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: MFVideoFormat_HEVC };
        let mut acts: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count = 0u32;
        unsafe {
            MFTEnumEx(
                MFT_CATEGORY_VIDEO_ENCODER,
                MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
                None,
                Some(&reg),
                &mut acts,
                &mut count,
            )?;
        }
        ensure!(count > 0, "sin encoder HEVC por hardware");
        let act = unsafe { (*acts).clone().ok_or_else(|| anyhow!("activate nulo"))? };
        let mft: IMFTransform = unsafe { act.ActivateObject()? };

        // asíncrono + D3D + baja latencia (sin lookahead)
        let attrs = unsafe { mft.GetAttributes()? };
        unsafe {
            attrs.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)?;
            let _ = attrs.SetUINT32(&MF_LOW_LATENCY, 1);
        }

        let mut token = 0u32;
        let mut manager: Option<IMFDXGIDeviceManager> = None;
        unsafe { MFCreateDXGIDeviceManager(&mut token, &mut manager)? };
        let manager = manager.unwrap();
        unsafe { manager.ResetDevice(&d.device, token)? };
        unsafe {
            mft.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager.as_raw() as usize)?;
        }

        // tipo de salida primero (así el MFT ofrece entradas compatibles)
        let out_ty = unsafe { MFCreateMediaType()? };
        unsafe {
            out_ty.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            out_ty.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_HEVC)?;
            out_ty.SetUINT32(&MF_MT_AVG_BITRATE, bitrate)?;
            out_ty.SetUINT64(&MF_MT_FRAME_SIZE, ((w as u64) << 32) | h as u64)?;
            let num = (fps * 1000.0).round() as u64;
            out_ty.SetUINT64(&MF_MT_FRAME_RATE, (num << 32) | 1000)?;
            out_ty.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            out_ty.SetUINT32(&MF_MT_VIDEO_PROFILE, 2)?;   // eAVEncH265VProfile_Main_420_10
            mft.SetOutputType(0, &out_ty, 0)?;
        }

        // entrada P010
        let in_ty = unsafe { MFCreateMediaType()? };
        unsafe {
            in_ty.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            in_ty.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_P010)?;
            in_ty.SetUINT64(&MF_MT_FRAME_SIZE, ((w as u64) << 32) | h as u64)?;
            let num = (fps * 1000.0).round() as u64;
            in_ty.SetUINT64(&MF_MT_FRAME_RATE, (num << 32) | 1000)?;
            in_ty.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            mft.SetInputType(0, &in_ty, 0)?;
        }

        // afinado del encoder: velocidad, sin B-frames, GOP 60 (como el motor del Mac)
        unsafe {
            use windows::Win32::Media::MediaFoundation::ICodecAPI;
            use windows::core::VARIANT;
            if let Ok(capi) = mft.cast::<ICodecAPI>() {
                let set = |guid: &windows::core::GUID, v: u32| {
                    let var = VARIANT::from(v);
                    let _ = unsafe { capi.SetValue(guid, &var) };
                };
                set(&CODECAPI_AVEncCommonQualityVsSpeed, 0);      // 0 = velocidad máxima
                set(&CODECAPI_AVEncMPVDefaultBPictureCount, 0);   // sin B-frames
                set(&CODECAPI_AVEncMPVGOPSize, 300);
                set(&CODECAPI_AVEncCommonRateControlMode, 3);     // CBR
                set(&CODECAPI_AVEncCommonMeanBitRate, bitrate);
                set(&CODECAPI_AVLowLatencyMode, 1);
            }
        }
        unsafe {
            mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
        }

        let events: IMFMediaEventGenerator = mft.cast()?;
        let p010 = ring_h.iter().map(|&h| d.open_shared(h)).collect::<Result<Vec<_>>>()?;

        Ok(MfEncoder {
            d: d.clone(), mft, events, p010, next_slot: 0, need_input: 0,
            pending: Default::default(), fps,
            mux, child: Some(child), frames_out: 0,
        })
    }

    /// bombea eventos; si `wait`, bloquea hasta el siguiente
    fn pump(&mut self, wait: bool) -> Result<bool> {
        let flags = if wait { MF_EVENT_FLAG_NONE } else { MF_EVENT_FLAG_NO_WAIT };
        match unsafe { self.events.GetEvent(flags) } {
            Ok(ev) => {
                let ty = unsafe { ev.GetType()? } as i32;
                if ty == METransformNeedInput.0 {
                    self.need_input += 1;
                } else if ty == METransformHaveOutput.0 {
                    self.drain_output()?;
                }
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }

    fn drain_output(&mut self) -> Result<()> {
        let mut out = [MFT_OUTPUT_DATA_BUFFER::default()];
        let mut status = 0u32;
        let hr = unsafe { self.mft.ProcessOutput(0, &mut out, &mut status) };
        if hr.is_err() { return Ok(()); }
        if let Some(sample) = out[0].pSample.take() {
            let buf = unsafe { sample.ConvertToContiguousBuffer()? };
            let mut ptr = std::ptr::null_mut();
            let mut len = 0u32;
            unsafe { buf.Lock(&mut ptr, None, Some(&mut len))? };
            let data = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
            if let Some(m) = self.mux.as_mut() {
                let _ = m.write_all(data);
            }
            unsafe { buf.Unlock()? };
            self.frames_out += 1;
        }
        Ok(())
    }

    fn feed_one(&mut self) -> Result<bool> {
        if self.need_input == 0 || self.pending.is_empty() { return Ok(false); }
        let (slot, frame_idx) = self.pending.pop_front().unwrap();
        self.need_input -= 1;
        let dst = self.p010[slot].clone();
        let buf = unsafe { MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &dst, 0, false)? };
        let sample = unsafe { MFCreateSample()? };
        unsafe {
            sample.AddBuffer(&buf)?;
            let t = (frame_idx as f64 * 10_000_000.0 / self.fps).round() as i64;
            let dur = (10_000_000.0 / self.fps).round() as i64;
            sample.SetSampleTime(t)?;
            sample.SetSampleDuration(dur)?;
            self.mft.ProcessInput(0, &sample, 0)?;
        }
        Ok(true)
    }

    /// el P010 del slot ya viene lleno (copias por plano en la cola D3D12)
    pub fn submit_slot(&mut self, slot: usize, frame_idx: i64) -> Result<()> {
        // si el anillo está a tope, hay que esperar créditos de verdad
        while self.pending.len() >= RING - 1 {
            self.pump(true)?;
            while self.feed_one()? {}
        }
        self.next_slot += 1;
        self.pending.push_back((slot, frame_idx));
        // atiende eventos pendientes y alimenta lo que se pueda, sin bloquear
        while self.pump(false)? {}
        while self.feed_one()? {}
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        // vacía la cola interna
        while !self.pending.is_empty() {
            self.pump(true)?;
            while self.feed_one()? {}
        }
        unsafe {
            self.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)?;
            self.mft.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)?;
        }
        // apura la cola hasta el DrainComplete
        loop {
            match unsafe { self.events.GetEvent(MF_EVENT_FLAG_NONE) } {
                Ok(ev) => {
                    let ty = unsafe { ev.GetType()? } as i32;
                    if ty == METransformHaveOutput.0 { self.drain_output()?; }
                    else if ty == METransformDrainComplete.0 { break; }
                }
                Err(_) => break,
            }
        }
        if let Some(m) = self.mux.take() { drop(m); }
        if let Some(mut c) = self.child.take() {
            let out = c.wait_with_output()?;
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.trim().is_empty() { eprintln!("ffmpeg mux: {}", &err[..err.len().min(400)]); }
        }
        eprintln!("   encoder: {} frames escritos", self.frames_out);
        Ok(())
    }
}
