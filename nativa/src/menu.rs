//! LA BARRA DE MENÚ, y con ella el fin de la opacidad.
//!
//! El taller tenía todo escondido en atajos de teclado: para saber qué se
//! podía hacer había que abrir la chuleta, y para saber dónde vivía el
//! proyecto o cuándo se había guardado, no había forma. Un editor puede ser
//! un taller y aun así decirte lo que está pasando.
//!
//! Va dibujada con el mismo pintor que todo lo demás —no es un menú del
//! sistema— por dos razones: es igual en Mac y en Windows, y en el Mac la
//! barra ocupa el hueco de la barra de título nativa (que queda transparente
//! con los semáforos flotando encima), así que no se pierde ni un píxel.

use crate::ui::{Dibujo, Familia};
use crate::paleta;

/// lo que el menú le pide a la aplicación. La barra no HACE nada: dice qué
/// se ha pedido y la aplicación lo ejecuta, que es lo que la mantiene honesta.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Accion {
    BobinaNueva, Abrir, Guardar, GuardarComo, MostrarEnCarpeta,
    Importar, ImportarCarpeta, Revelar, DondeVa,
    Deshacer, Rehacer, Cortar, AlCubo, Duplicar, SeleccionarTodo,
    Mesa, CuartoOscuro, Revelado, PantallaCompleta, Lupa, Iman,
    Chuleta, Ajustes, Acerca,
    /// lo que faltaba en la barra y solo vivía en una tecla
    Encuadre, Congelar, Desacopla, InsertaBobina, MarcaAqui, MarcasCompas, RangoEntrada, RangoSalida, RangoQuitar, Bucle,
    VentanaAjustes, VentanaChuleta, VentanaVigia, VentanaBobinas,
}

pub struct Entrada {
    pub texto: &'static str,
    pub atajo: &'static str,
    pub accion: Option<Accion>,   // None = separador
}

const fn e(texto: &'static str, atajo: &'static str, accion: Accion) -> Entrada {
    Entrada { texto, atajo, accion: Some(accion) }
}
const SEP: Entrada = Entrada { texto: "", atajo: "", accion: None };

pub struct Persiana {
    pub titulo: &'static str,
    pub entradas: &'static [Entrada],
}

pub const MENUS: &[Persiana] = &[
    Persiana { titulo: "Bobina", entradas: &[
        e("Bobina nueva…",        "⌘N", Accion::BobinaNueva),
        e("Abrir…",               "⌘O", Accion::Abrir),
        SEP,
        e("Guardar",              "⌘S", Accion::Guardar),
        e("Guardar como…",        "⇧⌘S", Accion::GuardarComo),
        e("Mostrar en la carpeta", "",  Accion::MostrarEnCarpeta),
        SEP,
        e("Importar material…",   "I",  Accion::Importar),
        e("Importar una carpeta…", "",  Accion::ImportarCarpeta),
        e("Insertar otra bobina…", "",  Accion::InsertaBobina),
        SEP,
        e("Revelar la bobina",    "⌘R", Accion::Revelar),
        e("Dónde va el máster…",  "",   Accion::DondeVa),
    ]},
    Persiana { titulo: "Editar", entradas: &[
        e("Deshacer",             "⌘Z", Accion::Deshacer),
        e("Rehacer",              "⇧⌘Z", Accion::Rehacer),
        SEP,
        e("La cuchilla",          "B",  Accion::Cortar),
        e("Al cubo de recortes",  "⌫",  Accion::AlCubo),
        e("Duplicar el clip",     "⌘D", Accion::Duplicar),
        e("Seleccionar todo",     "⌘A", Accion::SeleccionarTodo),
        SEP,
        e("El encuadre",          "E",  Accion::Encuadre),
        e("Congelar el fotograma", "F", Accion::Congelar),
        e("Desacoplar el sonido", "⇧D", Accion::Desacopla),
        e("Marca en la aguja",    "M",  Accion::MarcaAqui),
        e("Marcas al compás de la música", "", Accion::MarcasCompas),
    ]},
    Persiana { titulo: "Rango", entradas: &[
        e("Entrada aquí",         "⇧I", Accion::RangoEntrada),
        e("Salida aquí",          "⇧O", Accion::RangoSalida),
        e("En bucle",             "O",  Accion::Bucle),
        e("Quitar el rango",      "U",  Accion::RangoQuitar),
    ]},
    Persiana { titulo: "Salas", entradas: &[
        e("La mesa",              "1",  Accion::Mesa),
        e("El cuarto oscuro",     "2",  Accion::CuartoOscuro),
        e("El revelado",          "3",  Accion::Revelado),
    ]},
    Persiana { titulo: "Ver", entradas: &[
        e("Pantalla completa",    "⌃F", Accion::PantallaCompleta),
        e("La lupa cuentahílos",  "⌥",  Accion::Lupa),
        e("El imán de la bobina", "⌘I", Accion::Iman),
        SEP,
        e("La chuleta",           "?",  Accion::Chuleta),
    ]},
    Persiana { titulo: "Ventanas", entradas: &[
        e("Ajustes, aparte",      "",   Accion::VentanaAjustes),
        e("La chuleta, aparte",   "",   Accion::VentanaChuleta),
        e("El vigía (visor suelto)", "", Accion::VentanaVigia),
        e("Las bobinas, aparte",  "",   Accion::VentanaBobinas),
    ]},
    Persiana { titulo: "Taller", entradas: &[
        e("Ajustes…",             "⌘,", Accion::Ajustes),
        e("Sobre el taller",      "",   Accion::Acerca),
    ]},
];

pub const ALTO: f32 = 30.0;
/// en el Mac la barra empieza pasados los semáforos
pub fn sangria() -> f32 { if cfg!(target_os = "macos") && !hay_mandos() { 86.0 } else { 12.0 } }

// ── LOS MANDOS DE LA VENTANA (solo Windows) ──────────────────────────────
//
// En el Mac la barra de título se vuelve transparente y los semáforos flotan
// sobre el papel: la ventana es del taller y se nota. En Windows no hay nada
// equivalente — o hay marco del sistema (una franja blanca con el icono por
// defecto a la izquierda y los botones a la derecha, que rompe el papel por la
// mitad) o no hay marco ninguno.
//
// Así que no hay marco, y los tres mandos los dibuja el taller con su misma
// tinta, como todo lo demás. Nada de glifos del sistema: una raya, un cuadrado
// y un aspa a pulso.
pub const MANDO_W: f32 = 44.0;

/// ¿dibuja el taller su propio marco? En Windows siempre. En el Mac, con
/// `FL_MARCO=propio`, para poder **ver** cómo queda sin cambiar de máquina:
/// la regla de la casa es verlo funcionando, y el marco de Windows no se
/// puede mirar desde aquí de ninguna otra manera.
pub fn hay_mandos() -> bool {
    static Q: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *Q.get_or_init(|| cfg!(target_os = "windows")
        || std::env::var("FL_MARCO").as_deref() == Ok("propio"))
}

/// (x, ancho) de cada mando: 0 minimizar · 1 maximizar · 2 cerrar
pub fn mandos(ancho: f32) -> [(f32, f32); 3] {
    let x0 = ancho - MANDO_W * 3.0;
    [(x0, MANDO_W), (x0 + MANDO_W, MANDO_W), (x0 + MANDO_W * 2.0, MANDO_W)]
}

/// ¿sobre qué mando de ventana está el ratón?
pub fn mando_en(ancho: f32, mx: f32, my: f32) -> Option<usize> {
    if !hay_mandos() || my < 0.0 || my > ALTO { return None; }
    mandos(ancho).iter().position(|(x, w)| mx >= *x && mx < x + w)
}

/// los tres mandos, dibujados a mano
fn dibuja_mandos(d: &mut Dibujo, ancho: f32, raton: (f32, f32), maximizada: bool) {
    if !hay_mandos() { return; }
    let sobre = mando_en(ancho, raton.0, raton.1);
    for (k, (x, w)) in mandos(ancho).iter().enumerate() {
        let dentro = sobre == Some(k);
        if dentro {
            // el de cerrar se enciende en rojo; los otros, en tinta clara
            d.rect(*x, 0.0, *w, ALTO,
                   if k == 2 { [0.851, 0.2, 0.145, 0.9] } else { [1.0, 1.0, 1.0, 0.12] });
        }
        let c = if dentro && k == 2 { paleta::HUESO } else { [0.86, 0.84, 0.78, 1.0] };
        let (cx, cy) = (x + w / 2.0, ALTO / 2.0);
        match k {
            // ── minimizar: una raya
            0 => crate::trazo::linea(d, cx - 5.0, cy + 3.0, cx + 5.0, cy + 3.0, 1.3, c, 1700),
            // ── maximizar / restaurar: un cuadro (dos, si ya está maximizada)
            1 => {
                if maximizada {
                    crate::trazo::caja(d, cx - 6.0, cy - 2.0, 9.0, 9.0, 1.2, c, 1701);
                    crate::trazo::caja(d, cx - 3.0, cy - 5.0, 9.0, 9.0, 1.2, c, 1702);
                } else {
                    crate::trazo::caja(d, cx - 5.0, cy - 5.0, 10.0, 10.0, 1.3, c, 1703);
                }
            }
            // ── cerrar: un aspa
            _ => {
                crate::trazo::linea(d, cx - 5.0, cy - 5.0, cx + 5.0, cy + 5.0, 1.4, c, 1704);
                crate::trazo::linea(d, cx + 5.0, cy - 5.0, cx - 5.0, cy + 5.0, 1.4, c, 1705);
            }
        }
    }
}

/// dónde empieza cada título y cuánto ocupa
pub fn tiras() -> Vec<(f32, f32)> {
    let mut v = Vec::new();
    let mut x = sangria();
    for m in MENUS {
        let w = m.titulo.chars().count() as f32 * 7.2 + 22.0;
        v.push((x, w));
        x += w;
    }
    v
}

/// ¿sobre qué persiana está el ratón?
pub fn persiana_en(mx: f32, my: f32) -> Option<usize> {
    if my < 0.0 || my > ALTO { return None; }
    tiras().iter().position(|(x, w)| mx >= *x && mx < x + w)
}

/// ¿sobre qué entrada de la persiana abierta?
pub fn entrada_en(abierta: usize, mx: f32, my: f32) -> Option<usize> {
    let (x, _) = *tiras().get(abierta)?;
    let m = MENUS.get(abierta)?;
    let ancho = ancho_persiana(m);
    if mx < x || mx > x + ancho { return None; }
    let mut y = ALTO + 4.0;
    for (i, en) in m.entradas.iter().enumerate() {
        let h = if en.accion.is_none() { 7.0 } else { 22.0 };
        if my >= y && my < y + h && en.accion.is_some() { return Some(i); }
        y += h;
    }
    None
}

fn ancho_persiana(m: &Persiana) -> f32 {
    let mut w: f32 = 150.0;
    for en in m.entradas {
        let n = en.texto.chars().count() as f32 * 7.0 + en.atajo.chars().count() as f32 * 8.0 + 56.0;
        if n > w { w = n; }
    }
    w
}

/// La barra. `titulo` es lo que se enseña en el centro (la bobina abierta y
/// si tiene cambios sin guardar) — que es media transparencia del proyecto.
pub fn dibuja_barra(d: &mut Dibujo, ancho: f32, abierta: Option<usize>,
                    raton: (f32, f32), titulo: &str, sucio: bool) {
    dibuja_barra_con(d, ancho, abierta, raton, titulo, sucio, false)
}

/// la barra, sabiendo si la ventana está maximizada (para dibujar el mando de
/// restaurar en vez del de maximizar)
pub fn dibuja_barra_con(d: &mut Dibujo, ancho: f32, abierta: Option<usize>,
                        raton: (f32, f32), titulo: &str, sucio: bool,
                        maximizada: bool) {
    d.rect(0.0, 0.0, ancho, ALTO, [0.106, 0.10, 0.086, 1.0]);
    d.rect(0.0, ALTO - 1.0, ancho, 1.0, [0.0, 0.0, 0.0, 0.35]);
    dibuja_mandos(d, ancho, raton, maximizada);
    for (i, (x, w)) in tiras().iter().enumerate() {
        let sobre = abierta == Some(i)
            || (abierta.is_none() && raton.1 < ALTO && raton.0 >= *x && raton.0 < x + w);
        if sobre { d.rect(*x, 2.0, *w, ALTO - 5.0, [1.0, 1.0, 1.0, 0.10]); }
        d.texto_f(Familia::Grot, x + 11.0, 9.0, MENUS[i].titulo, 10.5,
                  if sobre { paleta::HUESO } else { [0.80, 0.78, 0.72, 1.0] });
    }
    // EL TÍTULO, en el centro: qué bobina y si está guardada. Se centra en el
    // hueco QUE QUEDA, no en la ventana entera: con los mandos a la derecha,
    // centrarlo a ojo lo metía debajo del aspa de cerrar.
    let libre = ancho - if hay_mandos() { MANDO_W * 3.0 } else { 0.0 };
    let tope = ((libre - sangria() - 240.0) / 6.0).max(8.0) as usize;
    let titulo: String = if titulo.chars().count() > tope {
        titulo.chars().take(tope).collect()
    } else { titulo.to_string() };
    let cx = libre * 0.5 - titulo.chars().count() as f32 * 3.0;
    d.texto_f(Familia::Mano, cx, 6.0, &titulo, 15.0, [0.86, 0.84, 0.78, 1.0]);
    if sucio {
        d.texto(cx - 16.0, 10.0, "●", 10.0, paleta::NARANJA);
    }
}

/// la persiana desplegada
/// LA CABECERA DE UNA VENTANA SECUNDARIA (solo donde no hay marco del
/// sistema): el título a la izquierda, el aspa de cerrar a la derecha, y el
/// resto de la franja para arrastrarla. Las ventanas del taller no llevan la
/// barra blanca de Windows ni cuando son pequeñas.
pub fn dibuja_cabecera_cristal(d: &mut Dibujo, ancho: f32, titulo: &str,
                               raton: (f32, f32)) {
    if !hay_mandos() { return; }
    d.rect(0.0, 0.0, ancho, ALTO, [0.106, 0.10, 0.086, 1.0]);
    d.rect(0.0, ALTO - 1.0, ancho, 1.0, [0.0, 0.0, 0.0, 0.35]);
    let t: String = titulo.chars().take(((ancho - MANDO_W - 24.0) / 6.0).max(4.0) as usize)
        .collect();
    d.texto_f(Familia::Grot, 12.0, 9.0, &t, 10.5, [0.80, 0.78, 0.72, 1.0]);
    let (x, w) = (ancho - MANDO_W, MANDO_W);
    let dentro = raton.1 >= 0.0 && raton.1 <= ALTO && raton.0 >= x;
    if dentro { d.rect(x, 0.0, w, ALTO, [0.851, 0.2, 0.145, 0.9]); }
    let c = if dentro { paleta::HUESO } else { [0.86, 0.84, 0.78, 1.0] };
    let (cx, cy) = (x + w / 2.0, ALTO / 2.0);
    crate::trazo::linea(d, cx - 5.0, cy - 5.0, cx + 5.0, cy + 5.0, 1.4, c, 1710);
    crate::trazo::linea(d, cx + 5.0, cy - 5.0, cx - 5.0, cy + 5.0, 1.4, c, 1711);
}

/// ¿el aspa de cerrar de una ventana secundaria?
pub fn cierra_cristal_en(ancho: f32, mx: f32, my: f32) -> bool {
    hay_mandos() && my >= 0.0 && my <= ALTO && mx >= ancho - MANDO_W
}

/// cuánto baja el contenido de una ventana secundaria por su cabecera
pub fn cabecera_cristal() -> f32 { if hay_mandos() { ALTO } else { 0.0 } }

/// la persiana desplegada. `pie` es el rótulo del final (en Editar dice QUÉ
/// deshace ⌘Z: hasta ahora era a ciegas, §4bis.7).
pub fn dibuja_persiana_con(d: &mut Dibujo, abierta: usize, raton: (f32, f32),
                           pie: Option<&str>) {
    dibuja_persiana(d, abierta, raton);
    let (Some(pie), Some(&(x, _))) = (pie, tiras().get(abierta)) else { return };
    let Some(m) = MENUS.get(abierta) else { return };
    let w = ancho_persiana(m);
    let alto: f32 = m.entradas.iter()
        .map(|en| if en.accion.is_none() { 7.0 } else { 22.0 }).sum::<f32>() + 10.0;
    let y = ALTO + 2.0 + alto;
    d.rect(x, y, w, 20.0, [0.118, 0.112, 0.096, 1.0]);
    d.rect(x, y, w, 1.0, [1.0, 1.0, 1.0, 0.10]);
    let t: String = pie.chars().take(((w - 24.0) / 5.4) as usize).collect();
    d.texto(x + 14.0, y + 5.0, &t, 8.5, [0.70, 0.68, 0.62, 1.0]);
}

pub fn dibuja_persiana(d: &mut Dibujo, abierta: usize, raton: (f32, f32)) {
    let Some(&(x, _)) = tiras().get(abierta) else { return };
    let Some(m) = MENUS.get(abierta) else { return };
    let w = ancho_persiana(m);
    let alto: f32 = m.entradas.iter()
        .map(|en| if en.accion.is_none() { 7.0 } else { 22.0 }).sum::<f32>() + 10.0;
    d.rect(x + 3.0, ALTO + 7.0, w, alto, [0.0, 0.0, 0.0, 0.30]);       // sombra
    d.rect(x, ALTO + 2.0, w, alto, [0.118, 0.112, 0.096, 1.0]);
    d.rect(x, ALTO + 2.0, w, 1.0, [1.0, 1.0, 1.0, 0.10]);
    let mut y = ALTO + 4.0;
    for (i, en) in m.entradas.iter().enumerate() {
        if en.accion.is_none() {
            d.rect(x + 10.0, y + 3.0, w - 20.0, 1.0, [1.0, 1.0, 1.0, 0.12]);
            y += 7.0;
            continue;
        }
        let sobre = entrada_en(abierta, raton.0, raton.1) == Some(i);
        if sobre { d.rect(x + 3.0, y, w - 6.0, 22.0, [0.851, 0.2, 0.145, 0.75]); }
        d.texto(x + 14.0, y + 6.0, en.texto, 9.5,
                if sobre { paleta::HUESO } else { [0.86, 0.84, 0.78, 1.0] });
        if !en.atajo.is_empty() {
            d.texto(x + w - 16.0 - en.atajo.chars().count() as f32 * 7.0, y + 6.0,
                    en.atajo, 9.0,
                    if sobre { [0.95, 0.9, 0.85, 1.0] } else { [0.55, 0.53, 0.48, 1.0] });
        }
        y += 22.0;
    }
}
