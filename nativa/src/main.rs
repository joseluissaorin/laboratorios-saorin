//! LABORATORIOS SAORÍN — la app NATIVA.
//!
//! Sin webview, sin servidor, sin decodificador en JavaScript: una ventana
//! wgpu donde el mismo motor que revela pinta el visor, y la interfaz del
//! taller se dibuja con la misma pipeline (papel hueso, tinta ultramar,
//! rótulos a mano). Esto es el esqueleto: visor + bobina + transporte.

// EN WINDOWS, SIN CONSOLA. Sin esto el taller abre una ventana de `cmd` detrás
// con un log que no le interesa a nadie (el `shell` ya lo llevaba puesto).
// Los `eprintln!` de diagnóstico siguen ahí: se ven lanzando el binario desde
// una consola, que es cuando se buscan.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Result;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

mod arrastre_fuera;
mod cabina;
mod doodles;
mod foto;
mod menu;
mod papel;
mod prefs;
mod titulo;
mod miniaturas;
mod ondas;
mod proyecto;
mod ritmo;
mod subtitulo;
mod sonido;
mod trazo;
mod ui;
mod visor;

use proyecto::{Proyecto, BobinaInfo, FORMATOS, FPS_OPCIONES};

/// la paleta del taller (la misma del zine)
pub mod paleta {
    pub const HUESO: [f32; 4] = [0.949, 0.933, 0.894, 1.0];
    pub const HUESO_HONDO: [f32; 4] = [0.922, 0.906, 0.863, 1.0];
    pub const TINTA: [f32; 4] = [0.169, 0.231, 0.780, 1.0];
    pub const TINTA_TENUE: [f32; 4] = [0.169, 0.231, 0.780, 0.40];
    pub const ROJO: [f32; 4] = [0.851, 0.200, 0.145, 1.0];
    pub const NARANJA: [f32; 4] = [0.910, 0.314, 0.102, 1.0];
    pub const AMBAR: [f32; 4] = [0.949, 0.780, 0.267, 1.0];
    pub const PELICULA: [f32; 4] = [0.114, 0.106, 0.086, 1.0];
    pub const NEGRO: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
    // la luz de seguridad del cuarto oscuro (tinta sobre papel tiza)
    pub const SAFE: [f32; 4] = [1.0, 0.46, 0.32, 1.0];
    pub const SAFE_VIVO: [f32; 4] = [1.0, 0.30, 0.16, 1.0];
    pub const SAFE_TENUE: [f32; 4] = [1.0, 0.46, 0.32, 0.45];
    pub const TIZA: [f32; 4] = [0.90, 0.87, 0.82, 1.0];
    pub const TIZA_TENUE: [f32; 4] = [0.90, 0.87, 0.82, 0.5];
}

struct App {
    estado: Option<Estado>,
    proyecto: Proyecto,
    /// EL TALLER SE CERRÓ SIN APAGAR LA LUZ (§4bis.6): había una marca de
    /// «abierto» al arrancar, así que la sesión anterior no terminó bien
    cierre_brusco: bool,
}

/// el rótulo de una ventana secundaria, sin la coletilla del taller
fn q_titulo(q: Ventana) -> &'static str {
    match q {
        Ventana::Ajustes => "AJUSTES",
        Ventana::Chuleta => "LA CHULETA",
        Ventana::Vigia => "EL VIGÍA",
        Ventana::Bobinas => "LAS BOBINAS",
    }
}

/// EL TALLER CABE EN LA PANTALLA. Los 1500×940 de siempre se salen por la
/// derecha en un portátil pequeño —en el GPD, con 1707×1067 de escritorio, la
/// primera vez se abrió con los mandos fuera— y una posición guardada en otro
/// monitor deja la ventana donde no se puede coger. Se acota el tamaño al
/// monitor (reservando la franja de la barra de tareas o del Dock) y la
/// esquina al área visible. Con `x`/`y` a NaN sólo se acota el tamaño y la
/// coloca el sistema, que es lo que hay que hacer la primera vez.
fn encaja_en_pantalla(mon: Option<winit::monitor::MonitorHandle>,
                      g: (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
    const BARRA: f64 = 48.0;
    let (mut x, mut y, mut w, mut h) = g;
    let Some(m) = mon else { return g };
    let s = m.scale_factor().max(0.1);
    let (mw, mh) = (m.size().width as f64 / s, m.size().height as f64 / s);
    if mw < 320.0 || mh < 240.0 { return g }   // un monitor que no dice nada
    let (mx, my) = (m.position().x as f64 / s, m.position().y as f64 / s);
    w = w.min(mw);
    h = h.min((mh - BARRA).max(240.0));
    if x.is_finite() && y.is_finite() {
        x = x.clamp(mx, (mx + mw - w).max(mx));
        y = y.clamp(my, (my + mh - h).max(my));
    }
    (x, y, w, h)
}

/// EL ICONO DEL TALLER, el mismo que lleva el shell. Sin él, Windows pone el
/// suyo por defecto en la barra de tareas — y con la ventana sin marco, ese
/// icono genérico era lo único del sistema que quedaba a la vista.
fn icono_del_taller() -> Option<winit::window::Icon> {
    // VA DENTRO DEL BINARIO. Antes se leía del árbol de compilación en tiempo
    // de ejecución: la app instalada en otra carpeta —o llevada a otra
    // máquina— se quedaba con el icono genérico de Windows.
    const PNG: &[u8] = include_bytes!("../../shell/icons/128x128.png");
    let img = image::load_from_memory(PNG).ok()?.to_rgba8();
    let (w, h) = (img.width(), img.height());
    winit::window::Icon::from_rgba(img.into_raw(), w, h).ok()
}

/// EL BORDE POR EL QUE SE REDIMENSIONA. Una ventana sin marco no la
/// redimensiona nadie por ti: hay que decir dónde empieza el borde y pedirle
/// al sistema que arrastre desde ahí.
#[cfg(target_os = "windows")]
fn borde_en(ancho: f32, alto: f32, mx: f32, my: f32)
    -> Option<winit::window::ResizeDirection> {
    use winit::window::ResizeDirection as R;
    const G: f32 = 6.0;
    let (izq, der) = (mx <= G, mx >= ancho - G);
    let (arr, aba) = (my <= G, my >= alto - G);
    Some(match (arr, aba, izq, der) {
        (true, _, true, _) => R::NorthWest,
        (true, _, _, true) => R::NorthEast,
        (_, true, true, _) => R::SouthWest,
        (_, true, _, true) => R::SouthEast,
        (true, ..) => R::North,
        (_, true, ..) => R::South,
        (_, _, true, _) => R::West,
        (_, _, _, true) => R::East,
        _ => return None,
    })
}

#[cfg(not(target_os = "windows"))]
fn borde_en(_: f32, _: f32, _: f32, _: f32) -> Option<winit::window::ResizeDirection> { None }

/// la marca de «el taller está abierto». Se pone al arrancar y se quita al
/// cerrar en condiciones; si al arrancar sigue ahí, la última vez se cerró de
/// golpe y hay que ofrecer la copia del archivador (§4bis.6).
fn marca_abierto(base: &std::path::Path) -> bool {
    let m = base.join(".abierto");
    let habia = m.is_file();
    let _ = std::fs::write(&m, b"1");
    habia
}

fn quita_marca(base: &std::path::Path) {
    let _ = std::fs::remove_file(base.join(".abierto"));
}

/// LAS VENTANAS DEL TALLER (§3). Todo vivía en una y los paneles se dibujaban
/// encima, tapando el trabajo. Los ajustes y la chuleta son candidatos
/// naturales a ventana propia, y un VISOR SUELTO para el segundo monitor es lo
/// más útil de todo: revelar mirando la imagen a pantalla completa mientras se
/// monta.
///
/// La FICHA DEL CLIP no se saca: vive en la mesa, al lado del visor, porque
/// hay que verla mientras se mueve la imagen. Sacarla sería empeorarla.
#[derive(PartialEq, Clone, Copy, Debug)]
enum Ventana { Ajustes, Chuleta, Vigia, Bobinas }

impl Ventana {
    fn titulo(self) -> &'static str {
        match self {
            Ventana::Ajustes => "AJUSTES · Laboratorios Saorín",
            Ventana::Chuleta => "LA CHULETA · Laboratorios Saorín",
            Ventana::Vigia => "EL VIGÍA · Laboratorios Saorín",
            Ventana::Bobinas => "LAS BOBINAS · Laboratorios Saorín",
        }
    }
    fn clave(self) -> &'static str {
        match self {
            Ventana::Ajustes => "ajustes",
            Ventana::Chuleta => "chuleta",
            Ventana::Vigia => "vigia",
            Ventana::Bobinas => "bobinas",
        }
    }
    fn tam(self) -> (f64, f64) {
        match self {
            Ventana::Ajustes => (620.0, 620.0),
            Ventana::Chuleta => (800.0, 660.0),
            Ventana::Vigia => (960.0, 560.0),
            Ventana::Bobinas => (460.0, 620.0),
        }
    }
}

/// UN CRISTAL: lo que hace falta para que una ventana se pinte. El taller
/// («lo que sabe el taller») se queda en `Estado`; aquí solo vive el dibujo.
struct Cristal {
    ventana: Arc<Window>,
    gpu: ui::Gpu,
    lienzo: ui::Lienzo,
    tipos: ui::Tipos,
    que: Ventana,
}

/// en qué habitación del taller estamos (NORTE §2: tres salas de verdad)
#[derive(PartialEq, Clone, Copy, Debug)]
enum Sala {
    Portada,
    Mesa,
    CuartoOscuro,
    Revelado,
}

/// el monitor de FUENTE abierto sobre una cinta
struct FuenteUi {
    cinta: proyecto::Cinta,
    marca_i: Option<f64>,
    marca_o: Option<f64>,
    /// a qué punto de la bobina volvemos (y dónde inserta ⏎)
    t_bobina: f64,
}

/// el diálogo de «bobina nueva» en la portada
struct NuevaBobina {
    nombre: String,
    /// índice en FORMATOS; FORMATOS.len() = «auto» (del primer clip)
    aspecto: usize,
    /// índice en FPS_OPCIONES (0 = del clip)
    fps: usize,
    /// índice en proyecto::ALTURAS (0 = del clip)
    alto: usize,
    aviso: String,
}

struct Estado {
    ventana: Arc<Window>,
    /// dónde vive el taller (para las preferencias, que no son del proyecto)
    taller: std::path::PathBuf,
    gpu: ui::Gpu,
    lienzo: ui::Lienzo,
    tipos: ui::Tipos,
    /// la capa SUPERIOR: modales y chuleta (encima de texto, minis y vidrio)
    lienzo2: ui::Lienzo,
    tipos2: ui::Tipos,
    /// la capa del TELÓN: el pliegue de papel va por encima de TODO
    lienzo3: ui::Lienzo,
    /// el texto del telón: la capa de MÁS ARRIBA. La necesita el menú, que
    /// tiene que tapar lo que haya debajo (antes se transparentaba con el
    /// título de la portada, que vive en `tipos2`).
    tipos3: ui::Tipos,
    atlas: ui::Atlas,
    minis: miniaturas::Miniaturas,
    ondas: ondas::Ondas,
    papel: ui::Estampa,
    tape: ui::Estampa,
    /// el papel procedural (NORTE §1.1) — debajo de TODO
    fondo: papel::Papel,
    /// el atlas de objetos del taller (latas, botellas, pinzas…)
    objetos: ui::Estampa,
    sala: Sala,
    bobinas: Vec<BobinaInfo>,
    nueva: Option<NuevaBobina>,
    chuleta: bool,
    ajustes: bool,
    /// LA PISTA DE MÚSICA SELECCIONADA. Sin esto la música no era un clip:
    /// no se podía elegir, ni ver su ficha, ni borrarla — solo añadirla y
    /// arrastrarla a ciegas.
    sel_audio: Option<usize>,
    /// la capa elegida (CAPAS §7): manda sobre la ficha y sobre la cuchilla
    sel_capa: Option<usize>,
    /// el diálogo de revelado: Some(preset elegido 0..4)
    revelar: Option<usize>,
    /// el diálogo de TÍTULO: el texto en edición
    titulando: Option<String>,
    /// renombrando una cinta: (nombre viejo, texto en edición)
    renombrando: Option<(String, String)>,
    /// la nota manuscrita del clip en edición: (índice, texto)
    notando: Option<(usize, String)>,
    /// las baldas plegadas de la estantería
    baldas_cerradas: std::collections::HashSet<String>,
    /// el filtro de la estantería: 0 todo · 1 vídeo · 2 audio · 3 fotos
    filtro: u8,
    /// el desplazamiento vertical de la estantería (rueda sobre la columna)
    estante_scroll: f32,
    /// el desplazamiento horizontal de la BOBINA en píxeles (la barra)
    desplaza: f32,
    /// la LUPA CUENTAHÍLOS sobre el vidrio (mantener L): centro en uv
    lupa: Option<(f32, f32)>,
    /// la caja de selección elástica: (x0, y0) del lápiz
    caja: Option<(f32, f32)>,
    /// la lanzadera J/K/L: 0 parado · ±1 normal · ±2 rápido · ±4 turbo
    lanzadera: i32,
    /// el reloj de la lanzadera (el avance va por TIEMPO, no por frames)
    lanzadera_reloj: std::time::Instant,
    /// el portapapeles del taller (un clip con todo su cuarto oscuro)
    portapapeles: Option<proyecto::Clip>,
    /// historial de gestos (80 pasos) — no existe acción sin undo
    /// (fotografía de la bobina ENTERA: clips + pista de música)
    historia: Vec<Paso>,
    futuro: Vec<Paso>,
    gesto_previo: Option<(Vec<proyecto::Clip>, Vec<proyecto::ClipAudio>,
                          Vec<proyecto::Capa>, Vec<proyecto::Marca>)>,
    /// el último paso anotado está esperando su rótulo: el primer aviso que
    /// llegue lo bautiza (§4bis.7 — deshacer a ciegas)
    espera_rotulo: bool,
    mods: winit::keyboard::ModifiersState,
    cursor_puesto: winit::window::CursorIcon,
    /// monitor de FUENTE: cinta, marcas I/O y el punto de bobina al que volver
    fuente: Option<FuenteUi>,
    ultima_lata: (usize, std::time::Instant),
    hover_lata: Option<(usize, std::time::Instant)>,
    visor: visor::Visor,
    banco_h: f32,
    /// el compás de cada cinta, medido una vez (ritmo.rs)
    compases: std::collections::HashMap<String, ritmo::Compas>,
    /// cuánto ha crecido el banco por las pistas de capa visibles (px)
    extra_capas: f32,
    /// … y por el carril del pie (0 si la bobina no lleva subtítulos)
    extra_sub: f32,
    /// carriles de música visibles (los usados más uno libre; mínimo 3)
    musica_vis: usize,
    raton: (f32, f32),
    arrastrando: Arrastre,
    estanteria: Vec<proyecto::Cinta>,
    /// las baldas del registro (nombre, carpeta enchufada) — caché junto a la estantería
    proyecto_baldas: Vec<(String, Option<std::path::PathBuf>)>,
    dib_frames: u32,
    dib_desde: std::time::Instant,
    sel: Option<usize>,          // clip seleccionado de la bobina (primario)
    seleccion: std::collections::HashSet<usize>,  // multi-selección (shift+clic)
    pxs: f32,                    // lupa de la bobina
    aviso: (String, std::time::Instant),
    revelando: Option<std::process::Child>,
    /// la última bobina revelada (clic en el rótulo: mostrarla en el Finder)
    ultima_revelada: Option<std::path::PathBuf>,
    /// progreso del revelado (pct 0..1, paso) leído del stderr del CLI
    progreso: std::sync::Arc<std::sync::Mutex<(f32, String)>>,
    revelado_desde: std::time::Instant,
    /// cambio de sala en vuelo: (desde, hacia, cuándo) — el pliegue/apagón
    transicion: Option<(Sala, Sala, std::time::Instant)>,
    /// EL CUBO DE RECORTES: lo apartado, sin tope y rescatable arrastrando.
    /// No es una papelera: es la mesa auxiliar donde uno deja el final de un
    /// plano mientras hace hueco en la bobina para meterlo en otro sitio.
    recortes: Vec<proyecto::Clip>,
    /// cuánto se ha bajado dentro del cubo (es infinito, así que se recorre)
    cubo_scroll: f32,
    /// de dónde salió el arrastre que viene del cubo, y desde qué punto (para
    /// distinguir un CLIC —rescatar a la aguja— de un ARRASTRE —soltar donde
    /// se suelte)
    cubo_pinza: Option<(usize, f32, f32)>,
    /// LA LATA COGIDA DE LA ESTANTERÍA (1.1): índice en la estantería y el
    /// punto donde se pulsó. Igual que `cubo_pinza`: si el ratón apenas se
    /// movió fue un clic (fuente / doble toque) y si se arrastró, la cinta
    /// entra en la bobina DONDE SE SUELTE.
    lata_pinza: Option<(usize, f32, f32)>,
    /// LA MARCA DE LA CUCHILLA (1.3): la primera `B` la pone, la segunda corta
    /// por ella. Ver dónde cae el corte antes de darlo es la mitad del oficio.
    marca_corte: Option<f64>,
    /// LA PREVIEW A PANTALLA COMPLETA (no la app): sólo la imagen, sin mesa
    /// ni menús, con el teclado vivo. Doble clic en el vidrio entra y sale.
    visor_lleno: bool,
    /// la regla del rango de la sala de revelado: (x de origen, ancho). Se
    /// guarda al empezar a arrastrar para no recalcular la planta en cada
    /// movimiento del ratón.
    regla_rango: Option<(f32, f32)>,
    /// CUÁNTO DURA CADA FICHERO, memorizado. Hace falta para saber hasta
    /// dónde se puede ESTIRAR un recorte, y preguntarlo abre el contenedor:
    /// eso no se hace en cada movimiento del ratón.
    duraciones: std::collections::HashMap<std::path::PathBuf, f64>,
    /// EL MODO ENCUADRE sobre la imagen (`E`): el clip que se está encuadrando
    modo_encuadre: Option<usize>,
    /// el gesto de encuadre en curso: (encuadre al pulsar, punto fijo en uv de
    /// lienzo, ratón al pulsar en uv de lienzo)
    enc_gesto: Option<(proyecto::Encuadre, (f32, f32), (f32, f32))>,
    /// el ENCUADRE CALCADO, para pegarlo en otro clip (§1.5 · C)
    encuadre_copiado: Option<proyecto::Encuadre>,
    /// escribiendo la nota de una marca: (índice, texto)
    marcando: Option<(usize, String)>,
    /// EL PIE: qué subtítulo está elegido y, si se está escribiendo, su texto
    sel_sub: Option<usize>,
    escribiendo_sub: Option<(usize, String)>,
    /// el oído en marcha (el shell transcribiendo) y adónde deja el .srt
    oyendo: Option<(std::process::Child, std::path::PathBuf)>,
    /// escribiendo un número a mano: (clip, campo, texto)
    tecleando: Option<(usize, u8, String)>,
    /// el rótulo que se está reescribiendo (§5): a qué clip sustituir
    retitulando: Option<usize>,
    /// renombrar / duplicar / borrar una bobina de la portada (§4bis.8)
    bobina_menu: Option<usize>,
    bobina_renombrando: Option<(String, String)>,
    /// el BUCLE del rango marcado (§4bis.2)
    bucle: bool,
    /// el aviso de recuperación tras un cierre inesperado (§4bis.6)
    rescate: Option<std::path::PathBuf>,
    /// LAS VENTANAS SECUNDARIAS abiertas (§3)
    cristales: Vec<Cristal>,
    /// el ratón DENTRO de la ventana secundaria que lo tenga
    raton_cristal: (f32, f32),
    /// una ventana pedida desde el menú: se abre en la siguiente vuelta del
    /// bucle, que es donde hay `ActiveEventLoop` para crearla
    ventana_pedida: Option<Ventana>,
    /// el acetato de guías sobre el vidrio (tercios + centro), tecla A
    acetato: bool,
    /// el preset del máster elegido en la sala de revelado
    preset_revelado: usize,
    /// EL CAJÓN DEL MÁSTER (tamaño, supermuestreo, códec, caudal, filtro) y
    /// si está abierto. El botón de siempre no lo necesita: con los valores
    /// por defecto el revelado es exactamente el de antes.
    master: prefs::Master,
    cajon_master: bool,
    /// QUÉ SECCIONES del panel de instrumentos están abiertas.
    ///
    /// El tamaño sale de `GRUPOS`, no de un número escrito a mano. Estaba
    /// clavado a 6, y al añadir la sección del filtro ND pasaron a ser siete:
    /// `secciones[6]` se salía del array y **la app se cerraba en el acto al
    /// entrar en el cuarto oscuro**. Un array paralelo a una tabla tiene que
    /// medir lo que mida la tabla, y punto.
    secciones: [bool; GRUPOS.len()],
    /// el cajón de gelatinas, abierto o no
    cajon: bool,
    /// la receta calcada (prefs + gelatinas), prendida con chincheta
    receta: Option<(serde_json::Value, Option<std::path::PathBuf>, Option<std::path::PathBuf>)>,
    /// la cola de latas esperando revelado (presets pendientes)
    cola_revelado: Vec<usize>,
    /// dónde va el máster (None = la carpeta out/ del taller)
    destino: Option<std::path::PathBuf>,
    /// LA ETIQUETA DE LA LATA: el nombre del máster (None = el de la bobina)
    etiqueta: Option<String>,
    /// la etiqueta en edición (clic en la etiqueta la abre)
    etiquetando: bool,
    /// LA PARED del autor: fotos pegadas con celo (base/pared/*)
    pared: Vec<ui::Estampa>,
    /// el archivador: última copia de seguridad rotatoria
    ultima_copia: std::time::Instant,
    /// la persiana del menú que está abierta
    menu_abierto: Option<usize>,
    /// cuándo se guardó por última vez y si hay cambios sin guardar. Esto es
    /// media transparencia del proyecto: saber si lo que ves está en disco.
    guardado_en: std::time::Instant,
    sucio: bool,
    /// cuándo se estampó el último sello REVELADA (para la animación)
    sello_en: Option<std::time::Instant>,
}

/// UN PASO DEL HISTORIAL, con su nombre. Antes era una tupla anónima y ⌘Z
/// era a ciegas: ahora el menú Editar y el aviso dicen QUÉ se va a deshacer
/// (§4bis.7).
#[derive(Clone)]
struct Paso {
    clips: Vec<proyecto::Clip>,
    audio: Vec<proyecto::ClipAudio>,
    capas: Vec<proyecto::Capa>,
    marcas: Vec<proyecto::Marca>,
    subs: Vec<subtitulo::Sub>,
    que: String,
}

#[derive(PartialEq)]
enum Arrastre {
    Nada, Aguja, Mando(usize), Mando48(usize, usize), ClipMueve(usize),
    TrimI(usize), TrimD(usize),
    MusicaMueve(usize), MusicaTrimI(usize), MusicaTrimD(usize), MusicaPunto(usize, usize),
    MusicaGain(usize),
    /// mover el encuadre del clip arrastrando la imagen
    Encuadre(usize),
    /// un TIRADOR del encuadre: esquinas 0..3, bordes 4..7, ancla 8, giro 9
    EncTirador(usize, u8),
    /// arrastrar un NÚMERO de la ficha del encuadre (§1.5 · B)
    FilaEnc(usize, u8),
    /// los mandos de nivel del margen: 0 la voz, 1 la música (§1.6)
    Volumen(u8),
    /// las banderas del rango de la bobina (§4bis.2): 0 entrada, 1 salida
    Rango(u8),
    /// el mismo rango, pero desde la regla de la sala de revelado
    RangoSala(u8),
    /// la capa: mover y recortar (CAPAS §7)
    CapaMueve(usize),
    CapaTrimI(usize),
    CapaTrimD(usize),
    /// colocar el PiP: alt-arrastre en el visor con una capa elegida
    CapaEncuadre(usize),
    /// EL PIE: mover el subtítulo y estirarlo por los bordes
    SubMueve(usize), SubTrimI(usize), SubTrimD(usize),
    Manivela, Barra, Caja,
}

/// LOS CAMPOS DEL ENCUADRE que se arrastran en la ficha, por orden de fila
mod campo {
    pub const ESCALA_X: u8 = 0;
    pub const ESCALA_Y: u8 = 1;
    pub const POS_X: u8 = 2;
    pub const POS_Y: u8 = 3;
    pub const GIRO: u8 = 4;
    pub const ANCLA_X: u8 = 5;
    pub const ANCLA_Y: u8 = 6;
}

/// LOS SELLOS DE LA SALA. Los dos primeros son **los caminos de la casa**: al
/// lienzo de la bobina, sin escalar y con el motor del chip. No dependen de
/// nada: por mucho que se toque el cajón, tocar REVELAR revela exactamente lo
/// mismo que ayer y a la misma velocidad.
///
/// El tercero, A MANO, es el único que mira el cajón. Están separados a
/// propósito: si el ajuste raro y el camino rápido comparten botón, un día se
/// queda puesto un 8K ×2 y el botón de siempre tarda diez veces más sin haber
/// avisado. Se elige el sello, y el sello dice lo que va a pasar.
///
/// (nombre, qué es, códec — vacío = «lo que diga el cajón»)
#[cfg(target_os = "macos")]
const PRESETS_REVELADO: &[(&str, &str, &str)] = &[
    ("REVELAR", "hevc 10 bits · el motor del chip · LO MÁS RÁPIDO", "hevc"),
    ("ARCHIVO", "prores hq · dos motores en paralelo", "prores422hq"),
    ("EN CLIPS", "un fichero por plano · para montarlo en otro sitio", "hevc"),
    ("A MANO", "lo que diga el cajón · puede tardar mucho más", ""),
];
/// en Windows ProRes no tiene motor de hardware (iría por software, diez veces
/// más lento): no se le pone sello. Sigue estando en EL CAJÓN, que es donde
/// vive lo que cuesta tiempo y el autor decide si le compensa.
#[cfg(not(target_os = "macos"))]
const PRESETS_REVELADO: &[(&str, &str, &str)] = &[
    ("REVELAR", "hevc 10 bits · el motor de la Radeon · LO MÁS RÁPIDO", "hevc"),
    ("EN CLIPS", "un fichero por plano · para montarlo en otro sitio", "hevc"),
    ("A MANO", "lo que diga el cajón · puede tardar mucho más", ""),
];

/// ¿cuál es el sello de «a mano»? (el único que mira el cajón)
const A_MANO: usize = PRESETS_REVELADO.len() - 1;

/// EL TALLER COMO LABORATORIO: no sale una bobina, sale una CARPETA con un
/// fichero por plano, ya revelado. El montaje lo pone otro programa —de eso
/// se trata— así que aquí no se pega nada.
const EN_CLIPS: usize = PRESETS_REVELADO.len() - 2;

/// DEFAULT_PREFS de studio/js/state.js — «saorín · revelado», el baño de la casa
fn prefs_de_la_casa() -> serde_json::Value {
    serde_json::from_str(r#"{
        "gain": 0.1, "pushPull": 0, "compImpact": 1.35, "compWP": 1.35, "compRange": 0.36,
        "shutter": 0.143, "grain": 0.13, "grainSize": 5, "grainRough": 0.47, "grainChroma": 0,
        "grainDefocus": 0.3, "grainShadows": 0.7, "grainMids": 1, "grainHighs": 0.61,
        "grainRed": 1.35, "grainBlue": 1.3, "filmRes": 1,
        "halation": 1.5, "halHue": 1, "halSat": 0.9, "halThr": 0.8, "halSpread": 0.6, "halWhite": 0.1,
        "bloom": 0.6, "bloomThr": 0.8, "bloomWarm": 0.3,
        "softness": 0.1, "acutance": 0.11, "colorSep": 0.03,
        "hueSkew": 0.96, "crosstalk": 1, "subtractive": 1, "stockSat": 1.15, "print": 0.06,
        "vignette": 0, "vigSize": 0.55, "vigRound": 1, "vigCX": 0.5, "vigCY": 0.5,
        "chroma": 0, "weave": 0.15, "weaveRot": 0.3, "flicker": 0, "breath": 0, "breathRate": 0.5,
        "dust": 0, "frameInset": 0, "frameCorner": 40, "frameWobble": 1,
        "inputLutOn": true, "lutOn": true, "wipe": 1
    }"#).expect("prefs de la casa")
}

/// LOS BAÑOS (studio/js/presets.js): nombre + diferencias sobre la casa
const BANOS: [(&str, &str); 5] = [
    ("saorín · revelado", "{}"),
    ("La Chimera · S16", r#"{"grain":0.45,"grainSize":3.4,"grainRough":0.5,"grainChroma":0.35,"grainDefocus":0.3,"grainShadows":0.7,"grainMids":1.0,"grainHighs":0.6,"grainBlue":1.3,"filmRes":0.7,"halation":0.25,"halHue":1.0,"halSat":0.9,"halThr":0.8,"halSpread":0.6,"halWhite":0.1,"bloom":0.2,"bloomThr":0.8,"bloomWarm":0.3,"softness":0.1,"vignette":0.2,"weave":0.15,"chroma":0}"#),
    ("La Chimera · Bolex", r#"{"grain":0.6,"grainSize":4.5,"grainRough":0.55,"grainChroma":0.35,"grainDefocus":0.4,"grainShadows":0.8,"grainMids":1.1,"grainHighs":0.6,"grainBlue":1.3,"filmRes":0.9,"halation":0.3,"halThr":0.75,"halSpread":0.6,"halWhite":0.1,"bloom":0.25,"bloomWarm":0.35,"softness":0.4,"vignette":0.35,"weave":0.5,"flicker":0.3}"#),
    ("CineStill 800T", r#"{"grain":0.35,"grainSize":3.0,"grainRough":0.4,"filmRes":0.5,"halation":1.2,"halHue":1.0,"halSat":1.0,"halThr":0.4,"halSpread":1.0,"halWhite":0.0,"bloom":0.5,"bloomThr":0.65,"bloomWarm":0.4,"softness":0.2,"vignette":0.3}"#),
    ("FX off", r#"{"grain":0,"halation":0,"bloom":0,"softness":0,"vignette":0,"weave":0,"flicker":0,"dust":0,"chroma":0,"shutter":0}"#),
];

/// LOS STOCKS: capas parciales sobre el baño activo
const STOCKS: [(&str, &str); 5] = [
    ("KODAK 50D", r#"{"hueSkew":1.2,"crosstalk":0.35,"subtractive":0.8,"stockSat":1.2,"print":0.7,"compImpact":0.3}"#),
    ("KODAK 250D", r#"{"hueSkew":1.0,"crosstalk":0.3,"subtractive":0.7,"stockSat":1.0,"print":0.6,"compImpact":0.15}"#),
    ("KODAK 500T", r#"{"hueSkew":1.1,"crosstalk":0.35,"subtractive":0.65,"stockSat":0.95,"print":0.6,"compImpact":0.2,"halation":0.6}"#),
    ("FUJI ETERNA", r#"{"hueSkew":0.8,"crosstalk":0.25,"subtractive":0.5,"stockSat":0.85,"print":0.4,"compImpact":0.4}"#),
    ("CINESTILL 800", r#"{"hueSkew":1.0,"crosstalk":0.3,"subtractive":0.65,"stockSat":0.95,"print":0.6,"halation":1.2,"halSpread":1.0,"halThr":0.4}"#),
];

/// EL PANEL DE INSTRUMENTOS (studio/js/darkroom.js): las agujas del taller.
/// `secciones` mide lo que mida esta tabla — ver el comentario de ese campo.
const GRUPOS: [(&str, &[(&str, &str, f32, f32)]); 7] = [
    ("EL REVELADO", &[
        ("gain", "exposición", -2.0, 2.0),
        ("pushPull", "push / pull", -2.0, 2.0),
        ("compImpact", "compresión", 0.0, 3.0),
        ("compRange", "rango", 0.0, 1.0),
        // CUÁNTA LUZ DE MÁS entra en el hombro: 1,0 = ninguna y el hombro no
        // hace nada. Con 1,6 se recoge un stop y medio de superblanco y se
        // deja en blanco limpio.
        ("compWP", "margen del hombro", 1.0, 3.0),
        ("shutter", "obturador", 0.0, 0.9),
    ]),
    // ── EL FILTRO ND ────────────────────────────────────────────────────
    // Un ND no es gris del todo. El variable tiñe de magenta a toda la
    // escala; el fijo deja pasar infrarrojo, que el canal rojo del sensor
    // recoge, y como eso es ADITIVO se come los negros: granates y telas
    // marrones. La cura de lo segundo va pesada a sombras y protegida por
    // saturación, para quitar el rojo QUE SOBRA sin tocar el que hay.
    ("EL FILTRO ND", &[
        ("ndFix", "quitar el infrarrojo", 0.0, 1.5),
        ("ndTint", "tinte del ND (magenta ⇄ verde)", -1.0, 1.0),
        ("ndShadow", "hasta dónde sube", 0.2, 8.0),
        ("ndGuard", "cuánto respeta el color", 0.02, 1.0),
    ]),
    ("EL COLOR DEL STOCK", &[
        ("hueSkew", "deriva de tono", 0.0, 1.5),
        ("crosstalk", "contagio de capas", 0.0, 1.5),
        ("subtractive", "color subtractivo", 0.0, 1.0),
        ("stockSat", "saturación", 0.0, 2.0),
        ("print", "positivado 2383", 0.0, 1.0),
    ]),
    ("LA HALACIÓN", &[
        ("halation", "halación", 0.0, 3.0),
        ("halThr", "umbral", 0.0, 1.0),
        ("halSpread", "extensión", 0.0, 1.0),
        ("halHue", "tono", 0.0, 2.0),
        ("halWhite", "blanqueo", 0.0, 1.0),
        ("bloom", "velo (bloom)", 0.0, 2.0),
        ("bloomThr", "umbral del velo", 0.0, 1.0),
        ("bloomWarm", "calidez del velo", 0.0, 1.0),
    ]),
    ("EL GRANO", &[
        ("grain", "cantidad", 0.0, 1.0),
        ("grainSize", "tamaño", 0.4, 8.0),
        ("grainRough", "aspereza", 0.0, 1.0),
        ("grainChroma", "croma", 0.0, 1.0),
        ("grainDefocus", "desenfoque", 0.0, 1.0),
        ("grainShadows", "en sombras", 0.0, 2.0),
        ("grainMids", "en medios", 0.0, 2.0),
        ("grainHighs", "en altas", 0.0, 2.0),
        ("filmRes", "resolución", 0.0, 1.0),
    ]),
    ("LA ÓPTICA", &[
        ("softness", "suavidad", 0.0, 1.0),
        ("acutance", "acutancia", 0.0, 1.0),
        ("chroma", "aberración", 0.0, 1.0),
        ("vignette", "viñeta", 0.0, 1.0),
        ("vigSize", "tamaño viñeta", 0.1, 1.0),
    ]),
    ("LA MECÁNICA", &[
        ("weave", "gate weave", 0.0, 1.0),
        ("flicker", "parpadeo", 0.0, 1.0),
        ("breath", "respiración", 0.0, 1.0),
        ("dust", "polvo y rayas", 0.0, 1.0),
        ("frameInset", "ventanilla", 0.0, 120.0),
    ]),
];

/// los mandos del cuarto oscuro que se enseñan (el resto vive en el JSON)
const MANDOS: [(&str, &str, f32, f32); 8] = [
    ("grain", "grano", 0.0, 0.6),
    ("halation", "halación", 0.0, 3.0),
    ("bloom", "bloom", 0.0, 2.0),
    ("vignette", "viñeta", 0.0, 1.0),
    ("stockSat", "saturación", 0.0, 2.0),
    ("print", "positivado", 0.0, 1.0),
    ("shutter", "obturador", 0.0, 0.6),
    ("weave", "vaivén", 0.0, 1.0),
];

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.estado.is_some() {
            return;
        }
        // LA VENTANA. El taller dibuja su propia cabecera, así que la barra
        // del sistema sobra: en el Mac se vuelve transparente y el contenido
        // sube hasta el borde (los semáforos quedan flotando sobre el papel,
        // que es lo que hacen las aplicaciones de edición de verdad).
        let mut attrs = Window::default_attributes()
            .with_title("LABORATORIOS SAORÍN")
            .with_window_icon(icono_del_taller())
            .with_inner_size(winit::dpi::LogicalSize::new(1500.0, 940.0));
        #[cfg(target_os = "macos")]
        {
            use winit::platform::macos::WindowAttributesExtMacOS;
            attrs = attrs
                .with_titlebar_transparent(true)
                .with_fullsize_content_view(true)
                .with_title_hidden(true);
        }
        // ── EN WINDOWS, SIN MARCO DEL SISTEMA ────────────────────────────
        // El Mac vuelve transparente la barra de título y los semáforos
        // flotan sobre el papel. Windows no tiene nada equivalente: o marco
        // del sistema —una franja blanca con el icono por defecto a la
        // izquierda y los botones a la derecha, que parte el papel por la
        // mitad— o ninguno. Aquí no hay ninguno: los tres mandos los dibuja
        // el taller en su propia barra, con su misma tinta.
        //
        // `with_undecorated_shadow` conserva la sombra y el borde que hacen
        // que la ventana siga pareciendo una ventana (sin él, un rectángulo
        // plano pegado al escritorio), y las esquinas redondas son las de
        // Windows 11.
        #[cfg(target_os = "windows")]
        {
            use winit::platform::windows::{WindowAttributesExtWindows, CornerPreference};
            attrs = attrs
                .with_decorations(false)
                .with_undecorated_shadow(true)
                .with_corner_preference(CornerPreference::Round)
                .with_taskbar_icon(icono_del_taller());
        }
        // DÓNDE ESTABA LA VENTANA la última vez (§5): se abría siempre a
        // 1500×940 en el centro, dieras el tamaño que le dieras
        let guardada = prefs::geometria(&self.proyecto.base, "taller");
        let (x, y, w, h) = encaja_en_pantalla(el.primary_monitor(),
                                              guardada.unwrap_or((f64::NAN, f64::NAN, 1500.0, 940.0)));
        attrs = attrs.with_inner_size(winit::dpi::LogicalSize::new(w, h));
        if guardada.is_some() {
            attrs = attrs.with_position(winit::dpi::LogicalPosition::new(x, y));
        }
        let ventana = Arc::new(el.create_window(attrs).expect("ventana"));
        let gpu = pollster::block_on(ui::Gpu::new(ventana.clone())).expect("gpu");
        let lienzo = ui::Lienzo::new(&gpu);
        let tipos = ui::Tipos::new(&gpu);
        let lienzo2 = ui::Lienzo::new(&gpu);
        let tipos2 = ui::Tipos::new(&gpu);
        let lienzo3 = ui::Lienzo::new(&gpu);
        let tipos3 = ui::Tipos::new(&gpu);
        let atlas = ui::Atlas::new(&gpu);
        let minis = miniaturas::Miniaturas::nueva();
        let ondas = ondas::Ondas::nuevas();
        // hueso liso + grano del zine tileado encima (paper.png no tilea limpio)
        let papel = ui::Estampa::new(&gpu, include_bytes!("../assets/grain.png"), true);
        let tape = ui::Estampa::new(&gpu, include_bytes!("../assets/splice_tape.png"), false);
        let mut fondo = papel::Papel::new(&gpu);
        fondo.siembra(&self.proyecto.nombre);
        let objetos = ui::Estampa::new(&gpu, include_bytes!("../assets/doodles.png"), false);
        // LA PARED del autor: hasta 3 fotos pegadas (base/pared/*.jpg|png)
        let pared: Vec<ui::Estampa> = {
            let mut fotos: Vec<std::path::PathBuf> = std::fs::read_dir(self.proyecto.base.join("pared"))
                .ok().map(|rd| rd.flatten().map(|e| e.path())
                    .filter(|p| p.extension().and_then(|x| x.to_str())
                        .map(|x| ["jpg", "jpeg", "png"].contains(&x.to_lowercase().as_str()))
                        .unwrap_or(false))
                    .collect()).unwrap_or_default();
            fotos.sort();
            fotos.iter().take(3)
                .filter_map(|f| std::fs::read(f).ok())
                .map(|b| ui::Estampa::new(&gpu, &b, false))
                .collect()
        };
        let bobinas = proyecto::bobinas(&self.proyecto.base);
        let mut visor = visor::Visor::new(&gpu, &self.proyecto).expect("visor");
        // el primer fotograma, YA: nada de vidrio vacío al abrir el taller
        visor.busca(&self.proyecto, 0.0);
        // y en los ratos muertos, TODOS los decoders calientes (proxies y másters)
        visor.precalienta(&self.proyecto);
        let estanteria = self.proyecto.estanteria();
        let proyecto_baldas = self.proyecto.baldas();
        self.estado = Some(Estado {
            ventana,
            taller: self.proyecto.base.clone(),
            gpu, lienzo, tipos, lienzo2, tipos2, lienzo3, tipos3, atlas, minis, ondas, papel, tape,
            fondo, objetos,
            // POR DÓNDE EMPIEZA. Normalmente la portada, pero `FL_SALA` deja
            // arrancar directamente en una sala: es la única forma de probar
            // sin ratón que una sala **se dibuja sin cerrarse**, y hace falta
            // — el cuarto oscuro se cerraba en el acto por un array paralelo
            // que se había quedado corto, y eso se habría visto en un segundo.
            sala: match std::env::var("FL_SALA").as_deref() {
                Ok("mesa") => Sala::Mesa,
                Ok("cuarto") => Sala::CuartoOscuro,
                Ok("revelado") => Sala::Revelado,
                _ => Sala::Portada,
            },
            bobinas,
            nueva: None,
            chuleta: false,
            sel_audio: None,
            sel_capa: None,
            ajustes: false,
            revelar: None,
            titulando: None,
            renombrando: None,
            notando: None,
            baldas_cerradas: std::collections::HashSet::new(),
            filtro: 0,
            estante_scroll: 0.0,
            desplaza: 0.0,
            lupa: None,
            caja: None,
            lanzadera: 0,
            lanzadera_reloj: std::time::Instant::now(),
            portapapeles: None,
            historia: Vec::new(),
            futuro: Vec::new(),
            gesto_previo: None,
            espera_rotulo: false,
            mods: winit::keyboard::ModifiersState::empty(),
            cursor_puesto: winit::window::CursorIcon::Default,
            fuente: None,
            ultima_lata: (usize::MAX, std::time::Instant::now()),
            hover_lata: None,
            visor,
            banco_h: 250.0,
            compases: std::collections::HashMap::new(),
            extra_capas: 0.0,
            extra_sub: 0.0,
            musica_vis: 3,
            raton: (0.0, 0.0),
            arrastrando: Arrastre::Nada,
            estanteria,
            proyecto_baldas,
            dib_frames: 0,
            dib_desde: std::time::Instant::now(),
            sel: None,
            seleccion: std::collections::HashSet::new(),
            pxs: 26.0,
            aviso: (String::new(), std::time::Instant::now()),
            revelando: None,
            ultima_revelada: None,
            progreso: std::sync::Arc::new(std::sync::Mutex::new((0.0, String::new()))),
            revelado_desde: std::time::Instant::now(),
            transicion: None,
            recortes: Vec::new(),
            cubo_scroll: 0.0,
            cubo_pinza: None,
            lata_pinza: None,
            marca_corte: None,
            visor_lleno: false,
            regla_rango: None,
            duraciones: std::collections::HashMap::new(),
            modo_encuadre: None,
            enc_gesto: None,
            encuadre_copiado: None,
            marcando: None,
            sel_sub: None,
            escribiendo_sub: None,
            oyendo: None,
            tecleando: None,
            retitulando: None,
            bobina_menu: None,
            bobina_renombrando: None,
            bucle: false,
            rescate: None,
            cristales: Vec::new(),
            raton_cristal: (0.0, 0.0),
            ventana_pedida: None,
            acetato: false,
            preset_revelado: 0,
            master: prefs::master_guardado(&self.proyecto.base),
            cajon_master: false,
            secciones: {
                // abiertas: el revelado y el color del stock (lo que uno toca
                // primero); el resto plegado para que quepa
                let mut v = [false; GRUPOS.len()];
                v[0] = true;
                if GRUPOS.len() > 2 { v[2] = true; }
                v
            },
            cajon: false,
            receta: None,
            cola_revelado: Vec::new(),
            destino: prefs::destino_guardado(&self.proyecto.base),
            etiqueta: None,
            etiquetando: false,
            pared,
            ultima_copia: std::time::Instant::now(),
            menu_abierto: None,
            guardado_en: std::time::Instant::now(),
            sucio: false,
            sello_en: None,
        });
        // ¿la última vez se cerró sin guardar del todo? entonces se ofrece la
        // copia más reciente del archivador (§4bis.6)
        if self.cierre_brusco {
            let carpeta = self.proyecto.base.join("backups");
            let ultima = std::fs::read_dir(&carpeta).ok().and_then(|rd| {
                rd.flatten().map(|e| e.path())
                  .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
                  .max_by_key(|p| p.metadata().and_then(|m| m.modified()).ok())
            });
            if let (Some(e), Some(u)) = (self.estado.as_mut(), ultima) {
                e.rescate = Some(u);
                e.di("la última vez el taller se cerró de golpe · ¿recupero la copia?");
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, ev: WindowEvent) {
        let Some(e) = self.estado.as_mut() else { return };
        // ── ¿ES DE UNA VENTANA SECUNDARIA? (§3 · 3) ─────────────────────
        // `ApplicationHandler` ya trae el WindowId; hasta ahora se ignoraba
        // porque solo había una ventana.
        if id != e.ventana.id() {
            let Some(k) = e.cristales.iter().position(|c| c.ventana.id() == id) else { return };
            match ev {
                // CERRAR UNA SECUNDARIA NO CIERRA EL TALLER (§3 · 4)
                WindowEvent::CloseRequested => e.cierra_cristal(k),
                WindowEvent::Resized(t) => e.cristales[k].gpu.redimensiona(t.width, t.height),
                WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                    e.cristales[k].gpu.pon_escala(scale_factor as f32);
                }
                WindowEvent::RedrawRequested => e.pinta_cristal(k, &self.proyecto),
                WindowEvent::KeyboardInput { event, .. } => {
                    // esc cierra la ventana que la reciba
                    if event.state == ElementState::Pressed
                        && event.physical_key == PhysicalKey::Code(KeyCode::Escape) {
                        e.cierra_cristal(k);
                    }
                }
                WindowEvent::CursorMoved { position, .. } => {
                    let f = e.cristales[k].gpu.escala;
                    e.raton_cristal = (position.x as f32 / f, position.y as f32 / f);
                }
                WindowEvent::MouseInput { state, .. } if state == ElementState::Pressed => {
                    let (aw, ah) = e.cristales[k].gpu.alto_ancho();
                    let (mx, my) = e.raton_cristal;
                    // el marco que dibujamos nosotros manda primero
                    if let Some(dir) = borde_en(aw, ah, mx, my) {
                        let _ = e.cristales[k].ventana.drag_resize_window(dir);
                        return;
                    }
                    if menu::cierra_cristal_en(aw, mx, my) { e.cierra_cristal(k); return; }
                    if my < menu::cabecera_cristal() {
                        let _ = e.cristales[k].ventana.drag_window();
                        return;
                    }
                    let cab = menu::cabecera_cristal();
                    match e.cristales[k].que {
                        Ventana::Vigia => e.visor.play_pausa(&self.proyecto),
                        Ventana::Ajustes => {
                            // la MISMA geometría que la del panel, sin el modal
                            let fila = ((my - cab - Estado::AJUSTES_Y0 + 6.0)
                                        / Estado::AJUSTES_FILA) as i32;
                            e.toca_ajuste(&mut self.proyecto, fila);
                        }
                        Ventana::Chuleta => {}
                        Ventana::Bobinas => {
                            let fila = ((my - cab - 46.0) / Estado::BOBINA_FILA) as i32;
                            let a = e.toca_bobina(&self.proyecto, fila);
                            drop(e);
                            self.aplica(a);
                            if let Some(e) = self.estado.as_mut() {
                                e.bobinas = proyecto::bobinas(&self.proyecto.base);
                            }
                            return;
                        }
                    }
                }
                _ => {}
            }
            return;
        }
        match ev {
            WindowEvent::CloseRequested => {
                // al cerrar, la ventana apunta dónde estaba y se apaga la luz
                e.guarda_geometria(&e.ventana.clone(), "taller");
                for c in &e.cristales { e.guarda_geometria(&c.ventana, c.que.clave()); }
                let _ = self.proyecto.guarda();
                quita_marca(&self.proyecto.base);
                el.exit();
            }
            WindowEvent::Resized(t) => e.gpu.redimensiona(t.width, t.height),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                // HiDPI y segundo monitor (§5): las coordenadas del taller son
                // lógicas, así que basta con que la escala esté al día
                e.gpu.pon_escala(scale_factor as f32);
                e.ventana.request_redraw();
            }
            WindowEvent::ModifiersChanged(m) => {
                e.mods = m.state();
                if !e.mods.alt_key() && e.lupa.is_some() {
                    e.lupa = None;
                    e.ventana.request_redraw();
                }
            }
            WindowEvent::DroppedFile(ruta) => {
                // material al taller arrastrándolo: POR REFERENCIA, cero copias.
                // Una CARPETA entera = una BALDA nueva enchufada a ella (§2bis)
                if ruta.is_dir() {
                    let mj = self.proyecto.media_json();
                    let (balda, n) = proyecto::importa_carpeta(&self.proyecto.base, &mj, &ruta);
                    e.estanteria = self.proyecto.estanteria();
                    e.proyecto_baldas = self.proyecto.baldas();
                    e.di(&format!("balda «{balda}»: {n} lata(s)"));
                    return;
                }
                let mj = self.proyecto.media_json();
                let nombre = ruta.file_name().map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let (n, saltados) = proyecto::importa_en(&self.proyecto.base, &mj, &[ruta]);
                if n > 0 || saltados > 0 {
                    e.estanteria = self.proyecto.estanteria();
                    e.proyecto_baldas = self.proyecto.baldas();
                }
                // ── SOLTARLO SOBRE LA BOBINA LO METE AHÍ (§4bis.5) ────────
                // Antes todo iba a la estantería y había que arrastrarlo otra
                // vez: si sueltas sobre el banco, es que quieres montarlo.
                let (mx, my) = e.raton;
                let en_bobina = e.sala == Sala::Mesa && mx > Estado::ESTANTE_W
                    && my > e.banco_y();
                let cinta = e.estanteria.iter().find(|c| c.nombre == nombre).cloned();
                match (en_bobina, cinta) {
                    (true, Some(c)) if c.fps < 0.0 => {
                        e.recuerda(&self.proyecto);
                        let dur = sonido::dur_de(&c.ruta).unwrap_or(30.0);
                        let start = e.tiempo_en(mx).max(0.0);
                        self.proyecto.audio.push(proyecto::ClipAudio {
                            media: c.nombre.clone(), ruta: c.ruta.clone(),
                            t_in: 0.0, t_out: dur, start, gain: 0.0,
                            fade_in: 0.0, fade_out: 0.0, banda: Vec::new(), mute: false,
                            pista: e.pista_en(my).unwrap_or(0), desfase: 0.0,
                        });
                        let _ = self.proyecto.guarda();
                        e.di(&format!("«{}» a la música en {start:.1} s", c.nombre));
                    }
                    (true, Some(c)) => {
                        e.recuerda(&self.proyecto);
                        let idx = e.junta_en(&self.proyecto, mx);
                        let nuevo = self.proyecto.clip_de(&c);
                        self.proyecto.clips.insert(idx, nuevo);
                        self.proyecto.cuantiza();
                        let _ = self.proyecto.guarda();
                        e.sel = Some(idx);
                        e.visor.foley(sonido::Foley::Lata);
                        e.visor.busca(&self.proyecto, e.visor.t);
                        e.di(&format!("«{}» a la bobina, donde lo soltaste", c.nombre));
                    }
                    _ if n > 0 => e.di(&format!("{n} cinta(s) a la estantería")),
                    _ if saltados > 0 => e.di("esa cinta ya estaba (o no es material)"),
                    _ => {}
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let f = e.ventana.scale_factor() as f32;
                let ant = e.raton;
                e.raton = (position.x as f32 / f, position.y as f32 / f);
                if e.sala == Sala::Portada {
                    e.pon_cursor(winit::window::CursorIcon::Default);
                    return;
                }
                if e.sala == Sala::Mesa {
                    e.cursor_mesa(&self.proyecto);
                } else {
                    e.pon_cursor(winit::window::CursorIcon::Default);
                }
                // hover de lata (para la hoja de contactos)
                // ⌥ sobre el vidrio = sostener la LUPA cuentahílos
                if e.sala == Sala::Mesa && e.mods.alt_key() && e.arrastrando == Arrastre::Nada {
                    let [gx, gy, gw, gh] = e.visor.rect_pantalla;
                    let (mx2, my2) = e.raton;
                    e.lupa = (mx2 >= gx && mx2 <= gx + gw && my2 >= gy && my2 <= gy + gh)
                        .then_some((mx2, my2));
                } else if e.lupa.is_some() {
                    e.lupa = None;
                }
                if e.arrastrando == Arrastre::Nada && e.sala == Sala::Mesa {
                    let (mx2, my2) = e.raton;
                    if let Some(fila) = e.lata_en(mx2, my2) {
                        match e.hover_lata {
                            Some((f, _)) if f == fila => {}
                            _ => e.hover_lata = Some((fila, std::time::Instant::now())),
                        }
                    } else {
                        e.hover_lata = None;
                    }
                }
                let dx = e.raton.0 - ant.0;
                match e.arrastrando {
                    Arrastre::Aguja => {
                        let t = if let Some(f) = &e.fuente {
                            e.t_fuente_en(e.raton.0, f.cinta.dur)
                        } else {
                            e.tiempo_en(e.raton.0)
                        };
                        e.visor.busca(&self.proyecto, t);
                        // SCRUB AUDIBLE: se oye lo que hay bajo la aguja
                        e.visor.chispa(&self.proyecto);
                    }
                    Arrastre::Mando(i) => {
                        let (k, _, lo, hi) = MANDOS[i];
                        let paso = (hi - lo) / 200.0 * dx;
                        e.cambia_mando(&mut self.proyecto, k, paso, lo, hi);
                    }
                    Arrastre::Mando48(gi, ri) => {
                        let (k, _, lo, hi) = GRUPOS[gi].1[ri];
                        let paso = (hi - lo) / 220.0 * dx;
                        e.cambia_mando(&mut self.proyecto, k, paso, lo, hi);
                    }
                    // ── ESTIRAR PARA RECUPERAR ──────────────────────────
                    // Un recorte no es un corte: lo que se quitó sigue en el
                    // fichero y tiene que poder volver. El tope de arriba es
                    // el FINAL DEL MATERIAL, no el sitio donde soltaste el
                    // tirador la última vez.
                    Arrastre::TrimD(i) => {
                        let d = (dx / e.pxs) as f64;
                        let tope = self.proyecto.clips.get(i)
                            .map(|c| e.dur_fuente(&c.ruta)).unwrap_or(0.0);
                        if let Some(c) = self.proyecto.clips.get_mut(i) {
                            let hasta = if tope > 0.01 { tope } else { f64::MAX };
                            c.t_out = (c.t_out + d).clamp(c.t_in + 0.1, hasta);
                        }
                    }
                    Arrastre::TrimI(i) => {
                        let d = (dx / e.pxs) as f64;
                        if let Some(c) = self.proyecto.clips.get_mut(i) {
                            c.t_in = (c.t_in + d).clamp(0.0, c.t_out - 0.1);
                        }
                    }
                    Arrastre::MusicaMueve(i) => {
                        let d = (dx / e.pxs) as f64;
                        // EL IMÁN EN LA MÚSICA: la canción se pega al
                        // principio o al final de un plano, o a una marca —
                        // que es donde uno quiere que entre. Se prueban los
                        // DOS bordes de la pista: pegar por el final es tan
                        // frecuente como pegar por el principio.
                        let radio = 10.0 / e.pxs as f64;
                        let iman = prefs::IMAN.load(std::sync::atomic::Ordering::Relaxed);
                        let puntos = if iman { self.proyecto.imanes() } else { Vec::new() };
                        if let Some(a) = self.proyecto.audio.get_mut(i) {
                            let mut nuevo = (a.start + d).max(0.0);
                            if iman {
                                let dur = a.dur();
                                // dos arranques posibles por cada imán: uno
                                // pega el PRINCIPIO de la canción al punto y
                                // el otro pega su FINAL
                                let mut cand: Vec<f64> = Vec::with_capacity(puntos.len() * 2);
                                for x in &puntos { cand.push(*x); cand.push(x - dur); }
                                cand.retain(|c| *c >= -0.001 && (c - nuevo).abs() <= radio);
                                if let Some(x) = cand.into_iter().min_by(|p, q| {
                                    (p - nuevo).abs().partial_cmp(&(q - nuevo).abs())
                                        .unwrap_or(std::cmp::Ordering::Equal)
                                }) { nuevo = x.max(0.0); }
                            }
                            a.start = nuevo;
                        }
                    }
                    // ARRASTRAR DENTRO = MOVER. El encuadre se toca en
                    // fracción de LIENZO, que es como lo guarda el modelo:
                    // se arrastra la imagen y va con la mano, sin traducir.
                    Arrastre::Encuadre(i) => {
                        let [_, _, gw, gh] = e.visor.rect_pantalla;
                        let dy2 = e.raton.1 - ant.1;
                        let recto = e.mods.shift_key();
                        if let Some(c) = self.proyecto.clips.get_mut(i) {
                            let (mx0, my0) = (dx / gw.max(1.0), dy2 / gh.max(1.0));
                            // ⇧ mientras se mueve: por un solo eje
                            if recto && mx0.abs() < my0.abs() { c.enc.pos.1 += my0; }
                            else if recto { c.enc.pos.0 += mx0; }
                            else { c.enc.pos.0 += mx0; c.enc.pos.1 += my0; }
                            e.visor.marca_cuarto(i);
                        }
                    }
                    Arrastre::EncTirador(i, k) => {
                        e.tira_del_encuadre(&mut self.proyecto, i, k);
                    }
                    Arrastre::FilaEnc(i, campo) => {
                        let fino = if e.mods.shift_key() { 0.15 }
                                   else if e.mods.alt_key() { 4.0 } else { 1.0 };
                        e.arrastra_numero(&mut self.proyecto, i, campo, dx * fino);
                    }
                    Arrastre::Volumen(cual) => {
                        // −40 … +12 dB en 180 px de recorrido
                        let d = (dx as f64) * 52.0 / 180.0;
                        let v = if cual == 0 { &mut self.proyecto.vol_voz }
                                else { &mut self.proyecto.vol_musica };
                        *v = (*v + d).clamp(-40.0, 12.0);
                        e.visor.manda_mezcla(&self.proyecto);
                    }
                    Arrastre::MusicaGain(i) => {
                        let d = (dx as f64) * 52.0 / 180.0;
                        if let Some(a) = self.proyecto.audio.get_mut(i) {
                            a.gain = (a.gain + d).clamp(-40.0, 12.0);
                        }
                    }
                    Arrastre::RangoSala(cual) => {
                        // la regla dibuja la bobina ENTERA de punta a punta:
                        // la x se traduce por proporción, no por la escala de
                        // la mesa (aquí no hay línea de tiempo)
                        let (bx3, bw3) = e.regla_rango.unwrap_or((0.0, 1.0));
                        let dur = self.proyecto.duracion().max(0.001);
                        let f = ((e.raton.0 - bx3) / bw3.max(1.0)).clamp(0.0, 1.0) as f64;
                        let t = (f * dur).clamp(0.0, dur);
                        let (mut a, mut b) = self.proyecto.tramo();
                        if cual == 0 { a = t.min(b - 0.04); } else { b = t.max(a + 0.04); }
                        self.proyecto.rango = Some((a, b));
                    }
                    Arrastre::Rango(cual) => {
                        let t = e.tiempo_en(e.raton.0).clamp(0.0, self.proyecto.duracion());
                        let (mut a, mut b) = self.proyecto.tramo();
                        if cual == 0 { a = t.min(b - 0.04); } else { b = t.max(a + 0.04); }
                        self.proyecto.rango = Some((a, b));
                    }
                    Arrastre::MusicaPunto(i, k) => {
                        let dy = e.raton.1 - ant.1;
                        if let Some(a) = self.proyecto.audio.get_mut(i) {
                            if let Some(p) = a.banda.get_mut(k) {
                                // 24 px de pista ≈ 30 dB de recorrido
                                p.1 = (p.1 - dy as f64 * 30.0 / 24.0).clamp(-24.0, 6.0);
                            }
                        }
                    }
                    // ── EL PIE: mover y estirar ────────────────────────
                    Arrastre::SubMueve(i) => {
                        let d = (dx / e.pxs) as f64;
                        let radio = 10.0 / e.pxs as f64;
                        let iman = prefs::IMAN.load(std::sync::atomic::Ordering::Relaxed);
                        let puntos = if iman { self.proyecto.imanes() } else { Vec::new() };
                        if let Some(sb) = self.proyecto.subs.get_mut(i) {
                            let dur = sb.t1 - sb.t0;
                            let mut nuevo = (sb.t0 + d).max(0.0);
                            if iman {
                                // los dos bordes, como en la música
                                let mut cand: Vec<f64> = Vec::with_capacity(puntos.len() * 2);
                                for x in &puntos { cand.push(*x); cand.push(x - dur); }
                                cand.retain(|c| *c >= -0.001 && (c - nuevo).abs() <= radio);
                                if let Some(x) = cand.into_iter().min_by(|p, q| {
                                    (p - nuevo).abs().partial_cmp(&(q - nuevo).abs())
                                        .unwrap_or(std::cmp::Ordering::Equal)
                                }) { nuevo = x.max(0.0); }
                            }
                            sb.t0 = nuevo;
                            sb.t1 = nuevo + dur;
                        }
                    }
                    Arrastre::SubTrimI(i) => {
                        let d = (dx / e.pxs) as f64;
                        if let Some(sb) = self.proyecto.subs.get_mut(i) {
                            sb.t0 = (sb.t0 + d).clamp(0.0, sb.t1 - 0.15);
                        }
                    }
                    Arrastre::SubTrimD(i) => {
                        let d = (dx / e.pxs) as f64;
                        if let Some(sb) = self.proyecto.subs.get_mut(i) {
                            sb.t1 = (sb.t1 + d).max(sb.t0 + 0.15);
                        }
                    }
                    Arrastre::CapaMueve(i) => {
                        // arrastrarla a otro carril LA CAMBIA DE PISTA
                        if let Some(p) = e.pista_capa_en(e.raton.1) {
                            if let Some(cp) = self.proyecto.capas.get_mut(i) {
                                cp.pista = p;
                            }
                        }
                        let d = (dx / e.pxs) as f64;
                        let radio = 10.0 / e.pxs as f64;
                        let iman = prefs::IMAN.load(std::sync::atomic::Ordering::Relaxed);
                        let puntos = if iman { self.proyecto.imanes() } else { Vec::new() };
                        if let Some(cp) = self.proyecto.capas.get_mut(i) {
                            let mut nuevo = (cp.start + d).max(0.0);
                            if iman {
                                let dur = cp.dur();
                                let mut cand: Vec<f64> = Vec::with_capacity(puntos.len() * 2);
                                for x in &puntos { cand.push(*x); cand.push(x - dur); }
                                cand.retain(|c| *c >= -0.001 && (c - nuevo).abs() <= radio);
                                if let Some(x) = cand.into_iter().min_by(|p, q| {
                                    (p - nuevo).abs().partial_cmp(&(q - nuevo).abs())
                                        .unwrap_or(std::cmp::Ordering::Equal)
                                }) { nuevo = x.max(0.0); }
                            }
                            cp.start = nuevo;
                        }
                    }
                    Arrastre::CapaEncuadre(k) => {
                        let [_, _, gw, gh] = e.visor.rect_pantalla;
                        let dy2 = e.raton.1 - ant.1;
                        if let Some(cp) = self.proyecto.capas.get_mut(k) {
                            cp.c.enc.pos.0 += dx / gw.max(1.0);
                            cp.c.enc.pos.1 += dy2 / gh.max(1.0);
                        }
                    }
                    Arrastre::CapaTrimI(i) => {
                        let d = (dx / e.pxs) as f64;
                        if let Some(cp) = self.proyecto.capas.get_mut(i) {
                            let d = d.clamp(-(cp.start.min(cp.c.t_in)),
                                            cp.c.t_out - cp.c.t_in - 0.1);
                            cp.c.t_in += d;
                            cp.start += d;
                        }
                    }
                    Arrastre::CapaTrimD(i) => {
                        let d = (dx / e.pxs) as f64;
                        let tope = self.proyecto.capas.get(i)
                            .map(|cp| e.dur_fuente(&cp.c.ruta)).unwrap_or(0.0);
                        if let Some(cp) = self.proyecto.capas.get_mut(i) {
                            let hasta = if tope > 0.01 { tope } else { f64::MAX };
                            cp.c.t_out = (cp.c.t_out + d).clamp(cp.c.t_in + 0.1, hasta);
                        }
                    }
                    Arrastre::MusicaTrimI(i) => {
                        // recortar la cabeza: avanza t_in Y start a la vez, de
                        // modo que lo que ya sonaba siga cayendo en el mismo
                        // segundo de la bobina. Hacia atrás llega hasta el
                        // principio del fichero (o hasta el de la bobina).
                        let d = (dx / e.pxs) as f64;
                        if let Some(a) = self.proyecto.audio.get_mut(i) {
                            let d = d.clamp(-(a.start.min(a.t_in)), a.t_out - a.t_in - 0.1);
                            a.t_in += d;
                            a.start += d;
                        }
                    }
                    Arrastre::MusicaTrimD(i) => {
                        let d = (dx / e.pxs) as f64;
                        let tope = self.proyecto.audio.get(i)
                            .map(|a| e.dur_fuente(&a.ruta)).unwrap_or(0.0);
                        if let Some(a) = self.proyecto.audio.get_mut(i) {
                            let hasta = if tope > 0.01 { tope } else { f64::MAX };
                            a.t_out = (a.t_out + d).clamp(a.t_in + 0.1, hasta);
                        }
                    }
                    Arrastre::ClipMueve(i) => {
                        // sobre la barra lateral no se reordena: ahí lo que
                        // hay es el cubo, y el clip está de camino a él
                        if e.raton.0 < Estado::ESTANTE_W { return; }
                        // CON EL IMÁN, el clip se pega al vecino mientras se
                        // arrastra (lo de siempre). SIN ÉL, no se toca nada
                        // hasta soltar: el clip se queda DONDE SE SUELTE, y el
                        // espacio que quede se convierte en hueco (§1.4).
                        if !prefs::IMAN.load(std::sync::atomic::Ordering::Relaxed) { return; }
                        let t = e.tiempo_en(e.raton.0);
                        if let Some((j, _)) = self.proyecto.en(t) {
                            if j != i {
                                let c = self.proyecto.clips.remove(i);
                                self.proyecto.clips.insert(j, c);
                                e.sel = Some(j);
                                e.arrastrando = Arrastre::ClipMueve(j);
                            }
                        }
                    }
                    Arrastre::Caja => {
                        if let Some((x0, _)) = e.caja {
                            let (a, b) = (x0.min(e.raton.0), x0.max(e.raton.0));
                            let t0 = e.tiempo_en(a);
                            let t1 = e.tiempo_en(b);
                            e.seleccion.clear();
                            let mut acc = 0.0f64;
                            for (i, c) in self.proyecto.clips.iter().enumerate() {
                                let (ci, co) = (acc, acc + c.dur());
                                if co > t0 && ci < t1 { e.seleccion.insert(i); }
                                acc = co;
                            }
                            e.sel = e.seleccion.iter().min().copied();
                        }
                    }
                    Arrastre::Barra => {
                        let max = e.desplaza_max(&self.proyecto);
                        if max > 0.0 {
                            let (ancho, _) = e.gpu.alto_ancho();
                            let bw = ancho - Estado::ESTANTE_W - 36.0;
                            let total = self.proyecto.duracion().max(0.1) as f32 * e.pxs + 60.0;
                            let frac_w = (bw * (bw / total)).clamp(28.0, bw);
                            let paso = max / (bw - frac_w).max(1.0);
                            e.desplaza = (e.desplaza + dx * paso).clamp(0.0, max);
                        }
                    }
                    Arrastre::Manivela => {
                        let (mcx, mcy) = e.manivela_centro();
                        let a1 = (ant.1 - mcy).atan2(ant.0 - mcx);
                        let a2 = (e.raton.1 - mcy).atan2(e.raton.0 - mcx);
                        let mut da = a2 - a1;
                        while da > std::f32::consts::PI { da -= std::f32::consts::TAU; }
                        while da < -std::f32::consts::PI { da += std::f32::consts::TAU; }
                        // una vuelta de manivela = un segundo de película
                        let t = (e.visor.t + (da / std::f32::consts::TAU) as f64).max(0.0);
                        e.visor.busca(&self.proyecto, t);
                        e.visor.chispa(&self.proyecto);
                    }
                    Arrastre::Nada => {}
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Right, .. } => {
                if state != ElementState::Pressed { return; }
                // ── LAS BOBINAS SE MANEJAN DESDE LA PORTADA (§4bis.8) ────
                if e.sala == Sala::Portada {
                    let (mx, my) = e.raton;
                    for (i, (x, y, w, h)) in e.tarjetas().iter().enumerate() {
                        if i == 0 { continue; }   // la de «bobina nueva» no
                        if mx >= *x && mx <= x + w && my >= *y && my <= y + h {
                            e.bobina_menu = Some(i - 1);
                            return;
                        }
                    }
                    e.bobina_menu = None;
                    return;
                }
                if e.sala != Sala::Mesa { return; }
                let (mx, my) = e.raton;
                if let Some(balda) = e.balda_en(mx, my) {
                    // «volver a mirar»: el rescan de la carpeta enchufada
                    if let Some(carp) = e.proyecto_baldas.iter()
                        .find(|(b, _)| *b == balda).and_then(|(_, c)| c.clone()) {
                        let antes = e.estanteria.len();
                        let mj = self.proyecto.media_json();
                        let _ = proyecto::importa_carpeta_como(&self.proyecto.base, &mj, &carp,
                                                               Some(&balda));
                        e.estanteria = self.proyecto.estanteria();
                        e.proyecto_baldas = self.proyecto.baldas();
                        let nuevas = e.estanteria.len().saturating_sub(antes);
                        e.di(&if nuevas > 0 { format!("{nuevas} lata(s) nuevas en «{balda}»") }
                             else { format!("«{balda}» está al día") });
                    }
                    return;
                }
                if let Some(fila) = e.lata_en(mx, my) {
                    if let Some(c) = e.estanteria.get(fila).cloned() {
                        if e.mods.shift_key() {
                            // ⇧+clic derecho: quitar del registro (el fichero no se toca)
                            if proyecto::quita_cinta(&self.proyecto.base, &c.nombre) {
                                e.estanteria = self.proyecto.estanteria();
                e.proyecto_baldas = self.proyecto.baldas();
                                e.di(&format!("«{}» fuera de la estantería (el fichero queda)", c.nombre));
                            } else {
                                e.di("esa vive en media/ (nombre físico)");
                            }
                        } else {
                            e.renombrando = Some((c.nombre.clone(), c.nombre.clone()));
                        }
                    }
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                if state == ElementState::Pressed {
                    // ── LA BARRA DE MENÚ, POR ENCIMA DE TODAS LAS SALAS ────
                    // Va la primera a propósito: cada sala atiende sus propios
                    // clics y sale, así que si la barra no manda antes, en la
                    // portada no se abriría ninguna persiana.
                    let (mx, my) = e.raton;
                    // ── LA VENTANA SIN MARCO (Windows) ───────────────────
                    // Los mandos y el borde de redimensionar mandan sobre
                    // todo lo demás: son el marco, aunque lo dibujemos
                    // nosotros.
                    let (aw, ah) = e.gpu.alto_ancho();
                    if let Some(dir) = borde_en(aw, ah, mx, my) {
                        let _ = e.ventana.drag_resize_window(dir);
                        return;
                    }
                    if let Some(k) = menu::mando_en(aw, mx, my) {
                        match k {
                            0 => e.ventana.set_minimized(true),
                            1 => {
                                let m = e.ventana.is_maximized();
                                e.ventana.set_maximized(!m);
                            }
                            _ => {
                                e.guarda_geometria(&e.ventana.clone(), "taller");
                                let _ = self.proyecto.guarda();
                                quita_marca(&self.proyecto.base);
                                el.exit();
                            }
                        }
                        return;
                    }
                    if let Some(k) = e.menu_abierto {
                        if let Some(i) = menu::entrada_en(k, mx, my) {
                            let acc = menu::MENUS[k].entradas[i].accion;
                            e.menu_abierto = None;
                            if let Some(a) = acc { e.hace(&mut self.proyecto, a); }
                            return;
                        }
                        if my > menu::ALTO && menu::persiana_en(mx, my).is_none() {
                            e.menu_abierto = None;   // clic fuera: se cierra
                            return;
                        }
                    }
                    if let Some(k) = menu::persiana_en(mx, my) {
                        e.menu_abierto = if e.menu_abierto == Some(k) { None } else { Some(k) };
                        e.visor.foley(sonido::Foley::Tick);
                        return;
                    }
                    if my < menu::ALTO {
                        // DOBLE CLIC EN LA BARRA: maximizar y restaurar, que es
                        // lo que hace cualquier ventana del sistema
                        let doble = e.ultima_lata.0 == usize::MAX - 40
                            && e.ultima_lata.1.elapsed().as_secs_f64() < 0.4;
                        e.ultima_lata = (usize::MAX - 40, std::time::Instant::now());
                        if doble {
                            let m = e.ventana.is_maximized();
                            e.ventana.set_maximized(!m);
                            return;
                        }
                        // el resto de la barra: arrastrar la ventana, como una de verdad
                        let _ = e.ventana.drag_window();
                        return;
                    }
                }
                if e.sala == Sala::Portada {
                    if state == ElementState::Pressed {
                        let accion = e.pulsa_portada(&self.proyecto);
                        self.aplica(accion);
                    }
                    return;
                }
                if state == ElementState::Released {
                    let (mx, my) = e.raton;
                    // ── ¿UN CLIP QUE CAE EN EL CUBO? ─────────────────────
                    // Sacarlo de la bobina y guardarlo. Es el gesto que hace
                    // útil el cubo: aparto el final de un plano, hago hueco
                    // donde quiera, y luego lo traigo de vuelta.
                    // ── ¿A LA PAPELERA? ──────────────────────────────────
                    // Se mira ANTES que el cubo: la papelera vive dentro de
                    // su columna y si no, el cubo se lo quedaría todo.
                    if let Arrastre::ClipMueve(i) = e.arrastrando {
                        if e.en_la_papelera(mx, my) && i < self.proyecto.clips.len() {
                            e.recuerda(&self.proyecto);
                            let c = self.proyecto.clips.remove(i);
                            e.sel = None;
                            e.seleccion.clear();
                            self.proyecto.cuantiza();
                            let _ = self.proyecto.guarda();
                            e.arrastrando = Arrastre::Nada;
                            e.visor.foley(sonido::Foley::Corte);
                            e.visor.busca(&self.proyecto, e.visor.t);
                            e.di(&format!("«{}» a la papelera · ⌘Z lo devuelve",
                                          c.media.chars().take(24).collect::<String>()));
                            return;
                        }
                    }
                    if let Arrastre::ClipMueve(i) = e.arrastrando {
                        if e.en_el_cubo(mx, my) && i < self.proyecto.clips.len() {
                            e.recuerda(&self.proyecto);
                            let c = self.proyecto.clips.remove(i);
                            let dur = c.dur();
                            e.recortes.push(c);
                            e.cubo_scroll = 0.0;      // lo nuevo, arriba y a la vista
                            e.sel = None;
                            e.seleccion.clear();
                            self.proyecto.cuantiza();
                            let _ = self.proyecto.guarda();
                            e.arrastrando = Arrastre::Nada;
                            e.visor.foley(sonido::Foley::Lata);
                            e.visor.busca(&self.proyecto, e.visor.t);
                            e.di(&format!("al cubo: {dur:.1} s guardados ({} dentro)",
                                          e.recortes.len()));
                            return;
                        }
                    }
                    // ── ¿UNA LATA QUE SALE DE LA ESTANTERÍA? (1.1) ───────
                    if e.suelta_lata(&mut self.proyecto, mx, my) {
                        e.arrastrando = Arrastre::Nada;
                        return;
                    }
                    // ── ¿UN RECORTE QUE SALE DEL CUBO? ───────────────────
                    if let Some((ir, px, py)) = e.cubo_pinza.take() {
                        if ir < e.recortes.len() {
                            let movido = (mx - px).abs() > 6.0 || (my - py).abs() > 6.0;
                            // UN RECORTE A LA PAPELERA: se va de verdad. Es lo
                            // que faltaba para que el cubo no creciera sin fin.
                            if movido && e.en_la_papelera(mx, my) {
                                let c = e.recortes.remove(ir);
                                e.visor.foley(sonido::Foley::Corte);
                                e.di(&format!("«{}» a la papelera ({} en el cubo)",
                                              c.media.chars().take(20).collect::<String>(),
                                              e.recortes.len()));
                                return;
                            }
                            let en_bobina = mx > Estado::ESTANTE_W;
                            if movido && !en_bobina {
                                e.di("suéltalo sobre la bobina para colocarlo");
                                return;
                            }
                            let c = e.recortes.remove(ir);
                            let dur = c.dur();
                            e.recuerda(&self.proyecto);
                            // arrastrado: donde se suelte. Clic: a la aguja.
                            let idx = if movido {
                                // DONDE SE SUELTA, de verdad: si el punto cae
                                // en la mitad derecha del clip que hay debajo,
                                // el recorte va DESPUÉS. Sin esto no había
                                // forma de dejarlo al final de la bobina.
                                e.junta_en(&self.proyecto, mx)
                            } else {
                                self.proyecto.en(e.visor.t).map(|x| x.0 + 1)
                                    .unwrap_or(self.proyecto.clips.len())
                            }.min(self.proyecto.clips.len());
                            self.proyecto.clips.insert(idx, c);
                            self.proyecto.cuantiza();
                            let _ = self.proyecto.guarda();
                            e.sel = Some(idx);
                            e.cubo_scroll = e.cubo_scroll.min(e.cubo_scroll_max());
                            e.visor.foley(sonido::Foley::Lata);
                            e.visor.busca(&self.proyecto, e.visor.t);
                            e.di(if movido { "recorte colocado donde lo soltaste" }
                                 else { "recorte rescatado, a la aguja" });
                            let _ = dur;
                            return;
                        }
                    }
                    if let Arrastre::ClipMueve(i) = e.arrastrando {
                        if !prefs::IMAN.load(std::sync::atomic::Ordering::Relaxed)
                            && mx > Estado::ESTANTE_W {
                            let t = e.tiempo_en(mx);
                            e.coloca_clip(&mut self.proyecto, i, t);
                        }
                    }
                    if e.arrastrando != Arrastre::Nada {
                        // fin de gesto: cuantizar a la REJILLA DE FRAMES,
                        // anotar en el historial (un gesto = UN paso) y guardar
                        self.proyecto.cuantiza();
                        let fps = self.proyecto.fps.max(1.0);
                        for a in &mut self.proyecto.audio {
                            a.start = (a.start * fps).round() / fps;
                            a.t_in = (a.t_in * fps).round() / fps;
                            a.t_out = ((a.t_out * fps).round() / fps).max(a.t_in + 1.0 / fps);
                        }
                        e.cierra_gesto(&self.proyecto);
                        let _ = self.proyecto.guarda();
                    }
                    e.arrastrando = Arrastre::Nada;
                    if e.caja.take().is_some() && !e.seleccion.is_empty() {
                        e.di(&format!("{} clip(s) elegidos", e.seleccion.len()));
                    }
                    return;
                }
                let (mx, my) = e.raton;
                // la cabecera con las tres salas manda en cualquier sala
                if let Some(k) = e.nav_en(mx, my) {
                    e.va_a([Sala::Mesa, Sala::CuartoOscuro, Sala::Revelado][k]);
                    return;
                }
                match e.sala {
                    Sala::CuartoOscuro => e.pulsa_cuarto(&mut self.proyecto),
                    Sala::Revelado => e.pulsa_revelado(&mut self.proyecto),
                    _ => e.pulsa(&mut self.proyecto),
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if e.sala == Sala::Portada || e.sala == Sala::Revelado { return; }
                if e.sala == Sala::CuartoOscuro {
                    let d = match delta {
                        winit::event::MouseScrollDelta::LineDelta(_, y) => y,
                        winit::event::MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                    };
                    let (ancho, _) = e.gpu.alto_ancho();
                    let (mx, my) = e.raton;
                    if mx > ancho - 320.0 {
                        // la rueda mueve la aguja que tenga debajo (P11)
                        let filas = e.filas_cuarto();
                        for (y, gi, ri) in filas {
                            if let Some(ri) = ri {
                                if my >= y - 2.0 && my < y + 31.0 {
                                    let (k, _, lo, hi) = GRUPOS[gi].1[ri];
                                    let paso = (hi - lo) / 60.0 * if d > 0.0 { 1.0 } else { -1.0 };
                                    e.cambia_mando(&mut self.proyecto, k, paso, lo, hi);
                                    let _ = self.proyecto.guarda();
                                    return;
                                }
                            }
                        }
                    }
                    return;
                }
                let d = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y,
                    winit::event::MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                // LA RUEDA SOBRE EL CUBO lo recorre: no tiene fondo, así que
                // hay que poder bajar hasta el recorte de hace media hora
                if e.sala == Sala::Mesa {
                    let (mx, my) = e.raton;
                    let (cy, ch) = e.cubo_caja();
                    if mx < Estado::ESTANTE_W && my > cy && my < cy + ch {
                        let max = e.cubo_scroll_max();
                        e.cubo_scroll = (e.cubo_scroll - d * Estado::CUBO_FIL * 0.6)
                            .clamp(0.0, max);
                        return;
                    }
                }
                // la rueda sobre la ESTANTERÍA la desplaza (baldas largas)
                if e.sala == Sala::Mesa {
                    let (mx, my) = e.raton;
                    let banco = e.banco_y();
                    let (cy, _) = e.cubo_caja();
                    if mx < Estado::ESTANTE_W && my > Estado::CABECERA + 50.0
                        && my < cy {
                        let plan_max = {
                            let baldas = e.proyecto_baldas.clone();
                            e.estantes(&baldas).last().map(|(_, y, _)| *y + e.estante_scroll)
                                .unwrap_or(0.0)
                        };
                        let _ = banco;
                        let espacio = cy - (Estado::CABECERA + 64.0);
                        let max = (plan_max + 116.0 - (Estado::CABECERA + 64.0) - espacio).max(0.0);
                        e.estante_scroll = (e.estante_scroll - d * 26.0).clamp(0.0, max);
                        return;
                    }
                }
                // la rueda sobre la MANIVELA: moverse por la película sin teclado
                if e.sala == Sala::Mesa {
                    let (mx, my) = e.raton;
                    // la rueda sobre la manivela, DESDE SU GEOMETRÍA: la tenía
                    // escrita a mano en tres sitios y al recomponer el margen
                    // se habrían quedado los tres apuntando al hueco de antes
                    let (mcx, mcy) = e.manivela_centro();
                    if (mx - mcx).powi(2) + (my - mcy).powi(2) < 34.0f32.powi(2) {
                        let paso = if e.mods.shift_key() {
                            1.0
                        } else if e.mods.alt_key() {
                            // al empalme siguiente/anterior
                            let t = if d > 0.0 {
                                self.proyecto.inicios().into_iter()
                                    .find(|&x| x > e.visor.t + 0.02)
                                    .unwrap_or(self.proyecto.duracion())
                            } else {
                                self.proyecto.inicios().into_iter().rev()
                                    .find(|&x| x < e.visor.t - 0.02).unwrap_or(0.0)
                            };
                            e.visor.busca(&self.proyecto, t);
                            e.visor.foley(sonido::Foley::Tick);
                            return;
                        } else {
                            1.0 / self.proyecto.fps.max(1.0)
                        };
                        let t = (e.visor.t + if d > 0.0 { paso } else { -paso }).max(0.0);
                        e.visor.busca(&self.proyecto, t);
                        return;
                    }
                }
                // alt + rueda sobre el vidrio: PUNCH-IN del clip (encuadre)
                let [gx, gy, gw, gh] = e.visor.rect_pantalla;
                let (mx, my) = e.raton;
                if (e.mods.alt_key() || e.modo_encuadre.is_some())
                    && mx >= gx && mx <= gx + gw && my >= gy && my <= gy + gh {
                    if let Some((i, _)) = self.proyecto.en(e.visor.t) {
                        e.recuerda(&self.proyecto);
                        // LA RUEDA AMPLÍA SOBRE EL PUNTERO, no sobre el centro:
                        // es lo que uno espera, y sin ello ampliar obliga a
                        // recolocar a mano después.
                        let k = if d > 0.0 { 1.06f32 } else { 1.0 / 1.06 };
                        let (pu, pv) = (((mx - gx) / gw.max(1.0)) - 0.5,
                                        ((my - gy) / gh.max(1.0)) - 0.5);
                        let z = {
                            let c = &mut self.proyecto.clips[i];
                            c.enc.escala.0 = (c.enc.escala.0 * k).clamp(0.05, 12.0);
                            c.enc.escala.1 = (c.enc.escala.1 * k).clamp(0.05, 12.0);
                            // el punto bajo el ratón se queda donde estaba
                            c.enc.pos.0 = pu - (pu - c.enc.pos.0) * k;
                            c.enc.pos.1 = pv - (pv - c.enc.pos.1) * k;
                            c.enc.escala.0
                        };
                        let _ = self.proyecto.guarda();
                        e.visor.marca_cuarto(i);
                        e.di(&format!("encuadre ×{z:.2}"));
                    }
                    return;
                }
                if e.sala == Sala::Mesa {
                    let (mx, my) = e.raton;
                    let banco = e.banco_y();
                    if my > banco && mx > Estado::ESTANTE_W {
                        if e.mods.super_key() || e.mods.control_key() {
                            // ⌘+rueda: la LUPA, anclada al tiempo bajo el cursor
                            let t_ancla = e.tiempo_en(mx);
                            e.pxs = (e.pxs * if d > 0.0 { 1.12 } else { 0.9 }).clamp(3.0, 200.0);
                            let max = e.desplaza_max(&self.proyecto);
                            e.desplaza = (Estado::ESTANTE_W + 12.0 + t_ancla as f32 * e.pxs - mx)
                                .clamp(0.0, max);
                        } else {
                            // rueda a secas: MOVERSE por la bobina sin tocar la aguja
                            let max = e.desplaza_max(&self.proyecto);
                            e.desplaza = (e.desplaza - d * 40.0).clamp(0.0, max);
                        }
                        return;
                    }
                    e.pxs = (e.pxs * if d > 0.0 { 1.12 } else { 0.9 }).clamp(3.0, 200.0);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                if e.sala == Sala::Portada {
                    let accion = e.teclado_portada(&self.proyecto, &event);
                    self.aplica(accion);
                    return;
                }
                // la chuleta se abre con «?» y se cierra con cualquier tecla
                if event.text.as_ref().map(|t| t.contains('?')).unwrap_or(false) {
                    e.chuleta = !e.chuleta;
                    return;
                }
                if e.chuleta {
                    e.chuleta = false;
                    return;
                }
                if e.ajustes {
                    e.ajustes = false;
                    return;
                }
                if e.notando.is_some() {
                    match event.physical_key {
                        // ⌫ SOBRE EL CUBO: tira ese recorte. Es el otro camino a
                    // la papelera, para quien no quiera arrastrar.
                    PhysicalKey::Code(KeyCode::Backspace | KeyCode::Delete)
                        if e.recorte_en(e.raton.0, e.raton.1).is_some() => {
                        if let Some(ir) = e.recorte_en(e.raton.0, e.raton.1) {
                            if ir < e.recortes.len() {
                                let c = e.recortes.remove(ir);
                                e.visor.foley(sonido::Foley::Corte);
                                e.di(&format!("«{}» a la papelera ({} en el cubo)",
                                              c.media.chars().take(20).collect::<String>(),
                                              e.recortes.len()));
                            }
                        }
                    }
                    PhysicalKey::Code(KeyCode::Escape) => { e.notando = None; }
                        PhysicalKey::Code(KeyCode::Enter) => {
                            if let Some((i, txt)) = e.notando.take() {
                                e.recuerda(&self.proyecto);
                                if let Some(c) = self.proyecto.clips.get_mut(i) {
                                    c.nota = txt.trim().to_string();
                                }
                                let _ = self.proyecto.guarda();
                                e.di("nota pegada al clip");
                            }
                        }
                        PhysicalKey::Code(KeyCode::Backspace) => {
                            if let Some((_, t)) = e.notando.as_mut() { t.pop(); }
                        }
                        _ => {
                            if let (Some((_, t)), Some(txt)) = (e.notando.as_mut(), event.text.as_ref()) {
                                for ch in txt.chars() {
                                    if !ch.is_control() && t.chars().count() < 60 { t.push(ch); }
                                }
                            }
                        }
                    }
                    return;
                }
                // ESCRIBIENDO UN NÚMERO DEL ENCUADRE (doble clic en la fila)
                if e.tecleando.is_some() {
                    match event.physical_key {
                        // ⌫ SOBRE EL CUBO: tira ese recorte. Es el otro camino a
                    // la papelera, para quien no quiera arrastrar.
                    PhysicalKey::Code(KeyCode::Backspace | KeyCode::Delete)
                        if e.recorte_en(e.raton.0, e.raton.1).is_some() => {
                        if let Some(ir) = e.recorte_en(e.raton.0, e.raton.1) {
                            if ir < e.recortes.len() {
                                let c = e.recortes.remove(ir);
                                e.visor.foley(sonido::Foley::Corte);
                                e.di(&format!("«{}» a la papelera ({} en el cubo)",
                                              c.media.chars().take(20).collect::<String>(),
                                              e.recortes.len()));
                            }
                        }
                    }
                    PhysicalKey::Code(KeyCode::Escape) => { e.tecleando = None; }
                        PhysicalKey::Code(KeyCode::Enter) => {
                            if let Some((i, campo, txt)) = e.tecleando.take() {
                                match txt.trim().replace(',', ".").parse::<f64>() {
                                    Ok(v) => {
                                        e.recuerda(&self.proyecto);
                                        if let Some(c) = self.proyecto.clips.get_mut(i) {
                                            Estado::pon_campo(&mut c.enc, campo, v);
                                        }
                                        let _ = self.proyecto.guarda();
                                        e.visor.marca_cuarto(i);
                                    }
                                    Err(_) => e.di("eso no es un número"),
                                }
                            }
                        }
                        PhysicalKey::Code(KeyCode::Backspace) => {
                            if let Some((_, _, t)) = e.tecleando.as_mut() { t.pop(); }
                        }
                        _ => {
                            if let (Some((_, _, t)), Some(txt)) =
                                (e.tecleando.as_mut(), event.text.as_ref()) {
                                for ch in txt.chars() {
                                    if (ch.is_ascii_digit() || ch == '.' || ch == ','
                                        || ch == '-') && t.chars().count() < 12 {
                                        t.push(ch);
                                    }
                                }
                            }
                        }
                    }
                    return;
                }
                // ── CORREGIR UN SUBTÍTULO ────────────────────────────────
                // Lo primero que se hace con un subtítulo automático es
                // arreglarlo: se escribe encima, en su sitio, sin diálogos.
                if e.escribiendo_sub.is_some() {
                    match event.physical_key {
                        PhysicalKey::Code(KeyCode::Escape) => { e.escribiendo_sub = None; }
                        PhysicalKey::Code(KeyCode::Enter) => {
                            if let Some((k, txt)) = e.escribiendo_sub.take() {
                                let t = txt.trim().to_string();
                                if let Some(sb) = self.proyecto.subs.get_mut(k) {
                                    if sb.texto != t {
                                        e.recuerda(&self.proyecto);
                                        self.proyecto.subs[k].texto = t;
                                        let _ = self.proyecto.guarda();
                                        let (pw2, ph2) = e.lienzo_del_master(&self.proyecto);
                                        self.proyecto.refresca_pie(pw2 as u32, ph2 as u32);
                                        e.visor.olvida_capas();
                                        e.visor.busca(&self.proyecto, e.visor.t);
                                    }
                                }
                                e.di("subtítulo corregido");
                            }
                        }
                        PhysicalKey::Code(KeyCode::Backspace) => {
                            if let Some((_, t)) = e.escribiendo_sub.as_mut() { t.pop(); }
                        }
                        _ => {
                            if let (Some((_, t)), Some(txt)) =
                                (e.escribiendo_sub.as_mut(), event.text.as_ref()) {
                                for ch in txt.chars() {
                                    if !ch.is_control() && t.chars().count() < 200 { t.push(ch); }
                                }
                            }
                        }
                    }
                    return;
                }
                // LA NOTA DE UNA MARCA (⇧M)
                if e.marcando.is_some() {
                    match event.physical_key {
                        // ⌫ SOBRE EL CUBO: tira ese recorte. Es el otro camino a
                    // la papelera, para quien no quiera arrastrar.
                    PhysicalKey::Code(KeyCode::Backspace | KeyCode::Delete)
                        if e.recorte_en(e.raton.0, e.raton.1).is_some() => {
                        if let Some(ir) = e.recorte_en(e.raton.0, e.raton.1) {
                            if ir < e.recortes.len() {
                                let c = e.recortes.remove(ir);
                                e.visor.foley(sonido::Foley::Corte);
                                e.di(&format!("«{}» a la papelera ({} en el cubo)",
                                              c.media.chars().take(20).collect::<String>(),
                                              e.recortes.len()));
                            }
                        }
                    }
                    PhysicalKey::Code(KeyCode::Escape) => { e.marcando = None; }
                        PhysicalKey::Code(KeyCode::Enter) => {
                            if let Some((k, txt)) = e.marcando.take() {
                                if let Some(m) = self.proyecto.marcas.get_mut(k) {
                                    m.nota = txt.trim().to_string();
                                }
                                let _ = self.proyecto.guarda();
                                e.di("marca anotada");
                            }
                        }
                        PhysicalKey::Code(KeyCode::Backspace) => {
                            if let Some((_, t)) = e.marcando.as_mut() { t.pop(); }
                        }
                        _ => {
                            if let (Some((_, t)), Some(txt)) =
                                (e.marcando.as_mut(), event.text.as_ref()) {
                                for ch in txt.chars() {
                                    if !ch.is_control() && t.chars().count() < 48 { t.push(ch); }
                                }
                            }
                        }
                    }
                    return;
                }
                if e.renombrando.is_some() {
                    match event.physical_key {
                        // ⌫ SOBRE EL CUBO: tira ese recorte. Es el otro camino a
                    // la papelera, para quien no quiera arrastrar.
                    PhysicalKey::Code(KeyCode::Backspace | KeyCode::Delete)
                        if e.recorte_en(e.raton.0, e.raton.1).is_some() => {
                        if let Some(ir) = e.recorte_en(e.raton.0, e.raton.1) {
                            if ir < e.recortes.len() {
                                let c = e.recortes.remove(ir);
                                e.visor.foley(sonido::Foley::Corte);
                                e.di(&format!("«{}» a la papelera ({} en el cubo)",
                                              c.media.chars().take(20).collect::<String>(),
                                              e.recortes.len()));
                            }
                        }
                    }
                    PhysicalKey::Code(KeyCode::Escape) => { e.renombrando = None; }
                        PhysicalKey::Code(KeyCode::Enter) => {
                            if let Some((viejo, nuevo)) = e.renombrando.take() {
                                let base = self.proyecto.base.clone();
                                if proyecto::renombra_cinta(&base,
                                                            &mut self.proyecto, &viejo, &nuevo) {
                                    e.estanteria = self.proyecto.estanteria();
                e.proyecto_baldas = self.proyecto.baldas();
                                    e.di(&format!("«{nuevo}»"));
                                } else {
                                    e.di("no se pudo renombrar (¿de media/? ¿repetido?)");
                                }
                            }
                        }
                        PhysicalKey::Code(KeyCode::Backspace) => {
                            if let Some((_, n)) = e.renombrando.as_mut() { n.pop(); }
                        }
                        _ => {
                            if let (Some((_, n)), Some(txt)) = (e.renombrando.as_mut(), event.text.as_ref()) {
                                for ch in txt.chars() {
                                    if !ch.is_control() && n.chars().count() < 60 { n.push(ch); }
                                }
                            }
                        }
                    }
                    return;
                }
                if e.titulando.is_some() {
                    match event.physical_key {
                        // ⌫ SOBRE EL CUBO: tira ese recorte. Es el otro camino a
                    // la papelera, para quien no quiera arrastrar.
                    PhysicalKey::Code(KeyCode::Backspace | KeyCode::Delete)
                        if e.recorte_en(e.raton.0, e.raton.1).is_some() => {
                        if let Some(ir) = e.recorte_en(e.raton.0, e.raton.1) {
                            if ir < e.recortes.len() {
                                let c = e.recortes.remove(ir);
                                e.visor.foley(sonido::Foley::Corte);
                                e.di(&format!("«{}» a la papelera ({} en el cubo)",
                                              c.media.chars().take(20).collect::<String>(),
                                              e.recortes.len()));
                            }
                        }
                    }
                    PhysicalKey::Code(KeyCode::Escape) => {
                            e.titulando = None;
                            e.retitulando = None;
                        }
                        PhysicalKey::Code(KeyCode::Enter) => {
                            let texto = e.titulando.take().unwrap_or_default();
                            if !texto.trim().is_empty() {
                                let (w, h) = self.proyecto.formato.as_ref()
                                    .map(|f| (f.w, f.h)).unwrap_or((1920, 1080));
                                match titulo::crea(&self.proyecto.base, &texto, w, h) {
                                    Some(ruta) => {
                                        proyecto::importa_en(&self.proyecto.base,
                                            &self.proyecto.media_json(), &[ruta.clone()]);
                                        e.recuerda(&self.proyecto);
                                        let nombre = ruta.file_name()
                                            .map(|n| n.to_string_lossy().to_string())
                                            .unwrap_or_default();
                                        // ¿ESTAMOS CORRIGIENDO UN RÓTULO? entonces
                                        // se cambia la tarjeta del clip que ya
                                        // está, sin tocar su sitio ni su duración
                                        if let Some(j) = e.retitulando.take()
                                            .filter(|j| *j < self.proyecto.clips.len()) {
                                            let c = &mut self.proyecto.clips[j];
                                            c.media = nombre;
                                            c.ruta = ruta;
                                            c.nota = texto.trim().chars().take(40).collect();
                                            let _ = self.proyecto.guarda();
                                            e.estanteria = self.proyecto.estanteria();
                                            e.proyecto_baldas = self.proyecto.baldas();
                                            e.visor.recarga(&e.gpu, &self.proyecto);
                                            e.di("rótulo reescrito");
                                            return;
                                        }
                                        let idx = self.proyecto.en(e.visor.t).map(|x| x.0 + 1)
                                            .unwrap_or(self.proyecto.clips.len())
                                            .min(self.proyecto.clips.len());
                                        let mut rotulo = self.proyecto.hueco_de(4.0);
                                        rotulo.media = nombre;
                                        rotulo.ruta = ruta;
                                        rotulo.hueco = false;
                                        rotulo.fade = 0.5;
                                        rotulo.lut_color = self.proyecto.lut_color.clone();
                                        rotulo.nota = texto.trim().chars().take(40).collect();
                                        self.proyecto.clips.insert(idx, rotulo);
                                        let _ = self.proyecto.guarda();
                                        e.estanteria = self.proyecto.estanteria();
                                        e.proyecto_baldas = self.proyecto.baldas();
                                        e.sel = Some(idx);
                                        e.di("título a la bobina (4 s, recortable)");
                                    }
                                    None => e.di("no pude rasterizar el título"),
                                }
                            }
                        }
                        PhysicalKey::Code(KeyCode::Backspace) => {
                            if let Some(t) = e.titulando.as_mut() { t.pop(); }
                        }
                        _ => {
                            if let (Some(t), Some(txt)) = (e.titulando.as_mut(), event.text.as_ref()) {
                                for ch in txt.chars() {
                                    if !ch.is_control() && t.chars().count() < 60 { t.push(ch); }
                                }
                            }
                        }
                    }
                    return;
                }
                if let Some(sel) = e.revelar {
                    match event.physical_key {
                        // ⌫ SOBRE EL CUBO: tira ese recorte. Es el otro camino a
                    // la papelera, para quien no quiera arrastrar.
                    PhysicalKey::Code(KeyCode::Backspace | KeyCode::Delete)
                        if e.recorte_en(e.raton.0, e.raton.1).is_some() => {
                        if let Some(ir) = e.recorte_en(e.raton.0, e.raton.1) {
                            if ir < e.recortes.len() {
                                let c = e.recortes.remove(ir);
                                e.visor.foley(sonido::Foley::Corte);
                                e.di(&format!("«{}» a la papelera ({} en el cubo)",
                                              c.media.chars().take(20).collect::<String>(),
                                              e.recortes.len()));
                            }
                        }
                    }
                    PhysicalKey::Code(KeyCode::Escape) => { e.revelar = None; }
                        PhysicalKey::Code(KeyCode::ArrowUp) => {
                            e.revelar = Some(sel.saturating_sub(1));
                        }
                        PhysicalKey::Code(KeyCode::ArrowDown) => {
                            e.revelar = Some((sel + 1).min(PRESETS_REVELADO.len() - 1));
                        }
                        PhysicalKey::Code(KeyCode::Enter) => {
                            e.revelar = None;
                            e.revela(&self.proyecto, sel, None);
                        }
                        _ => {}
                    }
                    return;
                }
                if matches!(event.physical_key, PhysicalKey::Code(KeyCode::Comma))
                    && (e.mods.super_key() || e.mods.control_key()) {
                    e.ajustes = true;
                    return;
                }
                // ── las tres salas: 1 mesa · 2 cuarto oscuro · 3 revelado ──
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::Digit1) => { e.va_a(Sala::Mesa); return; }
                    PhysicalKey::Code(KeyCode::Digit2) => { e.va_a(Sala::CuartoOscuro); return; }
                    PhysicalKey::Code(KeyCode::Digit3) => { e.va_a(Sala::Revelado); return; }
                    _ => {}
                }
                if e.sala == Sala::CuartoOscuro {
                    match event.physical_key {
                        PhysicalKey::Code(KeyCode::Escape) => e.va_a(Sala::Mesa),
                        PhysicalKey::Code(KeyCode::Space) => {
                        e.lanzadera = 0;
                        e.visor.play_pausa(&self.proyecto);
                    }
                        PhysicalKey::Code(KeyCode::Backslash) | PhysicalKey::Code(KeyCode::KeyW) => {
                            e.visor.wipe = !e.visor.wipe;
                            e.di(if e.visor.wipe { "tira de prueba A/B" } else { "revelado puesto" });
                        }
                        PhysicalKey::Code(KeyCode::ArrowLeft) => {
                            let t = (e.visor.t - 1.0 / self.proyecto.fps).max(0.0);
                            e.visor.busca(&self.proyecto, t);
                        }
                        PhysicalKey::Code(KeyCode::ArrowRight) => {
                            let t = e.visor.t + 1.0 / self.proyecto.fps;
                            e.visor.busca(&self.proyecto, t);
                        }
                        PhysicalKey::Code(KeyCode::KeyR) => e.va_a(Sala::Revelado),
                        _ => {}
                    }
                    return;
                }
                if e.sala == Sala::Revelado {
                    // escribiendo la etiqueta de la lata: el teclado es suyo
                    if e.etiquetando {
                        match event.physical_key {
                            PhysicalKey::Code(KeyCode::Escape)
                            | PhysicalKey::Code(KeyCode::Enter) => {
                                e.etiquetando = false;
                                if let Some(t) = e.etiqueta.clone() {
                                    if t.trim().is_empty() { e.etiqueta = None; }
                                }
                                e.di("etiqueta puesta");
                            }
                            PhysicalKey::Code(KeyCode::Backspace) => {
                                if let Some(t) = e.etiqueta.as_mut() { t.pop(); }
                            }
                            _ => {
                                if let (Some(t), Some(txt)) = (e.etiqueta.as_mut(),
                                                               event.text.as_ref()) {
                                    for ch in txt.chars() {
                                        if !ch.is_control() && t.chars().count() < 40 {
                                            t.push(ch);
                                        }
                                    }
                                }
                            }
                        }
                        return;
                    }
                    match event.physical_key {
                        PhysicalKey::Code(KeyCode::Escape) => e.va_a(Sala::Mesa),
                        PhysicalKey::Code(KeyCode::ArrowUp) => {
                            e.preset_revelado = e.preset_revelado.saturating_sub(1);
                        }
                        PhysicalKey::Code(KeyCode::ArrowDown) => {
                            e.preset_revelado = (e.preset_revelado + 1).min(PRESETS_REVELADO.len() - 1);
                        }
                        PhysicalKey::Code(KeyCode::Enter) => {
                            let p = e.preset_revelado;
                            e.revela(&self.proyecto, p, None);
                        }
                        _ => {}
                    }
                    return;
                }
                let manda = e.mods.super_key() || e.mods.control_key();
                // el monitor de FUENTE tiene sus teclas propias
                if e.fuente.is_some() {
                    match event.physical_key {
                        // ⌫ SOBRE EL CUBO: tira ese recorte. Es el otro camino a
                    // la papelera, para quien no quiera arrastrar.
                    PhysicalKey::Code(KeyCode::Backspace | KeyCode::Delete)
                        if e.recorte_en(e.raton.0, e.raton.1).is_some() => {
                        if let Some(ir) = e.recorte_en(e.raton.0, e.raton.1) {
                            if ir < e.recortes.len() {
                                let c = e.recortes.remove(ir);
                                e.visor.foley(sonido::Foley::Corte);
                                e.di(&format!("«{}» a la papelera ({} en el cubo)",
                                              c.media.chars().take(20).collect::<String>(),
                                              e.recortes.len()));
                            }
                        }
                    }
                    PhysicalKey::Code(KeyCode::Escape) => { e.sale_fuente(&self.proyecto); return; }
                        PhysicalKey::Code(KeyCode::KeyI) => {
                            if let Some(f) = e.fuente.as_mut() {
                                f.marca_i = Some(e.visor.t);
                                if f.marca_o.map(|o| o <= e.visor.t).unwrap_or(false) { f.marca_o = None; }
                            }
                            e.di("marca de entrada");
                            return;
                        }
                        PhysicalKey::Code(KeyCode::KeyO) => {
                            if let Some(f) = e.fuente.as_mut() {
                                f.marca_o = Some(e.visor.t);
                                if f.marca_i.map(|i| i >= e.visor.t).unwrap_or(false) { f.marca_i = None; }
                            }
                            e.di("marca de salida");
                            return;
                        }
                        PhysicalKey::Code(KeyCode::Enter) => {
                            let al_final = e.mods.shift_key();
                            e.inserta_fuente(&mut self.proyecto, al_final);
                            return;
                        }
                        _ => {}
                    }
                }
                match event.physical_key {
                    // ⌫ SOBRE EL CUBO: tira ese recorte. Es el otro camino a
                    // la papelera, para quien no quiera arrastrar.
                    PhysicalKey::Code(KeyCode::Backspace | KeyCode::Delete)
                        if e.recorte_en(e.raton.0, e.raton.1).is_some() => {
                        if let Some(ir) = e.recorte_en(e.raton.0, e.raton.1) {
                            if ir < e.recortes.len() {
                                let c = e.recortes.remove(ir);
                                e.visor.foley(sonido::Foley::Corte);
                                e.di(&format!("«{}» a la papelera ({} en el cubo)",
                                              c.media.chars().take(20).collect::<String>(),
                                              e.recortes.len()));
                            }
                        }
                    }
                    PhysicalKey::Code(KeyCode::Escape) => {
                        // Esc va deshaciendo capas: primero la cuchilla puesta,
                        // luego el modo encuadre, luego la selección, y solo
                        // con la mesa limpia vuelve a la portada
                        if e.visor_lleno {
                            e.alterna_visor_lleno();
                        } else if e.marca_corte.take().is_some() {
                            e.di("cuchilla quitada");
                        } else if e.modo_encuadre.take().is_some() {
                            e.di("encuadre cerrado");
                        } else if e.sel.is_some() || !e.seleccion.is_empty()
                            || e.sel_capa.is_some() || e.sel_audio.is_some() {
                            e.sel = None;
                            e.seleccion.clear();
                            e.sel_audio = None;
                            e.sel_capa = None;
                        } else {
                            e.bobinas = proyecto::bobinas(&self.proyecto.base);
                            e.sala = Sala::Portada;
                        }
                    }
                    PhysicalKey::Code(KeyCode::Space) => {
                        e.lanzadera = 0;
                        e.visor.play_pausa(&self.proyecto);
                    }
                    // ⌥← / ⌥→ : DE MARCA EN MARCA, y la nota de la que se pisa
                    PhysicalKey::Code(KeyCode::ArrowLeft) if e.mods.alt_key() => {
                        if let Some(m) = self.proyecto.marcas.iter().rev()
                            .find(|m| m.t < e.visor.t - 0.02).cloned() {
                            e.visor.busca(&self.proyecto, m.t);
                            if !m.nota.is_empty() { e.di(&format!("marca: {}", m.nota)); }
                        }
                    }
                    PhysicalKey::Code(KeyCode::ArrowRight) if e.mods.alt_key() => {
                        if let Some(m) = self.proyecto.marcas.iter()
                            .find(|m| m.t > e.visor.t + 0.02).cloned() {
                            e.visor.busca(&self.proyecto, m.t);
                            if !m.nota.is_empty() { e.di(&format!("marca: {}", m.nota)); }
                        }
                    }
                    PhysicalKey::Code(KeyCode::ArrowLeft) => {
                        let t = (e.visor.t - 1.0 / self.proyecto.fps).max(0.0);
                        e.visor.busca(&self.proyecto, t);
                    }
                    PhysicalKey::Code(KeyCode::ArrowRight) => {
                        let t = e.visor.t + 1.0 / self.proyecto.fps;
                        e.visor.busca(&self.proyecto, t);
                    }
                    // ↑/↓: al corte anterior / siguiente
                    PhysicalKey::Code(KeyCode::ArrowUp) => {
                        let t = self.proyecto.inicios().into_iter().rev()
                            .find(|&x| x < e.visor.t - 0.02).unwrap_or(0.0);
                        e.visor.busca(&self.proyecto, t);
                    }
                    PhysicalKey::Code(KeyCode::ArrowDown) => {
                        let fin = self.proyecto.duracion();
                        let t = self.proyecto.inicios().into_iter()
                            .find(|&x| x > e.visor.t + 0.02).unwrap_or(fin);
                        e.visor.busca(&self.proyecto, t);
                    }
                    PhysicalKey::Code(KeyCode::Home) => e.visor.busca(&self.proyecto, 0.0),
                    PhysicalKey::Code(KeyCode::End) => {
                        let fin = self.proyecto.duracion();
                        e.visor.busca(&self.proyecto, fin);
                    }
                    // ⌘C copia · ⌘V pega tras la selección (o en la aguja) · ⌘D duplica
                    PhysicalKey::Code(KeyCode::KeyC) if manda => {
                        if let Some(c) = e.sel.and_then(|i| self.proyecto.clips.get(i)) {
                            e.portapapeles = Some(c.clone());
                            e.di("clip copiado");
                        }
                    }
                    PhysicalKey::Code(KeyCode::KeyX) if manda => {
                        if let Some(i) = e.sel {
                            if let Some(c) = self.proyecto.clips.get(i).cloned() {
                                e.recuerda(&self.proyecto);
                                e.portapapeles = Some(c);
                                self.proyecto.quita(i);
                                e.sel = None;
                                let _ = self.proyecto.guarda();
                                e.di("clip cortado al portapapeles");
                            }
                        }
                    }
                    PhysicalKey::Code(KeyCode::KeyV) if manda => {
                        if let Some(c) = e.portapapeles.clone() {
                            e.recuerda(&self.proyecto);
                            let idx = e.sel.map(|i| i + 1)
                                .or_else(|| self.proyecto.en(e.visor.t).map(|x| x.0 + 1))
                                .unwrap_or(self.proyecto.clips.len())
                                .min(self.proyecto.clips.len());
                            self.proyecto.clips.insert(idx, c);
                            let _ = self.proyecto.guarda();
                            e.sel = Some(idx);
                            e.di("clip pegado");
                        }
                    }
                    // ⇧D: el sonido del plano, a su propia pista (§7)
                    PhysicalKey::Code(KeyCode::KeyD) if e.mods.shift_key() && !manda => {
                        e.desacopla(&mut self.proyecto);
                    }
                    PhysicalKey::Code(KeyCode::KeyD) if manda => {
                        if let Some(i) = e.sel {
                            if let Some(c) = self.proyecto.clips.get(i).cloned() {
                                e.recuerda(&self.proyecto);
                                self.proyecto.clips.insert(i + 1, c);
                                let _ = self.proyecto.guarda();
                                e.sel = Some(i + 1);
                                e.di("clip duplicado");
                            }
                        }
                    }
                    PhysicalKey::Code(KeyCode::KeyZ) if manda => {
                        if e.mods.shift_key() { e.rehace(&mut self.proyecto); }
                        else { e.deshace(&mut self.proyecto); }
                    }
                    // Shift+Z: la bobina entera a la vista (zoom-to-fit)
                    PhysicalKey::Code(KeyCode::KeyZ) if e.mods.shift_key() => {
                        let (ancho, _) = e.gpu.alto_ancho();
                        let dur = self.proyecto.duracion().max(0.5) as f32;
                        e.pxs = ((ancho - Estado::ESTANTE_W - 40.0) / dur).clamp(3.0, 200.0);
                        e.desplaza = 0.0;   // la bobina entera cabe: nada que desplazar
                    }
                    PhysicalKey::Code(KeyCode::KeyI) => e.importa_dialogo(&mut self.proyecto),
                    PhysicalKey::Code(KeyCode::KeyA) => { e.ajustes = true; }
                    // G: un hueco (negro con silencio) de 1 s en la aguja
                    // V: velocidad del clip seleccionado (0.25×→0.5×→1×→2×→4×)
                    PhysicalKey::Code(KeyCode::KeyV) if !manda => {
                        if let Some(i) = e.sel {
                            if self.proyecto.clips.get(i).map(|c| !c.hueco).unwrap_or(false) {
                                e.recuerda(&self.proyecto);
                                let pasos = [0.25, 0.5, 1.0, 2.0, 4.0];
                                let c = self.proyecto.clips.get_mut(i).unwrap();
                                let k = pasos.iter().position(|p| (p - c.speed).abs() < 0.01).unwrap_or(2);
                                c.speed = pasos[(k + 1) % pasos.len()];
                                let v = c.speed;
                                let _ = self.proyecto.guarda();
                                e.di(&format!("velocidad ×{v:.2}"));
                                e.visor.busca(&self.proyecto, e.visor.t.min(self.proyecto.duracion()));
                            }
                        }
                    }
                    PhysicalKey::Code(KeyCode::KeyT) => {
                        e.titulando = Some(String::new());
                    }
                    // E: EL ENCUADRE sobre la imagen (§1.5 · A)
                    PhysicalKey::Code(KeyCode::KeyE) if !manda => {
                        match e.modo_encuadre.take() {
                            Some(_) => e.di("encuadre cerrado"),
                            None => match e.sel.or_else(|| e.bajo_aguja(&self.proyecto)) {
                                Some(i) if !self.proyecto.clips[i].hueco => {
                                    e.abre_encuadre(&self.proyecto, i);
                                }
                                _ => e.di("no hay clip que encuadrar"),
                            },
                        }
                    }
                    // ⌘← / ⌘→ : LOS CUARTOS DE VUELTA, el caso más común de
                    // todos (material grabado de lado)
                    PhysicalKey::Code(KeyCode::ArrowLeft) if manda => {
                        e.gira_cuarto(&mut self.proyecto, 3);
                    }
                    PhysicalKey::Code(KeyCode::ArrowRight) if manda => {
                        e.gira_cuarto(&mut self.proyecto, 1);
                    }
                    // ⌘⌥C / ⌘⌥V: calcar y pegar EL ENCUADRE (§1.5 · C)
                    PhysicalKey::Code(KeyCode::KeyC) if manda && e.mods.alt_key() => {
                        if let Some(c) = e.sel.or_else(|| e.bajo_aguja(&self.proyecto))
                            .and_then(|i| self.proyecto.clips.get(i)) {
                            e.encuadre_copiado = Some(c.enc);
                            e.di("encuadre calcado");
                        }
                    }
                    PhysicalKey::Code(KeyCode::KeyV) if manda && e.mods.alt_key() => {
                        e.pega_encuadre(&mut self.proyecto);
                    }
                    // I / O: el RANGO de la bobina (§4bis.2)
                    PhysicalKey::Code(KeyCode::KeyI) if e.mods.shift_key() => {
                        let t = e.visor.t;
                        let (_, b) = self.proyecto.tramo();
                        self.proyecto.rango = Some((t, b.max(t + 0.04)));
                        let _ = self.proyecto.guarda();
                        e.di(&format!("entrada del rango en {t:.2} s"));
                    }
                    PhysicalKey::Code(KeyCode::KeyO) if e.mods.shift_key() => {
                        let t = e.visor.t;
                        let (a, _) = self.proyecto.tramo();
                        self.proyecto.rango = Some((a.min(t - 0.04).max(0.0), t));
                        let _ = self.proyecto.guarda();
                        e.di(&format!("salida del rango en {t:.2} s"));
                    }
                    PhysicalKey::Code(KeyCode::KeyO) if !manda => {
                        // O a secas: el BUCLE del tramo marcado
                        if self.proyecto.rango.is_none() {
                            e.di("marca el rango con ⇧I y ⇧O");
                        } else {
                            e.bucle = !e.bucle;
                            e.di(if e.bucle { "en bucle sobre el rango" }
                                 else { "bucle quitado" });
                        }
                    }
                    PhysicalKey::Code(KeyCode::KeyU) => {
                        // U: quitar el rango entero
                        if self.proyecto.rango.take().is_some() {
                            e.bucle = false;
                            let _ = self.proyecto.guarda();
                            e.di("rango quitado: la bobina entera");
                        }
                    }
                    // F: CONGELAR el fotograma de la aguja (§4bis.3)
                    PhysicalKey::Code(KeyCode::KeyF) if !manda => {
                        e.congela(&mut self.proyecto);
                    }
                    PhysicalKey::Code(KeyCode::Digit0) => {
                        if let Some((i, _)) = self.proyecto.en(e.visor.t) {
                            e.recuerda(&self.proyecto);
                            let c = &mut self.proyecto.clips[i];
                            // «a cero» es el vídeo DERECHO, no tumbado: la
                            // orientación del fichero es parte del encuadre
                            c.enc = proyecto::Encuadre::limpio(c.cuartos_fichero);
                            let _ = self.proyecto.guarda();
                            e.visor.marca_cuarto(i);
                            e.di("encuadre a cero");
                        }
                    }
                    PhysicalKey::Code(KeyCode::KeyG) if manda && e.mods.shift_key() => {
                        // ⌘⇧G: quitar la grapa del clip seleccionado (y sus hermanos)
                        if let Some(g) = e.sel.and_then(|i| self.proyecto.clips.get(i))
                            .and_then(|c| c.grupo) {
                            e.recuerda(&self.proyecto);
                            for c in self.proyecto.clips.iter_mut() {
                                if c.grupo == Some(g) { c.grupo = None; }
                            }
                            let _ = self.proyecto.guarda();
                            e.di("grapa quitada");
                        }
                    }
                    PhysicalKey::Code(KeyCode::KeyG) if manda => {
                        // ⌘G: grapar la multi-selección (los clips van juntos)
                        if e.seleccion.len() >= 2 {
                            e.recuerda(&self.proyecto);
                            let g = self.proyecto.clips.iter()
                                .filter_map(|c| c.grupo).max().unwrap_or(0) + 1;
                            for &i in &e.seleccion {
                                if let Some(c) = self.proyecto.clips.get_mut(i) {
                                    c.grupo = Some(g);
                                }
                            }
                            let _ = self.proyecto.guarda();
                            e.visor.foley(sonido::Foley::Corte);
                            e.di(&format!("{} clips grapados", e.seleccion.len()));
                        } else {
                            e.di("⇧+clic para elegir varios y ⌘G los grapa");
                        }
                    }
                    PhysicalKey::Code(KeyCode::KeyG) => {
                        e.recuerda(&self.proyecto);
                        let idx = self.proyecto.en(e.visor.t).map(|x| x.0 + 1)
                            .unwrap_or(self.proyecto.clips.len())
                            .min(self.proyecto.clips.len());
                        let hueco = self.proyecto.hueco_de(1.0);
                        self.proyecto.clips.insert(idx, hueco);
                        let _ = self.proyecto.guarda();
                        e.sel = Some(idx);
                        e.di("hueco de 1 s (recórtalo por los bordes)");
                    }
                    // M: marca en la aguja. Si ya hay una cerca la quita; con
                    // ⇧ se le escribe la NOTA (una marca sirve para acordarse
                    // de algo, §4bis.1) y con ⌥ se le cambia el color.
                    PhysicalKey::Code(KeyCode::KeyM) => {
                        let t = e.visor.t;
                        let cerca = self.proyecto.marcas.iter()
                            .position(|m| (m.t - t).abs() < 0.15);
                        match (cerca, e.mods.shift_key(), e.mods.alt_key()) {
                            (Some(k), true, _) => {
                                let n = self.proyecto.marcas[k].nota.clone();
                                e.marcando = Some((k, n));
                            }
                            (Some(k), _, true) => {
                                let m = &mut self.proyecto.marcas[k];
                                m.color = (m.color + 1) % 4;
                                e.di(&format!("marca en color {}", m.color + 1));
                            }
                            (Some(k), _, _) => {
                                self.proyecto.marcas.remove(k);
                                e.di("marca quitada");
                            }
                            (None, _, _) => {
                                let fps = self.proyecto.fps.max(1.0);
                                let k = self.proyecto.marcas.len();
                                self.proyecto.marcas.push(proyecto::Marca::nueva(
                                    (t * fps).round() / fps));
                                self.proyecto.marcas.sort_by(|a, b|
                                    a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
                                e.visor.foley(sonido::Foley::Tick);
                                if e.mods.shift_key() {
                                    e.marcando = Some((k.min(self.proyecto.marcas.len() - 1),
                                                       String::new()));
                                } else {
                                    e.di("marca · ⇧M le escribe la nota");
                                }
                            }
                        }
                        let _ = self.proyecto.guarda();
                    }
                    PhysicalKey::Code(KeyCode::KeyB) => e.cuchilla(&mut self.proyecto),
                    PhysicalKey::Code(KeyCode::Backspace) | PhysicalKey::Code(KeyCode::Delete) => {
                        // ¿hay un SUBTÍTULO elegido? fuera él (lo primero: es
                        // lo que más se borra al repasar una transcripción)
                        if let Some(k) = e.sel_sub.take() {
                            if k < self.proyecto.subs.len() {
                                e.recuerda(&self.proyecto);
                                let q = self.proyecto.subs.remove(k);
                                let _ = self.proyecto.guarda();
                                let (pw2, ph2) = e.lienzo_del_master(&self.proyecto);
                                self.proyecto.refresca_pie(pw2 as u32, ph2 as u32);
                                e.visor.olvida_capas();
                                e.visor.foley(sonido::Foley::Corte);
                                e.di(&format!("fuera «{}»",
                                    q.texto.chars().take(24).collect::<String>()));
                            }
                            return;
                        }
                        // ¿hay una CAPA elegida? fuera ella
                        if let Some(k) = e.sel_capa.take() {
                            if k < self.proyecto.capas.len() {
                                e.recuerda(&self.proyecto);
                                let q = self.proyecto.capas.remove(k);
                                let _ = self.proyecto.guarda();
                                e.visor.foley(sonido::Foley::Corte);
                                e.di(&format!("capa «{}» fuera", q.c.media));
                            }
                            return;
                        }
                        // ¿hay una pista de MÚSICA elegida? se quita ella
                        if let Some(a) = e.sel_audio.take() {
                            if a < self.proyecto.audio.len() {
                                e.recuerda(&self.proyecto);
                                let q = self.proyecto.audio.remove(a);
                                let _ = self.proyecto.guarda();
                                e.visor.foley(sonido::Foley::Lata);
                                e.visor.busca(&self.proyecto, e.visor.t);
                                e.di(&format!("«{}» fuera de la bobina", q.media));
                            }
                            return;
                        }
                        let mut idx: Vec<usize> = if !e.seleccion.is_empty() {
                            e.seleccion.iter().copied().collect()
                        } else { e.sel.into_iter().collect() };
                        if !idx.is_empty() {
                            e.recuerda(&self.proyecto);
                            idx.sort_unstable_by(|a, b| b.cmp(a));
                            let n = idx.len();
                            let lift = e.mods.alt_key();
                            for i in idx {
                                if lift {
                                    // ⌥⌫: deja el HUECO (lift) — el sitio se respeta
                                    let dur = self.proyecto.clips.get(i).map(|c| c.dur());
                                    if let Some(dur) = dur {
                                        let vacio = self.proyecto.hueco_de(dur);
                                        if let Some(c) = self.proyecto.clips.get_mut(i) {
                                            e.recortes.push(c.clone());
                                            *c = vacio;
                                        }
                                    }
                                } else {
                                    if let Some(c) = self.proyecto.clips.get(i) {
                                        if !c.hueco { e.recortes.push(c.clone()); }
                                    }
                                    self.proyecto.quita(i);
                                }
                            }
                            e.sel = None;
                            e.seleccion.clear();
                            let _ = self.proyecto.guarda();
                            e.visor.busca(&self.proyecto, e.visor.t.min(self.proyecto.duracion()));
                            e.visor.foley(sonido::Foley::Lata);
                            e.di(if lift { "al cubo (queda el hueco)" }
                                 else if n > 1 { "clips al cubo de recortes" }
                                 else { "al cubo de recortes" });
                        }
                    }
                    // [ / ]: recortar el borde del clip al fotograma de la aguja
                    PhysicalKey::Code(KeyCode::BracketLeft) => {
                        if let Some((i, src_t)) = self.proyecto.en(e.visor.t) {
                            e.recuerda(&self.proyecto);
                            if let Some(c) = self.proyecto.clips.get_mut(i) {
                                c.t_in = src_t.min(c.t_out - 0.08);
                            }
                            self.proyecto.cuantiza();
                            let _ = self.proyecto.guarda();
                            e.visor.busca(&self.proyecto, e.visor.t);
                            e.di("cabeza recortada a la aguja");
                        }
                    }
                    PhysicalKey::Code(KeyCode::BracketRight) => {
                        if let Some((i, src_t)) = self.proyecto.en(e.visor.t) {
                            e.recuerda(&self.proyecto);
                            if let Some(c) = self.proyecto.clips.get_mut(i) {
                                c.t_out = src_t.max(c.t_in + 0.08);
                            }
                            self.proyecto.cuantiza();
                            let _ = self.proyecto.guarda();
                            e.visor.busca(&self.proyecto, e.visor.t);
                            e.di("cola recortada a la aguja");
                        }
                    }
                    // A: el acetato de guías sobre el vidrio
                    PhysicalKey::Code(KeyCode::KeyA) => {
                        e.acetato = !e.acetato;
                        e.di(if e.acetato { "acetato de guías puesto" } else { "acetato quitado" });
                    }
                    // J / K / L: la lanzadera (atrás · para · adelante, con marchas)
                    PhysicalKey::Code(KeyCode::KeyJ) => {
                        e.lanzadera = if e.lanzadera < 0 { (e.lanzadera * 2).max(-8) } else { -1 };
                        e.lanzadera_reloj = std::time::Instant::now();
                        e.visor.tocando = false;
                        e.di(&format!("◀◀ ×{}", -e.lanzadera));
                    }
                    PhysicalKey::Code(KeyCode::KeyK) => {
                        e.lanzadera = 0;
                        if e.visor.tocando { e.visor.play_pausa(&self.proyecto); }
                        e.di("parada");
                    }
                    PhysicalKey::Code(KeyCode::KeyL) if !manda => {
                        if e.lanzadera > 0 {
                            e.lanzadera = (e.lanzadera * 2).min(8);
                        e.lanzadera_reloj = std::time::Instant::now();
                            e.visor.tocando = false;
                            e.di(&format!("▶▶ ×{}", e.lanzadera));
                        } else if e.lanzadera < 0 {
                            e.lanzadera = 1;
                        e.lanzadera_reloj = std::time::Instant::now();
                            e.visor.tocando = false;
                            e.di("▶ ×1");
                        } else if !e.visor.tocando {
                            e.visor.play_pausa(&self.proyecto);
                        } else {
                            e.lanzadera = 2;
                        e.lanzadera_reloj = std::time::Instant::now();
                            e.visor.tocando = false;
                            e.di("▶▶ ×2");
                        }
                    }
                    PhysicalKey::Code(KeyCode::KeyS) => {
                        match self.proyecto.guarda() { Ok(()) => e.di("bobina guardada"), Err(_) => e.di("no se pudo guardar") }
                    }
                    PhysicalKey::Code(KeyCode::KeyW) => { e.visor.wipe = !e.visor.wipe; }
                    PhysicalKey::Code(KeyCode::KeyR) => e.va_a(Sala::Revelado),
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                // ¿ha terminado el oído? Va aquí porque instalar los
                // subtítulos toca la bobina, y el dibujo la ve prestada
                e.atiende_al_oido(&mut self.proyecto);
                // ¿el pie rasterizado va con los subtítulos? Al abrir la
                // bobina y al deshacer no, y hay que rehacerlo (rasterizar
                // está cacheado en disco: si no cambió nada, no cuesta)
                let vivos = self.proyecto.subs.iter()
                    .filter(|s| !s.texto.trim().is_empty()).count();
                if self.proyecto.pie.len() != vivos {
                    e.refresca_pie(&mut self.proyecto);
                }
                e.frame(&self.proyecto);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if let Some(e) = self.estado.as_mut() {
            // ¿alguien pidió una ventana desde el menú? aquí hay dónde crearla
            let base = self.proyecto.base.clone();
            e.atiende_ventanas(el, &base);
            // drenar la cabina AQUÍ, no en el redraw: el fotograma recién
            // decodificado no espera a la siguiente vuelta del vsync
            e.visor.drena();
            e.ventana.request_redraw();
            for c in &e.cristales { c.ventana.request_redraw(); }
        }
    }
}

/// lo que la portada le pide a la app

/// LA PLANTA DE LA SALA DE REVELADO.
///
/// Dónde cae cada cosa, en un solo sitio, y lo leen **el dibujo y el ratón**.
/// Antes cada uno llevaba sus números a mano y se iban separando solos: al
/// añadir un cuarto sello, los sellos pasaron a medir 960 px de ancho y el
/// parte de salida —que se colocaba en `x0 + n·240`— se fue fuera de la
/// pantalla. Con la planta, mover algo es cambiar una línea y que las dos
/// mitades se enteren.
struct Planta {
    x0: f32,
    etiqueta: (f32, f32, f32, f32),
    destino: (f32, f32, f32, f32),
    boton: (f32, f32, f32, f32),
    /// un rectángulo por sello, ya repartidos en rejilla
    sellos: Vec<(f32, f32, f32, f32)>,
    parte: (f32, f32),
    normaliza: (f32, f32, f32, f32),
    llave: (f32, f32, f32, f32),
    /// la regla del rango, y los dos tiradores
    rango: (f32, f32, f32, f32),
    cubetas_y: f32,
    cajon: (f32, f32),
}

impl Planta {
    /// dentro de un rectángulo (x, y, ancho, alto)
    fn dentro(r: (f32, f32, f32, f32), mx: f32, my: f32) -> bool {
        mx >= r.0 && mx <= r.0 + r.2 && my >= r.1 && my <= r.1 + r.3
    }

    fn de(ancho: f32, alto: f32, n_sellos: usize) -> Planta {
        let x0 = (ancho / 2.0 - 470.0).max(50.0);
        let y_et = Estado::CABECERA + 36.0 + 56.0 + 34.0;
        // LOS SELLOS EN REJILLA de dos columnas: en fila se salían de la sala
        // en cuanto pasaron de tres. Dos columnas caben siempre y dejan la
        // mitad derecha libre para el parte, que es lo que hay que leer al
        // lado del sello elegido.
        let filas = n_sellos.div_ceil(2);
        let y_sellos = y_et + 76.0;
        let sellos = (0..n_sellos).map(|k| {
            (x0 + (k % 2) as f32 * 240.0, y_sellos + (k / 2) as f32 * 62.0, 224.0, 52.0)
        }).collect();
        let parte_x = x0 + 500.0;
        let y_rango = y_sellos + filas as f32 * 62.0 + 8.0;
        Planta {
            x0,
            etiqueta: (x0, y_et + 16.0, 300.0, 40.0),
            destino: (x0 + 340.0, y_et + 2.0, 270.0, 22.0),
            boton: (x0 + 340.0, y_et + 32.0, 270.0, 46.0),
            sellos,
            parte: (parte_x, y_sellos),
            normaliza: (parte_x + 4.0, y_sellos + 34.0, 13.0, 13.0),
            llave: (parte_x + 4.0, y_sellos + 52.0, 186.0, 20.0),
            rango: (x0, y_rango, 700.0, 44.0),
            cubetas_y: y_rango + 60.0,
            cajon: (x0, (y_rango + 60.0).min(alto - 260.0)),
        }
    }
}

enum AccionPortada {
    Nada,
    /// abrir la bobina con esta clave ("" = la clásica)
    Abrir(String),
    /// seguir con la bobina ya cargada
    Continuar,
}

impl App {
    fn aplica(&mut self, a: AccionPortada) {
        match a {
            AccionPortada::Nada => {}
            AccionPortada::Continuar => {
                if let Some(e) = self.estado.as_mut() { e.sala = Sala::Mesa; }
            }
            AccionPortada::Abrir(clave) => {
                let Some(e) = self.estado.as_mut() else { return };
                if proyecto::activa(&self.proyecto.base, &clave).is_err() { return; }
                match Proyecto::cargar() {
                    Ok(p) => {
                        self.proyecto = p;
                        e.estanteria = self.proyecto.estanteria();
                e.proyecto_baldas = self.proyecto.baldas();
                        e.sel = None;
                        e.nueva = None;
                        e.visor.recarga(&e.gpu, &self.proyecto);
                        e.sala = Sala::Mesa;
                        e.di(&format!("bobina «{}»", self.proyecto.nombre));
                    }
                    Err(_) => e.di("no pude abrir la bobina"),
                }
            }
        }
    }
}

impl Estado {
    const ESTANTE_W: f32 = 230.0;
    const INSPECTOR_W: f32 = 250.0;
    /// la cabecera de la sala empieza DEBAJO de la barra de menú
    const CABECERA: f32 = 64.0 + menu::ALTO;

    fn di(&mut self, m: &str) {
        self.aviso = (m.to_string(), std::time::Instant::now());
        // EL PASO RECIÉN ANOTADO SE BAUTIZA CON EL AVISO. El aviso ya dice
        // exactamente qué acaba de pasar («corte en 3,20 s», «clip duplicado»),
        // así que es el rótulo más honesto que hay: no se inventa nada.
        if self.espera_rotulo {
            self.espera_rotulo = false;
            if let Some(p) = self.historia.last_mut() {
                if p.que.is_empty() { p.que = m.to_string(); }
            }
        }
    }

    /// qué deshace ⌘Z ahora mismo
    fn que_deshace(&self) -> Option<&str> {
        self.historia.last().map(|p| if p.que.is_empty() { "el último gesto" } else { p.que.as_str() })
    }

    /// cambiar de sala con su ceremonia (pliegue de papel / apagón, NORTE §2)
    fn va_a(&mut self, s: Sala) {
        if self.sala == s {
            return;
        }
        self.transicion = Some((self.sala, s, std::time::Instant::now()));
        self.sala = s;
        // el clac del interruptor al entrar/salir del cuarto oscuro
        if s == Sala::CuartoOscuro || self.transicion.map(|t| t.0) == Some(Sala::CuartoOscuro) {
            self.visor.foley(sonido::Foley::Corte);
        } else {
            self.visor.foley(sonido::Foley::Tick);
        }
        self.ventana.request_redraw();
    }

    /// qué sala de la cabecera hay bajo el ratón (0 mesa · 1 cuarto · 2 revelado)
    fn nav_en(&self, mx: f32, my: f32) -> Option<usize> {
        if my > Self::CABECERA - 14.0 || my < 16.0 {
            return None;
        }
        let (ancho, _) = self.gpu.alto_ancho();
        let mut nx = (ancho - 840.0).max(620.0);
        for (k, w) in [78.0f32, 158.0, 108.0].iter().enumerate() {
            if mx >= nx - 6.0 && mx <= nx + w - 6.0 {
                return Some(k);
            }
            nx += w + 18.0;
        }
        None
    }

    // ── la estantería con BALDAS (NORTE §2bis): un solo layout para
    //    dibujar y para tocar ──

    /// ¿pasa la cinta el filtro de las pestañitas?
    fn pasa_filtro(&self, c: &proyecto::Cinta) -> bool {
        match self.filtro {
            1 => c.fps > 0.0,
            2 => c.fps < 0.0,
            3 => c.fps == 0.0,
            _ => true,
        }
    }

    /// los elementos de la estantería con su posición:
    /// (y, either cabecera de balda o índice de lata con su centro)
    fn estantes(&self, baldas: &[(String, Option<std::path::PathBuf>)])
        -> Vec<(f32, f32, Result<usize, String>)> {
        // Ok(idx) = lata (x,y = centro) · Err(nombre) = cabecera de balda (y)
        let mut v = Vec::new();
        let mut y = Self::CABECERA + 64.0 - self.estante_scroll;
        let mut col = 0usize;
        let mut mete_grupo = |v: &mut Vec<(f32, f32, Result<usize, String>)>,
                              y: &mut f32, col: &mut usize, balda: Option<&str>| {
            for (i, c) in self.estanteria.iter().enumerate() {
                if c.balda.as_deref() != balda || !self.pasa_filtro(c) {
                    continue;
                }
                v.push((62.0 + *col as f32 * 106.0, *y + 48.0, Ok(i)));
                *col += 1;
                if *col == 2 {
                    *col = 0;
                    *y += 116.0;
                }
            }
            if *col != 0 {
                *col = 0;
                *y += 116.0;
            }
        };
        mete_grupo(&mut v, &mut y, &mut col, None);
        for (nombre, _) in baldas {
            v.push((10.0, y, Err(nombre.clone())));
            y += 30.0;
            if !self.baldas_cerradas.contains(nombre) {
                mete_grupo(&mut v, &mut y, &mut col, Some(nombre.as_str()));
            }
        }
        v
    }

    /// qué lata hay bajo el ratón (None si el punto cae fuera)
    fn lata_en(&self, mx: f32, my: f32) -> Option<usize> {
        if mx >= Self::ESTANTE_W - 4.0 || mx < 9.0 || my < Self::CABECERA + 44.0 {
            return None;
        }
        let baldas = self.proyecto_baldas.clone();
        for (cx, cy, item) in self.estantes(&baldas) {
            if let Ok(i) = item {
                if (mx - cx).abs() < 52.0 && (my - cy).abs() < 54.0 {
                    return Some(i);
                }
            }
        }
        None
    }

    /// qué cabecera de balda hay bajo el ratón
    fn balda_en(&self, mx: f32, my: f32) -> Option<String> {
        if mx >= Self::ESTANTE_W - 4.0 {
            return None;
        }
        let baldas = self.proyecto_baldas.clone();
        for (_, y, item) in self.estantes(&baldas) {
            if let Err(nombre) = item {
                if my >= y - 4.0 && my <= y + 24.0 {
                    return Some(nombre);
                }
            }
        }
        None
    }

    /// x de pantalla → tiempo de la CINTA de la fuente (barra proporcional)
    fn t_fuente_en(&self, mx: f32, dur: f64) -> f64 {
        let (ancho, _) = self.gpu.alto_ancho();
        let x0 = Self::ESTANTE_W + 12.0;
        let x1 = ancho - 24.0;
        (((mx - x0) / (x1 - x0)).clamp(0.0, 1.0) as f64) * dur
    }

    fn sale_fuente(&mut self, pr: &Proyecto) {
        let volver = self.fuente.as_ref().map(|f| f.t_bobina).unwrap_or(0.0);
        self.fuente = None;
        self.visor.fuente = None;
        self.visor.tocando = false;
        self.visor.busca(pr, volver);
    }

    /// ⏎ inserta el tramo [I,O] de la fuente en la aguja (⇧⏎: al final)
    fn inserta_fuente(&mut self, pr: &mut Proyecto, al_final: bool) {
        let Some(f) = &self.fuente else { return };
        let c = f.cinta.clone();
        let i0 = f.marca_i.unwrap_or(0.0);
        let o0 = f.marca_o.unwrap_or(c.dur);
        if o0 <= i0 + 0.04 { self.di("marca I/O sin chicha"); return; }
        let volver = f.t_bobina;
        self.recuerda(pr);
        let mut nuevo = pr.clip_de(&c);
        nuevo.t_in = i0;
        nuevo.t_out = o0;
        let idx = if al_final { pr.clips.len() }
                  else { pr.en(volver).map(|x| x.0).unwrap_or(pr.clips.len()) };
        pr.clips.insert(idx.min(pr.clips.len()), nuevo);
        pr.cuantiza();
        let _ = pr.guarda();
        self.sel = Some(idx.min(pr.clips.len() - 1));
        self.di(&format!("{:.1} s de «{}» a la bobina", o0 - i0, c.nombre));
        self.sale_fuente(pr);
    }

    // ══════════ historial: no existe acción sin deshacer (80 pasos) ═══

    fn bobinas_iguales(a: &(Vec<proyecto::Clip>, Vec<proyecto::ClipAudio>,
                            Vec<proyecto::Capa>, Vec<proyecto::Marca>),
                       pr: &Proyecto) -> bool {
        if a.3 != pr.marcas { return false; }
        // las capas también cuentan como cambio
        if a.2.len() != pr.capas.len()
            || !a.2.iter().zip(&pr.capas).all(|(x, y)| {
                x.c.media == y.c.media && (x.start - y.start).abs() < 1e-9
                    && (x.c.t_in - y.c.t_in).abs() < 1e-9
                    && (x.c.t_out - y.c.t_out).abs() < 1e-9
                    && (x.fundido_in - y.fundido_in).abs() < 1e-9
                    && (x.fundido_out - y.fundido_out).abs() < 1e-9
                    && x.c.enc == y.c.enc
            }) { return false }
        a.0.len() == pr.clips.len() && a.0.iter().zip(&pr.clips).all(|(x, y)| {
            x.media == y.media && x.hueco == y.hueco
                && (x.t_in - y.t_in).abs() < 1e-9 && (x.t_out - y.t_out).abs() < 1e-9
                && (x.fade - y.fade).abs() < 1e-9 && (x.speed - y.speed).abs() < 1e-9
                && x.enc == y.enc && (x.desfase - y.desfase).abs() < 1e-9
        }) && a.1.len() == pr.audio.len() && a.1.iter().zip(&pr.audio).all(|(x, y)| {
            x.media == y.media && (x.start - y.start).abs() < 1e-9
                && (x.t_in - y.t_in).abs() < 1e-9 && (x.t_out - y.t_out).abs() < 1e-9
                && (x.gain - y.gain).abs() < 1e-9
                && (x.fade_in - y.fade_in).abs() < 1e-9
                && (x.fade_out - y.fade_out).abs() < 1e-9
                && x.banda == y.banda
        })
    }

    /// anota el estado ANTES de una mutación puntual (corte, quitar, añadir)
    fn recuerda(&mut self, pr: &Proyecto) {
        self.historia.push(Paso { clips: pr.clips.clone(), audio: pr.audio.clone(),
                                  capas: pr.capas.clone(), marcas: pr.marcas.clone(),
                                  subs: pr.subs.clone(), que: String::new() });
        if self.historia.len() > 80 { self.historia.remove(0); }
        self.futuro.clear();
        self.espera_rotulo = true;
        self.sucio = true;
    }

    /// al empezar un gesto de arrastre: foto del estado
    fn abre_gesto(&mut self, pr: &Proyecto) {
        self.gesto_previo = Some((pr.clips.clone(), pr.audio.clone(),
                                  pr.capas.clone(), pr.marcas.clone()));
    }

    /// al soltar: si el gesto cambió algo, UN paso de historial
    fn cierra_gesto(&mut self, pr: &Proyecto) {
        if let Some(prev) = self.gesto_previo.take() {
            if !Self::bobinas_iguales(&prev, pr) {
                self.historia.push(Paso { clips: prev.0, audio: prev.1,
                                          capas: prev.2, marcas: prev.3,
                                          subs: pr.subs.clone(), que: String::new() });
                if self.historia.len() > 80 { self.historia.remove(0); }
                self.futuro.clear();
                self.espera_rotulo = true;
                self.sucio = true;
            }
        }
    }

    /// LO QUE PIDE EL MENÚ. Todo pasa por aquí, así que cada entrada de la
    /// barra hace exactamente lo mismo que su atajo — no hay dos caminos que
    /// puedan divergir.
    fn hace(&mut self, pr: &mut Proyecto, a: menu::Accion) {
        use menu::Accion as A;
        match a {
            A::Guardar => {
                match pr.guarda() {
                    Ok(()) => {
                        self.guardado_en = std::time::Instant::now();
                        self.sucio = false;
                        self.di(&format!("guardado en {}", pr.ruta_json().display()));
                    }
                    Err(e) => self.di(&format!("NO se pudo guardar: {e}")),
                }
            }
            A::GuardarComo => {
                let sug = format!("{}.json", pr.nombre);
                if let Some(r) = rfd::FileDialog::new()
                    .set_title("guardar una copia de la bobina")
                    .set_file_name(&sug)
                    .add_filter("bobina", &["json"])
                    .save_file()
                {
                    let _ = pr.guarda();
                    match std::fs::copy(pr.ruta_json(), &r) {
                        Ok(_) => self.di(&format!("copia guardada en {}", r.display())),
                        Err(e) => self.di(&format!("no se pudo copiar: {e}")),
                    }
                }
            }
            A::MostrarEnCarpeta => {
                let r = pr.ruta_json();
                #[cfg(target_os = "macos")]
                let _ = std::process::Command::new("open").arg("-R").arg(&r).spawn();
                #[cfg(target_os = "windows")]
                let _ = std::process::Command::new("explorer")
                    .arg(format!("/select,{}", r.display())).spawn();
                self.di(&format!("la bobina vive en {}", r.display()));
            }
            A::Importar => self.importa_dialogo(pr),
            A::ImportarCarpeta => self.importa_carpeta_dialogo(pr),
            A::Deshacer => self.deshace(pr),
            A::Rehacer => self.rehace(pr),
            A::Mesa => self.va_a(Sala::Mesa),
            A::CuartoOscuro => self.va_a(Sala::CuartoOscuro),
            A::Revelado => self.va_a(Sala::Revelado),
            A::Revelar => { let k = self.preset_revelado; self.revela(pr, k, None); }
            A::Chuleta => self.chuleta = !self.chuleta,
            A::Ajustes => { self.ajustes = !self.ajustes; }
            A::Acerca => self.di("Laboratorios Saorín · un taller de revelado · MIT"),
            // ── LO QUE ANTES SOLO AVISABA, ahora lo HACE ──────────────────
            A::Encuadre => {
                match self.modo_encuadre.take() {
                    Some(_) => self.di("encuadre cerrado"),
                    None => match self.sel.or_else(|| self.bajo_aguja(pr)) {
                        Some(i) if !pr.clips[i].hueco => {
                            self.va_a(Sala::Mesa);
                            self.abre_encuadre(pr, i);
                        }
                        _ => self.di("no hay clip que encuadrar"),
                    },
                }
            }
            A::Congelar => self.congela(pr),
            A::Desacopla => self.desacopla(pr),
            A::InsertaBobina => self.inserta_bobina(pr),
            A::MarcasCompas => self.marcas_al_compas(pr),
            A::Subtitular => self.pon_el_oido(pr),
            A::PieFuera => {
                if pr.subs.is_empty() { self.di("no hay subtítulos que quitar"); }
                else {
                    self.recuerda(pr);
                    let n = pr.subs.len();
                    pr.subs.clear();
                    self.sel_sub = None;
                    let _ = pr.guarda();
                    self.refresca_pie(pr);
                    self.di(&format!("fuera {n} subtítulo(s)"));
                }
            }
            A::MarcaAqui => {
                let t = pr.fps.max(1.0);
                let t = (self.visor.t * t).round() / t;
                self.recuerda(pr);
                if let Some(k) = pr.marcas.iter().position(|m| (m.t - t).abs() < 0.15) {
                    pr.marcas.remove(k);
                    self.di("marca quitada");
                } else {
                    pr.marcas.push(proyecto::Marca::nueva(t));
                    pr.marcas.sort_by(|a, b|
                        a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
                    self.di("marca");
                }
                let _ = pr.guarda();
            }
            A::RangoEntrada => {
                let (_, b) = pr.tramo();
                let t = self.visor.t;
                pr.rango = Some((t, b.max(t + 0.04)));
                let _ = pr.guarda();
                self.di(&format!("entrada del rango en {t:.2} s"));
            }
            A::RangoSalida => {
                let (a2, _) = pr.tramo();
                let t = self.visor.t;
                pr.rango = Some((a2.min(t - 0.04).max(0.0), t));
                let _ = pr.guarda();
                self.di(&format!("salida del rango en {t:.2} s"));
            }
            A::RangoQuitar => {
                self.bucle = false;
                if pr.rango.take().is_some() {
                    let _ = pr.guarda();
                    self.di("rango quitado: la bobina entera");
                } else { self.di("no había rango"); }
            }
            A::Bucle => {
                if pr.rango.is_none() { self.di("marca el rango con ⇧I y ⇧O"); }
                else {
                    self.bucle = !self.bucle;
                    self.di(if self.bucle { "en bucle sobre el rango" } else { "bucle quitado" });
                }
            }
            A::VentanaAjustes => self.abre_ventana(Ventana::Ajustes),
            A::VentanaChuleta => self.abre_ventana(Ventana::Chuleta),
            A::VentanaVigia => self.abre_ventana(Ventana::Vigia),
            A::VentanaBobinas => {
                // la lista se relee al abrirla: si has cortado una bobina
                // nueva desde la portada, tiene que estar
                self.bobinas = proyecto::bobinas(&pr.base);
                self.abre_ventana(Ventana::Bobinas);
            }
            A::Iman => {
                let v = !prefs::IMAN.load(std::sync::atomic::Ordering::Relaxed);
                prefs::IMAN.store(v, std::sync::atomic::Ordering::Relaxed);
                prefs::guarda(&pr.base);
                self.di(if v { "imán encendido: los clips se pegan" }
                        else { "imán apagado: los clips van libres" });
            }
            A::PantallaCompleta => self.alterna_pantalla_completa(),
            // UN MENÚ QUE AVISA EN VEZ DE HACER es un menú que estorba: todas
            // estas hacen ya lo que dicen.
            A::BobinaNueva => {
                self.bobinas = proyecto::bobinas(&pr.base);
                self.sala = Sala::Portada;
                self.nueva = Some(NuevaBobina {
                    nombre: String::new(), aspecto: 0, fps: 0, alto: 2, aviso: String::new(),
                });
            }
            A::Abrir => {
                self.bobinas = proyecto::bobinas(&pr.base);
                self.sala = Sala::Portada;
                self.di("elige una bobina");
            }
            A::DondeVa => {
                if let Some(dir) = rfd::FileDialog::new()
                    .set_title("¿dónde cuelgo el máster?")
                    .set_directory(self.destino.clone().unwrap_or_else(|| pr.base.join("out")))
                    .pick_folder() {
                    prefs::guarda_destino(&pr.base, Some(&dir));
                    self.di(&format!("el máster irá a {}", dir.display()));
                    self.destino = Some(dir);
                }
            }
            A::Cortar => self.cuchilla(pr),
            A::AlCubo => {
                let idx: Vec<usize> = if !self.seleccion.is_empty() {
                    let mut v: Vec<usize> = self.seleccion.iter().copied().collect();
                    v.sort_unstable_by(|a, b| b.cmp(a)); v
                } else { self.sel.into_iter().collect() };
                if idx.is_empty() { self.di("elige un clip primero"); return; }
                self.recuerda(pr);
                let n = idx.len();
                for i in idx {
                    if i < pr.clips.len() {
                        let c = pr.clips.remove(i);
                        if !c.hueco { self.recortes.push(c); }
                    }
                }
                self.sel = None;
                self.seleccion.clear();
                let _ = pr.guarda();
                self.visor.busca(pr, self.visor.t.min(pr.duracion()));
                self.di(&format!("{n} clip(s) al cubo de recortes"));
            }
            A::Duplicar => {
                match self.sel.or_else(|| self.bajo_aguja(pr)) {
                    Some(i) if i < pr.clips.len() => {
                        self.recuerda(pr);
                        let c = pr.clips[i].clone();
                        pr.clips.insert(i + 1, c);
                        let _ = pr.guarda();
                        self.sel = Some(i + 1);
                        self.di("clip duplicado");
                    }
                    _ => self.di("elige un clip primero"),
                }
            }
            A::SeleccionarTodo => {
                self.seleccion = (0..pr.clips.len()).collect();
                self.sel = if pr.clips.is_empty() { None } else { Some(0) };
                self.di(&format!("{} clip(s) elegidos", pr.clips.len()));
            }
            A::Lupa => self.di("la lupa: mantén ⌥ sobre el vidrio"),
        }
    }

    // ══════════════════ LAS VENTANAS APARTE (§3) ═════════════════════

    /// pedir una ventana (o cerrarla si ya está abierta). No se crea aquí: hay
    /// que hacerlo con el `ActiveEventLoop` delante, y eso solo lo tiene el
    /// bucle — así que se apunta y se atiende en la siguiente vuelta.
    fn abre_ventana(&mut self, q: Ventana) {
        if let Some(k) = self.cristales.iter().position(|c| c.que == q) {
            self.cierra_cristal(k);
            self.di("ventana cerrada");
            return;
        }
        self.ventana_pedida = Some(q);
    }

    /// atender la ventana pedida. Se llama desde el bucle, que es quien tiene
    /// el `ActiveEventLoop`.
    fn atiende_ventanas(&mut self, el: &ActiveEventLoop, base: &std::path::Path) {
        let Some(q) = self.ventana_pedida.take() else { return };
        let (w, h) = q.tam();
        // DÓNDE ESTABA: posición y tamaño se recuerdan por ventana (§3 · 5)
        let geo = prefs::geometria(base, q.clave());
        let (gx, gy, gw, gh) = encaja_en_pantalla(el.primary_monitor(),
                                                  geo.unwrap_or((f64::NAN, f64::NAN, w, h)));
        let mut attrs = Window::default_attributes()
            .with_title(q.titulo())
            .with_window_icon(icono_del_taller())
            .with_inner_size(winit::dpi::LogicalSize::new(gw, gh));
        if geo.is_some() {
            attrs = attrs.with_position(winit::dpi::LogicalPosition::new(gx, gy));
        }
        // sin marco del sistema también aquí: una ventana del taller con la
        // franja blanca de Windows alrededor se ve peor que la principal
        #[cfg(target_os = "windows")]
        {
            use winit::platform::windows::{WindowAttributesExtWindows, CornerPreference};
            attrs = attrs
                .with_decorations(false)
                .with_undecorated_shadow(true)
                .with_corner_preference(CornerPreference::Round);
        }
        let Ok(ventana) = el.create_window(attrs) else {
            self.di("no pude abrir la ventana");
            return;
        };
        let ventana = Arc::new(ventana);
        match self.gpu.secundaria(ventana.clone()) {
            Ok(gpu) => {
                let lienzo = ui::Lienzo::new(&gpu);
                let tipos = ui::Tipos::new(&gpu);
                self.cristales.push(Cristal { ventana, gpu, lienzo, tipos, que: q });
                self.di(&format!("{} en su ventana", q.clave()));
            }
            Err(e) => self.di(&format!("no pude montar la ventana: {e}")),
        }
    }

    /// PINTAR UNA VENTANA SECUNDARIA. Cada cristal tiene su lienzo y sus
    /// tipos, pero el dispositivo es el mismo: el vigía enseña el resultado de
    /// la cadena que ya se compuso para la mesa, sin repetir un solo pase.
    fn pinta_cristal(&mut self, k: usize, pr: &Proyecto) {
        let Some(c) = self.cristales.get(k) else { return };
        let que = c.que;
        let (ancho, alto) = c.gpu.alto_ancho();
        let mut d = ui::Dibujo::nuevo();
        // la cabecera propia (donde no hay marco del sistema) y el contenido
        // debajo: `desplaza_y` mueve el bloque entero sin tocar sus
        // coordenadas
        let cab = menu::cabecera_cristal();
        menu::dibuja_cabecera_cristal(&mut d, ancho, q_titulo(que), self.raton_cristal);
        d.desplaza_y = cab;
        let alto = (alto - cab).max(80.0);
        match que {
            Ventana::Ajustes => self.dibuja_ajustes_en(pr, &mut d, ancho, alto, false),
            Ventana::Bobinas => self.dibuja_bobinas_en(pr, &mut d, ancho, alto),
            Ventana::Chuleta => self.dibuja_chuleta_en(&mut d, ancho, alto, false),
            Ventana::Vigia => {
                // el vigía dice lo justo: el timecode y si está proyectando
                let f = pr.fps.max(1.0);
                let t = self.visor.t;
                let tc = format!("{:02}:{:02}:{:02}:{:02}", (t as u32) / 3600,
                                 ((t as u32) / 60) % 60, (t as u32) % 60,
                                 ((t % 1.0) * f) as u32);
                d.texto(14.0, alto - 24.0, &tc, 13.0, [0.85, 0.83, 0.78, 0.85]);
                d.texto(120.0, alto - 24.0,
                        if self.visor.tocando { "proyectando · clic: pausa" }
                        else { "en pausa · clic: proyectar" },
                        10.0, [0.6, 0.58, 0.54, 0.8]);
            }
        }
        d.desplaza_y = 0.0;
        let Some(c) = self.cristales.get_mut(k) else { return };
        c.lienzo.sube(&c.gpu, &d);
        c.tipos.prepara(&c.gpu, &d);
        let enc = c.gpu.encoder();
        let escala = c.gpu.escala;
        let visor = &self.visor;
        let lienzo = &c.lienzo;
        let tipos = &c.tipos;
        // el vigía encaja el lienzo del proyecto dentro de su ventana
        let rect = {
            let p = visor.proporcion().max(0.05);
            let (mut w, mut h) = (ancho, ancho / p);
            if h > alto { h = alto; w = h * p; }
            [(ancho - w) / 2.0, cab + (alto - h) / 2.0, w, h]
        };
        let fondo = if que == Ventana::Vigia {
            wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }
        } else {
            wgpu::Color { r: 0.949, g: 0.933, b: 0.894, a: 1.0 }
        };
        let (fis_w, fis_h) = (c.gpu.config.width as f32, c.gpu.config.height as f32);
        c.gpu.pinta_sobre(enc, fondo, |rp| {
            if que == Ventana::Vigia {
                visor.pinta_en(rp, escala, rect);
                rp.set_viewport(0.0, 0.0, fis_w, fis_h, 0.0, 1.0);
            }
            lienzo.pinta(rp);
            tipos.pinta(rp);
        });
    }

    /// cerrar una secundaria SIN matar la aplicación (§3 · 4), guardando dónde
    /// estaba para volver a abrirla ahí
    fn cierra_cristal(&mut self, k: usize) {
        if k >= self.cristales.len() { return; }
        let c = self.cristales.remove(k);
        self.guarda_geometria(&c.ventana, c.que.clave());
    }

    fn guarda_geometria(&self, v: &Window, clave: &str) {
        let base = self.taller.clone();
        let esc = v.scale_factor();
        let t = v.inner_size();
        let p = v.outer_position().ok();
        prefs::guarda_geometria(&base, clave,
            p.map(|p| p.x as f64 / esc).unwrap_or(0.0),
            p.map(|p| p.y as f64 / esc).unwrap_or(0.0),
            t.width as f64 / esc, t.height as f64 / esc);
    }

    /// PANTALLA COMPLETA de la ventana (la del visor solo, aparte).
    /// LA PREVIEW LLENA LA PANTALLA. La ventana se va a pantalla completa y
    /// el taller deja de dibujarse: sólo queda la imagen, encajada por su
    /// proporción sobre negro. El teclado no cambia — espacio, J/K/L, flechas
    /// y las marcas siguen donde estaban, que es de lo que se trata.
    /// INSERTAR OTRA BOBINA como un clip (CAPAS §7): la anidada. El clip es
    /// una ventana sobre la hija — se recorta como un clip y por dentro la
    /// hija sigue viva (editarla y volver refresca).
    fn inserta_bobina(&mut self, pr: &mut Proyecto) {
        let Some(f) = rfd::FileDialog::new()
            .add_filter("bobina", &["json"])
            .set_title("¿qué bobina va dentro?")
            .set_directory(pr.base.join("projects"))
            .pick_file() else { return };
        let Some(clave) = f.file_stem().map(|s| s.to_string_lossy().to_string()) else { return };
        if clave == pr.nombre {
            self.di("una bobina no puede llevarse a sí misma dentro");
            return;
        }
        let info = proyecto::bobinas(&pr.base).into_iter()
            .find(|b| b.clave == clave);
        let dur = info.map(|b| b.dur).unwrap_or(0.0);
        if dur < 0.1 {
            self.di("esa bobina está vacía");
            return;
        }
        self.recuerda(pr);
        let mut c = pr.hueco_de(dur);
        c.hueco = false;
        c.media = format!("⤷ {clave}");
        c.anidada = Some(clave.clone());
        pr.clips.push(c);
        pr.recarga_subbobinas();
        let _ = pr.guarda();
        self.sel = Some(pr.clips.len() - 1);
        self.visor.foley(sonido::Foley::Lata);
        self.visor.busca(pr, self.visor.t);
        self.di(&format!("«{clave}» dentro, como un clip de {dur:.1} s"));
    }

    /// DESACOPLAR EL SONIDO del clip que manda (el seleccionado, y si no, el
    /// que hay bajo la aguja): baja a una pista de audio propia y el clip se
    /// calla. A partir de ahí es material como cualquier otro — se mueve, se
    /// corta con la cuchilla y se le pone su nivel.
    fn desacopla(&mut self, pr: &mut Proyecto) {
        let Some(i) = self.sel.or_else(|| self.bajo_aguja(pr)) else {
            self.di("elige un clip"); return;
        };
        self.recuerda(pr);
        match pr.desacopla(i) {
            Some(ia) => {
                let _ = pr.guarda();
                self.sel_audio = Some(ia);
                self.sel = None;
                self.visor.foley(sonido::Foley::Corte);
                self.visor.busca(pr, self.visor.t);
                self.di(&format!("el sonido del plano {}, en su pista · ahora se corta y se \
                                  mueve por su cuenta", i + 1));
            }
            None => {
                self.historia.pop();
                self.di("ese clip no tiene sonido que soltar (o ya está suelto)");
            }
        }
    }

    /// LO QUE DURA EL MATERIAL de un fichero, en segundos, memorizado.
    ///
    /// Es el número que faltaba para poder **estirar** un recorte. Sin él los
    /// tiradores no sabían dónde acababa la cinta: o no dejaban crecer, o
    /// dejaban crecer hacia el vacío. Devuelve 0 si no se puede saber, y
    /// entonces quien pregunta no pone tope (más vale dejar estirar de más
    /// que impedir recuperar lo que sí está).
    fn dur_fuente(&mut self, ruta: &std::path::Path) -> f64 {
        if let Some(&d) = self.duraciones.get(ruta) { return d }
        let d = filmlook_core::indice::sondea(ruta).map(|x| x.3).ok()
            .filter(|x| *x > 0.01)
            .or_else(|| sonido::dur_de(ruta))
            .unwrap_or(0.0);
        self.duraciones.insert(ruta.to_path_buf(), d);
        d
    }

    /// QUITAR EL RANGO: vuelve a revelarse la bobina entera. Devuelve `true`
    /// si había algo que quitar.
    fn rango_quita(&mut self, pr: &mut Proyecto) -> bool {
        if pr.rango.take().is_none() { return false; }
        let _ = pr.guarda();
        self.bucle = false;
        self.visor.foley(sonido::Foley::Tick);
        self.di("se revela la bobina entera");
        true
    }

    fn alterna_visor_lleno(&mut self) {
        self.visor_lleno = !self.visor_lleno;
        let lleno = self.visor_lleno;
        self.ventana.set_fullscreen(if lleno {
            Some(winit::window::Fullscreen::Borderless(None))
        } else { None });
        self.di(if lleno { "la imagen, a pantalla completa — esc o doble clic para volver" }
                else { "de vuelta al taller" });
        self.ventana.request_redraw();
    }

    fn alterna_pantalla_completa(&mut self) {
        let ahora = self.ventana.fullscreen().is_some();
        self.ventana.set_fullscreen(if ahora { None }
            else { Some(winit::window::Fullscreen::Borderless(None)) });
        self.di(if ahora { "ventana" } else { "pantalla completa (F para salir)" });
    }

    fn importa_carpeta_dialogo(&mut self, pr: &mut Proyecto) {
        let Some(dir) = rfd::FileDialog::new()
            .set_title("una carpeta entera al taller — por referencia")
            .pick_folder() else { return };
        let mj = pr.media_json();
        let (nombre, n) = proyecto::importa_carpeta_como(&pr.base, &mj, &dir, None);
        self.di(&format!("«{nombre}»: {n} cinta(s) a la estantería"));
        self.visor.foley(sonido::Foley::Lata);
    }

    fn deshace(&mut self, pr: &mut Proyecto) {
        // LA CUCHILLA ES LO PRIMERO QUE DESHACE ⌘Z. Está puesta pero no ha
        // cortado nada: deshacer el gesto anterior dejándola ahí sería
        // deshacer lo que no se ha hecho.
        if self.marca_corte.take().is_some() {
            self.visor.foley(sonido::Foley::Tick);
            self.di("cuchilla quitada");
            return;
        }
        let Some(prev) = self.historia.pop() else { self.di("nada que deshacer"); return };
        let rotulo = if prev.que.is_empty() { "el último gesto".to_string() }
                     else { prev.que.clone() };
        self.futuro.push(Paso { clips: pr.clips.clone(), audio: pr.audio.clone(),
                                capas: pr.capas.clone(), marcas: pr.marcas.clone(),
                                subs: pr.subs.clone(), que: prev.que });
        pr.clips = prev.clips;
        pr.audio = prev.audio;
        pr.capas = prev.capas;
        pr.marcas = prev.marcas;
        pr.subs = prev.subs;
        let _ = pr.guarda();
        self.sel = None;
        self.espera_rotulo = false;
        self.visor.busca(pr, self.visor.t.min(pr.duracion()));
        self.di(&format!("deshecho: {rotulo}"));
    }

    fn rehace(&mut self, pr: &mut Proyecto) {
        let Some(sig) = self.futuro.pop() else { self.di("nada que rehacer"); return };
        let rotulo = if sig.que.is_empty() { "el último gesto".to_string() }
                     else { sig.que.clone() };
        self.historia.push(Paso { clips: pr.clips.clone(), audio: pr.audio.clone(),
                                  capas: pr.capas.clone(), marcas: pr.marcas.clone(),
                                  subs: pr.subs.clone(), que: sig.que });
        pr.clips = sig.clips;
        pr.audio = sig.audio;
        pr.capas = sig.capas;
        pr.marcas = sig.marcas;
        pr.subs = sig.subs;
        let _ = pr.guarda();
        self.sel = None;
        self.espera_rotulo = false;
        self.visor.busca(pr, self.visor.t.min(pr.duracion()));
        self.di(&format!("rehecho: {rotulo}"));
    }

    // ══════════════════════════ importación (diálogo y arrastre) ═══

    fn importa_dialogo(&mut self, pr: &mut Proyecto) {
        let Some(rutas) = rfd::FileDialog::new()
            .add_filter("vídeo", &["mp4", "mov", "m4v", "mkv", "webm"])
            .set_title("cintas al taller — por referencia, sin copiar nada")
            .pick_files() else { return };
        let (n, s) = proyecto::importa_en(&pr.base, &pr.media_json(), &rutas);
        if n > 0 {
            self.estanteria = pr.estanteria();
            self.proyecto_baldas = pr.baldas();
            self.di(&format!("{n} cinta(s) a la estantería"));
        } else if s > 0 {
            self.di("ya estaban todas");
        }
    }

    // ══════════════ el cursor ES el indicador de modo (QoL (E)) ═══

    fn pon_cursor(&mut self, c: winit::window::CursorIcon) {
        if c != self.cursor_puesto {
            self.ventana.set_cursor(c);
            self.cursor_puesto = c;
        }
    }

    fn cursor_mesa(&mut self, pr: &Proyecto) {
        use winit::window::CursorIcon as CI;
        let cur = match self.arrastrando {
            Arrastre::ClipMueve(_) | Arrastre::MusicaMueve(_)
                | Arrastre::CapaMueve(_) | Arrastre::CapaEncuadre(_)
                | Arrastre::SubMueve(_) => CI::Grabbing,
            _ if self.cubo_pinza.is_some() => CI::Grabbing,
            Arrastre::MusicaTrimI(_) | Arrastre::MusicaTrimD(_)
                | Arrastre::CapaTrimI(_) | Arrastre::CapaTrimD(_)
                | Arrastre::SubTrimI(_) | Arrastre::SubTrimD(_) => CI::ColResize,
            Arrastre::MusicaPunto(_, _) | Arrastre::MusicaGain(_) => CI::NsResize,
            Arrastre::Encuadre(_) => CI::Move,
            Arrastre::EncTirador(_, k) => match k {
                0 | 2 => CI::NwseResize, 1 | 3 => CI::NeswResize,
                4 | 6 => CI::NsResize, 5 | 7 => CI::EwResize,
                8 => CI::Move, _ => CI::Grabbing,
            },
            Arrastre::FilaEnc(_, _) | Arrastre::Volumen(_) => CI::EwResize,
            Arrastre::Rango(_) | Arrastre::RangoSala(_) => CI::ColResize,
            Arrastre::TrimI(_) | Arrastre::TrimD(_) => CI::ColResize,
            Arrastre::Aguja => CI::Crosshair,
            Arrastre::Mando(_) | Arrastre::Mando48(_, _) => CI::EwResize,
            Arrastre::Manivela | Arrastre::Barra => CI::Grabbing,
            Arrastre::Caja => CI::Crosshair,
            Arrastre::Nada => {
                let (mx, my) = self.raton;
                let (ancho, alto_v) = self.gpu.alto_ancho();
                // el borde de redimensionar se anuncia con el cursor
                if let Some(dir) = borde_en(ancho, alto_v, mx, my) {
                    self.pon_cursor(dir.into());
                    return;
                }
                let banco = self.banco_y();
                let ty = self.tira_y();
                if my < Self::CABECERA {
                    CI::Default
                } else if mx < Self::ESTANTE_W {
                    CI::Pointer
                } else if mx > ancho - Self::INSPECTOR_W && my < banco {
                    CI::EwResize
                } else if my > banco {
                    let alto_tira = 88.0;
                    if my >= ty - 4.0 && my <= ty + alto_tira + 6.0 {
                        let mut acc = 0.0f64;
                        let mut c2 = CI::Crosshair;
                        for c in pr.clips.iter() {
                            let x0 = self.x_de(acc);
                            let x1 = self.x_de(acc + c.dur());
                            if (mx - x0).abs() <= 7.0 || (mx - x1).abs() <= 7.0 {
                                c2 = CI::ColResize;
                                break;
                            }
                            if mx > x0 && mx < x1 { c2 = CI::Grab; break; }
                            acc += c.dur();
                        }
                        c2
                    } else {
                        CI::Crosshair
                    }
                } else {
                    CI::Default
                }
            }
        };
        self.pon_cursor(cur);
    }

    /// la chuleta del taller (?): todos los gestos, a la vista
    // ══════════════════ las tres salas (NORTE §2) ══════════════════

    /// la cabecera común del taller: logo, bobina, nav de salas, timecode
    fn dibuja_cabecera(&self, pr: &Proyecto, d: &mut ui::Dibujo, ancho: f32,
                       activa: usize, tiza: bool) {
        use ui::Familia::*;
        // TODO LO DE LA CABECERA BAJA lo que ocupa la barra de menú: antes se
        // le montaba encima (el rótulo del taller quedaba partido por las
        // persianas). Se desplaza el pintor y se devuelve al salir, así las
        // coordenadas de dentro siguen contando desde cero.
        let previo = d.desplaza_y;
        d.desplaza_y = previo + menu::ALTO;
        let tinta = if tiza { paleta::SAFE } else { paleta::TINTA };
        let tenue = if tiza { paleta::SAFE_TENUE } else { paleta::TINTA_TENUE };
        let vivo = if tiza { paleta::SAFE_VIVO } else { paleta::ROJO };
        trazo::linea(d, 0.0, Self::CABECERA - 2.0, ancho, Self::CABECERA - 2.0, 2.2, tinta, 1);
        if tiza {
            // en el cuarto oscuro la tinta es una: la luz de seguridad
            d.texto_f(Grot, 22.0, 16.0, "LABORATORIOS", 26.0, tinta);
            d.texto_f(Grot, 218.0, 16.0, "SAORÍN", 26.0, vivo);
        } else {
            d.texto_f(Grot, 23.8, 17.6, "LABORATORIOS", 26.0, paleta::ROJO);
            d.texto_f(Grot, 22.0, 16.0, "LABORATORIOS", 26.0, paleta::TINTA);
            d.texto_f(Grot, 219.6, 17.4, "SAORÍN", 26.0, paleta::AMBAR);
            d.texto_f(Grot, 218.0, 16.0, "SAORÍN", 26.0, paleta::ROJO);
        }
        let n: String = pr.nombre.chars().take(18).collect();
        d.texto_f(Mano, 360.0, 10.0, &n, 24.0, tinta);
        d.texto(362.0, 40.0, &pr.rotulo_formato(), 10.0, tenue);
        let nav = [("LA MESA", 78.0f32), ("EL CUARTO OSCURO", 158.0), ("EL REVELADO", 108.0)];
        let mut nx = (ancho - 840.0).max(620.0);
        for (k, (nombre, w)) in nav.iter().enumerate() {
            let esta = k == activa;
            d.texto_f(Grot, nx, 22.0, nombre, 13.0, if esta { vivo } else { tinta });
            if esta {
                trazo::subraya(d, nx, nx + w - 12.0, 40.0, 1.8, vivo, 17);
            }
            nx += w + 18.0;
        }
        let tc = {
            let f = pr.fps.max(1.0);
            let t = self.visor.t;
            let fr = ((t % 1.0) * f) as u32;
            format!("{:02}:{:02}:{:02}:{:02}", (t as u32) / 3600, ((t as u32) / 60) % 60,
                    (t as u32) % 60, fr)
        };
        // el contador mecánico: cada dígito vive en su ficha (NORTE §7.8)
        {
            let chip = if tiza { [0.02, 0.015, 0.01, 1.0] } else { paleta::PELICULA };
            let tinta_tc = if tiza { paleta::SAFE } else { paleta::HUESO };
            let mut cx2 = ancho - 466.0;
            for ch in tc.chars() {
                if ch == ':' {
                    d.texto(cx2 + 1.0, 20.0, ":", 16.0, if tiza { paleta::SAFE_TENUE }
                            else { paleta::TINTA_TENUE });
                    cx2 += 8.0;
                } else {
                    d.rect(cx2, 14.0, 15.0, 30.0, chip);
                    d.rect(cx2, 28.5, 15.0, 1.0, [1.0, 1.0, 1.0, 0.10]);
                    d.texto(cx2 + 3.0, 20.0, &ch.to_string(), 17.0, tinta_tc);
                    cx2 += 17.0;
                }
            }
        }
        // el estado del transporte, DIBUJADO a tinta (los glifos ▶/⏸ los
        // pinta el sistema como emoji de color: aquí todo es del taller)
        {
            let (ix2, iy2) = (ancho - 300.0, 24.0);
            if self.visor.tocando {
                // el triángulo de proyección
                d.tri([ix2, iy2], [ix2 + 11.0, iy2 + 6.0], [ix2, iy2 + 12.0], vivo);
            } else {
                // las dos barras de la pausa
                d.rect(ix2, iy2, 4.0, 12.0, tenue);
                d.rect(ix2 + 6.5, iy2, 4.0, 12.0, tenue);
            }
            d.texto(ix2 + 18.0, 22.0,
                    &format!("{} · {:.0} fps",
                             if self.visor.tocando { "proyectando" } else { "en pausa" },
                             self.visor.fps_medido), 13.0, tenue);
        }
        if self.aviso.1.elapsed().as_secs_f32() < 3.0 && !self.aviso.0.is_empty() {
            // el aviso flota justo bajo la cabecera, sin pisar la nav
            d.texto(ancho / 2.0 - 110.0, Self::CABECERA + 10.0, &self.aviso.0, 14.0,
                    if tiza { paleta::SAFE_VIVO } else { paleta::NARANJA });
        }
        d.desplaza_y = previo;
    }


    /// LA PARED del autor: las fotos pegadas con celo en la columna derecha
    /// (la portada y la sala de revelado comparten pared)
    fn pega_pared(&mut self, ancho: f32, alto: f32, y0: f32) {
        for (k, foto) in self.pared.iter_mut().enumerate() {
            let (tw, th) = (foto.tw as f32, foto.th as f32);
            let w2 = 150.0;
            let h2 = (w2 * th / tw).min(190.0);
            let px2 = ancho - 236.0 + (k % 2) as f32 * 26.0;
            let py2 = y0 + k as f32 * (h2 + 34.0);
            if py2 + h2 > alto - 96.0 { break; }
            foto.quad_uv_rot(px2, py2, w2, h2, [0.0, 0.0, 1.0, 1.0],
                             ((k * 41 % 5) as f32 - 2.0) * 0.02);
            self.objetos.quad_uv_rot(px2 + w2 / 2.0 - 26.0, py2 - 10.0, 52.0, 24.0,
                                     doodles::uv(doodles::CELO), if k % 2 == 0 { -0.05 } else { 0.04 });
        }
    }

    // ══════════════════════ EL ENCUADRE (§1.5) ═══════════════════════
    //
    // Las dos manos son la MISMA verdad: tocar la imagen mueve los números y
    // tocar los números mueve la imagen. Por eso todo pasa por el mismo
    // `Encuadre` del clip y todo se dibuja desde la misma geometría.

    /// las dimensiones GUARDADAS del material de un clip (las que quiere el
    /// conform; la rotación del contenedor va en `enc.cuartos`)
    fn medidas_fuente(pr: &Proyecto, i: usize) -> (f32, f32) {
        pr.clips.get(i)
            .and_then(|c| filmlook_core::indice::sondea_orientado(&c.ruta).ok())
            .map(|(w, h, _, _, _)| (w as f32, h as f32))
            .unwrap_or((1920.0, 1080.0))
    }

    /// el lienzo del proyecto en píxeles (el del máster)
    fn lienzo(pr: &Proyecto) -> (f32, f32) {
        match &pr.formato {
            Some(f) => (f.w as f32, f.h as f32),
            None => {
                let p = pr.proporcion().max(0.05);
                (1080.0 * p, 1080.0)
            }
        }
    }

    /// LAS CUATRO ESQUINAS del cuadro del clip, en uv de lienzo y con el giro
    /// puesto. De aquí salen los tiradores, el recuadro tenue sobre el visor y
    /// el croquis de la ficha: una sola geometría para todo.
    fn cuadro_encuadre(pr: &Proyecto, i: usize) -> [(f32, f32); 4] {
        let Some(c) = pr.clips.get(i) else { return [(0.0, 0.0); 4] };
        let (sw, sh) = Self::medidas_fuente(pr, i);
        let (pw, ph) = Self::lienzo(pr);
        let (ew, eh) = filmlook_core::plan::extension(&c.enc, sw, sh, pw, ph);
        let e = &c.enc;
        let ar = pw / ph.max(1.0);
        let (ax, ay) = (0.5 + e.pos.0 + (e.ancla.0 - 0.5) * ew,
                        0.5 + e.pos.1 + (e.ancla.1 - 0.5) * eh);
        let (sn, cs) = e.giro.to_radians().sin_cos();
        let gira = |u: f32, v: f32| -> (f32, f32) {
            let (dx, dy) = ((u - ax) * ar, v - ay);
            ((ax * ar + dx * cs - dy * sn) / ar, ay + dx * sn + dy * cs)
        };
        let (cx, cy) = (0.5 + e.pos.0, 0.5 + e.pos.1);
        [gira(cx - ew / 2.0, cy - eh / 2.0), gira(cx + ew / 2.0, cy - eh / 2.0),
         gira(cx + ew / 2.0, cy + eh / 2.0), gira(cx - ew / 2.0, cy + eh / 2.0)]
    }

    /// uv de lienzo → píxeles de pantalla sobre el visor
    fn a_pantalla(&self, u: f32, v: f32) -> (f32, f32) {
        let [gx, gy, gw, gh] = self.visor.rect_pantalla;
        (gx + u * gw, gy + v * gh)
    }

    /// píxeles de pantalla → uv de lienzo
    fn a_lienzo(&self, x: f32, y: f32) -> (f32, f32) {
        let [gx, gy, gw, gh] = self.visor.rect_pantalla;
        ((x - gx) / gw.max(1.0), (y - gy) / gh.max(1.0))
    }

    /// LOS TIRADORES, en pantalla: 0..3 esquinas · 4..7 bordes · 8 el ancla ·
    /// 9 el giro (fuera de la esquina de abajo)
    fn tiradores(&self, pr: &Proyecto, i: usize) -> [(f32, f32); 10] {
        let q = Self::cuadro_encuadre(pr, i);
        let mut v = [(0.0f32, 0.0f32); 10];
        for k in 0..4 { v[k] = self.a_pantalla(q[k].0, q[k].1); }
        for k in 0..4 {
            let j = (k + 1) % 4;
            v[4 + k] = ((v[k].0 + v[j].0) / 2.0, (v[k].1 + v[j].1) / 2.0);
        }
        let c = pr.clips.get(i);
        let (sw, sh) = Self::medidas_fuente(pr, i);
        let (pw, ph) = Self::lienzo(pr);
        if let Some(c) = c {
            let (ew, eh) = filmlook_core::plan::extension(&c.enc, sw, sh, pw, ph);
            let (ax, ay) = (0.5 + c.enc.pos.0 + (c.enc.ancla.0 - 0.5) * ew,
                            0.5 + c.enc.pos.1 + (c.enc.ancla.1 - 0.5) * eh);
            v[8] = self.a_pantalla(ax, ay);
        }
        // el tirador del giro: prolongando la diagonal desde el centro
        let centro = ((v[0].0 + v[2].0) / 2.0, (v[0].1 + v[2].1) / 2.0);
        let (dx, dy) = (v[2].0 - centro.0, v[2].1 - centro.1);
        let l = (dx * dx + dy * dy).sqrt().max(1.0);
        v[9] = (v[2].0 + dx / l * 26.0, v[2].1 + dy / l * 26.0);
        v
    }

    /// ¿sobre qué tirador está el ratón? (None = dentro o fuera del cuadro)
    fn tirador_en(&self, pr: &Proyecto, i: usize, mx: f32, my: f32) -> Option<u8> {
        let t = self.tiradores(pr, i);
        // el ancla y el giro primero: viven encima de los demás
        for k in [8usize, 9] {
            if (mx - t[k].0).abs() < 11.0 && (my - t[k].1).abs() < 11.0 { return Some(k as u8); }
        }
        for k in 0..8 {
            if (mx - t[k].0).abs() < 9.0 && (my - t[k].1).abs() < 9.0 { return Some(k as u8); }
        }
        None
    }

    /// ¿cae el punto DENTRO del cuadro del clip? (para arrastrarlo)
    fn dentro_del_cuadro(&self, pr: &Proyecto, i: usize, mx: f32, my: f32) -> bool {
        let q = Self::cuadro_encuadre(pr, i);
        let p: Vec<(f32, f32)> = q.iter().map(|(u, v)| self.a_pantalla(*u, *v)).collect();
        // producto vectorial con los cuatro lados: dentro si todos coinciden
        let mut signo = 0.0f32;
        for k in 0..4 {
            let (a, b) = (p[k], p[(k + 1) % 4]);
            let c = (b.0 - a.0) * (my - a.1) - (b.1 - a.1) * (mx - a.0);
            if signo == 0.0 { signo = c; }
            else if signo * c < 0.0 { return false; }
        }
        true
    }

    /// ARRASTRAR UN TIRADOR. El punto fijo es la esquina opuesta (o el ancla
    /// con ⌥), y la escala sale de cuánto se ha movido el ratón respecto a él.
    fn tira_del_encuadre(&mut self, pr: &mut Proyecto, i: usize, k: u8) {
        let Some((enc0, fijo, _)) = self.enc_gesto else { return };
        let (mu, mv) = self.a_lienzo(self.raton.0, self.raton.1);
        let (sw, sh) = Self::medidas_fuente(pr, i);
        let (pw, ph) = Self::lienzo(pr);
        let (ew0, eh0) = filmlook_core::plan::extension(&enc0, sw, sh, pw, ph);
        let Some(c) = pr.clips.get_mut(i) else { return };
        match k {
            8 => {
                // EL ANCLA: se arrastra sobre el propio material, en su uv
                let (cx, cy) = (0.5 + enc0.pos.0, 0.5 + enc0.pos.1);
                c.enc.ancla = (((mu - cx) / ew0.max(1e-4) + 0.5).clamp(-1.0, 2.0),
                               ((mv - cy) / eh0.max(1e-4) + 0.5).clamp(-1.0, 2.0));
            }
            9 => {
                // EL GIRO, alrededor del ancla
                let ar = pw / ph.max(1.0);
                let a = ((mv - fijo.1) as f32).atan2(((mu - fijo.0) * ar) as f32).to_degrees();
                let paso = if self.mods.shift_key() { 15.0 } else { 0.0 };
                let g = a - 45.0;   // el tirador nace en la diagonal
                c.enc.giro = if paso > 0.0 { (g / paso).round() * paso } else { g };
            }
            _ => {
                let (dx, dy) = ((mu - fijo.0).abs().max(1e-4), (mv - fijo.1).abs().max(1e-4));
                let (mut rx, mut ry) = (dx / ew0.max(1e-4), dy / eh0.max(1e-4));
                // los bordes tocan un solo eje
                match k { 4 | 6 => rx = 1.0, 5 | 7 => ry = 1.0, _ => {} }
                if self.mods.shift_key() && k < 4 {
                    let r = (rx + ry) * 0.5;
                    rx = r; ry = r;
                }
                c.enc.escala = ((enc0.escala.0 * rx).clamp(0.02, 20.0),
                                (enc0.escala.1 * ry).clamp(0.02, 20.0));
                // y el punto fijo se queda donde estaba
                let (ew, eh) = filmlook_core::plan::extension(&c.enc, sw, sh, pw, ph);
                let (fu, fv) = Self::fijo_normalizado(k);
                c.enc.pos = (fijo.0 - 0.5 - (fu - 0.5) * ew, fijo.1 - 0.5 - (fv - 0.5) * eh);
            }
        }
        self.visor.marca_cuarto(i);
    }

    /// dónde vive el punto fijo DENTRO del cuadro, en uv del material (0..1),
    /// para cada tirador
    fn fijo_normalizado(k: u8) -> (f32, f32) {
        match k {
            0 => (1.0, 1.0), 1 => (0.0, 1.0), 2 => (0.0, 0.0), 3 => (1.0, 0.0),
            4 => (0.5, 1.0), 5 => (0.0, 0.5), 6 => (0.5, 0.0), 7 => (1.0, 0.5),
            _ => (0.5, 0.5),
        }
    }

    /// el punto fijo de un tirador, en uv de LIENZO
    fn punto_fijo(pr: &Proyecto, i: usize, k: u8, desde_ancla: bool) -> (f32, f32) {
        let Some(c) = pr.clips.get(i) else { return (0.5, 0.5) };
        let (sw, sh) = Self::medidas_fuente(pr, i);
        let (pw, ph) = Self::lienzo(pr);
        let (ew, eh) = filmlook_core::plan::extension(&c.enc, sw, sh, pw, ph);
        let (fu, fv) = if desde_ancla || k == 9 { c.enc.ancla } else { Self::fijo_normalizado(k) };
        (0.5 + c.enc.pos.0 + (fu - 0.5) * ew, 0.5 + c.enc.pos.1 + (fv - 0.5) * eh)
    }

    /// EL VALOR de un campo del encuadre (para dibujarlo y para escribirlo)
    fn valor_campo(e: &proyecto::Encuadre, campo: u8) -> f64 {
        match campo {
            campo::ESCALA_X => e.escala.0 as f64 * 100.0,
            campo::ESCALA_Y => e.escala.1 as f64 * 100.0,
            campo::POS_X => e.pos.0 as f64 * 100.0,
            campo::POS_Y => e.pos.1 as f64 * 100.0,
            campo::GIRO => e.giro as f64,
            campo::ANCLA_X => e.ancla.0 as f64 * 100.0,
            _ => e.ancla.1 as f64 * 100.0,
        }
    }

    fn pon_campo(e: &mut proyecto::Encuadre, campo: u8, v: f64) {
        let f = v as f32;
        match campo {
            campo::ESCALA_X => e.escala.0 = (f / 100.0).clamp(0.02, 20.0),
            campo::ESCALA_Y => e.escala.1 = (f / 100.0).clamp(0.02, 20.0),
            campo::POS_X => e.pos.0 = (f / 100.0).clamp(-4.0, 4.0),
            campo::POS_Y => e.pos.1 = (f / 100.0).clamp(-4.0, 4.0),
            campo::GIRO => e.giro = f.clamp(-180.0, 180.0),
            campo::ANCLA_X => e.ancla.0 = (f / 100.0).clamp(-100.0, 200.0),
            _ => e.ancla.1 = (f / 100.0).clamp(-100.0, 200.0),
        }
    }

    /// ARRASTRAR EL NÚMERO de una fila de la ficha. Es exactamente el gesto de
    /// los galvanómetros del cuarto oscuro: así no hay dos lenguajes distintos
    /// en la misma aplicación.
    fn arrastra_numero(&mut self, pr: &mut Proyecto, i: usize, campo: u8, dx: f32) {
        let paso = match campo {
            campo::GIRO => 0.25,
            _ => 0.5,
        };
        let Some(c) = pr.clips.get_mut(i) else { return };
        let v = Self::valor_campo(&c.enc, campo) + (dx * paso) as f64;
        Self::pon_campo(&mut c.enc, campo, v);
        self.visor.marca_cuarto(i);
    }

    /// ABRIR EL ENCUADRE de un clip. La aguja se va con él: encuadrar mirando
    /// otro plano sería encuadrar a ciegas.
    fn abre_encuadre(&mut self, pr: &Proyecto, i: usize) {
        self.modo_encuadre = Some(i);
        self.sel = Some(i);
        let ini = pr.inicios().get(i).copied().unwrap_or(0.0);
        let dentro = pr.clips.get(i).map(|c| c.dur() * 0.5).unwrap_or(0.0);
        if !(self.visor.t >= ini && self.visor.t < ini + dentro * 2.0) {
            self.visor.busca(pr, ini + dentro);
        }
        self.di("encuadre: tiradores en la imagen · ⇧ uniforme · \
                 ⌥ desde el ancla · rueda amplía · esc sale");
    }

    /// LAS FILAS DE LA FICHA DEL ENCUADRE: una geometría para dibujar y para
    /// tocar. Si cada uno la calculara por su cuenta, mover una fila
    /// descolocaría la otra mitad.
    fn filas_encuadre(&self, y0: f32) -> Vec<(f32, u8)> {
        [campo::ESCALA_X, campo::ESCALA_Y, campo::POS_X, campo::POS_Y,
         campo::GIRO, campo::ANCLA_X, campo::ANCLA_Y]
            .iter().enumerate().map(|(k, c)| (y0 + k as f32 * 14.0, *c)).collect()
    }

    fn rotulo_campo(e: &proyecto::Encuadre, campo: u8) -> (&'static str, String) {
        match campo {
            campo::ESCALA_X => ("escala X", format!("{:.1} %", e.escala.0 * 100.0)),
            campo::ESCALA_Y => ("escala Y", format!("{:.1} %", e.escala.1 * 100.0)),
            campo::POS_X => ("posición X", format!("{:.1}", e.pos.0 * 100.0)),
            campo::POS_Y => ("posición Y", format!("{:.1}", e.pos.1 * 100.0)),
            campo::GIRO => ("giro", format!("{:.1} °", e.giro)),
            campo::ANCLA_X => ("ancla X", format!("{:.1} %", e.ancla.0 * 100.0)),
            _ => ("ancla Y", format!("{:.1} %", e.ancla.1 * 100.0)),
        }
    }

    /// por dónde va el campo en su recorrido (0..1), para la barrita
    fn fraccion_campo(e: &proyecto::Encuadre, campo: u8) -> f32 {
        match campo {
            campo::ESCALA_X => (e.escala.0 / 4.0).clamp(0.0, 1.0),
            campo::ESCALA_Y => (e.escala.1 / 4.0).clamp(0.0, 1.0),
            campo::POS_X => (e.pos.0 + 1.0).clamp(0.0, 2.0) / 2.0,
            campo::POS_Y => (e.pos.1 + 1.0).clamp(0.0, 2.0) / 2.0,
            campo::GIRO => (e.giro + 180.0).clamp(0.0, 360.0) / 360.0,
            campo::ANCLA_X => e.ancla.0.clamp(0.0, 1.0),
            _ => e.ancla.1.clamp(0.0, 1.0),
        }
    }

    /// LOS CUARTOS DE VUELTA. Girar 90° **cambia la forma del clip**, así que
    /// no es «giro = 90»: es un campo aparte y el conform se rehace con el
    /// ancho y el alto intercambiados (§1.5).
    fn gira_cuarto(&mut self, pr: &mut Proyecto, cuantos: u8) {
        let Some(i) = self.sel.or_else(|| self.bajo_aguja(pr)) else { return };
        if pr.clips.get(i).map(|c| c.hueco).unwrap_or(true) { return; }
        self.recuerda(pr);
        let q = {
            let c = &mut pr.clips[i];
            c.enc.cuartos = (c.enc.cuartos + cuantos) % 4;
            c.enc.cuartos
        };
        let _ = pr.guarda();
        self.visor.marca_cuarto(i);
        self.visor.foley(sonido::Foley::Tick);
        self.di(&format!("orientación {}°", q as u32 * 90));
    }

    /// PEGAR EL ENCUADRE calcado. Con selección múltiple va a todos: aplicar
    /// el mismo reencuadre a diez planos de una tacada es el gesto que hace
    /// esto útil de verdad (§1.5 · C).
    fn pega_encuadre(&mut self, pr: &mut Proyecto) {
        let Some(enc) = self.encuadre_copiado else {
            self.di("no hay ningún encuadre calcado (⌘⌥C lo calca)");
            return;
        };
        let destinos: Vec<usize> = if !self.seleccion.is_empty() {
            let mut v: Vec<usize> = self.seleccion.iter().copied().collect();
            v.sort_unstable();
            v
        } else {
            self.sel.or_else(|| self.bajo_aguja(pr)).into_iter().collect()
        };
        if destinos.is_empty() { self.di("no hay clip donde pegarlo"); return; }
        self.recuerda(pr);
        for i in &destinos {
            if let Some(c) = pr.clips.get_mut(*i) {
                if c.hueco { continue; }
                // los CUARTOS no se pegan: son del fichero, y copiarlos
                // tumbaría el material que ya estaba derecho
                let q = c.enc.cuartos;
                c.enc = enc;
                c.enc.cuartos = q;
            }
        }
        let _ = pr.guarda();
        if let Some(i) = destinos.first() { self.visor.marca_cuarto(*i); }
        self.di(&format!("encuadre pegado en {} clip(s)", destinos.len()));
    }

    /// CONGELAR EL FOTOGRAMA de la aguja (§4bis.3). El gesto más pedido de
    /// cualquier montaje y hasta ahora no había forma: se parte el clip por la
    /// aguja y el trozo de la derecha se queda quieto dos segundos.
    fn congela(&mut self, pr: &mut Proyecto) {
        let Some((i, src_t)) = pr.en(self.visor.t) else { self.di("no hay clip"); return };
        if pr.clips[i].hueco { self.di("un hueco no se congela"); return; }
        if pr.clips[i].congelado() { self.di("ese clip ya está congelado"); return; }
        self.recuerda(pr);
        let fps = pr.fps.max(1.0);
        let src_t = (src_t * fps).round() / fps;
        let mut hielo = pr.clips[i].clone();
        hielo.speed = 0.0;
        hielo.fade = 0.0;
        hielo.nota = String::new();
        // un clip congelado guarda su DURACIÓN en el par entrada/salida: la
        // fuente es siempre el mismo fotograma
        hielo.t_in = src_t;
        hielo.t_out = src_t + 2.0;
        let idx = if pr.corta(self.visor.t) { i + 1 } else { i + 1 };
        pr.clips.insert(idx.min(pr.clips.len()), hielo);
        let _ = pr.guarda();
        self.sel = Some(idx.min(pr.clips.len() - 1));
        self.visor.foley(sonido::Foley::Corte);
        self.visor.busca(pr, self.visor.t);
        self.di("fotograma congelado, 2 s (recórtalo por los bordes)");
    }

    /// LA CUCHILLA, EN DOS TIEMPOS (§1.3). La primera pulsación pone la marca
    /// —donde esté el ratón si está sobre la bobina, y si no en la aguja— y la
    /// segunda corta por ella. Ver dónde vas a cortar antes de cortar es la
    /// mitad del oficio, y además permite cortar en un punto sin mover la
    /// aguja, que hasta ahora obligaba a perder el sitio donde estabas
    /// mirando.
    fn cuchilla(&mut self, pr: &mut Proyecto) {
        if let Some(t) = self.marca_corte.take() {
            self.recuerda(pr);
            // ¿QUÉ PISTA MUERDE? La que esté elegida. Con una música
            // seleccionada la cuchilla corta la música; si no, el vídeo. La
            // herramienta es la misma, el material es el que tú digas.
            if let Some(ia) = self.sel_audio {
                match pr.corta_audio(ia, t) {
                    Some(nueva) => {
                        let _ = pr.guarda();
                        self.sel_audio = Some(nueva);
                        self.visor.foley(sonido::Foley::Corte);
                        self.di(&format!("la música, partida en {t:.2} s"));
                    }
                    None => {
                        self.historia.pop();
                        self.di("ahí no cabe un corte en la música");
                    }
                }
                return;
            }
            if pr.corta(t) {
                let _ = pr.guarda();
                self.visor.foley(sonido::Foley::Corte);
                self.di(&format!("corte en {t:.2} s"));
            } else {
                self.historia.pop();
                self.di("ahí no cabe un corte (queda menos de un fotograma)");
            }
            return;
        }
        // la marca se pone DONDE ESTABA EL RATÓN AL PULSAR, no donde esté
        // después: esa es la gracia de verla antes de cortar
        let (mx, my) = self.raton;
        let sobre_bobina = mx > Self::ESTANTE_W && my > self.banco_y();
        let t = if sobre_bobina { self.tiempo_en(mx) } else { self.visor.t };
        let fps = pr.fps.max(1.0);
        let mut t = ((t * fps).round() / fps).clamp(0.0, pr.duracion());
        // EL IMÁN, TAMBIÉN PARA LA CUCHILLA: pegarla al empalme de un plano o
        // a una marca es lo que hace que cortar la música «con la imagen» sea
        // un gesto y no puntería. El radio se mide en píxeles, no en
        // segundos, para que sea el mismo con la bobina abierta o apretada.
        let mut pegada = false;
        if prefs::IMAN.load(std::sync::atomic::Ordering::Relaxed) {
            let radio = (self.tiempo_en(mx + 10.0) - self.tiempo_en(mx)).abs().max(1.0 / fps);
            if let Some(x) = pr.iman_cerca(t, radio) {
                if (x - t).abs() > 1e-9 { pegada = true; }
                t = (x * fps).round() / fps;
            }
        }
        self.marca_corte = Some(t);
        self.visor.foley(sonido::Foley::Tick);
        let donde = if self.sel_audio.is_some() { "en la música" } else { "en la bobina" };
        let iman = if pegada { " · pegada al empalme" } else { "" };
        self.di(&format!("cuchilla {donde} en {t:.2} s{iman} · B corta · clic, esc o ⌘Z la quitan"));
    }

    /// EL CLIP DEL CUARTO OSCURO: el que hay **bajo la aguja**, y no el
    /// seleccionado. Es la diferencia entre una previsualización que dice la
    /// verdad y una que enseña un plano mientras revelas otro: seleccionar un
    /// clip en la mesa cambiaba lo que se veía en el cuarto (§5).
    fn bajo_aguja(&self, pr: &Proyecto) -> Option<usize> {
        pr.en(self.visor.t).map(|x| x.0)
    }

    /// las filas del panel de instrumentos: (y, None=cabecera de grupo gi,
    /// Some((gi,ri))=aguja) — la MISMA geometría para dibujar y para tocar
    fn filas_cuarto(&self) -> Vec<(f32, usize, Option<usize>)> {
        let mut v = Vec::new();
        let mut y = Self::CABECERA + 64.0;
        for (gi, (_, filas)) in GRUPOS.iter().enumerate() {
            v.push((y, gi, None));
            y += 24.0;
            if self.secciones[gi] {
                for ri in 0..filas.len() {
                    v.push((y, gi, Some(ri)));
                    y += 33.0;
                }
                y += 6.0;
            }
        }
        v
    }

    /// verter un baño sobre el clip: la receta ENTERA se sustituye
    fn vierte_bano(&mut self, pr: &mut Proyecto, k: usize) {
        let idx = self.bajo_aguja(pr);
        let Some(i) = idx else { self.di("no hay clip bajo la aguja"); return };
        self.recuerda(pr);
        let mut prefs = prefs_de_la_casa();
        if let Ok(extra) = serde_json::from_str::<serde_json::Value>(BANOS[k].1) {
            if let (Some(o), Some(e)) = (prefs.as_object_mut(), extra.as_object()) {
                for (kk, vv) in e { o.insert(kk.clone(), vv.clone()); }
            }
        }
        if let Some(c) = pr.clips.get_mut(i) {
            c.prefs = prefs;
        }
        let _ = pr.guarda();
        self.visor.marca_cuarto(i);
        self.visor.foley(sonido::Foley::Lata);
        self.di(&format!("baño «{}» vertido", BANOS[k].0));
    }

    /// una caja de stock: capa PARCIAL sobre la receta del clip
    fn pon_stock(&mut self, pr: &mut Proyecto, k: usize) {
        let idx = self.bajo_aguja(pr);
        let Some(i) = idx else { self.di("no hay clip bajo la aguja"); return };
        self.recuerda(pr);
        if let Ok(extra) = serde_json::from_str::<serde_json::Value>(STOCKS[k].1) {
            if let Some(c) = pr.clips.get_mut(i) {
                if let (Some(o), Some(e)) = (c.prefs.as_object_mut(), extra.as_object()) {
                    for (kk, vv) in e { o.insert(kk.clone(), vv.clone()); }
                }
            }
        }
        let _ = pr.guarda();
        self.visor.marca_cuarto(i);
        self.visor.foley(sonido::Foley::Tick);
        self.di(&format!("stock {} sobre el baño", STOCKS[k].0));
    }

    /// LOS TAMAÑOS DE LA COPIA: el lienzo, el doble y el cuádruple. Con el
    /// lienzo la copia sale SUPERMUESTREADA (se revela al doble y se reduce:
    /// el grano y los bordes salen sin escalones); los grandes salen
    /// directos, porque a ×4 el doble ya no cabe en ningún codificador y
    /// además la resolución sola ya trae el detalle del material.
    const COPIA_TAM: [(&'static str, u32, f64); 3] = [
        ("el lienzo · supermuestreada", 1, 2.0),
        ("el doble", 2, 1.0),
        ("el cuádruple", 4, 1.0),
    ];
    /// EL PAPEL: en qué se escribe la copia. El motor siempre da PNG de 16
    /// bits (sin pérdida); los otros dos salen de convertir ESE fichero.
    const COPIA_PAPEL: [(&'static str, &'static str); 3] = [
        ("PNG · 16 bits", "png16"),
        ("PNG · 8 bits", "png8"),
        ("JPEG · calidad 95", "jpg"),
    ];

    /// LA AMPLIADORA, en un solo sitio (la lección de la sala de revelado:
    /// si el dibujo y el ratón llevan cada uno sus números, se separan solos).
    /// Va DEBAJO DEL VIDRIO: la copia se saca donde estás mirando la imagen.
    /// Devuelve (tamaño, papel, botón).
    fn ampliadora(gx: f32, gy_abajo: f32, gw: f32)
                  -> ((f32, f32, f32, f32), (f32, f32, f32, f32), (f32, f32, f32, f32)) {
        let y = gy_abajo + 26.0;
        let w = (gw.min(760.0) - 12.0).max(360.0);
        let c = gx + (gw - w) / 2.0;
        let ancho_cel = (w - 190.0) / 2.0;
        ((c, y, ancho_cel - 8.0, 24.0),
         (c + ancho_cel, y, ancho_cel - 8.0, 24.0),
         (c + w - 180.0, y - 4.0, 180.0, 32.0))
    }

    /// EL CUARTO OSCURO (NORTE §4) — papel tiza, luz de seguridad, la imagen
    /// retroiluminada y el panel de instrumentos
    fn dibuja_cuarto(&mut self, pr: &Proyecto, d: &mut ui::Dibujo, dt: &mut ui::DibujoTex,
                     d2: &mut ui::Dibujo, ancho: f32, alto: f32) {
        use ui::Familia::*;
        self.dibuja_cabecera(pr, d, ancho, 1, true);
        let panel_w = 310.0;
        let izq_w = 250.0;

        // ── el vidrio, retroiluminado (lo único a todo color de la sala) ──
        let zona_w = ancho - izq_w - panel_w - 60.0;
        let zona_h = alto - Self::CABECERA - 170.0;
        let prop = pr.proporcion();
        let mut gw = zona_w;
        let mut gh = gw / prop;
        if gh > zona_h { gh = zona_h; gw = gh * prop; }
        let gx = izq_w + 30.0 + (zona_w - gw) / 2.0;
        let gy = Self::CABECERA + 50.0 + (zona_h - gh) / 2.0;
        // el halo de la luz que atraviesa el fotograma
        for (k, a) in [(18.0, 0.045f32), (10.0, 0.06), (4.0, 0.10)] {
            d.rect(gx - k, gy - k, gw + k * 2.0, gh + k * 2.0, [1.0, 0.72, 0.5, a]);
        }
        d.rect(gx, gy, gw, gh, paleta::NEGRO);
        let (vw, vh) = self.visor.encaje(gw, gh);
        self.visor.rect_pantalla = [gx + (gw - vw) / 2.0, gy + (gh - vh) / 2.0, vw, vh];

        // ── LA AMPLIADORA: sacar ESTE fotograma en papel ────────────────
        // El revelado saca la bobina; la ampliadora saca UNA imagen — la que
        // se está mirando, con su receta, sus capas y su encuadre, revelada
        // por el mismo motor. No es un fotograma robado del máster: no pasa
        // por el códec ni por el YUV de rango limitado.
        {
            let (r_tam, r_pap, r_bot) = Self::ampliadora(gx, gy + gh, gw);
            let (pw2, ph2) = self.lienzo_del_master(pr);
            let k = Self::COPIA_TAM[(self.master.copia_tam as usize).min(2)];
            d.texto(r_tam.0, r_tam.1 - 15.0, "LA AMPLIADORA", 8.0, paleta::SAFE);
            let celda = |d: &mut ui::Dibujo, r: (f32, f32, f32, f32),
                         rot: &str, val: &str, orden: u32| {
                trazo::caja(d, r.0, r.1, r.2, r.3, 1.1, paleta::SAFE_TENUE, orden);
                d.texto(r.0 + 8.0, r.1 + 8.0, rot, 7.0, paleta::SAFE_TENUE);
                d.texto(r.0 + 62.0, r.1 + 7.5, val, 8.5, paleta::SAFE);
            };
            celda(d, r_tam, "TAMAÑO",
                  &format!("{} · {}×{}", k.0, pw2 * k.1, ph2 * k.1), 720);
            celda(d, r_pap, "PAPEL",
                  Self::COPIA_PAPEL[(self.master.copia_papel as usize).min(2)].0, 721);
            let sacando = self.revelando.is_some();
            trazo::caja(d, r_bot.0, r_bot.1, r_bot.2, r_bot.3, 1.5,
                        if sacando { paleta::SAFE_TENUE } else { paleta::SAFE_VIVO }, 722);
            d.texto(r_bot.0 + 26.0, r_bot.1 + 11.0,
                    if sacando { "REVELANDO…" } else { "SACAR LA COPIA" }, 9.5,
                    if sacando { paleta::SAFE_TENUE } else { paleta::SAFE_VIVO });
            d.texto(r_tam.0, r_bot.1 + 36.0,
                    "la copia sale a copias/ · el fotograma de la aguja, con su receta",
                    7.0, paleta::SAFE_TENUE);
        }

        // ── la cabecera de la receta: ENTRADA · COLOR · EL CAJÓN ──
        let fx = izq_w + 30.0;
        let gelatina = |l: &Option<std::path::PathBuf>| -> String {
            l.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "— sin gelatina —".into())
        };
        // LAS GELATINAS SON DEL CLIP, no del proyecto (§4bis.4). El modelo ya
        // las guardaba por clip y el motor las respetaba, pero la sala enseñaba
        // las de la bobina entera: material de dos cámaras en la misma bobina
        // —el caso normal— no se podía resolver porque no se veía.
        let clip_ahora = self.bajo_aguja(pr).and_then(|i| pr.clips.get(i));
        let (li, lc) = match clip_ahora {
            Some(c) => (c.lut_in.clone(), c.lut_color.clone()),
            None => (pr.lut_in.clone(), pr.lut_color.clone()),
        };
        let etiquetas = [("ENTRADA", gelatina(&li)), ("COLOR", gelatina(&lc))];
        for (k, (rot, val)) in etiquetas.iter().enumerate() {
            let ex = fx + k as f32 * 250.0;
            d.rect_rot(ex, Self::CABECERA + 8.0, 235.0, 34.0, (k as f32 - 0.5) * 0.008,
                       [0.05, 0.04, 0.035, 0.9]);
            trazo::caja(d, ex, Self::CABECERA + 8.0, 235.0, 34.0, 1.1, paleta::SAFE_TENUE,
                        200 + k as u32);
            d.texto(ex + 8.0, Self::CABECERA + 11.0, rot, 8.0, paleta::SAFE_TENUE);
            let v: String = val.chars().take(24).collect();
            d.texto_f(Mano, ex + 70.0, Self::CABECERA + 12.0, &v, 16.0, paleta::SAFE);
        }
        trazo::caja(d, fx + 506.0, Self::CABECERA + 8.0, 190.0, 34.0, 1.1, paleta::SAFE_TENUE, 205);
        d.texto(fx + 516.0, Self::CABECERA + 18.0,
                if self.cajon { "CERRAR EL CAJÓN" } else { "EL CAJÓN DE GELATINAS" },
                9.0, paleta::SAFE);
        d.texto(fx, Self::CABECERA + 46.0,
                "las gelatinas son DE ESTE CLIP · ⇧+clic en el cajón: a toda la bobina",
                7.0, paleta::SAFE_TENUE);
        if self.cajon {
            // el cajón se desliza: dos baldas de geles (entrada / color)
            let cy0 = Self::CABECERA + 50.0;
            d2.rect(fx + 300.0, cy0, 420.0, 250.0, [0.06, 0.05, 0.04, 0.96]);
            trazo::caja(d2, fx + 300.0, cy0, 420.0, 250.0, 1.4, paleta::SAFE, 206);
            let tintes: [[f32; 4]; 6] = [
                [0.9, 0.55, 0.3, 0.8], [0.4, 0.6, 0.9, 0.8], [0.9, 0.4, 0.5, 0.8],
                [0.5, 0.8, 0.5, 0.8], [0.85, 0.8, 0.4, 0.8], [0.7, 0.5, 0.85, 0.8],
            ];
            for (fila, tipo) in ["entrada", "color"].iter().enumerate() {
                let gy2 = cy0 + 24.0 + fila as f32 * 112.0;
                d2.texto(fx + 312.0, gy2 - 12.0, &format!("GELATINAS DE {}", tipo.to_uppercase()),
                         7.0, paleta::SAFE_TENUE);
                let dir = pr.base.join("luts").join(tipo);
                let mut geles: Vec<String> = std::fs::read_dir(&dir).ok().map(|rd| {
                    rd.filter_map(|e| e.ok())
                      .filter(|e| e.path().extension().map(|x| x == "cube").unwrap_or(false))
                      .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().to_string()))
                      .collect()
                }).unwrap_or_default();
                geles.sort();
                for (k, g) in geles.iter().take(4).enumerate() {
                    let gx2 = fx + 312.0 + k as f32 * 100.0;
                    d2.rect_rot(gx2, gy2, 92.0, 64.0, (k as f32 - 1.5) * 0.01,
                                tintes[(k + fila * 3) % 6]);
                    d2.rect(gx2 + 4.0, gy2 + 4.0, 84.0, 56.0, [1.0, 1.0, 1.0, 0.10]);
                    let nom: String = g.chars().take(13).collect();
                    d2.texto_f(Mano, gx2 + 6.0, gy2 + 66.0, &nom, 12.0, paleta::SAFE);
                }
                if geles.is_empty() {
                    d2.texto(fx + 312.0, gy2 + 20.0, "(no hay geles en luts/)", 8.0,
                             paleta::SAFE_TENUE);
                }
            }
            d2.texto(fx + 312.0, cy0 + 228.0, "clic: poner la gelatina al clip · fuera: cerrar",
                     7.0, paleta::SAFE_TENUE);
        }

        // ── LOS BAÑOS (izquierda): las botellas de la casa ──
        d.texto_f(Grot, 20.0, Self::CABECERA + 14.0, "LOS BAÑOS", 12.0, paleta::SAFE);
        trazo::subraya(d, 20.0, 110.0, Self::CABECERA + 32.0, 1.4, paleta::SAFE_TENUE, 31);
        let banos = ["saorín · revelado", "La Chimera · S16", "La Chimera · Bolex",
                     "CineStill 800T", "FX off"];
        for (k, nombre) in banos.iter().enumerate() {
            let bx = 24.0 + (k % 2) as f32 * 112.0;
            let by = Self::CABECERA + 48.0 + (k / 2) as f32 * 168.0;
            let ang = ((k * 31 % 5) as f32 - 2.0) * 0.014;
            self.objetos.quad_uv_rot(bx, by, 84.0, 152.0, doodles::uv(doodles::BOTELLA), ang);
            d2.rect_rot(bx + 6.0, by + 74.0, 72.0, 32.0, ang, [0.94, 0.92, 0.86, 0.94]);
            // partir la etiqueta por palabras (las botellas no cortan sílabas)
            let (n1, n2) = match nombre.char_indices().filter(|(_, c)| *c == ' ')
                .map(|(i, _)| i).filter(|&i| i <= 12).last() {
                Some(i) if nombre.len() > 12 => (&nombre[..i], &nombre[i + 1..]),
                _ => (*nombre, ""),
            };
            d2.texto_f(Mano, bx + 10.0, by + 72.0, n1, 13.0, [0.15, 0.1, 0.08, 1.0]);
            if !n2.is_empty() {
                d2.texto_f(Mano, bx + 10.0, by + 87.0, n2, 13.0, [0.15, 0.1, 0.08, 1.0]);
            }
        }
        // ── LOS STOCKS: cajas estampadas, capas parciales de color ──
        let sy0 = Self::CABECERA + 560.0;
        d.texto_f(Grot, 20.0, sy0, "LOS STOCKS", 12.0, paleta::SAFE);
        trazo::subraya(d, 20.0, 116.0, sy0 + 18.0, 1.4, paleta::SAFE_TENUE, 33);
        for (k, (nombre, _)) in STOCKS.iter().enumerate() {
            let bx = 22.0 + (k % 2) as f32 * 110.0;
            let by = sy0 + 30.0 + (k / 2) as f32 * 58.0;
            self.objetos.quad_uv_rot(bx, by, 102.0, 50.0, doodles::uv(doodles::CAJA),
                                     ((k * 19 % 5) as f32 - 2.0) * 0.012);
            d2.texto_f(Grot, bx + 12.0, by + 18.0, nombre, 9.0, [0.35, 0.22, 0.10, 1.0]);
        }
        d.texto_f(Mano, 24.0, sy0 + 216.0, "no abrir la puerta", 15.0, paleta::SAFE_TENUE);
        d.texto_f(Mano, 32.0, sy0 + 234.0, "con papel dentro", 15.0, paleta::SAFE_TENUE);

        // ── EL PANEL DE INSTRUMENTOS (derecha): 37 galvanómetros en 6 baterías ──
        let ix = ancho - panel_w;
        d.texto_f(Grot, ix, Self::CABECERA + 14.0, "EL PANEL DE INSTRUMENTOS", 11.0, paleta::SAFE);
        trazo::subraya(d, ix, ix + 216.0, Self::CABECERA + 32.0, 1.4, paleta::SAFE_TENUE, 32);
        let idx = self.bajo_aguja(pr);
        let nombre = idx.and_then(|i| pr.clips.get(i)).map(|c| c.media.clone()).unwrap_or_default();
        let n: String = nombre.chars().take(20).collect();
        d.texto_f(Mano, ix, Self::CABECERA + 38.0,
                  if n.is_empty() { "(sin clip bajo la aguja)" } else { &n }, 16.0, paleta::SAFE_VIVO);
        // calcar / pegar la receta (prendida con chincheta)
        d.texto(ix + 156.0, Self::CABECERA + 42.0, "calcar", 8.0, paleta::SAFE);
        trazo::caja(d, ix + 150.0, Self::CABECERA + 38.0, 44.0, 16.0, 1.0, paleta::SAFE_TENUE, 340);
        if self.receta.is_some() {
            d.texto(ix + 204.0, Self::CABECERA + 42.0, "pegar", 8.0, paleta::SAFE_VIVO);
            trazo::caja(d, ix + 198.0, Self::CABECERA + 38.0, 40.0, 16.0, 1.0, paleta::SAFE_VIVO, 341);
            self.objetos.quad_uv_rot(ix + 240.0, Self::CABECERA + 36.0, 14.0, 14.0,
                                     doodles::uv(doodles::CHINCHETA_ROJA), 0.1);
        }
        let prefs = idx.and_then(|i| pr.clips.get(i)).map(|c| c.prefs.clone());
        for (y, gi, ri) in self.filas_cuarto() {
            if y > alto - 120.0 { break; }
            match ri {
                None => {
                    // la cabecera de la batería (clic: pliega/despliega)
                    let (titulo, filas) = GRUPOS[gi];
                    d.texto(ix, y + 4.0, if self.secciones[gi] { "▾" } else { "▸" }, 9.0,
                            paleta::SAFE_VIVO);
                    d.texto_f(Grot, ix + 16.0, y + 2.0, titulo, 10.0, paleta::SAFE);
                    d.texto(ix + panel_w - 36.0, y + 4.0, &format!("{}", filas.len()), 8.0,
                            paleta::SAFE_TENUE);
                    // punteado de separación, a pulso
                    trazo::linea(d, ix, y + 18.0, ix + panel_w - 24.0, y + 18.0, 1.0,
                                 paleta::SAFE_TENUE, 350 + gi as u32);
                }
                Some(ri) => {
                    let (clave, etiqueta, lo, hi) = GRUPOS[gi].1[ri];
                    let v = prefs.as_ref().and_then(|p| p.get(clave)).and_then(|x| x.as_f64())
                        .unwrap_or(0.0) as f32;
                    let f = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
                    d.texto(ix + 4.0, y + 6.0, etiqueta, 9.0, paleta::SAFE);
                    d.texto(ix + panel_w - 58.0, y + 6.0, &format!("{v:.2}"), 10.0, paleta::TIZA);
                    // EL OBTURADOR LLEVA INTERRUPTOR. Es el desenfoque de
                    // movimiento de la emulación, y apagarlo clavando el mando
                    // en cero era un pulso fino de más. El interruptor guarda
                    // el valor anterior, así que encender vuelve a lo que había.
                    if clave == "shutter" {
                        let on = v > 0.001;
                        let (sx, sy) = (ix + 76.0, y + 5.0);
                        trazo::caja(d, sx, sy, 30.0, 13.0, 1.1,
                                    if on { paleta::SAFE_VIVO } else { paleta::SAFE_TENUE }, 690);
                        d.rect(if on { sx + 16.0 } else { sx + 2.0 }, sy + 2.0, 12.0, 9.0,
                               if on { paleta::SAFE_VIVO } else { paleta::SAFE_TENUE });
                        d.texto(sx + 34.0, y + 7.0, if on { "movimiento" } else { "congelado" },
                                7.5, if on { paleta::SAFE } else { paleta::SAFE_TENUE });
                    }
                    // el galvanómetro
                    let (gcx, gcy) = (ix + 168.0, y + 24.0);
                    let r = 15.0f32;
                    let mut arco = Vec::new();
                    for sk in 0..=10 {
                        let a = std::f32::consts::PI * (1.0 + sk as f32 / 10.0);
                        arco.push((gcx + a.cos() * r, gcy + a.sin() * r * 0.85));
                    }
                    trazo::pulso(d, &arco, 1.0, paleta::SAFE_TENUE,
                                 300 + (gi * 16 + ri) as u32);
                    let a = std::f32::consts::PI * (1.0 + f);
                    trazo::linea(d, gcx, gcy, gcx + a.cos() * (r - 1.0),
                                 gcy + a.sin() * (r - 1.0) * 0.85, 1.5, paleta::SAFE_VIVO,
                                 500 + (gi * 16 + ri) as u32);
                    d.rect(gcx - 1.5, gcy - 1.5, 3.0, 3.0, paleta::SAFE_VIVO);
                }
            }
        }
        d.texto(ix, alto - 108.0, "rueda o arrastre sobre la aguja · alt-clic: al valor del baño",
                7.0, paleta::SAFE_TENUE);
        // ── la tira de contactos del proyecto (etalonar sin salir) ──
        let n_cont = pr.clips.len().min(10);
        if n_cont > 0 {
            let aw = 54.0f32;
            let sx = izq_w + 30.0;
            let sy = alto - 66.0;
            d.texto(sx, sy - 14.0, "LA TIRA DE CONTACTOS", 7.0, paleta::SAFE_TENUE);
            let inicios = pr.inicios();
            for k in 0..n_cont {
                let c = &pr.clips[k];
                if c.hueco {
                    d.rect(sx + k as f32 * (aw + 6.0), sy, aw, 32.0, [0.0, 0.0, 0.0, 0.8]);
                    continue;
                }
                let proxy = pr.base.join(".proxies").join(&c.media);
                let ruta = if proxy.is_file() { proxy } else { c.ruta.clone() };
                let clave = (format!("cont:{}:{k}", c.media), (c.t_in * 100.0) as u32);
                let x2 = sx + k as f32 * (aw + 6.0);
                if let Some(slot) = self.minis.pide(clave, &ruta, c.t_in + 0.2) {
                    dt.quad(x2, sy, aw, 32.0, slot, 1.0);
                } else {
                    d.rect(x2, sy, aw, 32.0, [0.0, 0.0, 0.0, 0.8]);
                }
                let bajo = inicios.get(k).map(|&t0| {
                    self.visor.t >= t0 && self.visor.t < t0 + c.dur()
                }).unwrap_or(false);
                if bajo {
                    trazo::caja(d, x2 - 2.0, sy - 2.0, aw + 4.0, 36.0, 1.4, paleta::SAFE_VIVO,
                                600 + k as u32);
                }
            }
        }

        // ── el transporte del cuarto: play + A/B ──
        let ty = alto - 90.0;
        let (bcx, bcy) = (ancho / 2.0, ty + 20.0);      // el centro de la chapa
        d.rect(bcx - 26.0, ty, 52.0, 40.0, paleta::SAFE_VIVO);
        let tinta_boton = [0.05, 0.02, 0.01, 1.0];
        if self.visor.tocando {
            // pausa: dos barras simétricas respecto al centro
            d.rect(bcx - 7.0, bcy - 8.0, 5.0, 16.0, tinta_boton);
            d.rect(bcx + 2.0, bcy - 8.0, 5.0, 16.0, tinta_boton);
        } else {
            // play: el triángulo con su centroide EN el centro de la chapa
            d.tri([bcx - 5.0, bcy - 9.0], [bcx + 9.0, bcy], [bcx - 5.0, bcy + 9.0], tinta_boton);
        }
        // la casilla del A/B, dibujada (nada de ▣/▢ del sistema)
        let (qx, qy) = (bcx + 60.0, ty + 12.0);
        trazo::caja(d, qx, qy, 11.0, 11.0, 1.2, paleta::SAFE, 700);
        if self.visor.wipe {
            d.rect(qx + 2.5, qy + 2.5, 6.0, 6.0, paleta::SAFE_VIVO);
        }
        d.texto(qx + 18.0, qy - 1.0, "TIRA DE PRUEBA A/B (\\)", 11.0, paleta::SAFE);
    }

    /// clics del cuarto oscuro: baterías, agujas, baños, stocks, cajón, receta
    fn pulsa_cuarto(&mut self, pr: &mut Proyecto) {
        let (ancho, alto) = self.gpu.alto_ancho();
        let (mx, my) = self.raton;
        let panel_w = 310.0;
        let izq_w = 250.0;
        let ix = ancho - panel_w;
        let idx = self.bajo_aguja(pr);
        let fx = izq_w + 30.0;
        // ── LA AMPLIADORA (debajo del vidrio) ──────────────────────────
        // La misma geometría que la dibuja; se mira ANTES que nada porque
        // cae en el hueco entre el vidrio y el transporte, donde no hay
        // nada más que pulsar.
        {
            let zona_w = ancho - izq_w - panel_w - 60.0;
            let zona_h = alto - Self::CABECERA - 170.0;
            let prop = pr.proporcion();
            let (mut gw, mut gh) = (zona_w, zona_w / prop);
            if gh > zona_h { gh = zona_h; gw = gh * prop; }
            let gx = izq_w + 30.0 + (zona_w - gw) / 2.0;
            let gy = Self::CABECERA + 50.0 + (zona_h - gh) / 2.0;
            let (r_tam, r_pap, r_bot) = Self::ampliadora(gx, gy + gh, gw);
            let dentro = |r: (f32, f32, f32, f32)|
                mx >= r.0 && mx <= r.0 + r.2 && my >= r.1 && my <= r.1 + r.3;
            if dentro(r_tam) {
                self.master.copia_tam = (self.master.copia_tam + 1) % 3;
                prefs::guarda_master(&pr.base, &self.master);
                self.visor.foley(sonido::Foley::Tick);
                self.di(&format!("la copia sale {}",
                                 Self::COPIA_TAM[self.master.copia_tam as usize].0));
                return;
            }
            if dentro(r_pap) {
                self.master.copia_papel = (self.master.copia_papel + 1) % 3;
                prefs::guarda_master(&pr.base, &self.master);
                self.visor.foley(sonido::Foley::Tick);
                self.di(&format!("en {}",
                                 Self::COPIA_PAPEL[self.master.copia_papel as usize].0));
                return;
            }
            if dentro(r_bot) { self.saca_copia(pr); return; }
        }
        // el cajón abierto: poner gelatinas o cerrarlo
        if self.cajon {
            let cy0 = Self::CABECERA + 50.0;
            if mx >= fx + 300.0 && mx <= fx + 720.0 && my >= cy0 && my <= cy0 + 250.0 {
                let fila = if my < cy0 + 136.0 { 0 } else { 1 };
                let k = ((mx - fx - 312.0) / 100.0).floor();
                if k >= 0.0 && k < 4.0 {
                    let tipo = ["entrada", "color"][fila];
                    let dir = pr.base.join("luts").join(tipo);
                    let mut geles: Vec<std::path::PathBuf> = std::fs::read_dir(&dir).ok()
                        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path())
                             .filter(|p| p.extension().map(|x| x == "cube").unwrap_or(false))
                             .collect()).unwrap_or_default();
                    geles.sort();
                    if let (Some(g), Some(i)) = (geles.get(k as usize).cloned(), idx) {
                        self.recuerda(pr);
                        // ⇧: la gelatina va a TODA la bobina (y al proyecto,
                        // que es de donde la heredan los clips nuevos)
                        let todos = self.mods.shift_key();
                        let destinos: Vec<usize> = if todos { (0..pr.clips.len()).collect() }
                                                   else { vec![i] };
                        for j in destinos {
                            if let Some(c) = pr.clips.get_mut(j) {
                                if fila == 0 { c.lut_in = Some(g.clone()); }
                                else { c.lut_color = Some(g.clone()); }
                            }
                        }
                        if todos {
                            if fila == 0 { pr.lut_in = Some(g.clone()); }
                            else { pr.lut_color = Some(g.clone()); }
                        }
                        let _ = pr.guarda();
                        self.visor.marca_cuarto(i);
                        let nom = g.file_stem().map(|x| x.to_string_lossy().to_string())
                            .unwrap_or_default();
                        self.di(&if todos { format!("gelatina «{nom}» a TODA la bobina") }
                                 else { format!("gelatina «{nom}» a este clip") });
                    }
                }
                return;
            }
            self.cajon = false;
            return;
        }
        // el botón del cajón
        if mx >= fx + 506.0 && mx <= fx + 696.0 && my >= Self::CABECERA + 4.0
            && my <= Self::CABECERA + 44.0 {
            self.cajon = true;
            return;
        }
        // calcar / pegar la receta
        if my >= Self::CABECERA + 34.0 && my <= Self::CABECERA + 58.0 && mx >= ix + 148.0 {
            if mx <= ix + 196.0 {
                if let Some(c) = idx.and_then(|i| pr.clips.get(i)) {
                    self.receta = Some((c.prefs.clone(), c.lut_in.clone(), c.lut_color.clone()));
                    self.di("receta calcada (chincheta puesta)");
                }
                return;
            }
            if mx <= ix + 240.0 {
                if let (Some((prefs, li, lc)), Some(i)) = (self.receta.clone(), idx) {
                    self.recuerda(pr);
                    // ⇧: pegar la receta a TODOS los clips de la misma lata
                    if self.mods.shift_key() {
                        let media = pr.clips.get(i).map(|c| c.media.clone()).unwrap_or_default();
                        for c in pr.clips.iter_mut().filter(|c| c.media == media) {
                            c.prefs = prefs.clone();
                            c.lut_in = li.clone();
                            c.lut_color = lc.clone();
                        }
                        self.di("receta pegada a toda la lata");
                    } else if let Some(c) = pr.clips.get_mut(i) {
                        c.prefs = prefs;
                        c.lut_in = li;
                        c.lut_color = lc;
                        self.di("receta pegada");
                    }
                    let _ = pr.guarda();
                    self.visor.marca_cuarto(i);
                }
                return;
            }
        }
        // el panel de instrumentos: cabeceras pliegan, agujas se agarran
        if mx > ix - 10.0 {
            for (y, gi, ri) in self.filas_cuarto() {
                if my >= y - 2.0 && my < y + 30.0 {
                    match ri {
                        None => {
                            self.secciones[gi] = !self.secciones[gi];
                            self.visor.foley(sonido::Foley::Tick);
                        }
                        Some(ri) => {
                            // el interruptor del obturador (apaga el desenfoque
                            // de movimiento sin perder el valor que tenía)
                            let (cl0, _, _, _) = GRUPOS[gi].1[ri];
                            if cl0 == "shutter" && mx >= ix + 76.0 && mx <= ix + 112.0 {
                                if let Some(i) = idx {
                                    self.recuerda(pr);
                                    if let Some(c) = pr.clips.get_mut(i) {
                                        let v = c.prefs.get("shutter").and_then(|x| x.as_f64())
                                                 .unwrap_or(0.0);
                                        let guardado = c.prefs.get("shutterPrev")
                                                 .and_then(|x| x.as_f64()).unwrap_or(0.143);
                                        if let Some(o) = c.prefs.as_object_mut() {
                                            if v > 0.001 {
                                                o.insert("shutterPrev".into(), serde_json::json!(v));
                                                o.insert("shutter".into(), serde_json::json!(0.0));
                                            } else {
                                                o.insert("shutter".into(), serde_json::json!(guardado));
                                            }
                                        }
                                        let ahora = v <= 0.001;
                                        self.di(if ahora { "desenfoque de movimiento: encendido" }
                                                else { "desenfoque de movimiento: apagado" });
                                    }
                                    let _ = pr.guarda();
                                    self.visor.marca_cuarto(i);
                                    self.visor.foley(sonido::Foley::Tick);
                                }
                                return;
                            }
                            if self.mods.alt_key() {
                                // alt-clic: la aguja vuelve al valor del baño de la casa
                                let (clave, _, _, _) = GRUPOS[gi].1[ri];
                                if let Some(i) = idx {
                                    self.recuerda(pr);
                                    let def = prefs_de_la_casa();
                                    if let Some(c) = pr.clips.get_mut(i) {
                                        if let (Some(o), Some(v)) =
                                            (c.prefs.as_object_mut(), def.get(clave)) {
                                            o.insert(clave.to_string(), v.clone());
                                        }
                                    }
                                    let _ = pr.guarda();
                                    self.visor.marca_cuarto(i);
                                }
                            } else {
                                self.abre_gesto(pr);
                                self.arrastrando = Arrastre::Mando48(gi, ri);
                            }
                        }
                    }
                    return;
                }
            }
            return;
        }
        // los baños (izquierda): clic = verter
        if mx < izq_w {
            let sy0 = Self::CABECERA + 560.0;
            if my < sy0 - 10.0 {
                let col = if mx < 136.0 { 0 } else { 1 };
                let fila = ((my - Self::CABECERA - 48.0) / 168.0).floor();
                if fila >= 0.0 {
                    let k = fila as usize * 2 + col;
                    if k < BANOS.len() {
                        self.vierte_bano(pr, k);
                        return;
                    }
                }
            } else {
                // los stocks
                let col = if mx < 132.0 { 0 } else { 1 };
                let fila = ((my - sy0 - 30.0) / 58.0).floor();
                if fila >= 0.0 {
                    let k = fila as usize * 2 + col;
                    if k < STOCKS.len() {
                        self.pon_stock(pr, k);
                        return;
                    }
                }
            }
            return;
        }
        // la tira de contactos: saltar de plano sin salir del cuarto
        let sy = alto - 66.0;
        if my >= sy - 4.0 && my <= sy + 36.0 {
            let k = ((mx - fx) / 60.0).floor();
            if k >= 0.0 && (k as usize) < pr.clips.len().min(10) {
                let t = pr.inicios().get(k as usize).copied().unwrap_or(0.0);
                self.visor.busca(pr, t + 0.05);
                self.visor.foley(sonido::Foley::Tick);
                return;
            }
        }
        let ty = alto - 90.0;
        if my > ty && my < ty + 40.0 {
            if (mx - ancho / 2.0).abs() < 30.0 {
                self.visor.play_pausa(pr);
                return;
            }
            if mx > ancho / 2.0 + 50.0 && mx < ancho / 2.0 + 300.0 {
                self.visor.wipe = !self.visor.wipe;
                return;
            }
        }
        // el vidrio: play/pausa
        let [gx, gy, gw, gh] = self.visor.rect_pantalla;
        if mx >= gx && mx <= gx + gw && my >= gy && my <= gy + gh {
            self.visor.play_pausa(pr);
        }
    }

    /// EL REVELADO (NORTE §5) — la sala del resultado
    fn dibuja_revelado(&mut self, pr: &Proyecto, d: &mut ui::Dibujo, dt2: &mut ui::DibujoTex,
                       d2: &mut ui::Dibujo, ancho: f32, alto: f32) {
        use ui::Familia::*;
        self.dibuja_cabecera(pr, d, ancho, 2, false);
        let x0 = (ancho / 2.0 - 470.0).max(50.0);
        let mut y = Self::CABECERA + 36.0;
        // el titular, fuera de registro, con el sol (o la luna) del taller
        d.texto_f(Grot, x0 + 2.2, y + 2.2, "EL REVELADO", 40.0, paleta::ROJO);
        d.texto_f(Grot, x0, y, "EL REVELADO", 40.0, paleta::TINTA);
        let hora = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs()).unwrap_or(0) / 3600 + 1) % 24;
        let (scx, scy) = (x0 + 880.0, y + 24.0);
        if (8..21).contains(&hora) {
            trazo::circulo(d, scx, scy, 14.0, 14.0, 1.6, paleta::AMBAR, 400);
            for k in 0..8 {
                let a = k as f32 / 8.0 * std::f32::consts::TAU;
                trazo::linea(d, scx + a.cos() * 19.0, scy + a.sin() * 19.0,
                             scx + a.cos() * 27.0, scy + a.sin() * 27.0, 1.5, paleta::AMBAR,
                             401 + k as u32);
            }
        } else {
            trazo::circulo(d, scx, scy, 13.0, 13.0, 1.6, paleta::TINTA_TENUE, 400);
            trazo::circulo(d, scx + 6.0, scy - 3.0, 10.0, 10.0, 1.2, paleta::TINTA_TENUE, 402);
        }
        y += 56.0;
        // el parte
        let stem = |l: &Option<std::path::PathBuf>| -> String {
            l.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "—".into())
        };
        let gel = format!("{} + {}", stem(&pr.lut_in), stem(&pr.lut_color));
        d.texto(x0, y, &format!("DE LA MESA DE MONTAJE AL MÁSTER · {} EMPALME(S) · {:.1} S · GELATINAS: {}",
                                pr.clips.len(), pr.duracion().max(0.0),
                                gel.chars().take(52).collect::<String>()),
                10.0, paleta::TINTA);
        y += 34.0;
        // la etiqueta de la lata + el botón-sello
        d.rect_rot(x0, y + 16.0, 300.0, 40.0, -0.006, [1.0, 1.0, 1.0, 0.96]);
        self.objetos.quad_uv_rot(x0 + 230.0, y + 6.0, 56.0, 25.0, doodles::uv(doodles::CELO), 0.04);
        // la etiqueta se ESCRIBE a mano (clic para editarla)
        let puesto = self.etiqueta.clone().unwrap_or_else(|| pr.nombre.clone());
        let n: String = puesto.chars().take(20).collect();
        let n = if self.etiquetando { format!("{n}▏") } else { n };
        d.texto_f(Mano, x0 + 12.0, y + 20.0, &n, 22.0, paleta::TINTA);
        if self.etiquetando {
            trazo::linea(&mut *d, x0 + 10.0, y + 50.0, x0 + 290.0, y + 50.0, 1.6, paleta::ROJO, 540);
            d.texto(x0, y - 12.0, "ETIQUETA DE LA LATA · escribe · ⏎ o esc al terminar",
                    8.0, paleta::ROJO);
        } else {
            d.texto(x0, y - 12.0, "ETIQUETA DE LA LATA (clic para escribirla)", 8.0,
                    paleta::TINTA_TENUE);
        }
        // DÓNDE VA (la carpeta de destino, elegible)
        let dst = self.destino.clone()
            .unwrap_or_else(|| pr.base.join("out"));
        d.texto(x0 + 340.0, y - 12.0, "DÓNDE VA (clic: elegir carpeta)", 8.0, paleta::TINTA_TENUE);
        let dtxt: String = {
            let s2 = dst.to_string_lossy().to_string();
            if s2.chars().count() > 34 {
                format!("…{}", s2.chars().rev().take(33).collect::<String>()
                        .chars().rev().collect::<String>())
            } else { s2 }
        };
        trazo::caja(&mut *d, x0 + 340.0, y + 2.0, 270.0, 22.0, 1.1, paleta::TINTA_TENUE, 530);
        d.texto(x0 + 346.0, y + 7.0, &dtxt, 8.0, paleta::TINTA);
        let (bx, by, bw, bh) = (x0 + 340.0, y + 32.0, 270.0, 46.0);
        if self.revelando.is_none() {
            d.rect(bx + 4.0, by + 5.0, bw, bh, [0.1, 0.08, 0.06, 0.85]);
            d.rect_rot(bx, by, bw, bh, -0.008, paleta::ROJO);
            d.texto_f(Grot, bx + 26.0, by + 15.0, "REVELAR LA BOBINA", 17.0, paleta::HUESO);
        } else {
            let (pct, paso) = self.progreso.lock().map(|p| p.clone()).unwrap_or((0.0, String::new()));
            d.rect(bx, by, bw, bh, [0.922, 0.906, 0.863, 1.0]);
            trazo::caja(d, bx, by, bw, bh, 1.4, paleta::ROJO, 500);
            d.rect(bx + 4.0, by + bh - 14.0, (bw - 8.0) * pct.clamp(0.02, 1.0), 8.0, paleta::ROJO);
            let paso: String = paso.chars().take(30).collect();
            d.texto(bx + 8.0, by + 6.0, &format!("{:.0}% · {paso}", pct * 100.0), 10.0, paleta::TINTA);
            let gastado = self.revelado_desde.elapsed().as_secs_f32();
            if pct > 0.03 {
                let eta = gastado / pct * (1.0 - pct);
                d.texto(bx + 8.0, by + 20.0, &format!("quedan ~{:.0} s (clic: cancelar)", eta),
                        9.0, paleta::TINTA_TENUE);
            }
        }
        // los sellos del máster: SOLO los caminos que mastica el chip de
        // esta máquina (MOTOR §8bis). Ya no hay «el más rápido»: todos lo
        // son, porque los lentos no se dibujan. Van en REJILLA de dos
        // columnas: en fila se salían de la sala.
        let planta = Planta::de(ancho, alto, PRESETS_REVELADO.len());
        for (k, (nombre, sub, _)) in PRESETS_REVELADO.iter().enumerate() {
            let (sx, sy, sw2, sh2) = planta.sellos[k];
            let elegido = k == self.preset_revelado;
            if elegido {
                d.rect_rot(sx - 3.0, sy - 3.0, sw2 + 6.0, sh2 + 6.0, -0.006,
                           [0.851, 0.2, 0.145, 0.12]);
            }
            trazo::caja(d, sx, sy, sw2, sh2, if elegido { 1.8 } else { 1.1 },
                        if elegido { paleta::ROJO } else { paleta::TINTA_TENUE }, 520 + k as u32);
            d.texto_f(Grot, sx + 10.0, sy + 7.0, nombre, 13.0,
                      if elegido { paleta::ROJO } else { paleta::TINTA });
            d.texto(sx + 10.0, sy + 28.0, sub, 8.0, paleta::TINTA_TENUE);
        }
        y = planta.parte.1;
        // ── EL PARTE DE SALIDA y la llave del cajón ────────────────────
        {
            let (sw, sh, cw, ch) = self.medidas_master(pr);
            let sx = planta.parte.0;
            let fps = if (pr.fps - pr.fps.round()).abs() < 0.01 { format!("{:.0}", pr.fps) }
                      else { format!("{:.2}", pr.fps) };
            d.texto(sx + 4.0, y + 7.0, &format!("SALE A {sw}×{sh} · {fps} FPS"), 9.0, paleta::TINTA);
            let a_mano = self.preset_revelado == A_MANO;
            d.texto(sx + 4.0, y + 20.0,
                    &if !a_mano {
                        "el lienzo de la bobina · directo, sin escalar".to_string()
                    } else if (cw, ch) != (sw, sh) {
                        format!("se revela a {cw}×{ch} · {}",
                                if cw > sw { "supermuestreo" } else { "y se agranda" })
                    } else {
                        "a mano · al lienzo, sin escalar".to_string()
                    },
                    8.0, if a_mano { paleta::ROJO } else { paleta::TINTA_TENUE });
            // la casilla del sonido, que no toca el vídeo
            use std::sync::atomic::Ordering;
            let norm = prefs::NORMALIZA.load(Ordering::Relaxed);
            let (cx, cy) = (planta.normaliza.0, planta.normaliza.1);
            trazo::caja(&mut *d, cx, cy, 13.0, 13.0, 1.3, paleta::TINTA, 566);
            if norm {
                trazo::linea(&mut *d, cx + 2.5, cy + 7.0, cx + 5.5, cy + 10.5, 1.7, paleta::ROJO, 567);
                trazo::linea(&mut *d, cx + 5.5, cy + 10.5, cx + 11.5, cy + 1.5, 1.7, paleta::ROJO, 568);
            }
            d.texto(cx + 20.0, cy + 3.0, "normalizar el sonido", 8.5,
                    if norm { paleta::TINTA } else { paleta::TINTA_TENUE });
            // LA LLAVE DEL CAJÓN
            let (bx2, by2) = (planta.llave.0, planta.llave.1);
            let en_uso = self.preset_revelado == A_MANO;
            trazo::caja(&mut *d, bx2, by2, 186.0, 20.0, if self.cajon_master { 1.8 } else { 1.1 },
                        if en_uso { paleta::ROJO } else { paleta::TINTA }, 569);
            d.texto(bx2 + 8.0, by2 + 5.0,
                    match (self.cajon_master, en_uso) {
                        (true, _) => "CERRAR EL CAJÓN",
                        (false, true) => "EL CAJÓN (en uso)",
                        _ => "EL CAJÓN (sin usar)",
                    },
                    8.5, if en_uso { paleta::ROJO } else { paleta::TINTA });
        }

        // ── EL RANGO, AQUÍ MISMO (§15) ─────────────────────────────────
        // Se marcaba con ⇧I y ⇧O en la mesa y en esta sala no había forma de
        // saber siquiera si estaba puesto. Ahora es una regla: la bobina
        // entera de izquierda a derecha, el tramo elegido en rojo y dos
        // tiradores que se arrastran. Lo que se revela es lo que se ve.
        {
            let (rx, ry, rw, rh) = planta.rango;
            let dur = pr.duracion().max(0.001);
            let (ra, rb) = pr.tramo();
            let hay = pr.rango.is_some();
            d.texto(rx, ry - 12.0, "QUÉ SE REVELA (arrastra los tiradores)", 8.0,
                    paleta::TINTA_TENUE);
            let (bx3, bw3) = (rx, rw - 180.0);
            d.rect(bx3, ry + 12.0, bw3, 10.0, [0.80, 0.78, 0.72, 1.0]);
            let xa = bx3 + bw3 * (ra / dur) as f32;
            let xb = bx3 + bw3 * (rb / dur) as f32;
            d.rect(xa, ry + 12.0, (xb - xa).max(2.0), 10.0,
                   if hay { paleta::ROJO } else { paleta::TINTA_TENUE });
            // las juntas de la bobina, marcadas en la regla: pegar el rango a
            // un empalme es lo que uno quiere el 90 % de las veces
            let mut acc = 0.0f64;
            for c in &pr.clips {
                acc += c.dur();
                let x = bx3 + bw3 * (acc / dur) as f32;
                d.rect(x, ry + 8.0, 1.0, 18.0, [0.0, 0.0, 0.0, 0.25]);
            }
            for (k, x) in [xa, xb].iter().enumerate() {
                d.rect(x - 3.0, ry + 6.0, 6.0, 22.0, paleta::TINTA);
                d.texto(x - 8.0, ry + 30.0, if k == 0 { "in" } else { "out" }, 7.0,
                        paleta::TINTA_TENUE);
            }
            let etq = if hay {
                format!("del {ra:.2} s al {rb:.2} s · {:.2} s · {} fotograma(s)",
                        rb - ra, ((rb - ra) * pr.fps.max(1.0)).round() as i64)
            } else {
                format!("la bobina entera · {dur:.2} s")
            };
            d.texto(rx + rw - 174.0, ry + 4.0, &etq.chars().take(40).collect::<String>(),
                    8.0, if hay { paleta::ROJO } else { paleta::TINTA });
            trazo::caja(&mut *d, rx + rw - 174.0, ry + 18.0, 120.0, 18.0, 1.1,
                        if hay { paleta::TINTA } else { paleta::TINTA_TENUE }, 575);
            d.texto(rx + rw - 168.0, ry + 22.0,
                    if hay { "TODA LA BOBINA" } else { "(ya está entera)" }, 8.0,
                    if hay { paleta::TINTA } else { paleta::TINTA_TENUE });
        }

        y = planta.cubetas_y;
        // las cubetas del baño
        let nombres = ["revelador", "baño de paro", "fijador", "lavado"];
        let aguas: [[f32; 4]; 4] = [[0.45, 0.38, 0.22, 1.0], [0.72, 0.62, 0.38, 1.0],
                                    [0.62, 0.66, 0.58, 1.0], [0.55, 0.66, 0.72, 1.0]];
        let reloj = self.revelado_desde.elapsed().as_secs_f32();
        for k in 0..4 {
            let cx = x0 + k as f32 * 240.0;
            self.objetos.quad_uv_rot(cx, y, 224.0, 130.0, doodles::uv(doodles::CUBETA),
                                     ((k * 13 % 5) as f32 - 2.0) * 0.006);
            let mut agua = aguas[k];
            agua[3] = 0.88;
            d2.rect(cx + 14.0, y + 26.0, 196.0, 62.0, agua);
            // el líquido ondula mientras hay revelado en marcha
            if self.revelando.is_some() {
                for o in 0..3 {
                    let fase = reloj * 2.2 + k as f32 * 1.3 + o as f32 * 2.1;
                    let oy = y + 34.0 + o as f32 * 16.0 + fase.sin() * 3.0;
                    d2.rect(cx + 18.0 + (fase * 0.7).cos() * 6.0 + 6.0 * o as f32, oy,
                            160.0 - 12.0 * o as f32, 1.5, [1.0, 1.0, 1.0, 0.18]);
                }
            }
            d2.texto_f(Mano, cx + 20.0, y + 96.0, nombres[k], 15.0, paleta::TINTA);
        }
        // la TIRA pasando por las cubetas: el progreso ES la película
        if self.revelando.is_some() {
            let (pct, _) = self.progreso.lock().map(|p| p.clone()).unwrap_or((0.0, String::new()));
            let span = 4.0 * 240.0 + 160.0;
            let cabeza = x0 - 120.0 + span * pct.clamp(0.0, 1.0);
            let n_f = 7usize;
            for k in 0..n_f {
                let fx2 = cabeza - k as f32 * 62.0;
                if fx2 < x0 - 130.0 || fx2 > x0 + 4.0 * 240.0 { continue; }
                let hund = ((fx2 - x0) / 240.0).fract();
                let dy2 = if (x0..x0 + 960.0).contains(&fx2) && hund > 0.15 && hund < 0.75
                          { 10.0 + (reloj * 3.0 + k as f32).sin() * 2.0 } else { -6.0 };
                let ci = k % pr.clips.len().max(1);
                d2.rect(fx2 - 2.0, y - 26.0 + dy2, 62.0, 40.0, paleta::PELICULA);
                if let Some(c) = pr.clips.get(ci) {
                    if !c.hueco {
                        let proxy = pr.base.join(".proxies").join(&c.media);
                        let ruta = if proxy.is_file() { proxy } else { c.ruta.clone() };
                        let clave = (format!("cont:{}:{ci}", c.media), (c.t_in * 100.0) as u32);
                        if let Some(slot) = self.minis.pide(clave, &ruta, c.t_in + 0.2) {
                            dt2.quad(fx2, y - 24.0 + dy2, 58.0, 34.0, slot, 1.0);
                        }
                    }
                }
                for pk in 0..4 {
                    d2.rect(fx2 + 4.0 + pk as f32 * 15.0, y - 25.0 + dy2, 6.0, 3.5,
                            [0.9, 0.88, 0.8, 0.9]);
                }
            }
            self.ventana.request_redraw();
        }
        y += 150.0;
        // ── LA CUERDA DE SECADO: la galería de lo revelado ──
        trazo::linea(&mut *d, x0 - 20.0, y + 14.0, x0 + 960.0, y + 6.0, 1.6, paleta::TINTA, 600);
        d.texto_f(Mano, x0 + 700.0, y + 18.0, "aquí se cuelga lo revelado", 14.0, paleta::TINTA_TENUE);
        let out = pr.base.join("out");
        let mut reveladas: Vec<std::path::PathBuf> = std::fs::read_dir(&out).ok().map(|rd| {
            rd.flatten().map(|e| e.path())
              .filter(|p| p.extension().map(|x| x == "mp4" || x == "mov").unwrap_or(false))
              .collect()
        }).unwrap_or_default();
        reveladas.sort_by_key(|p| std::cmp::Reverse(
            p.metadata().and_then(|m| m.modified()).ok()));
        for (k, r) in reveladas.iter().take(5).enumerate() {
            let px2 = x0 + 20.0 + k as f32 * 190.0;
            let cuelga_y = y + 10.0 + (k % 2) as f32 * 4.0;
            let ang = ((k * 29 % 5) as f32 - 2.0) * 0.02;
            let nombre = r.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
            self.objetos.quad_uv_rot(px2 + 56.0, cuelga_y, 16.0, 35.0, doodles::uv(doodles::PINZA), 0.02);
            // el fotograma-póster colgado
            d.rect_rot(px2, cuelga_y + 30.0, 132.0, 96.0, ang, [1.0, 1.0, 1.0, 0.96]);
            let clave = (format!("seca:{nombre}"), 100);
            if let Some(slot) = self.minis.pide(clave, r, 1.0) {
                dt2.quad(px2 + 6.0, cuelga_y + 36.0, 120.0, 68.0, slot, 1.0);
            } else {
                d.rect(px2 + 6.0, cuelga_y + 36.0, 120.0, 68.0, paleta::PELICULA);
            }
            let n: String = nombre.chars().take(17).collect();
            d.texto_f(Mano, px2 + 8.0, cuelga_y + 104.0, &n, 13.0, paleta::TINTA);
            if k == 0 {
                d.texto(px2 + 8.0, cuelga_y + 126.0, "arrástralo fuera · ⌥clic: copiar", 7.0,
                        paleta::TINTA_TENUE);
            }
            // el sello REVELADA (el último, con su animación de estampado)
            let escala = if k == 0 {
                self.sello_en.map(|t0| {
                    let f = (t0.elapsed().as_secs_f32() / 0.4).min(1.0);
                    if f < 1.0 { self.ventana.request_redraw(); }
                    2.2 - 1.2 * f * (2.0 - f)
                }).unwrap_or(1.0)
            } else { 1.0 };
            if k == 0 || true {
                let sw = 66.0 * escala;
                let sx2 = px2 + 66.0 - sw / 2.0;
                let sy2 = cuelga_y + 66.0 - 9.0 * escala;
                d2.texto_f(Grot, sx2 + 4.0, sy2 + 2.0, "REVELADA", 11.0 * escala,
                           [0.851, 0.2, 0.145, 0.82]);
                trazo::caja(d2, sx2, sy2, sw, 18.0 * escala, 1.2,
                            [0.851, 0.2, 0.145, 0.66], 601 + k as u32);
            }
        }
        if reveladas.is_empty() {
            d.texto(x0 + 30.0, y + 40.0, "(nada tendido aún — revela la bobina)", 9.0,
                    paleta::TINTA_TENUE);
        }
        // la pared del autor también viste esta sala
        self.pega_pared(ancho, alto, Self::CABECERA + 340.0);
        // la mosca pasea por la cuerda (cada medio minuto se posa en otro sitio)
        {
            let tic = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs()).unwrap_or(0) / 31;
            let fmx = x0 + 80.0 + ((tic * 397) % 800) as f32;
            let fmy = y + 9.0 - ((tic * 131) % 5) as f32;
            let tinta_mosca = [0.12, 0.10, 0.08, 0.9];
            d.rect_rot(fmx, fmy, 5.0, 3.5, 0.4, tinta_mosca);
            trazo::linea(&mut *d, fmx + 4.0, fmy - 1.0, fmx + 8.0, fmy - 4.0, 1.0, tinta_mosca, 950);
            trazo::linea(&mut *d, fmx + 1.0, fmy - 1.0, fmx + 4.0, fmy - 5.0, 1.0, tinta_mosca, 951);
            for pk in 0..3 {
                d.rect(fmx - 2.0 - pk as f32 * 4.0, fmy + 3.0 + (pk % 2) as f32 * 2.0, 1.5, 1.5,
                       [0.12, 0.10, 0.08, 0.35]);
            }
        }
        // el colofón
        let cy = alto - 92.0;
        trazo::linea(&mut *d, x0, cy - 10.0, x0 + 420.0, cy - 12.0, 1.4, paleta::TINTA, 610);
        d.texto_f(Grot, x0, cy, "COLOFÓN", 11.0, paleta::TINTA);
        for (k, linea) in ["— hecho a mano en los Laboratorios Saorín · dos tintas sobre papel hueso",
                           "— el motor del look: el mismo del laboratorio, al fotograma",
                           "— LAB · SAORIN · 2026"].iter().enumerate() {
            d.texto_f(Serif, x0, cy + 18.0 + k as f32 * 15.0, linea, 10.0, paleta::TINTA);
        }
    }

    /// el fantasma del arrastre: un PNG con el fotograma del máster
    fn png_de_revelada(&self, r: &std::path::Path) -> Option<Vec<u8>> {
        let cache = r.with_extension("poster.png");
        if let Ok(b) = std::fs::read(&cache) {
            return Some(b);
        }
        let salida = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-ss", "1", "-i"])
            .arg(r)
            .args(["-frames:v", "1", "-vf", "scale=192:-2", "-y"])
            .arg(&cache)
            .status().ok()?;
        salida.success().then(|| std::fs::read(&cache).ok()).flatten()
    }

    /// clics de la sala de revelado: sellos, botón, la cuerda
    fn pulsa_revelado(&mut self, pr: &mut Proyecto) {
        let (ancho, alto) = self.gpu.alto_ancho();
        let (mx, my) = self.raton;
        // LA MISMA PLANTA QUE EL DIBUJO. Aquí estaba el fallo de fondo de esta
        // sala: los dos lados llevaban los números a mano y se separaban.
        let p = Planta::de(ancho, alto, PRESETS_REVELADO.len());
        let x0 = p.x0;
        // LA ETIQUETA DE LA LATA: clic para escribirla
        if Planta::dentro(p.etiqueta, mx, my) {
            if !self.etiquetando {
                self.etiqueta = Some(self.etiqueta.clone().unwrap_or_else(|| pr.nombre.clone()));
                self.etiquetando = true;
                self.visor.foley(sonido::Foley::Tick);
            }
            return;
        }
        if self.etiquetando {
            // clic fuera: cerrar la edición
            self.etiquetando = false;
        }
        // la ficha de DESTINO: elegir dónde va el máster
        if Planta::dentro(p.destino, mx, my) {
            if let Some(dir) = rfd::FileDialog::new()
                .set_title("¿dónde cuelgo el máster?")
                .set_directory(self.destino.clone().unwrap_or_else(|| pr.base.join("out")))
                .pick_folder() {
                self.destino = Some(dir.clone());
                prefs::guarda_destino(&pr.base, Some(&dir));
                self.di(&format!("el máster irá a {}", dir.display()));
            }
            return;
        }
        // el botón (o cancelar si está en marcha)
        if Planta::dentro(p.boton, mx, my) {
            if let Some(mut nino) = self.revelando.take() {
                let _ = nino.kill();
                self.di("revelado cancelado (la tira, fuera)");
            } else {
                let p = self.preset_revelado;
                self.revela(pr, p, None);
            }
            return;
        }
        // ── EL CAJÓN DEL MÁSTER, si está abierto, manda ────────────────
        if self.cajon_master {
            let (cx2, cy) = p.cajon;
            if Planta::dentro((cx2, cy, 700.0, 248.0), mx, my) {
                self.toca_cajon_master(pr, mx, my, cx2, cy);
                return;
            }
        }
        // la llave del cajón
        if Planta::dentro(p.llave, mx, my) {
            self.cajon_master = !self.cajon_master;
            self.visor.foley(sonido::Foley::Tick);
            return;
        }
        // ── EL RANGO: los dos tiradores y el «toda la bobina» ──────────
        {
            let (rx, ry, rw, rh) = p.rango;
            if Planta::dentro((rx + rw - 174.0, ry + 18.0, 120.0, 18.0), mx, my) {
                if self.rango_quita(pr) { return; }
            }
            if Planta::dentro((rx, ry, rw - 180.0, rh), mx, my) {
                let (bx3, bw3) = (rx, rw - 180.0);
                let dur = pr.duracion().max(0.001);
                let (ra, rb) = pr.tramo();
                let xa = bx3 + bw3 * (ra / dur) as f32;
                let xb = bx3 + bw3 * (rb / dur) as f32;
                // el tirador más cercano manda; la regla completa da el ancho
                let k = if (mx - xa).abs() <= (mx - xb).abs() { 0u8 } else { 1u8 };
                self.regla_rango = Some((bx3, bw3));
                self.arrastrando = Arrastre::RangoSala(k);
                self.visor.foley(sonido::Foley::Tick);
                return;
            }
        }
        // los sellos del máster
        {
            if let Some(k) = p.sellos.iter().position(|r| Planta::dentro(*r, mx, my)) {
                self.preset_revelado = k;
                // el cajón solo se abre solo cuando toca usarlo
                self.cajon_master = k == A_MANO;
                self.visor.foley(sonido::Foley::Tick);
                let (sw, sh, cw, ch) = self.medidas_master(pr);
                self.di(&if k == A_MANO {
                    format!("a mano: sale a {sw}×{sh} y se revela a {cw}×{ch}")
                } else if k == EN_CLIPS {
                    "en clips: una carpeta con un fichero por plano".to_string()
                } else {
                    format!("{}: al lienzo de la bobina, sin escalar", PRESETS_REVELADO[k].0)
                });
                return;
            }
        }
        // la casilla de normalizar el sonido (a la derecha de los sellos)
        {
            use std::sync::atomic::Ordering;
            let (cx, cy) = (p.normaliza.0, p.normaliza.1);
            if mx >= cx - 2.0 && mx <= cx + 150.0 && my >= cy - 3.0 && my <= cy + 18.0 {
                let v = !prefs::NORMALIZA.load(Ordering::Relaxed);
                prefs::NORMALIZA.store(v, Ordering::Relaxed);
                prefs::guarda(&pr.base);
                self.visor.foley(sonido::Foley::Tick);
                self.di(if v { "el máster saldrá con la sonoridad normalizada" }
                        else { "el máster saldrá con el sonido tal cual" });
                return;
            }
        }
        // la cuerda: clic en un póster = enseñarlo en el Finder/Explorer
        let y_cuerda = p.cubetas_y + 150.0;
        if my >= y_cuerda + 40.0 && my <= y_cuerda + 140.0 {
            let k = ((mx - x0 - 20.0) / 190.0).floor();
            if k >= 0.0 {
                let out = pr.base.join("out");
                let mut reveladas: Vec<std::path::PathBuf> = std::fs::read_dir(&out).ok()
                    .map(|rd| rd.flatten().map(|e| e.path())
                         .filter(|p| p.extension().map(|x| x == "mp4" || x == "mov")
                                 .unwrap_or(false)).collect()).unwrap_or_default();
                reveladas.sort_by_key(|p| std::cmp::Reverse(
                    p.metadata().and_then(|m| m.modified()).ok()));
                if let Some(r) = reveladas.get(k as usize) {
                    // ── ARRASTRAR FUERA: el máster sale de la cuerda al
                    //    Finder / Explorador / a otra app (arrastre del
                    //    sistema de verdad, NORTE §7.16)
                    if !self.mods.alt_key() && !self.mods.super_key() {
                        // el fantasma: la miniatura del propio render
                        let mini = self.png_de_revelada(r);
                        if arrastre_fuera::arrastra(&self.ventana, r, mini.as_deref()) {
                            self.di("suéltalo donde quieras");
                            return;
                        }
                    }
                    if self.mods.alt_key() || self.mods.super_key() {
                        // ⌥/⌘ + clic: el máster AL PORTAPAPELES como fichero
                        // (se pega en el Finder/Explorer con ⌘V — da lo que
                        // daría arrastrarlo fuera, sin FFI de Cocoa)
                        #[cfg(target_os = "macos")]
                        let ok = std::process::Command::new("osascript")
                            .arg("-e")
                            .arg(format!("set the clipboard to POSIX file \"{}\"", r.display()))
                            .status().map(|s| s.success()).unwrap_or(false);
                        #[cfg(target_os = "windows")]
                        let ok = std::process::Command::new("powershell")
                            .args(["-NoProfile", "-Command",
                                   &format!("Set-Clipboard -Path '{}'", r.display())])
                            .status().map(|s| s.success()).unwrap_or(false);
                        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                        let ok = false;
                        self.di(if ok { "máster al portapapeles: pégalo donde quieras" }
                                else { "no pude copiarlo" });
                        return;
                    }
                    #[cfg(target_os = "macos")]
                    let _ = std::process::Command::new("open").arg("-R").arg(r).spawn();
                    #[cfg(target_os = "windows")]
                    let _ = std::process::Command::new("explorer")
                        .arg(format!("/select,{}", r.display())).spawn();
                    return;
                }
            }
        }
        let _ = alto;
    }

    /// EL ALTO DE UNA FILA de la lista de bobinas (dibujo y ratón, el mismo)
    const BOBINA_FILA: f32 = 46.0;

    /// LA VENTANA DE LAS BOBINAS (§1).
    ///
    /// La portada del taller vive dentro de la ventana principal y para
    /// cambiar de bobina hay que salir de la mesa. Esto es la misma lista en
    /// una ventana propia: se deja abierta en un lado —o en el otro monitor—
    /// y saltar de una bobina a otra es un clic, sin perder de vista la que
    /// estás montando. Sin carteles: esta ventana no tiene capa de texturas,
    /// y prometer una miniatura que no se puede pintar sería mentir.
    fn dibuja_bobinas_en(&self, pr: &Proyecto, d: &mut ui::Dibujo, ancho: f32, alto: f32) {
        use ui::Familia::*;
        d.texto_f(Grot, 16.0, 12.0, "LAS BOBINAS DEL TALLER", 12.0, paleta::TINTA);
        trazo::subraya(d, 16.0, ancho - 16.0, 30.0, 1.2, paleta::TINTA_TENUE, 6);
        if self.bobinas.is_empty() {
            d.texto(16.0, 56.0, "no hay ninguna todavía", 9.0, paleta::TINTA_TENUE);
            return;
        }
        let mut y = 46.0;
        for b in &self.bobinas {
            if y > alto - 20.0 { break; }
            let abierta = b.nombre == pr.nombre;
            if abierta {
                d.rect(8.0, y - 4.0, ancho - 16.0, Self::BOBINA_FILA - 4.0,
                       [0.851, 0.2, 0.145, 0.10]);
            }
            d.texto_f(Mano, 18.0, y, &b.nombre.chars().take(26).collect::<String>(), 17.0,
                      if abierta { paleta::ROJO } else { paleta::TINTA });
            d.texto(18.0, y + 22.0,
                    &format!("{} clip(s) · {:.0} s · {}", b.clips, b.dur, b.formato),
                    8.0, paleta::TINTA_TENUE);
            if abierta {
                d.texto(ancho - 74.0, y + 6.0, "ABIERTA", 8.0, paleta::ROJO);
            }
            trazo::linea(d, 12.0, y + Self::BOBINA_FILA - 8.0, ancho - 12.0,
                         y + Self::BOBINA_FILA - 8.0, 1.0, paleta::TINTA_TENUE, 900);
            y += Self::BOBINA_FILA;
        }
        d.texto(16.0, alto - 18.0, "clic en una para abrirla", 8.0, paleta::TINTA_TENUE);
    }

    /// clic en la fila `fila` de la lista de bobinas
    fn toca_bobina(&mut self, pr: &Proyecto, fila: i32) -> AccionPortada {
        if fila < 0 { return AccionPortada::Nada; }
        let Some(b) = self.bobinas.get(fila as usize) else { return AccionPortada::Nada };
        if b.nombre == pr.nombre { return AccionPortada::Continuar; }
        self.visor.foley(sonido::Foley::Lata);
        AccionPortada::Abrir(b.clave.clone())
    }

    fn dibuja_chuleta(&self, d2: &mut ui::Dibujo, ancho: f32, alto: f32) {
        self.dibuja_chuleta_en(d2, ancho, alto, true);
    }

    /// LA CHULETA. Sale **del mismo sitio que el menú** (§5): los gestos que
    /// tienen entrada en la barra se leen de `menu::MENUS`, así que no pueden
    /// divergir; debajo van los que solo son gestos de ratón, que ningún menú
    /// puede contar.
    fn dibuja_chuleta_en(&self, d2: &mut ui::Dibujo, ancho: f32, alto: f32, modal: bool) {
        use ui::Familia::*;
        let (w, h, x, y) = if modal {
            d2.rect(0.0, 0.0, ancho, alto, [0.10, 0.10, 0.08, 0.45]);
            let (w, h) = (800.0f32.min(ancho - 40.0), 640.0f32.min(alto - 40.0));
            let (x, y) = ((ancho - w) / 2.0, (alto - h) / 2.0);
            d2.rect(x + 5.0, y + 7.0, w, h, [0.0, 0.0, 0.0, 0.2]);
            d2.rect(x, y, w, h, [0.965, 0.953, 0.918, 1.0]);
            (w, h, x, y)
        } else {
            (ancho, alto, 0.0, 0.0)
        };
        d2.rect(x, y, w, 3.0, paleta::TINTA);
        d2.texto_f(Grot, x + 28.0, y + 18.0, "LA CHULETA", 22.0, paleta::TINTA);
        d2.texto_f(Mano, x + 190.0, y + 16.0, "los gestos del taller", 20.0, paleta::TINTA_TENUE);

        // LOS GESTOS DE RATÓN, que no caben en una barra de menú
        let manos: [(&str, &str); 21] = [
            ("arrastrar de la estantería", "la cinta entra donde la sueltes"),
            ("doble toque en una lata", "la cinta entera, al final"),
            ("un toque en una lata", "monitor de fuente (I/O · ⏎)"),
            ("arrastrar un clip al cubo", "guardarlo para luego"),
            ("arrastrar del cubo", "colocarlo donde lo sueltes"),
            ("clic en la cinta de empalme", "fundido 0 / 0,5 / 1 / 2 s"),
            ("arrastrar la aguja", "scrub audible (la moviola)"),
            ("rueda sobre la bobina", "recorrerla · ⌘+rueda: la lupa"),
            ("rueda sobre la manivela", "fotograma (⇧ s · ⌥ empalme)"),
            ("⌥ sobre el vidrio", "la lupa cuentahílos ×4"),
            ("doble clic en el visor", "pantalla completa"),
            ("en encuadre: esquinas", "escala (⇧ uniforme · ⌥ del ancla)"),
            ("en encuadre: ✛", "el ancla, arrastrable"),
            ("en encuadre: rueda", "amplía sobre el puntero"),
            ("arrastrar un número", "⇧ fino · ⌥ grueso · 2 clics: teclear"),
            ("alt-clic en un número", "vuelve a su valor limpio"),
            ("⇧+clic en una música", "cambiarla de carril"),
            ("clic derecho en una lata", "renombrarla (⇧: quitarla)"),
            ("clic derecho en una bobina", "renombrar · duplicar · borrar"),
            ("esc", "quita la cuchilla, luego el encuadre,"),
            ("  (va por capas)", "luego la selección, y sale a la portada"),
        ];
        let filas = ((h - 108.0) / 30.0).max(4.0) as usize;
        let mut col = 0usize;
        let mut fila = 0usize;
        let mut pon = |d2: &mut ui::Dibujo, tecla: &str, que: &str, col: usize, fila: usize| {
            let xx = x + 28.0 + col as f32 * (w - 56.0) / 3.0;
            let yy = y + 62.0 + fila as f32 * 30.0;
            let t: String = tecla.chars().take(30).collect();
            let q: String = que.chars().take(34).collect();
            d2.texto(xx, yy, &t, 10.5, paleta::ROJO);
            d2.texto(xx, yy + 13.0, &q, 8.5, paleta::TINTA_TENUE);
        };
        // primero, LO QUE DICE EL MENÚ (una sola fuente de verdad)
        for m in menu::MENUS {
            for en in m.entradas {
                if en.accion.is_none() || en.atajo.is_empty() { continue; }
                if fila >= filas { fila = 0; col += 1; }
                if col > 2 { break; }
                pon(d2, en.atajo, en.texto, col, fila);
                fila += 1;
            }
        }
        if fila > 0 { fila = 0; col += 1; }
        for (tecla, que) in manos.iter() {
            if fila >= filas { fila = 0; col += 1; }
            if col > 2 { break; }
            pon(d2, tecla, que, col, fila);
            fila += 1;
        }
        d2.texto(x + 28.0, y + h - 26.0,
                 if modal { "cualquier tecla la cierra · Ventanas → «la chuleta, aparte»" }
                 else { "esta chuleta se genera del menú: no pueden divergir" },
                 10.0, paleta::TINTA_TENUE);
    }

    /// ajustes (⌘,): la app se explica — motor, formato, taller, caché
    fn dibuja_ajustes(&self, pr: &Proyecto, d2: &mut ui::Dibujo, ancho: f32, alto: f32) {
        self.dibuja_ajustes_en(pr, d2, ancho, alto, true);
    }

    /// EL PANEL DE AJUSTES DE VERDAD (§4). Dónde vive el taller, cuánto ocupa
    /// la caché y un botón para vaciarla, el motor detectado, el sonido, el
    /// imán, la normalización, las copias del archivador y **los caminos de
    /// revelado que hay en ESTA máquina**.
    fn dibuja_ajustes_en(&self, pr: &Proyecto, d2: &mut ui::Dibujo,
                         ancho: f32, alto: f32, modal: bool) {
        use ui::Familia::*;
        let (w, h, x, y) = if modal {
            d2.rect(0.0, 0.0, ancho, alto, [0.10, 0.10, 0.08, 0.45]);
            let (w, h) = (620.0f32.min(ancho - 40.0), 620.0f32.min(alto - 40.0));
            let (x, y) = ((ancho - w) / 2.0, (alto - h) / 2.0);
            d2.rect(x + 5.0, y + 7.0, w, h, [0.0, 0.0, 0.0, 0.2]);
            d2.rect(x, y, w, h, [0.965, 0.953, 0.918, 1.0]);
            (w, h, x, y)
        } else {
            (ancho, alto, 0.0, 0.0)
        };
        d2.rect(x, y, w, 3.0, paleta::TINTA);
        d2.texto_f(Grot, x + 28.0, y + 18.0, "AJUSTES", 22.0, paleta::TINTA);
        d2.texto_f(Mano, x + 160.0, y + 16.0, "el taller, a la vista", 20.0, paleta::TINTA_TENUE);
        for (k, (nombre, valor, tocable)) in self.filas_ajustes(pr).iter().enumerate() {
            let yy = y + Self::AJUSTES_Y0 + k as f32 * Self::AJUSTES_FILA;
            if yy > y + h - 40.0 { break; }
            d2.texto(x + 28.0, yy, nombre, 11.0,
                     if *tocable { paleta::ROJO } else { paleta::TINTA });
            let v: String = valor.chars().take(((w - 60.0) / 5.6) as usize).collect();
            d2.texto(x + 28.0, yy + 14.0, &v, 10.0, paleta::TINTA_TENUE);
        }
        d2.texto(x + 28.0, y + h - 24.0,
                 if modal { "lo ROJO se toca · cualquier tecla cierra · ? la chuleta" }
                 else { "lo ROJO se toca · esta ventana se recuerda donde la dejes" },
                 10.0, paleta::TINTA_TENUE);
    }

    const AJUSTES_Y0: f32 = 56.0;
    const AJUSTES_FILA: f32 = 32.0;

    /// LAS FILAS DE AJUSTES: (rótulo, valor, ¿se toca?). Una sola lista para
    /// dibujar y para pulsar — si divergen, el panel miente.
    fn filas_ajustes(&self, pr: &Proyecto) -> Vec<(String, String, bool)> {
        use std::sync::atomic::Ordering;
        let motor = if cfg!(target_os = "macos") { "VideoToolbox (nativo, en proceso)" }
                    else if cfg!(target_os = "windows") { "Media Foundation (nativo, en proceso)" }
                    else { "ffmpeg (compatibilidad)" };
        let (bytes, piezas) = Self::pesa_cache(&pr.base);
        let copias = Self::cuenta_copias(&pr.base);
        let caminos: Vec<&str> = PRESETS_REVELADO.iter().map(|p| p.0).collect();
        vec![
        ("motor de cine".into(), motor.to_string(), false),
        ("caminos de revelado EN ESTA MÁQUINA".into(),
         format!("{} · el resto no se dibuja porque iría por software",
                 caminos.join(" · ")), false),
        ("dónde vive el taller (clic: mostrarlo)".into(),
         pr.base.to_string_lossy().to_string(), true),
        ("dónde va el máster (clic: elegir)".into(),
         self.destino.clone().unwrap_or_else(|| pr.base.join("out"))
             .to_string_lossy().to_string(), true),
        ("LA CACHÉ (clic: vaciarla)".into(),
         format!("{piezas} pieza(s) · {:.1} GB en {}", bytes as f64 / 1e9,
                 pr.base.join(".cache").display()), true),
        ("el archivador (clic: recuperar una copia)".into(),
         format!("{copias} copia(s) en backups/ · una cada 5 min, quedan las 10 últimas"),
         true),
        ("preview (clic cambia)".into(), if prefs::PREVIEW_MEDIA.load(Ordering::Relaxed) {
            "media resolución (el refinado en pausa: completa)".into()
        } else { "COMPLETA también en movimiento (más caro)".to_string() }, true),
        ("sonido del taller (clic cambia)".into(), if prefs::FOLEY.load(Ordering::Relaxed) {
            "foley del oficio ACTIVO".into()
        } else { "en silencio".to_string() }, true),
        ("scrub audible (clic cambia)".into(), if prefs::SCRUB_AUDIBLE.load(Ordering::Relaxed) {
            "se oye el material al arrastrar la aguja".into()
        } else { "scrub en silencio".to_string() }, true),
        ("ducking (clic cambia)".into(), if sonido::DUCKING.load(Ordering::Relaxed) {
            "la música se aparta bajo la voz (−12 dB)".into()
        } else { "sin ducking: la mezcla, plana".to_string() }, true),
        ("el imán de la bobina (clic cambia)".into(), if prefs::IMAN.load(Ordering::Relaxed) {
            "encendido: los clips se pegan al vecino".into()
        } else { "apagado: un clip se queda donde lo sueltes".to_string() }, true),
        ("normalizar el sonido del máster (clic cambia)".into(),
         if prefs::NORMALIZA.load(Ordering::Relaxed) {
            "sí: −16 LUFS con techo −1,5 dBTP".into()
        } else { "no: el sonido sale tal cual".to_string() }, true),
        ("LA PARED (clic: pegar una foto…)".into(),
         format!("{} foto(s) pegadas · viven en pared/", self.pared.len()), true),
        // ── EL FORMATO DE LA BOBINA, A POSTERIORI ─────────────────────────
        // Se elegía al cortarla y ahí se quedaba. Pero el destino cambia —lo
        // que era un 16:9 acaba siendo un vertical— y rehacer la bobina para
        // eso no tiene ningún sentido: no hay nada dentro del montaje que
        // dependa del lienzo, el encuadre de cada clip se recalcula solo.
        ("bobina".into(), pr.nombre.clone(), false),
        ("  aspecto (clic: el siguiente)".into(),
         match &pr.formato {
             Some(f) => format!("{} · {}×{}", f.aspecto, f.w, f.h),
             None => "auto · el del primer clip".into(),
         }, true),
        ("  tamaño del lienzo (clic: el siguiente)".into(),
         match &pr.formato {
             Some(f) => format!("{}p · {} píxeles por fotograma",
                                f.h.min(f.w), f.w as u64 * f.h as u64),
             None => "el del primer clip".into(),
         }, true),
        ("  cadencia (clic: la siguiente)".into(),
         format!("{} fps{}", if (pr.fps - pr.fps.round()).abs() < 0.02 {
                     format!("{:.0}", pr.fps) } else { format!("{:.3}", pr.fps) },
                 if pr.fps <= 0.0 { " · la del primer clip" } else { "" }), true),
        // ── LAS GELATINAS (§14) ───────────────────────────────────────────
        // Se elegían al crear la bobina y no se podían cambiar; y las
        // carpetas estaban clavadas en el taller.
        ("gelatina de ENTRADA (clic: la siguiente)".into(),
         format!("{} · {} en la carpeta",
                 pr.lut_in.as_ref().and_then(|p| p.file_stem())
                   .map(|s| s.to_string_lossy().to_string())
                   .unwrap_or_else(|| "directo · sin transformar".into()),
                 prefs::gelatinas(&pr.base, "entrada").len()), true),
        ("gelatina de COLOR (clic: la siguiente)".into(),
         format!("{} · {} en la carpeta",
                 pr.lut_color.as_ref().and_then(|p| p.file_stem())
                   .map(|s| s.to_string_lossy().to_string())
                   .unwrap_or_else(|| "ninguna".into()),
                 prefs::gelatinas(&pr.base, "color").len()), true),
        ("  carpeta de las de entrada (clic: elegir)".into(),
         prefs::dir_luts(&pr.base, "entrada").to_string_lossy().to_string(), true),
        ("  carpeta de las de color (clic: elegir)".into(),
         prefs::dir_luts(&pr.base, "color").to_string_lossy().to_string(), true),
        ("historial".into(),
         format!("{} paso(s) · ⌘Z deshace «{}»", self.historia.len(),
                 self.que_deshace().unwrap_or("nada")), false),
        ("cintas en la estantería".into(), format!("{}", self.estanteria.len()), false),
        ]
    }

    /// TOCAR UNA FILA DE AJUSTES. El orden es el de `filas_ajustes`, que es
    /// también el que se dibuja: una lista, un sitio.
    fn toca_ajuste(&mut self, pr: &mut Proyecto, fila: i32) {
        use std::sync::atomic::Ordering;
        match fila {
            2 => {
                let r = pr.base.clone();
                #[cfg(target_os = "macos")]
                let _ = std::process::Command::new("open").arg(&r).spawn();
                #[cfg(target_os = "windows")]
                let _ = std::process::Command::new("explorer").arg(&r).spawn();
                self.di(&format!("el taller vive en {}", r.display()));
            }
            3 => {
                if let Some(dir) = rfd::FileDialog::new()
                    .set_title("¿dónde cuelgo el máster?")
                    .set_directory(self.destino.clone().unwrap_or_else(|| pr.base.join("out")))
                    .pick_folder() {
                    prefs::guarda_destino(&pr.base, Some(&dir));
                    self.di(&format!("el máster irá a {}", dir.display()));
                    self.destino = Some(dir);
                }
            }
            4 => {
                // VACIAR LA CACHÉ. Los proxies NO se tocan: son el instante del
                // scrub, y rehacerlos cuesta minutos.
                let dir = pr.base.join(".cache");
                let (antes, _) = Self::pesa_cache(&pr.base);
                let mut n = 0;
                if let Ok(rd) = std::fs::read_dir(&dir) {
                    for e in rd.flatten() {
                        if e.path().is_file() && std::fs::remove_file(e.path()).is_ok() { n += 1; }
                    }
                }
                let (ahora, _) = Self::pesa_cache(&pr.base);
                self.di(&format!("caché vaciada: {n} pieza(s), {:.1} GB liberados \
                                  (los proxies se quedan)",
                                 (antes.saturating_sub(ahora)) as f64 / 1e9));
            }
            5 => self.recupera_copia(pr),
            6 => {
                let v = !prefs::PREVIEW_MEDIA.load(Ordering::Relaxed);
                prefs::PREVIEW_MEDIA.store(v, Ordering::Relaxed);
                prefs::guarda(&pr.base);
                self.di(if v { "preview a media resolución" } else { "preview COMPLETA" });
            }
            7 => {
                let v = !prefs::FOLEY.load(Ordering::Relaxed);
                prefs::FOLEY.store(v, Ordering::Relaxed);
                prefs::guarda(&pr.base);
                self.di(if v { "el taller suena" } else { "taller en silencio" });
            }
            8 => {
                let v = !prefs::SCRUB_AUDIBLE.load(Ordering::Relaxed);
                prefs::SCRUB_AUDIBLE.store(v, Ordering::Relaxed);
                prefs::guarda(&pr.base);
                self.di(if v { "el scrub se oye" } else { "scrub en silencio" });
            }
            9 => {
                let v = !sonido::DUCKING.load(Ordering::Relaxed);
                sonido::DUCKING.store(v, Ordering::Relaxed);
                prefs::guarda(&pr.base);
                self.di(if v { "la música se aparta bajo la voz" }
                        else { "mezcla plana (sin ducking)" });
            }
            10 => {
                let v = !prefs::IMAN.load(Ordering::Relaxed);
                prefs::IMAN.store(v, Ordering::Relaxed);
                prefs::guarda(&pr.base);
                self.di(if v { "imán encendido: los clips se pegan" }
                        else { "imán apagado: los clips van libres" });
            }
            11 => {
                let v = !prefs::NORMALIZA.load(Ordering::Relaxed);
                prefs::NORMALIZA.store(v, Ordering::Relaxed);
                prefs::guarda(&pr.base);
                self.di(if v { "el máster saldrá normalizado" }
                        else { "el máster saldrá tal cual" });
            }
            12 => {
                // pegar una foto en LA PARED (se COPIA a pared/: la única
                // excepción a la regla de cero copias, NORTE §1.4)
                if let Some(f) = rfd::FileDialog::new()
                    .add_filter("foto", &["jpg", "jpeg", "png"])
                    .set_title("una foto para la pared del taller")
                    .pick_file() {
                    let destino = pr.base.join("pared");
                    let _ = std::fs::create_dir_all(&destino);
                    if let Some(nom) = f.file_name() {
                        if std::fs::copy(&f, destino.join(nom)).is_ok() {
                            if let Ok(b) = std::fs::read(destino.join(nom)) {
                                if self.pared.len() < 3 {
                                    self.pared.push(ui::Estampa::new(&self.gpu, &b, false));
                                }
                            }
                            self.di("foto pegada en la pared");
                        }
                    }
                }
            }
            // 13 es el rótulo «bobina», que no hace nada. 14, 15 y 16 son el
            // formato: aspecto, tamaño y cadencia, cambiables cuando quieras.
            14 => {
                let orden: Vec<&str> = std::iter::once("auto")
                    .chain(proyecto::FORMATOS.iter().map(|f| f.0)).collect();
                let ahora = pr.formato.as_ref().map(|f| f.aspecto.clone())
                    .unwrap_or_else(|| "auto".into());
                let k = orden.iter().position(|a| *a == ahora).unwrap_or(0);
                let sig = orden[(k + 1) % orden.len()];
                // el ALTO se conserva al cambiar de aspecto: cambia la forma
                // del lienzo, no cuántos píxeles tiene de alto
                let alto = pr.formato.as_ref().map(|f| f.h).unwrap_or(1080);
                pr.formato = proyecto::FORMATOS.iter().find(|f| f.0 == sig)
                    .map(|(a, _, w, h)| {
                        let (w, h) = (*w as f64, *h as f64);
                        let nh = alto.max(240);
                        let nw = ((nh as f64 * w / h).round() as u32).max(16);
                        proyecto::Formato { w: nw & !1, h: nh & !1, aspecto: a.to_string() }
                    });
                self.recuerda(pr);
                let _ = pr.guarda();
                self.visor.recarga(&self.gpu, pr);
                self.di(&format!("la bobina es {}", pr.rotulo_formato()));
            }
            15 => {
                const ALTOS: [u32; 5] = [720, 1080, 1440, 2160, 4320];
                let f = pr.formato.clone().unwrap_or(proyecto::Formato {
                    w: 1920, h: 1080, aspecto: "16:9".into() });
                let k = ALTOS.iter().position(|a| *a == f.h.min(f.w)).unwrap_or(1);
                let nh = ALTOS[(k + 1) % ALTOS.len()];
                // el lado corto es el que manda: en vertical, 1080p es 1080
                // de ANCHO. Escalar por el largo daría un 9:16 de 4K sin
                // querer, que es cuatro veces el trabajo.
                let vertical = f.h > f.w;
                let (nw, nh2) = if vertical {
                    (nh, (nh as f64 * f.h as f64 / f.w as f64).round() as u32)
                } else {
                    ((nh as f64 * f.w as f64 / f.h as f64).round() as u32, nh)
                };
                pr.formato = Some(proyecto::Formato {
                    w: nw & !1, h: nh2 & !1, aspecto: f.aspecto });
                self.recuerda(pr);
                let _ = pr.guarda();
                self.visor.recarga(&self.gpu, pr);
                self.di(&format!("la bobina es {}", pr.rotulo_formato()));
            }
            16 => {
                let k = proyecto::FPS_OPCIONES.iter()
                    .position(|f| (f - pr.fps).abs() < 0.02).unwrap_or(0);
                let sig = proyecto::FPS_OPCIONES[(k + 1) % proyecto::FPS_OPCIONES.len()];
                pr.fps = if sig <= 0.0 { 25.0 } else { sig };
                self.recuerda(pr);
                let _ = pr.guarda();
                self.di(&format!("la bobina va a {} · el remuestreo lo pone el revelado",
                                 pr.rotulo_formato()));
            }
            // 17 y 18: la gelatina siguiente de cada ranura. La lista lleva
            // un hueco al principio —«ninguna»— porque quitar la gelatina es
            // una opción tan legítima como ponerla.
            17 | 18 => {
                let ranura = if fila == 17 { "entrada" } else { "color" };
                let hay = prefs::gelatinas(&pr.base, ranura);
                let ahora = if fila == 17 { pr.lut_in.clone() } else { pr.lut_color.clone() };
                let k = ahora.as_ref().and_then(|a| hay.iter().position(|x| x == a))
                    .map(|i| i as i64).unwrap_or(-1);
                let sig = if k + 1 >= hay.len() as i64 { None }
                          else { hay.get((k + 1) as usize).cloned() };
                let nombre = sig.as_ref().and_then(|p| p.file_stem())
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "ninguna".into());
                if fila == 17 { pr.lut_in = sig; } else { pr.lut_color = sig; }
                let _ = pr.guarda();
                self.visor.recarga(&self.gpu, pr);
                self.di(&format!("gelatina de {ranura}: {nombre}"));
            }
            19 | 20 => {
                let ranura = if fila == 19 { "entrada" } else { "color" };
                if let Some(dir) = rfd::FileDialog::new()
                    .set_title(if fila == 19 { "¿dónde están las gelatinas de entrada?" }
                               else { "¿dónde están las gelatinas de color?" })
                    .set_directory(prefs::dir_luts(&pr.base, ranura))
                    .pick_folder() {
                    prefs::guarda_dir_luts(&pr.base, ranura, &dir);
                    self.di(&format!("gelatinas de {ranura}: {} ({} .cube)",
                                     dir.display(),
                                     prefs::gelatinas(&pr.base, ranura).len()));
                }
            }
            _ => {}
        }
    }

    /// NORMALIZAR UNA PISTA DE MÚSICA sin tocar las demás (§4bis.11).
    ///
    /// La normalización del máster es de la mezcla entera; esto es otra cosa:
    /// llevar ESTA canción a un nivel de referencia para que dos temas
    /// distintos no obliguen a estar subiendo y bajando el mando. Se mide el
    /// pico real de la onda que el taller ya tiene dibujada —no hay que
    /// decodificar nada otra vez— y se pone la ganancia que lo deja en −3 dB.
    /// SIEMBRA LA BOBINA DE MARCAS AL COMPÁS de la música (ritmo.rs): cada
    /// golpe, una marca ♩ — y como las marcas son imanes, la cuchilla y los
    /// bordes se pegan al pulso solos. Con una música elegida, sólo la suya;
    /// si no, el de todas las cintas de la bobina.
    fn marcas_al_compas(&mut self, pr: &mut Proyecto) {
        let objetivos: Vec<usize> = match self.sel_audio {
            Some(ia) if ia < pr.audio.len() => vec![ia],
            _ => (0..pr.audio.len()).collect(),
        };
        if objetivos.is_empty() {
            self.di("no hay música a la que buscarle el pulso");
            return;
        }
        // medir lo que falte (una vez por cinta; el análisis tarda menos que
        // abrir el fichero, pero no es gratis)
        for &ia in &objetivos {
            let (media, ruta) = (pr.audio[ia].media.clone(), pr.audio[ia].ruta.clone());
            if !self.compases.contains_key(&media) {
                if let Some(c) = ritmo::analiza(&ruta) {
                    self.compases.insert(media, c);
                }
            }
        }
        let mut nuevas: Vec<f64> = Vec::new();
        let mut bpm = 0.0f64;
        let mut sordas: Vec<String> = Vec::new();
        for &ia in &objetivos {
            let au = &pr.audio[ia];
            let Some(c) = self.compases.get(&au.media) else {
                sordas.push(au.media.clone());
                continue;
            };
            bpm = c.bpm;
            for &g in &c.golpes {
                if g >= au.t_in - 1e-6 && g <= au.t_out + 1e-6 {
                    let t = au.entra() + (g - au.t_in);
                    if t >= 0.0 && t <= pr.duracion() + 0.25 {
                        nuevas.push(t);
                    }
                }
            }
        }
        if nuevas.is_empty() {
            self.di(if sordas.is_empty() { "el pulso cae fuera del trozo usado" }
                    else { "a esa música no le encuentro el pulso" });
            return;
        }
        self.recuerda(pr);
        pr.marcas.retain(|m| m.nota != "♩");
        nuevas.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
        nuevas.dedup_by(|x, y| (*x - *y).abs() < 0.04);
        let cuantas = nuevas.len();
        for t in nuevas {
            pr.marcas.push(proyecto::Marca { t, nota: "♩".into(), color: 2 });
        }
        pr.marcas.sort_by(|x, y| x.t.partial_cmp(&y.t).unwrap_or(std::cmp::Ordering::Equal));
        let _ = pr.guarda();
        self.visor.foley(sonido::Foley::Tick);
        if sordas.is_empty() {
            self.di(&format!("el compás sembrado: {cuantas} marcas a {bpm:.0} BPM"));
        } else {
            self.di(&format!("{cuantas} marcas a {bpm:.0} BPM (sin pulso: {})",
                             sordas.join(", ")));
        }
    }

    /// quita TODAS las marcas de compás (las ♩); las del autor se quedan
    fn compas_fuera(&mut self, pr: &mut Proyecto) {
        let cuantas = pr.marcas.iter().filter(|m| m.nota == "♩").count();
        if cuantas == 0 {
            self.di("no hay marcas de compás puestas");
            return;
        }
        self.recuerda(pr);
        pr.marcas.retain(|m| m.nota != "♩");
        let _ = pr.guarda();
        self.di(&format!("fuera las {cuantas} marcas de compás"));
    }

    fn normaliza_musica(&mut self, pr: &mut Proyecto, ia: usize) {
        let Some(a) = pr.audio.get(ia) else { return };
        let (media, ruta) = (a.media.clone(), a.ruta.clone());
        let Some(picos) = self.ondas.pide(&media, &ruta).cloned() else {
            self.di("todavía estoy midiendo la onda: prueba en un segundo");
            return;
        };
        let pico = picos.iter().cloned().fold(0.0f32, f32::max);
        if pico < 1e-4 { self.di("esa pista está en silencio"); return; }
        // los picos vienen normalizados a 1 = fondo de escala
        let db = 20.0 * (pico as f64).log10();
        let ganancia = (-3.0 - db).clamp(-40.0, 12.0);
        pr.audio[ia].gain = ganancia;
        let _ = pr.guarda();
        self.visor.busca(pr, self.visor.t);
        self.di(&format!("«{media}» normalizada a −3 dB ({ganancia:+.1} dB)"));
    }

    /// UN AVISO DEL SISTEMA (§5). Sin dependencias nuevas: en el Mac lo da
    /// `osascript` y en Windows PowerShell. Si no se puede, no pasa nada — el
    /// taller ya lo dice por dentro.
    fn avisa_al_sistema(titulo: &str, cuerpo: &str) {
        let cuerpo: String = cuerpo.chars().filter(|c| *c != '"' && *c != '\\')
            .take(90).collect();
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("osascript")
                .arg("-e")
                .arg(format!("display notification \"{cuerpo}\" with title \
                              \"Laboratorios Saorín\" subtitle \"{titulo}\""))
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("powershell")
                .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command",
                       &format!("[reflection.assembly]::LoadWithPartialName('System.Windows.Forms')\
                                 |Out-Null; $n=New-Object System.Windows.Forms.NotifyIcon; \
                                 $n.Icon=[System.Drawing.SystemIcons]::Information; \
                                 $n.Visible=$true; \
                                 $n.ShowBalloonTip(6000,'{titulo}','{cuerpo}',0); Start-Sleep 7")])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        { let _ = (titulo, cuerpo); }
    }

    /// COLOCAR UN CLIP DONDE SE SUELTE, con el imán apagado (§1.4).
    ///
    /// La bobina es una cinta pegada: la posición de un clip es implícita (la
    /// suma de las duraciones anteriores). Rehacer eso a `start` explícito por
    /// clip es el cambio más hondo de la lista, y no hace falta: el hueco YA
    /// existe en el modelo (`Clip { hueco: true }`), así que el espacio que
    /// queda al separar dos planos **se convierte en un hueco automático**.
    /// Menos elegante, mucho menos arriesgado, y el resultado en el máster es
    /// exactamente el mismo (negro con silencio).
    fn coloca_clip(&mut self, pr: &mut Proyecto, i: usize, t: f64) {
        if i >= pr.clips.len() { return; }
        let fps = pr.fps.max(1.0);
        let t = ((t.max(0.0) * fps).round() / fps).max(0.0);
        let clip = pr.clips.remove(i);
        // dónde empieza cada junta con el clip ya fuera
        let inicios = pr.inicios();
        let fin = pr.duracion();
        let j = inicios.iter().position(|&x| x >= t - 1.0 / fps).unwrap_or(pr.clips.len());
        let inicio_j = inicios.get(j).copied().unwrap_or(fin);
        if t > inicio_j + 0.5 / fps {
            // hace falta espacio delante: si lo que hay justo antes ya es un
            // hueco, se estira; si no, se mete uno nuevo
            let espacio = t - inicio_j;
            match j.checked_sub(1).and_then(|k| pr.clips.get_mut(k)).filter(|c| c.hueco) {
                Some(h) => { h.t_out += espacio; }
                None => {
                    let hueco = pr.hueco_de(espacio);
                    pr.clips.insert(j, hueco);
                }
            }
            let idx = if j == 0 { 1 } else { j };
            let idx = idx.min(pr.clips.len());
            pr.clips.insert(idx, clip);
            self.sel = Some(idx);
            self.di(&format!("colocado en {t:.2} s (hueco de {espacio:.2} s antes)"));
        } else {
            let idx = j.min(pr.clips.len());
            pr.clips.insert(idx, clip);
            self.sel = Some(idx);
            self.di(&format!("colocado en {:.2} s", inicio_j));
        }
        // los huecos de menos de un fotograma no son huecos: son ruido
        let minimo = 1.0 / fps;
        pr.clips.retain(|c| !c.hueco || c.t_out > minimo);
    }

    /// VOLVER A ENLAZAR un clip cuyo fichero no aparece (§4). El montaje no se
    /// toca: solo se le dice al registro dónde está ahora el material, y con
    /// eso vuelven TODOS los clips que lo usaban.
    fn reenlaza(&mut self, pr: &mut Proyecto, i: usize) {
        let Some(media) = pr.clips.get(i).map(|c| c.media.clone()) else { return };
        let Some(nueva) = rfd::FileDialog::new()
            .set_title(&format!("¿dónde está ahora «{media}»?"))
            .add_filter("material", &["mp4", "mov", "m4v", "mkv", "webm",
                                      "jpg", "jpeg", "png", "wav", "mp3", "flac", "m4a"])
            .pick_file() else { return };
        // el registro de ESTA bobina apunta al sitio nuevo, con la clave vieja
        let mj = pr.media_json();
        let mut m: serde_json::Map<String, serde_json::Value> = std::fs::read(&mj).ok()
            .and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        m.insert(media.clone(), serde_json::json!(nueva.to_string_lossy()));
        if std::fs::write(&mj, serde_json::to_vec_pretty(
                &serde_json::Value::Object(m)).unwrap_or_default()).is_err() {
            self.di("no pude escribir el registro");
            return;
        }
        let cuartos = filmlook_core::indice::sondea_orientado(&nueva).map(|x| x.4).unwrap_or(0);
        let mut n = 0;
        for c in pr.clips.iter_mut().filter(|c| c.media == media) {
            c.ruta = nueva.clone();
            c.ausente = false;
            // la orientación es del FICHERO: si el nuevo viene girado, se nota
            if c.enc.cuartos == c.cuartos_fichero { c.enc.cuartos = cuartos; }
            c.cuartos_fichero = cuartos;
            n += 1;
        }
        for a in pr.audio.iter_mut().filter(|a| a.media == media) { a.ruta = nueva.clone(); }
        let _ = pr.guarda();
        self.estanteria = pr.estanteria();
        self.proyecto_baldas = pr.baldas();
        self.visor.recarga(&self.gpu, pr);
        self.di(&format!("«{media}» vuelve a estar: {n} clip(s) recuperados"));
    }

    /// EL ARCHIVADOR, a la vista (§4bis.6). Funcionaba desde siempre y no lo
    /// sabía nadie: aquí se elige una copia y se vuelve a ella (la bobina de
    /// ahora se guarda antes, que si no esto sería una trampa).
    fn recupera_copia(&mut self, pr: &mut Proyecto) {
        let carpeta = pr.base.join("backups");
        let mut copias: Vec<std::path::PathBuf> = std::fs::read_dir(&carpeta).ok()
            .map(|rd| rd.flatten().map(|e| e.path())
                 .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
                 .collect()).unwrap_or_default();
        copias.sort();
        if copias.is_empty() { self.di("todavía no hay copias en backups/"); return; }
        let Some(elegida) = rfd::FileDialog::new()
            .set_title("¿a qué copia vuelvo? (la de ahora se guarda antes)")
            .set_directory(&carpeta)
            .add_filter("bobina", &["json"])
            .pick_file() else { return };
        self.recupera_esta_copia(pr, &elegida);
    }

    /// volver a una copia CONCRETA del archivador
    fn recupera_esta_copia(&mut self, pr: &mut Proyecto, elegida: &std::path::Path) {
        let carpeta = pr.base.join("backups");
        let _ = std::fs::create_dir_all(&carpeta);
        let _ = pr.guarda();
        let sello = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs()).unwrap_or(0);
        let _ = std::fs::copy(pr.ruta_json(),
                              carpeta.join(format!("antes-de-recuperar-{sello}.json")));
        match std::fs::copy(&elegida, pr.ruta_json()) {
            Ok(_) => match Proyecto::cargar() {
                Ok(nuevo) => {
                    *pr = nuevo;
                    self.historia.clear();
                    self.futuro.clear();
                    self.sel = None;
                    self.seleccion.clear();
                    self.visor.recarga(&self.gpu, pr);
                    self.di(&format!("recuperada la copia {}",
                                     elegida.file_name().unwrap_or_default().to_string_lossy()));
                }
                Err(e) => self.di(&format!("la copia no abre: {e}")),
            },
            Err(e) => self.di(&format!("no pude recuperarla: {e}")),
        }
    }

    /// CUÁNTO OCUPA LA CACHÉ. Se limitaba a 300 piezas, pero eso son gigas y
    /// nadie lo sabía (§5).
    fn pesa_cache(base: &std::path::Path) -> (u64, usize) {
        let mut bytes = 0u64;
        let mut n = 0usize;
        for dir in [base.join(".cache"), base.join(".proxies")] {
            if let Ok(rd) = std::fs::read_dir(&dir) {
                for e in rd.flatten() {
                    if let Ok(m) = e.metadata() {
                        if m.is_file() { bytes += m.len(); n += 1; }
                    }
                }
            }
        }
        (bytes, n)
    }

    fn cuenta_copias(base: &std::path::Path) -> usize {
        std::fs::read_dir(base.join("backups")).map(|rd| {
            rd.flatten().filter(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))
              .count()
        }).unwrap_or(0)
    }

    // ══════════════════════════════════════════════════ la portada ═══

    const TARJ_W: f32 = 292.0;
    const TARJ_H: f32 = 214.0;

    /// rects de las tarjetas: la 0 es «bobina nueva», luego las bobinas
    fn tarjetas(&self) -> Vec<(f32, f32, f32, f32)> {
        let (ancho, _) = self.gpu.alto_ancho();
        let gap = 26.0;
        let por_fila = (((ancho - 80.0 + gap) / (Self::TARJ_W + gap)) as usize).max(1);
        let x0 = (ancho - (por_fila as f32 * (Self::TARJ_W + gap) - gap)) / 2.0;
        (0..=self.bobinas.len()).map(|i| {
            let (col, fila) = (i % por_fila, i / por_fila);
            (x0 + col as f32 * (Self::TARJ_W + gap),
             196.0 + fila as f32 * (Self::TARJ_H + gap),
             Self::TARJ_W, Self::TARJ_H)
        }).collect()
    }

    /// rect del diálogo de bobina nueva
    fn dialogo_rect(&self) -> (f32, f32, f32, f32) {
        let (ancho, alto) = self.gpu.alto_ancho();
        let (w, h) = (560.0, 392.0);
        ((ancho - w) / 2.0, (alto - h) / 2.0 - 20.0, w, h)
    }

    fn pulsa_portada(&mut self, pr: &Proyecto) -> AccionPortada {
        let (mx, my) = self.raton;
        // ── EL MENÚ DE LA BOBINA manda mientras esté abierto (§4bis.8) ──
        if let Some(k) = self.bobina_menu {
            let tarj = self.tarjetas();
            if let Some((x, y, w, _)) = tarj.get(k + 1).copied() {
                let (mw, mh) = (172.0f32, 84.0);
                let (mx0, my0) = (x + w - mw - 10.0, y + 10.0);
                if mx >= mx0 && mx <= mx0 + mw && my >= my0 && my <= my0 + mh {
                    let cual = ((my - my0 - 6.0) / 24.0).floor();
                    let clave = self.bobinas.get(k).map(|b| b.clave.clone()).unwrap_or_default();
                    let nombre = self.bobinas.get(k).map(|b| b.nombre.clone()).unwrap_or_default();
                    self.bobina_menu = None;
                    match cual as i32 {
                        0 => {
                            if clave.is_empty() {
                                self.di("la bobina clásica no se renombra");
                            } else {
                                self.bobina_renombrando = Some((clave.clone(), clave));
                            }
                        }
                        1 => match proyecto::duplica_bobina(&pr.base, &clave) {
                            Ok(n) => {
                                self.bobinas = proyecto::bobinas(&pr.base);
                                self.di(&format!("copia hecha: «{n}»"));
                            }
                            Err(e) => self.di(&format!("no pude duplicarla: {e}")),
                        },
                        2 => match proyecto::borra_bobina(&pr.base, &clave) {
                            Ok(()) => {
                                self.bobinas = proyecto::bobinas(&pr.base);
                                self.di(&format!("«{nombre}» borrada                                                   (queda una copia en backups/)"));
                            }
                            Err(e) => self.di(&format!("no pude borrarla: {e}")),
                        },
                        _ => {}
                    }
                    return AccionPortada::Nada;
                }
            }
            self.bobina_menu = None;
            return AccionPortada::Nada;
        }
        // ── el diálogo de bobina nueva ──
        if self.nueva.is_some() {
            let (dx, dy, dw, dh) = self.dialogo_rect();
            if mx < dx || mx > dx + dw || my < dy || my > dy + dh {
                self.nueva = None;
                return AccionPortada::Nada;
            }
            // chips de formato (fila y = dy+120): auto + los 6 presets
            let n = self.nueva.as_mut().unwrap();
            for i in 0..=FORMATOS.len() {
                let cx = dx + 24.0 + i as f32 * 74.0;
                if mx >= cx && mx <= cx + 66.0 && my >= dy + 118.0 && my <= dy + 146.0 {
                    n.aspecto = i;
                    return AccionPortada::Nada;
                }
            }
            // chips de RESOLUCIÓN (fila y = dy+186)
            for i in 0..proyecto::ALTURAS.len() {
                let cx = dx + 24.0 + i as f32 * 74.0;
                if mx >= cx && mx <= cx + 66.0 && my >= dy + 186.0 && my <= dy + 214.0 {
                    n.alto = i;
                    return AccionPortada::Nada;
                }
            }
            // chips de fps (fila y = dy+258)
            for i in 0..FPS_OPCIONES.len() {
                let cx = dx + 24.0 + i as f32 * 74.0;
                if mx >= cx && mx <= cx + 66.0 && my >= dy + 258.0 && my <= dy + 286.0 {
                    n.fps = i;
                    return AccionPortada::Nada;
                }
            }
            // el botón «crear»
            if mx >= dx + dw - 150.0 && mx <= dx + dw - 24.0
                && my >= dy + dh - 60.0 && my <= dy + dh - 24.0 {
                return self.crea_desde_dialogo(pr);
            }
            return AccionPortada::Nada;
        }
        // ── las tarjetas ──
        for (i, (x, y, w, h)) in self.tarjetas().iter().enumerate() {
            if mx >= *x && mx <= x + w && my >= *y && my <= y + h {
                if i == 0 {
                    self.nueva = Some(NuevaBobina {
                        nombre: String::new(), aspecto: 0, fps: 0, alto: 2,
                        aviso: String::new(),
                    });
                    return AccionPortada::Nada;
                }
                if let Some(b) = self.bobinas.get(i - 1) {
                    return AccionPortada::Abrir(b.clave.clone());
                }
            }
        }
        AccionPortada::Nada
    }

    fn crea_desde_dialogo(&mut self, pr: &Proyecto) -> AccionPortada {
        let Some(n) = self.nueva.as_mut() else { return AccionPortada::Nada };
        let aspecto = if n.aspecto >= FORMATOS.len() { "auto" } else { FORMATOS[n.aspecto].0 };
        let fps = FPS_OPCIONES[n.fps.min(FPS_OPCIONES.len() - 1)];
        let alto = proyecto::ALTURAS[n.alto.min(proyecto::ALTURAS.len() - 1)].0;
        let nombre_lut = |p: &Option<std::path::PathBuf>, def: &str| -> String {
            p.as_ref().and_then(|x| x.file_name()).map(|x| x.to_string_lossy().to_string())
                .unwrap_or_else(|| def.to_string())
        };
        match proyecto::crea_bobina(
            &pr.base, &n.nombre, aspecto, fps, alto, &pr.prefs,
            &nombre_lut(&pr.lut_in, "Directo · sin transformar.cube"),
            &nombre_lut(&pr.lut_color, "Saorín · 65 puntos.cube"),
        ) {
            Ok(()) => {
                let clave = n.nombre.trim().to_string();
                AccionPortada::Abrir(clave)
            }
            Err(e) => {
                n.aviso = format!("{e}");
                AccionPortada::Nada
            }
        }
    }

    fn teclado_portada(&mut self, pr: &Proyecto, ev: &winit::event::KeyEvent) -> AccionPortada {
        // renombrando una bobina: el teclado es suyo
        if self.bobina_renombrando.is_some() {
            match ev.physical_key {
                PhysicalKey::Code(KeyCode::Escape) => { self.bobina_renombrando = None; }
                PhysicalKey::Code(KeyCode::Enter) => {
                    if let Some((viejo, nuevo)) = self.bobina_renombrando.take() {
                        match proyecto::renombra_bobina(&pr.base, &viejo, &nuevo) {
                            Ok(n) => {
                                self.bobinas = proyecto::bobinas(&pr.base);
                                self.di(&format!("ahora se llama «{n}»"));
                            }
                            Err(e) => self.di(&format!("no pude renombrarla: {e}")),
                        }
                    }
                }
                PhysicalKey::Code(KeyCode::Backspace) => {
                    if let Some((_, n)) = self.bobina_renombrando.as_mut() { n.pop(); }
                }
                _ => {
                    if let (Some((_, n)), Some(txt)) =
                        (self.bobina_renombrando.as_mut(), ev.text.as_ref()) {
                        for c in txt.chars() {
                            if !c.is_control() && n.chars().count() < 40 { n.push(c); }
                        }
                    }
                }
            }
            return AccionPortada::Nada;
        }
        if self.nueva.is_some() {
            match ev.physical_key {
                PhysicalKey::Code(KeyCode::Escape) => { self.nueva = None; }
                PhysicalKey::Code(KeyCode::Enter) => return self.crea_desde_dialogo(pr),
                PhysicalKey::Code(KeyCode::Backspace) => {
                    if let Some(n) = self.nueva.as_mut() { n.nombre.pop(); }
                }
                _ => {
                    if let (Some(n), Some(txt)) = (self.nueva.as_mut(), ev.text.as_ref()) {
                        for c in txt.chars() {
                            if !c.is_control() && n.nombre.chars().count() < 40 {
                                n.nombre.push(c);
                            }
                        }
                    }
                }
            }
            return AccionPortada::Nada;
        }
        match ev.physical_key {
            PhysicalKey::Code(KeyCode::Escape) | PhysicalKey::Code(KeyCode::Enter)
            | PhysicalKey::Code(KeyCode::Space) => AccionPortada::Continuar,
            _ => AccionPortada::Nada,
        }
    }

    /// la portada: el taller te recibe con sus bobinas colgadas
    fn dibuja_portada(&mut self, pr: &Proyecto, d: &mut ui::Dibujo, dt: &mut ui::DibujoTex,
                      d2: &mut ui::Dibujo, ancho: f32, alto: f32) {
        use ui::Familia::*;
        // el másthead del zine, fuera de registro
        d2.texto_f(Grot, 47.4, 57.4, "LABORATORIOS", 42.0, paleta::ROJO);
        d2.texto_f(Grot, 44.0, 54.0, "LABORATORIOS", 42.0, paleta::TINTA);
        d2.texto_f(Grot, 358.8, 56.8, "SAORÍN", 42.0, paleta::AMBAR);
        d2.texto_f(Grot, 356.0, 54.0, "SAORÍN", 42.0, paleta::ROJO);
        d2.texto_f(Mano, 46.0, 104.0, "el taller de revelado — elige una bobina o corta una nueva",
                  21.0, paleta::TINTA_TENUE);
        trazo::linea(d2, 44.0, 151.0, ancho - 44.0, 151.0, 1.8, paleta::TINTA_TENUE, 700);
        // la foto de la casa, pegada con celo en la esquina
        self.objetos.quad_uv_rot(ancho - 230.0, 170.0, 168.0, 136.0,
                                 doodles::uv(doodles::FOTO_LAB), 0.03);
        self.objetos.quad_uv_rot(ancho - 190.0, 158.0, 62.0, 28.0, doodles::uv(doodles::CELO), -0.05);
        // las fotos del autor, pegadas con celo bajo la de la casa
        if self.nueva.is_none() {
            self.pega_pared(ancho, alto, 330.0);
        }
        // el colofón del zine
        let cy = alto - 74.0;
        trazo::linea(d2, 44.0, cy - 8.0, 420.0, cy - 10.0, 1.4, paleta::TINTA, 701);
        d2.texto_f(Grot, 44.0, cy, "COLOFÓN", 11.0, paleta::TINTA);
        d2.texto_f(Serif, 44.0, cy + 18.0,
                   "— hecho a mano en los Laboratorios Saorín · dos tintas sobre papel hueso",
                   10.0, paleta::TINTA);
        d2.texto_f(Serif, 44.0, cy + 33.0, "— LAB · SAORIN · 2026", 10.0, paleta::TINTA);

        let tarjetas = self.tarjetas();
        let (rx, ry) = self.raton;
        // con el diálogo abierto las tarjetas NO se dibujan: su texto vive en
        // la capa de tipos (la última) y se colaría por encima del modal.
        // Regla de la casa: un modal SUPRIME lo de debajo, no lo tapa.
        let tarjetas_visibles = if self.nueva.is_some() { &tarjetas[..0] } else { &tarjetas[..] };
        for (i, (x, y, w, h)) in tarjetas_visibles.iter().enumerate() {
            let (x, y, w, h) = (*x, *y, *w, *h);
            let hover = self.nueva.is_none()
                && rx >= x && rx <= x + w && ry >= y && ry <= y + h;
            d2.rect(x + 4.0, y + 6.0, w, h, [0.0, 0.0, 0.0, if hover { 0.12 } else { 0.07 }]);
            d2.rect(x, y, w, h, [0.965, 0.953, 0.918, 0.94]);
            trazo::caja(d2, x, y, w, h, if hover { 1.8 } else { 1.2 },
                        if hover { paleta::ROJO } else { paleta::TINTA }, 720 + i as u32);
            self.objetos.quad_uv_rot(x + w / 2.0 - 28.0, y - 12.0, 56.0, 25.0,
                                     doodles::uv(doodles::CELO), if i % 2 == 0 { 0.05 } else { -0.06 });
            if i == 0 {
                // «bobina nueva»
                d2.texto_f(Grot, x + 20.0, y + 26.0, "+", 64.0, paleta::TINTA_TENUE);
                d2.texto_f(Grot, x + 20.0, y + 116.0, "BOBINA NUEVA", 18.0, paleta::TINTA);
                d2.texto(x + 20.0, y + 146.0, "nombre, formato y fps", 11.0, paleta::TINTA_TENUE);
                continue;
            }
            let Some(b) = self.bobinas.get(i - 1) else { continue };
            // la miniatura de la primera cinta (con el diálogo abierto NO se
            // pinta: vive en la capa alta del atlas y se colaría por encima)
            if let Some((media, ruta)) = &b.primera {
                let clave = (format!("portada:{media}"), 100);
                match self.minis.pide(clave, ruta, 1.0) {
                    Some(slot) if self.nueva.is_none() => {
                        dt.quad(x + 14.0, y + 14.0, w - 28.0, 138.0, slot, 1.0);
                    }
                    _ => { d2.rect(x + 14.0, y + 14.0, w - 28.0, 138.0, paleta::PELICULA); }
                }
            } else {
                d2.rect(x + 14.0, y + 14.0, w - 28.0, 138.0, paleta::PELICULA);
                d2.texto(x + 24.0, y + 74.0, "(bobina virgen)", 11.0, [0.7, 0.68, 0.6, 1.0]);
            }
            let nombre: String = b.nombre.chars().take(22).collect();
            d2.texto_f(Mano, x + 16.0, y + 152.0, &nombre, 25.0, paleta::TINTA);
            let fmt = if b.formato == "auto" { String::from("auto") } else { b.formato.clone() };
            d2.texto(x + 16.0, y + 186.0,
                    &format!("{} clip(s) · {:.0} s · {}", b.clips, b.dur, fmt),
                    11.0, paleta::TINTA_TENUE);
        }

        // ── EL MENÚ DE LA BOBINA (clic derecho en su tarjeta, §4bis.8) ──
        if let Some(k) = self.bobina_menu {
            if let Some((x, y, w, _)) = self.tarjetas().get(k + 1).copied() {
                let (mw, mh) = (172.0f32, 84.0);
                let mx0 = x + w - mw - 10.0;
                let my0 = y + 10.0;
                d2.rect(mx0 + 3.0, my0 + 4.0, mw, mh, [0.0, 0.0, 0.0, 0.22]);
                d2.rect(mx0, my0, mw, mh, [0.118, 0.112, 0.096, 1.0]);
                for (i, rot) in ["renombrar…", "duplicar", "borrar"].iter().enumerate() {
                    let yy = my0 + 6.0 + i as f32 * 24.0;
                    let sobre = self.raton.0 >= mx0 && self.raton.0 <= mx0 + mw
                        && self.raton.1 >= yy && self.raton.1 < yy + 24.0;
                    if sobre { d2.rect(mx0 + 3.0, yy, mw - 6.0, 22.0, [0.851, 0.2, 0.145, 0.75]); }
                    d2.texto(mx0 + 14.0, yy + 6.0, rot, 9.5,
                             if sobre { paleta::HUESO } else { [0.86, 0.84, 0.78, 1.0] });
                }
            }
        }
        // ── RENOMBRAR UNA BOBINA ────────────────────────────────────────
        if let Some((viejo, nuevo)) = &self.bobina_renombrando {
            use ui::Familia::*;
            d2.rect(0.0, 0.0, ancho, alto, [0.10, 0.10, 0.08, 0.5]);
            let (w, h) = (520.0, 150.0);
            let (x, y) = ((ancho - w) / 2.0, (alto - h) / 2.0);
            d2.rect(x, y, w, h, [0.965, 0.953, 0.918, 1.0]);
            d2.rect(x, y, w, 3.0, paleta::TINTA);
            d2.texto_f(Grot, x + 24.0, y + 14.0, "RENOMBRAR LA BOBINA", 16.0, paleta::TINTA);
            d2.texto(x + 24.0, y + 38.0, &format!("antes: {viejo}"), 10.0, paleta::TINTA_TENUE);
            d2.rect(x + 24.0, y + 58.0, w - 48.0, 32.0, [1.0, 1.0, 1.0, 0.6]);
            d2.rect(x + 24.0, y + 88.0, w - 48.0, 2.0, paleta::TINTA);
            d2.texto_f(Mano, x + 32.0, y + 60.0, &format!("{nuevo}▏"), 22.0, paleta::TINTA);
            d2.texto(x + 24.0, y + h - 26.0,
                     "⏎ renombra · esc cierra · el material no se toca", 10.0,
                     paleta::TINTA_TENUE);
        }
        // ── el diálogo de bobina nueva ──
        if let Some(n) = &self.nueva {
            let (dx, dy, dw, dh) = self.dialogo_rect();
            d2.rect(0.0, 0.0, ancho, alto, [0.10, 0.10, 0.08, 0.55]);
            d2.rect(dx + 5.0, dy + 7.0, dw, dh, [0.0, 0.0, 0.0, 0.22]);
            d2.rect(dx, dy, dw, dh, [0.965, 0.953, 0.918, 1.0]);
            d2.rect(dx, dy, dw, 3.0, paleta::TINTA);
            d2.texto_f(Grot, dx + 24.0, dy + 18.0, "BOBINA NUEVA", 20.0, paleta::TINTA);
            // nombre
            d2.texto(dx + 24.0, dy + 56.0, "nombre", 11.0, paleta::TINTA_TENUE);
            d2.rect(dx + 24.0, dy + 74.0, dw - 48.0, 32.0, [1.0, 1.0, 1.0, 0.55]);
            d2.rect(dx + 24.0, dy + 104.0, dw - 48.0, 2.0, paleta::TINTA);
            let caret = if std::time::Instant::now().elapsed().as_millis() % 2 == 0 { "" } else { "" };
            let _ = caret;
            d2.texto_f(Mano, dx + 32.0, dy + 76.0, &format!("{}▏", n.nombre), 24.0, paleta::TINTA);
            // formato
            d2.texto(dx + 24.0, dy + 120.0 - 14.0, "formato", 11.0, paleta::TINTA_TENUE);
            for i in 0..=FORMATOS.len() {
                let cx = dx + 24.0 + i as f32 * 74.0;
                let activo = n.aspecto == i;
                let (etq, sub) = if i >= FORMATOS.len() { ("auto", "del clip") }
                                 else { (FORMATOS[i].0, FORMATOS[i].1) };
                d2.rect(cx, dy + 118.0, 66.0, 28.0,
                       if activo { paleta::TINTA } else { [1.0, 1.0, 1.0, 0.5] });
                d2.texto(cx + 8.0, dy + 124.0, etq, 12.0,
                        if activo { paleta::HUESO } else { paleta::TINTA });
                if activo {
                    d2.texto(dx + 24.0, dy + 152.0, sub, 10.0, paleta::NARANJA);
                }
            }
            // RESOLUCIÓN (la altura manda; el ancho sale del aspecto)
            d2.texto(dx + 24.0, dy + 186.0 - 14.0, "resolución", 11.0, paleta::TINTA_TENUE);
            for (i, (alto_px, etq)) in proyecto::ALTURAS.iter().enumerate() {
                let cx = dx + 24.0 + i as f32 * 74.0;
                let activo = n.alto == i;
                d2.rect(cx, dy + 186.0, 66.0, 28.0,
                       if activo { paleta::TINTA } else { [1.0, 1.0, 1.0, 0.5] });
                d2.texto(cx + 8.0, dy + 192.0, etq, 12.0,
                        if activo { paleta::HUESO } else { paleta::TINTA });
                if activo {
                    // el tamaño REAL que va a tener el lienzo
                    let dicho = if *alto_px == 0 {
                        "el lienzo lo dice el primer clip".to_string()
                    } else if n.aspecto >= FORMATOS.len() {
                        format!("{alto_px} px de alto · el ancho, del clip")
                    } else {
                        let f = FORMATOS[n.aspecto];
                        let prop = f.2 as f64 / f.3 as f64;
                        let w2 = ((*alto_px as f64 * prop / 2.0).round() * 2.0) as u32;
                        format!("{w2} × {alto_px}")
                    };
                    d2.texto(dx + 24.0, dy + 220.0, &dicho, 10.0, paleta::NARANJA);
                }
            }
            // fps
            d2.texto(dx + 24.0, dy + 258.0 - 14.0, "cadencia", 11.0, paleta::TINTA_TENUE);
            for (i, f) in FPS_OPCIONES.iter().enumerate() {
                let cx = dx + 24.0 + i as f32 * 74.0;
                let activo = n.fps == i;
                let etq = if *f == 0.0 { "auto".to_string() }
                          else if (*f - f.round()).abs() < 0.01 { format!("{:.0}", f) }
                          else { format!("{:.2}", f) };
                d2.rect(cx, dy + 258.0, 66.0, 28.0,
                       if activo { paleta::TINTA } else { [1.0, 1.0, 1.0, 0.5] });
                d2.texto(cx + 8.0, dy + 264.0, &etq, 12.0,
                        if activo { paleta::HUESO } else { paleta::TINTA });
            }
            // crear + aviso
            d2.rect(dx + dw - 150.0, dy + dh - 60.0, 126.0, 36.0, paleta::ROJO);
            d2.texto_f(Grot, dx + dw - 132.0, dy + dh - 52.0, "CREAR ⏎", 15.0, paleta::HUESO);
            if !n.aviso.is_empty() {
                d2.texto(dx + 24.0, dy + dh - 52.0, &n.aviso, 11.0, paleta::NARANJA);
            }
            d2.texto(dx + 24.0, dy + dh - 26.0,
                    "esc cierra · hereda el cuarto oscuro de la casa", 10.0, paleta::TINTA_TENUE);
        }
        let _ = pr;
    }

    // ── LA MESA DE SONIDO DEL MARGEN ────────────────────────────────────
    //
    // La esquina de abajo a la izquierda mide 230×250 y no cabía: las agujas
    // ocupaban de +189 a +223 y los mandos de nivel de +211 a +250 —se
    // pisaban—, y las barras de la mezcla se iban a +272, setenta y dos
    // píxeles POR DEBAJO del borde. Cada elemento estaba bien; lo que faltaba
    // era la composición.
    //
    // El error de fondo era tratar el margen como espacio libre. No lo es:
    // ya era **una leyenda de pistas**, con una etiqueta por carril alineada
    // con su carril a la derecha. Eso se entiende solo y se queda.
    //
    // Así que el reparto es éste:
    //
    //   +0 … +44    la cabecera de la bobina (lo que ya había)
    //   +46 … +176  LA LEYENDA DE PISTAS, y cada fila de sonido pasa a ser una
    //               TIRA DE CANAL: su nombre, su nivel y sus decibelios, que
    //               es donde uno los busca. Cero píxeles de más.
    //   +182 … +246 LOS INSTRUMENTOS, los cuatro en fila: la mezcla en L y R,
    //               las dos agujas y la manivela.
    //
    // Y la geometría vive aquí, en funciones que leen el dibujo Y el ratón,
    // que es lo único que impide que vuelvan a separarse. La manivela la
    // tenían escrita a mano en tres sitios.
    const NIVEL_W: f32 = 92.0;

    /// el mando de nivel `k` (0 = sonido del vídeo · 1 = música): la esquina
    /// de su barra, **en la fila de su pista**
    fn mando_nivel(&self, k: u8) -> (f32, f32) {
        let y = if k == 0 { self.tira_y() + self.alto_tira() - 12.0 }
                else { self.pista_y(0) + 16.0 };
        (76.0, y)
    }

    /// el alto de la tira de vídeo (lo usa la leyenda del margen)
    fn alto_tira(&self) -> f32 { 88.0 }

    /// el centro de la manivela
    fn manivela_centro(&self) -> (f32, f32) { (198.0, self.gpu.alto_ancho().1 - 40.0) }

    /// la esquina de arriba a la izquierda de las barras de la mezcla
    fn medidor_lr_caja(&self) -> (f32, f32) { (14.0, self.gpu.alto_ancho().1 - 64.0) }

    /// la línea de las dos agujas analógicas
    fn agujas_y(&self) -> f32 { self.gpu.alto_ancho().1 - 36.0 }

    /// ¿sobre qué mando de nivel está el ratón?
    fn nivel_en(&self, mx: f32, my: f32) -> Option<u8> {
        for k in 0..2u8 {
            let (bx, by) = self.mando_nivel(k);
            if mx >= bx - 4.0 && mx <= bx + Self::NIVEL_W + 30.0
                && my >= by - 7.0 && my <= by + 11.0 { return Some(k); }
        }
        None
    }

    /// LOS CARRILES DE MÚSICA (§2): con dos o tres canciones hace falta
    /// apilarlas, y para eso cada una tiene que saber en cuál va.
    const ALTO_PISTA: f32 = 26.0;

    /// y de la cabecera del carril `k`
    fn pista_y(&self, k: u8) -> f32 {
        self.tira_y() + 88.0 + 14.0 + self.extra_sub + k as f32 * Self::ALTO_PISTA
    }

    /// EL ALTO de cada pista de capa en pantalla
    const ALTO_CAPA: f32 = 24.0;

    /// EL CARRIL DEL PIE: alto en pantalla, y cuánto baja a la música
    const ALTO_SUB: f32 = 22.0;

    /// ¿se ve el carril de subtítulos? Sólo si hay pie (o se está haciendo):
    /// quien no subtitula no pierde mesa por ello.
    fn hay_pie(&self, pr: &Proyecto) -> bool { !pr.subs.is_empty() }

    /// la Y del carril del pie: pegado DEBAJO de la tira de vídeo, que es
    /// donde va un subtítulo — entre la imagen y el sonido
    fn sub_y(&self) -> f32 { self.tira_y() + 88.0 + 3.0 }

    /// el subtítulo bajo el ratón, si lo hay
    fn sub_en_punto(&self, pr: &Proyecto, mx: f32, my: f32) -> Option<usize> {
        if !self.hay_pie(pr) { return None; }
        let y = self.sub_y();
        if my < y - 2.0 || my > y + Self::ALTO_SUB { return None; }
        (0..pr.subs.len()).find(|&k| {
            let (x0, x1) = (self.x_de(pr.subs[k].t0), self.x_de(pr.subs[k].t1));
            mx >= x0 - 1.0 && mx <= x1 + 1.0
        })
    }

    /// CUÁNTAS PISTAS DE CAPA SE VEN: las que tienen material más una libre
    /// para soltar encima — el gesto de DaVinci de «la pista aparece cuando
    /// la necesitas», sin robarle mesa a quien no usa capas.
    fn pistas_capa_visibles(&self, pr: &Proyecto) -> usize {
        let usadas = pr.capas.iter().map(|c| c.pista as usize + 1).max().unwrap_or(0);
        (usadas + 1).min(proyecto::PISTAS_CAPA)
    }

    /// la Y del carril de la pista `p` (0 = V2, la más pegada al vídeo). Las
    /// pistas SUBEN: V2 justo encima de la tira, V3 encima de V2…
    fn capa_pista_y(&self, p: u8) -> f32 {
        self.tira_y() - 8.0 - Self::ALTO_CAPA * (p as f32 + 1.0)
    }

    /// qué pista de capa hay bajo una y de pantalla
    fn pista_capa_en(&self, my: f32) -> Option<u8> {
        for p in 0..proyecto::PISTAS_CAPA as u8 {
            let y = self.capa_pista_y(p);
            if y < self.banco_y() { continue; }   // ese carril aún no se ve
            if my >= y - 2.0 && my <= y + Self::ALTO_CAPA - 4.0 { return Some(p); }
        }
        None
    }

    /// la capa bajo el ratón, si la hay (dentro de su pista; la última gana)
    fn capa_en_punto(&self, pr: &Proyecto, mx: f32, my: f32) -> Option<usize> {
        let p = self.pista_capa_en(my)?;
        (0..pr.capas.len()).rev().find(|&k| {
            let cp = &pr.capas[k];
            cp.pista == p && {
                let x0 = self.x_de(cp.start);
                let x1 = self.x_de(cp.fin());
                mx >= x0 && mx <= x1
            }
        })
    }

    /// CUÁNTOS CARRILES DE MÚSICA SE VEN: los usados más uno libre, y nunca
    /// menos de tres (la mesa de siempre). El mismo gesto que las capas.
    fn musica_visibles(pr: &Proyecto) -> usize {
        let usadas = pr.audio.iter().map(|a| a.pista as usize + 1).max().unwrap_or(0);
        (usadas + 1).max(3).min(proyecto::PISTAS_MUSICA)
    }

    /// qué carril de música hay bajo una y de pantalla
    fn pista_en(&self, my: f32) -> Option<u8> {
        let y0 = self.pista_y(0);
        if my < y0 - 3.0 { return None; }
        let k = ((my - y0) / Self::ALTO_PISTA).floor();
        if k < 0.0 || k as usize >= self.musica_vis { return None; }
        Some(k as u8)
    }

    /// tiempo de bobina bajo una x de pantalla
    fn tiempo_en(&self, x: f32) -> f64 {
        ((x - Self::ESTANTE_W - 12.0 + self.desplaza) / self.pxs).max(0.0) as f64
    }
    fn x_de(&self, t: f64) -> f32 {
        Self::ESTANTE_W + 12.0 + t as f32 * self.pxs - self.desplaza
    }

    /// cuánto puede desplazarse la bobina (0 si cabe entera)
    fn desplaza_max(&self, pr: &Proyecto) -> f32 {
        let (ancho, _) = self.gpu.alto_ancho();
        let total = pr.duracion().max(0.0) as f32 * self.pxs + 60.0;
        let vista = ancho - Self::ESTANTE_W - 24.0;
        (total - vista).max(0.0)
    }

    fn banco_y(&self) -> f32 { self.gpu.alto_ancho().1 - self.banco_h }
    fn tira_y(&self) -> f32 { self.banco_y() + 46.0 + self.extra_capas }

    fn cambia_mando(&mut self, pr: &mut Proyecto, k: &str, paso: f32, lo: f32, hi: f32) {
        // el cuarto oscuro es DEL CLIP QUE HAY BAJO LA AGUJA: lo que se ve es
        // lo que se toca
        let Some(i) = self.bajo_aguja(pr) else { return };
        let Some(c) = pr.clips.get_mut(i) else { return };
        let v0 = c.prefs.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
        let v = (v0 + paso).clamp(lo, hi);
        if let Some(o) = c.prefs.as_object_mut() {
            o.insert(k.to_string(), serde_json::json!(v as f64));
        }
        self.visor.marca_cuarto(i);
    }

    /// EL LIENZO DEL MÁSTER, en un solo sitio: lo usan el cartel de la sala
    /// y el propio revelado. Si divergen, la sala miente.
    /// Sin esto el revelador tomaba la resolución del PRIMER CLIP: una
    /// bobina 1080p con material 4K se revelaba a 4K — cuatro veces más
    /// píxeles y encima el máster no era el formato pedido.
    // ── EL CUBO DE RECORTES, en un solo sitio ─────────────────────────
    // La geometría la usan el dibujo, el clic, la rueda y el arrastre: si
    // cada uno la calculara por su cuenta, tocar el diseño descolocaría los
    // otros (que es justo lo que pasaba con el «+N más»).
    const CUBO_X: f32 = 16.0;
    const CUBO_W: f32 = 198.0;
    const CUBO_COL: f32 = 64.0;      // ancho de cada recorte + hueco
    const CUBO_FIL: f32 = 40.0;      // alto de cada fila
    const CUBO_COLS: usize = 3;

    /// (y de la boca del cubo, alto útil)
    ///
    /// EL CUBO SE APOYA EN EL PIE, no cuelga de una constante. Antes era
    /// `banco - 330` a secas: en una ventana baja (1366×768 en Windows, que
    /// es donde el autor decía que «solo se puede borrar») el cubo se salía
    /// por arriba de la zona útil y el arrastre no encontraba dónde soltar.
    /// Ahora el fondo está fijo justo encima del pie de la estantería y lo
    /// que se encoge es el alto.
    fn cubo_caja(&self) -> (f32, f32) {
        let fondo = self.banco_y() - 198.0;
        let tope = Self::CABECERA + 116.0;
        let alto = (fondo - tope).clamp(56.0, 132.0);
        (fondo - alto, alto)
    }

    /// ¿cae este punto en la boca del cubo? La zona es TODA la columna de la
    /// izquierda a la altura del cubo, no el rectángulo exacto: al soltar un
    /// clip uno apunta al cubo, no acierta un rectángulo de 198 px.
    fn en_el_cubo(&self, mx: f32, my: f32) -> bool {
        let (cy, ch) = self.cubo_caja();
        mx < Self::ESTANTE_W && my > cy - 8.0 && my < cy + ch + 8.0
            && !self.en_la_papelera(mx, my)
    }

    /// LA PAPELERA, debajo del cubo: (x, y, ancho, alto).
    ///
    /// El cubo GUARDA —«por si acaso», y de ahí se saca— y la papelera TIRA.
    /// Son dos gestos distintos y hasta ahora sólo existía el primero: lo
    /// apartado se acumulaba sin forma de deshacerse de ello.
    fn papelera_caja(&self) -> (f32, f32, f32, f32) {
        let (cy, ch) = self.cubo_caja();
        (Self::CUBO_X, cy + ch + 18.0, Self::CUBO_W, 58.0)
    }

    /// LA PAPELERA ES UNIVERSAL: acepta un clip de la bobina, un recorte del
    /// cubo y una cinta de la estantería. Lo que hace con cada uno es lo
    /// suyo, pero el gesto —arrastrar hasta aquí— es el mismo.
    fn en_la_papelera(&self, mx: f32, my: f32) -> bool {
        let (x, y, w, h) = self.papelera_caja();
        mx >= x - 6.0 && mx <= x + w + 6.0 && my >= y - 6.0 && my <= y + h + 6.0
    }

    /// cuántas filas caben a la vista
    fn cubo_filas(&self) -> usize {
        let (_, alto) = self.cubo_caja();
        ((alto - 12.0) / Self::CUBO_FIL).floor().max(1.0) as usize
    }

    /// hasta dónde se puede bajar (el cubo no tiene fondo)
    fn cubo_scroll_max(&self) -> f32 {
        let filas = (self.recortes.len() + Self::CUBO_COLS - 1) / Self::CUBO_COLS;
        ((filas.saturating_sub(self.cubo_filas())) as f32 * Self::CUBO_FIL).max(0.0)
    }

    /// el recorte que hay bajo el ratón, si lo hay. El cubo se recorre de lo
    /// MÁS RECIENTE a lo más viejo: lo último que apartaste está arriba.
    fn recorte_en(&self, mx: f32, my: f32) -> Option<usize> {
        let (cy, alto) = self.cubo_caja();
        let y0 = cy + 22.0;
        if mx < Self::CUBO_X || mx > Self::CUBO_X + Self::CUBO_W { return None; }
        if my < y0 || my > cy + alto { return None; }
        let col = ((mx - Self::CUBO_X - 2.0) / Self::CUBO_COL).floor();
        let fil = ((my - y0 + self.cubo_scroll) / Self::CUBO_FIL).floor();
        if col < 0.0 || fil < 0.0 || col as usize >= Self::CUBO_COLS { return None; }
        let k = fil as usize * Self::CUBO_COLS + col as usize;
        // el índice visible k cuenta desde el final del vector
        self.recortes.len().checked_sub(1 + k)
    }

    fn lienzo_del_master(&self, pr: &Proyecto) -> (u32, u32) {
        match &pr.formato {
            Some(f) => (f.w, f.h),
            None => pr.clips.iter().find_map(|c| {
                filmlook_core::indice::sondea(&c.ruta).ok().map(|(w, h, _, _)| (w, h))
            }).unwrap_or((1920, 1080)),
        }
    }

    /// LAS CUATRO MEDIDAS DEL MÁSTER: (lo que sale, lo que se revela).
    ///
    /// El mismo cálculo que hace el shell, y por la misma razón que el lienzo:
    /// si divergen, la sala miente. La PROPORCIÓN es siempre la de la bobina —
    /// el formato es la decisión creativa y se tomó al cortarla; lo que el
    /// cajón cambia es cuántos píxeles.
    fn medidas_master(&self, pr: &Proyecto) -> (u32, u32, u32, u32) {
        let (bw, bh) = self.lienzo_del_master(pr);
        // LOS CAMINOS DE LA CASA NO MIRAN EL CAJÓN: al lienzo de la bobina y
        // sin escalar, pase lo que pase. Es lo que los hace de fiar.
        if self.preset_revelado != A_MANO { return (bw, bh, bw, bh); }
        let (sw, sh) = if self.master.alto == 0 || self.master.alto == bh {
            (bw, bh)
        } else {
            let prop = bw as f64 / bh.max(1) as f64;
            let alto = self.master.alto;
            ((((alto as f64 * prop / 2.0).round() * 2.0) as u32).max(2), alto & !1)
        };
        let (mut cw, mut ch) = (((sw as f64 * self.master.sup) as u32) & !1,
                                ((sh as f64 * self.master.sup) as u32) & !1);
        // el tope del codificador por hardware (8K): más arriba no hay motor
        if cw.max(ch) > 8192 {
            let k = 8192.0 / cw.max(ch) as f64;
            cw = ((cw as f64 * k) as u32) & !1;
            ch = ((ch as f64 * k) as u32) & !1;
        }
        (sw, sh, cw.max(2), ch.max(2))
    }

    /// tocar una fila del cajón: la MISMA geometría que la dibuja
    fn toca_cajon_master(&mut self, pr: &Proyecto, mx: f32, my: f32, x0: f32, y0: f32) {
        let filas = self.filas_master(y0 + 30.0);
        for (fy, k) in &filas {
            if my < *fy || my > *fy + 22.0 { continue; }
            let ancho = match k { 2 | 4 => 100.0f32, 5 => 58.0, _ => 76.0 };
            let i = ((mx - x0 - 92.0) / (ancho + 6.0)).floor();
            if i < 0.0 { return; }
            let i = i as usize;
            match k {
                0 => if let Some((a, _)) = prefs::ALTURAS_MASTER.get(i) {
                    self.master.alto = *a;
                },
                1 => if let Some((f, _, _)) = prefs::REVELADOS.get(i) { self.master.sup = *f; },
                2 => if let Some((c, _, _)) = prefs::CODECS_MASTER.get(i) {
                    self.master.codec = c.to_string();
                    if let Some(p) = PRESETS_REVELADO.iter().position(|x| x.2 == *c) {
                        self.preset_revelado = p;
                    }
                },
                3 => if let Some(mb) = prefs::CAUDALES.get(i) { self.master.mbps = *mb; },
                4 => if let Some((c, _)) = prefs::FILTROS_ESCALA.get(i) {
                    self.master.filtro = c.to_string();
                },
                _ => if let Some((f, _)) = prefs::CADENCIAS_MASTER.get(i) {
                    self.master.fps = *f;
                },
            }
            prefs::guarda_master(&pr.base, &self.master.clone());
            // tocar el cajón ES elegirlo: si cambias un ajuste, lo quieres
            self.preset_revelado = A_MANO;
            self.visor.foley(sonido::Foley::Tick);
            let (sw, sh, cw, ch) = self.medidas_master(pr);
            self.di(&format!("a mano: sale a {sw}×{sh} y se revela a {cw}×{ch}"));
            return;
        }
    }

    /// las filas del cajón del máster: (y, cuál) — una geometría, dos usos
    fn filas_master(&self, y0: f32) -> Vec<(f32, usize)> {
        (0..6).map(|k| (y0 + k as f32 * 34.0, k)).collect()
    }

    /// EL CAJÓN DEL MÁSTER: el catálogo que la sala se guardó mientras se
    /// medía el motor (MOTOR §8bis prometía abrirlo cuando estuviera hecho).
    /// El botón de siempre sigue siendo el botón: esto es para cuando el
    /// destino manda —un 8K para que la plataforma no se coma el grano, un
    /// supermuestreo para que no haya escalones— y da igual lo que tarde.
    fn dibuja_cajon_master(&self, pr: &Proyecto, d2: &mut ui::Dibujo, x0: f32, y0: f32) {
        use ui::Familia::*;
        let (w, h) = (700.0f32, 248.0);
        d2.rect(x0 + 4.0, y0 + 5.0, w, h, [0.0, 0.0, 0.0, 0.18]);
        d2.rect(x0, y0, w, h, [0.965, 0.953, 0.918, 1.0]);
        trazo::caja(d2, x0, y0, w, h, 1.6, paleta::ROJO, 1800);
        d2.texto_f(Grot, x0 + 14.0, y0 + 8.0, "EL CAJÓN DEL MÁSTER", 13.0, paleta::ROJO);
        // QUÉ SELLO ESTÁ PUESTO, aquí mismo: el cajón solo sale por la puerta
        // si el sello es «A MANO». Con cualquier otro esto es un formulario
        // que no hace nada, y hay que decirlo.
        let en_uso = self.preset_revelado == A_MANO;
        let (sw, sh, cw, ch) = self.medidas_master(pr);
        if en_uso {
            d2.texto(x0 + 210.0, y0 + 11.0,
                     &format!("EN USO · sale {sw}×{sh} · se revela {cw}×{ch}"),
                     8.5, paleta::ROJO);
        } else {
            d2.texto(x0 + 210.0, y0 + 11.0,
                     &format!("SIN USAR · el sello puesto es «{}» (al lienzo, sin escalar)",
                              PRESETS_REVELADO[self.preset_revelado.min(A_MANO)].0),
                     8.5, paleta::TINTA_TENUE);
        }
        let filas = self.filas_master(y0 + 30.0);
        let rotulos = ["sale a", "se revela a", "códec", "caudal", "al escalar",
                       "cadencia"];
        for (fy, k) in &filas {
            d2.texto(x0 + 14.0, *fy + 4.0, rotulos[*k], 8.5, paleta::TINTA_TENUE);
            let chip = |d2: &mut ui::Dibujo, i: usize, etq: &str, on: bool, ancho: f32| {
                let cx = x0 + 92.0 + i as f32 * (ancho + 6.0);
                if on { d2.rect(cx, *fy, ancho, 22.0, paleta::TINTA); }
                else { trazo::caja(d2, cx, *fy, ancho, 22.0, 1.0, paleta::TINTA_TENUE,
                                   1810 + (k * 8 + i) as u32); }
                d2.texto(cx + 6.0, *fy + 6.0, etq, 8.5,
                         if on { paleta::HUESO } else { paleta::TINTA });
            };
            match k {
                0 => for (i, (a, etq)) in prefs::ALTURAS_MASTER.iter().enumerate() {
                    chip(d2, i, etq, self.master.alto == *a, 76.0);
                },
                1 => for (i, (f, etq, _)) in prefs::REVELADOS.iter().enumerate() {
                    chip(d2, i, etq, (self.master.sup - f).abs() < 0.01, 76.0);
                },
                2 => for (i, (cl, etq, _)) in prefs::CODECS_MASTER.iter().enumerate() {
                    chip(d2, i, etq, self.master.codec == *cl, 100.0);
                },
                3 => for (i, mb) in prefs::CAUDALES.iter().enumerate() {
                    chip(d2, i, &format!("{mb} Mb/s"), self.master.mbps == *mb, 76.0);
                },
                4 => for (i, (cl, etq)) in prefs::FILTROS_ESCALA.iter().enumerate() {
                    chip(d2, i, etq, self.master.filtro == *cl, 100.0);
                },
                // LA CADENCIA: si no es la de la bobina, el revelado
                // interpola entre dos fotogramas de la fuente y no da tirón
                _ => for (i, (f, etq)) in prefs::CADENCIAS_MASTER.iter().enumerate() {
                    chip(d2, i, etq, (self.master.fps - f).abs() < 0.01, 58.0);
                },
            }
        }
        // el pie: lo que hay que saber de la fila elegida, sin dorar nada
        let pie = if let Some((_, _, nota)) = prefs::REVELADOS.iter()
            .find(|(f, _, _)| (self.master.sup - f).abs() < 0.01) {
            let cod = prefs::CODECS_MASTER.iter().find(|(c, _, _)| *c == self.master.codec)
                .map(|(_, _, n)| *n).unwrap_or("");
            format!("{nota} · {cod}")
        } else { String::new() };
        let pie: String = pie.chars().take(96).collect();
        d2.texto(x0 + 14.0, y0 + h - 24.0, &pie, 8.0, paleta::TINTA_TENUE);
        // y el precio en píxeles, que es lo que de verdad se paga
        let veces = (cw as f64 * ch as f64) / (sw.max(1) as f64 * sh.max(1) as f64);
        if veces > 1.05 {
            d2.texto(x0 + 14.0, y0 + h - 13.0,
                     &format!("el revelado mueve {veces:.1}× los píxeles: tardará ~{veces:.1}× más"),
                     8.0, paleta::ROJO);
        } else if veces < 0.95 {
            d2.texto(x0 + 14.0, y0 + h - 13.0,
                     "se revela más pequeño: va más rápido, y el grano sale más gordo",
                     8.0, paleta::TINTA_TENUE);
        }
    }

    /// EL NOMBRE DEL MÁSTER. Antes salían `bobina_2.mp4`, `bobina_3.mp4`…
    /// sin decir cuál era cuál (§5): ahora lleva la FECHA, que es lo que uno
    /// busca cuando abre la carpeta tres días después.
    fn nombre_del_master(&self, pr: &Proyecto) -> String {
        let base = self.etiqueta.clone()
            .filter(|e| !e.trim().is_empty())
            .unwrap_or_else(|| pr.nombre.clone())
            .replace(' ', "_");
        // sin cronos ni dependencias: el sello de UNIX pasado a AAAAMMDD-hhmm
        let seg = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs()).unwrap_or(0) as i64;
        let (dias, resto) = (seg / 86_400, seg % 86_400);
        let (hh, mm) = (resto / 3600, (resto % 3600) / 60);
        // civil_from_days de Howard Hinnant, que es cuatro líneas y no miente
        let z = dias + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let dia = doy - (153 * mp + 2) / 5 + 1;
        let mes = if mp < 10 { mp + 3 } else { mp - 9 };
        let anio = yoe + era * 400 + if mes <= 2 { 1 } else { 0 };
        let tramo = if pr.rango.is_some() { "_tramo" } else { "" };
        format!("{base}{tramo}_{anio:04}{mes:02}{dia:02}-{hh:02}{mm:02}")
    }

    /// EL OÍDO: subtítulos automáticos de TODA la bobina, con un modelo que
    /// corre en esta máquina y sin que salga nada a ninguna red.
    ///
    /// Se manda la lista de planos con sonido —fichero, trozo, dónde cae y a
    /// qué velocidad— y el shell carga el modelo UNA vez para todos. Los
    /// tiempos vuelven ya en segundos de la bobina.
    fn pon_el_oido(&mut self, pr: &mut Proyecto) {
        if self.oyendo.is_some() { self.di("el oído ya está escuchando"); return; }
        if self.revelando.is_some() { self.di("espera a que acabe el revelado"); return; }
        let inicios = pr.inicios();
        let trabajos: Vec<serde_json::Value> = pr.clips.iter().enumerate()
            // marcha atrás y congelado se quedan fuera: al revés no hay habla
            // que transcribir, y congelado no suena
            .filter(|(_, c)| !c.hueco && !c.mute && !c.ausente && c.anidada.is_none()
                    && c.speed > 0.02)
            .map(|(i, c)| serde_json::json!({
                "file": c.media,
                "in": c.t_in, "out": c.t_out,
                // el desfase del sonido cuenta: si la voz va corrida, el pie
                // tiene que ir corrido con ella
                "desde": inicios.get(i).copied().unwrap_or(0.0) + c.desfase,
                "speed": c.speed,
            })).collect();
        if trabajos.is_empty() {
            self.di("no hay ningún plano con sonido que escuchar");
            return;
        }
        let lista = std::env::temp_dir().join("saorin_oido.json");
        if std::fs::write(&lista, serde_json::json!(trabajos).to_string()).is_err() {
            self.di("no pude preparar el trabajo del oído"); return;
        }
        let srt = std::env::temp_dir().join("saorin_pie.srt");
        let _ = std::fs::remove_file(&srt);
        let nom = if cfg!(windows) { "laboratorios-saorin.exe" } else { "laboratorios-saorin" };
        let bin = std::env::current_exe().ok()
            .and_then(|p| p.parent().map(|d| d.join(nom)))
            .filter(|p| p.is_file())
            .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent().unwrap().join("shell/target/release").join(nom));
        match std::process::Command::new(&bin)
            .args(["cli", "oye", "--trabajos", lista.to_str().unwrap_or(""),
                   // sin «--modelo»: lo elige el taller según la máquina
                   "--idioma", "es",
                   "--out", srt.to_str().unwrap_or("")])
            .env("FL_MEDIA", pr.base.join("media"))
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(mut c) => {
                *self.progreso.lock().unwrap() = (0.0, "el oído".into());
                if let Some(err) = c.stderr.take() {
                    let prog = self.progreso.clone();
                    std::thread::spawn(move || {
                        use std::io::BufRead;
                        for linea in std::io::BufReader::new(err).lines().flatten() {
                            // el diario del shell viene con «⟨ 1.2s⟩ » delante
                            let l = linea.rsplit('⟩').next().unwrap_or(&linea).trim().to_string();
                            if !l.is_empty() && !l.starts_with('%') {
                                let mut p = prog.lock().unwrap();
                                p.1 = l.chars().take(60).collect();
                            }
                        }
                    });
                }
                self.oyendo = Some((c, srt));
                self.di("el oído: escuchando la bobina…");
            }
            Err(_) => self.di("no encuentro el oído (falta el binario del taller)"),
        }
    }

    /// ¿ha terminado el oído? Si sí, el .srt entra en la pista del pie.
    fn atiende_al_oido(&mut self, pr: &mut Proyecto) {
        let Some((hijo, srt)) = self.oyendo.as_mut() else { return };
        let Ok(Some(st)) = hijo.try_wait() else { return };
        let srt = srt.clone();
        self.oyendo = None;
        if !st.success() {
            let (_, motivo) = self.progreso.lock().map(|p| p.clone())
                .unwrap_or((0.0, String::new()));
            self.di(&format!("el oído falló: {motivo}"));
            return;
        }
        let Ok(texto) = std::fs::read_to_string(&srt) else {
            self.di("el oído no dejó nada escrito"); return;
        };
        let nuevos = subtitulo::de_srt(&texto);
        if nuevos.is_empty() { self.di("no se oyó nada que subtitular"); return; }
        self.recuerda(pr);
        pr.subs = nuevos;
        let _ = pr.guarda();
        self.refresca_pie(pr);
        self.visor.foley(sonido::Foley::Lata);
        self.di(&format!("{} subtítulo(s) — clic para corregirlos", pr.subs.len()));
        Self::avisa_al_sistema("Subtítulos listos",
                               &format!("{} líneas en la pista del pie", pr.subs.len()));
    }

    /// LA REJILLA DE MANDOS DEL PIE, en un solo sitio: la lección de la sala
    /// de revelado (si el dibujo y el ratón llevan sus números aparte, se
    /// separan solos en cuanto una ficha crece).
    fn pie_estilo_y(&self, pr: &Proyecto, is: usize) -> f32 {
        let vivo = self.escribiendo_sub.as_ref().filter(|(k, _)| *k == is)
            .map(|(_, t)| t.clone())
            .unwrap_or_else(|| pr.subs.get(is).map(|s| s.texto.clone()).unwrap_or_default());
        let n = subtitulo::parte(&vivo, pr.estilo_sub.ancho_linea as usize).len().max(1);
        // el mismo recorrido que el dibujo: cabecera, texto, cinco filas
        Self::CABECERA + 8.0 + 20.0 + 20.0 + 17.0 * n as f32 + 15.0 * 4.0 + 22.0 + 16.0
    }

    /// REHACER EL PIE tras tocar el texto o el estilo: rasteriza al lienzo
    /// del máster (para que la preview enseñe exactamente lo que va a salir)
    /// y le dice a la preview que se olvide de lo que tenía.
    fn refresca_pie(&mut self, pr: &mut Proyecto) {
        let (pw, ph) = self.lienzo_del_master(pr);
        pr.refresca_pie(pw as u32, ph as u32);
        self.visor.olvida_capas();
        self.visor.busca(pr, self.visor.t);
    }

    /// SACAR LA COPIA (la ampliadora del cuarto oscuro): el fotograma que se
    /// está mirando, revelado por el MISMO motor que la bobina —misma receta,
    /// mismas capas, mismo encuadre, mismo grano— y escrito como imagen. No
    /// es un fotograma robado del máster: no pasa por el códec ni por el YUV
    /// de rango limitado con el croma a la mitad.
    fn saca_copia(&mut self, pr: &Proyecto) {
        let t = self.visor.t;
        self.revela(pr, self.preset_revelado, Some(t));
    }

    fn revela(&mut self, pr: &Proyecto, preset: usize, copia: Option<f64>) {
        if pr.clips.is_empty() { self.di("la bobina está vacía"); return; }
        if self.revelando.is_some() {
            if copia.is_some() {
                self.di("espera: hay un revelado en marcha");
                return;
            }
            self.cola_revelado.push(preset);
            self.di(&format!("a la cola: {} lata(s) esperando", self.cola_revelado.len()));
            return;
        }
        // MATERIAL AUSENTE: se dice ANTES y por su nombre, no cien líneas más
        // abajo con un error del motor que no explica nada (§4)
        let faltan: Vec<String> = pr.clips.iter()
            .filter(|c| c.ausente).map(|c| c.media.clone()).collect();
        if !faltan.is_empty() {
            self.di(&format!("falta material: {} — vuelve a enlazarlo en la ficha",
                             faltan.join(", ")));
            return;
        }
        // EL SELLO MANDA. Los dos primeros son los caminos de la casa y no
        // miran el cajón: al lienzo, sin escalar, con el motor del chip. Solo
        // «A MANO» saca lo que haya en el cajón.
        let sello = PRESETS_REVELADO[preset.min(PRESETS_REVELADO.len() - 1)];
        let a_mano = preset == A_MANO;
        let codec = if a_mano { self.master.codec.clone() } else { sello.2.to_string() };
        let (alto, sup, mbps, filtro) = if a_mano {
            (self.master.alto, self.master.sup, self.master.mbps, self.master.filtro.clone())
        } else {
            (0, 1.0, 60, String::new())
        };
        // LA COPIA manda sobre el sello: su tamaño y su supermuestreo salen de
        // la ampliadora, no del cajón del revelado (son dos cosas distintas y
        // compartir botón acaba en un 8K puesto sin querer).
        let (alto, sup, filtro) = match copia {
            Some(_) => {
                let (_, mult, sp) = Self::COPIA_TAM[(self.master.copia_tam as usize).min(2)];
                let (_, ph_l) = self.lienzo_del_master(pr);
                (ph_l as u32 * mult, sp, String::new())
            }
            None => (alto, sup, filtro),
        };
        // LA CADENCIA DEL MÁSTER, también sólo «A MANO». Si es otra que la de
        // la bobina, el revelado interpola entre los dos fotogramas vecinos de
        // la fuente en vez de saltar al más cercano (plan.rs): es la
        // diferencia entre un movimiento liso y el tirón de 3, 2, 3, 2.
        let fps_master = if a_mano && self.master.fps > 0.0 {
            self.master.fps
        } else { pr.fps };
        let loudnorm = prefs::NORMALIZA.load(std::sync::atomic::Ordering::Relaxed);
        // EL FORMATO DEL PROYECTO VA EN EL PAYLOAD (mismo cálculo que el
        // cartel de la sala)
        let (pw, ph) = self.lienzo_del_master(pr);
        // ── ¿SOLO EL TRAMO MARCADO? (§4bis.2) ──────────────────────────
        // El rango recorta la bobina que se manda: es lo prometido en
        // MOTOR §7 y lo que permite enseñar un trozo sin sacarla entera.
        let (ra, rb) = pr.tramo();
        // UNA COPIA NO SE RECORTA: se pide un instante de la bobina entera, y
        // si el rango dejara fuera ese fotograma no habría copia que sacar
        let solo_tramo = pr.rango.is_some() && copia.is_none();
        let inicios = pr.inicios();
        let clips: Vec<serde_json::Value> = pr.clips.iter().enumerate().filter_map(|(i, c)| {
            let ini = inicios.get(i).copied().unwrap_or(0.0);
            let fin = ini + c.dur();
            if solo_tramo && (fin <= ra + 1e-6 || ini >= rb - 1e-6) { return None; }
            // el clip se recorta por donde lo parta el rango
            let (mut t_in, mut t_out) = (c.t_in, c.t_out);
            if solo_tramo && !c.congelado() {
                let v = c.speed.abs().max(0.02);
                if ini < ra { let d = (ra - ini) * v;
                    if c.speed < 0.0 { t_out -= d; } else { t_in += d; } }
                if fin > rb { let d = (fin - rb) * v;
                    if c.speed < 0.0 { t_in += d; } else { t_out -= d; } }
            } else if solo_tramo {
                // congelado: lo que se recorta es la DURACIÓN
                let dentro = fin.min(rb) - ini.max(ra);
                t_out = t_in + dentro.max(0.04);
            }
            let mut o = serde_json::json!({
                "file": c.media, "in": t_in, "out": t_out, "fade": c.fade,
                "speed": c.speed, "mute": c.mute, "desfase": c.desfase,
                // EL ENCUADRE, TAL CUAL. Es el mismo modelo en la app y en el
                // revelado, así que aquí no se traduce nada — que es lo que
                // hacía que el encuadre no saliera en el máster.
                "tf": c.enc.json(),
                "cuartos": c.cuartos_fichero,
                // la receta del clip, que puede no ser la de la bobina
                "prefs": c.prefs.clone()
            });
            // el clip anidado viaja con su clave; se aplana aquí mismo antes
            // de mandar (CAPAS §8)
            if let Some(a) = &c.anidada { o["anidada"] = serde_json::json!(a); }
            Some(o)
        }).collect();
        if clips.is_empty() { self.di("el rango no coge ni un clip"); return; }
        let desliza = if solo_tramo { ra } else { 0.0 };
        let payload = serde_json::json!({
            "project": { "w": pw, "h": ph, "fps": fps_master },
            "out_name": self.nombre_del_master(pr),
            "out_dir": self.destino.as_ref().map(|d| d.to_string_lossy().to_string()),
            "master": {
                "codec": codec, "loudnorm": loudnorm,
                // EL CAJÓN (§8bis, la promesa cumplida): a qué tamaño sale, a
                // qué escala se revela, con cuánto caudal y con qué filtro.
                // Con los valores por defecto esto es exactamente el camino
                // rápido de siempre: ni un pase de más.
                "alto": alto,
                "super": sup,
                // EL LABORATORIO: un fichero por plano en vez de una bobina
                "sueltos": preset == EN_CLIPS && copia.is_none(),
                // LA COPIA: un fotograma, no una película (MOTOR §12)
                "still": copia.map(|t| serde_json::json!({
                    "t": t,
                    "papel": Self::COPIA_PAPEL[(self.master.copia_papel as usize).min(2)].1,
                })),
                "bitrate": mbps as i64 * 1_000_000,
                "filtro": filtro,
            },
            // LAS GELATINAS DEL CUARTO OSCURO. Sin esto el revelador tiraba de
            // las de por defecto: el autor cambiaba el baño y el máster salía
            // con otro. (Estaban en la sala, en el parte de revelado, y no
            // llegaban al payload.)
            "lut_in": pr.lut_in.as_ref().and_then(|p| p.file_name())
                        .map(|s| s.to_string_lossy().to_string()),
            "lut": pr.lut_color.as_ref().and_then(|p| p.file_name())
                        .map(|s| s.to_string_lossy().to_string()),
            "clips": clips,
            // el nivel del margen se suma al de cada pista: lo que se oye en
            // la mesa es lo que sale al máster (§1.6)
            "audio": pr.audio.iter().filter(|a| !a.mute && !pr.mudo_musica)
                .filter(|a| !solo_tramo
                    || (a.entra() < rb && a.entra() + a.dur() > ra))
                .map(|a| serde_json::json!({
                    "file": a.media, "in": a.t_in, "out": a.t_out,
                    "start": (a.entra() - desliza).max(0.0),
                    "gain": a.gain + pr.vol_musica,
                    "fadeIn": a.fade_in, "fadeOut": a.fade_out,
                })).collect::<Vec<_>>(),
            "vol_voz": if pr.mudo_voz { -60.0 } else { pr.vol_voz },
            "prefs": pr.clips.first().map(|c| c.prefs.clone()).unwrap_or(pr.prefs.clone()),
        });
        // ── LAS CAPAS (CAPAS §3): el carril de encima, recortado al rango ──
        // EL PIE VIAJA CON LAS CAPAS: para el motor un subtítulo es una capa
        // RGBA más (subtitulo.rs), así que aquí no hay caso especial ninguno
        let mut orden_capas: Vec<usize> = (0..pr.cuantas_capas()).collect();
        orden_capas.sort_by_key(|&k| (pr.capa_num(k).map(|c| c.pista).unwrap_or(0), k));
        let capas_payload: Vec<serde_json::Value> = orden_capas.into_iter()
            .filter_map(|k| pr.capa_num(k)).filter_map(|cp| {
            let (ini, fin) = (cp.start, cp.fin());
            if solo_tramo && (fin <= ra + 1e-6 || ini >= rb - 1e-6) { return None; }
            let (mut c_in, mut c_out, mut st) = (cp.c.t_in, cp.c.t_out, ini);
            if solo_tramo {
                let v = cp.c.speed.abs().max(0.02);
                if ini < ra { c_in += (ra - ini) * v; st = ra; }
                if fin > rb { c_out -= (fin - rb) * v; }
            }
            let mut o = serde_json::json!({
                "file": cp.c.media, "in": c_in, "out": c_out,
                "start": st - if solo_tramo { ra } else { 0.0 },
                "speed": cp.c.speed,
                "tf": cp.c.enc.json(), "cuartos": cp.c.cuartos_fichero,
                "prefs": cp.c.prefs.clone(),
            });
            if cp.fundido_in > 0.001 { o["fadeIn"] = serde_json::json!(cp.fundido_in); }
            if cp.fundido_out > 0.001 { o["fadeOut"] = serde_json::json!(cp.fundido_out); }
            Some(o)
        }).collect();
        let mut payload = payload;
        if !capas_payload.is_empty() {
            payload["clips2"] = serde_json::json!(capas_payload);
        }
        // ── EL APLANADO de las anidadas, aquí y no en el shell: la app tiene
        // las hijas cargadas y el shell no sabe de claves (CAPAS §8)
        let hay_anidadas = pr.clips.iter().any(|c| c.anidada.is_some());
        if hay_anidadas {
            let subs = &pr.subbobinas;
            let media_dir = pr.base.join("media");
            let res = filmlook_core::plan::aplana_anidadas(&mut payload,
                &|clave| subs.get(clave).map(proyecto::payload_de_sub),
                &|f| {
                    let ruta = std::path::Path::new(f);
                    let ruta = if ruta.is_file() { ruta.to_path_buf() }
                               else { media_dir.join(f) };
                    filmlook_core::indice::sondea(&ruta).ok()
                        .map(|(w, h, _, _)| (w as f32, h as f32))
                });
            match res {
                Ok(n) if n > 0 => self.di(&format!("{n} bobina(s) anidada(s), aplanadas")),
                Err(e) => { self.di(&format!("anidadas: {e}")); return; }
                _ => {}
            }
        }
        let tmp = std::env::temp_dir().join("saorin_revelado.json");
        if std::fs::write(&tmp, payload.to_string()).is_err() { self.di("no se pudo preparar"); return; }
        // EL REVELADOR: primero AL LADO de este ejecutable (una instalación de
        // verdad, los tres binarios en la misma carpeta) y sólo si no está,
        // el árbol de compilación. Es el mismo orden que usa el shell para
        // encontrar el motor; sin esto, mover la app la dejaba sin revelado.
        let nom = if cfg!(windows) { "laboratorios-saorin.exe" } else { "laboratorios-saorin" };
        let bin = std::env::current_exe().ok()
            .and_then(|p| p.parent().map(|d| d.join(nom)))
            .filter(|p| p.is_file())
            .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent().unwrap().join("shell/target/release").join(nom));
        match std::process::Command::new(&bin)
            .args(["cli", "render", "--json", tmp.to_str().unwrap_or("")])
            .env("FL_MEDIA", pr.base.join("media"))
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(mut c) => {
                // el CLI escupe «· paso (NN%)» a stderr: un hilo lo lee
                *self.progreso.lock().unwrap() = (0.0, "preparando".into());
                self.revelado_desde = std::time::Instant::now();
                if let Some(err) = c.stderr.take() {
                    let prog = self.progreso.clone();
                    std::thread::spawn(move || {
                        use std::io::BufRead;
                        for linea in std::io::BufReader::new(err).lines().flatten() {
                            if let (Some(a), Some(b)) = (linea.rfind('('), linea.rfind("%)")) {
                                if let Ok(p) = linea[a + 1..b].trim().parse::<f32>() {
                                    let paso = linea.trim_start_matches('·').trim();
                                    let paso = paso[..paso.rfind('(').unwrap_or(paso.len())]
                                        .trim().to_string();
                                    *prog.lock().unwrap() = (p / 100.0, paso);
                                }
                            }
                        }
                    });
                }
                self.revelando = Some(c);
                self.di("revelando…");
            }
            Err(_) => self.di("no encuentro el revelador"),
        }
    }

    /// un clic: decide según la zona
    fn pulsa(&mut self, pr: &mut Proyecto) {
        // el panel de AJUSTES abierto: filas conmutables
        if self.ajustes {
            let (ancho, alto) = self.gpu.alto_ancho();
            let (w, h) = (620.0f32.min(ancho - 40.0), 620.0f32.min(alto - 40.0));
            let (x, y) = ((ancho - w) / 2.0, (alto - h) / 2.0);
            let (mx, my) = self.raton;
            if mx < x || mx > x + w || my < y || my > y + h {
                self.ajustes = false;
                return;
            }
            let fila = ((my - y - Self::AJUSTES_Y0 + 6.0) / Self::AJUSTES_FILA) as i32;
            self.toca_ajuste(pr, fila);
            return;
        }
        let (ancho, alto) = self.gpu.alto_ancho();
        let (mx, my) = self.raton;
        let banco = self.banco_y();

        // el aviso de rescate manda mientras esté puesto
        if let Some(copia) = self.rescate.clone() {
            let (bw, bh) = (520.0f32, 62.0);
            let bx = (ancho - bw) / 2.0;
            let by = Self::CABECERA + 6.0;
            if mx >= bx && mx <= bx + bw && my >= by && my <= by + bh {
                if my >= by + 36.0 && my <= by + 58.0 {
                    if mx >= bx + 14.0 && mx <= bx + 106.0 {
                        self.rescate = None;
                        self.recupera_esta_copia(pr, &copia);
                    } else if mx >= bx + 116.0 && mx <= bx + 208.0 {
                        self.rescate = None;
                        self.di("seguimos con la bobina como estaba");
                    }
                }
                return;
            }
        }

        // ── la cabecera: cancelar revelado / mostrar la revelada ──
        if my < Self::CABECERA {
            if self.revelando.is_some() && mx > ancho - 640.0 && mx < ancho - 490.0 {
                if let Some(mut h) = self.revelando.take() {
                    let _ = h.kill();
                    self.di("revelado CANCELADO");
                }
                return;
            }
            if let Some(r) = self.ultima_revelada.clone() {
                if mx > ancho - 640.0 && mx < ancho - 420.0 {
                    #[cfg(target_os = "macos")]
                    let _ = std::process::Command::new("open").arg("-R").arg(&r).spawn();
                    #[cfg(target_os = "windows")]
                    let _ = std::process::Command::new("explorer")
                        .arg(format!("/select,{}", r.display())).spawn();
                    return;
                }
            }
            return;
        }
        // ── la cinta de 6 de la FUENTE: tocar un fotograma salta allí ──
        if let Some(f) = &self.fuente {
            let zona_w = ancho - Self::ESTANTE_W - Self::INSPECTOR_W - 40.0;
            let zona_h = banco - Self::CABECERA - 30.0;
            let prop = pr.proporcion();
            let mut gw = zona_w;
            let mut gh = gw / prop;
            if gh > zona_h { gh = zona_h; gw = gh * prop; }
            let gx = Self::ESTANTE_W + 20.0 + (zona_w - gw) / 2.0;
            let gy = Self::CABECERA + 15.0 + (zona_h - gh) / 2.0;
            let aw6 = ((gw - 36.0) / 6.0).min(108.0);
            let ah6 = aw6 * 9.0 / 16.0;
            let sx6 = gx + (gw - aw6 * 6.0) / 2.0;
            let sy6 = gy + gh - ah6 - 24.0;
            if my >= sy6 - 4.0 && my <= sy6 + ah6 + 4.0 && mx >= sx6 && mx <= sx6 + aw6 * 6.0 {
                let k = (((mx - sx6) / aw6) as usize).min(5);
                let frac = [0.02f64, 0.2, 0.4, 0.6, 0.8, 0.98][k];
                let t6 = f.cinta.dur * frac;
                self.visor.busca(pr, t6);
                self.visor.foley(sonido::Foley::Tick);
                return;
            }
        }

        // ── la estantería ──
        //
        // `my < banco` NO SOBRA. Sin él esta rama se quedaba con la columna
        // izquierda ENTERA y siempre retornaba: el margen del banco —los dos
        // mandos de nivel, las dos palancas de silencio y la manivela— era
        // código inalcanzable. De ahí que «la esquina de abajo a la izquierda
        // no reaccione al ratón»: no es que no reaccionara, es que el clic se
        // lo comía la estantería veinte líneas antes.
        if mx < Self::ESTANTE_W && my > Self::CABECERA && my < banco {
            // «+ importar» en la cabecera del estante
            if my < Self::CABECERA + 28.0 {
                self.importa_dialogo(pr);
                return;
            }
            // las pestañitas de filtro
            if my < Self::CABECERA + 50.0 {
                let k = ((mx - 12.0) / 54.0).floor();
                if (0.0..4.0).contains(&k) {
                    self.filtro = k as u8;
                    self.visor.foley(sonido::Foley::Tick);
                }
                return;
            }
            // plegar/desplegar una balda
            if let Some(balda) = self.balda_en(mx, my) {
                if !self.baldas_cerradas.remove(&balda) {
                    self.baldas_cerradas.insert(balda);
                }
                self.visor.foley(sonido::Foley::Tick);
                return;
            }
            // ¿UN RECORTE DEL CUBO? Se coge con la pinza. Al soltar se
            // decide: si el ratón apenas se movió fue un CLIC (va a la aguja);
            // si se arrastró, cae donde se suelte.
            if let Some(ir) = self.recorte_en(mx, my) {
                self.cubo_pinza = Some((ir, mx, my));
                self.visor.foley(sonido::Foley::Tick);
                return;
            }
            // ── LA LATA SE COGE CON LA MANO (1.1) ────────────────────────
            // Pulsar NO hace nada todavía: se apunta de dónde salió. Al
            // soltar se decide — quieto es un clic (fuente / doble toque) y
            // movido es un ARRASTRE que mete la cinta donde se suelte.
            if let Some(idx) = self.lata_en(mx, my) {
                self.lata_pinza = Some((idx, mx, my));
                self.visor.foley(sonido::Foley::Tick);
            }
            return;
        }

        // ── la ficha del clip (panel derecho) ──
        self.pulsa_ficha(pr)
    }

    /// SOLTAR LA LATA que se cogió de la estantería. Devuelve `true` si el
    /// gesto era suyo.
    fn suelta_lata(&mut self, pr: &mut Proyecto, mx: f32, my: f32) -> bool {
        let Some((idx, px, py)) = self.lata_pinza.take() else { return false };
        let movido = (mx - px).abs() > 6.0 || (my - py).abs() > 6.0;
        let Some(c) = self.estanteria.get(idx).cloned() else { return true };
        // ── UNA CINTA A LA PAPELERA ──────────────────────────────────────
        // Sale de la ESTANTERÍA, no del disco. El taller trabaja por
        // referencia (NORTE §1.4): el material es del autor y no se borra
        // nunca desde aquí. Lo que se quita es la ficha.
        if movido && self.en_la_papelera(mx, my) {
            if proyecto::quita_cinta(&pr.base, &c.nombre) {
                self.estanteria = pr.estanteria();
                self.visor.foley(sonido::Foley::Corte);
                self.di(&format!("«{}» fuera de la estantería · el fichero sigue donde estaba",
                                 c.nombre.chars().take(24).collect::<String>()));
            } else {
                self.di("no pude quitarla de la estantería");
            }
            return true;
        }
        if movido {
            // ARRASTRE: la cinta entera entra en la junta que marca el ratón
            if mx <= Self::ESTANTE_W {
                self.di("suéltala sobre la bobina para colocarla");
                return true;
            }
            // ── SOLTARLA EN EL CARRIL DE LA CAPA = una capa nueva ────────
            // Vale un vídeo (PiP) o una foto/rótulo (con su alfa). El carril
            // es la franja fina encima de la tira.
            if c.fps >= 0.0 && self.pista_capa_en(my).is_some() && mx > Self::ESTANTE_W {
                self.recuerda(pr);
                let start = self.tiempo_en(mx).max(0.0);
                let mut clip = pr.clip_de(&c);
                // una capa entra DIRECTA: el baño de la casa es para el
                // material de cámara, no para un rótulo
                if crate::foto::es_foto(&clip.ruta) {
                    clip.prefs = serde_json::json!({});
                    clip.lut_in = None;
                    clip.lut_color = None;
                }
                // a la pista que haya bajo el ratón (V2 si no acierta)
                let pista = self.pista_capa_en(my).unwrap_or(0);
                pr.capas.push(proyecto::Capa {
                    c: clip, start, pista, fundido_in: 0.0, fundido_out: 0.0,
                });
                let _ = pr.guarda();
                self.sel_capa = Some(pr.capas.len() - 1);
                self.sel = None;
                self.visor.foley(sonido::Foley::Lata);
                self.di(&format!("«{}» a la capa, en {start:.1} s", c.nombre));
                return true;
            }
            if c.fps < 0.0 {
                // una cinta de AUDIO cae en su carril, empezando donde se suelte
                self.recuerda(pr);
                let dur = sonido::dur_de(&c.ruta).unwrap_or(30.0);
                let start = self.tiempo_en(mx).max(0.0);
                pr.audio.push(proyecto::ClipAudio {
                    media: c.nombre.clone(), ruta: c.ruta.clone(),
                    t_in: 0.0, t_out: dur, start, gain: 0.0,
                    fade_in: 0.0, fade_out: 0.0, banda: Vec::new(), mute: false,
                    pista: self.pista_en(my).unwrap_or(0),
                    desfase: 0.0,
                });
                let _ = pr.guarda();
                self.sel_audio = Some(pr.audio.len() - 1);
                self.visor.foley(sonido::Foley::Lata);
                self.di(&format!("«{}» a la música en {start:.1} s", c.nombre));
                return true;
            }
            self.recuerda(pr);
            let idx_ins = self.junta_en(pr, mx);
            let mut nuevo = pr.clip_de(&c);
            nuevo.prefs = pr.prefs.clone();
            nuevo.lut_in = pr.lut_in.clone();
            nuevo.lut_color = pr.lut_color.clone();
            pr.clips.insert(idx_ins, nuevo);
            pr.cuantiza();
            let _ = pr.guarda();
            self.sel = Some(idx_ins);
            self.seleccion.clear();
            self.visor.foley(sonido::Foley::Lata);
            self.visor.busca(pr, self.visor.t);
            self.di(&format!("«{}» a la bobina, donde la soltaste", c.nombre));
            return true;
        }
        // CLIC: lo de siempre — un toque abre la fuente, dos la meten entera
        let doble = self.ultima_lata.0 == idx
            && self.ultima_lata.1.elapsed().as_secs_f64() < 0.45;
        self.ultima_lata = (idx, std::time::Instant::now());
        if doble && c.fps < 0.0 {
            // cinta de AUDIO: a la pista de música, en la aguja
            self.recuerda(pr);
            let dur = sonido::dur_de(&c.ruta).unwrap_or(30.0);
            pr.audio.push(proyecto::ClipAudio {
                media: c.nombre.clone(), ruta: c.ruta.clone(),
                t_in: 0.0, t_out: dur,
                start: self.visor.t, gain: 0.0,
                fade_in: 0.0, fade_out: 0.0,
                banda: Vec::new(),
                mute: false, pista: 0, desfase: 0.0,
            });
            let _ = pr.guarda();
            self.di(&format!("«{}» a la pista de música", c.nombre));
            return true;
        }
        if doble {
            // doble toque: la cinta ENTERA a la bobina
            if self.fuente.is_some() { self.sale_fuente(pr); }
            self.recuerda(pr);
            pr.anade(&c);
            let _ = pr.guarda();
            self.sel = Some(pr.clips.len() - 1);
            self.di(&format!("«{}» a la bobina", c.nombre));
            self.visor.foley(sonido::Foley::Lata);
            self.visor.busca(pr, self.visor.t);
        } else if c.fps < 0.0 {
            self.di("doble toque: a la música · arrástrala para colocarla");
        } else {
            // un toque: al monitor de FUENTE
            let volver = if let Some(f) = &self.fuente { f.t_bobina } else { self.visor.t };
            self.fuente = Some(FuenteUi {
                cinta: c.clone(), marca_i: None, marca_o: None, t_bobina: volver,
            });
            self.visor.fuente = Some((c.ruta.clone(), c.dur.max(0.1)));
            self.visor.tocando = false;
            self.visor.busca(pr, 0.0);
            self.di(&format!("fuente: «{}» — I/O marcan, ⏎ inserta", c.nombre));
        }
        true
    }

    /// LA JUNTA donde cae un punto de la bobina: si el ratón está en la mitad
    /// derecha del clip que hay debajo, el material entra DESPUÉS. Es la
    /// misma regla que usa el cubo, y ahora la comparten los dos.
    fn junta_en(&self, pr: &Proyecto, mx: f32) -> usize {
        let t = self.tiempo_en(mx);
        match pr.en(t) {
            Some((j, _)) => {
                let ini = pr.inicios().get(j).copied().unwrap_or(0.0);
                let dur = pr.clips[j].dur().max(1e-6);
                if (t - ini) / dur > 0.5 { j + 1 } else { j }
            }
            None => pr.clips.len(),
        }.min(pr.clips.len())
    }

    /// LAS DOS FILAS DE BOTONES de la ficha de música: una geometría, dos
    /// usos (dibujar y pulsar)
    /// LAS TRES FILAS DE BOTONES de la ficha de la música. Una geometría con
    /// nombre que leen el dibujo Y el clic — el mismo trato que la sala de
    /// revelado, y por el mismo motivo.
    fn musica_botones_y() -> (f32, f32) {
        (Self::CABECERA + 213.0, Self::CABECERA + 237.0)
    }
    /// la tercera fila: los fundidos, que en la ficha del CLIP se ciclan con
    /// un clic y aquí no existían
    fn musica_fila3_y() -> f32 { Self::CABECERA + 261.0 }
    /// la cuarta: el compás (marcas al ritmo, y quitarlas)
    fn musica_fila4_y() -> f32 { Self::CABECERA + 285.0 }

    /// clics del panel derecho, el banco y el vidrio
    fn pulsa_ficha(&mut self, pr: &mut Proyecto) {
        let (ancho, alto) = self.gpu.alto_ancho();
        let (mx, my) = self.raton;
        let banco = self.banco_y();
        // ── LA FICHA DEL PIE manda si hay un subtítulo elegido ───────────
        if let Some(is) = self.sel_sub {
            let fx = ancho - Self::INSPECTOR_W + 10.0;
            if mx > fx - 6.0 && is < pr.subs.len() {
                // el texto: clic encima = escribirlo
                let alto_txt = 14.0 + 17.0 * subtitulo::parte(&pr.subs[is].texto,
                    pr.estilo_sub.ancho_linea as usize).len().max(1) as f32;
                let y_txt = Self::CABECERA + 26.0;
                if my >= y_txt - 2.0 && my <= y_txt + alto_txt {
                    self.escribiendo_sub = Some((is, pr.subs[is].texto.clone()));
                    self.di("escribe el subtítulo · ⏎ para guardarlo");
                    return;
                }
                // LOS MANDOS DEL ESTILO: la misma rejilla que los dibuja
                let ey = self.pie_estilo_y(pr, is);
                let col = if mx >= fx + 4.0 && mx <= fx + 78.0 { Some(0) }
                          else if mx >= fx + 84.0 && mx <= fx + 158.0 { Some(1) }
                          else { None };
                let fila = if my >= ey && my <= ey + 116.0 {
                    Some(((my - ey) / 24.0).floor() as usize) } else { None };
                if let (Some(c), Some(f)) = (col, fila) {
                    let atras = self.mods.shift_key();
                    let paso = |v: f32, d: f32, lo: f32, hi: f32, atras: bool| -> f32 {
                        let n = if atras { v - d } else { v + d };
                        if n > hi + 1e-4 { lo } else if n < lo - 1e-4 { hi } else { n }
                    };
                    self.recuerda(pr);
                    // cualquier gesto de la ficha cierra la escritura: si no,
                    // el índice guardado puede señalar a otro subtítulo
                    // después de partir o de quitar
                    self.escribiendo_sub = None;
                    let e = &mut pr.estilo_sub;
                    let mut quitar = false;
                    let mut partir = false;
                    match (f, c) {
                        (0, 0) => e.familia = (e.familia + 1) % 3,
                        (0, 1) => e.tinta = (e.tinta + 1) % 4,
                        (1, 0) => e.cuerpo = paso(e.cuerpo, 0.004, 0.026, 0.075, atras),
                        (1, 1) => e.margen = paso(e.margen, 0.015, 0.03, 0.30, atras),
                        (2, 0) => e.sombra = paso(e.sombra, 0.25, 0.0, 1.0, atras),
                        (2, 1) => e.caja = if e.caja > 0.01 { 0.0 } else { 0.55 },
                        (3, 0) => e.mayusculas = !e.mayusculas,
                        (3, 1) => e.ancho_linea = if e.ancho_linea >= 46 { 28 }
                                                  else { e.ancho_linea + 6 },
                        (4, 0) => partir = true,
                        (4, 1) => quitar = true,
                        _ => {}
                    }
                    if quitar {
                        let q = pr.subs.remove(is);
                        self.sel_sub = None;
                        self.di(&format!("fuera «{}»",
                                         q.texto.chars().take(20).collect::<String>()));
                    } else if partir {
                        // PARTIR EN DOS por la aguja: el gesto de siempre para
                        // un subtítulo que se ha comido dos frases
                        let t = self.visor.t;
                        let sb = pr.subs[is].clone();
                        if t > sb.t0 + 0.2 && t < sb.t1 - 0.2 {
                            let trozos = subtitulo::parte(&sb.texto,
                                (sb.texto.chars().count() / 2).max(4));
                            let (a, b) = (trozos.first().cloned().unwrap_or_default(),
                                          trozos.get(1..).map(|x| x.join(" ")).unwrap_or_default());
                            pr.subs[is] = subtitulo::Sub { t0: sb.t0, t1: t, texto: a };
                            pr.subs.insert(is + 1,
                                subtitulo::Sub { t0: t, t1: sb.t1, texto: b });
                            self.di("subtítulo partido por la aguja");
                        } else {
                            self.di("pon la aguja DENTRO del subtítulo para partirlo");
                        }
                    }
                    let _ = pr.guarda();
                    self.refresca_pie(pr);
                    self.visor.foley(sonido::Foley::Tick);
                    return;
                }
                return;
            }
        }
        // ── LA FICHA DE LA CAPA manda si hay una elegida (CAPAS §7) ──────
        if let Some(k) = self.sel_capa {
            let fx = ancho - Self::INSPECTOR_W + 10.0;
            let (y1, _y2) = Self::musica_botones_y();
            if mx > fx - 6.0 && k < pr.capas.len() {
                let cual = |y: f32| -> Option<usize> {
                    if my < y || my > y + 20.0 { return None; }
                    if mx >= fx + 4.0 && mx <= fx + 78.0 { Some(0) }
                    else if mx >= fx + 84.0 && mx <= fx + 158.0 { Some(1) }
                    else { None }
                };
                // fila 1: fundidos de alfa, ciclados como los del clip
                if let Some(b) = cual(y1) {
                    self.recuerda(pr);
                    let pasos = [0.0, 0.3, 0.6, 1.2];
                    let cp = &mut pr.capas[k];
                    let v = if b == 0 { &mut cp.fundido_in } else { &mut cp.fundido_out };
                    let j = pasos.iter().position(|p| (p - *v).abs() < 0.01).unwrap_or(0);
                    *v = pasos[(j + 1) % pasos.len()];
                    let nuevo = *v;
                    let _ = pr.guarda();
                    self.di(&format!("la capa {} {nuevo:.1} s",
                                     if b == 0 { "entra en" } else { "sale en" }));
                    return;
                }
                // fila 2: quitar
                if let Some(b) = cual(Self::musica_fila3_y()) {
                    if b == 1 {
                        self.recuerda(pr);
                        let q = pr.capas.remove(k);
                        self.sel_capa = None;
                        let _ = pr.guarda();
                        self.di(&format!("capa «{}» fuera", q.c.media));
                        return;
                    }
                    // b == 0: el encuadre a cero (útil tras un PiP torcido)
                    self.recuerda(pr);
                    let cf = pr.capas[k].c.cuartos_fichero;
                    pr.capas[k].c.enc = proyecto::Encuadre::limpio(cf);
                    let _ = pr.guarda();
                    self.di("el encuadre de la capa, a cero");
                    return;
                }
                return;
            }
        }
        // ── LA FICHA DE LA MÚSICA manda si hay una pista elegida ─────────
        if let Some(ia) = self.sel_audio {
            let fx = ancho - Self::INSPECTOR_W + 10.0;
            let (y1, y2) = Self::musica_botones_y();
            if mx > fx - 6.0 && ia < pr.audio.len() {
                let cual = |y: f32| -> Option<usize> {
                    if my < y || my > y + 20.0 { return None; }
                    if mx >= fx + 4.0 && mx <= fx + 78.0 { Some(0) }
                    else if mx >= fx + 84.0 && mx <= fx + 158.0 { Some(1) }
                    else { None }
                };
                if let Some(k) = cual(y1) {
                    self.recuerda(pr);
                    if k == 0 {
                        pr.audio[ia].mute = !pr.audio[ia].mute;
                        let m = pr.audio[ia].mute;
                        self.di(if m { "pista callada" } else { "pista sonando" });
                    } else {
                        let q = pr.audio.remove(ia);
                        self.sel_audio = None;
                        self.di(&format!("«{}» fuera de la bobina", q.media));
                    }
                    let _ = pr.guarda();
                    self.visor.busca(pr, self.visor.t);
                    return;
                }
                // los fundidos: el mismo ciclo que en la ficha del clip
                if let Some(k) = cual(Self::musica_fila3_y()) {
                    self.recuerda(pr);
                    let pasos = [0.0, 0.5, 1.0, 2.0];
                    let a = &mut pr.audio[ia];
                    let v = if k == 0 { &mut a.fade_in } else { &mut a.fade_out };
                    let j = pasos.iter().position(|p| (p - *v).abs() < 0.01).unwrap_or(0);
                    *v = pasos[(j + 1) % pasos.len()];
                    let nuevo = *v;
                    let _ = pr.guarda();
                    self.di(&format!("fundido de {} {nuevo:.1} s",
                                     if k == 0 { "entrada" } else { "salida" }));
                    return;
                }
                if let Some(k) = cual(y2) {
                    self.recuerda(pr);
                    if k == 0 { self.normaliza_musica(pr, ia); }
                    else {
                        let n = self.musica_vis as u8;
                        pr.audio[ia].pista = (pr.audio[ia].pista + 1) % n;
                        let _ = pr.guarda();
                        self.di(&format!("al carril {}", pr.audio[ia].pista + 1));
                    }
                    return;
                }
                // la fila del compás: sembrar las marcas ♩ o quitarlas
                if let Some(k) = cual(Self::musica_fila4_y()) {
                    if k == 0 { self.marcas_al_compas(pr); }
                    else { self.compas_fuera(pr); }
                    return;
                }
                // el mando del volumen de la pista
                let ybarra = Self::CABECERA + 8.0 + 22.0 + 26.0 + 16.0 + 16.0 + 20.0 + 15.0;
                if my >= ybarra - 5.0 && my <= ybarra + 12.0 {
                    self.abre_gesto(pr);
                    self.arrastrando = Arrastre::MusicaGain(ia);
                    return;
                }
            }
        }
        // ── la ficha del clip (panel derecho) ──
        if mx > ancho - Self::INSPECTOR_W && my > Self::CABECERA && my < banco {
            let ix = ancho - Self::INSPECTOR_W;
            let fx = ix + 12.0;
            let fy = Self::CABECERA + 36.0;
            let fw = Self::INSPECTOR_W - 24.0;
            let Some(i) = self.sel.or_else(|| pr.en(self.visor.t).map(|x| x.0)) else { return };
            if pr.clips.get(i).map(|c| c.hueco).unwrap_or(true) { return; }
            let y1 = fy + 92.0;
            let y2 = y1 + 34.0;
            let y3 = y2 + 24.0;
            let y5 = y3 + 26.0;
            let prop = pr.proporcion();
            let cw = 92.0f32;
            let chh = (cw / prop).clamp(34.0, 70.0);
            let (cx0, cy0) = (fx + 8.0, y5 + 16.0);
            let filas = self.filas_encuadre(cy0 + chh + 8.0);
            let y6 = filas.last().map(|(y, _)| y + 20.0).unwrap_or(cy0 + chh + 14.0);
            let y7 = y6 + 26.0;
            let y8 = y7 + 52.0;
            // LA VELOCIDAD (§4bis.3): el clic cicla las marchas del gramófono
            // —ahora con la MARCHA ATRÁS y el CONGELADO dentro—, el arrastre
            // la mueve fina y el doble clic la escribe a mano.
            if my >= y2 - 6.0 && my <= y2 + 14.0 {
                if self.mods.alt_key() {
                    self.recuerda(pr);
                    pr.clips[i].speed = 1.0;
                    let _ = pr.guarda();
                    self.visor.busca(pr, self.visor.t);
                    self.di("velocidad ×1");
                    return;
                }
                self.recuerda(pr);
                let pasos = [-2.0, -1.0, 0.0, 0.25, 0.5, 1.0, 1.5, 2.0, 4.0];
                let c = &mut pr.clips[i];
                let k = pasos.iter().position(|p| (p - c.speed).abs() < 0.01).unwrap_or(5);
                c.speed = pasos[(k + 1) % pasos.len()];
                let _ = pr.guarda();
                let v = pr.clips[i].speed;
                self.visor.busca(pr, self.visor.t);
                self.di(&match v {
                    v if v.abs() < 0.02 => "fotograma CONGELADO".to_string(),
                    v if v < 0.0 => format!("marcha atrás ×{:.2}", -v),
                    v => format!("velocidad ×{v:.2}"),
                });
                return;
            }
            // «volver a enlazar» el material ausente (§4)
            if pr.clips[i].ausente && my >= fy + 42.0 && my <= fy + 64.0
                && mx >= fx + 102.0 && mx <= fx + 214.0 {
                self.reenlaza(pr, i);
                return;
            }
            // ── los botones de ORIENTACIÓN, el encaje y el volteo ─────────
            {
                let (bx1, bx2) = (cx0 + cw + 10.0, cx0 + cw + 56.0);
                if my >= cy0 && my <= cy0 + 20.0 {
                    if mx >= bx1 && mx <= bx1 + 40.0 { self.gira_cuarto(pr, 3); return; }
                    if mx >= bx2 && mx <= bx2 + 40.0 { self.gira_cuarto(pr, 1); return; }
                }
                let ey = cy0 + 40.0;
                if my >= ey - 3.0 && my <= ey + 12.0 && mx >= bx1 {
                    let k = ((mx - bx1) / 30.0).floor();
                    if (0.0..3.0).contains(&k) {
                        self.recuerda(pr);
                        pr.clips[i].enc.encaje = [proyecto::Encaje::Dentro,
                            proyecto::Encaje::Llena, proyecto::Encaje::Estira][k as usize];
                        let _ = pr.guarda();
                        self.visor.marca_cuarto(i);
                        self.di(&format!("encaje: {}", pr.clips[i].enc.encaje.rotulo()));
                        return;
                    }
                }
                if my >= ey + 14.0 && my <= ey + 30.0 && mx >= bx1 {
                    let k = ((mx - bx1) / 24.0).floor();
                    if (0.0..2.0).contains(&k) {
                        self.recuerda(pr);
                        let v = &mut pr.clips[i].enc.voltea;
                        if k == 0.0 { v.0 = !v.0; } else { v.1 = !v.1; }
                        let _ = pr.guarda();
                        self.visor.marca_cuarto(i);
                        self.di("volteado");
                        return;
                    }
                }
            }
            // ── LOS NÚMEROS DEL ENCUADRE ────────────────────────────────
            for (fy2, campo) in &filas {
                if my >= *fy2 - 3.0 && my <= *fy2 + 12.0 && mx >= fx + 8.0 {
                    if self.mods.alt_key() {
                        // alt-clic: el campo vuelve a su valor limpio
                        self.recuerda(pr);
                        let limpio = proyecto::Encuadre::limpio(pr.clips[i].cuartos_fichero);
                        let v = Self::valor_campo(&limpio, *campo);
                        Self::pon_campo(&mut pr.clips[i].enc, *campo, v);
                        let _ = pr.guarda();
                        self.visor.marca_cuarto(i);
                        return;
                    }
                    let doble = self.ultima_lata.0 == usize::MAX - 2 - *campo as usize
                        && self.ultima_lata.1.elapsed().as_secs_f64() < 0.4;
                    self.ultima_lata = (usize::MAX - 2 - *campo as usize,
                                        std::time::Instant::now());
                    if doble {
                        let v = Self::valor_campo(&pr.clips[i].enc, *campo);
                        self.tecleando = Some((i, *campo, format!("{v:.1}")));
                        return;
                    }
                    self.abre_gesto(pr);
                    self.arrastrando = Arrastre::FilaEnc(i, *campo);
                    return;
                }
            }
            // la palanca del sonido del vídeo
            if my >= y3 - 6.0 && my <= y3 + 16.0 && mx > fx + 140.0 {
                self.recuerda(pr);
                pr.clips[i].mute = !pr.clips[i].mute;
                let _ = pr.guarda();
                self.visor.busca(pr, self.visor.t);
                let m = if pr.clips[i].mute { "clip silenciado" } else { "clip con su sonido" };
                self.di(m);
                return;
            }
            // el croquis del encuadre: arrastrar reencuadra; alt-clic resetea
            if mx >= cx0 - 6.0 && mx <= cx0 + 116.0 && my >= cy0 - 6.0 && my <= cy0 + chh + 6.0 {
                if self.mods.alt_key() {
                    self.recuerda(pr);
                    let c = &mut pr.clips[i];
                    c.enc = proyecto::Encuadre::limpio(c.cuartos_fichero);
                    let _ = pr.guarda();
                    self.visor.marca_cuarto(i);
                    self.di("encuadre al natural");
                } else {
                    // el recuadro rojo de la ficha ABRE el modo encuadre sobre
                    // la imagen: las dos manos, la misma verdad (§1.5 · A)
                    self.abre_encuadre(pr, i);
                }
                return;
            }
            // la washi
            if my >= y6 - 4.0 && my <= y6 + 14.0 && mx >= fx + 58.0 {
                let k = ((mx - fx - 62.0) / 40.0).floor();
                if (0.0..4.0).contains(&k) {
                    self.recuerda(pr);
                    let k = k as u8;
                    let c = &mut pr.clips[i];
                    c.washi = if c.washi == Some(k) { None } else { Some(k) };
                    let _ = pr.guarda();
                    self.visor.foley(sonido::Foley::Tick);
                    return;
                }
            }
            // «→ llévala al cuarto oscuro». La aguja se va CON la ficha: el
            // cuarto revela lo que hay bajo la aguja, así que llevarla allí
            // sin mover la aguja enseñaría otro plano.
            if my >= y7 + 20.0 && my <= y7 + 44.0 {
                self.sel = Some(i);
                let t = pr.inicios().get(i).copied().unwrap_or(0.0)
                    + pr.clips.get(i).map(|c| c.dur() * 0.5).unwrap_or(0.0);
                self.visor.busca(pr, t);
                self.va_a(Sala::CuartoOscuro);
                return;
            }
            // la segunda fila de acciones: nota
            if my >= y8 + 22.0 && my <= y8 + 50.0 && mx >= fx + 8.0 && mx <= fx + 78.0 {
                let texto = pr.clips.get(i).map(|c| c.nota.clone()).unwrap_or_default();
                self.notando = Some((i, texto));
                return;
            }
            // las acciones: contacto · duplicar · al cubo
            if my >= y8 - 4.0 && my <= y8 + 24.0 {
                let k = ((mx - fx - 12.0) / 70.0).floor();
                match k as i32 {
                    // EL TEXTO DEL RÓTULO: se vuelve a quemar la tarjeta con lo
                    // que se escriba y el clip pasa a apuntar a la nueva
                    0 if pr.clips[i].media.starts_with("titulo_") => {
                        let texto = pr.clips[i].nota.clone();
                        self.titulando = Some(texto);
                        self.retitulando = Some(i);
                        return;
                    }
                    0 => {
                        // la copia de contacto: el fotograma actual a un PNG
                        let c = &pr.clips[i];
                        let carpeta = pr.base.join("contactos");
                        let _ = std::fs::create_dir_all(&carpeta);
                        let t_src = pr.en(self.visor.t).map(|x| x.1).unwrap_or(c.t_in);
                        let salida = carpeta.join(format!("{}_{}.png",
                            c.media.replace('.', "_"), (t_src * 100.0) as u64));
                        let _ = std::process::Command::new("ffmpeg")
                            .arg("-ss").arg(format!("{t_src:.3}"))
                            .arg("-i").arg(&c.ruta)
                            .arg("-frames:v").arg("1").arg("-y").arg(&salida)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn();
                        self.di("copia de contacto a contactos/");
                    }
                    1 => {
                        self.recuerda(pr);
                        let c = pr.clips[i].clone();
                        pr.clips.insert(i + 1, c);
                        let _ = pr.guarda();
                        self.sel = Some(i + 1);
                        self.visor.foley(sonido::Foley::Lata);
                        self.di("clip duplicado (el tampón)");
                    }
                    2 => {
                        self.recuerda(pr);
                        let c = pr.clips.remove(i);
                        if !c.hueco { self.recortes.push(c); }
                        self.sel = None;
                        let _ = pr.guarda();
                        self.visor.busca(pr, self.visor.t.min(pr.duracion()));
                        self.visor.foley(sonido::Foley::Lata);
                        self.di("al cubo de recortes");
                    }
                    _ => {}
                }
                return;
            }
            return;
        }

        // ── el banco: aguja arriba, clips abajo ──
        if my > banco {
            // el margen izquierdo: la manivela y las palancas de las bandas
            if mx < Self::ESTANTE_W {
                let ty = self.tira_y();
                let my_y = self.pista_y(0);
                // LOS MANDOS DE NIVEL (§1.6): arrastrar mueve; alt-clic los
                // devuelve a 0 dB, que es el gesto de la casa
                if let Some(k) = self.nivel_en(mx, my) {
                    if self.mods.alt_key() {
                        if k == 0 { pr.vol_voz = 0.0; } else { pr.vol_musica = 0.0; }
                        self.visor.manda_mezcla(pr);
                        let _ = pr.guarda();
                        self.di("nivel a 0 dB");
                    } else {
                        self.arrastrando = Arrastre::Volumen(k);
                    }
                    return;
                }
                // la manivela, DESDE LA MISMA GEOMETRÍA que la dibuja
                let (mcx, mcy) = self.manivela_centro();
                if (mx - mcx).powi(2) + (my - mcy).powi(2) < 34.0f32.powi(2) {
                    self.arrastrando = Arrastre::Manivela;
                    return;
                }
                if mx > 198.0 && mx < 234.0 {
                    if my > ty + 88.0 - 34.0 && my < ty + 88.0 + 2.0 {
                        pr.mudo_voz = !pr.mudo_voz;
                        let _ = pr.guarda();
                        self.visor.manda_mezcla(pr);
                        let m = if pr.mudo_voz { "el sonido del vídeo, silenciado" }
                                else { "el sonido del vídeo, puesto" };
                        self.di(m);
                        return;
                    }
                    if my > my_y - 2.0 && my < my_y + Self::ALTO_PISTA * self.musica_vis as f32 {
                        pr.mudo_musica = !pr.mudo_musica;
                        let _ = pr.guarda();
                        self.visor.manda_mezcla(pr);
                        let m = if pr.mudo_musica { "la música, silenciada" }
                                else { "la música, puesta" };
                        self.di(m);
                        return;
                    }
                }
                return;
            }
            // ── EL CARRIL DEL PIE: elegir, mover, estirar ────────────────
            // Los tiradores viven DENTRO de su propio bloque (la lección de
            // los bordes compartidos de la música): así dos subtítulos
            // pegados no se roban el tirador.
            if self.hay_pie(pr) {
                if let Some(k) = self.sub_en_punto(pr, mx, my) {
                    self.sel_sub = Some(k);
                    self.sel = None; self.sel_audio = None; self.sel_capa = None;
                    let (x0, x1) = (self.x_de(pr.subs[k].t0), self.x_de(pr.subs[k].t1));
                    self.abre_gesto(pr);
                    self.arrastrando = if mx - x0 < 7.0 && x1 - x0 > 18.0 {
                        Arrastre::SubTrimI(k)
                    } else if x1 - mx < 7.0 && x1 - x0 > 18.0 {
                        Arrastre::SubTrimD(k)
                    } else {
                        Arrastre::SubMueve(k)
                    };
                    self.escribiendo_sub = None;
                    return;
                }
                // el carril, pero fuera de todo bloque: se deselecciona
                let sy = self.sub_y();
                if my >= sy - 2.0 && my <= sy + Self::ALTO_SUB && mx > Self::ESTANTE_W {
                    self.sel_sub = None;
                    self.escribiendo_sub = None;
                    return;
                }
            }
            // ── QUITAR LA CUCHILLA CON UN CLIC ────────────────────────────
            // Una vez puesta, la única salida era Esc o cortar: si no sabías
            // lo de Esc, estabas obligado a cortar. Ahora se quita clicándola
            // (y con ⌘Z, y con Esc).
            if let Some(tc) = self.marca_corte {
                if (mx - self.x_de(tc)).abs() <= 8.0 {
                    self.marca_corte = None;
                    self.visor.foley(sonido::Foley::Tick);
                    self.di("cuchilla quitada");
                    return;
                }
            }
            // la BARRA de desplazamiento de la bobina
            {
                let ty = self.tira_y();
                let by = ty + 88.0 + 14.0 + 44.0;
                if self.desplaza_max(pr) > 1.0 && my >= by - 5.0 && my <= by + 14.0
                    && mx > Self::ESTANTE_W {
                    self.arrastrando = Arrastre::Barra;
                    return;
                }
            }
            // ── EL CARRIL DE LA CAPA (CAPAS §7): elegir, mover, recortar ──
            if let Some(k) = self.capa_en_punto(pr, mx, my) {
                self.sel_capa = Some(k);
                self.sel = None;
                self.seleccion.clear();
                self.sel_audio = None;
                let cp = &pr.capas[k];
                let x0 = self.x_de(cp.start);
                let x1 = self.x_de(cp.fin());
                self.abre_gesto(pr);
                self.arrastrando = if mx >= x0 && mx <= x0 + 7.0 {
                    Arrastre::CapaTrimI(k)
                } else if mx >= x1 - 7.0 && mx < x1 {
                    Arrastre::CapaTrimD(k)
                } else {
                    Arrastre::CapaMueve(k)
                };
                self.visor.foley(sonido::Foley::Tick);
                return;
            }
            // la cinta de empalme (franja ALTA de la junta) cicla el fundido
            if my >= self.tira_y() - 10.0 && my < self.tira_y() + 12.0 {
                let mut acc2 = 0.0f64;
                for i in 0..pr.clips.len() {
                    let x0 = self.x_de(acc2);
                    if (mx - x0).abs() <= 9.0 {
                        self.recuerda(pr);
                        let pasos = [0.0, 0.5, 1.0, 2.0];
                        let f = pr.clips[i].fade;
                        let k = pasos.iter().position(|p| (p - f).abs() < 0.01).unwrap_or(0);
                        pr.clips[i].fade = pasos[(k + 1) % pasos.len()];
                        let msg = if pr.clips[i].fade > 0.0 {
                            format!("fundido {:.1} s", pr.clips[i].fade)
                        } else { "corte seco".to_string() };
                        self.di(&msg);
                        let _ = pr.guarda();
                        return;
                    }
                    acc2 += pr.clips[i].dur();
                }
            }
            // las BANDERAS del rango, en la regla
            if let Some((ra, rb)) = pr.rango {
                if my < self.tira_y() && my > self.tira_y() - 30.0 {
                    for (k, t) in [ra, rb].iter().enumerate() {
                        if (mx - self.x_de(*t)).abs() < 8.0 {
                            self.arrastrando = Arrastre::Rango(k as u8);
                            self.abre_gesto(pr);
                            return;
                        }
                    }
                }
            }
            if my < self.tira_y() {
                self.arrastrando = Arrastre::Aguja;
                let t = if let Some(f) = &self.fuente {
                    self.t_fuente_en(mx, f.cinta.dur)
                } else {
                    self.tiempo_en(mx)
                };
                self.visor.busca(pr, t);
                return;
            }
            // ¿la pista de MÚSICA? (los carriles, bajo la tira)
            if let Some(carril) = self.pista_en(my) {
                let my_top = self.pista_y(carril);
                for (i, a) in pr.audio.iter().enumerate() {
                    if a.pista != carril { continue; }
                    let ax0 = self.x_de(a.entra());
                    let aw = (a.dur() as f32 * self.pxs).max(6.0);
                    if mx >= ax0 - 6.0 && mx <= ax0 + aw + 6.0 {
                        // LOS PUNTOS DE VOLUMEN se arrastran SIN modificador:
                        // la banda existía pero era invisible y solo se tocaba
                        // con alt-clic, que es tanto como no tenerla (§2)
                        let ya = |db: f64| my_top + 24.0
                            * (1.0 - ((db + 24.0) / 30.0).clamp(0.0, 1.0)) as f32;
                        let xa = |t: f64| ax0
                            + (((t - a.t_in) / (a.t_out - a.t_in).max(1e-9)) as f32) * aw;
                        if let Some(k) = a.banda.iter().position(|(t, g)| {
                            (xa(*t) - mx).abs() < 7.0 && (ya(*g) - my).abs() < 8.0
                        }) {
                            self.sel_audio = Some(i);
                            self.abre_gesto(pr);
                            self.arrastrando = Arrastre::MusicaPunto(i, k);
                            return;
                        }
                        // y sobre la LÍNEA de nivel de una pista sin puntos,
                        // el arrastre mueve la ganancia entera
                        if a.banda.is_empty() && (ya(a.gain) - my).abs() < 5.0 {
                            self.sel_audio = Some(i);
                            self.abre_gesto(pr);
                            self.arrastrando = Arrastre::MusicaGain(i);
                            return;
                        }
                        // alt+clic: añadir o quitar un punto de la banda
                        if self.mods.alt_key() {
                            let t_f = pr.audio[i].t_in
                                + (((mx - ax0) / aw) as f64) * (pr.audio[i].t_out - pr.audio[i].t_in);
                            self.recuerda(pr);
                            let a = pr.audio.get_mut(i).unwrap();
                            if let Some(k) = a.banda.iter().position(|(t, _)| {
                                ((t - a.t_in) / (a.t_out - a.t_in).max(1e-9)
                                 * aw as f64 - (mx - ax0) as f64).abs() < 6.0
                            }) {
                                a.banda.remove(k);
                                self.di("punto quitado");
                            } else {
                                a.banda.push((t_f, 0.0));
                                a.banda.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
                                let k = a.banda.iter().position(|(t, _)| (*t - t_f).abs() < 1e-9).unwrap_or(0);
                                self.arrastrando = Arrastre::MusicaPunto(i, k);
                                self.di("punto de volumen (arrastra ↑↓)");
                            }
                            let _ = pr.guarda();
                            return;
                        }
                        // esquina superior: cicla el fundido (0/0.5/1/2 s)
                        if my <= my_top + 8.0 {
                            let pasos = [0.0, 0.5, 1.0, 2.0];
                            let borde_izq = (mx - ax0).abs() <= 14.0;
                            let borde_der = (mx - (ax0 + aw)).abs() <= 14.0;
                            if borde_izq || borde_der {
                                self.recuerda(pr);
                                let a = pr.audio.get_mut(i).unwrap();
                                let f = if borde_izq { &mut a.fade_in } else { &mut a.fade_out };
                                let k = pasos.iter().position(|p| (p - *f).abs() < 0.01).unwrap_or(0);
                                *f = pasos[(k + 1) % pasos.len()];
                                let val = *f;
                                let _ = pr.guarda();
                                self.di(&format!("fundido de música {:.1} s", val));
                                return;
                            }
                        }
                        // ⇧+clic: SUBIR O BAJAR DE CARRIL
                        if self.mods.shift_key() {
                            self.recuerda(pr);
                            let n = self.musica_vis as u8;
                            if let Some(a) = pr.audio.get_mut(i) { a.pista = (a.pista + 1) % n; }
                            let _ = pr.guarda();
                            self.sel_audio = Some(i);
                            self.di(&format!("al carril {}", pr.audio[i].pista + 1));
                            return;
                        }
                        // seleccionar: la ficha pasa a ser la de esta pista
                        self.sel_audio = Some(i);
                        self.sel = None;
                        self.sel_sub = None;
                        self.escribiendo_sub = None;
                        self.seleccion.clear();
                        // LAS DOS ZONAS DE TIRADOR NO SE PISAN. Antes eran
                        // ±7 px alrededor de cada borde, y con dos trozos
                        // pegados —que es justo lo que deja la cuchilla— el
                        // borde compartido caía dentro de las dos: cogías la
                        // cola de uno o la cabeza del otro según el orden en
                        // la lista, o sea al azar. Ahora cada zona vive DENTRO
                        // de su propio trozo y el reparto es exacto.
                        self.arrastrando = if mx >= ax0 && mx <= ax0 + 7.0 {
                            Arrastre::MusicaTrimI(i)
                        } else if mx >= ax0 + aw - 7.0 && mx < ax0 + aw {
                            Arrastre::MusicaTrimD(i)
                        } else {
                            Arrastre::MusicaMueve(i)
                        };
                        self.abre_gesto(pr);
                        return;
                    }
                }
            }
            let mut acc = 0.0f64;
            let alto_tira = 88.0;
            for (i, c) in pr.clips.iter().enumerate() {
                let x0 = self.x_de(acc);
                let x1 = self.x_de(acc + c.dur());
                // el clip solo atrapa el clic DENTRO de su tira; el resto del
                // banco es de la aguja (scrub)
                if mx >= x0 - 6.0 && mx <= x1 + 6.0
                    && my >= self.tira_y() - 4.0 && my <= self.tira_y() + alto_tira + 6.0 {
                    if self.mods.shift_key() {
                        if !self.seleccion.insert(i) { self.seleccion.remove(&i); }
                    } else if !self.seleccion.contains(&i) {
                        self.seleccion.clear();
                        self.seleccion.insert(i);
                        // la grapa: sus hermanos entran con él
                        if let Some(g) = c.grupo {
                            for (j, c2) in pr.clips.iter().enumerate() {
                                if c2.grupo == Some(g) { self.seleccion.insert(j); }
                            }
                        }
                    }
                    self.sel = Some(i);
                    self.sel_audio = None;
                    self.sel_sub = None;
                    self.escribiendo_sub = None;
                    self.arrastrando = if (mx - x0).abs() <= 7.0 { Arrastre::TrimI(i) }
                                       else if (mx - x1).abs() <= 7.0 { Arrastre::TrimD(i) }
                                       else { Arrastre::ClipMueve(i) };
                    // foto del estado: al soltar, si cambió, UN paso de undo
                    self.abre_gesto(pr);
                    self.visor.marca_cuarto(i);
                    return;
                }
                acc += c.dur();
            }
            // bajo la tira y sin clip: el LÁPIZ dibuja la caja de selección
            if my > self.tira_y() + 88.0 + 44.0 {
                self.sel = None;
                self.seleccion.clear();
                self.caja = Some((mx, my));
                self.arrastrando = Arrastre::Caja;
                return;
            }
            self.sel = None;
            self.seleccion.clear();
            self.arrastrando = Arrastre::Aguja;
            let t = self.tiempo_en(mx);
            self.visor.busca(pr, t);
            return;
        }

        // ── el vidrio ────────────────────────────────────────────────────
        let _ = alto;
        let [gx, gy, gw, gh] = self.visor.rect_pantalla;
        let en_vidrio = mx >= gx && mx <= gx + gw && my >= gy && my <= gy + gh;
        // EL MODO ENCUADRE (§1.5 · A): tiradores de esquina, de borde, el
        // ancla y el giro. Todo EN VIVO: se ve mientras se mueve, que era el
        // fallo de antes (había que soltar para ver el resultado).
        if let Some(i) = self.modo_encuadre {
            if en_vidrio && i < pr.clips.len() {
                // doble clic: al encuadre limpio
                let doble = self.ultima_lata.0 == usize::MAX - 1
                    && self.ultima_lata.1.elapsed().as_secs_f64() < 0.4;
                self.ultima_lata = (usize::MAX - 1, std::time::Instant::now());
                if doble {
                    self.recuerda(pr);
                    let c = &mut pr.clips[i];
                    c.enc = proyecto::Encuadre::limpio(c.cuartos_fichero);
                    let _ = pr.guarda();
                    self.visor.marca_cuarto(i);
                    self.di("encuadre limpio");
                    return;
                }
                let k = self.tirador_en(pr, i, mx, my);
                self.abre_gesto(pr);
                let enc0 = pr.clips[i].enc;
                match k {
                    Some(k) => {
                        let fijo = Self::punto_fijo(pr, i, k, self.mods.alt_key());
                        self.enc_gesto = Some((enc0, fijo, self.a_lienzo(mx, my)));
                        self.arrastrando = Arrastre::EncTirador(i, k);
                    }
                    None if self.dentro_del_cuadro(pr, i, mx, my) => {
                        self.arrastrando = Arrastre::Encuadre(i);
                    }
                    None => {
                        // fuera del cuadro y fuera de los tiradores: se sale
                        self.modo_encuadre = None;
                        self.di("encuadre cerrado");
                    }
                }
                return;
            }
        }
        if self.mods.alt_key() && en_vidrio {
            // con una CAPA elegida, el alt-arrastre coloca EL PiP (CAPAS §7);
            // si no, el encuadre del clip base, como siempre
            if let Some(k) = self.sel_capa {
                if k < pr.capas.len() {
                    self.abre_gesto(pr);
                    self.arrastrando = Arrastre::CapaEncuadre(k);
                    return;
                }
            }
            if let Some((i, _)) = pr.en(self.visor.t) {
                self.abre_gesto(pr);
                self.arrastrando = Arrastre::Encuadre(i);
            }
            return;
        }
        // DOBLE CLIC EN EL VISOR: LA IMAGEN a pantalla completa — no la
        // ventana. Antes esto agrandaba la app entera y lo que crecía era la
        // mesa, con la preview del mismo tamaño en medio. Ahora desaparece
        // todo menos el fotograma y el teclado sigue mandando.
        if en_vidrio {
            let doble = self.ultima_lata.0 == usize::MAX
                && self.ultima_lata.1.elapsed().as_secs_f64() < 0.4;
            self.ultima_lata = (usize::MAX, std::time::Instant::now());
            if doble {
                self.alterna_visor_lleno();
                return;
            }
        }
        self.visor.play_pausa(pr);
    }

    fn frame(&mut self, pr: &Proyecto) {
        // LA MESA CRECE CON LOS CARRILES: el banco gana el alto de las
        // pistas de capa visibles (suben desde la tira) y el de los
        // carriles de música más allá de los tres de siempre — así ocho
        // pistas no se comen el visor: se lo piden al banco.
        self.extra_capas =
            self.pistas_capa_visibles(pr).saturating_sub(1) as f32 * Self::ALTO_CAPA;
        self.musica_vis = Self::musica_visibles(pr);
        self.extra_sub = if self.hay_pie(pr) { Self::ALTO_SUB + 6.0 } else { 0.0 };
        self.banco_h = 250.0 + self.extra_capas + self.extra_sub
            + self.musica_vis.saturating_sub(3) as f32 * Self::ALTO_PISTA;
        self.dib_frames += 1;
        let v = self.dib_desde.elapsed().as_secs_f64();
        if v >= 2.0 {
            if std::env::var("FL_CRONO").is_ok() {
                eprintln!("  redraw: {:.1} Hz", self.dib_frames as f64 / v);
            }
            self.dib_frames = 0;
            self.dib_desde = std::time::Instant::now();
        }
        // la LANZADERA: J/K/L mueve la película por TIEMPO REAL (así la
        // marcha es la misma vaya el redraw a 50 Hz o a 8)
        if self.lanzadera != 0 {
            let dt = self.lanzadera_reloj.elapsed().as_secs_f64().min(0.25);
            self.lanzadera_reloj = std::time::Instant::now();
            let t = (self.visor.t + self.lanzadera as f64 * dt)
                .clamp(0.0, pr.duracion().max(0.0));
            self.visor.busca(pr, t);
            if t <= 0.0 || t >= pr.duracion() { self.lanzadera = 0; }
            self.ventana.request_redraw();
        }
        // el AMBIENTE de la sala (NORTE §1.6): se rellena solo si hace falta
        {
            let amb = match self.sala {
                Sala::Mesa => sonido::Ambiente::Mesa,
                Sala::CuartoOscuro => sonido::Ambiente::Cuarto,
                Sala::Revelado => sonido::Ambiente::Revelado,
                Sala::Portada => sonido::Ambiente::Ninguno,
            };
            let reloj = self.dib_desde.elapsed().as_secs_f32()
                + (self.dib_frames as f32) * 0.001;
            self.visor.ambiente(amb, reloj);
        }
        self.visor.avanza(pr);
        // EL BUCLE sobre el tramo marcado (§4bis.2): ajustar el color de un
        // plano mirándolo una y otra vez es media sesión de cuarto oscuro
        if self.bucle && self.visor.tocando {
            if let Some((ra, rb)) = pr.rango {
                if self.visor.t >= rb - 0.01 || self.visor.t < ra - 0.01 {
                    self.visor.busca(pr, ra);
                }
            }
        }
        // ¿terminó el revelado?
        if let Some(hijo) = self.revelando.as_mut() {
            if let Ok(Some(st)) = hijo.try_wait() {
                if st.success() {
                    let out = self.destino.clone().unwrap_or_else(|| pr.base.join("out"));
                    self.ultima_revelada = std::fs::read_dir(&out).ok().and_then(|rd| {
                        rd.flatten()
                            .filter(|e| e.path().extension().map(|x| x == "mp4" || x == "mov").unwrap_or(false))
                            .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
                            .map(|e| e.path())
                    });
                    self.di("bobina REVELADA — clic en su nombre para verla");
                    self.sello_en = Some(std::time::Instant::now());
                    self.visor.foley(sonido::Foley::Lata);
                    // UN AVISO DEL SISTEMA: si tarda un minuto uno se va a otra
                    // cosa, y el póster de la cuerda solo lo ve quien mira (§5)
                    let n = self.ultima_revelada.as_ref()
                        .and_then(|r| r.file_name())
                        .map(|x| x.to_string_lossy().to_string())
                        .unwrap_or_else(|| "la bobina".into());
                    Self::avisa_al_sistema("Bobina revelada", &n);
                } else {
                    // POR QUÉ falló, no «mira el log» (§4): el CLI escupe el
                    // motivo por stderr y el hilo del progreso lo guarda
                    let (_, motivo) = self.progreso.lock().map(|p| p.clone())
                        .unwrap_or((0.0, String::new()));
                    if motivo.starts_with("FALLÓ") {
                        self.di(&format!("el revelado {motivo}"));
                    } else {
                        self.di("el revelado FALLÓ (mira el diario del shell)");
                    }
                    Self::avisa_al_sistema("El revelado falló", &motivo);
                }
                self.revelando = None;
                // ¿hay latas esperando en la cola? la siguiente, a la cubeta
                if let Some(p) = self.cola_revelado.first().copied() {
                    self.cola_revelado.remove(0);
                    self.revela(pr, p, None);
                }
            }
        }
        if self.revelando.is_some() {
            // el líquido y la tira en marcha piden pantalla
            self.ventana.request_redraw();
        }
        // el archivador: cada 5 min, una copia de la bobina (quedan las 10 últimas)
        if self.ultima_copia.elapsed().as_secs() > 300 {
            self.ultima_copia = std::time::Instant::now();
            let actual = std::fs::read_to_string(pr.base.join("current.txt")).unwrap_or_default();
            let actual = actual.trim();
            let origen = if actual.is_empty() { pr.base.join("project.json") }
                         else { pr.base.join("projects").join(format!("{actual}.json")) };
            if origen.is_file() {
                let carpeta = pr.base.join("backups");
                let _ = std::fs::create_dir_all(&carpeta);
                let sello = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                let nombre = format!("{}-{}.json", pr.nombre.replace(' ', "_"), sello);
                let _ = std::fs::copy(&origen, carpeta.join(nombre));
                let mut copias: Vec<std::path::PathBuf> = std::fs::read_dir(&carpeta).ok()
                    .map(|rd| rd.flatten().map(|e| e.path())
                         .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
                         .collect()).unwrap_or_default();
                copias.sort();
                while copias.len() > 10 {
                    let _ = std::fs::remove_file(copias.remove(0));
                }
            }
        }
        self.minis.tic();
        self.minis.recibe(&self.gpu, &mut self.atlas);
        self.ondas.recibe();
        let (ancho, alto) = self.gpu.alto_ancho();
        let banco = self.banco_y();
        let mut d = ui::Dibujo::nuevo();
        let mut dt = ui::DibujoTex::nuevo();
        let mut d2 = ui::Dibujo::nuevo();
        let mut dt2 = ui::DibujoTex::nuevo();
        self.papel.limpia();
        self.tape.limpia();
        self.objetos.limpia();
        for f in &mut self.pared { f.limpia(); }
        // el papel procedural del taller, SIEMPRE debajo de todo (semilla =
        // el nombre de la bobina: cada proyecto tiene SU papel)
        self.fondo.siembra(&pr.nombre);
        self.fondo.modo = papel::MODO_MESA;

        let es_mesa = self.sala == Sala::Mesa;
        self.fondo.modo = match self.sala {
            Sala::CuartoOscuro => papel::MODO_TIZA,
            Sala::Revelado => papel::MODO_REVELADO,
            _ => papel::MODO_MESA,
        };
        if self.sala == Sala::Portada {
            self.dibuja_portada(pr, &mut d, &mut dt2, &mut d2, ancho, alto);
        } else if self.sala == Sala::CuartoOscuro {
            self.dibuja_cuarto(pr, &mut d, &mut dt, &mut d2, ancho, alto);
        } else if self.sala == Sala::Revelado {
            self.dibuja_revelado(pr, &mut d, &mut dt2, &mut d2, ancho, alto);
        } else {

        self.dibuja_cabecera(pr, &mut d, ancho, 0, false);

        // ── la estantería: latas de verdad sobre baldas (NORTE §3.1) ──
        trazo::linea(&mut d, Self::ESTANTE_W - 1.0, Self::CABECERA + 8.0,
                     Self::ESTANTE_W - 1.0, alto - 10.0, 1.6, paleta::TINTA_TENUE, 42);
        d.texto_f(ui::Familia::Grot, 14.0, Self::CABECERA + 8.0, "ESTANTERÍA DE MATERIAL",
                  10.0, paleta::TINTA);
        trazo::subraya(&mut d, 14.0, 176.0, Self::CABECERA + 24.0, 1.2, paleta::TINTA_TENUE, 5);
        d.texto(182.0, Self::CABECERA + 8.0, "+ (I)", 10.0, paleta::NARANJA);
        let (rx, ry) = self.raton;
        let hov = self.lata_en(rx, ry);
        let pie_y = banco - 158.0;   // el pie de la estantería: la foto y el susurro
        // el cubo de RECORTES, encima del pie. Su geometría es la de
        // `cubo_caja()`: la estantería de arriba se para donde empiece él.
        let (cubo_y, _) = self.cubo_caja();
        if !self.estanteria.is_empty() {
            // flecha roja a mano hacia la primera lata
            trazo::flecha(&mut d, 196.0, Self::CABECERA + 34.0, 116.0, Self::CABECERA + 60.0,
                          1.8, paleta::ROJO, 99);
        }
        // las pestañitas de filtro (todo · vídeo · audio · fotos)
        for (k, rot) in ["todo", "vídeo", "audio", "fotos"].iter().enumerate() {
            let px2 = 12.0 + k as f32 * 54.0;
            let activo = self.filtro == k as u8;
            if activo {
                d.rect_rot(px2, Self::CABECERA + 30.0, 50.0, 17.0, -0.01,
                           [0.851, 0.2, 0.145, 0.14]);
            }
            trazo::caja(&mut d, px2, Self::CABECERA + 30.0, 50.0, 17.0, 1.0,
                        if activo { paleta::ROJO } else { paleta::TINTA_TENUE },
                        880 + k as u32);
            d.texto(px2 + 7.0, Self::CABECERA + 34.0, rot, 8.0,
                    if activo { paleta::ROJO } else { paleta::TINTA });
        }
        let baldas_dib = self.proyecto_baldas.clone();
        let plan = self.estantes(&baldas_dib);
        if std::env::var("FL_PLAN").is_ok() && self.dib_frames == 1 {
            for (cx, cy, it) in &plan {
                eprintln!("plan: {cx},{cy} {it:?}");
            }
            eprintln!("cubo_y={cubo_y}");
        }
        let mut ultima_fila_y = Self::CABECERA + 64.0;
        for (cx, cy, item) in &plan {
            let (cx, cy) = (*cx, *cy);
            let hueco_necesario = if item.is_err() { 26.0 } else { 54.0 };
            if cy - 50.0 < Self::CABECERA + 52.0 - 0.1 && self.estante_scroll > 0.0
                && cy + hueco_necesario < Self::CABECERA + 120.0 {
                // asoma por arriba con el scroll: no pintarla sobre las pestañas
                continue;
            }
            if cy + hueco_necesario > cubo_y { break; }
            match item {
                Err(nombre) => {
                    // la cabecera de la balda: etiqueta manuscrita + cablecito
                    let plegada = self.baldas_cerradas.contains(nombre);
                    d.texto(14.0, cy + 4.0, if plegada { "▸" } else { "▾" }, 9.0, paleta::ROJO);
                    let n: String = nombre.chars().take(16).collect();
                    d2.rect_rot(26.0, cy, 118.0, 20.0, -0.008, [1.0, 1.0, 1.0, 0.92]);
                    d2.texto_f(ui::Familia::Mano, 32.0, cy - 2.0, &n, 15.0, paleta::TINTA);
                    if let Some(carp) = baldas_dib.iter()
                        .find(|(b, _)| b == nombre).and_then(|(_, c)| c.clone()) {
                        // el cablecito a la carpeta de origen + «volver a mirar»
                        trazo::linea(&mut d, 144.0, cy + 10.0, 168.0, cy + 6.0, 1.1,
                                     paleta::TINTA_TENUE, 890);
                        let carp: String = carp.file_name().map(|x| x.to_string_lossy().to_string())
                            .unwrap_or_default().chars().take(9).collect();
                        d.texto(170.0, cy + 2.0, &format!("⌂{carp}"), 7.0, paleta::TINTA_TENUE);
                        d.texto(170.0, cy + 12.0, "clic-dcho: mirar", 6.0, paleta::TINTA_TENUE);
                    }
                    continue;
                }
                Ok(i) => {
                    let i = *i;
                    let Some(c) = self.estanteria.get(i) else { continue };
                    ultima_fila_y = ultima_fila_y.max(cy + 52.0);
            let hover = hov == Some(i);
            let cy = if hover { cy - 2.0 } else { cy };
            let r = 42.0;
            let ang = ((i * 37 % 7) as f32 - 3.0) * 0.012;
            // la miniatura ASOMA por el agujero de la tapa (detrás de la lata)
            if c.fps >= 0.0 {
                let proxy = pr.base.join(".proxies").join(&c.nombre);
                let ruta = if proxy.is_file() { proxy } else { c.ruta.clone() };
                let clave = (format!("lata:{}", c.nombre), ((c.dur * 0.5) * 100.0) as u32);
                if let Some(slot) = self.minis.pide(clave, &ruta, c.dur * 0.5) {
                    dt.quad(cx - 36.0, cy - 21.0, 72.0, 42.0, slot, 1.0);
                } else {
                    d.rect(cx - 22.0, cy - 21.0, 44.0, 42.0, paleta::PELICULA);
                }
            } else {
                // lata de audio: onda dibujada en el agujero
                d.rect(cx - 21.0, cy - 21.0, 42.0, 42.0, paleta::PELICULA);
                for k in 0..7 {
                    let h2 = 6.0 + 10.0 * (1.0 - ((k as f32) - 3.0).abs() / 3.0);
                    d.rect(cx - 15.0 + k as f32 * 5.0, cy - h2 / 2.0, 3.0, h2, paleta::AMBAR);
                }
            }
            self.objetos.quad_uv_rot(cx - r, cy - r, r * 2.0, r * 2.0,
                                     doodles::uv(doodles::LATA), ang);
            if hover {
                trazo::circulo(&mut d2, cx, cy, r + 5.0, r + 3.0, 1.8, paleta::ROJO,
                               i as u32 * 7 + 3);
            }
            // la etiqueta troquelada, pegada torcida sobre la lata
            d2.rect_rot(cx - 45.0, cy + 14.0, 90.0, 21.0, ang * 2.0, [1.0, 1.0, 1.0, 0.94]);
            let n: String = c.nombre.chars().take(11).collect();
            d2.texto_f(ui::Familia::Mano, cx - 41.0, cy + 12.0, &n, 16.0, paleta::TINTA);
            d2.texto(cx - 12.0, cy + 38.0, &format!("{:.0} s", c.dur), 9.0, paleta::TINTA_TENUE);
                }
            }
        }
        // las baldas: una línea a pulso bajo cada fila completa de latas
        {
            let mut ys: Vec<i32> = plan.iter()
                .filter_map(|(_, cy, it)| it.as_ref().ok().map(|_| (*cy + 52.0) as i32))
                .collect();
            ys.dedup();
            for (f, y) in ys.iter().enumerate() {
                let y = *y as f32;
                if y > cubo_y { break; }
                trazo::linea(&mut d, 10.0, y, Self::ESTANTE_W - 14.0, y, 2.2, paleta::TINTA,
                             f as u32 * 13 + 7);
            }
        }
        // ── LA PAPELERA ──────────────────────────────────────────────
        // Debajo del cubo, y a propósito con la misma pinta de cacharro del
        // taller: un cubo de metal con la boca abierta. Se abre cuando llevas
        // algo encima, igual que el de recortes, para que se vea que va a
        // caer ahí. Acepta clips de la bobina, recortes del cubo y cintas de
        // la estantería.
        {
            let (px, py, pw, ph) = self.papelera_caja();
            let (rmx0, rmy0) = self.raton;
            let arrastrando_algo = matches!(self.arrastrando, Arrastre::ClipMueve(_))
                || self.cubo_pinza.is_some() || self.lata_pinza.is_some();
            let apunta_p = arrastrando_algo && self.en_la_papelera(rmx0, rmy0);
            let tinta = if apunta_p { paleta::ROJO } else { paleta::TINTA_TENUE };
            if apunta_p {
                d.rect(px - 4.0, py - 4.0, pw + 8.0, ph + 8.0, [0.851, 0.2, 0.145, 0.14]);
            }
            // el cacharro: un trapecio con tapa y dos asas
            let boca_p = py + 14.0;
            trazo::pulso(&mut d, &[(px + 26.0, boca_p), (px + 34.0, py + ph),
                                   (px + pw - 34.0, py + ph), (px + pw - 26.0, boca_p)],
                         1.6, tinta, 845);
            trazo::linea(&mut d, px + 18.0, boca_p, px + pw - 18.0, boca_p, 1.8, tinta, 846);
            trazo::linea(&mut d, px + pw / 2.0 - 12.0, boca_p - 7.0,
                         px + pw / 2.0 + 12.0, boca_p - 7.0, 1.4, tinta, 847);
            // tres rayas: que se lea «papelera» de un vistazo
            for k in 0..3 {
                let rx = px + 44.0 + k as f32 * 18.0;
                trazo::linea(&mut d, rx, boca_p + 10.0, rx - 2.0, py + ph - 8.0, 1.1, tinta,
                             848 + k as u32);
            }
            d.texto_f(ui::Familia::Grot, 14.0, py - 12.0, "LA PAPELERA", 10.0, tinta);
            d.texto_f(ui::Familia::Mano, 108.0, py - 14.0,
                      if apunta_p { "¡suéltalo!" } else { "(esto sí se va)" }, 12.0, tinta);
        }
        // el CUBO DE RECORTES (por si acaso): lo quitado, con sus fotogramas
        d.texto_f(ui::Familia::Grot, 14.0, cubo_y, "RECORTES", 10.0, paleta::TINTA);
        d.texto_f(ui::Familia::Mano, 92.0, cubo_y - 2.0, "(por si acaso)", 13.0, paleta::ROJO);
        // el cubo: un trapecio a pulso
        let (_, cubo_h) = self.cubo_caja();
        let boca = cubo_y + 22.0;
        trazo::pulso(&mut d, &[(20.0, boca), (16.0, cubo_y + cubo_h),
                               (214.0, cubo_y + cubo_h), (210.0, boca)],
                     1.6, paleta::TINTA, 840);
        trazo::linea(&mut d, 14.0, boca, 216.0, boca, 1.8, paleta::TINTA, 841);
        // ¿está el ratón encima con un clip en la mano? el cubo se abre
        let (rmx, rmy) = self.raton;
        let apunta = matches!(self.arrastrando, Arrastre::ClipMueve(_))
            && self.en_el_cubo(rmx, rmy);
        if apunta {
            d.rect(Self::CUBO_X - 2.0, boca, Self::CUBO_W + 4.0, cubo_h - 22.0,
                   [0.949, 0.78, 0.267, 0.20]);
            d.texto_f(ui::Familia::Mano, 40.0, cubo_y + cubo_h * 0.5, "¡suelta aquí!",
                      16.0, paleta::ROJO);
        }
        if self.recortes.is_empty() && !apunta {
            d.texto(46.0, cubo_y + 56.0, "(vacío — arrastra aquí lo que", 8.0, paleta::TINTA_TENUE);
            d.texto(46.0, cubo_y + 68.0, "quieras guardar para luego)", 8.0, paleta::TINTA_TENUE);
        }
        // EL CUBO NO TIENE FONDO: se recorre con la rueda, y lo último que
        // apartaste está arriba (que es lo que uno busca).
        let filas_v = self.cubo_filas();
        let desde = (self.cubo_scroll / Self::CUBO_FIL).floor().max(0.0) as usize;
        let n_rec = self.recortes.len();
        for fila in desde..(desde + filas_v + 1).min((n_rec + Self::CUBO_COLS - 1) / Self::CUBO_COLS) {
            for col in 0..Self::CUBO_COLS {
                let k = fila * Self::CUBO_COLS + col;
                let Some(idx) = n_rec.checked_sub(1 + k) else { continue };
                let Some(c) = self.recortes.get(idx) else { continue };
                let sx = Self::CUBO_X + 6.0 + col as f32 * Self::CUBO_COL;
                let sy = boca + 6.0 + fila as f32 * Self::CUBO_FIL - self.cubo_scroll;
                // recortado por la boca del cubo: lo que asoma por arriba, no
                if sy < boca - 2.0 || sy + 30.0 > cubo_y + cubo_h { continue; }
                let ang = ((k * 23 % 5) as f32 - 2.0) * 0.03;
                let cogido = self.cubo_pinza.map(|(i, _, _)| i == idx).unwrap_or(false);
                d.rect_rot(sx - 2.0, sy - 2.0, 58.0, 36.0, ang,
                           if cogido { paleta::ROJO } else { paleta::PELICULA });
                let proxy = pr.base.join(".proxies").join(&c.media);
                let ruta = if proxy.is_file() { proxy } else { c.ruta.clone() };
                let tmed = (c.t_in + c.t_out) * 0.5;
                let clave = (format!("recorte:{}:{:.1}", c.media, c.t_in), (tmed * 100.0) as u32);
                if let Some(slot) = self.minis.pide(clave, &ruta, tmed) {
                    dt.quad(sx, sy, 54.0, 30.0, slot, if cogido { 0.45 } else { 1.0 });
                }
                // DE DÓNDE SALIÓ: con muchos recortes, la duración sola no
                // dice nada — lo que uno busca es «el final de tal plano» (§5)
                d.texto(sx + 1.0, sy + 30.0, &format!("{:.1}s", c.t_out - c.t_in),
                        7.0, paleta::AMBAR);
                let n: String = c.media.chars().take(9).collect();
                d.texto(sx + 22.0, sy + 30.0, &n, 6.0, paleta::TINTA_TENUE);
            }
        }
        // el contador y la barrita de recorrido: el cubo dice cuánto guarda
        if n_rec > 0 {
            // el contador, a la derecha del rótulo (encima lo pisaba)
            d.texto(Self::CUBO_X + Self::CUBO_W - 34.0, cubo_y + 8.0,
                    &format!("{n_rec}"), 10.0, paleta::ROJO);
            let max = self.cubo_scroll_max();
            if max > 0.0 {
                let alto_v = cubo_h - 28.0;
                let frac = (filas_v as f32 * Self::CUBO_FIL) / (max + filas_v as f32 * Self::CUBO_FIL);
                let bh = (alto_v * frac).clamp(14.0, alto_v);
                let by = boca + 4.0 + (alto_v - bh) * (self.cubo_scroll / max).clamp(0.0, 1.0);
                d.rect(Self::CUBO_X + Self::CUBO_W - 5.0, by, 3.0, bh, paleta::TINTA_TENUE);
            }
        }

        // el pie: la foto del laboratorista pegada con celo + el susurro
        self.objetos.quad_uv_rot(14.0, pie_y, 126.0, 102.0, doodles::uv(doodles::FOTO_LAB), -0.035);
        self.objetos.quad_uv_rot(48.0, pie_y - 11.0, 56.0, 25.0, doodles::uv(doodles::CELO), 0.05);
        // el susurro del margen ROTA y responde al uso real (NORTE §7.12)
        let mut susurros: Vec<(&str, &str)> = vec![("las latas se abren", "con dos toques")];
        if pr.marcas.is_empty() { susurros.push(("M clava una chincheta", "en la aguja")); }
        if pr.audio.is_empty() { susurros.push(("doble toque en la lata", "de audio: música")); }
        if !self.recortes.is_empty() { susurros.push(("del cubo a la bobina:", "arrastra y suelta")); }
        else { susurros.push(("arrastra un clip al cubo", "y vuelve a por él luego")); }
        if pr.clips.iter().all(|c| c.fade < 0.01) {
            susurros.push(("toca la cinta de empalme:", "el corte se funde"));
        }
        susurros.push(("la rueda sobre la manivela", "mueve la película"));
        let cual = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs()).unwrap_or(0) / 25) as usize % susurros.len();
        let (l1, l2) = susurros[cual];
        d2.texto_f(ui::Familia::Mano, 20.0, pie_y + 104.0, l1, 15.0, paleta::TINTA);
        d2.texto_f(ui::Familia::Mano, 32.0, pie_y + 121.0, l2, 15.0, paleta::TINTA);
        // el código de barras de la bobina (la decoración informa)
        {
            let mut h = 5381u32;
            for b in pr.nombre.bytes() { h = h.wrapping_mul(33) ^ b as u32; }
            let mut bx = 150.0f32;
            let mut k = 0u32;
            while bx < Self::ESTANTE_W - 16.0 {
                let w = 1.0 + ((h >> (k % 27)) & 2) as f32;
                d.rect(bx, pie_y + 6.0, w, 26.0, paleta::PELICULA);
                bx += w + 1.0 + ((h >> ((k + 11) % 23)) & 1) as f32;
                k += 3;
            }
            d.texto(172.0, pie_y + 34.0, "BOBINA", 8.0, paleta::TINTA_TENUE);
        }

        // ── LA FICHA DEL CLIP (NORTE §3.3): el panel derecho ──
        //
        // NO SE DIBUJA SI ESTÁS MIRANDO UNA MÚSICA. Taparla con un rectángulo
        // opaco no servía: la ficha del clip lleva miniatura, y las texturas
        // van en el atlas, que se pinta SIEMPRE por encima de la capa de
        // rectángulos. Se veían las dos a la vez y no se leía ninguna. Un
        // panel, una ficha: la que hayas elegido.
        // la ficha del clip NO se dibuja si manda otra: las texturas del
        // atlas van siempre por encima de los rectángulos y se transparenta
        // (la lección de la ficha de la música, §1.8)
        let solo_musica = self.sel_audio.is_some() || self.sel_capa.is_some()
            || self.sel_sub.is_some();
        let ix = ancho - Self::INSPECTOR_W;
        if !solo_musica {
        trazo::linea(&mut d, ix + 1.0, Self::CABECERA + 8.0, ix + 1.0, banco - 10.0,
                     1.6, paleta::TINTA_TENUE, 43);
        d.texto_f(ui::Familia::Grot, ix + 14.0, Self::CABECERA + 8.0, "LA FICHA DEL CLIP", 11.0, paleta::TINTA);
        trazo::subraya(&mut d, ix + 14.0, ix + 140.0, Self::CABECERA + 24.0, 1.2, paleta::TINTA_TENUE, 6);
        let idx = self.sel.or_else(|| pr.en(self.visor.t).map(|x| x.0));
        let fx = ix + 12.0;
        let fy = Self::CABECERA + 36.0;
        let fw = Self::INSPECTOR_W - 24.0;
        let fh = banco - fy - 18.0;
        // la ficha de catálogo, con su sombra y su clip de papel
        d.rect(fx + 3.0, fy + 4.0, fw, fh, [0.0, 0.0, 0.0, 0.10]);
        d.rect_rot(fx, fy, fw, fh, 0.004, [1.0, 1.0, 1.0, 0.92]);
        trazo::circulo(&mut d, fx + 18.0, fy + 2.0, 9.0, 13.0, 1.6, [0.45, 0.45, 0.48, 1.0], 60);
        trazo::circulo(&mut d, fx + 18.0, fy + 4.0, 5.0, 9.0, 1.3, [0.45, 0.45, 0.48, 1.0], 61);
        match idx.and_then(|i| pr.clips.get(i).map(|c| (i, c.clone()))) {
            Some((i, c)) if !c.hueco => {
                let _ = i;
                // la esquina doblada = el fundido de cabeza
                if c.fade > 0.01 {
                    let f = (c.fade as f32 * 10.0).clamp(8.0, 24.0);
                    d.tri([fx + fw - f, fy], [fx + fw, fy + f], [fx + fw - f, fy + f],
                          [0.85, 0.83, 0.78, 1.0]);
                    d.texto(fx + fw - 46.0, fy + 4.0, &format!("{:.1}s", c.fade), 7.0,
                            paleta::TINTA_TENUE);
                }
                // el fotograma + el sello del material
                let proxy = pr.base.join(".proxies").join(&c.media);
                let ruta = if proxy.is_file() { proxy } else { c.ruta.clone() };
                let tmed = (c.t_in + c.t_out) * 0.5;
                let clave = (format!("ficha:{}", c.media), (tmed * 10.0) as u32);
                if let Some(slot) = self.minis.pide(clave, &ruta, tmed) {
                    dt.quad(fx + 12.0, fy + 14.0, 82.0, 46.0, slot, 1.0);
                } else {
                    d.rect(fx + 12.0, fy + 14.0, 82.0, 46.0, paleta::PELICULA);
                }
                if let Ok((w2, h2, fps2, _)) = filmlook_core::indice::sondea(&c.ruta) {
                    d.texto(fx + 102.0, fy + 16.0, &format!("{w2}×{h2}"), 8.0, paleta::TINTA);
                    d.texto(fx + 102.0, fy + 28.0, &format!("{fps2:.2} fps"), 8.0, paleta::TINTA);
                }
                // NO HAY PISTA DE QUÉ CLIP ES CUÁL: todos los del mismo
                // fichero se llamaban igual. Ahora se numeran (§5).
                let hermanos: Vec<usize> = pr.clips.iter().enumerate()
                    .filter(|(_, x)| x.media == c.media).map(|(k, _)| k).collect();
                let cual = hermanos.iter().position(|k| *k == i).unwrap_or(0) + 1;
                let n: String = c.media.chars().take(20).collect();
                d.texto_f(ui::Familia::Mano, fx + 12.0, fy + 62.0, &n, 17.0, paleta::ROJO);
                if hermanos.len() > 1 {
                    d.texto(fx + 12.0, fy + 80.0,
                            &format!("{cual} de {}", hermanos.len()), 8.0, paleta::TINTA_TENUE);
                }
                // MATERIAL AUSENTE, A LA VISTA (§4): el clip se conserva con su
                // corte y su receta, pero hay que poder volver a enlazarlo
                if c.ausente {
                    d.rect(fx + 12.0, fy + 14.0, 82.0, 46.0, [0.851, 0.2, 0.145, 0.30]);
                    d.texto_f(ui::Familia::Grot, fx + 18.0, fy + 30.0, "FALTA", 13.0,
                              paleta::ROJO);
                    trazo::caja(&mut d, fx + 104.0, fy + 44.0, 108.0, 18.0, 1.3,
                                paleta::ROJO, 96);
                    d.texto(fx + 110.0, fy + 48.0, "volver a enlazar", 8.0, paleta::ROJO);
                }
                // los TC estampados
                let tc_de = |t: f64| {
                    let f2 = pr.fps.max(1.0);
                    format!("{:02}:{:02}:{:02}", (t as u32) / 60, (t as u32) % 60,
                            ((t % 1.0) * f2) as u32)
                };
                let y1 = fy + 92.0;
                d.rect(fx + 12.0, y1, 64.0, 16.0, paleta::PELICULA);
                d.texto(fx + 16.0, y1 + 3.0, &tc_de(c.t_in), 9.0, paleta::HUESO);
                d.rect(fx + 84.0, y1, 64.0, 16.0, paleta::PELICULA);
                d.texto(fx + 88.0, y1 + 3.0, &tc_de(c.t_out), 9.0, paleta::HUESO);
                d.texto(fx + 156.0, y1 + 3.0, &format!("{:.1}s", c.dur()), 10.0, paleta::TINTA);
                d.texto(fx + 12.0, y1 - 11.0, "ENTRADA", 6.0, paleta::TINTA_TENUE);
                d.texto(fx + 84.0, y1 - 11.0, "SALIDA", 6.0, paleta::TINTA_TENUE);
                // velocidad (clic: cicla) + sello si no es 1
                let y2 = y1 + 34.0;
                d.texto(fx + 12.0, y2, "velocidad", 9.0, paleta::TINTA);
                d.texto(fx + 84.0, y2, &format!("×{:.2}", c.speed), 10.0, paleta::TINTA);
                d.texto(fx + 132.0, y2 + 1.0, "(clic: cicla)", 7.0, paleta::TINTA_TENUE);
                if (c.speed - 1.0).abs() > 0.01 {
                    d.rect_rot(fx + fw - 46.0, y2 - 6.0, 34.0, 20.0, -0.12, [0.851, 0.2, 0.145, 0.14]);
                    trazo::caja(&mut d, fx + fw - 46.0, y2 - 6.0, 34.0, 20.0, 1.2, paleta::ROJO, 62);
                    d.texto_f(ui::Familia::Grot, fx + fw - 40.0, y2 - 3.0,
                              &format!("{}×", if c.speed >= 1.0 { c.speed as u32 } else { 0 }),
                              11.0, paleta::ROJO);
                }
                // el sonido del vídeo: la palanca
                let y3 = y2 + 24.0;
                d.texto(fx + 12.0, y3, "el sonido del vídeo", 9.0, paleta::TINTA);
                let on = !c.mute;
                d.rect(fx + 150.0, y3 + 4.0, 16.0, 7.0, [0.35, 0.33, 0.3, 1.0]);
                let (dx2, dy2) = if on { (10.0, -7.0) } else { (10.0, 11.0) };
                trazo::linea(&mut d, fx + 158.0, y3 + 7.0, fx + 158.0 + dx2, y3 + 7.0 + dy2, 2.2,
                             if on { paleta::TINTA } else { paleta::ROJO }, 63);
                if c.mute {
                    d.texto(fx + 178.0, y3, "muda", 8.0, paleta::ROJO);
                }
                // el croquis del encuadre (clic: modo encuadre · alt-clic: reset)
                let y5 = y3 + 26.0;
                d.texto(fx + 12.0, y5, "el encuadre", 9.0, paleta::TINTA);
                d.texto(fx + 92.0, y5 + 1.0,
                        if self.modo_encuadre == Some(i) { "(abierto · esc cierra)" }
                        else { "(clic: E · alt-clic resetea)" },
                        7.0, if self.modo_encuadre == Some(i) { paleta::ROJO }
                             else { paleta::TINTA_TENUE });
                let prop = pr.proporcion();
                let cw = 92.0f32;
                let chh = (cw / prop).clamp(34.0, 70.0);
                let cx0 = fx + 8.0;
                let cy0 = y5 + 16.0;
                trazo::caja(&mut d, cx0, cy0, cw, chh, 1.3, paleta::TINTA_TENUE, 64);
                // el cuadro del material sobre el lienzo, con su giro puesto:
                // la MISMA geometría que los tiradores del visor
                {
                    let q = Self::cuadro_encuadre(pr, i);
                    let p: Vec<(f32, f32)> = q.iter()
                        .map(|(u, v)| (cx0 + u * cw, cy0 + v * chh)).collect();
                    for k in 0..4 {
                        let j = (k + 1) % 4;
                        trazo::linea(&mut d, p[k].0, p[k].1, p[j].0, p[j].1, 1.5,
                                     paleta::ROJO, 65 + k as u32);
                    }
                    // el ancla, si no está en el centro
                    if (c.enc.ancla.0 - 0.5).abs() > 0.01 || (c.enc.ancla.1 - 0.5).abs() > 0.01 {
                        let t = self.tiradores(pr, i);
                        let _ = t;
                        let (sw, sh) = Self::medidas_fuente(pr, i);
                        let (pw, ph) = Self::lienzo(pr);
                        let (ew, eh) = filmlook_core::plan::extension(&c.enc, sw, sh, pw, ph);
                        let ax = cx0 + (0.5 + c.enc.pos.0 + (c.enc.ancla.0 - 0.5) * ew) * cw;
                        let ay = cy0 + (0.5 + c.enc.pos.1 + (c.enc.ancla.1 - 0.5) * eh) * chh;
                        trazo::linea(&mut d, ax - 4.0, ay, ax + 4.0, ay, 1.2, paleta::TINTA, 69);
                        trazo::linea(&mut d, ax, ay - 4.0, ax, ay + 4.0, 1.2, paleta::TINTA, 70);
                    }
                }
                // ── LOS NÚMEROS DEL ENCUADRE (§1.5 · B) ─────────────────
                // Arrastrar el número lo cambia (fino con ⇧, grueso con ⌥),
                // doble clic lo escribe a mano y alt-clic lo devuelve a su
                // valor limpio. Es el mismo gesto que los galvanómetros del
                // cuarto oscuro: no hay dos lenguajes en la misma aplicación.
                {
                    // los dos botones de orientación, bien visibles
                    let (bx1, bx2) = (cx0 + cw + 10.0, cx0 + cw + 56.0);
                    trazo::caja(&mut d, bx1, cy0, 40.0, 20.0, 1.2, paleta::TINTA, 78);
                    d.texto(bx1 + 8.0, cy0 + 5.0, "↺ 90", 9.0, paleta::TINTA);
                    trazo::caja(&mut d, bx2, cy0, 40.0, 20.0, 1.2, paleta::TINTA, 79);
                    d.texto(bx2 + 8.0, cy0 + 5.0, "↻ 90", 9.0, paleta::TINTA);
                    d.texto(bx1, cy0 + 24.0,
                            &format!("{}°", c.enc.cuartos as u32 * 90), 9.0, paleta::ROJO);
                    // el encaje y el volteo
                    let ey = cy0 + 40.0;
                    for (k, (etq, cual)) in [("dentro", proyecto::Encaje::Dentro),
                                             ("llena", proyecto::Encaje::Llena),
                                             ("estira", proyecto::Encaje::Estira)]
                        .iter().enumerate() {
                        let ex = bx1 + k as f32 * 30.0;
                        let on = c.enc.encaje == *cual;
                        if on { d.rect(ex - 2.0, ey - 2.0, 30.0, 14.0, [0.851, 0.2, 0.145, 0.16]); }
                        d.texto(ex, ey, &etq[..3], 7.5,
                                if on { paleta::ROJO } else { paleta::TINTA_TENUE });
                    }
                    for (k, etq) in ["↔", "↕"].iter().enumerate() {
                        let ex = bx1 + k as f32 * 24.0;
                        let on = if k == 0 { c.enc.voltea.0 } else { c.enc.voltea.1 };
                        trazo::caja(&mut d, ex, ey + 16.0, 12.0, 12.0, 1.1,
                                    if on { paleta::ROJO } else { paleta::TINTA_TENUE }, 80 + k as u32);
                        d.texto(ex + 16.0, ey + 18.0, etq, 8.0,
                                if on { paleta::ROJO } else { paleta::TINTA_TENUE });
                    }
                }
                let filas = self.filas_encuadre(cy0 + chh + 8.0);
                for (fy2, campo) in &filas {
                    let (rot, val) = Self::rotulo_campo(&c.enc, *campo);
                    d.texto(fx + 12.0, *fy2, rot, 8.0, paleta::TINTA_TENUE);
                    let escrito = self.tecleando.as_ref()
                        .filter(|(ci, ca, _)| *ci == i && ca == campo)
                        .map(|(_, _, t)| format!("{t}▏"));
                    d.texto(fx + 96.0, *fy2, escrito.as_deref().unwrap_or(&val), 9.5,
                            if escrito.is_some() { paleta::ROJO } else { paleta::TINTA });
                    // la barrita del recorrido, que dice de un vistazo por dónde va
                    let f = Self::fraccion_campo(&c.enc, *campo);
                    d.rect(fx + 154.0, *fy2 + 4.0, 62.0, 3.0, [0.80, 0.78, 0.72, 1.0]);
                    d.rect(fx + 154.0, *fy2 + 4.0, 62.0 * f, 3.0, paleta::TINTA_TENUE);
                }
                // la cinta washi (etiqueta de color)
                let y6 = filas.last().map(|(y, _)| y + 20.0).unwrap_or(cy0 + chh + 14.0);
                d.texto(fx + 12.0, y6, "washi", 9.0, paleta::TINTA);
                for k in 0..4u8 {
                    let wx = fx + 62.0 + k as f32 * 40.0;
                    self.objetos.quad_uv_rot(wx, y6 - 1.0, 34.0, 13.0,
                                             doodles::uv(doodles::WASHI[k as usize]),
                                             (k as f32 - 1.5) * 0.02);
                    if c.washi == Some(k) {
                        trazo::circulo(&mut d, wx + 17.0, y6 + 5.0, 22.0, 11.0, 1.4, paleta::ROJO,
                                       66 + k as u32);
                    }
                }
                // la receta del cuarto oscuro, en resumen
                let y7 = y6 + 26.0;
                trazo::linea(&mut d, fx + 10.0, y7 - 6.0, fx + fw - 10.0, y7 - 6.0, 1.1,
                             paleta::TINTA_TENUE, 70);
                d.texto(fx + 12.0, y7, "el cuarto oscuro", 8.0, paleta::TINTA_TENUE);
                let lee = |k: &str| c.prefs.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
                d.texto(fx + 12.0, y7 + 13.0,
                        &format!("grano {:.2} · halación {:.2} · sat {:.2}",
                                 lee("grain"), lee("halation"), lee("stockSat")),
                        8.0, paleta::TINTA);
                d.texto_f(ui::Familia::Mano, fx + 12.0, y7 + 26.0, "→ llévala al cuarto oscuro",
                          15.0, paleta::ROJO);
                // las acciones de tampón. Un RÓTULO enseña «texto» en vez de
                // «contacto»: hasta ahora se escribía al crearlo y ya (§5), y
                // lo que uno quiere es corregir la errata.
                let y8 = y7 + 52.0;
                let es_rotulo = c.media.starts_with("titulo_");
                for (k, rot) in [if es_rotulo { "texto" } else { "contacto" },
                                 "duplicar", "al cubo", "nota"].iter().enumerate() {
                    let bx2 = fx + 12.0 + (k % 3) as f32 * 70.0;
                    let by2 = y8 + (k / 3) as f32 * 26.0;
                    trazo::caja(&mut d, bx2, by2, 62.0, 20.0, 1.1, paleta::TINTA_TENUE,
                                72 + k as u32);
                    d.texto(bx2 + 8.0, by2 + 5.0, rot, 8.0, paleta::TINTA);
                }
                // la nota manuscrita, si la hay
                if !c.nota.is_empty() {
                    let y9 = y8 + 56.0;
                    d.rect_rot(fx + 12.0, y9, fw - 24.0, 30.0, -0.01, [1.0, 0.96, 0.62, 0.95]);
                    let nn: String = c.nota.chars().take(28).collect();
                    d.texto_f(ui::Familia::Mano, fx + 18.0, y9 + 4.0, &nn, 14.0, [0.2, 0.16, 0.05, 1.0]);
                }
            }
            _ => {
                // sin clip: EL PARTE DEL PROYECTO
                d.texto(fx + 12.0, fy + 16.0, "EL PARTE DEL PROYECTO", 8.0, paleta::TINTA_TENUE);
                let n: String = pr.nombre.chars().take(18).collect();
                d.texto_f(ui::Familia::Mano, fx + 12.0, fy + 28.0, &n, 19.0, paleta::TINTA);
                d.rect_rot(fx + 12.0, fy + 58.0, 120.0, 22.0, -0.015, [0.851, 0.2, 0.145, 0.10]);
                trazo::caja(&mut d, fx + 12.0, fy + 58.0, 120.0, 22.0, 1.2, paleta::ROJO, 75);
                d.texto_f(ui::Familia::Grot, fx + 20.0, fy + 62.0, &pr.rotulo_formato(), 11.0,
                          paleta::ROJO);
                let dur = pr.duracion().max(0.0);
                // el pietaje: 35 mm, 16 fotogramas por pie (el contador de cine)
                let cuadros = (dur * pr.fps.max(1.0)) as u64;
                let (pies, fr) = (cuadros / 16, cuadros % 16);
                for (k, (rot, val)) in [
                    ("duración", format!("{dur:.1} s")),
                    ("empalmes", format!("{}", pr.clips.len())),
                    ("música", format!("{} clip(s)", pr.audio.len())),
                    ("pietaje", format!("{pies} ft {fr} fr")),
                    ("marcas", format!("{}", pr.marcas.len())),
                    ("recortes en el cubo", format!("{}", self.recortes.len())),
                ].iter().enumerate() {
                    let yy = fy + 104.0 + k as f32 * 20.0;
                    d.texto(fx + 12.0, yy, rot, 9.0, paleta::TINTA_TENUE);
                    d.texto(fx + 128.0, yy, val, 10.0, paleta::TINTA);
                }
                // la chapa de mantenimiento del motor (NORTE §7.13)
                let y9 = fy + 104.0 + 6.0 * 20.0 + 10.0;
                trazo::caja(&mut d, fx + 12.0, y9, fw - 24.0, 34.0, 1.2, paleta::TINTA_TENUE, 76);
                let motor = if cfg!(target_os = "macos") { "VideoToolbox" } else { "Media Foundation" };
                d.texto(fx + 20.0, y9 + 5.0, &format!("motor {motor}"), 8.0, paleta::TINTA);
                d.texto(fx + 20.0, y9 + 18.0,
                        &format!("proyección {:.0} fps · al fotograma", self.visor.fps_medido),
                        8.0, paleta::TINTA_TENUE);
                d.texto(fx + 12.0, y9 + 48.0, "toca un clip de la bobina", 8.0, paleta::TINTA_TENUE);
                d.texto(fx + 12.0, y9 + 60.0, "para abrir su ficha", 8.0, paleta::TINTA_TENUE);
            }
        }
        }   // fin de «si no estás mirando una música»

        // ── LA FICHA DE LA CAPA (CAPAS §7) ──────────────────────────────
        if let Some(k) = self.sel_capa {
            if let Some(cp) = pr.capas.get(k) {
                let fx = ancho - Self::INSPECTOR_W + 10.0;
                let mut y = Self::CABECERA + 8.0;
                let alto_ficha = (banco - 10.0 - (y - 6.0)).max(250.0);
                trazo::linea(&mut d, fx - 10.0, Self::CABECERA + 8.0,
                             fx - 10.0, banco - 10.0, 1.6, paleta::TINTA_TENUE, 43);
                d.rect(fx - 6.0, y - 6.0, Self::INSPECTOR_W - 8.0, alto_ficha,
                       [0.98, 0.97, 0.94, 1.0]);
                trazo::caja(&mut d, fx - 6.0, y - 6.0, Self::INSPECTOR_W - 8.0,
                            alto_ficha, 1.4, paleta::TINTA, 930);
                d.texto_f(ui::Familia::Grot, fx + 4.0, y, "LA CAPA", 11.0, paleta::TINTA);
                y += 22.0;
                let nom: String = cp.c.media.chars().take(28).collect();
                d.texto_f(ui::Familia::Mano, fx + 4.0, y, &nom, 17.0, paleta::TINTA);
                y += 26.0;
                let fila = |d: &mut ui::Dibujo, y: f32, k2: &str, v: &str| {
                    d.texto(fx + 4.0, y, k2, 8.5, paleta::TINTA_TENUE);
                    d.texto(fx + 108.0, y, v, 9.5, paleta::TINTA);
                };
                fila(&mut d, y, "entra en la bobina", &format!("{:.2} s", cp.start)); y += 16.0;
                fila(&mut d, y, "dura", &format!("{:.2} s", cp.dur())); y += 16.0;
                fila(&mut d, y, "del original", &format!("{:.2} → {:.2}",
                                                         cp.c.t_in, cp.c.t_out)); y += 16.0;
                fila(&mut d, y, "qué es", if crate::foto::es_foto(&cp.c.ruta) {
                    "foto o rótulo (con su alfa)" } else { "vídeo (PiP)" }); y += 16.0;
                fila(&mut d, y, "pista", &format!("V{}", cp.pista as usize + 2));
                y += 20.0;
                let _ = y;
                let (y1b, _) = Self::musica_botones_y();
                let bot = |d: &mut ui::Dibujo, x: f32, y: f32, t: &str, on: bool| {
                    trazo::caja(d, x, y, 74.0, 20.0, 1.2,
                                if on { paleta::ROJO } else { paleta::TINTA_TENUE }, 941);
                    d.texto(x + 8.0, y + 5.0, t, 8.5,
                            if on { paleta::ROJO } else { paleta::TINTA });
                };
                bot(&mut d, fx + 4.0, y1b, &format!("entra {:.1}s", cp.fundido_in),
                    cp.fundido_in > 0.01);
                bot(&mut d, fx + 84.0, y1b, &format!("sale {:.1}s", cp.fundido_out),
                    cp.fundido_out > 0.01);
                let y3b = Self::musica_fila3_y();
                bot(&mut d, fx + 4.0, y3b, "encuadre a 0", false);
                bot(&mut d, fx + 84.0, y3b, "quitar (⌫)", false);
                d.texto(fx + 4.0, y3b + 26.0, "arrástrala para moverla · bordes: recortar",
                        8.0, paleta::TINTA_TENUE);
                d.texto(fx + 4.0, y3b + 37.0, "alt-arrastre en el visor: colocar el PiP",
                        8.0, paleta::TINTA_TENUE);
            }
        } else
        // ── LA FICHA DEL PIE (subtitulo.rs) ─────────────────────────────
        // El texto, sus tiempos y EL ESTILO DE TODA LA PISTA: un subtítulo
        // no se estila suelto, se estila la película entera.
        if let Some(is) = self.sel_sub {
            if let Some(sb) = pr.subs.get(is) {
                let fx = ancho - Self::INSPECTOR_W + 10.0;
                let mut y = Self::CABECERA + 8.0;
                let alto_ficha = (banco - 10.0 - (y - 6.0)).max(250.0);
                trazo::linea(&mut d, fx - 10.0, Self::CABECERA + 8.0,
                             fx - 10.0, banco - 10.0, 1.6, paleta::TINTA_TENUE, 43);
                d.rect(fx - 6.0, y - 6.0, Self::INSPECTOR_W - 8.0, alto_ficha,
                       [0.98, 0.97, 0.94, 1.0]);
                trazo::caja(&mut d, fx - 6.0, y - 6.0, Self::INSPECTOR_W - 8.0, alto_ficha,
                            1.4, paleta::TINTA, 930);
                d.texto_f(ui::Familia::Grot, fx + 4.0, y, "EL PIE", 11.0, paleta::TINTA);
                y += 20.0;
                // el texto, en grande y partido como saldrá
                let escribiendo = self.escribiendo_sub.as_ref()
                    .filter(|(k, _)| *k == is).map(|(_, t)| t.clone());
                let vivo = escribiendo.clone().unwrap_or_else(|| sb.texto.clone());
                let lineas = subtitulo::parte(&vivo, pr.estilo_sub.ancho_linea as usize);
                d.rect(fx + 2.0, y - 2.0, Self::INSPECTOR_W - 20.0,
                       14.0 + 17.0 * lineas.len().max(1) as f32,
                       if escribiendo.is_some() { [0.99, 0.94, 0.90, 1.0] }
                       else { [0.96, 0.95, 0.92, 1.0] });
                for (li, l) in lineas.iter().enumerate() {
                    let t = if escribiendo.is_some() && li + 1 == lineas.len() {
                        format!("{l}|") } else { l.clone() };
                    d.texto_f(ui::Familia::Mano, fx + 8.0, y + 3.0 + li as f32 * 17.0,
                              &t, 15.0, paleta::TINTA);
                }
                y += 20.0 + 17.0 * lineas.len().max(1) as f32;
                let fila = |d: &mut ui::Dibujo, y: f32, k: &str, v: &str| {
                    d.texto(fx + 4.0, y, k, 8.5, paleta::TINTA_TENUE);
                    d.texto(fx + 108.0, y, v, 9.5, paleta::TINTA);
                };
                fila(&mut d, y, "entra", &format!("{:.2} s", sb.t0)); y += 15.0;
                fila(&mut d, y, "sale", &format!("{:.2} s", sb.t1)); y += 15.0;
                fila(&mut d, y, "dura", &format!("{:.2} s", sb.dur())); y += 15.0;
                // la velocidad de lectura: el número que de verdad importa
                let cps = vivo.chars().count() as f64 / sb.dur().max(0.1);
                fila(&mut d, y, "caracteres/s", &format!("{cps:.1}{}", 
                     if cps > 21.0 { "  ¡corre!" } else { "" })); y += 15.0;
                fila(&mut d, y, "en la pista", &format!("{} de {}", is + 1, pr.subs.len()));
                y += 22.0;
                d.texto_f(ui::Familia::Grot, fx + 4.0, y, "EL ESTILO DE LA PISTA", 9.0,
                          paleta::TINTA); y += 16.0;
                let e = &pr.estilo_sub;
                let bot = |d: &mut ui::Dibujo, x: f32, y: f32, t: &str, on: bool| {
                    trazo::caja(d, x, y, 74.0, 20.0, 1.2,
                                if on { paleta::ROJO } else { paleta::TINTA_TENUE }, 941);
                    d.texto(x + 6.0, y + 5.0, t, 8.0,
                            if on { paleta::ROJO } else { paleta::TINTA });
                };
                // la Y sale del MISMO sitio que la lee el ratón
                let (ex, ey) = (fx + 4.0, self.pie_estilo_y(pr, is));
                let _ = y;
                bot(&mut d, ex, ey, subtitulo::FAMILIAS[(e.familia as usize).min(2)].0,
                    false);
                bot(&mut d, ex + 80.0, ey, subtitulo::TINTAS[(e.tinta as usize).min(3)].0,
                    false);
                bot(&mut d, ex, ey + 24.0, &format!("cuerpo {:.1}%", e.cuerpo * 100.0), false);
                bot(&mut d, ex + 80.0, ey + 24.0, &format!("alto {:.0}%", e.margen * 100.0),
                    false);
                bot(&mut d, ex, ey + 48.0,
                    &format!("sombra {:.0}%", e.sombra * 100.0), e.sombra > 0.01);
                bot(&mut d, ex + 80.0, ey + 48.0,
                    if e.caja > 0.01 { "con caja" } else { "sin caja" }, e.caja > 0.01);
                bot(&mut d, ex, ey + 72.0,
                    if e.mayusculas { "MAYÚSCULAS" } else { "normal" }, e.mayusculas);
                bot(&mut d, ex + 80.0, ey + 72.0, &format!("{} letras", e.ancho_linea), false);
                bot(&mut d, ex, ey + 96.0, "partir aquí", false);
                bot(&mut d, ex + 80.0, ey + 96.0, "quitar (⌫)", false);
                d.texto(fx + 4.0, ey + 122.0, "clic en el texto: escribirlo · ⏎ guarda",
                        8.0, paleta::TINTA_TENUE);
                d.texto(fx + 4.0, ey + 133.0, "clic en un mando: lo cicla · ⇧+clic: atrás",
                        8.0, paleta::TINTA_TENUE);
                d.texto(fx + 4.0, ey + 144.0, "los bordes del bloque estiran el tiempo",
                        8.0, paleta::TINTA_TENUE);
            }
        } else
        // ── LA FICHA DE LA MÚSICA ───────────────────────────────────────
        // Una pista de música es un clip como otro cualquiera y merece su
        // ficha: hasta ahora solo se podía añadir y arrastrar a ciegas.
        if let Some(ia) = self.sel_audio {
            if let Some(a) = pr.audio.get(ia) {
                let fx = ancho - Self::INSPECTOR_W + 10.0;
                let mut y = Self::CABECERA + 8.0;
                // el panel es SUYO: la del clip no se ha dibujado
                let alto_ficha = (banco - 10.0 - (y - 6.0)).max(250.0);
                trazo::linea(&mut d, fx - 10.0, Self::CABECERA + 8.0,
                             fx - 10.0, banco - 10.0, 1.6, paleta::TINTA_TENUE, 43);
                d.rect(fx - 6.0, y - 6.0, Self::INSPECTOR_W - 8.0, alto_ficha,
                       [0.98, 0.97, 0.94, 1.0]);
                trazo::caja(&mut d, fx - 6.0, y - 6.0, Self::INSPECTOR_W - 8.0, alto_ficha,
                            1.4, paleta::TINTA, 930);
                d.texto_f(ui::Familia::Grot, fx + 4.0, y, "LA PISTA DE MÚSICA", 11.0, paleta::TINTA);
                y += 22.0;
                let nom: String = a.media.chars().take(28).collect();
                d.texto_f(ui::Familia::Mano, fx + 4.0, y, &nom, 17.0, paleta::TINTA);
                y += 26.0;
                let fila = |d: &mut ui::Dibujo, y: f32, k: &str, v: &str| {
                    d.texto(fx + 4.0, y, k, 8.5, paleta::TINTA_TENUE);
                    d.texto(fx + 108.0, y, v, 9.5, paleta::TINTA);
                };
                fila(&mut d, y, "entra en la bobina", &format!("{:.2} s", a.start)); y += 16.0;
                fila(&mut d, y, "dura", &format!("{:.2} s", a.dur())); y += 16.0;
                fila(&mut d, y, "del original", &format!("{:.2} → {:.2}", a.t_in, a.t_out)); y += 20.0;
                // EL VOLUMEN, a la vista y con un mando de verdad
                d.texto(fx + 4.0, y, "volumen", 8.5, paleta::TINTA_TENUE);
                d.texto(fx + 108.0, y, &format!("{:+.1} dB", a.gain), 10.0,
                        if a.mute { paleta::TINTA_TENUE } else { paleta::ROJO });
                y += 15.0;
                let (bx, bw) = (fx + 6.0, Self::INSPECTOR_W - 34.0);
                d.rect(bx, y, bw, 5.0, [0.80, 0.78, 0.72, 1.0]);
                let f = ((a.gain + 40.0) / 52.0).clamp(0.0, 1.0) as f32;   // −40 … +12 dB
                d.rect(bx, y, bw * f, 5.0, if a.mute { paleta::TINTA_TENUE } else { paleta::ROJO });
                d.rect(bx + bw * f - 2.0, y - 4.0, 4.0, 13.0, paleta::TINTA);
                y += 20.0;
                fila(&mut d, y, "fundidos", &format!("{:.1} s / {:.1} s", a.fade_in, a.fade_out)); y += 16.0;
                fila(&mut d, y, "puntos de banda", &format!("{}", a.banda.len())); y += 16.0;
                fila(&mut d, y, "carril", &format!("{} de {}", a.pista + 1,
                                                   proyecto::PISTAS_MUSICA)); y += 16.0;
                fila(&mut d, y, "desfase fino", &format!("{:+.2} s", a.desfase));
                // LOS BOTONES viven en una geometría con nombre: la misma que
                // lee el clic, para que no puedan descolocarse
                let (y, y2) = Self::musica_botones_y();
                let bot = |d: &mut ui::Dibujo, x: f32, y: f32, t: &str, on: bool| {
                    trazo::caja(d, x, y, 74.0, 20.0, 1.2,
                                if on { paleta::ROJO } else { paleta::TINTA_TENUE }, 940);
                    d.texto(x + 8.0, y + 5.0, t, 8.5,
                            if on { paleta::ROJO } else { paleta::TINTA });
                };
                bot(&mut d, fx + 4.0, y, if a.mute { "callada" } else { "sonando" }, a.mute);
                bot(&mut d, fx + 84.0, y, "quitar (⌫)", false);
                // NORMALIZAR ESTA PISTA sin tocar las demás (§4bis.11)
                bot(&mut d, fx + 4.0, y2, "normalizar", false);
                bot(&mut d, fx + 84.0, y2, "al carril →", false);
                // LOS FUNDIDOS, como en la ficha del clip: un clic los cicla.
                // Estaban en el modelo y se dibujaban, pero no había forma de
                // tocarlos desde aquí — que es de lo que se quejaba el autor:
                // la música no terminaba de tener los mandos de un clip.
                let y3 = Self::musica_fila3_y();
                bot(&mut d, fx + 4.0, y3, &format!("entra {:.1}s", a.fade_in), a.fade_in > 0.01);
                bot(&mut d, fx + 84.0, y3, &format!("sale {:.1}s", a.fade_out), a.fade_out > 0.01);
                // EL COMPÁS: marcas al ritmo de esta cinta (imanes al pulso)
                let y4 = Self::musica_fila4_y();
                let con_compas = pr.marcas.iter().any(|m| m.nota == "♩");
                bot(&mut d, fx + 4.0, y4, "al compás ♩", con_compas);
                bot(&mut d, fx + 84.0, y4, "compás fuera", false);
                d.texto(fx + 4.0, y4 + 26.0, "arrastra la cinta para moverla", 8.0,
                        paleta::TINTA_TENUE);
                d.texto(fx + 4.0, y4 + 37.0, "los bordes RECORTAN Y ESTIRAN · B corta",
                        8.0, paleta::TINTA_TENUE);
                d.texto(fx + 4.0, y4 + 48.0, "los puntos de volumen, sin modificador",
                        8.0, paleta::TINTA_TENUE);
            }
        }

        // ── el vidrio: la PROPORCIÓN DEL PROYECTO manda (letterbox real) ──
        let zona_w = ancho - Self::ESTANTE_W - Self::INSPECTOR_W - 40.0;
        let zona_h = banco - Self::CABECERA - 30.0;
        let prop = pr.proporcion();
        let mut gw = zona_w;
        let mut gh = gw / prop;
        if gh > zona_h { gh = zona_h; gw = gh * prop; }
        let gx = Self::ESTANTE_W + 20.0 + (zona_w - gw) / 2.0;
        let gy = Self::CABECERA + 15.0 + (zona_h - gh) / 2.0;
        d.rect(gx - 3.0, gy - 3.0, gw + 6.0, gh + 6.0, paleta::TINTA);
        d.rect(gx, gy, gw, gh, paleta::NEGRO);
        // marcas de registro ⌖ en las esquinas del vidrio (fuera del cristal)
        for (mx2, my2) in [(gx - 12.0, gy - 12.0), (gx + gw + 12.0, gy - 12.0),
                           (gx - 12.0, gy + gh + 12.0), (gx + gw + 12.0, gy + gh + 12.0)] {
            let sem = trazo::semilla_de(mx2, my2);
            trazo::circulo(&mut d, mx2, my2, 5.0, 5.0, 1.1, paleta::TINTA_TENUE, sem);
            trazo::linea(&mut d, mx2 - 8.0, my2, mx2 + 8.0, my2, 1.0, paleta::TINTA_TENUE, sem ^ 1);
            trazo::linea(&mut d, mx2, my2 - 8.0, mx2, my2 + 8.0, 1.0, paleta::TINTA_TENUE, sem ^ 2);
        }
        // el material encaja DENTRO del lienzo del proyecto (fit, jamás estirar)
        let (vw, vh) = self.visor.encaje(gw, gh);
        let vx = gx + (gw - vw) / 2.0;
        let vy = gy + (gh - vh) / 2.0;
        self.visor.rect_pantalla = [vx, vy, vw, vh];

        // ── LA LUPA CUENTAHÍLOS (mantener ⌥ sobre el vidrio) ──
        if let Some((lx, ly)) = self.lupa {
            // el CUENTAHÍLOS de artes gráficas: base cuadrada, patas y retícula
            let r = 84.0f32;
            for (o, c) in [(6.0f32, [0.30, 0.24, 0.13, 0.55]), (3.0, [0.55, 0.44, 0.24, 1.0])] {
                trazo::caja(&mut d2, lx - r - o, ly - r - o, (r + o) * 2.0, (r + o) * 2.0,
                            2.6, c, 960 + o as u32);
            }
            // las patas plegables del cuentahílos
            for sx in [-1.0f32, 1.0] {
                trazo::linea(&mut d2, lx + sx * (r + 4.0), ly - r - 4.0,
                             lx + sx * (r + 26.0), ly + r + 16.0, 2.0,
                             [0.45, 0.36, 0.2, 1.0], 963 + sx as u32 as u32);
            }
            // la retícula grabada en el vidrio
            for k in -2..=2 {
                let f = k as f32 * 22.0;
                d2.rect(lx - r + 4.0, ly + f, r * 2.0 - 8.0, 0.6, [0.2, 0.25, 0.5, 0.22]);
                d2.rect(lx + f, ly - r + 4.0, 0.6, r * 2.0 - 8.0, [0.2, 0.25, 0.5, 0.22]);
            }
            d2.texto(lx - 30.0, ly + r + 20.0, "×4 · al píxel", 8.0, paleta::TINTA);
        }
        // ── el acetato de guías (tercios + centro), tecla A ──
        if self.acetato && self.fuente.is_none() {
            let ac = [0.55, 0.42, 0.35, 0.55];
            for k in 1..3 {
                let f = k as f32 / 3.0;
                trazo::linea(&mut d2, gx + gw * f, gy + 4.0, gx + gw * f, gy + gh - 4.0, 1.1, ac,
                             900 + k as u32);
                trazo::linea(&mut d2, gx + 4.0, gy + gh * f, gx + gw - 4.0, gy + gh * f, 1.1, ac,
                             910 + k as u32);
            }
            trazo::circulo(&mut d2, gx + gw / 2.0, gy + gh / 2.0, 8.0, 8.0, 1.1, ac, 920);
            // la pestañita del acetato asomando
            d2.rect_rot(gx + gw - 40.0, gy - 8.0, 34.0, 14.0, 0.06, [0.85, 0.88, 0.9, 0.55]);
            d2.texto(gx + gw - 36.0, gy - 6.0, "guías", 7.0, [0.2, 0.25, 0.3, 0.9]);
        }

        // ── EL ENCUADRE SOBRE LA IMAGEN (§1.5 · A) ──────────────────────
        // Con el modo abierto, los tiradores; sin él, el recuadro TENUE del
        // clip reencuadrado — para saber que lo está sin tener que entrar.
        {
            let bajo = pr.en(self.visor.t).map(|x| x.0);
            let (cual, vivo) = match self.modo_encuadre {
                Some(i) => (Some(i), true),
                None => (bajo.filter(|i| pr.clips.get(*i)
                    .map(|c| !c.hueco && !c.enc.es_limpio(c.cuartos_fichero))
                    .unwrap_or(false)), false),
            };
            if let Some(i) = cual.filter(|i| *i < pr.clips.len() && self.fuente.is_none()) {
                let q = Self::cuadro_encuadre(pr, i);
                let p: Vec<(f32, f32)> = q.iter().map(|(u, v)| self.a_pantalla(*u, *v)).collect();
                let tinta = if vivo { paleta::ROJO } else { [0.85, 0.2, 0.145, 0.35] };
                for k in 0..4 {
                    let j = (k + 1) % 4;
                    trazo::linea(&mut d2, p[k].0, p[k].1, p[j].0, p[j].1,
                                 if vivo { 1.8 } else { 1.1 }, tinta, 1200 + k as u32);
                }
                if vivo {
                    let t = self.tiradores(pr, i);
                    // ○ las esquinas y los bordes
                    for k in 0..8 {
                        let (hx, hy) = t[k];
                        d2.rect(hx - 4.0, hy - 4.0, 8.0, 8.0, paleta::HUESO);
                        trazo::caja(&mut d2, hx - 4.0, hy - 4.0, 8.0, 8.0, 1.3, paleta::ROJO,
                                    1210 + k as u32);
                    }
                    // ✛ el ancla, arrastrable
                    let (ax, ay) = t[8];
                    trazo::linea(&mut d2, ax - 9.0, ay, ax + 9.0, ay, 1.6, paleta::AMBAR, 1230);
                    trazo::linea(&mut d2, ax, ay - 9.0, ax, ay + 9.0, 1.6, paleta::AMBAR, 1231);
                    trazo::circulo(&mut d2, ax, ay, 5.0, 5.0, 1.4, paleta::AMBAR, 1232);
                    // ↻ el giro
                    let (rx2, ry2) = t[9];
                    trazo::circulo(&mut d2, rx2, ry2, 7.0, 7.0, 1.5, paleta::AMBAR, 1233);
                    let c = &pr.clips[i];
                    let rot = format!("×{:.2} · {:.1}° · {}°", c.enc.escala.0, c.enc.giro,
                                      c.enc.cuartos as u32 * 90);
                    d2.texto(gx + 8.0, gy + 8.0, &rot, 10.0, paleta::AMBAR);
                }
            }
        }

        // ── el banco: la bobina ──
        d.rect(0.0, banco, ancho, self.banco_h, [0.922, 0.906, 0.863, 0.45]);
        trazo::linea(&mut d, 0.0, banco + 1.0, ancho, banco + 1.0, 2.2, paleta::TINTA, 91);
        // el margen izquierdo del banco: el rótulo de la bobina y los datos
        d.texto_f(ui::Familia::Grot, 10.0, banco + 8.0, "LA BOBINA", 11.0, paleta::TINTA);
        d.texto(10.0, banco + 26.0,
                &format!("{:.1} s · {} clip(s)", pr.duracion().max(0.0), pr.clips.len()),
                9.0, paleta::TINTA_TENUE);
        d.texto(10.0, banco + 40.0, "? = la chuleta", 9.0, paleta::TINTA_TENUE);
        let ty = self.tira_y();
        let alto_tira = 88.0;
        // ── el monitor de FUENTE: banner y barra con marcas I/O ──
        if let Some(f) = &self.fuente {
            d.rect(gx, gy, gw, 26.0, [0.851, 0.2, 0.145, 0.92]);
            let n: String = f.cinta.nombre.chars().take(28).collect();
            d.texto_f(ui::Familia::Grot, gx + 10.0, gy + 5.0, &format!("FUENTE · {n}"), 13.0, paleta::HUESO);
            if gw > 620.0 {
                d.texto(gx + gw - 330.0, gy + 7.0, "I/O marcan · ⏎ inserta · ⇧⏎ al final · esc vuelve",
                        10.0, [0.98, 0.92, 0.88, 1.0]);
            } else {
                d.texto(gx + 10.0, gy + 30.0, "I/O marcan · ⏎ inserta · esc vuelve",
                        10.0, paleta::NARANJA);
            }
            let x0 = Self::ESTANTE_W + 12.0;
            let x1 = ancho - 24.0;
            let by = banco + 24.0;
            d.rect(x0, by, x1 - x0, 12.0, paleta::PELICULA);
            let px = |t: f64| x0 + ((t / f.cinta.dur.max(0.1)) as f32) * (x1 - x0);
            let (mi, mo) = (f.marca_i.unwrap_or(0.0), f.marca_o.unwrap_or(f.cinta.dur));
            d.rect(px(mi), by, (px(mo) - px(mi)).max(2.0), 12.0, [0.949, 0.78, 0.267, 0.55]);
            if let Some(i0) = f.marca_i { d.rect(px(i0) - 1.5, by - 4.0, 3.0, 20.0, paleta::AMBAR); }
            if let Some(o0) = f.marca_o { d.rect(px(o0) - 1.5, by - 4.0, 3.0, 20.0, paleta::AMBAR); }
            let ax = px(self.visor.t);
            d.rect(ax - 1.0, by - 6.0, 2.5, 24.0, paleta::ROJO);
            d.texto(x0, by + 16.0, &format!("{:.1} / {:.1} s · {}x{} · {:.2} fps",
                    self.visor.t, f.cinta.dur, f.cinta.w, f.cinta.h, f.cinta.fps),
                    10.0, paleta::TINTA_TENUE);
            // la CINTA DE 6 FOTOGRAMAS, desenrollada al pie del vidrio
            // (NORTE §3.1: navegación — tocar un fotograma salta allí)
            let proxy = pr.base.join(".proxies").join(&f.cinta.nombre);
            let ruta6 = if proxy.is_file() { proxy } else { f.cinta.ruta.clone() };
            let aw6 = ((gw - 36.0) / 6.0).min(108.0);
            let ah6 = aw6 * 9.0 / 16.0;
            let sx6 = gx + (gw - aw6 * 6.0) / 2.0;
            let sy6 = gy + gh - ah6 - 24.0;
            d.rect(sx6 - 6.0, sy6 - 8.0, aw6 * 6.0 + 12.0, ah6 + 30.0, [0.07, 0.065, 0.05, 0.85]);
            for k in 0..6 {
                let frac = [0.02f64, 0.2, 0.4, 0.6, 0.8, 0.98][k];
                let t6 = f.cinta.dur * frac;
                let clave = (format!("fuente6:{}:{k}", f.cinta.nombre), (t6 * 100.0) as u32);
                let x6 = sx6 + k as f32 * aw6;
                if let Some(slot) = self.minis.pide(clave, &ruta6, t6) {
                    dt2.quad(x6 + 2.0, sy6, aw6 - 4.0, ah6, slot, 1.0);
                }
                // perforaciones entre fotogramas
                for pk in 0..3 {
                    d.rect(x6 - 1.5, sy6 + 4.0 + pk as f32 * (ah6 / 3.0), 3.5, 5.0,
                           [0.9, 0.88, 0.8, 0.9]);
                }
                d.texto(x6 + 4.0, sy6 + ah6 + 4.0, &format!("{t6:.1}s"), 8.0,
                        [0.9, 0.85, 0.7, 0.9]);
            }
        }
        // ── EL RANGO DE LA BOBINA (§4bis.2) ────────────────────────────
        // El JSON traía el campo desde siempre y no lo leía nadie. Con él
        // vienen de golpe el bucle, revelar solo el tramo y sacar un trozo
        // para enseñárselo a alguien.
        if let Some((ra, rb)) = pr.rango {
            let (xa, xb) = (self.x_de(ra), self.x_de(rb));
            // lo que queda FUERA del rango se apaga
            if xa > Self::ESTANTE_W {
                d.rect_rec(Self::ESTANTE_W, Self::ESTANTE_W, ty - 22.0,
                           xa - Self::ESTANTE_W, alto_tira + 30.0, [0.10, 0.10, 0.08, 0.16]);
            }
            if xb < ancho {
                d.rect_rec(Self::ESTANTE_W, xb, ty - 22.0, ancho - xb, alto_tira + 30.0,
                           [0.10, 0.10, 0.08, 0.16]);
            }
            for (k, x) in [xa, xb].iter().enumerate() {
                if *x < Self::ESTANTE_W || *x > ancho { continue; }
                d.rect(x - 1.5, ty - 26.0, 3.0, alto_tira + 34.0, paleta::AMBAR);
                // la banderita, para poder cogerla
                d.tri([*x, ty - 26.0], [x + if k == 0 { 12.0 } else { -12.0 }, ty - 22.0],
                      [*x, ty - 18.0], paleta::AMBAR);
            }
            let etq = format!("rango {:.1}–{:.1} s{}", ra, rb,
                              if self.bucle { " · en bucle" } else { "" });
            if xa > Self::ESTANTE_W {
                d.texto(xa + 4.0, ty - 44.0, &etq, 8.0, paleta::NARANJA);
            }
        }
        // la regla del tiempo, a pulso (la línea azul del zine)
        trazo::linea(&mut d, Self::ESTANTE_W + 4.0, ty - 14.0, ancho - 6.0, ty - 14.0,
                     1.8, paleta::TINTA, 77);
        let mut s0 = 0.0f64;
        while self.x_de(s0) < ancho && s0 < pr.duracion() + 5.0 {
            let x = self.x_de(s0);
            if x > Self::ESTANTE_W {
                d.rect(x, ty - 20.0, 1.2, 7.0, paleta::TINTA);
                if self.pxs > 12.0 {
                    d.texto(x + 2.0, ty - 34.0, &format!("{:.0}s", s0), 9.0, paleta::TINTA_TENUE);
                }
            }
            s0 += if self.pxs > 40.0 { 1.0 } else if self.pxs > 12.0 { 5.0 } else { 15.0 };
        }
        let mut acc = 0.0f64;
        for (i, c) in pr.clips.iter().enumerate() {
            let x = self.x_de(acc);
            let w = (c.dur() as f32 * self.pxs).max(6.0);
            if x + w > Self::ESTANTE_W && x < ancho {
                d.rect_rec(Self::ESTANTE_W, x, ty, w, alto_tira,
                           if c.hueco { [0.08, 0.07, 0.05, 1.0] }
                           else if c.ausente { [0.30, 0.10, 0.08, 1.0] }
                           else { paleta::PELICULA });
                if c.anidada.is_some() && x + w > Self::ESTANTE_W {
                    // UNA BOBINA DENTRO: marco doble, como una lata precintada
                    let x0 = x.max(Self::ESTANTE_W);
                    let x1 = (x + w).min(ancho);
                    trazo::caja(&mut d2, x0 + 3.0, ty + 9.0, (x1 - x0 - 6.0).max(4.0),
                                alto_tira - 18.0, 1.2, [0.16, 0.25, 0.65, 0.9], 1460);
                    d2.texto_f(ui::Familia::Mano, x0 + 10.0, ty + 14.0,
                               &format!("⤷ {}", c.anidada.clone().unwrap_or_default()
                                        .chars().take(18).collect::<String>()),
                               14.0, [0.16, 0.25, 0.65, 1.0]);
                }
                if c.ausente && x + w > Self::ESTANTE_W {
                    // rayado a mano: se ve de lejos que ese plano no tiene
                    // fichero, y la ficha dice cómo recuperarlo
                    let mut sx2 = x.max(Self::ESTANTE_W);
                    while sx2 < (x + w).min(ancho) {
                        trazo::linea(&mut d, sx2, ty + alto_tira, sx2 + 18.0, ty, 1.0,
                                     [0.949, 0.78, 0.267, 0.5], 1500);
                        sx2 += 14.0;
                    }
                    let n: String = format!("FALTA {}", c.media);
                    d.texto_f(ui::Familia::Grot, (x + 6.0).max(Self::ESTANTE_W + 6.0),
                              ty + alto_tira / 2.0 - 6.0,
                              &n.chars().take(((w - 12.0) / 7.0).max(0.0) as usize)
                                 .collect::<String>(),
                              11.0, paleta::AMBAR);
                }
                if !c.hueco {
                    let mut px = (x + 5.0).max(Self::ESTANTE_W);
                    let _ = &mut px;
                    // las perforaciones caen en la rejilla del clip aunque
                    // la cabeza quede fuera de la vista
                    if x + 5.0 < Self::ESTANTE_W {
                        let saltos = ((Self::ESTANTE_W - x - 5.0) / 12.0).ceil();
                        px = x + 5.0 + saltos * 12.0;
                    }
                    while px < (x + w - 7.0).min(ancho) {
                        d.rect(px, ty + 4.0, 6.5, 5.0, paleta::HUESO);
                        d.rect(px, ty + alto_tira - 9.0, 6.5, 5.0, paleta::HUESO);
                        px += 12.0;
                    }
                    // fotogramas REALES dentro de la tira (del proxy, instantáneos).
                    // Un clip ANIDADO enseña los del primer clip de su hija.
                    let (media_m, ruta_m) = match c.anidada.as_ref()
                        .and_then(|k| pr.subbobinas.get(k))
                        .and_then(|sb| sb.clips.first()) {
                        Some(hc) => (hc.media.clone(), hc.ruta.clone()),
                        None => (c.media.clone(), c.ruta.clone()),
                    };
                    let proxy = pr.base.join(".proxies").join(&media_m);
                    let ruta = if proxy.is_file() { proxy } else { ruta_m };
                    let alto_foto = alto_tira - 22.0;
                    // el lienzo de la miniatura es 16:9 fijo; el contenido ya
                    // encaja dentro con sus bandas (fuente vertical incluida)
                    let ancho_foto = (alto_foto * 16.0 / 9.0).round();
                    let mut k = 0f32;
                    while x + k * ancho_foto < (x + w).min(ancho) {
                        let xk = x + k * ancho_foto;
                        let restante = (x + w - xk).min(ancho_foto);
                        k += 1.0;
                        if restante < 8.0 || xk + restante < Self::ESTANTE_W { continue; }
                        let src_t = (c.t_in + ((xk - x) / self.pxs) as f64)
                            .clamp(c.t_in, (c.t_out - 0.02).max(c.t_in));
                        let clave = (media_m.clone(), (src_t * 100.0) as u32);
                        if let Some(slot) = self.minis.pide(clave, &ruta, src_t) {
                            dt.quad_rec(Self::ESTANTE_W, xk, ty + 11.0, restante, alto_foto,
                                        slot, restante / ancho_foto);
                        }
                    }
                    // la ONDA del clip, dentro de la tira (franja baja)
                    if let Some(picos) = self.ondas.pide(&c.media, &c.ruta).cloned() {
                        let oy = ty + alto_tira - 26.0;
                        let oh = 14.0f32;
                        let dur_total = (ondas::CUBOS as f64).max(1.0);
                        let _ = dur_total;
                        // clip [t_in, t_out] sobre la cinta entera → índices de cubo
                        if let Ok((_, _, _, dur_cinta)) = filmlook_core::indice::sondea(&c.ruta) {
                            let paso_px = 3.0f32;
                            let mut px2 = x.max(Self::ESTANTE_W);
                            while px2 < (x + w).min(ancho) - 1.0 {
                                let frac = ((px2 - x) / w) as f64;
                                let t_src = c.t_in + frac * (c.t_out - c.t_in);
                                let k = ((t_src / dur_cinta.max(0.1)) * ondas::CUBOS as f64) as usize;
                                let v = picos.get(k.min(ondas::CUBOS - 1)).copied().unwrap_or(0.0);
                                let h2 = (v * oh).max(1.0);
                                d2.rect(px2, oy + (oh - h2) / 2.0, 2.0, h2,
                                        [0.949, 0.78, 0.267, 0.75]);
                                px2 += paso_px;
                            }
                        }
                    }
                    if w > 60.0 {
                        // el nombre del clip, manuscrito en rojo sobre la tira
                        // (si la cabeza se fue por la izquierda, se ancla al margen)
                        let xr = (x + 6.0).max(Self::ESTANTE_W + 6.0);
                        let visible = (x + w - xr).max(0.0);
                        if visible > 44.0 {
                            let n: String = c.media.chars().take((visible / 7.0) as usize).collect();
                            d.texto_f(ui::Familia::Mano, xr, ty - 13.0, &n, 14.0, paleta::ROJO);
                        }
                    }
                    // la nota manuscrita asoma como pico de post-it
                    if !c.nota.is_empty() && x + w - 26.0 > Self::ESTANTE_W {
                        d.rect_rot(x + w - 26.0, ty + 4.0, 18.0, 14.0, -0.12,
                                   [1.0, 0.96, 0.62, 0.95]);
                    }
                    // la grapa une a los hermanos contiguos
                    if let Some(g) = c.grupo {
                        if i + 1 < pr.clips.len() && pr.clips[i + 1].grupo == Some(g)
                            && x + w - 10.0 > Self::ESTANTE_W {
                            self.objetos.quad_uv_rot(x + w - 10.0, ty - 8.0, 20.0, 20.0,
                                                     doodles::uv(doodles::GRAPA),
                                                     std::f32::consts::FRAC_PI_2);
                        }
                    }
                }
                if !c.enc.es_limpio(c.cuartos_fichero) {
                    let etq = if c.enc.cuartos != c.cuartos_fichero {
                        format!("⌖{}°", (c.enc.cuartos as u32 * 90) % 360)
                    } else {
                        format!("⌖{:.1}", c.enc.escala.0)
                    };
                    d.texto_f(ui::Familia::Grot, x + 4.0, ty + 4.0, &etq, 10.0, paleta::AMBAR);
                }
                if (c.speed - 1.0).abs() > 0.01 {
                    d.texto_f(ui::Familia::Grot, x + w - 46.0, ty - 15.0,
                              &format!("×{:.2}", c.speed), 11.0, paleta::ROJO);
                }
                if c.fade > 0.01 {
                    let fw = (c.fade as f32 * self.pxs).max(14.0);
                    d.rect(x, ty, fw.min(w), alto_tira, [1.0, 1.0, 1.0, 0.22]);
                    d.rect(x, ty, fw.min(w), 2.0, paleta::AMBAR);
                    d.texto(x + 3.0, ty - 15.0, &format!("{:.1}s", c.fade), 9.0, paleta::NARANJA);
                }
                if Some(i) == self.sel || self.seleccion.contains(&i) {
                    d.rect(x - 2.0, ty - 2.0, w + 4.0, 2.0, paleta::ROJO);
                    d.rect_rec(Self::ESTANTE_W, x - 2.0, ty + alto_tira, w + 4.0, 2.0, paleta::ROJO);
                    if x - 2.0 > Self::ESTANTE_W {
                        d.rect(x - 2.0, ty - 2.0, 2.0, alto_tira + 4.0, paleta::ROJO);
                    }
                    if x + w > Self::ESTANTE_W {
                        d.rect(x + w, ty - 2.0, 2.0, alto_tira + 4.0, paleta::ROJO);
                    }
                }
                // la cinta de empalme DE VERDAD (splice_tape.png) sobre la junta
                if x + w - 8.0 > Self::ESTANTE_W {
                    self.tape.quad_uv(x + w - 8.0, ty - 9.0, 16.0, alto_tira + 18.0,
                                      [0.0, 0.0, 1.0, 1.0]);
                }
            }
            acc += c.dur();
        }
        // ── la pista de MÚSICA: cinta magnética marrón (NORTE §3.5) ──
        let my_y = self.pista_y(0);
        // los CARRILES vacíos, dibujados tenues: con dos o tres canciones hay
        // que verlas apiladas y no encimadas (§2)
        for k in 0..self.musica_vis {
            let py = self.pista_y(k as u8);
            trazo::linea(&mut d, Self::ESTANTE_W + 4.0, py + Self::ALTO_PISTA - 2.0,
                         ancho - 6.0, py + Self::ALTO_PISTA - 2.0, 0.9,
                         [0.169, 0.231, 0.78, 0.14], 1300 + k as u32);
        }
        for (ia, a) in pr.audio.iter().enumerate() {
            let ax0 = self.x_de(a.entra());
            let aw = (a.dur() as f32 * self.pxs).max(6.0);
            let my_y = self.pista_y(a.pista);
            if ax0 + aw > Self::ESTANTE_W && ax0 < ancho {
                // la cinta: marrón óxido con los cantos oscuros
                let mudo = pr.mudo_musica || a.mute;
                if self.sel_audio == Some(ia) {
                    d.rect_rec(Self::ESTANTE_W, ax0 - 2.0, my_y - 2.0, aw + 4.0, 28.0,
                               [0.851, 0.2, 0.145, 0.35]);
                }
                d.rect_rec(Self::ESTANTE_W, ax0, my_y, aw, 24.0,
                           if mudo { [0.42, 0.33, 0.26, 0.5] } else { [0.42, 0.30, 0.20, 1.0] });
                d.rect_rec(Self::ESTANTE_W, ax0, my_y, aw, 2.5, [0.25, 0.17, 0.11, 1.0]);
                d.rect_rec(Self::ESTANTE_W, ax0, my_y + 21.5, aw, 2.5, [0.25, 0.17, 0.11, 1.0]);
                // la onda, pintada en lápiz blanco sobre la cinta
                if let Some(picos) = self.ondas.pide(&a.media, &a.ruta).cloned() {
                    let mut px3 = ax0.max(Self::ESTANTE_W);
                    while px3 < (ax0 + aw).min(ancho) - 1.0 {
                        let frac = ((px3 - ax0) / aw) as f64;
                        let t_src = a.t_in + frac * (a.t_out - a.t_in);
                        let k = ((t_src / (a.t_out - a.t_in).max(0.1)).min(1.0)
                                 * ondas::CUBOS as f64) as usize;
                        let v = picos.get(k.min(ondas::CUBOS - 1)).copied().unwrap_or(0.0);
                        let h3 = (v * 17.0).max(1.0);
                        d.rect(px3, my_y + (24.0 - h3) / 2.0, 2.0, h3,
                               [0.95, 0.93, 0.88, if mudo { 0.35 } else { 0.85 }]);
                        px3 += 3.0;
                    }
                }
                // LA BANDA ELÁSTICA, siempre a la vista. Existía pero solo se
                // tocaba con alt-clic y no se veía nada: una pista sin puntos
                // enseña ahora su nivel como una línea, y los puntos se
                // arrastran sin modificador (§2).
                {
                    let ya = |db: f64| my_y + 24.0 * (1.0 - ((db + 24.0) / 30.0).clamp(0.0, 1.0)) as f32;
                    let xa = |t: f64| ax0 + (((t - a.t_in) / (a.t_out - a.t_in).max(1e-9)) as f32) * aw;
                    if a.banda.is_empty() {
                        // el nivel plano de la pista: una línea de punta a punta
                        let y0 = ya(a.gain);
                        trazo::linea(&mut d, ax0.max(Self::ESTANTE_W), y0,
                                     (ax0 + aw).min(ancho), y0, 1.2,
                                     [0.910, 0.314, 0.102, 0.65], 810 + ia as u32);
                    } else {
                        // LA BANDA SE RECORTA CON SU TROZO. Los puntos se
                        // guardan en tiempo de FUENTE, así que al recortar la
                        // pista unos cuantos caen fuera de la ventana que se
                        // ve — y se seguían dibujando: la línea de volumen se
                        // salía por encima de los vecinos y no se entendía
                        // nada. Ahora el tramo que cruza el borde se corta
                        // EN el borde, interpolando su altura, y los puntos de
                        // fuera no se pintan.
                        let (v0, v1) = (a.t_in, a.t_out);
                        let dentro = |t: f64| t >= v0 - 1e-9 && t <= v1 + 1e-9;
                        for w2 in a.banda.windows(2) {
                            let ((mut t0, mut g0), (mut t1, mut g1)) = (w2[0], w2[1]);
                            if t1 <= v0 || t0 >= v1 { continue; }   // entero fuera
                            let dt = (t1 - t0).max(1e-9);
                            if t0 < v0 { g0 += (g1 - g0) * (v0 - t0) / dt; t0 = v0; }
                            if t1 > v1 { g1 -= (g1 - g0) * (t1 - v1) / dt; t1 = v1; }
                            if xa(t1) < Self::ESTANTE_W { continue; }
                            trazo::linea(&mut d, xa(t0).max(Self::ESTANTE_W), ya(g0),
                                         xa(t1), ya(g1), 1.3, paleta::NARANJA, 810);
                        }
                        // y el nivel PLANO donde la banda todavía no empieza o
                        // ya se acabó, para que el trozo no aparezca sin línea
                        if let (Some(pr0), Some(ul)) = (a.banda.first(), a.banda.last()) {
                            if pr0.0 > v0 {
                                trazo::linea(&mut d, ax0.max(Self::ESTANTE_W), ya(pr0.1),
                                             xa(pr0.0), ya(pr0.1), 1.2,
                                             [0.910, 0.314, 0.102, 0.55], 811 + ia as u32);
                            }
                            if ul.0 < v1 {
                                trazo::linea(&mut d, xa(ul.0).max(Self::ESTANTE_W), ya(ul.1),
                                             (ax0 + aw).min(ancho), ya(ul.1), 1.2,
                                             [0.910, 0.314, 0.102, 0.55], 812 + ia as u32);
                            }
                        }
                        for (t, g) in &a.banda {
                            if !dentro(*t) || xa(*t) < Self::ESTANTE_W { continue; }
                            self.objetos.quad_uv_rot(xa(*t) - 6.0, ya(*g) - 6.0, 12.0, 12.0,
                                                     doodles::uv(doodles::CHINCHETA_ROJA), 0.0);
                        }
                    }
                }
                if a.fade_in > 0.01 {
                    let fw = (a.fade_in as f32 * self.pxs).max(8.0).min(aw);
                    d.rect_rec(Self::ESTANTE_W, ax0, my_y, fw, 24.0, [1.0, 1.0, 1.0, 0.3]);
                }
                if a.fade_out > 0.01 {
                    let fw = (a.fade_out as f32 * self.pxs).max(8.0).min(aw);
                    d.rect_rec(Self::ESTANTE_W, ax0 + aw - fw, my_y, fw, 24.0, [1.0, 1.0, 1.0, 0.3]);
                }
                let axr = (ax0 + 4.0).max(Self::ESTANTE_W + 4.0);
                if ax0 + aw - axr > 40.0 {
                    let n: String = a.media.chars().take(((ax0 + aw - axr) / 8.0) as usize).collect();
                    d.texto_f(ui::Familia::Mano, axr, my_y + 24.0, &n, 13.0, paleta::TINTA);
                }
            }
        }
        // ── los rótulos del margen: qué banda es qué (NORTE §3.5) ──
        d.texto_f(ui::Familia::Mano, 148.0, ty + 8.0, "vídeo", 15.0, paleta::TINTA);
        d.texto_f(ui::Familia::Mano, 76.0, ty + alto_tira - 34.0, "sonido del vídeo", 13.0,
                  paleta::TINTA_TENUE);
        d.texto_f(ui::Familia::Mano, 76.0, my_y - 2.0, "la música", 14.0, paleta::TINTA);
        // las palancas de silencio (interruptores del margen)
        for (k, (on, py)) in [(!pr.mudo_voz, ty + alto_tira - 26.0),
                              (!pr.mudo_musica, my_y + 2.0)].iter().enumerate() {
            let px2 = 208.0;
            let py = *py;
            d.rect(px2 - 3.0, py + 6.0, 16.0, 7.0, [0.35, 0.33, 0.3, 1.0]);
            let (dx2, dy2) = if *on { (10.0, -6.0) } else { (10.0, 12.0) };
            trazo::linea(&mut d, px2 + 5.0, py + 9.0, px2 + 5.0 + dx2, py + 9.0 + dy2, 2.4,
                         if *on { paleta::TINTA } else { paleta::ROJO }, 820 + k as u32);
            d.rect(px2 + 3.0 + dx2, py + 7.0 + dy2, 5.0, 5.0,
                   if *on { paleta::TINTA } else { paleta::ROJO });
        }
        // ── CADA PISTA DE SONIDO, SU TIRA DE CANAL ──────────────────────
        // El nivel va en la fila de su pista, junto a su nombre y su palanca
        // de silencio: donde uno lo busca, y sin pedir ni un píxel de más.
        let (pico_voz_m, pico_mus_m) = sonido::medidor();
        for (k, (db, mudo, pico)) in [(pr.vol_voz, pr.mudo_voz, pico_voz_m),
                                      (pr.vol_musica, pr.mudo_musica, pico_mus_m)]
            .iter().enumerate() {
            let (bx, by) = self.mando_nivel(k as u8);
            let f = ((db + 40.0) / 52.0).clamp(0.0, 1.0) as f32;
            d.rect(bx, by, Self::NIVEL_W, 4.0, [0.80, 0.78, 0.72, 1.0]);
            d.rect(bx, by, Self::NIVEL_W * f, 4.0,
                   if *mudo { paleta::TINTA_TENUE } else { paleta::ROJO });
            d.rect(bx + Self::NIVEL_W * f - 2.0, by - 4.0, 4.0, 12.0, paleta::TINTA);
            d.texto(bx + Self::NIVEL_W + 6.0, by - 3.0, &format!("{db:+.0}"), 8.0,
                    if *mudo { paleta::TINTA_TENUE } else { paleta::TINTA });
            // y lo que esa banda está midiendo, en la misma línea: un hilo
            // fino debajo del mando, para no repetir instrumento
            let p = if *mudo { 0.0 } else { pico.sqrt().clamp(0.0, 1.0) };
            d.rect(bx, by + 7.0, Self::NIVEL_W * p, 2.0,
                   if p > 0.92 { paleta::ROJO } else { [0.55, 0.53, 0.49, 0.9] });
        }

        // ── LOS INSTRUMENTOS, los cuatro en fila ────────────────────────
        // La mezcla en L y R (lo que se mira antes de entregar), las dos
        // agujas (que dicen el gesto, no el número) y la manivela.
        {
            let (bx0, by0) = self.medidor_lr_caja();
            let (pl, prr) = sonido::medidor_lr();
            let (bw, bh) = (10.0f32, 46.0);
            for (ck, (etq, v)) in [("L", pl), ("R", prr)].iter().enumerate() {
                let bx = bx0 + ck as f32 * 16.0;
                d.rect(bx, by0, bw, bh, [0.80, 0.78, 0.72, 1.0]);
                // escala de oído (raíz), no de número
                let h = bh * v.sqrt().clamp(0.0, 1.0);
                d.rect(bx, by0 + bh - h, bw, h,
                       if *v > 0.95 { paleta::ROJO } else { paleta::TINTA });
                // el testigo de −6 dB, que es donde uno apunta
                let y6 = by0 + bh - bh * 0.5f32.sqrt();
                d.rect(bx - 2.0, y6, bw + 4.0, 1.0, [0.0, 0.0, 0.0, 0.35]);
                d.texto(bx + 1.0, by0 + bh + 3.0, etq, 7.0, paleta::TINTA_TENUE);
            }
            d.texto(bx0, by0 - 11.0, "la mezcla", 7.0, paleta::TINTA_TENUE);
        }
        // las dos agujas: con el transporte parado enseñan la onda que hay
        // bajo la aguja, que es lo único que se puede decir sin mentir
        {
            let onda = pr.en(self.visor.t).and_then(|(i, src_t)| {
                let c = pr.clips.get(i)?;
                if c.hueco { return Some(0.0); }
                let picos = self.ondas.pide(&c.media, &c.ruta)?;
                let dur = filmlook_core::indice::sondea(&c.ruta).ok()?.3;
                let k = ((src_t / dur.max(0.1)) * ondas::CUBOS as f64) as usize;
                Some(picos.get(k.min(ondas::CUBOS - 1)).copied().unwrap_or(0.0))
            }).unwrap_or(0.0);
            let sonando = self.visor.tocando;
            let ay = self.agujas_y();
            for (vk, esc) in [(0usize, 1.0f32), (1, 0.85)] {
                let (vcx, vcy) = (86.0 + vk as f32 * 54.0, ay);
                let r = 18.0f32;
                let mut arco = Vec::new();
                for sk in 0..=8 {
                    let a = std::f32::consts::PI * (1.0 + sk as f32 / 8.0);
                    arco.push((vcx + a.cos() * r, vcy + a.sin() * r * 0.9));
                }
                trazo::pulso(&mut d, &arco, 1.0, paleta::TINTA_TENUE, 860 + vk as u32);
                let medido = if vk == 0 { pico_voz_m } else { pico_mus_m };
                let f = if sonando { medido.clamp(0.02, 1.0) }
                        else { (onda * esc * 0.6).clamp(0.02, 1.0) };
                let a = std::f32::consts::PI * (1.0 + f);
                trazo::linea(&mut d, vcx, vcy, vcx + a.cos() * (r - 2.0),
                             vcy + a.sin() * (r - 2.0) * 0.9, 1.5,
                             if f > 0.85 { paleta::ROJO } else { paleta::TINTA },
                             870 + vk as u32);
                d.rect(vcx - 1.5, vcy - 1.5, 3.0, 3.0, paleta::TINTA);
                d.texto(vcx - 12.0, vcy + 8.0, if vk == 0 { "voz" } else { "música" }, 7.0,
                        paleta::TINTA_TENUE);
            }
        }
        // ── la MANIVELA (NORTE §3.2): gira con la proyección, obedece la rueda ──
        {
            let (mcx, mcy) = self.manivela_centro();
            let ang = (self.visor.t * pr.fps.max(1.0) * 0.26) as f32;
            trazo::circulo(&mut d, mcx, mcy, 22.0, 22.0, 2.0, paleta::TINTA, 830);
            trazo::circulo(&mut d, mcx, mcy, 3.5, 3.5, 1.5, paleta::TINTA, 831);
            for k in 0..3 {
                let a = ang + k as f32 * std::f32::consts::TAU / 3.0;
                trazo::linea(&mut d, mcx + a.cos() * 4.0, mcy + a.sin() * 4.0,
                             mcx + a.cos() * 19.0, mcy + a.sin() * 19.0, 1.6, paleta::TINTA,
                             832 + k as u32);
            }
            let a = ang + 0.6;
            d.rect_rot(mcx + a.cos() * 26.0 - 4.0, mcy + a.sin() * 26.0 - 4.0, 8.0, 8.0, a,
                       paleta::ROJO);
            d.texto(mcx - 22.0, mcy + 30.0, "la manivela", 7.0, paleta::TINTA_TENUE);
        }

        // ── el recorte que viene del cubo: fantasma y sitio de caída ──
        if let Some((ir, px, py)) = self.cubo_pinza {
            let movido = (self.raton.0 - px).abs() > 6.0 || (self.raton.1 - py).abs() > 6.0;
            if movido {
                if let Some(c) = self.recortes.get(ir) {
                    let w = (c.dur() as f32 * self.pxs).max(8.0);
                    let gx = self.raton.0 - w / 2.0;
                    d.rect(gx, ty + 6.0, w, alto_tira - 12.0, [0.851, 0.2, 0.145, 0.30]);
                    d.rect(gx, ty + 6.0, w, 2.0, paleta::ROJO);
                    d.rect(gx, ty + alto_tira - 8.0, w, 2.0, paleta::ROJO);
                    if self.raton.0 > Self::ESTANTE_W {
                        let t = self.tiempo_en(self.raton.0);
                        let j = pr.en(t).map(|x| x.0).unwrap_or(pr.clips.len());
                        let xj = self.x_de(pr.inicios().get(j).copied()
                            .unwrap_or(pr.duracion()));
                        d.rect(xj - 1.5, ty - 14.0, 3.0, alto_tira + 22.0, paleta::ROJO);
                    }
                }
            }
        }
        // ── LA CUCHILLA PUESTA (§1.3) ───────────────────────────────────
        // La marca se dibuja como una línea de tijera a pulso sobre la tira,
        // con su tiempo en pequeño. Mover la aguja NO la mueve: esa es la
        // gracia — se puede cortar en un punto sin perder el sitio donde
        // estabas mirando.
        // ── EL CARRIL DEL PIE (subtitulo.rs) ────────────────────────────
        // Debajo de la tira, que es donde va un subtítulo: entre la imagen y
        // el sonido. Cada bloque con su texto dentro; el elegido, en rojo.
        if self.hay_pie(pr) {
            let sy = self.sub_y();
            d2.texto(Self::ESTANTE_W + 4.0, sy + 6.0, "PIE", 7.5, paleta::TINTA_TENUE);
            trazo::linea(&mut d2, Self::ESTANTE_W + 24.0, sy + Self::ALTO_SUB - 2.0,
                         ancho - 8.0, sy + Self::ALTO_SUB - 2.0, 1.0,
                         [0.2, 0.18, 0.15, 0.16], 1470);
            for (k, sb) in pr.subs.iter().enumerate() {
                let x0 = self.x_de(sb.t0).max(Self::ESTANTE_W + 24.0);
                let x1 = self.x_de(sb.t1).min(ancho);
                if x1 <= x0 { continue; }
                let elegido = self.sel_sub == Some(k);
                d2.rect(x0, sy, x1 - x0, Self::ALTO_SUB - 5.0,
                        if elegido { [0.851, 0.2, 0.145, 0.22] }
                        else { [0.169, 0.231, 0.78, 0.10] });
                trazo::caja(&mut d2, x0, sy, x1 - x0, Self::ALTO_SUB - 5.0,
                            if elegido { 1.6 } else { 1.0 },
                            if elegido { paleta::ROJO } else { [0.2, 0.18, 0.15, 0.7] },
                            1480 + k as u32);
                // el texto dentro, si cabe (y lo que se está escribiendo)
                let escribiendo = self.escribiendo_sub.as_ref()
                    .filter(|(j, _)| *j == k).map(|(_, t)| t.clone());
                let cuantos = (((x1 - x0) - 10.0) / 4.6).max(0.0) as usize;
                if cuantos > 2 {
                    let t: String = escribiendo.clone().unwrap_or_else(|| sb.texto.clone())
                        .chars().take(cuantos).collect();
                    let t = if escribiendo.is_some() { format!("{t}|") } else { t };
                    d2.texto(x0 + 5.0, sy + 3.0, &t, 8.0,
                             if elegido { paleta::ROJO } else { paleta::TINTA });
                }
            }
        }

        // ── EL CARRIL DE LA CAPA (CAPAS §7) ─────────────────────────────
        // Tiras finas encima de la tira de vídeo: se ve QUÉ hay encima y
        // CUÁNDO. La elegida, en rojo; los fundidos, como cuñas.
        {
            // LAS PISTAS DE VÍDEO (V2..V4), como en cualquier editor: carriles
            // apilados encima de la tira, cada uno con sus clips; el de
            // arriba compone sobre el de abajo. Sólo se dibujan las usadas
            // más una libre — la pista aparece cuando la necesitas.
            let visibles = self.pistas_capa_visibles(pr);
            for p in 0..visibles as u8 {
                let cy = self.capa_pista_y(p);
                // el rótulo del carril y su raya de suelo
                let rotulo = format!("V{}", p as usize + 2);
                d2.texto(Self::ESTANTE_W + 4.0, cy + 6.0, &rotulo, 7.5,
                         paleta::TINTA_TENUE);
                trazo::linea(&mut d2, Self::ESTANTE_W + 24.0, cy + Self::ALTO_CAPA - 5.0,
                             ancho - 8.0, cy + Self::ALTO_CAPA - 5.0, 1.0,
                             [0.2, 0.18, 0.15, 0.16], 1440 + p as u32);
            }
            for (k, cp) in pr.capas.iter().enumerate() {
                if cp.pista as usize >= visibles { continue }
                let cy = self.capa_pista_y(cp.pista);
                let x0 = self.x_de(cp.start).max(Self::ESTANTE_W + 24.0);
                let x1 = self.x_de(cp.fin()).min(ancho);
                if x1 <= x0 { continue }
                let elegida = self.sel_capa == Some(k);
                let fondo = if crate::foto::es_foto(&cp.c.ruta) {
                    [0.949, 0.78, 0.267, 0.85]      // ámbar: foto o rótulo
                } else {
                    [0.42, 0.55, 0.78, 0.85]        // azulado: vídeo (PiP)
                };
                d2.rect(x0, cy, x1 - x0, 18.0, fondo);
                if elegida {
                    trazo::caja(&mut d2, x0, cy, x1 - x0, 18.0, 1.8, paleta::ROJO, 1450);
                } else {
                    trazo::caja(&mut d2, x0, cy, x1 - x0, 18.0, 1.0,
                                [0.2, 0.18, 0.15, 0.8], 1450 + k as u32);
                }
                // las cuñas de los fundidos
                if cp.fundido_in > 0.01 {
                    let fw = (cp.fundido_in as f32 * self.pxs).min(x1 - x0);
                    d2.rect(x0, cy, fw, 18.0, [1.0, 1.0, 1.0, 0.25]);
                }
                if cp.fundido_out > 0.01 {
                    let fw = (cp.fundido_out as f32 * self.pxs).min(x1 - x0);
                    d2.rect(x1 - fw, cy, fw, 18.0, [1.0, 1.0, 1.0, 0.25]);
                }
                let n: String = cp.c.media.chars().take(((x1 - x0 - 8.0) / 6.0)
                                                        .max(0.0) as usize).collect();
                d2.texto(x0 + 4.0, cy + 4.0, &n, 8.0, [0.1, 0.09, 0.08, 1.0]);
            }
        }
        // LA CUCHILLA SE DIBUJA DONDE VA A MORDER. Con una música elegida
        // corta la música, pero la tijera se seguía pintando sobre la tira de
        // vídeo: marcabas en un sitio y cortaba en otro. Ver el corte antes de
        // darlo es toda la gracia de esta herramienta, así que la hoja baja al
        // carril de la pista elegida.
        if let Some(tc) = self.marca_corte {
            let xc = self.x_de(tc);
            if xc > Self::ESTANTE_W && xc < ancho {
                let (cy0, cy1) = match self.sel_audio.and_then(|ia| pr.audio.get(ia)) {
                    Some(a) => {
                        let y = self.pista_y(a.pista);
                        (y - 4.0, y + Self::ALTO_PISTA - 4.0)
                    }
                    None => (ty - 20.0, ty + alto_tira + 8.0),
                };
                trazo::linea(&mut d2, xc, cy0, xc, cy1, 1.6, paleta::ROJO, 1400);
                // las dos hojas de la tijera, sobre el carril que toque
                let hy = cy0 - 10.0;
                trazo::linea(&mut d2, xc - 7.0, hy - 10.0, xc + 3.0, hy + 4.0, 1.4,
                             paleta::ROJO, 1401);
                trazo::linea(&mut d2, xc + 7.0, hy - 10.0, xc - 3.0, hy + 4.0, 1.4,
                             paleta::ROJO, 1402);
                trazo::circulo(&mut d2, xc - 7.0, hy - 12.0, 3.5, 3.5, 1.2, paleta::ROJO, 1403);
                trazo::circulo(&mut d2, xc + 7.0, hy - 12.0, 3.5, 3.5, 1.2, paleta::ROJO, 1404);
                let que = if self.sel_audio.is_some() { "B corta la música" } else { "B corta" };
                d2.texto(xc + 10.0, cy1 + 2.0, &format!("{tc:.2} s · {que}"),
                         8.0, paleta::ROJO);
            }
        }
        // ── LA LATA QUE VIENE DE LA ESTANTERÍA (§1.1) ───────────────────
        if let Some((il, px, py)) = self.lata_pinza {
            let movido = (self.raton.0 - px).abs() > 6.0 || (self.raton.1 - py).abs() > 6.0;
            if movido {
                if let Some(c) = self.estanteria.get(il) {
                    let dur = if c.dur > 0.1 { c.dur } else { 4.0 };
                    let w = (dur as f32 * self.pxs).max(10.0).min(400.0);
                    let audio = c.fps < 0.0;
                    let (gy2, gh2) = if audio { (self.pista_y(self.pista_en(self.raton.1)
                                                              .unwrap_or(0)), 24.0) }
                                     else { (ty + 6.0, alto_tira - 12.0) };
                    let gx2 = self.raton.0 - w / 2.0;
                    d2.rect(gx2, gy2, w, gh2, [0.851, 0.2, 0.145, 0.28]);
                    d2.rect(gx2, gy2, w, 2.0, paleta::ROJO);
                    d2.rect(gx2, gy2 + gh2 - 2.0, w, 2.0, paleta::ROJO);
                    let n: String = c.nombre.chars().take(20).collect();
                    d2.texto_f(ui::Familia::Mano, gx2 + 6.0, gy2 + 2.0, &n, 14.0, paleta::ROJO);
                    if self.raton.0 > Self::ESTANTE_W && !audio {
                        let j = self.junta_en(pr, self.raton.0);
                        let xj = self.x_de(pr.inicios().get(j).copied()
                            .unwrap_or(pr.duracion()));
                        d2.rect(xj - 1.5, ty - 14.0, 3.0, alto_tira + 22.0, paleta::ROJO);
                    }
                }
            }
        }
        // ── fantasma de arrastre + línea de inserción (QoL (E)) ──
        // EL FANTASMA NO SE PINTA SI VAS A SOLTARLO FUERA. Arrastrando hacia
        // el cubo, el clip seguía dibujándose sobre la línea de tiempo con su
        // línea de inserción, como si fuera a caer ahí: dos cosas contrarias
        // a la vez. El cubo ya se abre solo, que es la señal.
        let fuera = self.en_el_cubo(self.raton.0, self.raton.1)
            || self.en_la_papelera(self.raton.0, self.raton.1);
        if let Arrastre::ClipMueve(i) = self.arrastrando {
            if let Some(c) = pr.clips.get(i).filter(|_| !fuera) {
                let w = (c.dur() as f32 * self.pxs).max(6.0);
                let gx2 = self.raton.0 - w / 2.0;
                // el fantasma sigue al ratón, translúcido
                d.rect(gx2, ty + 6.0, w, alto_tira - 12.0, [0.114, 0.106, 0.086, 0.45]);
                d.rect(gx2, ty + 6.0, w, 2.0, [0.949, 0.78, 0.267, 0.8]);
                d.rect(gx2, ty + alto_tira - 8.0, w, 2.0, [0.949, 0.78, 0.267, 0.8]);
                // la línea de inserción: la junta donde caerá
                let t = self.tiempo_en(self.raton.0);
                if let Some((j, _)) = pr.en(t) {
                    let xj = self.x_de(pr.inicios().get(j).copied().unwrap_or(0.0));
                    d.rect(xj - 1.5, ty - 14.0, 3.0, alto_tira + 22.0, paleta::NARANJA);
                }
            }
        }
        // las marcas: chinchetas clavadas en la regla (NORTE §3.5)
        for (mk, m) in pr.marcas.iter().enumerate() {
            let mx2 = self.x_de(m.t);
            if mx2 > Self::ESTANTE_W && mx2 < ancho {
                // LOS GOLPES DEL COMPÁS (♩) van a palitos de metrónomo, no a
                // chinchetas: puede haber trescientos y una pared de
                // chinchetas taparía la mesa. De lejos ni se dibujan (los
                // imanes siguen ahí); de cerca, una empalizada fina.
                if m.nota == "♩" {
                    if self.pxs > 4.0 {
                        d2.rect(mx2 - 0.7, ty - 22.0, 1.4, 12.0,
                                [0.906, 0.639, 0.129, 0.85]);
                    }
                    continue;
                }
                // EL COLOR es de la marca, no de su sitio en la lista: la
                // chincheta amarilla siempre quiere decir lo mismo (§4bis.1)
                let ch = [doodles::CHINCHETA_AMBAR, doodles::CHINCHETA_TINTA,
                          doodles::CHINCHETA_ROJA, doodles::CHINCHETA_AMBAR]
                          [(m.color % 4) as usize];
                self.objetos.quad_uv_rot(mx2 - 8.0, ty - 30.0, 16.0, 16.0, doodles::uv(ch),
                                         ((mk * 17 % 5) as f32 - 2.0) * 0.06);
                // y la NOTA, escrita al lado: para eso está la marca
                if !m.nota.is_empty() && self.pxs > 6.0 {
                    let n: String = m.nota.chars().take(18).collect();
                    d2.texto_f(ui::Familia::Mano, mx2 + 10.0, ty - 32.0, &n, 13.0,
                               paleta::TINTA);
                }
            }
        }
        // la caja de selección elástica, a lápiz
        if let (Arrastre::Caja, Some((cx0, cy0))) = (&self.arrastrando, self.caja) {
            let (mx2, my2) = self.raton;
            let (x0, y0) = (cx0.min(mx2), cy0.min(my2));
            let (w2, h2) = ((mx2 - cx0).abs(), (my2 - cy0).abs());
            d2.rect(x0, y0, w2, h2, [0.169, 0.231, 0.78, 0.10]);
            trazo::caja(&mut d2, x0, y0, w2, h2, 1.4, paleta::TINTA, 970);
        }
        // ── la barra de la bobina: dónde estás cuando la lupa no cabe ──
        let max_despl = self.desplaza_max(pr);
        if max_despl > 1.0 {
            let bx0 = Self::ESTANTE_W + 12.0;
            let bw = ancho - bx0 - 24.0;
            let by = my_y + 44.0;
            trazo::linea(&mut d, bx0, by + 4.0, bx0 + bw, by + 4.0, 1.4, paleta::TINTA_TENUE, 990);
            let total = pr.duracion().max(0.1) as f32 * self.pxs + 60.0;
            let frac_w = (bw * (bw / total)).clamp(28.0, bw);
            let frac_x = bx0 + (bw - frac_w) * (self.desplaza / max_despl);
            // el tirador: un lomo de madera con sus muescas
            d.rect_rot(frac_x, by, frac_w, 9.0, 0.0, [0.76, 0.62, 0.42, 1.0]);
            trazo::caja(&mut d, frac_x, by, frac_w, 9.0, 1.1, paleta::TINTA, 991);
            for k in 1..3 {
                d.rect(frac_x + frac_w / 2.0 - 8.0 + k as f32 * 5.0, by + 2.5, 1.5, 4.0,
                       [0.4, 0.3, 0.18, 0.8]);
            }
        }
        // al proyectar, la vista sigue a la aguja (paginado suave)
        if self.visor.tocando && self.arrastrando != Arrastre::Barra {
            let ax0 = self.x_de(self.visor.t);
            let max = self.desplaza_max(pr);
            if ax0 > ancho - 60.0 {
                self.desplaza = (self.desplaza + (ax0 - (ancho - 60.0))).clamp(0.0, max);
            } else if ax0 < Self::ESTANTE_W + 16.0 {
                self.desplaza = (self.desplaza - (Self::ESTANTE_W + 60.0 - ax0)).clamp(0.0, max);
            }
        }
        // la aguja: brazo metálico con contrapeso
        let ax = self.x_de(self.visor.t);
        if ax > Self::ESTANTE_W {
            d.rect(ax + 1.0, banco + 21.0, 2.0, self.banco_h - 20.0, [0.0, 0.0, 0.0, 0.18]);
            d.rect(ax - 0.5, banco + 20.0, 2.0, self.banco_h - 20.0, paleta::ROJO);
            d.rect_rot(ax - 7.0, banco + 14.0, 14.0, 14.0, std::f32::consts::FRAC_PI_4, paleta::ROJO);
            d.rect(ax - 3.0, banco + 34.0, 7.0, 4.0, [0.5, 0.1, 0.08, 1.0]);
        }

        } // fin de la mesa

        // el revelado en marcha: la cubeta parpadea en la cabecera
        if es_mesa && self.revelando.is_some() {
            let (pct, paso) = self.progreso.lock().unwrap().clone();
            let ancho_badge = 210.0;
            d.rect(ancho - 640.0, 10.0, ancho_badge, 30.0, [0.949, 0.78, 0.267, 0.55]);
            d.rect(ancho - 640.0, 10.0, ancho_badge * pct.clamp(0.0, 1.0), 30.0, paleta::AMBAR);
            let eta = if pct > 0.03 {
                let total = self.revelado_desde.elapsed().as_secs_f64() / pct as f64;
                let falta = (total * (1.0 - pct as f64)).max(0.0);
                if falta > 90.0 { format!(" · ~{:.0} min", falta / 60.0) }
                else { format!(" · ~{falta:.0} s") }
            } else { String::new() };
            d.texto_f(ui::Familia::Grot, ancho - 632.0, 13.0,
                      &format!("REVELANDO {:.0}%{eta}", pct * 100.0), 12.0, paleta::PELICULA);
            let paso: String = paso.chars().take(30).collect();
            d.texto(ancho - 632.0, 28.0, &format!("{paso} · clic: cancelar"), 8.0, paleta::PELICULA);
        }
        // la última revelada, colgada de la cabecera (clic: al Finder)
        if es_mesa && self.revelando.is_none() {
            if let Some(r) = &self.ultima_revelada {
                let n = r.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
                let n: String = n.chars().take(24).collect();
                d.texto(ancho - 640.0, 40.0, &format!("⤓ {n}"), 10.0, paleta::NARANJA);
            }
        }
        // ── la HOJA DE CONTACTOS: hover sostenido sobre una lata ──
        if es_mesa && self.nueva.is_none() && !self.chuleta && !self.ajustes {
            if let Some((fila, desde)) = self.hover_lata {
                if desde.elapsed().as_secs_f64() > 0.35 {
                    if let Some(c) = self.estanteria.get(fila).cloned() {
                        if c.fps >= 0.0 {
                            let y0 = {
                                let baldas = self.proyecto_baldas.clone();
                                self.estantes(&baldas).iter()
                                    .find(|(_, _, it)| matches!(it, Ok(k) if *k == fila))
                                    .map(|(_, cy, _)| cy - 48.0)
                                    .unwrap_or(Self::CABECERA + 64.0)
                            }.min(alto - 240.0);
                            let (px, pw, ph) = (Self::ESTANTE_W + 10.0, 396.0, 214.0);
                            d2.rect(px + 4.0, y0 + 6.0, pw, ph, [0.0, 0.0, 0.0, 0.2]);
                            d2.rect(px, y0, pw, ph, [0.965, 0.953, 0.918, 0.98]);
                            d2.rect(px, y0, pw, 3.0, paleta::TINTA);
                            let n: String = c.nombre.chars().take(30).collect();
                            d2.texto_f(ui::Familia::Grot, px + 12.0, y0 + 8.0, &n, 13.0, paleta::TINTA);
                            let meta = if c.fps > 0.0 {
                                format!("{}x{} · {:.2} fps · {:.0} s", c.w, c.h, c.fps, c.dur)
                            } else { format!("{:.0} s", c.dur) };
                            d2.texto(px + 12.0, y0 + 26.0, &meta, 10.0, paleta::TINTA_TENUE);
                            let proxy = pr.base.join(".proxies").join(&c.nombre);
                            let ruta = if proxy.is_file() { proxy } else { c.ruta.clone() };
                            for (k, frac) in [0.05f64, 0.2, 0.4, 0.6, 0.8, 0.95].iter().enumerate() {
                                let t = (c.dur * frac).max(0.0);
                                let clave = (format!("hoja:{}:{k}", c.nombre), (t * 100.0) as u32);
                                let (cx, cy) = (px + 12.0 + (k % 3) as f32 * 126.0,
                                                y0 + 46.0 + (k / 3) as f32 * 80.0);
                                if let Some(slot) = self.minis.pide(clave, &ruta, t) {
                                    dt2.quad(cx, cy, 120.0, 68.0, slot, 1.0);
                                } else {
                                    d2.rect(cx, cy, 120.0, 68.0, paleta::PELICULA);
                                }
                            }
                        }
                    }
                }
            }
        }
        // ── EL RESCATE TRAS UN CIERRE DE GOLPE (§4bis.6) ────────────────
        if let Some(copia) = self.rescate.clone() {
            if es_mesa {
                let (bw, bh) = (520.0f32, 62.0);
                let bx = (ancho - bw) / 2.0;
                let by = Self::CABECERA + 6.0;
                d2.rect(bx + 4.0, by + 5.0, bw, bh, [0.0, 0.0, 0.0, 0.18]);
                d2.rect(bx, by, bw, bh, [0.965, 0.953, 0.918, 1.0]);
                trazo::caja(&mut d2, bx, by, bw, bh, 1.6, paleta::ROJO, 1600);
                d2.texto_f(ui::Familia::Grot, bx + 14.0, by + 8.0,
                           "LA ÚLTIMA VEZ SE CERRÓ SIN GUARDAR DEL TODO", 11.0, paleta::ROJO);
                let n: String = copia.file_name().map(|x| x.to_string_lossy().to_string())
                    .unwrap_or_default().chars().take(46).collect();
                d2.texto(bx + 14.0, by + 26.0, &format!("hay una copia: {n}"), 9.0,
                         paleta::TINTA_TENUE);
                trazo::caja(&mut d2, bx + 14.0, by + 38.0, 92.0, 18.0, 1.2, paleta::ROJO, 1601);
                d2.texto(bx + 22.0, by + 42.0, "recuperarla", 8.5, paleta::ROJO);
                trazo::caja(&mut d2, bx + 116.0, by + 38.0, 92.0, 18.0, 1.2,
                            paleta::TINTA_TENUE, 1602);
                d2.texto(bx + 126.0, by + 42.0, "seguir así", 8.5, paleta::TINTA);
            }
        }
        // la chuleta vive en la capa superior, sobre todo lo demás
        if self.chuleta && es_mesa {
            self.dibuja_chuleta(&mut d2, ancho, alto);
        }
        if self.ajustes && es_mesa {
            self.dibuja_ajustes(pr, &mut d2, ancho, alto);
        }
        if let Some((_, texto)) = &self.notando {
            if es_mesa {
                use ui::Familia::*;
                d2.rect(0.0, 0.0, ancho, alto, [0.10, 0.10, 0.08, 0.4]);
                let (w, h) = (520.0, 150.0);
                let (x, y) = ((ancho - w) / 2.0, (alto - h) / 2.0);
                d2.rect(x + 5.0, y + 7.0, w, h, [0.0, 0.0, 0.0, 0.2]);
                d2.rect_rot(x, y, w, h, -0.006, [1.0, 0.96, 0.62, 1.0]);
                d2.texto_f(Grot, x + 24.0, y + 14.0, "LA NOTA DEL CLIP", 16.0, [0.25, 0.2, 0.05, 1.0]);
                d2.texto_f(Mano, x + 30.0, y + 52.0, &format!("{texto}▏"), 24.0, [0.2, 0.16, 0.05, 1.0]);
                d2.texto(x + 24.0, y + h - 26.0, "⏎ pega la nota · esc cierra · vacía = la quita",
                         10.0, [0.35, 0.3, 0.12, 1.0]);
            }
        }
        if let Some((viejo, nuevo)) = &self.renombrando {
            if es_mesa {
                use ui::Familia::*;
                d2.rect(0.0, 0.0, ancho, alto, [0.10, 0.10, 0.08, 0.4]);
                let (w, h) = (520.0, 150.0);
                let (x, y) = ((ancho - w) / 2.0, (alto - h) / 2.0);
                d2.rect(x, y, w, h, [0.965, 0.953, 0.918, 1.0]);
                d2.rect(x, y, w, 3.0, paleta::TINTA);
                d2.texto_f(Grot, x + 24.0, y + 14.0, "RENOMBRAR CINTA", 16.0, paleta::TINTA);
                d2.texto(x + 24.0, y + 38.0, &format!("antes: {viejo}"), 10.0, paleta::TINTA_TENUE);
                d2.rect(x + 24.0, y + 58.0, w - 48.0, 32.0, [1.0, 1.0, 1.0, 0.6]);
                d2.rect(x + 24.0, y + 88.0, w - 48.0, 2.0, paleta::TINTA);
                d2.texto_f(Mano, x + 32.0, y + 60.0, &format!("{nuevo}▏"), 22.0, paleta::TINTA);
                d2.texto(x + 24.0, y + h - 26.0, "⏎ renombra · esc cierra · el fichero no se toca",
                         10.0, paleta::TINTA_TENUE);
            }
        }
        if let Some(texto) = &self.titulando {
            if es_mesa {
                use ui::Familia::*;
                d2.rect(0.0, 0.0, ancho, alto, [0.10, 0.10, 0.08, 0.45]);
                let (w, h) = (620.0, 200.0);
                let (x, y) = ((ancho - w) / 2.0, (alto - h) / 2.0);
                d2.rect(x + 5.0, y + 7.0, w, h, [0.0, 0.0, 0.0, 0.2]);
                d2.rect(x, y, w, h, [0.965, 0.953, 0.918, 1.0]);
                d2.rect(x, y, w, 3.0, paleta::TINTA);
                d2.texto_f(Grot, x + 24.0, y + 16.0, "UN TÍTULO", 20.0, paleta::TINTA);
                d2.texto_f(Mano, x + 168.0, y + 14.0, "se quema en su propia tarjeta", 18.0, paleta::TINTA_TENUE);
                d2.rect(x + 24.0, y + 62.0, w - 48.0, 54.0, [0.06, 0.055, 0.05, 1.0]);
                d2.texto_f(Grot, x + 36.0, y + 76.0, &format!("{texto}▏"), 22.0, [0.95, 0.93, 0.89, 1.0]);
                d2.texto(x + 24.0, y + h - 32.0,
                         "⏎ a la bobina (4 s, con fundido) · esc cierra", 10.0, paleta::TINTA_TENUE);
            }
        }
        if let Some(sel) = self.revelar {
            if es_mesa {
                use ui::Familia::*;
                d2.rect(0.0, 0.0, ancho, alto, [0.10, 0.10, 0.08, 0.45]);
                let (w, h) = (520.0, 320.0);
                let (x, y) = ((ancho - w) / 2.0, (alto - h) / 2.0);
                d2.rect(x + 5.0, y + 7.0, w, h, [0.0, 0.0, 0.0, 0.2]);
                d2.rect(x, y, w, h, [0.965, 0.953, 0.918, 1.0]);
                d2.rect(x, y, w, 3.0, paleta::ROJO);
                d2.texto_f(Grot, x + 24.0, y + 18.0, "REVELAR", 22.0, paleta::TINTA);
                d2.texto_f(Mano, x + 156.0, y + 16.0, "¿para qué destino?", 20.0, paleta::TINTA_TENUE);
                for (k, (nombre, sub, _)) in PRESETS_REVELADO.iter().enumerate() {
                    let yy = y + 66.0 + k as f32 * 50.0;
                    let activo = k == sel;
                    d2.rect(x + 20.0, yy, w - 40.0, 42.0,
                            if activo { paleta::TINTA } else { [1.0, 1.0, 1.0, 0.5] });
                    d2.texto_f(Grot, x + 32.0, yy + 6.0, nombre, 13.0,
                               if activo { paleta::HUESO } else { paleta::TINTA });
                    d2.texto(x + 32.0, yy + 24.0, sub, 10.0,
                             if activo { [0.95, 0.9, 0.85, 1.0] } else { paleta::TINTA_TENUE });
                }
                d2.texto(x + 24.0, y + h - 28.0, "↑↓ elige · ⏎ revela · esc cierra",
                         10.0, paleta::TINTA_TENUE);
            }
        }

        // ── el cambio de sala: pliegue de papel / apagón del cuarto (NORTE §2) ──
        let mut d3 = ui::Dibujo::nuevo();
        // EL CAJÓN DEL MÁSTER va en el TELÓN, que es la capa que tapa de
        // verdad: en la capa de la sala se le colaban por encima las cubetas y
        // los pósters de la cuerda (su texto se pinta después de los rectos).
        if self.sala == Sala::Revelado && self.cajon_master {
            let x0 = (ancho / 2.0 - 470.0).max(50.0);
            let cy = (Self::CABECERA + 36.0 + 56.0 + 34.0 + 76.0 + 78.0).min(alto - 240.0);
            self.dibuja_cajon_master(pr, &mut d3, x0, cy);
        }
        // ── LA BARRA DE MENÚ, encima de todo ────────────────────────────
        {
            let nom = if self.sala == Sala::Portada { "el taller".to_string() }
                      else { pr.nombre.clone() };
            // CUÁNDO SE GUARDÓ, preguntándoselo al disco. Llevar la cuenta a
            // mano era mentir en cuanto alguien guardara por otro camino —y
            // hay muchos, porque cada gesto guarda—. El fichero sabe la verdad.
            let cuando = std::fs::metadata(pr.ruta_json()).ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.elapsed().ok())
                .map(|d| d.as_secs());
            let estado = match cuando {
                None => " · sin guardar todavía".to_string(),
                Some(s) if s < 3 => " · guardado".to_string(),
                Some(s) if s < 90 => format!(" · guardado hace {s} s"),
                Some(s) if s < 5400 => format!(" · guardado hace {} min", s / 60),
                Some(s) => format!(" · guardado hace {} h", s / 3600),
            };
            menu::dibuja_barra_con(&mut d3, ancho, self.menu_abierto, self.raton,
                                   &format!("{nom}{estado}"),
                                   self.sucio && cuando.map(|s| s > 2).unwrap_or(true),
                                   self.ventana.is_maximized());
            if let Some(k) = self.menu_abierto {
                // en EDITAR, el pie dice QUÉ deshace ⌘Z (§4bis.7)
                let pie = (k == 1).then(|| format!("⌘Z deshace: {}",
                    self.que_deshace().unwrap_or("nada")));
                menu::dibuja_persiana_con(&mut d3, k, self.raton, pie.as_deref());
            }
        }

        if let Some((desde, hacia, t0)) = self.transicion {
            let p = t0.elapsed().as_secs_f32() / 0.32;
            if p >= 1.0 {
                self.transicion = None;
            } else {
                let oscuro = desde == Sala::CuartoOscuro || hacia == Sala::CuartoOscuro;
                if oscuro {
                    // el apagón: la oscuridad sube desde abajo y se retira hacia
                    // abajo — el papel tiza se «despliega» debajo
                    if p < 0.5 {
                        let borde = alto * (1.0 - p * 2.0);
                        d3.rect(0.0, borde, ancho, alto - borde, [0.0, 0.0, 0.0, 0.97]);
                        d3.rect(0.0, borde - 3.0, ancho, 3.0, [1.0, 1.0, 1.0, 0.25]);
                    } else {
                        let borde = alto * (p * 2.0 - 1.0);
                        d3.rect(0.0, borde, ancho, alto - borde, [0.0, 0.0, 0.0, 0.97]);
                        d3.rect(0.0, borde - 3.0, ancho, 3.0, [1.0, 0.5, 0.3, 0.35]);
                    }
                } else {
                    // EL PLIEGUE: la hoja saliente se dobla por su centro y
                    // se retira. Dos mitades que se comprimen la una contra la
                    // otra, con la sombra del valle y el filo iluminado de la
                    // cresta — el papel doblándose de verdad, sin pases extra.
                    let hoja = if desde == Sala::Revelado { [0.940, 0.936, 0.912, 1.0] }
                               else { paleta::HUESO };
                    let e = 1.0 - (1.0 - p) * (1.0 - p);   // ease-out
                    let ancho_hoja = ancho * (1.0 - e);
                    if ancho_hoja > 2.0 {
                        let mitad = ancho_hoja / 2.0;
                        // el papel, en bandas: la sombra crece hacia el doblez
                        let bandas = 22;
                        for k in 0..bandas {
                            let f = k as f32 / (bandas - 1) as f32;    // 0 borde → 1 doblez
                            let bw = mitad / bandas as f32;
                            // sombra del valle: cuanto más doblado, más honda
                            let sombra = f.powi(3) * e * 0.55;
                            let c = [hoja[0] * (1.0 - sombra), hoja[1] * (1.0 - sombra),
                                     hoja[2] * (1.0 - sombra), 1.0];
                            d3.rect(k as f32 * bw, 0.0, bw + 0.5, alto, c);
                            // la otra mitad, espejada (la cresta mira al frente)
                            let brillo = f.powi(2) * e * 0.30;
                            let c2 = [(hoja[0] + brillo).min(1.0), (hoja[1] + brillo).min(1.0),
                                      (hoja[2] + brillo).min(1.0), 1.0];
                            d3.rect(ancho_hoja - (k + 1) as f32 * bw, 0.0, bw + 0.5, alto, c2);
                        }
                        // el filo del doblez y la sombra que la hoja arroja
                        d3.rect(mitad - 1.0, 0.0, 2.0, alto, [1.0, 1.0, 1.0, 0.5 * e]);
                        d3.rect(ancho_hoja, 0.0, 26.0, alto, [0.0, 0.0, 0.0, 0.14 * (1.0 - e)]);
                    }
                }
                self.ventana.request_redraw();
            }
        }

        // ── pintar: papel, lienzo, miniaturas, objetos, cinta, texto, vidrio y capa 2 ──
        let m0 = std::time::Instant::now();
        self.fondo.sube(&self.gpu);
        self.lienzo.sube(&self.gpu, &d);
        self.atlas.sube(&self.gpu, &dt);
        self.papel.sube(&self.gpu);
        self.objetos.sube(&self.gpu);
        self.tape.sube(&self.gpu);
        self.tipos.prepara(&self.gpu, &d);
        self.lienzo2.sube(&self.gpu, &d2);
        self.atlas.sube2(&self.gpu, &dt2);
        self.tipos2.prepara(&self.gpu, &d2);
        self.lienzo3.sube(&self.gpu, &d3);
        self.tipos3.prepara(&self.gpu, &d3);
        let m1 = std::time::Instant::now();
        let mut enc = self.gpu.encoder();
        let t = self.visor.t;
        self.visor.lupa_centro = self.lupa.unwrap_or((0.0, 0.0));
        let con_visor = matches!(self.sala, Sala::Mesa | Sala::CuartoOscuro);
        if con_visor {
            self.visor.cadena(&self.gpu, pr, &mut enc, t);
        }
        let m2 = std::time::Instant::now();
        for f in &mut self.pared { f.sube(&self.gpu); }
        let visor = &self.visor;
        let lienzo = &self.lienzo;
        let atlas = &self.atlas;
        let fondo = &self.fondo;
        let objetos = &self.objetos;
        let pared = &self.pared;
        let tape = &self.tape;
        let tipos = &self.tipos;
        let lienzo2 = &self.lienzo2;
        let tipos2 = &self.tipos2;
        let lienzo3 = &self.lienzo3;
        let tipos3 = &self.tipos3;
        let escala = self.gpu.escala;
        let (fis_w, fis_h) = (self.gpu.config.width as f32, self.gpu.config.height as f32);
        let en_hueco = es_mesa && self.fuente.is_none()
            && pr.en(self.visor.t).map(|(i, _)| pr.clips.get(i).map(|c| c.hueco).unwrap_or(false))
                .unwrap_or(false);
        let pinta_visor = con_visor && !en_hueco;
        let lupa = self.lupa;
        // ── LA PREVIEW A PANTALLA COMPLETA ────────────────────────────────
        // No se dibuja NADA del taller: ni papel, ni mesa, ni rótulos. Sólo
        // el fotograma, encajado por su proporción sobre el negro. Es una
        // rama aparte y corta a propósito: cuanto menos comparta con el
        // pintado normal, menos se puede colar algo por encima.
        if self.visor_lleno {
            let (aw, ah) = self.gpu.alto_ancho();
            let p = self.visor.proporcion().max(0.01);
            let (mut w, mut h) = (aw, aw / p);
            if h > ah { h = ah; w = ah * p; }
            let rect = [(aw - w) * 0.5, (ah - h) * 0.5, w, h];
            let visor = &self.visor;
            self.gpu.pinta_sobre(enc, wgpu::Color::BLACK, |rp| {
                visor.pinta_en(rp, escala, rect);
                rp.set_viewport(0.0, 0.0, fis_w, fis_h, 0.0, 1.0);
            });
            return;
        }
        self.gpu.pinta(enc, |rp| {
            fondo.pinta(rp);
            lienzo.pinta(rp);
            atlas.pinta(rp);
            objetos.pinta(rp);
            for f in pared { f.pinta(rp); }
            tape.pinta(rp);
            tipos.pinta(rp);
            if pinta_visor {
                visor.pinta(rp, escala);
                if let Some((lx, ly)) = lupa {
                    visor.pinta_lupa(rp, escala, lx, ly, 168.0, fis_w, fis_h);
                }
                // el visor deja puesto SU viewport: la capa 2 es a pantalla completa
                rp.set_viewport(0.0, 0.0, fis_w, fis_h, 0.0, 1.0);
            }
            lienzo2.pinta(rp);
            atlas.pinta2(rp);
            tipos2.pinta(rp);
            // el TELÓN: la transición de sala tapa hasta la última letra
            lienzo3.pinta(rp);
            tipos3.pinta(rp);
        });
        if self.dib_frames == 1 && std::env::var("FL_CRONO").is_ok() {
            eprintln!("  frame: texto {:.1} ms · cadena {:.1} ms · presenta {:.1} ms",
                      (m1 - m0).as_secs_f64() * 1e3,
                      (m2 - m1).as_secs_f64() * 1e3,
                      m2.elapsed().as_secs_f64() * 1e3);
        }
    }
}

fn main() -> Result<()> {
    let proyecto = Proyecto::cargar()?;
    prefs::carga(&proyecto.base);
    let cierre_brusco = marca_abierto(&proyecto.base);
    eprintln!("🎞  saorin-nativa v3 · bobina: {} clip(s) · {:.1} s · {} fps",
              proyecto.clips.len(), proyecto.duracion(), proyecto.fps);
    let el = EventLoop::new()?;
    el.set_control_flow(winit::event_loop::ControlFlow::Poll);
    let base = app_base(&proyecto);
    let mut app = App { estado: None, proyecto, cierre_brusco };
    el.run_app(&mut app)?;
    quita_marca(&base);
    Ok(())
}

fn app_base(p: &Proyecto) -> std::path::PathBuf { p.base.clone() }

#[cfg(test)]
mod pruebas_baldas {
    /// necesita un taller de verdad en /tmp/taller (cuatro cintas o más).
    /// Si no está —el reinicio nocturno barre /tmp— se lo salta DICIÉNDOLO,
    /// que un rojo por un fixture ausente es ruido, no información.
    #[test]
    fn estanteria_lee_baldas() {
        let cuantos = std::fs::read_dir("/tmp/taller/media")
            .map(|rd| rd.count())
            .unwrap_or(0);
        if cuantos < 4 {
            eprintln!("sin taller de pruebas en /tmp/taller/media ({cuantos} ficheros) — me lo salto");
            return;
        }
        std::env::set_var("FL_MEDIA", "/tmp/taller/media");
        let pr = crate::proyecto::Proyecto::cargar().unwrap();
        let est = pr.estanteria();
        for c in &est {
            eprintln!("cinta {} balda {:?} fps {}", c.nombre, c.balda, c.fps);
        }
        eprintln!("baldas: {:?}", pr.baldas());
        assert!(est.len() >= 4);
    }
}
