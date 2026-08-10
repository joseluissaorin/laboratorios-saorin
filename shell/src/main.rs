//! LABORATORIOS SAORÍN — shell nativo (Tauri) del estudio.
//!
//! La UI, el motor WebGL y la lutoteca van EMBEBIDOS en el binario; el
//! servidor HTTP (puerto local) es el mismo contrato que server.py y la
//! webview nativa apunta a él. Media y renders viven en ~/filmlab.

#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

#[cfg(feature = "oido")]
mod oido;
#[cfg(not(feature = "oido"))]
#[path = "oido_no.rs"]
mod oido;
mod server;

/// El plan de bobina, incluido por ruta desde `core`: la matriz del encuadre
/// tiene que ser LA MISMA que usa el motor, o el máster saldría encuadrado de
/// otra manera. Se incluye en vez de depender del crate entero porque ese
/// arrastra wgpu y winit, que aquí no pintan nada.
#[path = "../../core/src/plan.rs"]
mod plan;

fn main() {
    // modo agente: `saorin cli …` (sin ventana) y `saorin serve` (solo HTTP)
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("cli") => std::process::exit(server::cli(&args[2..])),
        Some("serve") => {
            let port = server::start();
            eprintln!("(headless) http://127.0.0.1:{port}/");
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3600));
            }
        }
        _ => {}
    }

    ventana();
}

/// LA VENTANA DEL ESTUDIO VIEJO. Sin la bandera `ventana` este binario es lo
/// que de verdad es hoy: la herramienta de línea de órdenes que revela y
/// escucha, y a la que llama el editor nativo.
#[cfg(not(feature = "ventana"))]
fn ventana() {
    eprintln!("LABORATORIOS SAORÍN · el taller sin ventana.\n\
               ·  el editor es «saorin-nativa»\n\
               ·  aquí: `cli render --json …`, `cli oye …`, o `serve`");
}

#[cfg(feature = "ventana")]
fn ventana() {
    let port = server::start();
    let q = std::env::var("FL_QUERY").map(|v| format!("?{v}")).unwrap_or_default();
    let url: tauri::Url = format!("http://127.0.0.1:{port}/{q}").parse().unwrap();

    tauri::Builder::default()
        // soltar ficheros del Finder/Explorador: los coge Tauri (la webview
        // no ve los eventos HTML5 de arrastre) y se registran POR REFERENCIA,
        // sin copiar un solo byte
        .on_webview_event(|_wv, ev| {
            if let tauri::WebviewEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = ev {
                let rutas: Vec<String> =
                    paths.iter().map(|p| p.to_string_lossy().to_string()).collect();
                std::thread::spawn(move || { server::soltar(&rutas); });
            }
        })
        .setup(move |app| {
            tauri::WebviewWindowBuilder::new(
                app,
                "principal",
                tauri::WebviewUrl::External(url.clone()),
            )
            .title("LABORATORIOS SAORÍN")
            .inner_size(1500.0, 940.0)
            .min_inner_size(1100.0, 700.0)
            .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("no arranca la webview");
}
