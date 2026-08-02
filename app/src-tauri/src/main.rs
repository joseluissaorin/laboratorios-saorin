// film-look lab — backend Tauri: ficheros nativos, conversión LUT y render
// (ffmpeg pipe) sin servidor web ni navegador externo.

use serde_json::json;
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use tauri::Manager;

struct RenderState(Mutex<Option<Child>>);

/// Mini-servidor HTTP en 127.0.0.1:8741 para el canal de frames (mucho más
/// rápido que el puente IPC de la webview: ~10 ms vs ~117 ms por frame 4K).
/// POST /render_start (json) · POST /render_frame (raw RGBA) · POST /render_done
fn start_frame_server(state: &'static RenderState) {
    use std::io::{BufRead, BufReader, Read};
    use std::net::TcpListener;
    std::thread::spawn(move || {
        let listener = match TcpListener::bind("127.0.0.1:8741") {
            Ok(l) => l,
            Err(e) => { eprintln!("frame server: {e}"); return; }
        };
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() { continue; }
            let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
            let mut content_len = 0usize;
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).is_err() || h == "\r\n" { break; }
                if let Some(v) = h.to_lowercase().strip_prefix("content-length:") {
                    content_len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; content_len];
            let _ = reader.read_exact(&mut body);
            let resp = if line.starts_with("OPTIONS") {
                "200 OK"
            } else { match path.as_str() {
                "/render_start" => {
                    match serde_json::from_slice::<serde_json::Value>(&body) {
                        Ok(job) => {
                            let out = job["out"].as_str().unwrap_or("").to_string();
                            let fps = job["fps"].as_f64().unwrap_or(24.0);
                            let w = job["width"].as_u64().unwrap_or(1920);
                            let h = job["height"].as_u64().unwrap_or(1080);
                            let audio = job["audioSrc"].as_str().unwrap_or("").to_string();
                            let codec = job["codec"].as_str().unwrap_or("prores_ks");
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
                                "-i", &audio, "-map", "0:v", "-map", "1:a?",
                                "-vf", "vflip", "-c:v", cv,
                            ].into_iter().map(String::from).collect();
                            args.extend(profile.into_iter().map(String::from));
                            args.extend(["-c:a", "aac", "-b:a", "256k", "-pix_fmt", pix, &out]
                                .into_iter().map(String::from));
                            match Command::new("ffmpeg").args(&args)
                                .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null())
                                .spawn() {
                                Ok(child) => { *RENDER_STATE.0.lock().unwrap() = Some(child); "200 OK" }
                                Err(_) => "500 ERR",
                            }
                        }
                        Err(_) => "400 ERR",
                    }
                }
                "/render_frame" => {
                    let mut g = RENDER_STATE.0.lock().unwrap();
                    if let Some(child) = g.as_mut() {
                        if let Some(stdin) = child.stdin.as_mut() {
                            let _ = stdin.write_all(&body);
                        }
                    }
                    "204 OK"
                }
                "/render_done" => {
                    let mut g = RENDER_STATE.0.lock().unwrap();
                    if let Some(mut child) = g.take() {
                        if let Some(stdin) = child.stdin.take() { drop(stdin); }
                        let _ = child.wait();
                    }
                    "200 OK"
                }
                _ => "404",
            } };
            let _ = stream.write_all(format!(
                "HTTP/1.1 {} x\r\nContent-Length: 0\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: POST, OPTIONS\r\nAccess-Control-Allow-Headers: *\r\nConnection: close\r\n\r\n", resp
            ).as_bytes());
        }
    });
}

#[tauri::command]
fn read_bytes(path: String) -> Result<Vec<u8>, String> {
    std::fs::read(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn write_text(path: String, text: String) -> Result<(), String> {
    std::fs::write(&path, text).map_err(|e| e.to_string())
}

/// Convierte un .cube (texto) a (size, float32 LE bytes). Si es una imagen
/// (hald tif/dpx/png), usa ffmpeg para pasarla a PNG 8-bit y remuestrea.
#[tauri::command]
fn convert_lut(path: String) -> Result<serde_json::Value, String> {
    let p = std::path::Path::new(&path);
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let cube_text = if ext == "cube" || ext.is_empty() {
        std::fs::read_to_string(p).map_err(|e| e.to_string())?
    } else {
        let tmp = std::env::temp_dir().join("filmlook_hald.png");
        let st = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(p)
            .arg(&tmp)
            .status()
            .map_err(|e| e.to_string())?;
        if !st.success() {
            return Err("ffmpeg no pudo leer la imagen".into());
        }
        return hald_to_bin(&tmp);
    };
    cube_to_bin(&cube_text)
}

fn cube_to_bin(text: &str) -> Result<serde_json::Value, String> {
    let mut size = 0usize;
    let mut vals: Vec<f32> = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        if let Some(rest) = l.strip_prefix("LUT_3D_SIZE") {
            size = rest.trim().parse().unwrap_or(0);
            continue;
        }
        if l.chars().next().unwrap().is_alphabetic() {
            continue;
        }
        for tok in l.split_whitespace() {
            vals.push(tok.parse().map_err(|_| "valor inválido en .cube")?);
        }
    }
    if size == 0 || vals.len() != size * size * size * 3 {
        return Err(format!(".cube inválido (size {}, {} valores)", size, vals.len()));
    }
    let bytes: Vec<u8> = vals.iter().flat_map(|v| v.to_le_bytes()).collect();
    Ok(json!({ "size": size, "bytes": bytes }))
}

fn hald_to_bin(png: &std::path::Path) -> Result<serde_json::Value, String> {
    // PNG → RGB vía ffmpeg rawvideo (sin dependencias de imagen)
    let out = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(png)
        .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err("ffmpeg rawvideo falló".into());
    }
    let px = out.stdout;
    // deducir nivel: side = N³ píxeles; probar niveles 4..=16
    let npix = px.len() / 3;
    let mut level = 0usize;
    for l in 4..=16usize {
        if l * l * l * l * l * l == npix {
            level = l;
            break;
        }
    }
    if level == 0 {
        return Err(format!("la imagen no es un hald válido ({} px)", npix));
    }
    let n = level * level;
    let side = n * n * n;
    let mut cube = vec![0f32; n * n * n * 3];
    for idx in 0..side {
        let (b, g, r) = (idx % n, (idx / n) % n, idx / (n * n));
        let dst = (r + n * g + n * n * b) * 3;
        for c in 0..3 {
            cube[dst + c] = px[idx * 3 + c] as f32 / 255.0;
        }
    }
    let bytes: Vec<u8> = cube.iter().flat_map(|v| v.to_le_bytes()).collect();
    Ok(json!({ "size": n, "bytes": bytes }))
}

#[tauri::command]
fn render_start(
    
    out: String,
    fps: f64,
    width: u32,
    height: u32,
    audio_src: String,
    codec: String,
) -> Result<(), String> {
    let (cv, profile, pix) = if codec == "hevc_videotoolbox" {
        ("hevc_videotoolbox", vec![], "yuv420p10le")
    } else {
        ("prores_ks", vec!["-profile:v", "4"], "yuv444p10le")
    };
    let mut args: Vec<String> = vec![
        "-hide_banner", "-loglevel", "error", "-y",
        "-f", "rawvideo", "-pix_fmt", "rgba",
        "-s", &format!("{}x{}", width, height),
        "-framerate", &fps.to_string(), "-i", "-",
        "-i", &audio_src,
        "-map", "0:v", "-map", "1:a?",
        "-vf", "vflip",
        "-c:v", cv,
    ].into_iter().map(String::from).collect();
    args.extend(profile.into_iter().map(String::from));
    args.extend(["-c:a", "aac", "-b:a", "256k", "-pix_fmt", pix, &out].into_iter().map(String::from));
    let child = Command::new("ffmpeg")
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    *RENDER_STATE.0.lock().unwrap() = Some(child);
    Ok(())
}

#[tauri::command]
fn render_frame( request: tauri::ipc::Request<'_>) -> Result<(), String> {
    let bytes = match request.body() {
        tauri::ipc::InvokeBody::Raw(b) => b,
        _ => return Err("se esperaba cuerpo binario".into()),
    };
    let mut g = RENDER_STATE.0.lock().unwrap();
    let child = g.as_mut().ok_or("render no iniciado")?;
    let stdin = child.stdin.as_mut().ok_or("sin stdin")?;
    stdin.write_all(bytes).map_err(|e| e.to_string())
}

#[tauri::command]
fn render_done(state: tauri::State<RenderState>) -> Result<(), String> {
    let mut g = RENDER_STATE.0.lock().unwrap();
    if let Some(mut child) = g.take() {
        if let Some(stdin) = child.stdin.take() {
            drop(stdin);
        }
        let _ = child.wait();
    }
    Ok(())
}

#[tauri::command]
fn probe_fps(path: String) -> Result<f64, String> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0",
               "-show_entries", "stream=r_frame_rate", "-of", "csv=p=0"])
        .arg(&path)
        .output()
        .map_err(|e| e.to_string())?;
    let s = String::from_utf8_lossy(&out.stdout);
    let mut it = s.trim().split('/');
    let num: f64 = it.next().and_then(|v| v.parse().ok()).unwrap_or(24.0);
    let den: f64 = it.next().and_then(|v| v.parse().ok()).unwrap_or(1.0);
    Ok(num / den.max(1.0))
}

static RENDER_STATE: RenderState = RenderState(Mutex::new(None));

fn main() {
    start_frame_server(&RENDER_STATE);
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            read_bytes, write_text, convert_lut, probe_fps,
            render_start, render_frame, render_done
        ])
        .setup(|app| {
            #[cfg(debug_assertions)]
            app.get_webview_window("main").unwrap().open_devtools();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error al lanzar la app");
}
