//! EL PLAN DE BOBINA COMPILADO (MOTOR §5 y §5bis).
//!
//! La bobina que llega del taller es una lista de clips con sus tiempos. Aquí
//! se convierte, **una vez y antes de arrancar**, en una tabla plana con un
//! renglón por fotograma de salida. A partir de ahí el motor no decide nada
//! por fotograma: lee el renglón que toca y ejecuta.
//!
//! Lo que ese cambio hace desaparecer:
//!
//! - **El corte.** Un corte seco es que el renglón `t` mire a una fuente y el
//!   `t+1` a otra. Ni pase extra, ni fichero intermedio, ni re-codificación.
//!   La bobina no se pega nunca porque no se despedaza nunca.
//! - **La fase de fundidos.** Un encadenado es `mix(a, b, peso)` metido en el
//!   pase del revelado. Antes costaba una pasada entera del máster (`xfade`
//!   sobre todas las piezas).
//! - **Los fundidos a negro y a blanco**, que ni siquiera necesitan una
//!   segunda fuente: son una mezcla contra una constante.

use serde_json::Value;

/// no hay segunda fuente en este renglón
pub const NINGUNA: u32 = u32::MAX;

/// cuántas capas pueden CONVIVIR en un fotograma. Coincide con el máximo de
/// pistas de vídeo de la mesa: con las ocho ocupadas a la vez, se dibujan
/// las ocho.
pub const MAX_CAPAS: usize = 8;

/// una capa en un renglón: qué fuente, en qué segundo y con cuánto alfa
#[derive(Clone, Copy, Debug)]
pub struct CapaR {
    pub fuente: u32,
    pub t: f64,
    pub alfa: f32,
}

impl CapaR {
    pub const VACIA: CapaR = CapaR { fuente: NINGUNA, t: 0.0, alfa: 0.0 };
}

/// CÓMO ENCAJA el material en el lienzo del proyecto
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Encaje {
    /// letterbox: cabe entero, con bandas
    Dentro,
    /// llena el lienzo recortando lo que sobra
    Llena,
    /// estira cada eje por su cuenta (deforma a propósito)
    Estira,
}

impl Encaje {
    pub fn clave(self) -> &'static str {
        match self { Encaje::Dentro => "fit", Encaje::Llena => "fill", Encaje::Estira => "stretch" }
    }
    pub fn de(s: &str) -> Encaje {
        match s { "fill" => Encaje::Llena, "stretch" => Encaje::Estira, _ => Encaje::Dentro }
    }
    pub fn rotulo(self) -> &'static str {
        match self { Encaje::Dentro => "dentro", Encaje::Llena => "llena", Encaje::Estira => "estira" }
    }
}

/// EL ENCUADRE DE UN CLIP, y **uno solo**.
///
/// Antes había dos modelos: la aplicación guardaba `zoom` + centro y el
/// revelado quería escala + giro + desplazamiento, y se traducían al vuelo al
/// mandar el trabajo. Esa traducción ya costó un fallo entero —el encuadre no
/// salía en el máster— y la única cura es que no exista: este es el modelo, lo
/// guarda el proyecto tal cual y lo leen el visor y el motor sin tocarlo
/// (PENDIENTE §1.5).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Encuadre {
    /// LA ORIENTACIÓN, en cuartos de vuelta a derechas (0..3). Va aparte del
    /// giro fino a propósito: girar 90° **cambia la forma del clip** (un 16:9
    /// pasa a ser 9:16), así que hay que intercambiar ancho y alto ANTES de
    /// conformarlo al lienzo. Metido en el ángulo libre saldría encajado como
    /// si siguiera siendo apaisado.
    pub cuartos: u8,
    /// separada en X e Y (permite estirar a propósito)
    pub escala: (f32, f32),
    /// desplazamiento, en fracción del lienzo
    pub pos: (f32, f32),
    /// grados: la inclinación FINA, sobre la orientación
    pub giro: f32,
    /// el punto sobre el que gira y escala (0.5, 0.5 = el centro)
    pub ancla: (f32, f32),
    pub voltea: (bool, bool),
    pub encaje: Encaje,
}

impl Default for Encuadre {
    fn default() -> Self { Encuadre::limpio(0) }
}

impl Encuadre {
    /// el encuadre natural de un material que viene girado `cuartos` en su
    /// contenedor: la orientación del fichero y la que ponga el autor son el
    /// MISMO campo, así que «encuadre a cero» devuelve el vídeo derecho, no
    /// tumbado
    pub const fn limpio(cuartos: u8) -> Encuadre {
        Encuadre {
            cuartos, escala: (1.0, 1.0), pos: (0.0, 0.0), giro: 0.0,
            ancla: (0.5, 0.5), voltea: (false, false), encaje: Encaje::Dentro,
        }
    }

    /// ¿está el clip tal y como salió de la cámara?
    pub fn es_limpio(&self, cuartos_fichero: u8) -> bool {
        *self == Encuadre::limpio(cuartos_fichero)
    }

    /// el JSON del proyecto (y el del payload de revelado: son el mismo)
    pub fn json(&self) -> Value {
        serde_json::json!({
            "cuartos": self.cuartos,
            "escala": [self.escala.0, self.escala.1],
            "pos": [self.pos.0, self.pos.1],
            "rot": self.giro,
            "ancla": [self.ancla.0, self.ancla.1],
            "voltea": [self.voltea.0, self.voltea.1],
            "fit": self.encaje.clave(),
        })
    }

    /// Lee el `tf` del JSON. Entiende las DOS formas: la de hoy y la vieja
    /// (`{scale, rot, x, y, fit}`, con la escala en un solo número), porque
    /// hay bobinas guardadas con ella y una bobina no se rompe por cambiar de
    /// modelo.
    pub fn de_json(tf: &Value, cuartos_fichero: u8) -> Encuadre {
        let mut e = Encuadre::limpio(cuartos_fichero);
        if !tf.is_object() { return e; }
        let par = |v: &Value, d: (f32, f32)| -> (f32, f32) {
            match v.as_array() {
                Some(a) if a.len() == 2 => (a[0].as_f64().unwrap_or(d.0 as f64) as f32,
                                            a[1].as_f64().unwrap_or(d.1 as f64) as f32),
                _ => d,
            }
        };
        if let Some(q) = tf["cuartos"].as_u64() { e.cuartos = (q % 4) as u8; }
        e.escala = match tf["escala"].as_array() {
            Some(_) => par(&tf["escala"], (1.0, 1.0)),
            None => {
                let s = tf["scale"].as_f64().unwrap_or(1.0) as f32;
                (s, s)
            }
        };
        e.pos = match tf["pos"].as_array() {
            Some(_) => par(&tf["pos"], (0.0, 0.0)),
            None => (tf["x"].as_f64().unwrap_or(0.0) as f32,
                     tf["y"].as_f64().unwrap_or(0.0) as f32),
        };
        e.giro = tf["rot"].as_f64().unwrap_or(0.0) as f32;
        e.ancla = par(&tf["ancla"], (0.5, 0.5));
        e.voltea = match tf["voltea"].as_array() {
            Some(a) if a.len() == 2 => (a[0].as_bool().unwrap_or(false),
                                        a[1].as_bool().unwrap_or(false)),
            _ => (false, false),
        };
        e.encaje = Encaje::de(tf["fit"].as_str().unwrap_or("fit"));
        e.escala = (e.escala.0.clamp(0.02, 20.0), e.escala.1.clamp(0.02, 20.0));
        e.pos = (e.pos.0.clamp(-4.0, 4.0), e.pos.1.clamp(-4.0, 4.0));
        e.ancla = (e.ancla.0.clamp(-1.0, 2.0), e.ancla.1.clamp(-1.0, 2.0));
        e
    }
}

/// ¿ES UNA FOTO (o un rótulo, que es un PNG)? Vive aquí, y no en `foto.rs`,
/// porque `plan.rs` se incluye POR RUTA en el shell y en el motor del Mac —
/// que no arrastran el crate entero— y todos tienen que decidirlo igual.
pub fn es_foto(ruta: &std::path::Path) -> bool {
    matches!(ruta.extension().and_then(|e| e.to_str())
                 .map(|e| e.to_lowercase()).as_deref(),
             Some("jpg") | Some("jpeg") | Some("png") | Some("webp") | Some("bmp"))
}

/// una fuente del plan: un fichero con su receta
#[derive(Clone)]
pub struct Fuente {
    pub fichero: String,
    /// negro puro (el hueco de la bobina): no se abre ni se decodifica
    pub hueco: bool,
    pub prefs: Value,
    pub lut_in: Option<String>,
    pub lut: Option<String>,
    /// el encuadre del clip (el único modelo que hay)
    pub enc: Encuadre,
    /// velocidad de reproducción
    pub veloc: f64,
    /// UNA FOTO O UN RÓTULO: no hay nada que decodificar, se sube una vez y
    /// se queda residente en la GPU (§4bis.10)
    pub foto: bool,
    /// ES UNA CAPA (CAPAS §1): se compone ENCIMA del fotograma en vez de
    /// serlo. Cambia la semántica del shader: fuera de su encuadre queda
    /// transparente —no negro— y si es RGBA su alfa viaja por píxel.
    pub capa: bool,
    /// MATRIZ EXPLÍCITA (CAPAS §8): el aplanado de bobinas anidadas compone
    /// las afines de fuera y de dentro en una sola; cuando viene, manda
    /// sobre el encuadre. Es lo que permite que el motor no sepa que hubo
    /// anidamiento.
    pub mat: Option<[f32; 6]>,
}

/// UN FOTOGRAMA DE SALIDA
#[derive(Clone, Copy, Debug)]
pub struct Renglon {
    pub fuente_a: u32,
    /// el otro lado del fundido; `NINGUNA` si no hay
    pub fuente_b: u32,
    /// cuánto pesa el segundo lado: 0 = solo A, 1 = solo B
    pub peso_b: f32,
    /// tiempo dentro de la fuente A (segundos)
    pub t_a: f64,
    pub t_b: f64,
    /// fundido contra un color plano (negro=0, blanco=1); `nivel` es cuánto
    pub color_fijo: f32,
    pub nivel_color: f32,
    /// LAS CAPAS de este fotograma (CAPAS §3): dibujos de más, encima de A y
    /// B, DE ABAJO ARRIBA. Hasta `MAX_CAPAS` a la vez — que con ocho pistas
    /// de vídeo son TODAS: no hay ninguna que se quede sin dibujar. El alfa
    /// es el global de cada capa (sus fundidos); el alfa por píxel, si es
    /// RGBA, va aparte y lo multiplica el shader.
    pub capas: [CapaR; MAX_CAPAS],
    /// AQUÍ EMPIEZA UN PLANO NUEVO. Un corte seco no tiene continuidad de luz:
    /// el arrastre del obturador **no cruza el empalme**. Sin esto, el primer
    /// fotograma del clip entrante llevaba encima un 14 % del clip que se iba
    /// (el valor de la casa) y se veía el plano anterior uno o dos fotogramas
    /// después de haber cortado. En un encadenado NO se marca: ahí las dos
    /// imágenes conviven de verdad y el arrastre es legítimo.
    pub corte: bool,
}

/// UN TRAMO de la bobina y de quién depende. Es la unidad de la caché fina:
/// si el autor toca el grade de un clip, solo se recalculan los tramos que
/// miran a ese clip (MOTOR §7).
#[derive(Clone, Debug)]
pub struct Tramo {
    /// primer renglón y cuántos
    pub desde: usize,
    pub cuantos: usize,
    /// las fuentes de las que depende (una, o dos si es una junta)
    pub fuentes: Vec<u32>,
}

pub struct Plan {
    pub w: u32,
    pub h: u32,
    pub fps: f64,
    pub fuentes: Vec<Fuente>,
    pub renglones: Vec<Renglon>,
    pub salida: String,
    pub codec: String,
    pub bitrate: i64,
}

/// Compila la bobina del taller a la tabla. `clips` son los de siempre:
/// `{file, in, out, fade, fadeIn, fadeOut, gap, speed, tf, prefs, lut, lut_in}`.
pub fn compila(payload: &Value) -> Result<Plan, String> {
    let w = payload["project"]["w"].as_u64().unwrap_or(1920) as u32 & !1;
    let h = payload["project"]["h"].as_u64().unwrap_or(1080) as u64 as u32 & !1;
    let fps = payload["project"]["fps"].as_f64().unwrap_or(25.0).max(1.0);
    let clips = payload["clips"].as_array().cloned().unwrap_or_default();
    if clips.is_empty() { return Err("la bobina está vacía".into()); }

    let prefs_com = payload.get("prefs").cloned().unwrap_or(Value::Null);
    let mut fuentes = Vec::new();
    // (fuente, entrada, salida, número de fotogramas, fundido de entrada en
    // fotogramas, fundido de salida en fotogramas)
    struct Tr { src: usize, t_in: f64, t_out: f64, n: usize, fi: usize, fo: usize }
    let mut tramos: Vec<Tr> = Vec::new();

    /// EL TIEMPO DE FUENTE de un fotograma del tramo. Aquí viven las tres
    /// marchas del gramófono (PENDIENTE §4bis.3):
    ///   · velocidad normal → se avanza desde la entrada;
    ///   · MARCHA ATRÁS (velocidad negativa) → se retrocede desde la salida;
    ///   · CONGELADO (velocidad 0) → siempre el mismo fotograma, el de la
    ///     entrada, y la duración del clip es la del tramo en la bobina.
    fn t_fuente(t_in: f64, t_out: f64, i: usize, fps: f64, veloc: f64) -> f64 {
        if veloc.abs() < 0.02 { return t_in; }
        let d = i as f64 / fps * veloc.abs();
        if veloc < 0.0 { (t_out - d).max(t_in) } else { t_in + d }
    }

    for c in &clips {
        let hueco = c["gap"].as_bool().unwrap_or(false);
        let veloc = c["speed"].as_f64().unwrap_or(1.0).clamp(-8.0, 8.0);
        let veloc = if veloc.abs() < 0.02 { 0.0 } else { veloc };
        fuentes.push(Fuente {
            fichero: c["file"].as_str().unwrap_or("").to_string(),
            hueco,
            // la receta del clip solo si TIENE receta: un `{}` vacío es un
            // clip que nunca pasó por el cuarto oscuro, y darle los valores
            // por defecto sería revelarlo sin look ninguno
            prefs: match c["prefs"].as_object() {
                Some(o) if !o.is_empty() => c["prefs"].clone(),
                _ => prefs_com.clone(),
            },
            lut_in: c["lut_in"].as_str().or(payload["lut_in"].as_str()).map(String::from),
            lut: c["lut"].as_str().or(payload["lut"].as_str()).map(String::from),
            // el encuadre viene tal cual del proyecto: nada que traducir
            enc: Encuadre::de_json(&c["tf"], c["cuartos"].as_u64().unwrap_or(0) as u8),
            veloc,
            foto: !hueco && es_foto(
                std::path::Path::new(c["file"].as_str().unwrap_or(""))),
            capa: false,
            mat: mat_de_json(&c["mat"]),
        });
        let t_in = c["in"].as_f64().unwrap_or(0.0);
        let t_out = c["out"].as_f64().unwrap_or(0.0).max(t_in);
        // LA REJILLA ES LA DEL PROYECTO: la duración del tramo se redondea a
        // fotogramas enteros del máster. Así no hay deriva acumulada, que era
        // justo el motivo por el que existía el corte con re-codificación.
        // Congelado (velocidad 0): el tramo dura lo que diga la bobina.
        let dur = if veloc.abs() < 0.02 { t_out - t_in } else { (t_out - t_in) / veloc.abs() };
        let n = (dur * fps).round().max(1.0) as usize;
        let fi = (c["fadeIn"].as_f64().unwrap_or(0.0) * fps).round() as usize;
        let fo = (c["fadeOut"].as_f64().unwrap_or(0.0) * fps).round() as usize;
        tramos.push(Tr { src: fuentes.len() - 1, t_in, t_out, n, fi, fo });
    }

    // El encadenado con el siguiente lo pide el clip que se va (`fade`).
    let solapes: Vec<usize> = clips.iter().enumerate().map(|(k, c)| {
        if k + 1 >= tramos.len() { return 0; }
        let s = (c["fade"].as_f64().unwrap_or(0.0) * fps).round().max(0.0) as usize;
        // una junta no puede comerse un clip entero por ninguno de los lados
        s.min(tramos[k].n.saturating_sub(1)).min(tramos[k + 1].n.saturating_sub(1))
    }).collect();

    // DÓNDE EMPIEZA CADA TRAMO en la bobina: el siguiente entra `solape`
    // fotogramas ANTES de que acabe el anterior — ahí es donde conviven.
    let mut arranque = Vec::with_capacity(tramos.len());
    let mut t0 = 0usize;
    for (k, tr) in tramos.iter().enumerate() {
        arranque.push(t0);
        t0 += tr.n - solapes[k];
    }
    let total = t0 + solapes.last().copied().unwrap_or(0);

    // el renglón de cada fotograma de salida
    let mut renglones: Vec<Renglon> = Vec::with_capacity(total);
    let mut k = 0usize;                       // el tramo que manda ahora
    for t in 0..total {
        while k + 1 < tramos.len() && t >= arranque[k] + tramos[k].n { k += 1; }
        let Tr { src, t_in, t_out, n, fi: f_in, fo: f_out } = tramos[k];
        let i = t - arranque[k];              // fotograma dentro del tramo
        let f = &fuentes[src];
        let mut r = Renglon {
            fuente_a: src as u32, fuente_b: NINGUNA, peso_b: 0.0,
            t_a: t_fuente(t_in, t_out, i, fps, f.veloc),
            t_b: 0.0, color_fijo: 0.0, nivel_color: 0.0,
            capas: [CapaR::VACIA; MAX_CAPAS],
            corte: false,
        };
        // ¿estamos dentro de la junta con el siguiente?
        let solape = solapes[k];
        if solape > 0 && i >= n - solape {
            let j = i - (n - solape);
            let sig = tramos[k + 1].src;
            let g = &fuentes[sig];
            r.fuente_b = sig as u32;
            r.peso_b = (j as f32 + 0.5) / solape as f32;
            r.t_b = t_fuente(tramos[k + 1].t_in, tramos[k + 1].t_out, j, fps, g.veloc);
        }
        // fundidos a negro del propio clip
        if f_in > 0 && i < f_in {
            r.nivel_color = 1.0 - (i as f32 + 0.5) / f_in as f32;
        }
        let al_final = n - 1 - i.min(n - 1);
        if f_out > 0 && al_final < f_out {
            r.nivel_color = r.nivel_color.max(1.0 - (al_final as f32 + 0.5) / f_out as f32);
        }
        // ¿EMPIEZA UN PLANO? Si la fuente cambia y no venimos de una junta con
        // ella (o sea, no es el final de un encadenado), es un corte seco.
        r.corte = match renglones.last() {
            None => true,
            Some(p) => p.fuente_a != r.fuente_a && p.fuente_b != r.fuente_a,
        };
        renglones.push(r);
    }

    // ── EL REMUESTREO DE CADENCIA ─────────────────────────────────────────
    // Medido antes de tocar nada, con una barra que avanza un número exacto de
    // píxeles por fotograma de origen: de 59,94 a 30 el avance es 2,000 clavado
    // (desviación 0,000) y de 59,94 a 24 alterna 3, 2, 3, 2 (desviación 0,500).
    // ESO es el tirón: el objeto recorre un 25 % más en un fotograma que en el
    // siguiente. No es un fallo de cuentas, es que se elegía el fotograma de
    // origen MÁS CERCANO y punto.
    //
    // Lo que se hace ahora es lo mismo que el filtro de reducción hace en el
    // espacio (§1.5), pero en el tiempo: el fotograma del máster cae ENTRE dos
    // de la fuente y se toman los dos, pesados por lo cerca que está de cada
    // uno. El centro de la imagen avanza entonces exactamente 2,4975 fotogramas
    // cada vez, sin alternancia — que es la definición de que no hay tirón.
    //
    // Y se hace SIN tocar los motores: mezclar dos fuentes con un peso es
    // justo lo que ya hacen para un encadenado. Cada fuente que necesite
    // interpolar estrena una GEMELA (el mismo fichero, la misma receta, su
    // propio decodificador) y cada renglón la usa como lado B.
    //
    // Tres cosas que NO hace, a propósito:
    //   · si la cadencia del máster divide exacta a la de la fuente (60→30,
    //     60→60, 50→25) no interpola nada: el peso sale 0 y la imagen queda
    //     idéntica a la de antes, nítida.
    //   · si el máster va MÁS RÁPIDO que la fuente (30→60) tampoco: ahí
    //     mezclar sólo inventaría fantasmas donde antes había un fotograma
    //     repetido, que es lo honesto.
    //   · dentro de un encadenado el lado B ya está ocupado por el otro plano;
    //     esos pocos fotogramas siguen yendo al vecino más cercano.
    if !renglones.is_empty() {
        // cuántos fotogramas de fuente hay por cada uno del máster
        let cadencias: Vec<f64> = clips.iter()
            .map(|c| c["fps_src"].as_f64().unwrap_or(0.0))
            .collect();
        let mut gemela: Vec<Option<u32>> = vec![None; fuentes.len()];
        let mut cuantas = 0usize;
        for t in 0..renglones.len() {
            let src = renglones[t].fuente_a as usize;
            let Some(&fs) = cadencias.get(src) else { continue };
            let (hueco, foto, veloc) = {
                let f = &fuentes[src];
                (f.hueco, f.foto, f.veloc)
            };
            if fs <= 0.0 || hueco || foto || veloc.abs() < 0.02 { continue }
            if fs / fps <= 1.001 { continue }          // el máster no reduce: nada que mezclar
            if renglones[t].fuente_b != NINGUNA { continue }   // en plena junta
            let k = renglones[t].t_a * fs;
            let mut k0 = k.floor().max(0.0);
            let mut w = (k - k0) as f32;
            // CASI ENCIMA DE UNO DE LOS DOS: se toma ése y no se mezcla. Pero
            // «casi» tiene que ser MUY casi, y ahí me equivoqué: con el margen
            // en 0,02 el fotograma se movía hasta 0,02 de su sitio, y como el
            // desvío alterna de signo entre uno y otro, el avance salía
            // 2,49 / 2,51 / 2,48 / 2,52… con la amplitud creciendo. Ese era
            // TODO el tirón que quedaba — medido: bajando el margen a 0,004 la
            // desviación cae de 0,106 a la centésima parte, y ni el códec ni
            // el espacio de color tenían nada que ver (ProRes con gelatinas
            // neutras daba lo mismo que HEVC con la LUT de la casa).
            //
            // El margen no es cero por una razón práctica: sin él, una razón
            // exacta como 60→30 acabaría mezclando por culpa del último bit de
            // la coma flotante y pagaríamos un segundo decodificador por nada.
            const PEGADO: f32 = 0.004;
            if w > 1.0 - PEGADO { k0 += 1.0; w = 0.0; }
            if w < PEGADO { w = 0.0; }
            renglones[t].t_a = k0 / fs;
            if w == 0.0 { continue }
            let g = match gemela[src] {
                Some(g) => g,
                None => {
                    let copia = fuentes[src].clone();
                    fuentes.push(copia);
                    let g = (fuentes.len() - 1) as u32;
                    gemela[src] = Some(g);
                    cuantas += 1;
                    g
                }
            };
            renglones[t].fuente_b = g;
            renglones[t].t_b = (k0 + 1.0) / fs;
            renglones[t].peso_b = w;
        }
        if cuantas > 0 {
            eprintln!("   cadencia: {cuantas} fuente(s) se interpolan entre dos fotogramas");
        }
    }

    // ── LAS CAPAS (CAPAS §3) ─────────────────────────────────────────────
    // `clips2` es el carril de encima: clips COLOCADOS (con `start`), no en
    // secuencia. Por fotograma se elige la capa de más arriba que cubra ese
    // instante (la última de la lista: el orden ES el apilado) y su alfa
    // global sale de sus fundidos de entrada y salida.
    let capas = payload["clips2"].as_array().cloned().unwrap_or_default();
    if !capas.is_empty() {
        struct Cp { src: usize, start: f64, dur: f64, t_in: f64, t_out: f64,
                    veloc: f64, fi: f64, fo: f64 }
        let mut lista: Vec<Cp> = Vec::new();
        for c in &capas {
            let veloc = c["speed"].as_f64().unwrap_or(1.0).clamp(-8.0, 8.0);
            let veloc = if veloc.abs() < 0.02 { 0.0 } else { veloc };
            let ruta = c["file"].as_str().unwrap_or("");
            fuentes.push(Fuente {
                fichero: ruta.to_string(),
                hueco: false,
                prefs: match c["prefs"].as_object() {
                    Some(o) if !o.is_empty() => c["prefs"].clone(),
                    // una capa sin receta va DIRECTA: un rótulo no pasó por
                    // la cámara y no le toca el baño de la casa
                    _ => Value::Null,
                },
                lut_in: c["lut_in"].as_str().map(String::from),
                lut: c["lut"].as_str().map(String::from),
                enc: Encuadre::de_json(&c["tf"], c["cuartos"].as_u64().unwrap_or(0) as u8),
                veloc,
                foto: es_foto(std::path::Path::new(ruta)),
                capa: true,
                mat: mat_de_json(&c["mat"]),
            });
            let t_in = c["in"].as_f64().unwrap_or(0.0);
            let t_out = c["out"].as_f64().unwrap_or(0.0).max(t_in);
            let dur = if veloc.abs() < 0.02 { t_out - t_in }
                      else { (t_out - t_in) / veloc.abs() };
            lista.push(Cp {
                src: fuentes.len() - 1,
                start: c["start"].as_f64().unwrap_or(0.0).max(0.0),
                dur, t_in, t_out, veloc,
                fi: c["fadeIn"].as_f64().unwrap_or(0.0).max(0.0),
                fo: c["fadeOut"].as_f64().unwrap_or(0.0).max(0.0),
            });
        }
        // el apilado es POR PISTA (y a igual pista, el orden de llegada)
        let mut orden: Vec<usize> = (0..lista.len()).collect();
        orden.sort_by_key(|&k| (capas[k]["pista"].as_u64().unwrap_or(0), k));
        for (t, r) in renglones.iter_mut().enumerate() {
            let seg = t as f64 / fps;
            // TODAS las que cubren este instante, de abajo arriba. Con más de
            // MAX_CAPAS solapadas a la vez se quedan las de encima — y con el
            // máximo igual al número de pistas, eso no pasa nunca.
            let cubren: Vec<&Cp> = orden.iter().map(|&k| &lista[k])
                .filter(|c| seg >= c.start - 1e-9 && seg < c.start + c.dur - 1e-9)
                .collect();
            let desde = cubren.len().saturating_sub(MAX_CAPAS);
            for (hueco, cp) in cubren[desde..].iter().enumerate() {
                let dentro = seg - cp.start;
                let i = (dentro * fps).round() as usize;
                let tt = t_fuente(cp.t_in, cp.t_out, i, fps, cp.veloc);
                let mut a = 1.0f32;
                if cp.fi > 0.001 { a = a.min((dentro / cp.fi) as f32); }
                if cp.fo > 0.001 { a = a.min(((cp.dur - dentro) / cp.fo) as f32); }
                r.capas[hueco] = CapaR {
                    fuente: cp.src as u32, t: tt, alfa: a.clamp(0.0, 1.0),
                };
            }
        }
    }

    // fundidos de cabeza y cola de la BOBINA entera
    let cabeza = (payload["project"]["fadeHead"].as_f64().unwrap_or(0.0) * fps).round() as usize;
    let cola = (payload["project"]["fadeTail"].as_f64().unwrap_or(0.0) * fps).round() as usize;
    for t in 0..total {
        if cabeza > 0 && t < cabeza {
            let v = 1.0 - (t as f32 + 0.5) / cabeza as f32;
            renglones[t].nivel_color = renglones[t].nivel_color.max(v);
        }
        if cola > 0 && total - 1 - t < cola {
            let v = 1.0 - ((total - 1 - t) as f32 + 0.5) / cola as f32;
            renglones[t].nivel_color = renglones[t].nivel_color.max(v);
        }
    }

    Ok(Plan {
        w, h, fps, fuentes, renglones,
        salida: payload["out"].as_str().unwrap_or("master.mp4").to_string(),
        codec: payload["master"]["codec"].as_str().unwrap_or("hevc").to_string(),
        bitrate: payload["master"]["bitrate"].as_i64().unwrap_or(60_000_000),
    })
}

/// Trocea la bobina en tramos por fuente, cortando en las juntas. Un tramo de
/// cuerpo depende de UN clip; uno de junta, de los dos que se cruzan. Así, si
/// cambia la receta de un clip, los tramos que no lo miran salen de la caché
/// tal cual (MOTOR §7).
pub fn tramos(renglones: &[Renglon]) -> Vec<Tramo> {
    let mut v: Vec<Tramo> = Vec::new();
    for (i, r) in renglones.iter().enumerate() {
        let mut f = vec![r.fuente_a];
        if r.fuente_b != NINGUNA { f.push(r.fuente_b); }
        // LAS CAPAS SON DEPENDENCIA: sin esto, la caché fina daba por bueno
        // un tramo viejo después de mover o recolorear una capa que lo cruza.
        for c in &r.capas {
            if c.fuente != NINGUNA { f.push(c.fuente); }
        }
        match v.last_mut() {
            Some(t) if t.fuentes == f => t.cuantos += 1,
            _ => v.push(Tramo { desde: i, cuantos: 1, fuentes: f }),
        }
    }
    v
}

// ── LA MATRIZ DEL ENCUADRE ────────────────────────────────────────────────
//
// Una afín 2×3 y nada más. Antes eran dos vec4 con la escala por un lado y el
// coseno/seno del giro por otro, y el shader tenía que saber en qué orden
// aplicarlos y corregir el aspecto a mano; con el ancla y los cuartos de
// vuelta encima eso ya no se sostenía. Aquí se compone la cadena entera en la
// CPU —volteo, cuartos, conform, escala, posición, giro sobre el ancla— y al
// shader le llegan seis números: `uv_fuente = M · uv_lienzo`.

/// afín 2×3: (x,y) → (a·x + b·y + tx, c·x + d·y + ty)
#[derive(Clone, Copy)]
struct Af { a: f32, b: f32, tx: f32, c: f32, d: f32, ty: f32 }

impl Af {
    const ID: Af = Af { a: 1.0, b: 0.0, tx: 0.0, c: 0.0, d: 1.0, ty: 0.0 };
    fn escala(sx: f32, sy: f32) -> Af { Af { a: sx, d: sy, ..Af::ID } }
    fn mueve(dx: f32, dy: f32) -> Af { Af { tx: dx, ty: dy, ..Af::ID } }
    fn gira(rad: f32) -> Af {
        let (s, k) = rad.sin_cos();
        Af { a: k, b: -s, tx: 0.0, c: s, d: k, ty: 0.0 }
    }
    /// `self ∘ o`: se aplica `o` primero
    fn por(self, o: Af) -> Af {
        Af {
            a: self.a * o.a + self.b * o.c,
            b: self.a * o.b + self.b * o.d,
            tx: self.a * o.tx + self.b * o.ty + self.tx,
            c: self.c * o.a + self.d * o.c,
            d: self.c * o.b + self.d * o.d,
            ty: self.c * o.tx + self.d * o.ty + self.ty,
        }
    }
}

/// deshacer `q` cuartos de vuelta a derechas: uv del material COLOCADO → uv
/// del fichero. Girar el material 90° a derechas lleva su punto (x,y) a
/// (1−y, x), así que la vuelta es la inversa de eso.
fn cuartos_inv(q: u8) -> Af {
    match q & 3 {
        1 => Af { a: 0.0, b: 1.0, tx: 0.0, c: -1.0, d: 0.0, ty: 1.0 },
        2 => Af { a: -1.0, b: 0.0, tx: 1.0, c: 0.0, d: -1.0, ty: 1.0 },
        3 => Af { a: 0.0, b: -1.0, tx: 1.0, c: 1.0, d: 0.0, ty: 0.0 },
        _ => Af::ID,
    }
}

/// cuánto ocupa el material en el lienzo, en fracción de lienzo (ancho, alto).
/// Lo usan la matriz y la interfaz (el recuadro del encuadre sobre el visor).
pub fn extension(e: &Encuadre, sw: f32, sh: f32, pw: f32, ph: f32) -> (f32, f32) {
    let (sw, sh) = (sw.max(1.0), sh.max(1.0));
    let (pw, ph) = (pw.max(1.0), ph.max(1.0));
    // EL MATERIAL GIRADO UN CUARTO DE VUELTA CAMBIA DE FORMA: el conform se
    // calcula con el ancho y el alto intercambiados o el encaje sale mal
    let (dw, dh) = if e.cuartos % 2 == 1 { (sh, sw) } else { (sw, sh) };
    let (kx, ky) = match e.encaje {
        Encaje::Estira => (pw / dw, ph / dh),
        Encaje::Llena => { let k = (pw / dw).max(ph / dh); (k, k) }
        Encaje::Dentro => { let k = (pw / dw).min(ph / dh); (k, k) }
    };
    let (ex, ey) = (e.escala.0.abs().max(0.01), e.escala.1.abs().max(0.01));
    ((dw * kx * ex / pw).max(1e-5), (dh * ky * ey / ph).max(1e-5))
}

/// LA MATRIZ DEL ENCUADRE, lista para el uniforme.
///
/// Devuelve `(enc_a, enc_b, paso)`:
///   · `enc_a = [m00, m01, m10, m11]` y `enc_b.xy = [m02, m12]` son la afín
///     `uv_fuente = M · uv_lienzo`;
///   · `enc_b.zw` son CUÁNTAS MUESTRAS hacen falta por eje (1..4);
///   · `paso` son los dos vectores de separación entre muestras, en uv de
///     fuente por píxel de salida.
///
/// Lo último es el filtro de reducción (PENDIENTE §1.5, segunda parte). Al
/// agrandar, un tap bilineal vale; al REDUCIR —y se reduce siempre, porque
/// conformar 4K a 1080 ya es reducir a la mitad— un solo tap se salta píxeles
/// y eso es el hormigueo de las barandillas y del pelo. Con la afín completa
/// el factor real de reducción se sabe exactamente, así que el número de
/// muestras se decide aquí y no a ojo en el shader.
pub fn matriz(e: &Encuadre, sw: f32, sh: f32, pw: f32, ph: f32)
    -> ([f32; 4], [f32; 4], [f32; 4])
{
    let (sw, sh) = (sw.max(1.0), sh.max(1.0));
    let (pw, ph) = (pw.max(1.0), ph.max(1.0));
    let (ew, eh) = extension(e, sw, sh, pw, ph);
    let ar = pw / ph;
    // dónde cae el ANCLA sobre el lienzo, antes de girar
    let ax = 0.5 + e.pos.0 + (e.ancla.0 - 0.5) * ew;
    let ay = 0.5 + e.pos.1 + (e.ancla.1 - 0.5) * eh;

    // uv de lienzo → uv de fichero, deshaciendo la cadena al revés
    let mut m = Af::escala(ar, 1.0);                       // a espacio isótropo
    m = Af::mueve(ax * ar, ay)
            .por(Af::gira(-e.giro.to_radians()))
            .por(Af::mueve(-ax * ar, -ay))
            .por(m);                                        // desgirar sobre el ancla
    m = Af::escala(1.0 / ar, 1.0).por(m);                   // de vuelta al lienzo
    m = Af::mueve(0.5, 0.5)
            .por(Af::escala(1.0 / ew, 1.0 / eh))
            .por(Af::mueve(-(0.5 + e.pos.0), -(0.5 + e.pos.1)))
            .por(m);                                        // a uv del material colocado
    m = cuartos_inv(e.cuartos).por(m);                      // deshacer los cuartos
    let vx = if e.voltea.0 { Af { a: -1.0, tx: 1.0, ..Af::ID } } else { Af::ID };
    let vy = if e.voltea.1 { Af { d: -1.0, ty: 1.0, ..Af::ID } } else { Af::ID };
    m = vy.por(vx).por(m);                                  // y el volteo

    // el paso entre muestras, en uv de FUENTE por píxel de salida
    let paso = [m.a / pw, m.c / pw, m.b / ph, m.d / ph];
    // cuántos téxeles de la fuente cubre un píxel de salida en cada eje
    let fx = ((paso[0] * sw).powi(2) + (paso[1] * sh).powi(2)).sqrt();
    let fy = ((paso[2] * sw).powi(2) + (paso[3] * sh).powi(2)).sqrt();
    let taps = |f: f32| -> f32 {
        if !f.is_finite() { return 1.0; }
        f.ceil().clamp(1.0, 4.0)
    };
    ([m.a, m.b, m.c, m.d], [m.tx, m.ty, taps(fx), taps(fy)], paso)
}

// ── EL APLANADO DE BOBINAS ANIDADAS (CAPAS §8) ───────────────────────────
//
// Una bobina dentro de otra NO llega al motor: aquí se sustituye el clip
// anidado por los clips reales de la hija —recortados a su ventana, con su
// receta y con la matriz compuesta—, y sus capas y su música se desplazan al
// tiempo del padre. El motor revela clips normales y no sabe que hubo
// anidamiento; la preview resuelve por su lado con el mismo modelo.
//
// `carga` devuelve el PAYLOAD de la bobina hija por su clave (quien llama
// decide de dónde: la app de sus subbobinas, el shell de un fichero, los
// tests de un mapa). `dims` da el ancho y alto de un fichero de material,
// que hacen falta para componer la matriz interior.

/// aplana todos los clips con `anidada` del payload. Devuelve cuántos
/// aplanó. Profundidad máxima 3 y guarda de ciclos por clave.
pub fn aplana_anidadas(payload: &mut Value,
                       carga: &dyn Fn(&str) -> Option<Value>,
                       dims: &dyn Fn(&str) -> Option<(f32, f32)>)
    -> Result<usize, String>
{
    let mut vistos: Vec<String> = Vec::new();
    aplana_nivel(payload, carga, dims, 0, &mut vistos)
}

fn aplana_nivel(payload: &mut Value,
                carga: &dyn Fn(&str) -> Option<Value>,
                dims: &dyn Fn(&str) -> Option<(f32, f32)>,
                hondo: usize, vistos: &mut Vec<String>)
    -> Result<usize, String>
{
    if hondo > 3 { return Err("bobinas anidadas a más de 3 niveles".into()); }
    let fps = payload["project"]["fps"].as_f64().unwrap_or(25.0).max(1.0);
    let pw = payload["project"]["w"].as_f64().unwrap_or(1920.0) as f32;
    let ph = payload["project"]["h"].as_f64().unwrap_or(1080.0) as f32;
    let clips = payload["clips"].as_array().cloned().unwrap_or_default();
    if !clips.iter().any(|c| c["anidada"].as_str().is_some()) { return Ok(0); }

    // LOS ARRANQUES en la bobina padre, con la MISMA cuenta que `compila`:
    // duración redondeada a fotogramas y el solape de los encadenados. Sin
    // esto las capas y la música de la hija caerían corridas.
    let dur_de = |c: &Value| -> f64 {
        let v = c["speed"].as_f64().unwrap_or(1.0).clamp(-8.0, 8.0);
        let d = c["out"].as_f64().unwrap_or(0.0) - c["in"].as_f64().unwrap_or(0.0);
        let d = if v.abs() < 0.02 { d } else { d / v.abs() };
        ((d * fps).round().max(1.0)) / fps
    };
    let mut arranques = Vec::with_capacity(clips.len());
    let mut t0 = 0.0f64;
    for (k, c) in clips.iter().enumerate() {
        arranques.push(t0);
        let solape = if k + 1 < clips.len() {
            c["fade"].as_f64().unwrap_or(0.0).max(0.0)
        } else { 0.0 };
        t0 += (dur_de(c) - solape).max(0.0);
    }

    let mut nuevos: Vec<Value> = Vec::new();
    let mut capas_extra: Vec<Value> = Vec::new();
    let mut audio_extra: Vec<Value> = Vec::new();
    let mut cuantas = 0usize;

    for (k, c) in clips.iter().enumerate() {
        let Some(clave) = c["anidada"].as_str().map(String::from) else {
            nuevos.push(c.clone());
            continue;
        };
        if vistos.contains(&clave) {
            return Err(format!("la bobina «{clave}» se contiene a sí misma"));
        }
        let Some(mut hija) = carga(&clave) else {
            return Err(format!("no encuentro la bobina anidada «{clave}»"));
        };
        vistos.push(clave.clone());
        aplana_nivel(&mut hija, carga, dims, hondo + 1, vistos)?;
        vistos.pop();
        cuantas += 1;

        let cw = hija["project"]["w"].as_f64().unwrap_or(1920.0) as f32;
        let ch = hija["project"]["h"].as_f64().unwrap_or(1080.0) as f32;
        // la matriz EXTERIOR: lienzo padre → lienzo hijo (el encuadre que el
        // autor le puso al clip anidado, tratando a la hija como material)
        let enc_out = Encuadre::de_json(&c["tf"], 0);
        let (oa, ob, _) = matriz(&enc_out, cw, ch, pw, ph);
        let fuera = (oa, ob);
        // la VENTANA sobre la hija, en tiempo de bobina hija
        let v_in = c["in"].as_f64().unwrap_or(0.0).max(0.0);
        let v_out = c["out"].as_f64().unwrap_or(0.0).max(v_in);
        if c["speed"].as_f64().unwrap_or(1.0) != 1.0 {
            // v1: el clip anidado va a ×1 (se avisa en vez de mentir)
            eprintln!("   ⚠ clip anidado «{clave}» con velocidad ≠ 1: se trata como ×1");
        }
        let pos_padre = arranques[k];

        // compone la matriz de un elemento de la hija con la exterior
        let compon = |el: &Value| -> Option<Value> {
            let dentro: ([f32; 4], [f32; 4]) = if let Some(m) = mat_de_json(&el["mat"]) {
                ([m[0], m[1], m[3], m[4]], [m[2], m[5], 0.0, 0.0])
            } else {
                let (fw, fh) = dims(el["file"].as_str().unwrap_or(""))?;
                let e = Encuadre::de_json(&el["tf"],
                                          el["cuartos"].as_u64().unwrap_or(0) as u8);
                let (ia, ib, _) = matriz(&e, fw, fh, cw, ch);
                (ia, ib)
            };
            let m = compon_mat(dentro, fuera);
            Some(serde_json::json!(m.to_vec()))
        };

        // ── los clips de la hija, recortados a la ventana ────────────────
        let hijos = hija["clips"].as_array().cloned().unwrap_or_default();
        let mut acc = 0.0f64;
        let mut primero = true;
        let n_antes = nuevos.len();
        for hc in &hijos {
            let d = {
                let v = hc["speed"].as_f64().unwrap_or(1.0).clamp(-8.0, 8.0);
                let d = hc["out"].as_f64().unwrap_or(0.0) - hc["in"].as_f64().unwrap_or(0.0);
                if v.abs() < 0.02 { d } else { d / v.abs() }
            };
            let (ini, fin) = (acc, acc + d);
            acc = fin;
            if fin <= v_in + 1e-9 || ini >= v_out - 1e-9 { continue; }
            let mut nc = hc.clone();
            // el recorte, EXACTO para las tres marchas: con v>0 un segundo de
            // bobina son v de fuente desde la entrada; con v<0, desde la
            // salida; congelado, la duración es literal
            let corta_cabeza = (v_in - ini).max(0.0);
            let corta_cola = (fin - v_out).max(0.0);
            let v = hc["speed"].as_f64().unwrap_or(1.0).clamp(-8.0, 8.0);
            let (mut hi, mut ho) = (hc["in"].as_f64().unwrap_or(0.0),
                                    hc["out"].as_f64().unwrap_or(0.0));
            if v.abs() < 0.02 {
                ho -= corta_cabeza + corta_cola;      // el mismo fotograma, menos rato
            } else if v > 0.0 {
                hi += corta_cabeza * v;
                ho -= corta_cola * v;
            } else {
                ho -= corta_cabeza * v.abs();
                hi += corta_cola * v.abs();
            }
            nc["in"] = serde_json::json!(hi);
            nc["out"] = serde_json::json!(ho.max(hi));
            if let Some(m) = compon(hc) { nc["mat"] = m; }
            if primero {
                // el fundido a negro de entrada del clip anidado, si lo tenía
                if let Some(f) = c["fadeIn"].as_f64() {
                    if f > 0.0 { nc["fadeIn"] = serde_json::json!(f); }
                }
                primero = false;
            }
            nuevos.push(nc);
        }
        if nuevos.len() == n_antes {
            return Err(format!("la ventana del clip anidado «{clave}» no coge nada"));
        }
        // el encadenado del clip anidado con el siguiente lo hereda su último
        if let Some(ult) = nuevos.last_mut() {
            if let Some(f) = c["fade"].as_f64() {
                if f > 0.0 { ult["fade"] = serde_json::json!(f); }
            }
            if let Some(f) = c["fadeOut"].as_f64() {
                if f > 0.0 { ult["fadeOut"] = serde_json::json!(f); }
            }
        }

        // ── las capas de la hija, desplazadas y recortadas ───────────────
        for cp in hija["clips2"].as_array().cloned().unwrap_or_default() {
            let st = cp["start"].as_f64().unwrap_or(0.0);
            let d = cp["out"].as_f64().unwrap_or(0.0) - cp["in"].as_f64().unwrap_or(0.0);
            let (ini, fin) = (st, st + d.max(0.0));
            if fin <= v_in + 1e-9 || ini >= v_out - 1e-9 { continue; }
            let mut nc = cp.clone();
            let corta_cabeza = (v_in - ini).max(0.0);
            let corta_cola = (fin - v_out).max(0.0);
            nc["in"] = serde_json::json!(cp["in"].as_f64().unwrap_or(0.0) + corta_cabeza);
            nc["out"] = serde_json::json!((cp["out"].as_f64().unwrap_or(0.0) - corta_cola)
                                          .max(cp["in"].as_f64().unwrap_or(0.0)));
            nc["start"] = serde_json::json!(pos_padre + (ini.max(v_in) - v_in));
            if let Some(m) = compon(&cp) { nc["mat"] = m; }
            capas_extra.push(nc);
        }

        // ── la música de la hija, salvo que el clip anidado esté mudo ────
        if !c["mute"].as_bool().unwrap_or(false) {
            for au in hija["audio"].as_array().cloned().unwrap_or_default() {
                let st = au["start"].as_f64().unwrap_or(0.0);
                let d = au["out"].as_f64().unwrap_or(0.0) - au["in"].as_f64().unwrap_or(0.0);
                let (ini, fin) = (st, st + d.max(0.0));
                if fin <= v_in + 1e-9 || ini >= v_out - 1e-9 { continue; }
                let mut na = au.clone();
                let corta_cabeza = (v_in - ini).max(0.0);
                let corta_cola = (fin - v_out).max(0.0);
                na["in"] = serde_json::json!(au["in"].as_f64().unwrap_or(0.0) + corta_cabeza);
                na["out"] = serde_json::json!((au["out"].as_f64().unwrap_or(0.0) - corta_cola)
                                              .max(au["in"].as_f64().unwrap_or(0.0)));
                na["start"] = serde_json::json!(pos_padre + (ini.max(v_in) - v_in));
                audio_extra.push(na);
            }
        }
    }

    payload["clips"] = serde_json::json!(nuevos);
    if !capas_extra.is_empty() {
        let mut c2 = payload["clips2"].as_array().cloned().unwrap_or_default();
        c2.extend(capas_extra);
        payload["clips2"] = serde_json::json!(c2);
    }
    if !audio_extra.is_empty() {
        let mut au = payload["audio"].as_array().cloned().unwrap_or_default();
        au.extend(audio_extra);
        payload["audio"] = serde_json::json!(au);
    }
    Ok(cuantas)
}

/// los seis números de una matriz explícita del payload, si vienen
pub fn mat_de_json(v: &Value) -> Option<[f32; 6]> {
    let a = v.as_array()?;
    if a.len() != 6 { return None }
    let mut m = [0.0f32; 6];
    for (k, x) in a.iter().enumerate() { m[k] = x.as_f64()? as f32; }
    Some(m)
}

/// la matriz de una fuente del plan (el atajo que usan los motores).
///
/// Si la fuente trae `mat` —el aplanado de una anidada—, manda la matriz
/// explícita, y el paso del filtro de reducción y sus muestras se calculan
/// de ella exactamente igual que se calculan de la compuesta.
pub fn matriz_de(f: &Fuente, sw: f32, sh: f32, pw: f32, ph: f32)
    -> ([f32; 4], [f32; 4], [f32; 4])
{
    if let Some(m) = f.mat {
        let (pw, ph) = (pw.max(1.0), ph.max(1.0));
        let (sw, sh) = (sw.max(1.0), sh.max(1.0));
        // m = [a, b, tx, c, d, ty]: uv de lienzo → uv de fuente
        let paso = [m[0] / pw, m[3] / pw, m[1] / ph, m[4] / ph];
        let fx = ((paso[0] * sw).powi(2) + (paso[1] * sh).powi(2)).sqrt();
        let fy = ((paso[2] * sw).powi(2) + (paso[3] * sh).powi(2)).sqrt();
        let taps = |f: f32| -> f32 {
            if !f.is_finite() { return 1.0; }
            f.ceil().clamp(1.0, 6.0)
        };
        return ([m[0], m[1], m[3], m[4]], [m[2], m[5], taps(fx), taps(fy)], paso);
    }
    matriz(&f.enc, sw, sh, pw, ph)
}

/// COMPONER DOS AFINES del encuadre: primero `fuera` (uv del lienzo padre →
/// uv del lienzo hijo) y luego `dentro` (uv del lienzo hijo → uv del
/// fichero). Es el corazón del aplanado de anidadas: el resultado es una
/// sola matriz que el motor aplica sin saber que hubo dos.
pub fn compon_mat(dentro: ([f32; 4], [f32; 4]), fuera: ([f32; 4], [f32; 4]))
    -> [f32; 6]
{
    let (a2, b2) = dentro;   // y = A2·x + b2
    let (a1, b1) = fuera;    // x = A1·u + b1
    // total = A2·A1·u + A2·b1 + b2
    [
        a2[0] * a1[0] + a2[1] * a1[2],
        a2[0] * a1[1] + a2[1] * a1[3],
        a2[0] * b1[0] + a2[1] * b1[1] + b2[0],
        a2[2] * a1[0] + a2[3] * a1[2],
        a2[2] * a1[1] + a2[3] * a1[3],
        a2[2] * b1[0] + a2[3] * b1[1] + b2[1],
    ]
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn aplica(m: ([f32; 4], [f32; 4], [f32; 4]), u: f32, v: f32) -> (f32, f32) {
        let (a, b, _) = m;
        (a[0] * u + a[1] * v + b[0], a[2] * u + a[3] * v + b[1])
    }

    #[test]
    fn identidad_cuando_el_material_es_el_lienzo() {
        let m = matriz(&Encuadre::limpio(0), 1920.0, 1080.0, 1920.0, 1080.0);
        for (u, v) in [(0.0, 0.0), (1.0, 1.0), (0.25, 0.75)] {
            let (x, y) = aplica(m, u, v);
            assert!((x - u).abs() < 1e-4 && (y - v).abs() < 1e-4, "{x} {y} vs {u} {v}");
        }
        // sin reducción no hace falta más de una muestra
        assert_eq!(m.1[2], 1.0);
        assert_eq!(m.1[3], 1.0);
    }

    #[test]
    fn un_cuarto_de_vuelta_endereza_el_movil() {
        // material 1920×1080 grabado de lado (va girado 90°) en un lienzo 9:16
        let e = Encuadre::limpio(1);
        let m = matriz(&e, 1920.0, 1080.0, 1080.0, 1920.0);
        // el centro del lienzo es el centro del fichero
        let (x, y) = aplica(m, 0.5, 0.5);
        assert!((x - 0.5).abs() < 1e-4 && (y - 0.5).abs() < 1e-4, "{x} {y}");
        // la esquina de ARRIBA-IZQUIERDA del lienzo mira a la de abajo-izquierda
        // del fichero (girar a derechas la lleva allí)
        let (x, y) = aplica(m, 0.0, 0.0);
        assert!((x - 0.0).abs() < 1e-3, "x={x}");
        assert!((y - 1.0).abs() < 1e-3, "y={y}");
        // y ocupa el lienzo entero: 1080×1920 girado ES 9:16
        assert!((extension(&e, 1920.0, 1080.0, 1080.0, 1920.0).0 - 1.0).abs() < 1e-3);
    }

    #[test]
    fn el_letterbox_cae_fuera() {
        // 16:9 dentro de un lienzo 9:16: arriba y abajo, fuera
        let m = matriz(&Encuadre::limpio(0), 1920.0, 1080.0, 1080.0, 1920.0);
        let (_, y) = aplica(m, 0.5, 0.02);
        assert!(y < 0.0, "la banda de arriba tendría que caer fuera, y={y}");
        let (_, y) = aplica(m, 0.5, 0.5);
        assert!((0.0..=1.0).contains(&y));
    }

    #[test]
    fn reducir_4k_a_1080_pide_mas_de_una_muestra() {
        let m = matriz(&Encuadre::limpio(0), 3840.0, 2160.0, 1920.0, 1080.0);
        assert_eq!(m.1[2], 2.0, "4K a 1080 son dos téxeles por píxel");
        assert_eq!(m.1[3], 2.0);
    }

    #[test]
    fn el_ancla_es_el_punto_que_no_se_mueve() {
        let mut e = Encuadre::limpio(0);
        e.ancla = (0.2, 0.8);
        e.giro = 30.0;
        let m = matriz(&e, 1920.0, 1080.0, 1920.0, 1080.0);
        // el ancla está en el lienzo justo donde estaría sin giro
        let (x, y) = aplica(m, 0.2, 0.8);
        assert!((x - 0.2).abs() < 1e-3 && (y - 0.8).abs() < 1e-3, "{x} {y}");
    }

    #[test]
    fn voltear_espeja() {
        let mut e = Encuadre::limpio(0);
        e.voltea = (true, false);
        let m = matriz(&e, 1920.0, 1080.0, 1920.0, 1080.0);
        let (x, _) = aplica(m, 0.25, 0.5);
        assert!((x - 0.75).abs() < 1e-4, "x={x}");
    }

    /// una bobina de dos clips, con o sin encadenado
    fn bobina(fade: f64) -> Value {
        serde_json::json!({
            "project": {"w": 1920, "h": 1080, "fps": 25.0},
            "clips": [
                {"file": "a.mp4", "in": 0.0, "out": 2.0, "fade": fade},
                {"file": "b.mp4", "in": 0.0, "out": 2.0},
            ],
        })
    }

    #[test]
    fn el_corte_seco_marca_el_arranque_del_plano() {
        let p = compila(&bobina(0.0)).unwrap();
        // el primer fotograma de la bobina y el primero del segundo clip
        let marcados: Vec<usize> = p.renglones.iter().enumerate()
            .filter(|(_, r)| r.corte).map(|(i, _)| i).collect();
        assert_eq!(marcados, vec![0, 50], "50 = 2 s a 25 fps");
        // y el fotograma anterior al corte sigue siendo del primer clip
        assert_eq!(p.renglones[49].fuente_a, 0);
        assert_eq!(p.renglones[50].fuente_a, 1);
    }

    #[test]
    fn un_encadenado_no_es_un_corte() {
        // con medio segundo de junta, el arrastre del obturador es legítimo:
        // las dos imágenes conviven de verdad
        let p = compila(&bobina(0.5)).unwrap();
        let marcados: Vec<usize> = p.renglones.iter().enumerate()
            .filter(|(_, r)| r.corte).map(|(i, _)| i).collect();
        assert_eq!(marcados, vec![0], "solo el arranque de la bobina");
        // y la junta existe: hay renglones con dos fuentes
        assert!(p.renglones.iter().any(|r| r.fuente_b != NINGUNA));
    }

    #[test]
    fn el_modelo_viejo_se_sigue_leyendo() {
        // `{scale, x, y}` de las bobinas ya guardadas
        let v = serde_json::json!({"scale": 2.0, "x": 0.1, "y": -0.2, "rot": 0.0, "fit": "fit"});
        let e = Encuadre::de_json(&v, 0);
        assert_eq!(e.escala, (2.0, 2.0));
        assert_eq!(e.pos, (0.1, -0.2));
        assert_eq!(e.cuartos, 0);
    }

    #[test]
    fn ida_y_vuelta_por_json() {
        let mut e = Encuadre::limpio(3);
        e.escala = (1.5, 1.2);
        e.pos = (0.05, -0.1);
        e.giro = 12.5;
        e.ancla = (0.3, 0.4);
        e.voltea = (true, true);
        e.encaje = Encaje::Llena;
        assert_eq!(Encuadre::de_json(&e.json(), 0), e);
    }

    // ── EL REMUESTREO DE CADENCIA ─────────────────────────────────────────

    fn bobina_de(fps: f64, fps_src: f64) -> Plan {
        compila(&serde_json::json!({
            "project": {"w": 1920, "h": 1080, "fps": fps},
            "clips": [{"file": "x.mp4", "in": 0.0, "out": 3.0, "fps_src": fps_src}],
        })).unwrap()
    }

    /// dónde cae de verdad la imagen de cada renglón, en fotogramas de la
    /// fuente: el punto medio pesado de los dos que se mezclan
    fn posiciones(p: &Plan, fs: f64) -> Vec<f64> {
        p.renglones.iter().map(|r| {
            let a = r.t_a * fs;
            if r.fuente_b == NINGUNA { a }
            else { a * (1.0 - r.peso_b as f64) + r.t_b * fs * r.peso_b as f64 }
        }).collect()
    }

    fn tiron(p: &Plan, fs: f64) -> f64 {
        let k = posiciones(p, fs);
        let d: Vec<f64> = k.windows(2).map(|w| w[1] - w[0]).collect();
        let m = d.iter().sum::<f64>() / d.len() as f64;
        (d.iter().map(|x| (x - m).powi(2)).sum::<f64>() / d.len() as f64).sqrt()
    }

    #[test]
    fn cadencia_exacta_no_se_toca() {
        // 59,94 → 29,97 es dos a uno: ni gemela ni mezcla, y la imagen que
        // sale tiene que ser LA MISMA que antes de que esto existiera
        let p = bobina_de(60000.0 / 1001.0 / 2.0, 60000.0 / 1001.0);
        assert_eq!(p.fuentes.len(), 1, "no hacía falta gemela");
        assert!(p.renglones.iter().all(|r| r.fuente_b == NINGUNA));
        let k = posiciones(&p, 60000.0 / 1001.0);
        for (n, x) in k.iter().enumerate() {
            assert!((x - 2.0 * n as f64).abs() < 1e-6, "renglón {n}: {x}");
        }
    }

    #[test]
    fn el_tiron_de_59_94_a_24_desaparece() {
        // ANTES (medido con una barra que avanza 10 px por fotograma): el
        // avance alternaba 3, 2, 3, 2 — desviación 0,500. La cuenta de
        // entonces era «el fotograma de origen más cercano», que es esto:
        let fs = 60000.0 / 1001.0;
        let p = bobina_de(24.0, fs);
        let cercano: Vec<f64> = (0..p.renglones.len())
            .map(|n| (n as f64 / 24.0 * fs).round()).collect();
        let d: Vec<f64> = cercano.windows(2).map(|w| w[1] - w[0]).collect();
        let m = d.iter().sum::<f64>() / d.len() as f64;
        let antes = (d.iter().map(|x| (x - m).powi(2)).sum::<f64>() / d.len() as f64).sqrt();
        assert!(antes > 0.4, "el tirón de antes era {antes}");
        // AHORA: la posición aparente avanza 2,4975 cada vez, sin alternar
        assert!(tiron(&p, fs) < 0.02, "sigue habiendo tirón: {}", tiron(&p, fs));
        assert_eq!(p.fuentes.len(), 2, "hace falta una gemela y sólo una");
        assert!(p.renglones.iter().any(|r| r.fuente_b != NINGUNA));
    }

    /// EL PLAN NO TIENE TIRÓN, Y SE MIDE. La posición aparente de cada
    /// fotograma —la media pesada de las dos muestras— tiene que avanzar
    /// SIEMPRE lo mismo. Esta prueba es la que separa las culpas: si algún día
    /// vuelve el tirón y esto sigue en cero, el fallo está en el motor y no
    /// aquí, que es exactamente lo que pasó la primera vez que lo medí.
    #[test]
    fn el_plan_reparte_los_fotogramas_sin_tiron() {
        let fs = 60000.0 / 1001.0;
        for fps in [24.0, 25.0, 30.0, 50.0] {
            let p = bobina_de(fps, fs);
            let t = tiron(&p, fs);
            assert!(t < 0.002, "a {fps} fps el plan da un tirón de {t}");
        }
    }

    #[test]
    fn hacia_arriba_no_se_inventa_nada() {
        // 30 → 60: mezclar sólo pondría fantasmas donde antes había un
        // fotograma repetido. Se deja como estaba.
        let p = bobina_de(60.0, 30.0);
        assert_eq!(p.fuentes.len(), 1);
        assert!(p.renglones.iter().all(|r| r.fuente_b == NINGUNA));
    }

    // ── EL APLANADO DE ANIDADAS ──────────────────────────────────────

    fn hija_simple() -> Value {
        serde_json::json!({
            "project": {"w": 1080, "h": 1920, "fps": 10.0},
            "clips": [
                {"file": "h1.mp4", "in": 2.0, "out": 5.0},
                {"file": "h2.mp4", "in": 0.0, "out": 4.0},
            ],
            "clips2": [{"file": "rotulo.png", "start": 1.0, "in": 0.0, "out": 5.0}],
            "audio": [{"file": "cancion.m4a", "start": 0.0, "in": 10.0, "out": 17.0}],
        })
    }

    #[test]
    fn la_anidada_se_aplana_con_su_ventana() {
        let mut p = serde_json::json!({
            "project": {"w": 1920, "h": 1080, "fps": 10.0},
            "clips": [
                {"file": "a.mp4", "in": 0.0, "out": 2.0},
                // la hija dura 7 s; la ventana coge del 1 al 6
                {"anidada": "hija", "in": 1.0, "out": 6.0},
                {"file": "b.mp4", "in": 0.0, "out": 1.0},
            ],
        });
        let n = aplana_anidadas(&mut p,
            &|k| if k == "hija" { Some(hija_simple()) } else { None },
            &|_| Some((1920.0, 1080.0))).unwrap();
        assert_eq!(n, 1);
        let c = p["clips"].as_array().unwrap();
        // a + (h1 recortado, h2 recortado) + b
        assert_eq!(c.len(), 4);
        // h1 dura 3 s (2..5): la ventana le quita 1 de cabeza → fuente 3..5
        assert!((c[1]["in"].as_f64().unwrap() - 3.0).abs() < 1e-9);
        assert!((c[1]["out"].as_f64().unwrap() - 5.0).abs() < 1e-9);
        // h2 dura 4 s: la ventana acaba en 6 → le quita 1 de cola → 0..3
        assert!((c[2]["out"].as_f64().unwrap() - 3.0).abs() < 1e-9);
        // los aplanados llevan matriz compuesta
        assert!(c[1]["mat"].is_array() && c[2]["mat"].is_array());
        // la capa de la hija empezaba en 1,0 = justo donde abre la ventana:
        // en el padre cae al arrancar el clip anidado (t = 2,0)
        let c2 = p["clips2"].as_array().unwrap();
        assert_eq!(c2.len(), 1);
        assert!((c2[0]["start"].as_f64().unwrap() - 2.0).abs() < 1e-9);
        // y la música: empezaba en 0, la ventana la recorta 1 s de cabeza
        let au = p["audio"].as_array().unwrap();
        assert!((au[0]["start"].as_f64().unwrap() - 2.0).abs() < 1e-9);
        assert!((au[0]["in"].as_f64().unwrap() - 11.0).abs() < 1e-9);
        // y el resultado COMPILA con las capas dentro
        let plan = compila(&p).unwrap();
        assert!(plan.renglones.iter().any(|r| r.capas[0].fuente != NINGUNA));
    }

    #[test]
    fn el_ciclo_se_detecta() {
        let mut p = serde_json::json!({
            "project": {"w": 1920, "h": 1080, "fps": 10.0},
            "clips": [{"anidada": "a", "in": 0.0, "out": 1.0}],
        });
        let e = aplana_anidadas(&mut p,
            &|k| Some(serde_json::json!({
                "project": {"w": 1920, "h": 1080, "fps": 10.0},
                "clips": [{"anidada": if k == "a" { "b" } else { "a" },
                           "in": 0.0, "out": 1.0}],
            })),
            &|_| Some((1920.0, 1080.0)));
        assert!(e.is_err(), "un ciclo a→b→a tiene que negarse");
    }

    #[test]
    fn dos_niveles_componen_las_tres_matrices() {
        // la nieta llena su lienzo; la hija la encoge a la mitad centrada; el
        // padre la vuelve a encoger a la mitad. El centro queda en el centro y
        // la esquina del lienzo padre cae FUERA del material (transparencia
        // del conform, no del fichero).
        let nieta = serde_json::json!({
            "project": {"w": 1920, "h": 1080, "fps": 10.0},
            "clips": [{"file": "n.mp4", "in": 0.0, "out": 2.0}],
        });
        let hija = serde_json::json!({
            "project": {"w": 1920, "h": 1080, "fps": 10.0},
            "clips": [{"anidada": "nieta", "in": 0.0, "out": 2.0,
                       "tf": {"escala": [0.5, 0.5], "pos": [0.0, 0.0]}}],
        });
        let mut p = serde_json::json!({
            "project": {"w": 1920, "h": 1080, "fps": 10.0},
            "clips": [{"anidada": "hija", "in": 0.0, "out": 2.0,
                       "tf": {"escala": [0.5, 0.5], "pos": [0.0, 0.0]}}],
        });
        let hoja = nieta.clone();
        aplana_anidadas(&mut p,
            &move |k| match k { "hija" => Some(hija.clone()),
                                "nieta" => Some(hoja.clone()), _ => None },
            &|_| Some((1920.0, 1080.0))).unwrap();
        let c = p["clips"].as_array().unwrap();
        assert_eq!(c.len(), 1);
        let m = mat_de_json(&c[0]["mat"]).expect("matriz compuesta");
        // uv 0,5 → 0,5 (el centro no se mueve)
        let cx = m[0] * 0.5 + m[1] * 0.5 + m[2];
        let cy = m[3] * 0.5 + m[4] * 0.5 + m[5];
        assert!((cx - 0.5).abs() < 1e-4 && (cy - 0.5).abs() < 1e-4, "{cx} {cy}");
        // dos mitades = un cuarto: la esquina cae muy fuera de 0..1
        let ex = m[0] * 0.0 + m[1] * 0.0 + m[2];
        assert!(ex < -0.9, "esquina → {ex} (esperaba ≈ −1,5)");
    }

    // ── CAPAS Y ANIDADAS ─────────────────────────────────────────────

    #[test]
    fn la_capa_cubre_sus_fotogramas_y_solo_los_suyos() {
        let p = compila(&serde_json::json!({
            "project": {"w": 1920, "h": 1080, "fps": 10.0},
            "clips": [{"file": "base.mp4", "in": 0.0, "out": 4.0}],
            "clips2": [{"file": "titulo.png", "start": 1.0, "in": 0.0, "out": 2.0,
                        "fadeIn": 0.5, "fadeOut": 0.5}],
        })).unwrap();
        assert_eq!(p.fuentes.len(), 2);
        assert!(p.fuentes[1].capa && p.fuentes[1].foto);
        // fotogramas 0..9 sin capa; 10..29 con ella; 30..39 sin
        assert!(p.renglones[..10].iter().all(|r| r.capas[0].fuente == NINGUNA));
        assert!(p.renglones[10..30].iter().all(|r| r.capas[0].fuente == 1));
        assert!(p.renglones[30..].iter().all(|r| r.capas[0].fuente == NINGUNA));
        // la rampa: a mitad del fundido de entrada, alfa ≈ 0,5; en el cuerpo, 1
        let a = p.renglones[12].capas[0].alfa;   // 0,25 s dentro de 0,5 s
        assert!((a - 0.5).abs() < 0.11, "alfa de la rampa = {a}");
        assert!((p.renglones[20].capas[0].alfa - 1.0).abs() < 1e-6);
    }

    #[test]
    fn dos_capas_gana_la_de_encima() {
        let p = compila(&serde_json::json!({
            "project": {"w": 1920, "h": 1080, "fps": 10.0},
            "clips": [{"file": "base.mp4", "in": 0.0, "out": 3.0}],
            "clips2": [
                {"file": "abajo.png",  "start": 0.0, "in": 0.0, "out": 3.0},
                {"file": "arriba.png", "start": 1.0, "in": 0.0, "out": 1.0},
            ],
        })).unwrap();
        assert_eq!(p.renglones[5].capas[0].fuente, 1);   // sólo la de abajo
        assert_eq!(p.renglones[5].capas[1].fuente, NINGUNA);
        // donde conviven: la de abajo en el hueco 0 y la de encima en el 1
        assert_eq!(p.renglones[15].capas[0].fuente, 1);
        assert_eq!(p.renglones[15].capas[1].fuente, 2);
        assert_eq!(p.renglones[25].capas[0].fuente, 1);
        assert_eq!(p.renglones[25].capas[1].fuente, NINGUNA);
    }

    #[test]
    fn ocho_capas_conviven_y_ninguna_se_cae() {
        let capas: Vec<Value> = (0..8).map(|k| serde_json::json!({
            "file": format!("c{k}.png"), "start": 0.0, "in": 0.0, "out": 1.0,
            "pista": k,
        })).collect();
        let p = compila(&serde_json::json!({
            "project": {"w": 1920, "h": 1080, "fps": 10.0},
            "clips": [{"file": "base.mp4", "in": 0.0, "out": 1.0}],
            "clips2": capas,
        })).unwrap();
        let r = &p.renglones[5];
        for k in 0..8 {
            assert_eq!(r.capas[k].fuente, 1 + k as u32,
                       "el hueco {k} lleva su pista, de abajo arriba");
        }
    }

    #[test]
    fn la_capa_es_dependencia_de_su_tramo() {
        let p = compila(&serde_json::json!({
            "project": {"w": 1920, "h": 1080, "fps": 10.0},
            "clips": [{"file": "base.mp4", "in": 0.0, "out": 2.0}],
            "clips2": [{"file": "t.png", "start": 1.0, "in": 0.0, "out": 1.0}],
        })).unwrap();
        let tr = tramos(&p.renglones);
        assert_eq!(tr.len(), 2, "el tramo se parte donde entra la capa");
        assert!(tr[1].fuentes.contains(&1),
                "sin la capa en las dependencias, la caché mentiría");
    }

    #[test]
    fn la_matriz_explicita_manda_y_compone_bien() {
        // dentro: identidad · fuera: escala ×2 centrada → el total debe ser
        // exactamente la afín de fuera
        let dentro = ([1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0, 0.0]);
        let fuera = ([0.5, 0.0, 0.0, 0.5], [0.25, 0.25, 0.0, 0.0]);
        let m = compon_mat(dentro, fuera);
        assert_eq!(&m[..], &[0.5, 0.0, 0.25, 0.0, 0.5, 0.25]);
        // y matriz_de con mat aplicada a una fuente: el centro va al centro
        let f = Fuente {
            fichero: "x.mp4".into(), hueco: false, prefs: Value::Null,
            lut_in: None, lut: None, enc: Encuadre::limpio(0), veloc: 1.0,
            foto: false, capa: false, mat: Some(m),
        };
        let (a, b, _) = matriz_de(&f, 1920.0, 1080.0, 1920.0, 1080.0);
        let (u, v) = (0.5f32, 0.5f32);
        let x = a[0] * u + a[1] * v + b[0];
        let y = a[2] * u + a[3] * v + b[1];
        assert!((x - 0.5).abs() < 1e-6 && (y - 0.5).abs() < 1e-6, "{x} {y}");
    }

    #[test]
    fn sin_saber_la_cadencia_no_se_toca() {
        let p = bobina_de(24.0, 0.0);
        assert_eq!(p.fuentes.len(), 1);
        assert!(p.renglones.iter().all(|r| r.fuente_b == NINGUNA));
    }
}
