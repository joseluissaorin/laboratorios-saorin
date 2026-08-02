//! Vídeo por pipes ffmpeg (v1): decode yuv420p10le crudo, encode rgba crudo.
//! El camino zero-copy (VideoToolbox/MF/VAAPI) se enchufa aquí después.

use std::io::{Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

pub struct FfmpegDecoder {
    child: Child,
    stdout: ChildStdout,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    ybuf: Vec<u8>,
    cbuf: Vec<u8>,
}

pub fn probe(path: &str) -> anyhow::Result<(u32, u32, f64, f64)> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0",
               "-show_entries", "stream=width,height,r_frame_rate,duration",
               "-of", "json"])
        .arg(path)
        .output()?;
    let j: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let s = &j["streams"][0];
    let w = s["width"].as_u64().unwrap() as u32;
    let h = s["height"].as_u64().unwrap() as u32;
    let rate = s["r_frame_rate"].as_str().unwrap_or("24/1");
    let mut it = rate.split('/');
    let num: f64 = it.next().unwrap().parse().unwrap_or(24.0);
    let den: f64 = it.next().and_then(|v| v.parse().ok()).unwrap_or(1.0);
    let dur = s["duration"].as_str().and_then(|v| v.parse().ok()).unwrap_or(0.0);
    Ok((w, h, num / den.max(1.0), dur))
}

impl FfmpegDecoder {
    pub fn open(path: &str) -> anyhow::Result<Self> {
        let (width, height, _, _) = probe(path)?;
        Self::open_at(path, 0.0, width, height)
    }

    /// Abre con seek (-ss, rápido por keyframe) y escalado en el decoder —
    /// para la preview: menos bytes por el pipe = menos ms por frame.
    pub fn open_at(path: &str, start_s: f64, out_w: u32, out_h: u32) -> anyhow::Result<Self> {
        let (_, _, fps, _) = probe(path)?;
        let (width, height) = (out_w & !1, out_h & !1);
        #[cfg(target_os = "macos")]
        let hwaccel = "videotoolbox";
        #[cfg(target_os = "windows")]
        let hwaccel = "d3d11va";
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let hwaccel = "auto";
        let ss = format!("{:.3}", start_s.max(0.0));
        let vf = format!("scale={}:{}", width, height);
        let mut child = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error",
                   "-hwaccel", hwaccel,
                   "-ss", &ss,
                   "-i", path,
                   "-vf", &vf,
                   "-f", "rawvideo", "-pix_fmt", "yuv420p10le", "-"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdout = child.stdout.take().unwrap();
        let ybuf = vec![0u8; (width * height * 2) as usize];
        let cbuf = vec![0u8; (width * height) as usize];   // U+V (w/2·h/2·2B cada uno)
        Ok(FfmpegDecoder { child, stdout, width, height, fps, ybuf, cbuf })
    }

    /// Lee el siguiente frame como planos u16 (Y, U, V). None = fin.
    pub fn next_frame(&mut self) -> Option<(&[u16], &[u16], &[u16])> {
        if self.stdout.read_exact(&mut self.ybuf).is_err() { return None; }
        if self.stdout.read_exact(&mut self.cbuf).is_err() { return None; }
        let (y, rest) = self.ybuf.split_at((self.width * self.height * 2) as usize);
        let _ = rest;
        let half = (self.width * self.height / 2) as usize;   // bytes por plano croma
        let (u, v) = self.cbuf.split_at(half);
        let cast = |b: &[u8]| -> &[u16] {
            unsafe { std::slice::from_raw_parts(b.as_ptr() as *const u16, b.len() / 2) }
        };
        Some((cast(y), cast(u), cast(v)))
    }
}

impl Drop for FfmpegDecoder {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

pub struct FfmpegEncoder {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl FfmpegEncoder {
    pub fn new(out: &str, w: u32, h: u32, fps: f64, audio_src: &str, codec: &str) -> anyhow::Result<Self> {
        let (cv, profile, pix) = if codec == "hevc_videotoolbox" {
            ("hevc_videotoolbox", vec![], "yuv420p10le")
        } else {
            ("prores_ks", vec!["-profile:v", "4"], "yuv444p10le")
        };
        let mut args: Vec<String> = vec![
            "-hide_banner", "-loglevel", "error", "-y",
            "-f", "rawvideo", "-pix_fmt", "rgba",
            "-s", &format!("{}x{}", w, h),
            "-framerate", &fps.to_string(), "-i", "-",
            "-i", audio_src, "-map", "0:v", "-map", "1:a?",
            "-c:v", cv,
        ].into_iter().map(String::from).collect();
        args.extend(profile.into_iter().map(String::from));
        args.extend(["-c:a", "aac", "-b:a", "256k", "-pix_fmt", pix, out].into_iter().map(String::from));
        let mut child = Command::new("ffmpeg")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take();
        Ok(FfmpegEncoder { child, stdin })
    }

    pub fn write_frame(&mut self, rgba: &[u8]) {
        if let Some(s) = self.stdin.as_mut() { let _ = s.write_all(rgba); }
    }

    pub fn finish(mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}
