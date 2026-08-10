//! Puerto en Rust de studio/server.py: mismo contrato HTTP, con la UI, el
//! motor WebGL y la lutoteca embebidos en el binario (rust-embed).

use percent_encoding::percent_decode_str;
use rust_embed::RustEmbed;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tiny_http::{Header, Method, Response, Server};

/// Command sin ventana de consola en Windows (CREATE_NO_WINDOW): una app de
/// subsistema "windows" que lanza hijos de consola abriría un terminal POR
/// CADA ffmpeg/ffprobe — la lluvia de ventanas negras.
fn quiet_cmd<S: AsRef<std::ffi::OsStr>>(program: S) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c
}


#[derive(RustEmbed)]
#[folder = "../studio"]
#[exclude = "server.py"]
#[exclude = "tools/*"]
#[exclude = "*.DS_Store"]
struct Studio;

#[derive(RustEmbed)]
#[folder = "../app/ui"]
#[exclude = "*.DS_Store"]
struct Engine;

struct Dirs {
    media: PathBuf,
    out: PathBuf,
    thumbs: PathBuf,
    tmp: PathBuf,
    luts: PathBuf,
    project: PathBuf,
}

fn dirs() -> Dirs {
    let base = std::env::var("FL_MEDIA")
        .map(|m| PathBuf::from(m).parent().unwrap_or(Path::new(".")).to_path_buf())
        .unwrap_or_else(|_| dirs_home().join("filmlab"));
    let media = std::env::var("FL_MEDIA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| base.join("media"));
    let d = Dirs {
        media,
        out: base.join("out"),
        thumbs: base.join(".thumbs"),
        tmp: base.join(".tmp"),
        luts: base.join("luts"),
        project: base.join("project.json"),
    };
    for p in [&d.media, &d.out, &d.thumbs, &d.tmp, &d.luts] {
        let _ = std::fs::create_dir_all(p);
    }
    d
}

// ── índice de medios: como el media pool de DaVinci, por REFERENCIA ────────
// media.json: { "nombre.mp4": "/ruta/absoluta.mp4" }. Lo que esté físicamente
// en la carpeta media también cuenta, sin registrarlo.

fn index_path(d: &Dirs) -> PathBuf {
    d.media.parent().unwrap_or(Path::new(".")).join("media.json")
}

/// EL REGISTRO DE MATERIAL, los DOS: el del taller y el de la bobina abierta.
///
/// El material se importa por referencia y se apunta en
/// `registros/<bobina>.json`. Aquí solo se leía `media.json`, así que un clip
/// cuyo fichero solo constaba en el registro de su bobina **no se resolvía y
/// el revelado fallaba** — con el agravante de que dentro del programa se veía
/// y se editaba perfectamente. El de la bobina manda sobre el global.
fn load_index(d: &Dirs) -> serde_json::Map<String, serde_json::Value> {
    let lee = |p: PathBuf| -> serde_json::Map<String, serde_json::Value> {
        std::fs::read(p).ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default()
    };
    let base = d.media.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut idx = lee(index_path(d));
    let actual = std::fs::read_to_string(base.join("current.txt")).unwrap_or_default();
    let actual = actual.trim();
    if !actual.is_empty() {
        for (k, v) in lee(base.join("registros").join(format!("{actual}.json"))) {
            idx.insert(k, v);
        }
    }
    idx
}

fn save_index(d: &Dirs, idx: &serde_json::Map<String, serde_json::Value>) {
    let _ = std::fs::write(index_path(d), serde_json::Value::Object(idx.clone()).to_string());
}

/// resuelve un nombre de la estantería a su fichero real
fn resolve_media(d: &Dirs, name: &str) -> PathBuf {
    // UNA RUTA ENTERA QUE EXISTE ES LA RUTA: antes se le quitaba la carpeta
    // siempre y se buscaba en media/, así que un fichero del taller que no
    // viviera ahí —los PNG del pie, en .subs/— no se encontraba nunca.
    let tal_cual = Path::new(name);
    if tal_cual.is_absolute() && tal_cual.is_file() {
        return tal_cual.to_path_buf();
    }
    let name = tal_cual.file_name().unwrap_or_default();
    let local = d.media.join(name);
    if local.is_file() {
        return local;
    }
    let idx = load_index(d);
    if let Some(p) = idx.get(&name.to_string_lossy().to_string()).and_then(|v| v.as_str()) {
        return PathBuf::from(p);
    }
    local
}

const VIDEO_EXT: [&str; 5] = ["mp4", "mov", "m4v", "mkv", "webm"];
const AUDIO_EXT: [&str; 6] = ["wav", "mp3", "m4a", "aac", "flac", "ogg"];
const IMAGE_EXT: [&str; 7] = ["jpg", "jpeg", "png", "heic", "webp", "tif", "bmp"];

fn ext_of(name: &str) -> String {
    name.rsplit('.').next().unwrap_or("").to_ascii_lowercase()
}
fn is_video(name: &str) -> bool {
    VIDEO_EXT.contains(&ext_of(name).as_str())
}
fn is_audio(name: &str) -> bool {
    AUDIO_EXT.contains(&ext_of(name).as_str())
}
fn is_image(name: &str) -> bool {
    IMAGE_EXT.contains(&ext_of(name).as_str())
}
fn is_media(name: &str) -> bool {
    is_video(name) || is_audio(name) || is_image(name)
}

/// escapa una ruta para usarla dentro de un filtro de ffmpeg (lut3d=…)
fn ff_escape(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/").replace(':', "\\:").replace('\'', "\\'")
}

/// color_transfer y color_range del stream (para HDR y full-range)
fn probe_color(path: &Path) -> (String, String) {
    let out = quiet_cmd(ffbin("ffprobe"))
        .args(["-v", "error", "-select_streams", "v:0",
               "-show_entries", "stream=color_transfer,color_range",
               "-of", "csv=p=0", path.to_str().unwrap_or("")])
        .output();
    let s = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let mut it = s.split(',');
    (it.next().unwrap_or("").to_string(), it.next().unwrap_or("").to_string())
}

/// el tier de render: "nativo" (zero-copy) o "compat" (ffmpeg + lut3d, sin
/// grano — el look horneado en las dos LUTs; corre en una patata)
fn render_tier() -> &'static str {
    if std::env::var("FL_TIER").ok().as_deref() == Some("compat") {
        return "compat";
    }
    if renderer().exists() { "nativo" } else { "compat" }
}

/// registra rutas soltadas sobre la ventana — POR REFERENCIA, sin copiar nada
pub fn soltar(paths: &[String]) -> Vec<String> {
    let d = dirs();
    register_paths(&d, paths)
}

/// registra rutas absolutas en el índice (sin copiar nada); devuelve los nombres
fn register_paths(d: &Dirs, paths: &[String]) -> Vec<String> {
    let mut idx = load_index(d);
    let mut added = Vec::new();
    for p in paths {
        let pb = PathBuf::from(p);
        let Some(base) = pb.file_name().map(|n| n.to_string_lossy().to_string()) else { continue };
        if !pb.is_file() || !is_media(&base) {
            continue;
        }
        // nombre único en la estantería
        let mut name = base.clone();
        let mut k = 2;
        while (d.media.join(&name).exists() || idx.contains_key(&name))
            && idx.get(&name).and_then(|v| v.as_str()) != Some(p.as_str())
        {
            let (stem, ext) = base.rsplit_once('.').unwrap_or((&base, ""));
            name = format!("{stem} ({k}).{ext}");
            k += 1;
        }
        idx.insert(name.clone(), serde_json::json!(pb.to_string_lossy()));
        added.push(name);
    }
    save_index(d, &idx);
    if !added.is_empty() {
        MEDIA_VERSION.fetch_add(1, Ordering::SeqCst);
    }
    // los sidecars se cuecen YA, en paralelo — no cuando alguien los pida
    for name in added.clone() {
        std::thread::spawn(move || {
            let d2 = dirs();
            let _ = ensure_proxy(&d2, &name);
            let _ = ensure_audio_m4a(&d2, &name);
        });
    }
    added
}

// ── proxies de scrubbing: 640p ALL-INTRA (cada frame es keyframe) ──────────
// El scrub decodifica UN frame por seek: extremadamente rápido, como los
// proxies de DaVinci. Se generan en segundo plano al pedirlos.

static PROXY_INFLIGHT: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn proxies_dir(d: &Dirs) -> PathBuf {
    let p = d.media.parent().unwrap_or(Path::new(".")).join(".proxies");
    let _ = std::fs::create_dir_all(&p);
    p
}

fn ensure_proxy(d: &Dirs, name: &str) -> bool {
    if !is_video(name) {
        return false;   // el audio no necesita proxy de scrubbing
    }
    let dst = proxies_dir(d).join(name);
    if dst.is_file() {
        return true;
    }
    let src = resolve_media(d, name);
    if !src.is_file() {
        return false;
    }
    {
        let mut g = PROXY_INFLIGHT.lock().unwrap();
        if g.contains(&name.to_string()) {
            return false;   // ya se está cocinando
        }
        g.push(name.to_string());
    }
    let name = name.to_string();
    let dstd = dst.clone();
    let tmp = dst.with_extension("tmp.mp4");
    std::thread::spawn(move || {
        let hw = if cfg!(windows) { "h264_amf" } else { "h264_videotoolbox" };
        let mut ok = quiet_cmd(ffbin("ffmpeg"))
            .args(["-hide_banner", "-loglevel", "error", "-y",
                   "-i", src.to_str().unwrap(),
                   "-vf", "scale=640:-2",
                   "-c:v", hw, "-b:v", "8M", "-g", "1", "-c:a", "aac", "-b:a", "96k",
                   "-movflags", "+faststart",
                   tmp.to_str().unwrap()])
            .status().map(|st| st.success()).unwrap_or(false);
        if !ok {
            // sin encoder hw: x264 intra ultrarrápido
            ok = quiet_cmd(ffbin("ffmpeg"))
                .args(["-hide_banner", "-loglevel", "error", "-y",
                       "-i", src.to_str().unwrap(),
                       "-vf", "scale=640:-2",
                       "-c:v", "libx264", "-preset", "ultrafast", "-crf", "23",
                       "-g", "1", "-c:a", "aac", "-b:a", "96k",
                       "-movflags", "+faststart",
                       tmp.to_str().unwrap()])
                .status().map(|st| st.success()).unwrap_or(false);
        }
        if ok {
            let _ = std::fs::rename(&tmp, &dstd);   // aparición atómica
        } else {
            let _ = std::fs::remove_file(&tmp);
        }
        PROXY_INFLIGHT.lock().unwrap().retain(|n| n != &name);
    });
    false
}

/// sidecar de audio para la reproducción en la preview: AAC .m4a por cinta
/// (los <audio> del webview lo tragan siempre, venga de mkv, wav o flac)
fn ensure_audio_m4a(d: &Dirs, name: &str) -> Option<PathBuf> {
    let dst = d.thumbs.join(format!("{name}.m4a"));
    if dst.is_file() {
        return Some(dst);
    }
    let src = resolve_media(d, name);
    if !src.is_file() {
        return None;
    }
    let tmp = d.thumbs.join(format!("{name}.tmp.m4a"));
    let ok = quiet_cmd(ffbin("ffmpeg"))
        .args(["-hide_banner", "-loglevel", "error", "-y",
               "-i", src.to_str().unwrap(),
               "-vn", "-map", "0:a:0", "-c:a", "aac", "-b:a", "192k",
               "-movflags", "+faststart",
               tmp.to_str().unwrap()])
        .status().map(|st| st.success()).unwrap_or(false);
    if ok {
        let _ = std::fs::rename(&tmp, &dst);
        Some(dst)
    } else {
        let _ = std::fs::remove_file(&tmp);
        None
    }
}

/// diálogo NATIVO de importación (osascript en Mac, WinForms en Windows):
/// cero dependencias, corre bien desde el hilo del servidor
fn import_dialog() -> Vec<String> {
    if cfg!(target_os = "macos") {
        let script = r#"tell application "System Events" to set frontmost of every process whose unix id is (do shell script "echo $PPID") to true
try
  activate
  set fs to choose file with prompt "Trae material al taller" of type {"public.movie", "public.audio", "public.image"} with multiple selections allowed
  set out to ""
  repeat with f in fs
    set out to out & POSIX path of f & linefeed
  end repeat
  return out
on error
  return ""
end try"#;
        let out = quiet_cmd("osascript").arg("-e").arg(script).output();
        out.map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
    } else {
        let script = r#"Add-Type -AssemblyName System.Windows.Forms
$d = New-Object System.Windows.Forms.OpenFileDialog
$d.Multiselect = $true
$d.Filter = 'Media|*.mp4;*.mov;*.m4v;*.mkv;*.webm;*.wav;*.mp3;*.m4a;*.aac;*.flac;*.ogg;*.jpg;*.jpeg;*.png;*.webp;*.bmp'
$d.Title = 'Trae material al taller'
if ($d.ShowDialog() -eq 'OK') { $d.FileNames | ForEach-Object { Write-Output $_ } }"#;
        let out = quiet_cmd("powershell")
            .args(["-NoProfile", "-STA", "-Command", script])
            .output();
        out.map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
    }
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn ffbin(name: &str) -> String {
    // la app lanzada desde Finder/Explorer NO hereda el PATH del shell:
    // hay que resolver rutas absolutas conocidas
    let candidates: Vec<String> = if cfg!(windows) {
        vec![
            format!(r"C:\ProgramData\chocolatey\bin\{name}.exe"),
            format!(r"C:\ffmpeg\bin\{name}.exe"),
        ]
    } else {
        vec![
            format!("/opt/homebrew/bin/{name}"),
            format!("/usr/local/bin/{name}"),
        ]
    };
    for c in &candidates {
        if Path::new(c).exists() {
            return c.clone();
        }
    }
    name.to_string()
}

fn renderer() -> PathBuf {
    if let Ok(r) = std::env::var("FL_RENDERER") {
        return PathBuf::from(r);
    }
    let lab = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    // junto al ejecutable (app empaquetada) o en el árbol del repo (dev)
    let candidates: Vec<PathBuf> = if cfg!(windows) {
        vec![
            exe_dir().join("winlab.exe"),
            lab.join("winlab/target/release/winlab.exe"),
        ]
    } else {
        vec![
            exe_dir().join("filmlook-metal"),
            lab.join("metal/target/release/filmlook-metal"),
        ]
    };
    candidates
        .iter()
        .find(|p| p.exists())
        .cloned()
        .unwrap_or_else(|| candidates.last().unwrap().clone())
}

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "mp4" | "m4v" => "video/mp4",
        "m4a" => "audio/mp4",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "ogg" => "audio/ogg",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "cube" => "text/plain",
        "bin" => "application/octet-stream",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn hdr(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap()
}

// ── estado del render ──────────────────────────────────────────────────────

#[derive(Clone)]
struct RenderState {
    state: String,
    step: String,
    pct: f64,
    log: String,
    out: String,
    started: u64,
}

/// la ampliadora NATIVA (wgpu a resolución completa, sin techo de WebGL):
/// un proceso hijo con su ventana propia, alimentado por stdin
static NATIVA: Mutex<Option<std::process::Child>> = Mutex::new(None);

fn preview_bin() -> PathBuf {
    if let Ok(p) = std::env::var("FL_PREVIEW") {
        return PathBuf::from(p);
    }
    let lab = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let n = if cfg!(windows) { "preview.exe" } else { "preview" };
    let cands = [exe_dir().join(n), lab.join("core/target/release").join(n)];
    cands.iter().find(|p| p.exists()).cloned()
        .unwrap_or_else(|| cands.last().unwrap().clone())
}

/// manda una orden a la ampliadora (la abre si hace falta)
fn nativa_cmd(d: &Dirs, v: &serde_json::Value) -> Result<(), String> {
    use std::io::Write as _;
    let mut g = NATIVA.lock().unwrap();
    // ¿sigue viva?
    if let Some(c) = g.as_mut() {
        if matches!(c.try_wait(), Ok(Some(_))) { *g = None; }
    }
    if g.is_none() {
        let clip = v["clip"].as_str().unwrap_or("");
        if clip.is_empty() { return Err("sin clip".into()); }
        let bin = preview_bin();
        if !bin.exists() { return Err(format!("no encuentro la ampliadora: {}", bin.display())); }
        let prefs_path = d.tmp.join("preview_prefs.json");
        let _ = std::fs::write(&prefs_path, v.get("prefs").cloned().unwrap_or(serde_json::json!({})).to_string());
        let lut_in = d.luts.join("entrada").join(
            v["lut_in"].as_str().unwrap_or("Directo · sin transformar.cube"));
        let lut_c = d.luts.join("color").join(
            v["lut"].as_str().unwrap_or("Saorín · 65 puntos.cube"));
        let mut cmd = quiet_cmd(&bin);
        cmd.args([clip, "--ipc", "--scale", "1.0",
                  "--prefs", prefs_path.to_str().unwrap_or(""),
                  "--lut-in", lut_in.to_str().unwrap_or(""),
                  "--lut", lut_c.to_str().unwrap_or("")])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        match cmd.spawn() {
            Ok(c) => *g = Some(c),
            Err(e) => return Err(format!("no arranca la ampliadora: {e}")),
        }
    }
    if let Some(c) = g.as_mut() {
        if let Some(si) = c.stdin.as_mut() {
            let _ = writeln!(si, "{}", v);
            let _ = si.flush();
        }
    }
    Ok(())
}

fn nativa_stop() {
    use std::io::Write as _;
    let mut g = NATIVA.lock().unwrap();
    if let Some(c) = g.as_mut() {
        if let Some(si) = c.stdin.as_mut() { let _ = writeln!(si, "salir"); }
        let _ = c.kill();
    }
    *g = None;
}

static RENDER: Mutex<Option<RenderState>> = Mutex::new(None);
static RUNNING: AtomicBool = AtomicBool::new(false);
/// sube cada vez que cambia la estantería: la UI lo sondea y se refresca sola
pub static MEDIA_VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static CANCEL: AtomicBool = AtomicBool::new(false);

/// mantiene el Mac despierto mientras dura el revelado (caffeinate -i)
struct Awake(Option<std::process::Child>);
impl Awake {
    fn new() -> Self {
        if cfg!(target_os = "macos") {
            Awake(quiet_cmd("caffeinate").arg("-i")
                .stdout(std::process::Stdio::null())
                .spawn().ok())
        } else {
            Awake(None)
        }
    }
}
impl Drop for Awake {
    fn drop(&mut self) {
        if let Some(c) = self.0.as_mut() { let _ = c.kill(); }
    }
}

fn render_status_json() -> String {
    let g = RENDER.lock().unwrap();
    let s = g.clone().unwrap_or(RenderState {
        state: "idle".into(),
        step: String::new(),
        pct: 0.0,
        log: String::new(),
        out: String::new(),
        started: 0,
    });
    serde_json::json!({
        "state": s.state, "step": s.step, "pct": s.pct, "log": s.log, "out": s.out,
        "started": s.started
    })
    .to_string()
}

fn set_render(f: impl FnOnce(&mut RenderState)) {
    let mut g = RENDER.lock().unwrap();
    let mut s = g.clone().unwrap_or(RenderState {
        state: "idle".into(),
        step: String::new(),
        pct: 0.0,
        log: String::new(),
        out: String::new(),
        started: 0,
    });
    f(&mut s);
    *g = Some(s);
}

/// materializa la lutoteca embebida a disco (el renderizador nativo lee ficheros)
fn extract_luts(d: &Dirs) {
    for name in Studio::iter() {
        if let Some(rel) = name.strip_prefix("luts/") {
            let dst = d.luts.join(rel);
            if !dst.exists() {
                if let Some(f) = Studio::get(&name) {
                    let _ = std::fs::create_dir_all(dst.parent().unwrap());
                    let _ = std::fs::write(&dst, f.data.as_ref());
                }
            }
        }
    }
}

fn run_logged(cmd: &mut Command, tag: &str) -> Result<(), String> {
    set_render(|s| s.log += &format!("$ {tag}\n"));
    let out = cmd.output().map_err(|e| format!("{tag}: {e}"))?;
    let tail = String::from_utf8_lossy(if out.stderr.is_empty() { &out.stdout } else { &out.stderr });
    let tail = &tail[tail.len().saturating_sub(1200)..];
    set_render(|s| s.log += &format!("{tail}\n"));
    if !out.status.success() {
        return Err(format!("{tag} falló ({:?})", out.status));
    }
    Ok(())
}

fn projects_dir(d: &Dirs) -> PathBuf {
    let p = d.media.parent().unwrap_or(Path::new(".")).join("projects");
    let _ = std::fs::create_dir_all(&p);
    p
}

fn current_marker(d: &Dirs) -> PathBuf {
    d.media.parent().unwrap_or(Path::new(".")).join("current.txt")
}

/// la bobina abierta: base/projects/<nombre>.json, o la clásica project.json
fn current_project_path(d: &Dirs) -> PathBuf {
    if let Ok(name) = std::fs::read_to_string(current_marker(d)) {
        let n = name.trim();
        if !n.is_empty() {
            return projects_dir(d).join(format!("{n}.json"));
        }
    }
    d.project.clone()
}

fn sane_name(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' })
        .collect::<String>().trim().to_string()
}

// ── caché incremental de piezas: hash(fuente+corte+look) → pieza revelada ──
// cambiar un corte en una película de 20 clips = re-revelar UN clip

fn pieces_cache(d: &Dirs) -> PathBuf {
    let p = d.media.parent().unwrap_or(Path::new(".")).join(".cache").join("pieces");
    let _ = std::fs::create_dir_all(&p);
    p
}

fn evict_cache(dir: &Path, keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<_> = rd.flatten()
        .filter_map(|e| e.metadata().ok().and_then(|m| m.modified().ok()).map(|t| (t, e.path())))
        .collect();
    if files.len() <= keep {
        return;
    }
    files.sort_by_key(|(t, _)| *t);
    for (_, p) in files.iter().take(files.len() - keep) {
        let _ = std::fs::remove_file(p);
    }
}

fn file_sig(p: &Path) -> String {
    let m = std::fs::metadata(p).ok();
    format!("{}:{}:{}", p.display(),
        m.as_ref().map(|m| m.len()).unwrap_or(0),
        m.and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs()).unwrap_or(0))
}

/// EL DIARIO DEL REVELADO: cada paso con su marca de tiempo, a stderr.
/// Cuando algo se atasca, el último renglón dice EXACTAMENTE dónde.
/// (`FL_DIARIO=0` lo calla; por defecto habla — un revelado son segundos
/// y saber dónde muere vale más que unas líneas de consola.)
static DIARIO_T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

fn diario(msg: &str) {
    if std::env::var("FL_DIARIO").map(|v| v == "0").unwrap_or(false) {
        return;
    }
    let t0 = DIARIO_T0.get_or_init(std::time::Instant::now);
    eprintln!("⟨{:7.3}s⟩ {msg}", t0.elapsed().as_secs_f64());
}

/// mide un paso: si tarda, se ve en el diario; si se cuelga, el «…» queda
/// como última línea y señala al culpable
fn paso<T>(que: &str, f: impl FnOnce() -> T) -> T {
    diario(&format!("… {que}"));
    let t = std::time::Instant::now();
    let r = f();
    diario(&format!("✓ {que} ({:.0} ms)", t.elapsed().as_secs_f64() * 1e3));
    r
}

// ── EL CAJÓN DEL MÁSTER ──────────────────────────────────────────────────
//
// Durante la obra del motor la sala se podó hasta dejar un botón: el máster
// salía al lienzo de la bobina y con el códec que mastica el chip, y punto
// (MOTOR §8bis). Aquello era lo correcto MIENTRAS se medía la velocidad —un
// menú que ofrece un camino lento es un menú que miente—, y venía con una
// promesa escrita: «cuando el motor esté terminado y medido, añadir un camino
// nuevo será trivial: es un códec de salida, no una tubería distinta».
//
// El motor está terminado y medido. Esto es esa promesa.
//
// El camino rápido NO se toca: con los valores de siempre (`alto` = el de la
// bobina, `super` = 1) no hay ni un pase de más y el revelado va exactamente
// igual de rápido. Lo que se abre es el cajón para quien quiera pagar tiempo.

/// LO QUE SE ESCRIBE: el alto pedido con la PROPORCIÓN DE LA BOBINA, que no se
/// toca nunca — el formato es la decisión creativa y se tomó al cortarla.
fn salida_del_master(m: &serde_json::Value, bw: u64, bh: u64) -> (u64, u64) {
    let alto = m["alto"].as_u64().unwrap_or(0);
    if alto == 0 || alto == bh { return (bw, bh); }
    let prop = bw as f64 / bh.max(1) as f64;
    let w = ((alto as f64 * prop / 2.0).round() * 2.0) as u64;
    (w.max(2), (alto.max(2)) & !1)
}

/// EL LIENZO DE LA CADENA, con el tope del codificador por hardware.
/// VideoToolbox y AMF llegan a 8K; por encima de eso no hay motor y el máster
/// no saldría, así que se avisa y se recorta en vez de fallar al final.
fn lienzo_de_cadena(sw: u64, sh: u64, sup: f64) -> (u64, u64) {
    let (mut w, mut h) = ((sw as f64 * sup) as u64 & !1, (sh as f64 * sup) as u64 & !1);
    const TOPE: u64 = 8192;
    if w.max(h) > TOPE {
        let k = TOPE as f64 / w.max(h) as f64;
        let (w2, h2) = (((w as f64 * k) as u64) & !1, ((h as f64 * k) as u64) & !1);
        diario(&format!("⚠ {w}×{h} pasa del tope del codificador: se revela a {w2}×{h2}"));
        w = w2; h = h2;
    }
    (w.max(2), h.max(2))
}

/// EL CAUDAL, por píxel. El número del payload se entiende «para 1080p»; a
/// 4K son cuatro veces más píxeles y a 8K, dieciséis. Con un caudal fijo, un
/// máster de 8K saldría peor que el de 1080 — que es justo lo contrario de
/// para lo que se pide un 8K.
fn caudal_del_master(m: &serde_json::Value, sw: u64, sh: u64) -> i64 {
    let base = m["bitrate"].as_i64().unwrap_or(60_000_000);
    let px = (sw * sh) as f64 / (1920.0 * 1080.0);
    // la raíz suaviza: el caudal no hace falta que crezca del todo con los
    // píxeles (un 8K con el mismo detalle se comprime mejor por píxel)
    (base as f64 * px.max(0.05).powf(0.8)).round() as i64
}

fn render_job(payload: serde_json::Value, d: &Dirs) -> Result<(), String> {
    let _ = DIARIO_T0.get_or_init(std::time::Instant::now);
    diario(&format!("REVELADO: {} clip(s), {} de audio",
                    payload["clips"].as_array().map(|a| a.len()).unwrap_or(0),
                    payload["audio"].as_array().map(|a| a.len()).unwrap_or(0)));
    diario(&format!("carpetas: media={} out={} tmp={} luts={}",
                    d.media.display(), d.out.display(), d.tmp.display(), d.luts.display()));
    let clips = payload["clips"].as_array().cloned().unwrap_or_default();
    if clips.is_empty() {
        return Err("la bobina está vacía".into());
    }
    let prefs = payload.get("prefs").cloned().unwrap_or(serde_json::json!({}));

    // ajustes de proyecto: del payload, o tomados del PRIMER clip. Todo se
    // conforma a esta resolución/fps (letterbox, nunca estirar) — sin esto una
    // timeline con resoluciones mezcladas producía xfade roto o concat inválido
    // ANTES DE NADA: ¿está todo el material? Un fichero que no se encuentra
    // tiene que decirse por su nombre, no reventar cien líneas más abajo con
    // un error del motor que no dice nada.
    {
        let faltan: Vec<String> = clips.iter()
            .filter(|c| !c["gap"].as_bool().unwrap_or(false))
            .map(|c| c["file"].as_str().unwrap_or("").to_string())
            .filter(|f| !f.is_empty() && !resolve_media(d, f).is_file())
            .collect();
        if !faltan.is_empty() {
            for f in &faltan { diario(&format!("✗ NO ENCUENTRO el material «{f}»")); }
            return Err(format!("falta material: {}. Vuelve a importarlo en la mesa \
                                y prueba otra vez.", faltan.join(", ")));
        }
    }
    let first = resolve_media(d, clips[0]["file"].as_str().unwrap_or(""));
    diario(&format!("primer clip: {} (existe: {})", first.display(), first.is_file()));
    let fp = paso("ffprobe del primer clip", || probe(&first));
    diario(&format!("   → {}x{} @ {} fps",
                    fp["w"].as_u64().unwrap_or(0), fp["h"].as_u64().unwrap_or(0),
                    fp["fps"].as_f64().unwrap_or(0.0)));
    // ── EL LIENZO DE LA BOBINA ────────────────────────────────────────
    // Es la decisión creativa: la proporción y el tamaño con los que se montó.
    let bw = payload["project"]["w"].as_u64().unwrap_or(fp["w"].as_u64().unwrap_or(1920)).max(2) & !1;
    let bh = payload["project"]["h"].as_u64().unwrap_or(fp["h"].as_u64().unwrap_or(1080)).max(2) & !1;
    let pfps = payload["project"]["fps"].as_f64().unwrap_or(fp["fps"].as_f64().unwrap_or(25.0)).max(1.0);
    // ── Y EL DEL MÁSTER, que ya no tiene por qué ser el mismo ─────────
    // (`master.alto` en el payload; el ancho sale de la proporción de la
    // bobina, que no se toca nunca). Un máster 8K de material 4K no es un
    // capricho: las plataformas le dan mucho más caudal y el grano llega
    // entero en vez de deshacerse en el recompresor.
    let (sw, sh) = salida_del_master(&payload["master"], bw, bh);
    // EL LIENZO DE LA CADENA: lo que se revela de verdad. Con
    // `master.super > 1` el look se calcula MÁS GRANDE y se reduce al final
    // (supermuestreo: bordes y grano sin escalones); con `< 1` se revela más
    // pequeño y se agranda (el grano del original, más gordo y aún más a
    // prueba de compresión). Con 1, no hay pase de escalado ninguno y el
    // camino rápido de siempre queda intacto.
    let sup = payload["master"]["super"].as_f64().unwrap_or(1.0).clamp(0.25, 4.0);
    let (pw, ph) = lienzo_de_cadena(sw, sh, sup);
    if (pw, ph) != (sw, sh) {
        diario(&format!("MÁSTER A MANO: se revela a {pw}×{ph} y sale a {sw}×{sh} ({})",
                        if sup > 1.0 { "supermuestreo" } else { "agrandado" }));
    } else if (sw, sh) != (bw, bh) {
        diario(&format!("MÁSTER A MANO: la bobina es {bw}×{bh} y el máster sale a {sw}×{sh}"));
    }
    let conform = format!(
        "scale={pw}:{ph}:force_original_aspect_ratio=decrease:force_divisible_by=2,\
         pad={pw}:{ph}:(ow-iw)/2:(oh-ih)/2:color=black,fps={pfps:.3}"
    );
    let out_name: String = payload["out_name"]
        .as_str()
        .unwrap_or("timeline")
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    diario(&format!("lienzo del proyecto: {pw}x{ph} @ {pfps:.3} fps · salida «{out_name}»"));
    let prefs_path = d.tmp.join("render_prefs.json");
    paso("escribir render_prefs.json", || {
        std::fs::create_dir_all(&d.tmp).ok();
        std::fs::write(&prefs_path, prefs.to_string())
    }).map_err(|e| format!("no pude escribir {}: {e}", prefs_path.display()))?;

    let lut_pick = |slot: &str, name: Option<&str>, fallback: &str| -> PathBuf {
        if let Some(n) = name {
            let base = Path::new(n).file_name().unwrap_or_default();
            let cand = d.luts.join(slot).join(base);
            if cand.is_file() {
                return cand;
            }
        }
        d.luts.join(slot).join(fallback)
    };
    let lut_in = lut_pick("entrada", payload["lut_in"].as_str(), "Directo · sin transformar.cube");
    let lut_grade = lut_pick("color", payload["lut"].as_str(), "Saorín · 65 puntos.cube");

    diario(&format!("gelatina de entrada: {} (existe: {})", lut_in.display(), lut_in.is_file()));
    diario(&format!("gelatina de color:   {} (existe: {})", lut_grade.display(), lut_grade.is_file()));
    let ffmpeg = paso("localizar ffmpeg", || ffbin("ffmpeg"));
    diario(&format!("   → {ffmpeg} (existe: {})", Path::new(&ffmpeg).exists()));
    let rend = paso("localizar el MOTOR de film-look", renderer);
    diario(&format!("   → {} (existe: {})", rend.display(), rend.is_file()));
    if !rend.is_file() {
        return Err(format!("falta el motor de film-look: {}", rend.display()));
    }
    let _n = clips.len().max(1) as f64;
    let mut pieces: Vec<PathBuf> = Vec::new();
    let _awake = Awake::new();   // el Mac no se duerme a mitad de revelado
    CANCEL.store(false, Ordering::SeqCst);

    // el máster: códec y bitrate elegidos en la sala de revelado
    let m_codec = payload["master"]["codec"].as_str().unwrap_or("hevc").to_string();
    let prores = m_codec.starts_with("prores");
    // EL CAUDAL SE ESCALA CON LOS PÍXELES. 60 Mb/s son de sobra en 1080 y una
    // miseria en 8K: dejarlo fijo era prometer un máster de 8K y entregar un
    // 1080 estirado. El número del payload se entiende «para 1080».
    let m_bitrate = caudal_del_master(&payload["master"], sw, sh);
    let piece_ext = if prores { "mov" } else { "mp4" };

    let m_filtro = payload["master"]["filtro"].as_str().unwrap_or("").to_string();
    let parte = Master { cadena: (pw, ph), salida: (sw, sh),
                         codec: &m_codec, bitrate: m_bitrate, filtro: &m_filtro };
    let cache = paso("preparar la caché de piezas", || pieces_cache(d));
    diario(&format!("   → {}", cache.display()));
    let luts_sig = format!("{}|{}", file_sig(&lut_in), file_sig(&lut_grade));
    let tier = paso("decidir el tier del motor", render_tier);
    let engine_sig = format!("{}|{}", file_sig(&rend), tier);
    diario(&format!("   → tier «{tier}» · códec {m_codec} · piezas .{piece_ext}"));
    // en compat no hay VideoToolbox garantizado: todo por software
    let compat = tier == "compat";

    // ── ¿MOTOR DE BOBINA? (MOTOR §5) ──────────────────────────────────
    // El camino nuevo: el motor lee la bobina entera y la revela de un tirón.
    // Desaparecen la fase de corte (que era la mitad del tiempo), la de
    // fundidos (una pasada más sobre el máster completo) y la concatenación.
    // Se cae al camino viejo cuando la bobina trae algo que el motor aún no
    // sabe fabricar por su cuenta.
    let quiere = std::env::var("FL_MOTOR").unwrap_or_default();
    let hay_imagen = clips.iter().any(|c| is_image(c["file"].as_str().unwrap_or("")));
    // Las DOS máquinas revelan ya la bobina entera de un tirón, **también con
    // fotos y rótulos**: desde que son fuentes sintéticas del motor (una
    // textura residente, PENDIENTE §4bis.10) una bobina con una sola tarjeta
    // de título ya no paga el camino viejo entero, que era tres veces más
    // lento. Se sigue cayendo a él —sin perder nada— si el motor no puede por
    // lo que sea (tamaños mezclados, por ejemplo).
    if hay_imagen { diario("la bobina trae fotos o rótulos: fuentes residentes del motor"); }
    let puede_bobina = !compat && quiere != "ffmpeg";
    if puede_bobina || quiere == "bobina" {
        diario("MOTOR DE BOBINA: sin corte, sin fase de fundidos, sin concatenar");
        match revela_bobina(&payload, d, &rend, &ffmpeg, pw, ph, pfps, &out_name,
                            &lut_in, &lut_grade, &prefs_path, &m_codec, m_bitrate, prores,
                            parte) {
            Ok(()) => return Ok(()),
            Err(e) => {
                diario(&format!("⚠ el motor de bobina no pudo: {e}"));
                diario("   → sigo por el camino de siempre (corte + look + fundidos)");
                set_render(|s| s.log += &format!("bobina: {e}; sigo por piezas\n"));
            }
        }
    }

    // ── PIEZAS EN PARALELO: los puntos de corte son fronteras naturales ──
    // corte de la pieza N+1 mientras la N pasa por el motor; en el M4 Max los
    // dos motores ProRes mastican piezas distintas A LA VEZ
    let hilos: usize = std::env::var("FL_HILOS").ok().and_then(|v| v.parse().ok())
        .unwrap_or(if cfg!(windows) { 2 } else { 3 });
    let done = std::sync::atomic::AtomicUsize::new(0);
    let n_total = clips.len();
    diario(&format!("PREPARACIÓN LISTA · {n_total} pieza(s) con {hilos} hilo(s)"));
    let do_piece = |i: usize, c: &serde_json::Value| -> Result<PathBuf, String> {
        if CANCEL.load(Ordering::SeqCst) {
            return Err("cancelado".into());
        }

        let src = resolve_media(d, c["file"].as_str().unwrap_or(""));
        diario(&format!("pieza {}: {} (existe: {}) [{:.2}–{:.2}]", i + 1,
                        src.display(), src.is_file(),
                        c["in"].as_f64().unwrap_or(0.0), c["out"].as_f64().unwrap_or(0.0)));
        // Mac: .mov con audio PCM — el corte no gasta una generación de AAC (el
        // motor codifica a AAC UNA vez al muxar su pieza). Windows conserva
        // .mp4+AAC (no está verificado que MF lea HEVC en .mov).
        let (cut_ext, cut_acodec, cut_abr): (&str, &str, &[&str]) = if cfg!(windows) {
            ("mp4", "aac", &["-b:a", "256k"])
        } else {
            ("mov", "pcm_s16le", &[])
        };
        let cut = d.tmp.join(format!("cut_{i}.{cut_ext}"));
        let piece = d.tmp.join(format!("piece_{i}.{piece_ext}"));
        set_render(|s| s.log += &format!("clip {}: corte\n", i + 1));
        // corte FRAME-EXACTO: re-encode hardware a bitrate alto (10 bits);
        // -c copy alineaba a keyframe y la duración no era la pedida
        let hw = if compat { "libx264" }
                 else if cfg!(windows) { "hevc_amf" } else { "hevc_videotoolbox" };
        // ganancia / mudo / fundidos de audio del clip, aplicados en el corte
        let cin = c["in"].as_f64().unwrap_or(0.0);
        let cout = c["out"].as_f64().unwrap_or(0.0);
        let cdur = (cout - cin).max(0.01);
        let mut af: Vec<String> = Vec::new();
        if c["mute"].as_bool().unwrap_or(false) {
            af.push("volume=0".into());
        } else if c["gain"].as_f64().unwrap_or(0.0).abs() > 0.01 {
            af.push(format!("volume={:.2}dB", c["gain"].as_f64().unwrap()));
        }
        // fundidos de apertura/cierre de la BOBINA, sobre el primer/último clip
        let head = if i == 0 { payload["project"]["fadeHead"].as_f64().unwrap_or(0.0) } else { 0.0 };
        let tail = if i + 1 == clips.len() { payload["project"]["fadeTail"].as_f64().unwrap_or(0.0) } else { 0.0 };
        let fi = c["fadeIn"].as_f64().unwrap_or(0.0).max(head);
        let fo = c["fadeOut"].as_f64().unwrap_or(0.0).max(tail);
        if fi > 0.005 {
            af.push(format!("afade=t=in:st=0:d={fi:.3}"));
        }
        if fo > 0.005 {
            af.push(format!("afade=t=out:st={:.3}:d={fo:.3}", (cdur - fo).max(0.0)));
        }
        let mut vfade = String::new();
        if head > 0.005 {
            vfade += &format!(",fade=t=in:st=0:d={head:.3}");
        }
        if tail > 0.005 {
            vfade += &format!(",fade=t=out:st={:.3}:d={tail:.3}", (cdur - tail).max(0.0));
        }
        // ¿esta pieza ya está revelada con EXACTAMENTE esta receta?
        let mut hasher = DefaultHasher::new();
        file_sig(&src).hash(&mut hasher);
        format!("{cin:.4}|{cout:.4}").hash(&mut hasher);
        conform.hash(&mut hasher);
        af.join(",").hash(&mut hasher);
        vfade.hash(&mut hasher);
        c["tf"].to_string().hash(&mut hasher);
        prefs.to_string().hash(&mut hasher);
        luts_sig.hash(&mut hasher);
        engine_sig.hash(&mut hasher);
        m_codec.hash(&mut hasher);
        m_bitrate.hash(&mut hasher);
        let key = format!("{:016x}", hasher.finish());
        let cached = cache.join(format!("{key}.{piece_ext}"));
        if cached.is_file() {
            set_render(|s| s.log += &format!("clip {}: de la caché ({key})\n", i + 1));
            // toca el mtime para que la evicción LRU no se lo lleve
            let _ = std::fs::File::options().append(true).open(&cached);
            return Ok(cached);
        }

        let is_gap = c["gap"].as_bool().unwrap_or(false);
        let img = !is_gap && is_image(c["file"].as_str().unwrap_or(""));
        let speed = c["speed"].as_f64().unwrap_or(1.0).clamp(0.1, 8.0);
        let out_dur = cdur / speed;

        // ── EL CORTE, DENTRO DEL MOTOR (MOTOR §0) ─────────────────────────
        // Windows aún no revela la bobina entera de un tirón, pero sí puede
        // cortar y conformar él solo: se salta la pasada de ffmpeg que
        // decodificaba y re-codificaba el material completo antes de tocarlo
        // —la mitad del tiempo del revelado, y la razón de que la 890M se
        // arrastrase—. Solo para el caso limpio: vídeo de verdad, a
        // velocidad normal y sin fundido de cabeza/cola en la pieza.
        let limpio = !compat && !is_gap && !img && (speed - 1.0).abs() < 0.001
                     && vfade.is_empty() && af.is_empty() && src.is_file();
        if cfg!(windows) && limpio && std::env::var("FL_MOTOR").as_deref() != Ok("ffmpeg") {
            let n_f = (out_dur * pfps).round().max(1.0) as u64;
            let sonda = probe(&src);
            // las GUARDADAS, no las visibles: la rotación del contenedor la
            // aplica el encuadre (`cuartos`), no un intercambio a ciegas
            let (sw, sh) = (sonda["wsrc"].as_u64().unwrap_or(pw) as f32,
                            sonda["hsrc"].as_u64().unwrap_or(ph) as f32);
            // el encuadre del clip, en el ÚNICO modelo que hay. `sw`/`sh` son
            // las que declara el contenedor, así que la rotación del fichero
            // (móviles) viaja en `cuartos` y el conform la tiene en cuenta.
            let cuartos = sonda["cuartos"].as_u64().unwrap_or(0) as u8;
            let enc_clip = crate::plan::Encuadre::de_json(&c["tf"], cuartos);
            let (ma, mb, paso) = crate::plan::matriz(&enc_clip, sw, sh, pw as f32, ph as f32);
            let enc = format!("{},{},{},{},{},{},{},{},{},{},{},{}",
                              ma[0], ma[1], ma[2], ma[3], mb[0], mb[1], mb[2], mb[3],
                              paso[0], paso[1], paso[2], paso[3]);
            let mut cmd = quiet_cmd(&rend);
            cmd.args(["render", src.to_str().unwrap(), "-o", piece.to_str().unwrap()])
               .args(["--prefs", prefs_path.to_str().unwrap()])
               .args(["--lut-in", lut_in.to_str().unwrap()])
               .args(["--lut", lut_grade.to_str().unwrap()])
               .args(["--desde", &format!("{cin:.4}")])
               .args(["--cuantos", &n_f.to_string()])
               .args(["--enc", &enc])
               .args(["--lienzo", &format!("{pw}x{ph}")])
               // el paso del PROYECTO, que puede no ser el de la fuente: sin
               // esto el motor leía de corrido y una bobina a 25 con material
               // a 59,94 salía estampada a 59,94 y con solo los primeros 18 s
               .args(["--fps", &format!("{pfps:.4}")]);
            let path = std::env::var("PATH").unwrap_or_default();
            cmd.env("PATH", format!(r"{path};C:\ProgramData\chocolatey\bin"));
            diario(&format!("pieza {}: corte EN EL MOTOR [{cin:.2}–{cout:.2}] · {n_f} fotogramas",
                            i + 1));
            let t_m = std::time::Instant::now();
            match run_logged(&mut cmd, "motor con corte") {
                Ok(()) => {
                    let seg = t_m.elapsed().as_secs_f64().max(0.001);
                    diario(&format!("pieza {}: HECHA en {seg:.1} s → {:.0} fps",
                                    i + 1, n_f as f64 / seg));
                    // el sonido lo pone el mux del propio motor desde la fuente
                    let real = if prores && !piece.exists() { piece.with_extension("mov") }
                               else { piece.clone() };
                    if std::fs::rename(&real, &cached).is_ok() { return Ok(cached); }
                    return Ok(real);
                }
                Err(e) => {
                    diario(&format!("⚠ el corte en el motor falló ({e}); sigo con ffmpeg"));
                }
            }
        }
        let mut cutcmd = quiet_cmd(&ffmpeg);
        cutcmd.args(["-hide_banner", "-loglevel", "error", "-y"]);
        if is_gap {
            // el hueco: negro con silencio, del tamaño y fps del proyecto
            cutcmd.args(["-f", "lavfi", "-t", &format!("{out_dur:.3}"),
                         "-i", &format!("color=black:s={pw}x{ph}:r={pfps:.3}"),
                         "-f", "lavfi", "-t", &format!("{out_dur:.3}"),
                         "-i", "anullsrc=channel_layout=stereo:sample_rate=48000"]);
        } else if img {
            // foto fija: se le da cuerda el tiempo del clip
            cutcmd.args(["-loop", "1", "-t", &format!("{out_dur:.3}"),
                         "-i", src.to_str().unwrap(),
                         "-f", "lavfi", "-t", &format!("{out_dur:.3}"),
                         "-i", "anullsrc=channel_layout=stereo:sample_rate=48000"]);
        } else {
            // DECODIFICAR POR HARDWARE: sin esto, un HEVC 4K se descomprime
            // en la CPU y el corte cae a ~13 fps (medido en la 890M). El
            // decodificador del chip lo hace mientras el codificador trabaja.
            // Si el formato no le entra, ffmpeg cae solo a software.
            if !compat {
                if cfg!(windows) {
                    cutcmd.args(["-hwaccel", "d3d11va"]);
                } else {
                    cutcmd.args(["-hwaccel", "videotoolbox"]);
                }
            }
            cutcmd.args([
                "-ss", &format!("{cin:.3}"),
                "-to", &format!("{cout:.3}"),
                "-i", src.to_str().unwrap(),
            ]);
        }
        // el encuadre por clip (escala/giro/posición/encaje) — mismo modelo
        // que la preview: lienzo del proyecto + overlay (recorta lo que sale)
        let tf = &c["tf"];
        if tf.is_object() {
            let s_ = tf["scale"].as_f64().unwrap_or(1.0).clamp(0.2, 5.0);
            let rotd = tf["rot"].as_f64().unwrap_or(0.0);
            let xo = tf["x"].as_f64().unwrap_or(0.0).clamp(-1.5, 1.5) * pw as f64;
            let yo = tf["y"].as_f64().unwrap_or(0.0).clamp(-1.5, 1.5) * ph as f64;
            let mm = if tf["fit"].as_str() == Some("fill") { "max" } else { "min" };
            let mut vch = String::from("[0:v]");
            if rotd.abs() > 0.01 {
                let r = rotd.to_radians();
                vch += &format!("rotate={r:.6}:ow=rotw({r:.6}):oh=roth({r:.6}):c=black,");
            }
            vch += &format!(
                "scale=w='trunc(iw*{mm}({pw}/iw\\,{ph}/ih)*{s_:.4}/2)*2':\
                 h='trunc(ih*{mm}({pw}/iw\\,{ph}/ih)*{s_:.4}/2)*2'[img];\
                 color=black:s={pw}x{ph}:r={pfps:.3}[bg];\
                 [bg][img]overlay=x='({pw}-w)/2+{xo:.1}':y='({ph}-h)/2+{yo:.1}':shortest=1,\
                 fps={pfps:.3}{vfade}[vout]");
            // foto/hueco: el silencio viene como entrada 1 (anullsrc) — SIEMPRE
            // hay pista de audio o los fundidos ([N:a]) no encuentran el stream
            let amap = if is_gap || img { "1:a" } else { "0:a?" };
            cutcmd.args(["-filter_complex", &vch, "-map", "[vout]", "-map", amap]);
            if is_gap || img { cutcmd.arg("-shortest"); }
        } else {
            // HDR → 709 y full-range → tv, ANTES del conform
            let mut pre = String::new();
            if !is_gap && !img && src.is_file() {
                let (trc, range) = probe_color(&src);
                if trc == "smpte2084" || trc == "arib-std-b67" {
                    pre += "zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,\
tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv,format=yuv420p10le,";
                    set_render(|s| s.log += &format!("clip {}: HDR → 709 (tonemap)\n", i + 1));
                } else if range == "pc" {
                    pre += "scale=in_range=pc:out_range=tv,";
                }
            }
            let spd = if (speed - 1.0).abs() > 0.001 {
                format!(",setpts=PTS/{speed:.4}")
            } else {
                String::new()
            };
            // SALTAR EL CONFORM CUANDO NO HACE NADA: si el clip ya viene al
            // lienzo y al paso del proyecto, scale+pad+fps son tres filtros
            // que copian píxeles para dejarlos igual. `null` no cuesta.
            let mio = if is_gap || img { conform.clone() } else {
                let s = probe(&src);
                let (cw, ch) = (s["w"].as_u64().unwrap_or(0), s["h"].as_u64().unwrap_or(0));
                let cf = s["fps"].as_f64().unwrap_or(0.0);
                if cw == pw && ch == ph && (cf - pfps).abs() < 0.01 {
                    diario(&format!("pieza {}: ya viene a {pw}x{ph}@{pfps:.3} — sin conform", i + 1));
                    "null".to_string()
                } else { conform.clone() }
            };
            let vf_own = format!("{pre}{mio}{spd}{vfade}");
            let amap = if is_gap || img { "1:a" } else { "0:a?" };
            cutcmd.args(["-map", "0:v:0", "-map", amap, "-vf", &vf_own]);
            if is_gap || img { cutcmd.arg("-shortest"); }
            if (speed - 1.0).abs() > 0.001 && !is_gap && !img {
                // atempo mantiene el tono; se encadena para salir de [0.5,2]
                let mut sp = speed;
                let mut chain: Vec<String> = Vec::new();
                while sp > 2.0 { chain.push("atempo=2.0".into()); sp /= 2.0; }
                while sp < 0.5 { chain.push("atempo=0.5".into()); sp *= 2.0; }
                chain.push(format!("atempo={sp:.4}"));
                af.insert(0, chain.join(","));
            }
        }
        if compat {
            // x264 no traga p010le ni el tag hvc1: 10-bit clásico y crf alto
            cutcmd.args([
                "-c:v", "libx264", "-preset", "fast", "-crf", "14",
                "-pix_fmt", "yuv420p10le",
                "-c:a", cut_acodec,
            ]);
        } else {
            cutcmd.args(["-c:v", hw]);
            if cfg!(windows) {
                // hevc_amf firma "Main" aunque el contenido sea 10-bit y
                // Media Foundation rechaza la inconsistencia (0xC00D36B4)
                cutcmd.args(["-profile:v", "main10"]);
            }
            cutcmd.args([
                "-b:v", "120M", "-pix_fmt", "p010le",
                "-tag:v", "hvc1",
                "-c:a", cut_acodec,
            ]);
        }
        cutcmd.args(cut_abr);
        if !af.is_empty() {
            cutcmd.args(["-af", &af.join(",")]);
        }
        cutcmd.arg(cut.to_str().unwrap());
        diario(&format!("pieza {}: corte → {} ({:.1} s de material)",
                        i + 1, cut.display(), cdur));
        let t_corte = std::time::Instant::now();
        run_logged(&mut cutcmd, "corte")?;
        let seg = t_corte.elapsed().as_secs_f64().max(0.001);
        diario(&format!("pieza {}: corte HECHO en {:.1} s → {:.0} fps",
                        i + 1, seg, cdur * pfps / seg));
        set_render(|s| s.log += &format!("clip {}: film-look\n", i + 1));
        if compat {
            // tier patata: el look horneado en las dos LUTs (lut3d), x264.
            // Sin grano ni halación — pero corre en cualquier máquina.
            let vf = format!("lut3d='{}',lut3d='{}'", ff_escape(&lut_in), ff_escape(&lut_grade));
            run_logged(
                quiet_cmd(&ffmpeg).args([
                    "-hide_banner", "-loglevel", "error", "-y",
                    "-i", cut.to_str().unwrap(),
                    "-vf", &vf,
                    "-c:v", "libx264", "-preset", "medium", "-crf", "17",
                    "-pix_fmt", "yuv420p",
                    "-c:a", "aac", "-b:a", "256k",
                    piece.to_str().unwrap(),
                ]),
                "render compat (lut3d)",
            )?;
        } else {
            let mut cmd = quiet_cmd(&rend);
            cmd.args(["render", cut.to_str().unwrap(), "-o", piece.to_str().unwrap()])
                .args(["--prefs", prefs_path.to_str().unwrap()])
                .args(["--lut-in", lut_in.to_str().unwrap()])
                .args(["--lut", lut_grade.to_str().unwrap()]);
            if !cfg!(windows) {
                cmd.args(["--codec", &m_codec]);
                if !prores {
                    cmd.args(["--bitrate", &m_bitrate.to_string()]);
                }
            }
            if cfg!(windows) {
                let path = std::env::var("PATH").unwrap_or_default();
                cmd.env("PATH", format!(r"{path};C:\ProgramData\chocolatey\bin"));
            }
            run_logged(&mut cmd, "render nativo")?;
        }
        let _ = std::fs::remove_file(&cut);   // el corte ya no hace falta
        // a la caché con su clave (el motor ProRes fuerza .mov él solo)
        let real_piece = if prores && !piece.exists() {
            piece.with_extension("mov")
        } else {
            piece.clone()
        };
        if std::fs::rename(&real_piece, &cached).is_ok() {
            Ok(cached)
        } else {
            Ok(real_piece)
        }
    };

    let resultados: Mutex<Vec<Option<Result<PathBuf, String>>>> =
        Mutex::new((0..n_total).map(|_| None).collect());
    std::thread::scope(|sc| {
        let (tok_tx, tok_rx) = std::sync::mpsc::channel::<()>();
        for _ in 0..hilos { let _ = tok_tx.send(()); }
        let tok_rx = Mutex::new(tok_rx);
        for (i, c) in clips.iter().enumerate() {
            let _ = tok_rx.lock().unwrap().recv();
            let tok_tx = tok_tx.clone();
            let do_piece = &do_piece;
            let resultados = &resultados;
            let done = &done;
            sc.spawn(move || {
                let r = do_piece(i, c);
                let d2 = done.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                set_render(|s| {
                    s.step = format!("piezas: {d2}/{n_total} reveladas");
                    s.pct = 0.05 + 0.85 * (d2 as f64 / n_total as f64);
                });
                resultados.lock().unwrap()[i] = Some(r);
                let _ = tok_tx.send(());
            });
        }
    });
    for r in resultados.into_inner().unwrap() {
        match r {
            Some(Ok(p)) => pieces.push(p),
            Some(Err(e)) => return Err(e),
            None => return Err("pieza sin resultado".into()),
        }
    }
    evict_cache(&cache, 300);

    set_render(|s| {
        s.step = "concatenando".into();
        s.pct = 0.9;
    });
    let out_ext = if prores { "mov" } else { "mp4" };
    // la carpeta de destino la manda quien revela (el autor elige dónde va el
    // máster); si no viene o no se puede crear, la del taller
    let destino = payload["out_dir"].as_str()
        .map(PathBuf::from)
        .filter(|p| p.is_dir() || std::fs::create_dir_all(p).is_ok())
        .unwrap_or_else(|| d.out.clone());
    // sin sobrescritura silenciosa: bobina.mp4 → bobina_2.mp4 → bobina_3.mp4…
    let mut out_path = destino.join(format!("{out_name}.{out_ext}"));
    let mut k = 2;
    while out_path.exists() {
        out_path = destino.join(format!("{out_name}_{k}.{out_ext}"));
        k += 1;
    }
    // el máster se monta en un temporal y aparece de golpe al final (nunca un
    // fichero a medias con el nombre bueno)
    let master = d.tmp.join(format!("master.{out_ext}"));
    // fundidos: clips[i].fade = segundos de disolvencia hacia el SIGUIENTE clip
    let fades: Vec<f64> = clips.iter().map(|c| c["fade"].as_f64().unwrap_or(0.0)).collect();
    let any_fade = pieces.len() > 1 && fades.iter().take(pieces.len() - 1).any(|f| *f > 0.01);
    if any_fade {
        // duraciones reales de las piezas (ya con el look)
        let durs: Vec<f64> = pieces.iter().map(|p| {
            probe(p)["dur"].as_f64().unwrap_or(0.0)
        }).collect();
        let mut fg = String::new();
        let mut vin = "[0:v]".to_string();
        let mut ain = "[0:a]".to_string();
        let mut acc = durs[0];
        for i in 1..pieces.len() {
            let f = fades[i - 1].clamp(0.0, durs[i - 1].min(durs[i]) / 2.0).max(0.001);
            let off = (acc - f).max(0.0);
            let vo = format!("[v{i}]");
            let ao = format!("[a{i}]");
            let trans = match clips[i - 1]["fadeType"].as_str() {
                Some("fadeblack") => "fadeblack",
                Some("fadewhite") => "fadewhite",
                _ => "fade",
            };
            fg += &format!("{vin}[{i}:v]xfade=transition={trans}:duration={f:.3}:offset={off:.3}{vo};");
            fg += &format!("{ain}[{i}:a]acrossfade=d={f:.3}{ao};");
            vin = vo;
            ain = ao;
            acc = acc - f + durs[i];
        }
        let fg = fg.trim_end_matches(';').to_string();
        let hw = if compat { "libx264" }
                 else if prores { "prores_videotoolbox" }
                 else if cfg!(windows) { "hevc_amf" } else { "hevc_videotoolbox" };
        let mut cmd = quiet_cmd(&ffmpeg);
        cmd.args(["-hide_banner", "-loglevel", "error", "-y"]);
        for p in &pieces {
            cmd.args(["-i", p.to_str().unwrap()]);
        }
        cmd.args(["-filter_complex", &fg,
                  "-map", &format!("{vin}"), "-map", &format!("{ain}")]);
        if prores {
            let profile = if m_codec == "prores4444" { "4" } else { "3" };
            cmd.args(["-c:v", hw, "-profile:v", profile,
                      "-pix_fmt", if m_codec == "prores4444" { "yuva444p10le" } else { "p210le" }]);
        } else if compat {
            cmd.args(["-c:v", hw, "-crf", "18", "-pix_fmt", "yuv420p"]);
        } else {
            cmd.args(["-c:v", hw, "-b:v", &m_bitrate.to_string(), "-pix_fmt", "p010le"]);
        }
        cmd.args(["-c:a", "aac", "-b:a", "256k",
                  "-movflags", "+faststart",
                  master.to_str().unwrap()]);
        run_logged(&mut cmd, "fundidos")?;
    } else {
        let lst = d.tmp.join("concat.txt");
        let mut txt = String::new();
        for p in &pieces {
            txt += &format!("file '{}'\n", p.to_str().unwrap().replace('\'', r"'\''"));
        }
        std::fs::write(&lst, txt).map_err(|e| e.to_string())?;
        run_logged(
            quiet_cmd(&ffmpeg).args([
                "-hide_banner", "-loglevel", "error", "-y",
                "-f", "concat", "-safe", "0",
                "-i", lst.to_str().unwrap(),
                "-c", "copy", master.to_str().unwrap(),
            ]),
            "concat",
        )?;
    }
    termina_master(&payload, d, &ffmpeg, &master, &out_name, out_ext, pfps, parte)
}

/// LA COLA COMPARTIDA de los dos caminos: mezclar la música bajo la bobina,
/// normalizar si toca y dejar el máster con su nombre definitivo. El vídeo no
/// se vuelve a codificar aquí (`-c:v copy`): solo se le pone el sonido.
/// LO QUE EL MÁSTER TIENE QUE SER. Viajaba suelto en media docena de
/// argumentos y ahora que hay cajón se le da nombre: si se añade una opción,
/// se añade aquí y la ven todos los caminos.
#[derive(Clone, Copy)]
pub struct Master<'a> {
    /// el lienzo al que se ha revelado de verdad
    pub cadena: (u64, u64),
    /// el lienzo que hay que escribir
    pub salida: (u64, u64),
    pub codec: &'a str,
    pub bitrate: i64,
    /// «nítido» (lanczos) o «suave» (area) al escalar
    pub filtro: &'a str,
}

impl<'a> Master<'a> {
    fn escala(&self) -> bool { self.cadena != self.salida }
    /// el filtro de ffmpeg, con el valor por defecto que toca en cada sentido:
    /// al AGRANDAR, lanczos (nítido); al REDUCIR, area (el promedio de caja,
    /// que es lo correcto para un supermuestreo y no deja anillos)
    fn flags(&self) -> &str {
        match self.filtro {
            "lanczos" | "area" | "bicubic" | "spline" => self.filtro,
            _ if self.salida.0 > self.cadena.0 => "lanczos",
            _ => "area",
        }
    }
}

fn termina_master(payload: &serde_json::Value, d: &Dirs, ffmpeg: &str, master: &Path,
                  out_name: &str, out_ext: &str, _pfps: f64, m: Master) -> Result<(), String> {
    let clips = payload["clips"].as_array().cloned().unwrap_or_default();
    // la carpeta de destino la manda quien revela; si no viene, la del taller
    let destino = payload["out_dir"].as_str()
        .map(PathBuf::from)
        .filter(|p| p.is_dir() || std::fs::create_dir_all(p).is_ok())
        .unwrap_or_else(|| d.out.clone());
    // sin sobrescritura silenciosa: bobina.mp4 → bobina_2.mp4 → bobina_3.mp4…
    let mut out_path = destino.join(format!("{out_name}.{out_ext}"));
    let mut k = 2;
    while out_path.exists() {
        out_path = destino.join(format!("{out_name}_{k}.{out_ext}"));
        k += 1;
    }
    let fades: Vec<f64> = clips.iter().map(|c| c["fade"].as_f64().unwrap_or(0.0)).collect();
    // ── la pista de audio (música/voz bajo la bobina): mezcla sin re-encodear vídeo ──
    let music = payload["audio"].as_array().cloned().unwrap_or_default();
    let loudnorm = payload["master"]["loudnorm"].as_bool().unwrap_or(false);
    // EL NIVEL DEL SONIDO DEL VÍDEO, el mando del margen de la mesa (§1.6):
    // lo que se oye montando es lo que sale al máster
    let vol_voz = payload["vol_voz"].as_f64().unwrap_or(0.0);
    // ── EL ESCALADO DEL MÁSTER, en la MISMA pasada que la mezcla ───────
    // Si hay que escalar y además hay sonido que mezclar, se hace todo de una
    // vez: una generación, no dos. Si no hay sonido, el escalado se paga solo.
    if !music.is_empty() || loudnorm || vol_voz.abs() > 0.01 || m.escala() {
        set_render(|s| {
            s.step = "mezclando el sonido".into();
            s.pct = 0.95;
        });
        // los fundidos acortan la salida: tiempo de UI → tiempo de máster
        let clip_durs: Vec<f64> = clips.iter()
            .map(|c| (c["out"].as_f64().unwrap_or(0.0) - c["in"].as_f64().unwrap_or(0.0)).max(0.01))
            .collect();
        let eff_fades: Vec<f64> = (0..clips.len().saturating_sub(1))
            .map(|i| {
                let f = fades[i];
                if f > 0.01 { f.clamp(0.0, clip_durs[i].min(clip_durs[i + 1]) / 2.0) } else { 0.0 }
            })
            .collect();
        let junctions: Vec<f64> = clip_durs.iter().scan(0.0, |acc, d| { *acc += d; Some(*acc) }).collect();
        let ui_to_out = |ui: f64| -> f64 {
            let eaten: f64 = junctions.iter().zip(eff_fades.iter())
                .filter(|(j, _)| **j <= ui + 0.001)
                .map(|(_, f)| *f)
                .sum();
            (ui - eaten).max(0.0)
        };
        let mdur = probe(&master)["dur"].as_f64().unwrap_or(0.0).max(0.1);
        let base_has_audio = {
            let o = quiet_cmd(ffbin("ffprobe"))
                .args(["-v", "error", "-select_streams", "a:0",
                       "-show_entries", "stream=codec_type", "-of", "csv=p=0",
                       master.to_str().unwrap()])
                .output();
            o.map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty()).unwrap_or(false)
        };
        let mut cmd = quiet_cmd(&ffmpeg);
        cmd.args(["-hide_banner", "-loglevel", "error", "-y",
                  "-i", master.to_str().unwrap()]);
        for a in &music {
            let src = resolve_media(d, a["file"].as_str().unwrap_or(""));
            cmd.args(["-i", src.to_str().unwrap_or("")]);
        }
        if !base_has_audio {
            cmd.args(["-f", "lavfi", "-t", &format!("{mdur:.3}"),
                      "-i", "anullsrc=channel_layout=stereo:sample_rate=48000"]);
        }
        let base_label = if base_has_audio { "[0:a]".to_string() }
                         else { format!("[{}:a]", music.len() + 1) };
        let mut fg = String::new();
        // el mando de la voz, antes de la mezcla
        let base_label = if vol_voz.abs() > 0.01 {
            fg += &format!("{base_label}volume={vol_voz:.2}dB[voz];");
            "[voz]".to_string()
        } else { base_label };
        let mut mix_ins = format!("{base_label}");
        for (k, a) in music.iter().enumerate() {
            let ain = a["in"].as_f64().unwrap_or(0.0);
            let aout = a["out"].as_f64().unwrap_or(0.0).max(ain + 0.01);
            let adur = aout - ain;
            let start_ms = (ui_to_out(a["start"].as_f64().unwrap_or(0.0)) * 1000.0).round() as i64;
            let mut chain = format!("[{}:a]atrim=start={ain:.3}:end={aout:.3},asetpts=PTS-STARTPTS,\
aresample=48000,aformat=channel_layouts=stereo", k + 1);
            if a["mute"].as_bool().unwrap_or(false) {
                chain += ",volume=0";
            } else if let Some(env) = a["env"].as_array().filter(|e| e.len() >= 2) {
                // banda elástica: interpolación lineal en dB entre puntos {t, db}
                let pts: Vec<(f64, f64)> = env.iter()
                    .filter_map(|p2| Some((p2["t"].as_f64()?, p2["db"].as_f64()?)))
                    .collect();
                if pts.len() >= 2 {
                    let mut expr = format!("{:.4}", 10f64.powf(pts.last().unwrap().1 / 20.0));
                    for w in pts.windows(2).rev() {
                        let (t0, d0) = w[0];
                        let (t1, d1) = w[1];
                        let (v0, v1) = (10f64.powf(d0 / 20.0), 10f64.powf(d1 / 20.0));
                        expr = format!(
                            "if(lt(t\\,{t1:.3})\\,{v0:.4}+({v1:.4}-{v0:.4})*(t-{t0:.3})/({:.3})\\,{expr})",
                            (t1 - t0).max(0.001));
                    }
                    let first = format!("if(lt(t\\,{:.3})\\,{:.4}\\,{expr})",
                        pts[0].0, 10f64.powf(pts[0].1 / 20.0));
                    chain += &format!(",volume=volume='{first}':eval=frame");
                }
            } else if a["gain"].as_f64().unwrap_or(0.0).abs() > 0.01 {
                chain += &format!(",volume={:.2}dB", a["gain"].as_f64().unwrap());
            }
            let fi = a["fadeIn"].as_f64().unwrap_or(0.0);
            let fo = a["fadeOut"].as_f64().unwrap_or(0.0);
            if fi > 0.005 { chain += &format!(",afade=t=in:st=0:d={fi:.3}"); }
            if fo > 0.005 { chain += &format!(",afade=t=out:st={:.3}:d={fo:.3}", (adur - fo).max(0.0)); }
            if start_ms > 0 { chain += &format!(",adelay={start_ms}|{start_ms}"); }
            fg += &format!("{chain}[m{k}];");
            mix_ins += &format!("[m{k}]");
        }
        if music.is_empty() {
            fg += &format!("{base_label}anull[aout]");
        } else {
            fg += &format!("{mix_ins}amix=inputs={}:duration=first:dropout_transition=0:normalize=0[aout]",
                           music.len() + 1);
        }
        if loudnorm {
            // sonoridad de plataforma: −16 LUFS, techo −1.5 dBTP
            fg = fg.replace("[aout]", "[premix]");
            fg += ";[premix]loudnorm=I=-16:TP=-1.5:LRA=11[aout]";
        }
        cmd.args(["-filter_complex", &fg, "-map", "0:v"]);
        if m.escala() {
            // el vídeo se re-codifica porque hay que cambiarle el tamaño: es
            // la única generación de más que cuesta el cajón, y solo la paga
            // quien lo abre
            let (w, h) = m.salida;
            cmd.args(["-vf", &format!("scale={w}:{h}:flags={}", m.flags())]);
            pon_codec(&mut cmd, m.codec, m.bitrate);
        } else {
            cmd.args(["-c:v", "copy"]);
        }
        cmd.args(["-map", "[aout]", "-c:a", "aac", "-b:a", "256k",
                  "-movflags", "+faststart",
                  out_path.to_str().unwrap()]);
        run_logged(&mut cmd, if m.escala() { "escalar y mezclar" } else { "mezcla" })?;
        let _ = std::fs::remove_file(&master);
    } else {
        let _ = std::fs::remove_file(&out_path);
        std::fs::rename(&master, &out_path).map_err(|e| format!("mover el máster: {e}"))?;
    }

    // los temporales no se quedan a vivir (las piezas viven en la caché)
    let _ = std::fs::remove_file(d.tmp.join("concat.txt"));
    // LA RUTA ENTERA, no sólo el nombre. Antes se guardaba «/out/loquesea.mp4»
    // y quien lo leía lo pegaba a la carpeta del taller: si el máster había
    // ido a otra carpeta —lo normal— el parte decía una ruta que no existía.
    // Cuesta muy caro: dos veces he medido un fichero viejo creyendo que era
    // el recién revelado, y las dos he sacado la conclusión contraria.
    let salida = out_path.to_string_lossy().to_string();
    set_render(move |s| {
        s.state = "done".into();
        s.step = "terminado".into();
        s.pct = 1.0;
        s.out = salida.clone();
    });
    Ok(())
}


/// EL CAMINO NUEVO: el motor revela la bobina entera y aquí solo se hornea el
/// sonido. Una pasada de vídeo (antes: corte + look + fundidos + concat, o sea
/// tres compresiones del material) y una de audio.
#[allow(clippy::too_many_arguments)]
fn revela_bobina(payload: &serde_json::Value, d: &Dirs, rend: &Path, ffmpeg: &str,
                 pw: u64, ph: u64, pfps: f64, out_name: &str,
                 lut_in: &Path, lut_grade: &Path, prefs_path: &Path,
                 m_codec: &str, m_bitrate: i64, prores: bool,
                 parte: Master) -> Result<(), String> {
    let clips = payload["clips"].as_array().cloned().unwrap_or_default();
    let out_ext = if prores { "mov" } else { "mp4" };
    let mudo = d.tmp.join(format!("bobina_mudo.{out_ext}"));
    let _ = std::fs::remove_file(&mudo);

    // ── ATAJO: IDENTIDAD → REMUX (MOTOR §7) ───────────────────────────
    // Si no hay look que aplicar, ni encuadre, ni fundidos, ni conversión de
    // cadencia, y el material ya viene en el códec y el lienzo de salida,
    // entonces «revelar» es COPIAR: se recortan los flujos sin tocar un solo
    // píxel. De minutos a segundos. Es raro en esta casa —el look es la
    // firma— pero cuesta veinte líneas y cuando toca, vuela.
    if let Some(unico) = clips.first().filter(|_| clips.len() == 1) {
        let sin_look = !prefs_hace_algo(&payload["prefs"])
            && !prefs_hace_algo(&unico["prefs"]);
        let sin_gelatina = payload["lut_in"].as_str().is_none()
            && payload["lut"].as_str().is_none();
        // EL ENCUADRE, con el modelo de verdad. Esto leía `tf.scale`/`x`/`y`,
        // que son los nombres VIEJOS: con el modelo de hoy (`escala`, `pos`,
        // `cuartos`) daba «sin encuadre» siempre, y el atajo se habría llevado
        // por delante cualquier reencuadre sin decir nada (PENDIENTE §6).
        let cuartos = unico["cuartos"].as_u64().unwrap_or(0) as u8;
        let sin_encuadre = crate::plan::Encuadre::de_json(&unico["tf"], cuartos)
            .es_limpio(cuartos);
        let sin_fundidos = unico["fadeIn"].as_f64().unwrap_or(0.0) < 0.005
            && unico["fadeOut"].as_f64().unwrap_or(0.0) < 0.005
            && payload["project"]["fadeHead"].as_f64().unwrap_or(0.0) < 0.005
            && payload["project"]["fadeTail"].as_f64().unwrap_or(0.0) < 0.005;
        let normal = (unico["speed"].as_f64().unwrap_or(1.0) - 1.0).abs() < 1e-6
            && !unico["gap"].as_bool().unwrap_or(false)
            && !unico["mute"].as_bool().unwrap_or(false)
            // con CAPAS encima, matriz explícita o anidadas hay que revelar
            // de verdad: el remux copiaría el material pelado (CAPAS §9)
            && payload["clips2"].as_array().map(|a| a.is_empty()).unwrap_or(true)
            && unico["mat"].is_null()
            && unico["anidada"].is_null();
        let src = resolve_media(d, unico["file"].as_str().unwrap_or(""));
        if sin_look && sin_gelatina && sin_encuadre && sin_fundidos && normal && src.is_file() {
            let sp = probe(&src);
            let mismo = sp["w"].as_u64() == Some(pw) && sp["h"].as_u64() == Some(ph)
                && (sp["fps"].as_f64().unwrap_or(0.0) - pfps).abs() < 0.01
                && sp["codec"].as_str().map(|c| c == "hevc").unwrap_or(false)
                && !prores;
            if mismo {
                diario("ATAJO: nada que revelar y el material ya está en formato — REMUX");
                let t_in = unico["in"].as_f64().unwrap_or(0.0);
                let t_out = unico["out"].as_f64().unwrap_or(0.0).max(t_in);
                let master = d.tmp.join(format!("master.{out_ext}"));
                let _ = std::fs::remove_file(&master);
                let r = run_logged(quiet_cmd(ffmpeg).args([
                    "-hide_banner", "-loglevel", "error", "-y",
                    "-ss", &format!("{t_in:.4}"), "-to", &format!("{t_out:.4}"),
                    "-i", src.to_str().unwrap(),
                    "-c", "copy", "-movflags", "+faststart",
                    master.to_str().unwrap()]), "remux");
                if r.is_ok() && master.is_file() {
                    return termina_master(payload, d, ffmpeg, &master, out_name, out_ext,
                                          pfps, parte);
                }
                diario("   el remux no salió; sigo revelando de verdad");
            }
        }
    }

    // el plan: la misma bobina, con las rutas ya resueltas
    let mut plan = payload.clone();
    plan["project"] = serde_json::json!({
        "w": pw, "h": ph, "fps": pfps,
        "fadeHead": payload["project"]["fadeHead"].as_f64().unwrap_or(0.0),
        "fadeTail": payload["project"]["fadeTail"].as_f64().unwrap_or(0.0),
    });
    plan["out"] = serde_json::json!(mudo.to_string_lossy());
    plan["master"] = serde_json::json!({ "codec": m_codec, "bitrate": m_bitrate });
    plan["lut_in"] = serde_json::json!(lut_in.to_string_lossy());
    plan["lut"] = serde_json::json!(lut_grade.to_string_lossy());
    if let Some(a) = plan["clips"].as_array_mut() {
        for c in a.iter_mut() {
            let f = c["file"].as_str().unwrap_or("").to_string();
            let ruta = resolve_media(d, &f);
            // LA ORIENTACIÓN DEL FICHERO viaja con el clip. Los móviles graban
            // apaisado y declaran «esto va girado 90°» en el contenedor; el
            // corte de ffmpeg lo aplicaba con `autorotate` y ese corte ya no
            // existe, así que sin esto el máster sale tumbado (§1.5).
            // LA CADENCIA DE LA FUENTE viaja con el clip por el mismo motivo:
            // el plan la necesita para saber si el máster cae entre dos
            // fotogramas de origen y hay que interpolarlos (el tirón).
            if !c["gap"].as_bool().unwrap_or(false)
                && (c["cuartos"].is_null() || c["fps_src"].is_null()) {
                let p = probe(&ruta);
                if c["cuartos"].is_null() {
                    let q = p["cuartos"].as_u64().unwrap_or(0);
                    if q != 0 { c["cuartos"] = serde_json::json!(q); }
                }
                if c["fps_src"].is_null() {
                    let f = p["fps"].as_f64().unwrap_or(0.0);
                    if f > 0.0 { c["fps_src"] = serde_json::json!(f); }
                }
            }
            c["file"] = serde_json::json!(ruta.to_string_lossy());
        }
    }
    // ── LAS BOBINAS ANIDADAS, APLANADAS (CAPAS §8) ────────────────────
    // La app manda los payloads ya aplanados; esto cubre el CLI y el bot,
    // donde «anidada» puede ser la RUTA a un payload hijo en JSON.
    if plan["clips"].as_array().map(|a| a.iter().any(|c| !c["anidada"].is_null()))
        .unwrap_or(false) {
        let n = crate::plan::aplana_anidadas(&mut plan,
            &|clave| std::fs::read_to_string(clave).ok()
                .and_then(|s| serde_json::from_str(&s).ok()),
            &|f| {
                let p = probe(std::path::Path::new(f));
                let (w, h) = (p["w"].as_f64()?, p["h"].as_f64()?);
                if w > 0.0 && h > 0.0 { Some((w as f32, h as f32)) } else { None }
            }).map_err(|e| format!("anidadas: {e}"))?;
        if n > 0 { diario(&format!("   anidadas: {n} bobina(s) aplanada(s)")); }
    }

    // ── LAS CAPAS: mismas sondas que los clips ────────────────────────
    if let Some(a) = plan["clips2"].as_array_mut() {
        for c in a.iter_mut() {
            let f = c["file"].as_str().unwrap_or("").to_string();
            let ruta = resolve_media(d, &f);
            if !c["gap"].as_bool().unwrap_or(false)
                && (c["cuartos"].is_null() || c["fps_src"].is_null()) {
                let p = probe(&ruta);
                if c["cuartos"].is_null() {
                    let q = p["cuartos"].as_u64().unwrap_or(0);
                    if q != 0 { c["cuartos"] = serde_json::json!(q); }
                }
                if c["fps_src"].is_null() {
                    let f2 = p["fps"].as_f64().unwrap_or(0.0);
                    if f2 > 0.0 { c["fps_src"] = serde_json::json!(f2); }
                }
            }
            c["file"] = serde_json::json!(ruta.to_string_lossy());
        }
        if !a.is_empty() { diario(&format!("   capas: {} encima de la bobina", a.len())); }
    }

    let ruta_plan = d.tmp.join("plan_bobina.json");
    std::fs::write(&ruta_plan, plan.to_string()).map_err(|e| format!("escribir el plan: {e}"))?;

    // ── UNA CARPETA DE CLIPS SUELTOS ──────────────────────────────────
    // El taller como LABORATORIO: revelar cada plano en su propio fichero,
    // ya con el look y el encuadre, para montarlos en Resolve o donde sea.
    // Mismo motor y mismo plan; lo único que cambia es que no se pega nada al
    // final y que cada tramo sale por la puerta con su nombre.
    //
    // Va AQUÍ y no antes porque necesita el plan con las rutas ya resueltas:
    // con el payload crudo el motor recibía «hd10.mp4» y no encontraba nada.
    //
    // La bandera se lee del PAYLOAD y no del plan: unas líneas más arriba
    // `plan["master"]` se reescribe entero con el códec y el caudal, y se
    // llevaba por delante todo lo demás.
    if payload["master"]["sueltos"].as_bool().unwrap_or(false) {
        return revela_sueltos(&plan, d, rend, ffmpeg, &ruta_plan, out_name, out_ext, pfps);
    }

    // ── UNA COPIA: EL FOTOGRAMA, no la película (MOTOR §12) ───────────
    // La ampliadora del cuarto oscuro. Mismo plan y mismo motor; lo único
    // distinto es que se pide UN renglón y que la salida es una imagen.
    if !payload["master"]["still"].is_null() {
        return revela_copia(&payload, &plan, d, rend, ffmpeg, &ruta_plan, out_name,
                            pfps, parte.cadena, parte.salida);
    }

    // ── LA CACHÉ DE LA BOBINA (MOTOR §7) ──────────────────────────────
    // El camino viejo cacheaba PIEZAS: cambiar el grade de un clip no
    // recalculaba los demás. El motor de bobina no tiene piezas, así que se
    // cachea la bobina entera por su plan: revelar dos veces lo mismo (tras
    // un cierre, una cancelación, o para probar otro sonido) es instantáneo.
    // Lo que aún NO hace es afinar por clip; eso pide caché por GOP.
    let mut hasher = DefaultHasher::new();
    plan["clips"].to_string().hash(&mut hasher);
    plan["clips2"].to_string().hash(&mut hasher);
    plan["prefs"].to_string().hash(&mut hasher);
    format!("{pw}x{ph}@{pfps:.4}").hash(&mut hasher);
    format!("{m_codec}|{m_bitrate}").hash(&mut hasher);
    file_sig(lut_in).hash(&mut hasher);
    file_sig(lut_grade).hash(&mut hasher);
    file_sig(rend).hash(&mut hasher);
    for c in &clips {
        file_sig(&resolve_media(d, c["file"].as_str().unwrap_or(""))).hash(&mut hasher);
    }
    let clave = format!("bobina_{:016x}.{out_ext}", hasher.finish());
    let cache = pieces_cache(d);
    let guardada = cache.join(&clave);

    if guardada.is_file() {
        diario(&format!("ESTA BOBINA YA ESTABA REVELADA ({clave}): del cajón, gratis"));
        set_render(|s| s.log += "bobina: de la caché\n");
        std::fs::copy(&guardada, &mudo).map_err(|e| format!("sacar de la caché: {e}"))?;
        // toca el mtime para que la evicción no se la lleve
        let _ = std::fs::File::options().append(true).open(&guardada);
    } else {
        set_render(|s| { s.step = "revelando la bobina".into(); s.pct = 0.1; });
        let t = std::time::Instant::now();
        // ── LA CACHÉ FINA, POR TRAMOS (MOTOR §7) ──────────────────────
        // La bobina se trocea en un tramo por clip (con su junta incluida) y
        // cada tramo se cachea por su CONTENIDO: qué fuente, en qué segundo y
        // con qué receta. Así, tocar el grade de un clip recalcula ese clip y
        // nada más; y como la clave no lleva la posición en la bobina, un
        // tramo sigue valiendo aunque lo de delante haya cambiado de duración.
        let hecho = revela_por_tramos(&plan, d, rend, ffmpeg, &ruta_plan, &mudo,
                                      &cache, out_ext, pfps)?;
        let seg = t.elapsed().as_secs_f64().max(0.001);
        let dur_master = probe(&mudo)["dur"].as_f64().unwrap_or(0.0);
        diario(&format!("BOBINA REVELADA en {seg:.1} s → {:.0} fps ({:.1} s de máster · {hecho})",
                        dur_master * pfps / seg, dur_master));
        if mudo.is_file() { let _ = std::fs::copy(&mudo, &guardada); }
        evict_cache(&cache, 300);
    }
    if !mudo.is_file() { return Err("el motor no dejó máster".into()); }

    // ── el sonido de los clips, en UNA pasada ──
    set_render(|s| { s.step = "la banda de voces".into(); s.pct = 0.75; });
    let voces = d.tmp.join("voces.m4a");
    let _ = std::fs::remove_file(&voces);
    let con_voz = hornea_voces(d, ffmpeg, &clips, pfps, &voces).unwrap_or_else(|e| {
        diario(&format!("⚠ sin banda de voces: {e}"));
        false
    });

    let master = d.tmp.join(format!("master.{out_ext}"));
    let _ = std::fs::remove_file(&master);
    if con_voz {
        run_logged(quiet_cmd(ffmpeg).args([
            "-hide_banner", "-loglevel", "error", "-y",
            "-i", mudo.to_str().unwrap(), "-i", voces.to_str().unwrap(),
            "-map", "0:v", "-map", "1:a", "-c", "copy", "-shortest",
            master.to_str().unwrap()]), "pegar la voz")?;
        let _ = std::fs::remove_file(&mudo);
    } else {
        std::fs::rename(&mudo, &master).map_err(|e| format!("mover el máster: {e}"))?;
    }
    termina_master(payload, d, ffmpeg, &master, out_name, out_ext, pfps, parte)
}

/// REVELAR POR TRAMOS: uno por clip (con su junta). Cada uno se cachea por su
/// contenido, así que cambiar un clip no recalcula la bobina entera. Los
/// tramos se pegan con `concat -c copy` porque cada uno empieza con un
/// fotograma clave: el motor fuerza uno en su primer fotograma.
///
/// Si algo no cuadra —un tramo que no sale, un concat que falla— se cae al
/// revelado de la bobina de un tirón, que siempre funciona.
#[allow(clippy::too_many_arguments)]
/// REVELAR LA BOBINA COMO UNA CARPETA DE CLIPS SUELTOS.
///
/// Un fichero por plano, numerado en el orden de la bobina y con el nombre
/// del material de origen, dentro de `<destino>/<bobina>/`. Cada uno lleva ya
/// el look, el encuadre y la cadencia del máster; lo que NO lleva es el
/// montaje: ni juntas, ni fundidos de la bobina, ni música.
///
/// Por qué existe: para usar el taller como laboratorio de revelado digital y
/// llevarse los planos revelados a otro editor. Ahí el corte lo pone el otro
/// programa, así que pegarlos aquí sólo estorbaría.
///
/// El sonido de cada plano SÍ va dentro: se saca del original con el mismo
/// recorte, sin recodificar la imagen. Un plano sin su sonido no sirve para
/// montar.
#[allow(clippy::too_many_arguments)]
/// LA COPIA: un fotograma revelado en papel (MOTOR §12).
///
/// El motor escribe SIEMPRE un PNG de 16 bits al lienzo de la cadena; aquí se
/// reduce (si hubo supermuestreo) y se convierte al papel pedido. La copia es
/// mejor que un fotograma sacado del máster con ffmpeg, y por eso existe: no
/// pasa por el códec, ni por el submuestreo de croma, ni por el rango
/// limitado del YUV.
///
/// LA CARRERILLA no es un adorno: con obturador (`shutter`) el arrastre se
/// forma con los fotogramas anteriores. Sin ella, la copia de un plano en
/// movimiento saldría más limpia que el máster en ese mismo segundo — o sea,
/// mentiría sobre lo que va a salir.
#[allow(clippy::too_many_arguments)]
fn revela_copia(payload: &serde_json::Value, plan: &serde_json::Value, d: &Dirs,
                rend: &Path, ffmpeg: &str, ruta_plan: &Path, out_name: &str,
                pfps: f64, cadena: (u64, u64), salida: (u64, u64)) -> Result<(), String> {
    let st = &payload["master"]["still"];
    let t = st["t"].as_f64().unwrap_or(0.0).max(0.0);
    let papel = st["papel"].as_str().unwrap_or("png16");
    let comp = crate::plan::compila(plan).map_err(|e| e.to_string())?;
    let total = comp.renglones.len();
    if total == 0 { return Err("la bobina no tiene ni un fotograma".into()); }
    // EL RENGLÓN DE ESE SEGUNDO. Se redondea al fotograma que el máster
    // tendría ahí: la copia y el máster han de enseñar lo mismo.
    let k = ((t * pfps).round() as usize).min(total - 1);
    // la carrerilla que quepa: hasta 12 fotogramas antes
    let carrerilla = k.min(12);
    diario(&format!("LA COPIA: fotograma {k} de {total} (t={t:.3} s) \
                     · carrerilla {carrerilla} · lienzo {}×{}", cadena.0, cadena.1));
    set_render(|s| { s.state = "running".into(); s.step = "revelando el fotograma".into();
                     s.pct = 0.2; });

    let crudo = d.tmp.join("copia_cruda.png");
    let _ = std::fs::remove_file(&crudo);
    let mut cmd = quiet_cmd(rend.to_str().unwrap_or_default());
    cmd.args(["bobina", ruta_plan.to_str().unwrap(),
              "--luts", d.luts.to_str().unwrap(),
              "--desde", &k.to_string(), "--cuantos", "1",
              "--carrerilla", &carrerilla.to_string(),
              "--out", crudo.to_str().unwrap()]);
    run_logged(&mut cmd, "copia")?;
    if !crudo.is_file() { return Err("el motor no escribió el fotograma".into()); }

    // ── el papel y el tamaño ──────────────────────────────────────────
    let ext = if papel == "jpg" { "jpg" } else { "png" };
    let destino = payload["out_dir"].as_str()
        .map(PathBuf::from)
        .filter(|p| p.is_dir() || std::fs::create_dir_all(p).is_ok())
        .unwrap_or_else(|| d.out.clone())
        .join("copias");
    std::fs::create_dir_all(&destino)
        .map_err(|e| format!("no pude crear {}: {e}", destino.display()))?;
    // el nombre lleva el SEGUNDO: dos copias del mismo plano no se pisan
    let sello = format!("{out_name}_{:02}m{:06.3}s", (t / 60.0) as u64, t % 60.0)
        .replace('.', "_");
    let mut out_path = destino.join(format!("{sello}.{ext}"));
    let mut n = 2;
    while out_path.exists() {
        out_path = destino.join(format!("{sello}_{n}.{ext}"));
        n += 1;
    }
    set_render(|s| { s.step = "el papel".into(); s.pct = 0.8; });
    // ¿hay algo que hacer? Si el papel es PNG de 16 y el lienzo ya es el
    // tamaño pedido, el fichero del motor ES la copia: se mueve y punto.
    let reducir = cadena != salida;
    if papel == "png16" && !reducir {
        std::fs::rename(&crudo, &out_path)
            .or_else(|_| std::fs::copy(&crudo, &out_path).map(|_| ()))
            .map_err(|e| format!("no pude dejar la copia: {e}"))?;
    } else {
        let mut c = quiet_cmd(ffmpeg);
        c.args(["-v", "error", "-y", "-i", crudo.to_str().unwrap()]);
        if reducir {
            // lanczos: el mismo reductor que el máster supermuestreado
            c.args(["-vf", &format!("scale={}:{}:flags=lanczos", salida.0, salida.1)]);
        }
        match papel {
            "jpg" => { c.args(["-q:v", "2", "-pix_fmt", "yuvj444p"]); }
            "png8" => { c.args(["-pix_fmt", "rgb24"]); }
            _ => { c.args(["-pix_fmt", "rgb48be"]); }
        }
        c.arg(out_path.to_str().unwrap());
        run_logged(&mut c, "papel")?;
        let _ = std::fs::remove_file(&crudo);
    }
    let (fw, fh) = if reducir { salida } else { cadena };
    let bytes = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
    diario(&format!("LA COPIA: {}×{fh} {papel} · {} KB → {}",
                    fw, bytes / 1024, out_path.display()));
    let ruta = out_path.to_string_lossy().to_string();
    set_render(move |s| {
        s.state = "done".into();
        s.step = "la copia, en papel".into();
        s.pct = 1.0;
        s.out = ruta.clone();
    });
    Ok(())
}

fn revela_sueltos(payload: &serde_json::Value, d: &Dirs, rend: &Path, ffmpeg: &str,
                  ruta_plan: &Path, out_name: &str, out_ext: &str, pfps: f64)
    -> Result<(), String> {
    let comp = crate::plan::compila(payload).map_err(|e| e.to_string())?;
    // UN PLANO = renglones seguidos con la misma fuente A. No sirve
    // `tramos()`: ése parte también donde cambia el lado B, y desde el
    // remuestreo de cadencia el lado B entra y sale casi en cada fotograma —
    // tres clips salían en cuatro ficheros, dos de ellos de diez fotogramas.
    let mut tramos: Vec<(usize, usize)> = Vec::new();
    for (i, r) in comp.renglones.iter().enumerate() {
        match tramos.last_mut() {
            Some((desde, cuantos)) if comp.renglones[*desde].fuente_a == r.fuente_a => {
                *cuantos += 1;
            }
            _ => tramos.push((i, 1)),
        }
    }
    if tramos.is_empty() { return Err("la bobina no tiene ni un plano".into()); }
    let destino = payload["out_dir"].as_str().map(PathBuf::from)
        .unwrap_or_else(|| d.out.clone())
        .join(out_name);
    std::fs::create_dir_all(&destino).map_err(|e| format!("crear la carpeta: {e}"))?;
    diario(&format!("CLIPS SUELTOS: {} plano(s) → {}", tramos.len(), destino.display()));
    let clips = payload["clips"].as_array().cloned().unwrap_or_default();
    let total = tramos.len();
    for (k, &(desde, cuantos)) in tramos.iter().enumerate() {
        if CANCEL.load(Ordering::SeqCst) { return Err("cancelado".into()); }
        // el nombre: número de orden + el del material, que es como se
        // reconoce un plano en una carpeta de cien
        let src = comp.renglones.get(desde).map(|r| r.fuente_a as usize).unwrap_or(0);
        let base = comp.fuentes.get(src)
            .map(|f| Path::new(&f.fichero).file_stem()
                 .map(|s| s.to_string_lossy().to_string()).unwrap_or_default())
            .unwrap_or_default();
        let base: String = base.chars().filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .take(48).collect();
        let salida = destino.join(format!("{:03}_{}.{out_ext}", k + 1, base));
        let mudo = d.tmp.join(format!("suelto_mudo.{out_ext}"));
        let _ = std::fs::remove_file(&mudo);
        let mut cmd = quiet_cmd(rend.to_str().unwrap_or_default());
        cmd.args(["bobina", ruta_plan.to_str().unwrap(),
                  "--luts", d.luts.to_str().unwrap(),
                  "--desde", &desde.to_string(),
                  "--cuantos", &cuantos.to_string(),
                  "--out", mudo.to_str().unwrap()]);
        run_logged(&mut cmd, &format!("plano {} de {total}", k + 1))?;
        if !mudo.is_file() { return Err(format!("el plano {} no salió", k + 1)); }
        // el sonido del original, con el mismo recorte y sin tocar la imagen
        let sonido = clips.get(src).and_then(|c| {
            let f = c["file"].as_str()?;
            let t0 = c["in"].as_f64().unwrap_or(0.0);
            let dur = cuantos as f64 / pfps.max(1.0);
            Some((resolve_media(d, f), t0, dur))
        });
        let _ = std::fs::remove_file(&salida);
        let ok = match &sonido {
            Some((ruta, t0, dur)) if ruta.is_file() => {
                run_logged(quiet_cmd(ffmpeg).args([
                    "-hide_banner", "-loglevel", "error", "-y",
                    "-i", mudo.to_str().unwrap(),
                    "-ss", &format!("{t0:.4}"), "-t", &format!("{dur:.4}"),
                    "-i", ruta.to_str().unwrap(),
                    "-map", "0:v:0", "-map", "1:a:0?", "-c:v", "copy",
                    "-c:a", "aac", "-b:a", "256k", "-shortest",
                    salida.to_str().unwrap()]), "sonido del plano").is_ok()
            }
            _ => false,
        };
        // sin pista de audio utilizable, el plano sale mudo y se dice
        if !ok || !salida.is_file() {
            let _ = std::fs::remove_file(&salida);
            std::fs::rename(&mudo, &salida)
                .map_err(|e| format!("sacar el plano {}: {e}", k + 1))?;
        }
        let hechos = k + 1;
        set_render(move |s| {
            s.pct = 0.1 + 0.85 * hechos as f64 / total as f64;
            s.step = format!("plano {hechos} de {total}");
        });
    }
    diario(&format!("LISTO: {total} plano(s) revelados en {}", destino.display()));
    set_render(move |s| { s.pct = 1.0; s.step = "terminado".into(); });
    Ok(())
}

/// CUÁNTOS FOTOGRAMAS tiene de verdad un fichero. Cuenta paquetes, que es lo
/// único fiable: la duración del contenedor se puede escribir bien aunque
/// dentro no haya nada, y un fichero roto ni siquiera la trae.
fn fotogramas_de(p: &Path) -> usize {
    let Ok(o) = quiet_cmd(ffbin("ffprobe")).args([
        "-v", "error", "-select_streams", "v:0", "-count_packets",
        "-show_entries", "stream=nb_read_packets", "-of", "csv=p=0",
        p.to_str().unwrap_or("")]).output() else { return 0 };
    String::from_utf8_lossy(&o.stdout).trim().parse().unwrap_or(0)
}

fn revela_por_tramos(plan: &serde_json::Value, d: &Dirs, rend: &Path, ffmpeg: &str,
                     ruta_plan: &Path, mudo: &Path, cache: &Path,
                     out_ext: &str, pfps: f64) -> Result<String, String> {
    let entero = |quien: &str| -> Result<String, String> {
        diario(&format!("   tramos: {quien} — revelo la bobina de un tirón"));
        let mut cmd = quiet_cmd(rend.to_str().unwrap_or_default());
        cmd.args(["bobina", ruta_plan.to_str().unwrap(),
                  "--luts", d.luts.to_str().unwrap()]);
        run_logged(&mut cmd, "bobina")?;
        Ok("de un tirón".into())
    };

    let Ok(comp) = crate::plan::compila(plan) else { return entero("no compila") };
    let tr = crate::plan::tramos(&comp.renglones);
    // un tramo por CLIP: cada junta se pega al tramo que la precede
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for t in &tr {
        match spans.last_mut() {
            Some(s) if t.fuentes.len() > 1 => s.1 += t.cuantos,   // la junta, al anterior
            _ => spans.push((t.desde, t.cuantos)),
        }
    }
    // ── TROCEAR MÁS FINO LOS CLIPS LARGOS (PENDIENTE §6) ──────────────
    // Un tramo se invalidaba ENTERO si cambiaba cualquier cosa del clip: en un
    // plano de tres minutos, recortarle medio segundo la cola obligaba a
    // revelar los tres minutos otra vez. Los tramos empiezan siempre con
    // fotograma clave (el motor fuerza uno), así que partirlos por dentro no
    // cuesta nada en el pegado: solo unos pocos keyframes más.
    //
    // El corte se hace en la rejilla ABSOLUTA de la bobina, no en la relativa
    // al clip: si se hiciera relativa, mover el clip un fotograma desplazaría
    // todos los trozos y la caché no acertaría ninguno.
    let piezas_max: usize = std::env::var("FL_TROZO").ok().and_then(|v| v.parse().ok())
        .unwrap_or_else(|| (comp.fps * 6.0).round().max(24.0) as usize);
    let mut finos: Vec<(usize, usize)> = Vec::new();
    for (desde, cuantos) in spans {
        if cuantos <= piezas_max * 3 / 2 { finos.push((desde, cuantos)); continue; }
        let mut i = desde;
        let fin = desde + cuantos;
        while i < fin {
            // el corte cae en un múltiplo del trozo contado desde el ORIGEN
            let siguiente = ((i / piezas_max) + 1) * piezas_max;
            let hasta = siguiente.clamp(i + 1, fin);
            // no dejar una colilla ridícula al final
            let hasta = if fin - hasta < piezas_max / 4 { fin } else { hasta };
            finos.push((i, hasta - i));
            i = hasta;
        }
    }
    let spans = finos;
    // trocear de más cuesta más de lo que ahorra: cada tramo es un proceso
    if spans.len() < 2 || spans.len() > 400 { return entero("no compensa trocear"); }
    diario(&format!("   tramos: {} pieza(s) de hasta {piezas_max} fotograma(s)", spans.len()));

    let mut piezas: Vec<PathBuf> = Vec::new();
    let (mut nuevos, mut cacheados) = (0usize, 0usize);
    for (k, &(desde, cuantos)) in spans.iter().enumerate() {
        // la clave es el CONTENIDO del tramo, no dónde cae en la bobina
        let mut h = DefaultHasher::new();
        for r in &comp.renglones[desde..(desde + cuantos).min(comp.renglones.len())] {
            let mut linea = format!("{}|{}|{:.5}|{:.5}|{:.5}|{:.5}",
                    r.fuente_a, r.fuente_b, r.peso_b, r.t_a, r.t_b, r.nivel_color);
            for c in &r.capas {
                if c.fuente == crate::plan::NINGUNA { continue }
                linea += &format!("|{}~{:.5}~{:.4}", c.fuente, c.t, c.alfa);
            }
            linea.hash(&mut h);
        }
        for f in comp.renglones[desde..(desde + cuantos).min(comp.renglones.len())].iter()
                     .flat_map(|r| {
                         let mut v = vec![r.fuente_a, r.fuente_b];
                         v.extend(r.capas.iter().map(|c| c.fuente));
                         v
                     })
                     .filter(|x| (*x as usize) < comp.fuentes.len())
                     .collect::<std::collections::BTreeSet<_>>() {
            let s = &comp.fuentes[f as usize];
            file_sig(Path::new(&s.fichero)).hash(&mut h);
            s.prefs.to_string().hash(&mut h);
            // el ENCUADRE entero entra en la clave: si cambia, el tramo se
            // vuelve a revelar (y si no, sale del cajón tal cual)
            // la matriz explícita y la marca de capa TAMBIÉN son la receta:
            // mover una anidada o convertir algo en capa cambia el tramo
            format!("{:?}|{:?}|{:?}|{}|{}|{:?}",
                    s.lut_in, s.lut, s.enc, s.foto, s.capa, s.mat).hash(&mut h);
        }
        format!("{}x{}@{:.4}|{}|{}", comp.w, comp.h, comp.fps, comp.codec, comp.bitrate)
            .hash(&mut h);
        file_sig(rend).hash(&mut h);
        let pieza = cache.join(format!("tramo_{:016x}.{out_ext}", h.finish()));
        if pieza.is_file() {
            let _ = std::fs::File::options().append(true).open(&pieza);
            cacheados += 1;
        } else {
            let mut cmd = quiet_cmd(rend.to_str().unwrap_or_default());
            cmd.args(["bobina", ruta_plan.to_str().unwrap(),
                      "--luts", d.luts.to_str().unwrap(),
                      "--desde", &desde.to_string(),
                      "--cuantos", &cuantos.to_string(),
                      "--out", pieza.to_str().unwrap()]);
            // ── CADA PIEZA SE CUENTA ─────────────────────────────────
            // Que el motor salga con código 0 y deje un fichero NO basta: un
            // tramo puede escribir menos fotogramas de los pedidos —o
            // ninguno— y el fichero queda ahí, ilegible. El `concat` se topa
            // con él y **corta el máster en seco**: al autor le faltaba el
            // último plano de una bobina de 71 s, que salía de 52,4 s, sin un
            // solo aviso en ninguna parte. Contar cuesta 200 ms por pieza.
            let mut intenta = |cmd: &mut std::process::Command, etq: &str| -> bool {
                run_logged(cmd, etq).is_ok() && pieza.is_file() && {
                    let n = fotogramas_de(&pieza);
                    let bien = n + 2 >= cuantos;   // el mux puede dejar uno fuera
                    if !bien {
                        diario(&format!("   ⚠ el tramo {} escribió {n} de {cuantos} \
                                         fotograma(s): no vale", k + 1));
                    }
                    bien
                }
            };
            // UN REINTENTO. El fallo que se vio era transitorio —el mismo
            // tramo, lanzado solo, escribía sus 360 fotogramas sin rechistar—
            // así que rendirse a la primera es dejar al autor sin máster por
            // algo que se arregla repitiendo. Si falla dos veces, ya no es
            // mala suerte.
            let mut ok = intenta(&mut cmd, &format!("tramo {}", k + 1));
            if !ok {
                let _ = std::fs::remove_file(&pieza);
                diario(&format!("   el tramo {} se repite", k + 1));
                ok = intenta(&mut cmd, &format!("tramo {} (repetido)", k + 1));
            }
            if !ok {
                let _ = std::fs::remove_file(&pieza);
                for p in &piezas { let _ = std::fs::remove_file(p); }
                return entero("un tramo salió incompleto");
            }
            nuevos += 1;
        }
        piezas.push(pieza);
        // EL PROGRESO DICE QUÉ FALTA, no solo un porcentaje (§5)
        let n_tramos = spans.len();
        set_render(move |s| {
            s.pct = 0.1 + 0.6 * (k + 1) as f64 / n_tramos as f64;
            s.step = format!("tramo {} de {n_tramos}", k + 1);
        });
    }

    // pegar: cada tramo empieza con fotograma clave, así que es copia pura
    let lst = d.tmp.join("tramos.txt");
    let mut txt = String::new();
    for p in &piezas {
        txt += &format!("file '{}'\n", p.to_str().unwrap().replace('\'', r"'\''"));
    }
    if std::fs::write(&lst, txt).is_err() { return entero("no pude escribir la lista"); }
    let _ = std::fs::remove_file(mudo);
    if run_logged(quiet_cmd(ffmpeg).args([
        "-hide_banner", "-loglevel", "error", "-y", "-f", "concat", "-safe", "0",
        "-i", lst.to_str().unwrap(), "-c", "copy", mudo.to_str().unwrap()]),
        "pegar los tramos").is_err() || !mudo.is_file() {
        return entero("el pegado falló");
    }
    let _ = pfps;
    Ok(format!("{} tramo(s): {nuevos} revelados, {cacheados} del cajón", spans.len()))
}

/// EL CODIFICADOR DE SALIDA, con lo que de verdad hay en cada máquina.
///
/// La regla vieja era «si en esta máquina no lo hace el chip, no aparece». La
/// nueva es más útil y sigue siendo honesta: **aparece todo, y lo que va por
/// software lo dice**. El autor decide si le compensa esperar; lo que no puede
/// pasar es que espere sin saber por qué.
fn pon_codec(cmd: &mut Command, codec: &str, bitrate: i64) {
    match codec {
        "prores422hq" | "prores4444" => {
            let (enc, soft) = if cfg!(target_os = "macos") {
                ("prores_videotoolbox", false)
            } else {
                ("prores_ks", true)   // en Windows no hay motor: software
            };
            if soft { diario("   ProRes por SOFTWARE (en esta máquina no hay motor): va a tardar"); }
            let cuatro = codec == "prores4444";
            cmd.args(["-c:v", enc, "-profile:v", if cuatro { "4" } else { "3" },
                      "-pix_fmt", if cuatro { "yuva444p10le" } else { "p210le" }]);
        }
        "h264" => {
            let enc = if cfg!(windows) { "h264_amf" } else { "h264_videotoolbox" };
            // el H.264 por hardware es de 8 bits: se dice, no se disimula
            diario("   H.264 son 8 bits: el máster pierde el 10-bit del look");
            cmd.args(["-c:v", enc, "-b:v", &bitrate.to_string(), "-pix_fmt", "yuv420p"]);
        }
        "hevc_soft" => {
            diario("   HEVC por SOFTWARE (x265): lento, pero manda el autor");
            cmd.args(["-c:v", "libx265", "-crf", "14", "-preset", "slow",
                      "-pix_fmt", "yuv420p10le", "-tag:v", "hvc1"]);
        }
        _ => {
            let enc = if cfg!(windows) { "hevc_amf" } else { "hevc_videotoolbox" };
            cmd.args(["-c:v", enc, "-b:v", &bitrate.to_string(),
                      "-pix_fmt", "p010le", "-tag:v", "hvc1"]);
        }
    }
}

/// La banda de las VOCES: el sonido propio de los clips, con sus fundidos y
/// sus encadenados, en una sola pasada de ffmpeg. El vídeo ya no la trae
/// porque el motor entrega la bobina muda.
fn hornea_voces(d: &Dirs, ffmpeg: &str, clips: &[serde_json::Value], fps: f64,
                salida: &Path) -> Result<bool, String> {
    let mut cmd = quiet_cmd(ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y"]);
    let mut fg = String::new();
    let mut etiquetas: Vec<String> = Vec::new();
    let mut n_in = 0usize;
    // EL DESPEGUE DE LA JUNTA (§ el tirón del audio). Un corte seco pega dos
    // ondas por donde caigan, y donde una acaba en −0,3 y la siguiente empieza
    // en +0,4 hay un escalón: eso es el chasquido. Tres milisegundos de
    // fundido a cada lado del empalme lo quitan y no se oyen — es el
    // equivalente sonoro del empalme de la moviola, que tampoco es a hueso.
    const DESPEGUE: f64 = 0.003;
    for (ic, c) in clips.iter().enumerate() {
        let hueco = c["gap"].as_bool().unwrap_or(false);
        let t_in = c["in"].as_f64().unwrap_or(0.0);
        let t_out = c["out"].as_f64().unwrap_or(0.0).max(t_in);
        let vel = c["speed"].as_f64().unwrap_or(1.0).clamp(0.1, 8.0);
        // la MISMA rejilla que el plan de bobina, o el sonido se desliza
        let n_f = ((t_out - t_in) / vel * fps).round().max(1.0) as u64;
        let dur = n_f as f64 / fps;
        let src = resolve_media(d, c["file"].as_str().unwrap_or(""));
        let tiene = !hueco && !is_image(c["file"].as_str().unwrap_or(""))
                    && tiene_audio(&src);
        if tiene {
            cmd.args(["-ss", &format!("{t_in:.4}"), "-to", &format!("{t_out:.4}"),
                      "-i", src.to_str().unwrap()]);
        } else {
            cmd.args(["-f", "lavfi", "-t", &format!("{dur:.4}"),
                      "-i", "anullsrc=channel_layout=stereo:sample_rate=48000"]);
        }
        let mut ch = format!("[{n_in}:a]aresample=48000,aformat=channel_layouts=stereo");
        if tiene && (vel - 1.0).abs() > 0.001 {
            let mut sp = vel;
            while sp > 2.0 { ch += ",atempo=2.0"; sp /= 2.0; }
            while sp < 0.5 { ch += ",atempo=0.5"; sp *= 2.0; }
            ch += &format!(",atempo={sp:.4}");
        }
        // recortar EXACTO a la rejilla del vídeo y rellenar si faltara.
        // EN MUESTRAS, no en segundos: `atrim=0:3.0400` corta en un punto que
        // casi nunca cae en una muestra entera, y ese medio error por clip se
        // va sumando corte a corte hasta que el sonido se desliza del vídeo.
        let muestras = (dur * 48000.0).round() as u64;
        ch += &format!(",apad,atrim=end_sample={muestras},asetpts=PTS-STARTPTS");
        if c["mute"].as_bool().unwrap_or(false) { ch += ",volume=0"; }
        let fi = c["fadeIn"].as_f64().unwrap_or(0.0);
        let fo = c["fadeOut"].as_f64().unwrap_or(0.0);
        if fi > 0.005 { ch += &format!(",afade=t=in:st=0:d={fi:.3}"); }
        if fo > 0.005 { ch += &format!(",afade=t=out:st={:.3}:d={fo:.3}", (dur - fo).max(0.0)); }
        // el despegue, SOLO en los empalmes a hueso: donde hay encadenado ya
        // se ocupa el propio `acrossfade`, y donde hay fundido, el fundido
        let junta_antes = ic == 0
            || clips[ic - 1]["fade"].as_f64().unwrap_or(0.0) <= 0.01;
        let junta_despues = ic + 1 >= clips.len()
            || c["fade"].as_f64().unwrap_or(0.0) <= 0.01;
        if junta_antes && fi <= 0.005 && dur > DESPEGUE * 3.0 {
            ch += &format!(",afade=t=in:st=0:d={DESPEGUE:.4}");
        }
        if junta_despues && fo <= 0.005 && dur > DESPEGUE * 3.0 {
            ch += &format!(",afade=t=out:st={:.4}:d={DESPEGUE:.4}", dur - DESPEGUE);
        }
        let et = format!("[v{n_in}]");
        fg += &format!("{ch}{et};");
        etiquetas.push(et);
        n_in += 1;
    }
    if etiquetas.is_empty() { return Ok(false); }

    // las juntas: donde el vídeo encadena, el sonido también
    let mut actual = etiquetas[0].clone();
    for i in 1..etiquetas.len() {
        let f = clips[i - 1]["fade"].as_f64().unwrap_or(0.0);
        let sig = format!("[j{i}]");
        if f > 0.01 {
            fg += &format!("{actual}{}acrossfade=d={f:.3}{sig};", etiquetas[i]);
        } else {
            fg += &format!("{actual}{}concat=n=2:v=0:a=1{sig};", etiquetas[i]);
        }
        actual = sig;
    }
    let fg = format!("{}", fg.trim_end_matches(';'));
    cmd.args(["-filter_complex", &fg, "-map", &actual,
              "-c:a", "aac", "-b:a", "256k", salida.to_str().unwrap()]);
    run_logged(&mut cmd, "voces")?;
    Ok(salida.is_file())
}

/// ¿esta receta hace ALGO? Si todos los mandos del cuarto oscuro están a
/// cero, el revelado no cambia un píxel y se puede copiar en vez de revelar.
fn prefs_hace_algo(p: &serde_json::Value) -> bool {
    const MANDOS: [&str; 14] = ["grain", "halation", "bloom", "vignette", "dust",
        "flicker", "breath", "softness", "acutance", "colorSep", "chroma",
        "shutter", "weave", "frameInset"];
    for k in MANDOS { if p[k].as_f64().unwrap_or(0.0).abs() > 1e-6 { return true; } }
    // los que su neutro NO es cero
    if (p["gain"].as_f64().unwrap_or(0.0)).abs() > 1e-6 { return true; }
    if (p["pushPull"].as_f64().unwrap_or(0.0)).abs() > 1e-6 { return true; }
    if (p["compImpact"].as_f64().unwrap_or(0.0)).abs() > 1e-6 { return true; }
    if (p["stockSat"].as_f64().unwrap_or(1.0) - 1.0).abs() > 1e-6 { return true; }
    if (p["print"].as_f64().unwrap_or(0.0)).abs() > 1e-6 { return true; }
    if (p["subtractive"].as_f64().unwrap_or(0.0)).abs() > 1e-6 { return true; }
    if (p["crosstalk"].as_f64().unwrap_or(0.0)).abs() > 1e-6 { return true; }
    if (p["hueSkew"].as_f64().unwrap_or(1.0) - 1.0).abs() > 1e-6 { return true; }
    if (p["filmRes"].as_f64().unwrap_or(0.0)).abs() > 1e-6 { return true; }
    false
}

fn tiene_audio(p: &Path) -> bool {
    quiet_cmd(ffbin("ffprobe"))
        .args(["-v", "error", "-select_streams", "a:0",
               "-show_entries", "stream=codec_type", "-of", "csv=p=0",
               p.to_str().unwrap_or("")])
        .output()
        .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false)
}

// ── ffprobe ────────────────────────────────────────────────────────────────

fn probe(path: &Path) -> serde_json::Value {
    let out = quiet_cmd(ffbin("ffprobe"))
        .args([
            "-v", "error", "-select_streams", "v:0",
            "-show_entries",
            "stream=width,height,r_frame_rate,avg_frame_rate,duration,codec_name:stream_side_data=rotation",
            "-of", "json", path.to_str().unwrap_or(""),
        ])
        .output();
    let fallback = serde_json::json!({"w": 0, "h": 0, "fps": 0, "dur": 0});
    let Ok(out) = out else { return fallback };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else { return fallback };
    let s = &v["streams"][0];
    let parse_rate = |r: &str| -> f64 {
        let (num, den) = r.split_once('/').unwrap_or((r, "1"));
        num.parse::<f64>().unwrap_or(0.0) / den.parse::<f64>().unwrap_or(1.0).max(1.0)
    };
    let fps = parse_rate(s["r_frame_rate"].as_str().unwrap_or("0/1"));
    let avg = parse_rate(s["avg_frame_rate"].as_str().unwrap_or("0/1"));
    // VFR: la cadencia declarada y la media difieren de verdad
    let vfr = fps > 0.0 && avg > 0.0 && (fps - avg).abs() / fps > 0.02;
    let mut dur = s["duration"].as_str().and_then(|d| d.parse::<f64>().ok()).unwrap_or(0.0);
    if dur == 0.0 {
        if let Ok(o2) = quiet_cmd(ffbin("ffprobe"))
            .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0",
                   path.to_str().unwrap_or("")])
            .output()
        {
            dur = String::from_utf8_lossy(&o2.stdout).trim().parse().unwrap_or(0.0);
        }
    }
    // rotación de metadatos (móviles en vertical): las dimensiones ÚTILES son
    // las giradas — el corte re-encodea con autorotate y las aplica de verdad
    let rot = s["side_data_list"].as_array()
        .and_then(|l| l.iter().find_map(|sd| sd["rotation"].as_f64()))
        .unwrap_or(0.0);
    let ws = s["width"].as_u64().unwrap_or(0);
    let hs = s["height"].as_u64().unwrap_or(0);
    let (mut w, mut h) = (ws, hs);
    // LOS CUARTOS DE VUELTA, en el mismo modelo que el resto del taller: 1 =
    // 90° a derechas. ffprobe da la rotación con el signo cambiado respecto a
    // la matriz del `tkhd` (devuelve −90 para un giro de un cuarto a
    // derechas), así que aquí se normaliza una vez y no se vuelve a pensar.
    let cuartos = (((-rot / 90.0).round() as i64).rem_euclid(4)) as u64;
    if cuartos % 2 == 1 {
        std::mem::swap(&mut w, &mut h);
    }
    serde_json::json!({
        "w": w,
        "h": h,
        // las dimensiones TAL Y COMO ESTÁN GUARDADAS: es lo que entrega el
        // decodificador, y lo que necesita el conform del motor
        "wsrc": ws,
        "hsrc": hs,
        "cuartos": cuartos,
        "fps": (if vfr { avg } else { fps } * 1000.0).round() / 1000.0,
        "dur": (dur * 1000.0).round() / 1000.0,
        "rot": rot,
        // el códec, que lo pide el atajo de identidad (MOTOR §7)
        "codec": s["codec_name"].as_str().unwrap_or(""),
        "vfr": vfr,
    })
}

// ── ficheros con Range (vídeo en la webview) ───────────────────────────────

fn serve_file_c(rq: tiny_http::Request, path: &Path, ranged: bool, cache: bool) {
    serve_file_inner(rq, path, ranged, cache)
}
fn serve_file(rq: tiny_http::Request, path: &Path, ranged: bool) {
    serve_file_inner(rq, path, ranged, false)
}
fn serve_file_inner(rq: tiny_http::Request, path: &Path, ranged: bool, cache: bool) {
    if !path.is_file() {
        let _ = rq.respond(Response::from_string("not found").with_status_code(404));
        return;
    }
    let ctype = mime(&path.to_string_lossy());
    let size = path.metadata().map(|m| m.len()).unwrap_or(0);
    let range = if ranged {
        rq.headers().iter().find(|h| h.field.equiv("Range")).map(|h| h.value.to_string())
    } else {
        None
    };
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => {
            let _ = rq.respond(Response::from_string("io").with_status_code(500));
            return;
        }
    };
    if let Some(r) = range {
        let r = r.trim_start_matches("bytes=");
        let (a, b) = r.split_once('-').unwrap_or((r, ""));
        let a: u64 = a.parse().unwrap_or(0);
        let b: u64 = b.parse().unwrap_or(size.saturating_sub(1)).min(size.saturating_sub(1));
        let len = b.saturating_sub(a) + 1;
        let _ = f.seek(SeekFrom::Start(a));
        let mut buf = vec![0u8; len as usize];
        let _ = f.read_exact(&mut buf);
        let mut resp = Response::from_data(buf)
            .with_status_code(206)
            .with_header(hdr("Content-Type", ctype))
            .with_header(hdr("Content-Range", &format!("bytes {a}-{b}/{size}")))
            .with_header(hdr("Accept-Ranges", "bytes"));
        if cache {
            resp = resp.with_header(hdr("Cache-Control", "max-age=604800"));
        }
        let _ = rq.respond(resp);
    } else {
        let mut resp = Response::from_file(f)
            .with_header(hdr("Content-Type", ctype))
            .with_header(hdr("Accept-Ranges", "bytes"));
        if cache {
            resp = resp.with_header(hdr("Cache-Control", "max-age=604800"));
        }
        let _ = rq.respond(resp);
    }
}

fn serve_embed<E: RustEmbed>(rq: tiny_http::Request, key: &str) {
    match E::get(key) {
        Some(f) => {
            let resp = Response::from_data(f.data.into_owned())
                .with_header(hdr("Content-Type", mime(key)))
                .with_header(hdr("Cache-Control", "no-store"));
            let _ = rq.respond(resp);
        }
        None => {
            let _ = rq.respond(Response::from_string("not found").with_status_code(404));
        }
    }
}

fn decode(p: &str) -> String {
    percent_decode_str(p).decode_utf8_lossy().to_string()
}

fn json_resp(rq: tiny_http::Request, body: String) {
    let resp = Response::from_string(body)
        .with_header(hdr("Content-Type", "application/json"))
        .with_header(hdr("Cache-Control", "no-store"));
    let _ = rq.respond(resp);
}

// ── modo agente: CLI sin ventana ───────────────────────────────────────────

fn media_json(d: &Dirs) -> String {
    let mut items = Vec::new();
    let mut names: Vec<_> = std::fs::read_dir(&d.media)
        .map(|it| it.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect())
        .unwrap_or_default();
    names.sort();
    for name in names {
        if is_media(&name) {
            let mut v = probe(&d.media.join(&name));
            v["name"] = serde_json::json!(name);
            v["kind"] = serde_json::json!(if is_audio(&name) { "audio" } else { "video" });
            v["path"] = serde_json::json!(d.media.join(&name).to_string_lossy());
            items.push(v);
        }
    }
    serde_json::json!(items).to_string()
}

fn luts_json() -> String {
    let mut out = serde_json::Map::new();
    for slot in ["entrada", "color"] {
        let mut list: Vec<String> = Studio::iter()
            .filter_map(|k| {
                k.strip_prefix(&format!("luts/{slot}/"))
                    .filter(|n| n.to_lowercase().ends_with(".cube"))
                    .map(|n| n.to_string())
            })
            .collect();
        list.sort();
        out.insert(slot.into(), serde_json::json!(list));
    }
    serde_json::Value::Object(out).to_string()
}

/// `saorin cli media|luts|dirs|render --json <timeline.json|->`
pub fn cli(args: &[String]) -> i32 {
    let d = dirs();
    match args.first().map(|s| s.as_str()) {
        Some("media") => {
            println!("{}", media_json(&d));
            0
        }
        // ── EL OÍDO: subtítulos automáticos, en casa y sin red ─────────
        Some("oye") => {
            let arg = |k: &str| args.iter().position(|a| a == k)
                .and_then(|i| args.get(i + 1)).cloned();
            // o UN fichero (--media) o LA BOBINA ENTERA (--trabajos, una
            // lista de planos con su trozo y su sitio): con la lista el
            // modelo se carga UNA vez, que es lo caro
            let mut mal_json: Option<String> = None;
            let lista: Vec<crate::oido::Trabajo> = match arg("--trabajos") {
                Some(f) => {
                    // EL BOM: en Windows medio mundo escribe UTF-8 con marca
                    // (PowerShell lo hace de serie) y serde no la traga. Antes
                    // el fallo se tragaba con un `[]` y el parte decía «falta
                    // --trabajos» cuando el fichero estaba ahí y bien puesto.
                    let v: serde_json::Value = match std::fs::read(&f) {
                        Ok(b) => {
                            let b = b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&b).to_vec();
                            match serde_json::from_slice(&b) {
                                Ok(v) => v,
                                Err(e) => {
                                    mal_json = Some(format!("{f}: {e}"));
                                    serde_json::json!([])
                                }
                            }
                        }
                        Err(e) => { mal_json = Some(format!("{f}: {e}")); serde_json::json!([]) }
                    };
                    v.as_array().cloned().unwrap_or_default().iter().map(|j| {
                        crate::oido::Trabajo {
                            fichero: resolve_media(&d, j["file"].as_str().unwrap_or("")),
                            t_in: j["in"].as_f64().unwrap_or(0.0),
                            t_out: j["out"].as_f64().unwrap_or(0.0),
                            desde: j["desde"].as_f64().unwrap_or(0.0),
                            velocidad: j["speed"].as_f64().unwrap_or(1.0),
                        }
                    }).collect()
                }
                None => Vec::new(),
            };
            let media = arg("--media").unwrap_or_default();
            if let Some(e) = &mal_json {
                eprintln!("no pude leer la lista de planos — {e}");
                return 2;
            }
            if lista.is_empty() && media.is_empty() {
                eprintln!("falta --media o --trabajos"); return 2;
            }
            let ruta = resolve_media(&d, &media);
            let idioma = arg("--idioma").unwrap_or_else(|| "es".into());
            // sin --modelo manda la máquina: Metal aguanta el bueno, la CPU no
            let cual = arg("--modelo").and_then(|s| s.parse::<usize>().ok())
                .unwrap_or_else(crate::oido::el_de_esta_maquina);
            let salida = arg("--out").map(PathBuf::from)
                .unwrap_or_else(|| d.tmp.join("subs.srt"));
            let aviso = |m: &str| {
                diario(m);
                set_render(|s| { s.state = "running".into();
                                 s.step = m.to_string(); s.pct = 0.35; });
            };
            set_render(|s| { s.state = "running".into();
                             s.step = "el oído: preparando".into(); s.pct = 0.05; });
            // el modelo vive en el TALLER (la carpeta madre de media/), no
            // en project.json — que es un fichero, no una carpeta
            let taller = d.media.parent().unwrap_or(&d.media).to_path_buf();
            let r = crate::oido::modelo(&taller, cual, &aviso)
                .and_then(|m| {
                    set_render(|s| { s.pct = 0.25; s.step = "el oído: escuchando".into(); });
                    let largo = arg("--largo").and_then(|s| s.parse::<i32>().ok())
                        .unwrap_or(crate::oido::LARGO_PIE);
                    if lista.is_empty() {
                        crate::oido::escucha(&m, &ffbin("ffmpeg"), &ruta, &idioma, largo, &aviso)
                    } else {
                        crate::oido::escucha_bobina(&m, &ffbin("ffmpeg"), &lista, &idioma,
                                                    largo, &aviso)
                    }
                });
            match r {
                Ok((trozos, palabras)) => {
                    let texto = crate::oido::srt(&trozos);
                    if let Some(p) = salida.parent() { std::fs::create_dir_all(p).ok(); }
                    // LAS PALABRAS, al lado del .srt y con su mismo nombre:
                    // es lo que deja a la app recomponer los subtítulos sin
                    // volver a escuchar (otro largo de línea, otro corte)
                    let jpal = salida.with_extension("palabras.json");
                    if let Err(e) = std::fs::write(&jpal, crate::oido::palabras_json(&palabras)) {
                        diario(&format!("   ⚠ no pude escribir las palabras: {e}"));
                    }
                    if let Err(e) = std::fs::write(&salida, texto) {
                        eprintln!("no pude escribir {}: {e}", salida.display());
                        set_render(|s| { s.state = "error".into(); s.step = format!("{e}"); });
                        return 1;
                    }
                    diario(&format!("EL OÍDO: {} trozo(s) · {} palabra(s) → {}",
                                    trozos.len(), palabras.len(), salida.display()));
                    let ruta_s = salida.to_string_lossy().to_string();
                    set_render(move |s| {
                        s.state = "done".into();
                        s.step = "subtítulos escritos".into();
                        s.pct = 1.0;
                        s.out = ruta_s.clone();
                    });
                    println!("{}", serde_json::json!({"state": "done",
                        "out": salida.to_string_lossy(), "trozos": trozos.len(),
                        "palabras": palabras.len()}));
                    0
                }
                Err(e) => {
                    eprintln!("el oído falló: {e}");
                    let m = e.clone();
                    set_render(move |s| { s.state = "error".into(); s.step = m.clone(); });
                    println!("{}", serde_json::json!({"state": "error", "error": e}));
                    1
                }
            }
        }
        Some("luts") => {
            println!("{}", luts_json());
            0
        }
        Some("dirs") => {
            println!(
                "{}",
                serde_json::json!({
                    "media": d.media, "out": d.out, "luts": d.luts,
                    "project": d.project, "renderer": renderer(),
                })
            );
            0
        }
        Some("render") => {
            extract_luts(&d);
            let src = args.iter().position(|a| a == "--json").and_then(|i| args.get(i + 1));
            let body = match src.map(|s| s.as_str()) {
                Some("-") | None => {
                    let mut s = String::new();
                    use std::io::Read as _;
                    let _ = std::io::stdin().read_to_string(&mut s);
                    s
                }
                Some(p) => std::fs::read_to_string(p).unwrap_or_default(),
            };
            let payload: serde_json::Value = match serde_json::from_str(&body) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("timeline JSON inválido: {e}");
                    return 2;
                }
            };
            // progreso a stderr mientras el render corre en este hilo
            let watcher = std::thread::spawn(|| {
                let mut last = String::new();
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(700));
                    let g = RENDER.lock().unwrap().clone();
                    let Some(s) = g else { continue };
                    if s.step != last {
                        eprintln!("· {} ({:.0}%)", s.step, s.pct * 100.0);
                        last = s.step.clone();
                    }
                    if s.state == "done" || s.state == "error" {
                        break;
                    }
                }
            });
            set_render(|s| {
                *s = RenderState {
                    state: "running".into(),
                    step: "preparando".into(),
                    pct: 0.0,
                    log: String::new(),
                    out: String::new(),
                    started: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
                }
            });
            let result = render_job(payload, &d);
            match &result {
                Ok(()) => {}
                Err(e) => {
                    // QUE SE ENTIENDA POR QUÉ FALLA. Decía «mira el log», y el
                    // log está en otra ventana: el motivo viaja por stderr con
                    // la misma marca que el progreso, así que la sala lo
                    // enseña sin tener que ir a buscarlo (§4).
                    eprintln!("· FALLÓ: {e} (0%)");
                    eprintln!("✗ el revelado falló: {e}");
                    set_render(|s| {
                        s.state = "error".into();
                        s.step = format!("FALLÓ: {e}");
                    });
                }
            }
            let _ = watcher.join();
            let g = RENDER.lock().unwrap().clone().unwrap();
            // ya viene absoluta; el prefijo «/out/» es el de la ruta web
            // del servidor y sólo se traduce si aparece
            let out_abs = if g.out.is_empty() {
                serde_json::Value::Null
            } else if let Some(rel) = g.out.strip_prefix("/out/") {
                serde_json::json!(d.out.join(rel).to_string_lossy())
            } else {
                serde_json::json!(g.out)
            };
            println!(
                "{}",
                serde_json::json!({"state": g.state, "out": out_abs, "log": g.log})
            );
            if result.is_ok() { 0 } else { 1 }
        }
        _ => {
            eprintln!("uso: saorin cli media|luts|dirs|render --json <timeline.json|->");
            eprintln!("timeline: {{\"clips\":[{{\"file\":\"a.mp4\",\"in\":0,\"out\":3.5}}],");
            eprintln!("           \"lut_in\":\"…\",\"lut\":\"…\",\"prefs\":{{…}},\"out_name\":\"x\"}}");
            2
        }
    }
}

// ── servidor ───────────────────────────────────────────────────────────────

pub fn start() -> u16 {
    let d = dirs();
    extract_luts(&d);
    let mut port = 8788u16;
    let server = loop {
        match Server::http(("127.0.0.1", port)) {
            Ok(s) => break s,
            Err(_) => {
                port += 1;
                assert!(port < 8820, "sin puerto libre");
            }
        }
    };
    eprintln!("🎬 LABORATORIOS SAORÍN · http://127.0.0.1:{port}");
    eprintln!("   media: {}", d.media.display());
    eprintln!("   renderer: {}", renderer().display());

    // 8 obreros: las peticiones lentas (miniaturas, proxies) no bloquean al
    // resto — el servidor era MONO-HILO y un ffmpeg parab todo el taller
    let server = std::sync::Arc::new(server);
    for _ in 0..8 {
        let server = server.clone();
        std::thread::spawn(move || {
            let d = dirs();
            while let Ok(rq) = server.recv() {
                handle_req(rq, &d);
            }
        });
    }
    port
}

fn handle_req(mut rq: tiny_http::Request, d: &Dirs) {
    {
            let url = rq.url().to_string();
            let (path_raw, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
            let path = decode(path_raw);
            let q: std::collections::HashMap<String, String> = query
                .split('&')
                .filter_map(|kv| kv.split_once('='))
                .map(|(k, v)| (k.to_string(), decode(&v.replace('+', " "))))
                .collect();

            match (rq.method().clone(), path.as_str()) {
                (Method::Get, "/") | (Method::Get, "/index.html") => {
                    serve_embed::<Studio>(rq, "index.html")
                }
                (Method::Get, p) if p.starts_with("/css/") || p.starts_with("/js/")
                    || p.starts_with("/assets/") || p.starts_with("/zine/") =>
                {
                    serve_embed::<Studio>(rq, p.trim_start_matches('/'))
                }
                (Method::Get, p) if p.starts_with("/engine/") => {
                    serve_embed::<Engine>(rq, &p["/engine/".len()..].to_string())
                }
                (Method::Get, p) if p.starts_with("/luts/") => {
                    // primero la lutoteca embebida; si no, el cajón del disco
                    let key = p.trim_start_matches('/').to_string();
                    if Studio::get(&key).is_some() {
                        serve_embed::<Studio>(rq, &key)
                    } else {
                        let rel = key.trim_start_matches("luts/");
                        serve_file(rq, &d.luts.join(rel), false)
                    }
                }
                (Method::Get, p) if p.starts_with("/media/") => {
                    let name = Path::new(&p["/media/".len()..]).file_name().unwrap_or_default().to_string_lossy().to_string();
                    serve_file_c(rq, &resolve_media(&d, &name), true, true)
                }
                (Method::Get, p) if p.starts_with("/out/") => {
                    let name = Path::new(&p["/out/".len()..]).file_name().unwrap_or_default().to_owned();
                    serve_file(rq, &d.out.join(name), true)
                }
                (Method::Get, "/api/media") => {
                    let mut names: Vec<String> = std::fs::read_dir(&d.media)
                        .map(|it| it.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect())
                        .unwrap_or_default();
                    for (name, _) in load_index(&d) {
                        if !names.contains(&name) {
                            names.push(name);
                        }
                    }
                    names.sort();
                    let mut items = Vec::new();
                    for name in names {
                        if is_media(&name) {
                            let path = resolve_media(&d, &name);
                            // referencia rota: la lata aparece OFFLINE (nunca se
                            // esconde en silencio — la estructura del proyecto manda)
                            let mut v = if path.is_file() {
                                probe(&path)
                            } else {
                                serde_json::json!({"w": 0, "h": 0, "fps": 0, "dur": 0, "missing": true})
                            };
                            v["name"] = serde_json::json!(name);
                            v["kind"] = serde_json::json!(if is_audio(&name) { "audio" } else { "video" });
                            v["url"] = serde_json::json!(format!(
                                "/media/{}",
                                percent_encoding::utf8_percent_encode(&name, percent_encoding::NON_ALPHANUMERIC)
                            ));
                            items.push(v);
                        }
                    }
                    json_resp(rq, serde_json::json!(items).to_string())
                }
                (Method::Get, "/api/import/dialog") => {
                    let paths = import_dialog();
                    let added = register_paths(&d, &paths);
                    json_resp(rq, serde_json::json!({"added": added}).to_string())
                }
                (Method::Post, "/api/import") => {
                    let mut body = String::new();
                    let _ = rq.as_reader().read_to_string(&mut body);
                    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                    let paths: Vec<String> = v["paths"].as_array().cloned().unwrap_or_default()
                        .iter().filter_map(|p| p.as_str().map(String::from)).collect();
                    let added = register_paths(&d, &paths);
                    json_resp(rq, serde_json::json!({"added": added}).to_string())
                }
                (Method::Post, "/api/media/rename") => {
                    let mut body = String::new();
                    let _ = rq.as_reader().read_to_string(&mut body);
                    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                    let from = v["from"].as_str().unwrap_or("");
                    let to_raw = v["to"].as_str().unwrap_or("");
                    let ext = ext_of(from);
                    let mut to: String = to_raw.chars()
                        .map(|c| if c == '/' || c == '\\' || c == ':' { '_' } else { c })
                        .collect::<String>().trim().to_string();
                    if to.is_empty() || from.is_empty() {
                        json_resp(rq, r#"{"error":"nombre vacío"}"#.into());
                    } else {
                        if !to.to_ascii_lowercase().ends_with(&format!(".{ext}")) {
                            to = format!("{to}.{ext}");
                        }
                        let mut idx = load_index(&d);
                        // colisiones: sufijo (2), (3)…
                        let base = to.clone();
                        let mut k = 2;
                        while idx.contains_key(&to) || d.media.join(&to).exists() {
                            let (stem, e2) = base.rsplit_once('.').unwrap_or((&base, ""));
                            to = format!("{stem} ({k}).{e2}");
                            k += 1;
                        }
                        if let Some(pv) = idx.remove(from) {
                            idx.insert(to.clone(), pv);
                        }
                        let local = d.media.join(from);
                        if local.is_file() {
                            let _ = std::fs::rename(&local, d.media.join(&to));
                        }
                        save_index(&d, &idx);
                        // los sidecars cacheados del nombre viejo se tiran
                        let _ = std::fs::remove_file(proxies_dir(&d).join(from));
                        let _ = std::fs::remove_file(d.thumbs.join(format!("{from}.m4a")));
                        let _ = std::fs::remove_file(d.thumbs.join(format!("{from}.wave.png")));
                        json_resp(rq, serde_json::json!({"name": to}).to_string());
                    }
                }
                (Method::Post, "/api/media/remove") => {
                    let mut body = String::new();
                    let _ = rq.as_reader().read_to_string(&mut body);
                    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                    if let Some(name) = v["name"].as_str() {
                        let mut idx = load_index(&d);
                        idx.remove(name);
                        save_index(&d, &idx);
                        let local = d.media.join(Path::new(name).file_name().unwrap_or_default());
                        if local.is_file() {
                            // lo físico no se borra: se aparta a la papelera del taller
                            let pap = d.media.parent().unwrap_or(Path::new(".")).join(".papelera");
                            let _ = std::fs::create_dir_all(&pap);
                            let _ = std::fs::rename(&local, pap.join(local.file_name().unwrap()));
                        }
                    }
                    json_resp(rq, "{}".into())
                }
                (Method::Get, "/api/proxy") => {
                    let name = q.get("f").cloned().unwrap_or_default();
                    let name = Path::new(&name).file_name().unwrap_or_default().to_string_lossy().to_string();
                    let ready = ensure_proxy(&d, &name);
                    json_resp(rq, serde_json::json!({
                        "ready": ready,
                        "url": format!("/proxy/{}", percent_encoding::utf8_percent_encode(&name, percent_encoding::NON_ALPHANUMERIC)),
                    }).to_string())
                }
                (Method::Get, p) if p.starts_with("/proxy/") => {
                    let name = Path::new(&p["/proxy/".len()..]).file_name().unwrap_or_default().to_owned();
                    serve_file_c(rq, &proxies_dir(&d).join(name), true, true)
                }
                (Method::Get, "/api/audio") => {
                    // audio del clip como m4a (con Range: los <audio> hacen seek)
                    let name = q.get("f").cloned().unwrap_or_default();
                    let name = Path::new(&name).file_name().unwrap_or_default().to_string_lossy().to_string();
                    match ensure_audio_m4a(&d, &name) {
                        Some(p) => serve_file_c(rq, &p, true, true),
                        None => {
                            let _ = rq.respond(Response::from_string("sin audio").with_status_code(404));
                        }
                    }
                }
                (Method::Get, "/api/wave") => {
                    // forma de onda del clip entero, cacheada (terracota sobre nada)
                    let name = q.get("f").cloned().unwrap_or_default();
                    let name = Path::new(&name).file_name().unwrap_or_default().to_string_lossy().to_string();
                    let prox = proxies_dir(&d).join(&name);
                    let src = if prox.is_file() { prox } else { resolve_media(&d, &name) };
                    let dst = d.thumbs.join(format!("{name}.wave.png"));
                    if !dst.is_file() && src.is_file() {
                        let _ = quiet_cmd(ffbin("ffmpeg"))
                            .args(["-hide_banner", "-loglevel", "error", "-y",
                                   "-i", src.to_str().unwrap(),
                                   "-filter_complex",
                                   "aformat=channel_layouts=mono,showwavespic=s=2048x64:colors=#b45a38",
                                   "-frames:v", "1", dst.to_str().unwrap()])
                            .output();
                    }
                    serve_file_c(rq, &dst, false, true)
                }
                (Method::Get, "/api/thumb") => {
                    let name = q.get("f").cloned().unwrap_or_default();
                    let name = Path::new(&name).file_name().unwrap_or_default().to_string_lossy().to_string();
                    let t: f64 = q.get("t").and_then(|v| v.parse().ok()).unwrap_or(1.0);
                    // del proxy si existe (all-intra: seek instantáneo)
                    let prox = proxies_dir(&d).join(&name);
                    let src = if prox.is_file() { prox } else { resolve_media(&d, &name) };
                    let dst = d.thumbs.join(format!("{name}_{t:.1}.jpg"));
                    if !dst.is_file() && src.is_file() {
                        let _ = quiet_cmd(ffbin("ffmpeg"))
                            .args(["-hide_banner", "-loglevel", "error", "-y",
                                   "-ss", &t.to_string(), "-i", src.to_str().unwrap(),
                                   "-frames:v", "1", "-vf", "scale=320:-2",
                                   dst.to_str().unwrap()])
                            .output();
                    }
                    serve_file_c(rq, &dst, false, true)
                }
                (Method::Get, "/api/project") => {
                    let pp = current_project_path(&d);
                    if pp.is_file() {
                        serve_file(rq, &pp, false)
                    } else {
                        json_resp(rq, "null".into())
                    }
                }
                (Method::Get, "/api/projects") => {
                    let cur = std::fs::read_to_string(current_marker(&d)).unwrap_or_default().trim().to_string();
                    let mut names: Vec<String> = std::fs::read_dir(projects_dir(&d))
                        .map(|it| it.flatten()
                            .filter_map(|e| {
                                let n = e.file_name().to_string_lossy().to_string();
                                n.strip_suffix(".json").map(|s2| s2.to_string())
                            })
                            .collect())
                        .unwrap_or_default();
                    names.sort();
                    json_resp(rq, serde_json::json!({"current": cur, "projects": names}).to_string())
                }
                (Method::Post, "/api/projects/open") => {
                    let mut body = String::new();
                    let _ = rq.as_reader().read_to_string(&mut body);
                    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                    let name = sane_name(v["name"].as_str().unwrap_or(""));
                    let _ = std::fs::write(current_marker(&d), &name);
                    json_resp(rq, "{}".into())
                }
                (Method::Post, "/api/projects/new") => {
                    let mut body = String::new();
                    let _ = rq.as_reader().read_to_string(&mut body);
                    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                    let name = sane_name(v["name"].as_str().unwrap_or(""));
                    if name.is_empty() {
                        json_resp(rq, r#"{"error":"nombre vacío"}"#.into());
                    } else {
                        let pp = projects_dir(&d).join(format!("{name}.json"));
                        if !pp.exists() {
                            let _ = std::fs::write(&pp, "null");
                        }
                        let _ = std::fs::write(current_marker(&d), &name);
                        json_resp(rq, serde_json::json!({"name": name}).to_string())
                    }
                }
                (Method::Get, "/api/engine") => {
                    let r = renderer();
                    json_resp(rq, serde_json::json!({
                        "name": r.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                        "path": r.to_string_lossy(),
                        "exists": r.exists(),
                        "tier": render_tier(),
                        "zero_copy": render_tier() == "nativo",
                    }).to_string())
                }
                (Method::Post, "/api/media/relink") => {
                    // buscar el fichero perdido en una carpeta, recursivamente
                    let mut body = String::new();
                    let _ = rq.as_reader().read_to_string(&mut body);
                    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                    let name = v["name"].as_str().unwrap_or("").to_string();
                    let folder = if cfg!(target_os = "macos") {
                        quiet_cmd("osascript").arg("-e")
                            .arg(r#"try
  POSIX path of (choose folder with prompt "¿Dónde vive ahora el material?")
on error
  ""
end try"#)
                            .output().ok()
                            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                            .unwrap_or_default()
                    } else { String::new() };
                    let mut found: Vec<(String, PathBuf)> = Vec::new();
                    if !folder.is_empty() {
                        // qué nombres siguen perdidos (todos, no solo el pedido)
                        let idx = load_index(&d);
                        let missing: Vec<String> = idx.keys()
                            .filter(|n| !resolve_media(&d, n).is_file())
                            .cloned().collect();
                        let targets: Vec<String> = if missing.is_empty() && !name.is_empty() {
                            vec![name.clone()]
                        } else { missing };
                        fn walk(dir: &Path, depth: usize, hits: &mut Vec<PathBuf>) {
                            if depth > 6 { return; }
                            let Ok(rd) = std::fs::read_dir(dir) else { return };
                            for e in rd.flatten() {
                                let p2 = e.path();
                                if p2.is_dir() {
                                    walk(&p2, depth + 1, hits);
                                } else {
                                    hits.push(p2);
                                }
                            }
                        }
                        let mut files = Vec::new();
                        walk(Path::new(&folder), 0, &mut files);
                        let mut idx2 = load_index(&d);
                        for t in &targets {
                            // el nombre físico original (sin el sufijo " (2)")
                            let base = Path::new(t).file_name().unwrap_or_default().to_string_lossy().to_string();
                            if let Some(hit) = files.iter().find(|f| {
                                f.file_name().map(|n| n.to_string_lossy() == base.as_str()).unwrap_or(false)
                            }) {
                                idx2.insert(t.clone(), serde_json::json!(hit.to_string_lossy()));
                                found.push((t.clone(), hit.clone()));
                            }
                        }
                        save_index(&d, &idx2);
                    }
                    json_resp(rq, serde_json::json!({
                        "relinked": found.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>()
                    }).to_string())
                }
                (Method::Post, "/api/upload") => {
                    // drag&drop del Finder: los bytes llegan por HTTP y el
                    // fichero se guarda FÍSICAMENTE en el taller
                    let name = q.get("name").cloned().unwrap_or_default();
                    let name = Path::new(&name).file_name().unwrap_or_default().to_string_lossy().to_string();
                    if !is_media(&name) {
                        json_resp(rq, r#"{"error":"formato no soportado"}"#.into());
                    } else {
                        let mut dst = d.media.join(&name);
                        let mut k = 2;
                        while dst.exists() {
                            let (stem, ext) = name.rsplit_once('.').unwrap_or((&name, ""));
                            dst = d.media.join(format!("{stem} ({k}).{ext}"));
                            k += 1;
                        }
                        let tmp = dst.with_extension("subiendo");
                        let mut f = std::fs::File::create(&tmp).ok();
                        let mut ok = false;
                        if let Some(fh) = f.as_mut() {
                            ok = std::io::copy(rq.as_reader(), fh).is_ok();
                        }
                        if ok {
                            let _ = std::fs::rename(&tmp, &dst);
                            json_resp(rq, serde_json::json!({
                                "name": dst.file_name().unwrap().to_string_lossy()
                            }).to_string());
                        } else {
                            let _ = std::fs::remove_file(&tmp);
                            json_resp(rq, r#"{"error":"no se pudo guardar"}"#.into());
                        }
                    }
                }
                (Method::Post, "/api/preview") => {
                    let mut body = String::new();
                    let _ = rq.as_reader().read_to_string(&mut body);
                    let mut v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                    // el nombre de cinta se resuelve a ruta real
                    if let Some(n) = v["clip"].as_str().map(|x| x.to_string()) {
                        let real = resolve_media(&d, &n);
                        v["clip"] = serde_json::json!(real.to_string_lossy());
                    }
                    match nativa_cmd(&d, &v) {
                        Ok(()) => json_resp(rq, r#"{"ok":true}"#.into()),
                        Err(e) => json_resp(rq, serde_json::json!({"error": e}).to_string()),
                    }
                }
                (Method::Post, "/api/preview/stop") => {
                    nativa_stop();
                    json_resp(rq, "{}".into())
                }
                (Method::Post, "/api/log") => {
                    // banco de pruebas: la página escribe sus medidas aquí
                    let mut body = String::new();
                    let _ = rq.as_reader().read_to_string(&mut body);
                    eprintln!("[banco] {body}");
                    use std::io::Write as _;
                    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true)
                        .open(d.tmp.join("banco.log")) {
                        let _ = writeln!(f, "{body}");
                    }
                    json_resp(rq, "{}".into())
                }
                (Method::Get, "/api/media-version") => {
                    json_resp(rq, serde_json::json!({
                        "v": MEDIA_VERSION.load(Ordering::SeqCst)
                    }).to_string())
                }
                (Method::Post, "/api/render/cancel") => {
                    CANCEL.store(true, Ordering::SeqCst);
                    json_resp(rq, "{}".into())
                }
                (Method::Get, "/api/luts") => {
                    let mut out = serde_json::Map::new();
                    for slot in ["entrada", "color"] {
                        let mut list: Vec<String> = Studio::iter()
                            .filter_map(|k| {
                                k.strip_prefix(&format!("luts/{slot}/"))
                                    .filter(|n| n.to_lowercase().ends_with(".cube"))
                                    .map(|n| n.to_string())
                            })
                            .collect();
                        // + los .cube que el usuario deje en luts/<slot>/ del taller
                        if let Ok(rd) = std::fs::read_dir(d.luts.join(slot)) {
                            for e in rd.flatten() {
                                let n = e.file_name().to_string_lossy().to_string();
                                if n.to_lowercase().ends_with(".cube") && !list.contains(&n) {
                                    list.push(n);
                                }
                            }
                        }
                        list.sort();
                        out.insert(slot.into(), serde_json::json!(list));
                    }
                    json_resp(rq, serde_json::Value::Object(out).to_string())
                }
                (Method::Get, "/api/renders") => {
                    let mut items: Vec<serde_json::Value> = std::fs::read_dir(&d.out)
                        .map(|it| {
                            it.flatten()
                                .filter(|e| {
                                    let n = e.file_name().to_string_lossy().to_lowercase();
                                    n.ends_with(".mp4") || n.ends_with(".mov")
                                })
                                .map(|e| {
                                    let name = e.file_name().to_string_lossy().to_string();
                                    let st = e.metadata().ok();
                                    serde_json::json!({
                                        "name": name,
                                        "url": format!("/out/{}", percent_encoding::utf8_percent_encode(&name, percent_encoding::NON_ALPHANUMERIC)),
                                        "bytes": st.as_ref().map(|m| m.len()).unwrap_or(0),
                                        "mtime": st.and_then(|m| m.modified().ok())
                                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                            .map(|d| d.as_secs()).unwrap_or(0),
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    items.sort_by_key(|v| -(v["mtime"].as_u64().unwrap_or(0) as i64));
                    json_resp(rq, serde_json::json!(items).to_string())
                }
                (Method::Get, "/api/render/status") => json_resp(rq, render_status_json()),
                (Method::Post, "/api/project") => {
                    let mut body = String::new();
                    let _ = rq.as_reader().read_to_string(&mut body);
                    // el cuerpo tiene que ser JSON de verdad: jamás pisar la
                    // única copia con basura
                    if serde_json::from_str::<serde_json::Value>(&body).is_err() {
                        json_resp(rq, r#"{"error":"proyecto inválido"}"#.into());
                    } else {
                        let pp = current_project_path(&d);
                        // backup rotatorio de lo que había (20 copias)
                        if pp.is_file() {
                            let bdir = pp.parent().unwrap_or(Path::new(".")).join(".backups");
                            let _ = std::fs::create_dir_all(&bdir);
                            let ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d2| d2.as_secs()).unwrap_or(0);
                            let stem = pp.file_stem().unwrap_or_default().to_string_lossy().to_string();
                            let _ = std::fs::copy(&pp, bdir.join(format!("{stem}-{ts}.json")));
                            evict_cache(&bdir, 20);
                        }
                        // guardado atómico: temp + rename
                        let tmp = pp.with_extension("json.tmp");
                        if std::fs::write(&tmp, &body).is_ok() {
                            let _ = std::fs::rename(&tmp, &pp);
                        }
                        json_resp(rq, "{}".into())
                    }
                }
                (Method::Post, "/api/render") => {
                    let mut body = String::new();
                    let _ = rq.as_reader().read_to_string(&mut body);
                    let payload: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                    if RUNNING.swap(true, Ordering::SeqCst) {
                        json_resp(rq, r#"{"error":"ya hay un render"}"#.into());
                    } else {
                        set_render(|s| {
                            *s = RenderState {
                                state: "running".into(),
                                step: "preparando".into(),
                                pct: 0.0,
                                log: String::new(),
                                out: String::new(),
                                started: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
                            };
                        });
                        let dd = dirs();
                        std::thread::spawn(move || {
                            if let Err(e) = render_job(payload, &dd) {
                                set_render(|s| {
                                    s.state = "error".into();
                                    s.step = e;
                                });
                            }
                            RUNNING.store(false, Ordering::SeqCst);
                        });
                        json_resp(rq, "{}".into());
                    }
                }
                _ => {
                    let _ = rq.respond(Response::from_string("?").with_status_code(404));
                }
            }
    }
}
