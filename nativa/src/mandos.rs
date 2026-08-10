//! LOS MANDOS: los primitivos con los que se elige algo en el taller.
//!
//! Hasta ahora todo se elegía **ciclando**: un botón que al pulsarlo pasa al
//! siguiente valor. Está bien para dos o tres opciones y para un valor que se
//! ajusta a ojo (el cuerpo de la letra, un fundido), pero es malo para una
//! lista: no ves lo que hay, no sabes cuántas quedan, y llegar a la de abajo
//! son siete clics a ciegas.
//!
//! Aquí vive lo que faltaba:
//!
//! - **el desplegable** (`Desplegable`): enseña lo elegido y, al pulsarlo,
//!   ABRE LA LISTA ENTERA. Se ve lo que hay y se va directo;
//! - **la regla de dos tiradores** (`Regla`): un rango se elige arrastrando,
//!   no escribiendo dos números.
//!
//! Los dos siguen la regla de la casa: **una sola geometría** que leen el
//! dibujo y el ratón, para que no puedan descolocarse.

use crate::paleta;
use crate::trazo;
use crate::ui::{Dibujo, Familia};

/// UN DESPLEGABLE: dónde está, qué dice y qué opciones tiene.
pub struct Desplegable<'a> {
    pub x: f32,
    pub y: f32,
    pub ancho: f32,
    /// el rótulo de arriba (en versalitas)
    pub rotulo: &'a str,
    pub opciones: &'a [String],
    pub elegida: usize,
}

/// EL ALTO DE LA CAJA de un desplegable (sin el rótulo)
pub const ALTO: f32 = 26.0;
/// el alto de cada opción cuando está abierto
pub const FILA: f32 = 24.0;
/// cuánto ocupa un desplegable con su rótulo
pub const CON_ROTULO: f32 = ALTO + 16.0;

impl<'a> Desplegable<'a> {
    /// la caja que se pulsa para abrirlo
    pub fn caja(&self) -> (f32, f32, f32, f32) {
        (self.x, self.y + 16.0, self.ancho, ALTO)
    }

    /// ¿el ratón está encima de la caja?
    pub fn en_la_caja(&self, mx: f32, my: f32) -> bool {
        let (x, y, w, h) = self.caja();
        mx >= x && mx <= x + w && my >= y && my <= y + h
    }

    /// LA LISTA ABIERTA: dónde cae cada opción. Se abre hacia abajo salvo que
    /// no quepa, y entonces hacia arriba — una lista que se sale de la
    /// ventana es una lista que no se puede usar.
    pub fn lista(&self, alto_ventana: f32) -> (f32, f32, f32, f32) {
        let (x, y, w, _) = self.caja();
        let h = self.opciones.len() as f32 * FILA + 8.0;
        let abajo = y + ALTO + 2.0;
        let arriba = y - h - 2.0;
        let cabe_abajo = abajo + h <= alto_ventana - 6.0;
        (x, if cabe_abajo || arriba < 6.0 { abajo } else { arriba }, w, h)
    }

    /// qué opción hay bajo el ratón con la lista abierta
    pub fn opcion_en(&self, alto_ventana: f32, mx: f32, my: f32) -> Option<usize> {
        let (x, y, w, h) = self.lista(alto_ventana);
        if mx < x || mx > x + w || my < y || my > y + h { return None; }
        let k = ((my - y - 4.0) / FILA).floor();
        if k < 0.0 { return None; }
        let k = k as usize;
        if k < self.opciones.len() { Some(k) } else { None }
    }

    /// EL MANDO CERRADO: lo elegido y la flecha que dice que hay más
    pub fn dibuja(&self, d: &mut Dibujo, sobre: bool, orden: u32) {
        d.texto(self.x, self.y, self.rotulo, 7.5, paleta::TINTA_TENUE);
        let (x, y, w, h) = self.caja();
        d.rect(x, y, w, h, [1.0, 1.0, 1.0, if sobre { 0.55 } else { 0.30 }]);
        trazo::caja(d, x, y, w, h, if sobre { 1.6 } else { 1.2 },
                    if sobre { paleta::ROJO } else { paleta::TINTA_TENUE }, orden);
        let vacia = String::new();
        let t = self.opciones.get(self.elegida).unwrap_or(&vacia);
        let cabe = ((w - 34.0) / 7.2).max(4.0) as usize;
        let t: String = t.chars().take(cabe).collect();
        d.texto_f(Familia::Mano, x + 9.0, y + 4.0, &t, 16.0, paleta::TINTA);
        // la flecha: tres rayas que bajan, dibujadas y no un carácter del
        // sistema (que no está en el atlas del taller)
        let (fx, fy) = (x + w - 17.0, y + h * 0.5 - 2.0);
        for k in 0..4 {
            let g = k as f32;
            d.rect(fx - 4.0 + g, fy + g, 9.0 - 2.0 * g, 1.4, paleta::TINTA);
        }
    }

    /// LA LISTA ABIERTA. Va la última en dibujarse: tapa lo que haya debajo.
    pub fn dibuja_abierta(&self, d: &mut Dibujo, alto_ventana: f32,
                          raton: (f32, f32), orden: u32) {
        let (x, y, w, h) = self.lista(alto_ventana);
        d.rect(x + 3.0, y + 4.0, w, h, [0.0, 0.0, 0.0, 0.22]);        // sombra
        d.rect(x, y, w, h, [0.996, 0.988, 0.965, 1.0]);
        trazo::caja(d, x, y, w, h, 1.4, paleta::TINTA, orden);
        let bajo = self.opcion_en(alto_ventana, raton.0, raton.1);
        for (i, op) in self.opciones.iter().enumerate() {
            let fy = y + 4.0 + i as f32 * FILA;
            if bajo == Some(i) {
                d.rect(x + 2.0, fy, w - 4.0, FILA, [0.851, 0.2, 0.145, 0.16]);
            }
            // la elegida lleva su marca: se ve de un vistazo dónde estás
            if i == self.elegida {
                d.rect(x + 7.0, fy + FILA * 0.5 - 2.5, 5.0, 5.0, paleta::ROJO);
            }
            let cabe = ((w - 32.0) / 6.6).max(4.0) as usize;
            let t: String = op.chars().take(cabe).collect();
            d.texto(x + 18.0, fy + 7.0, &t, 9.5,
                    if bajo == Some(i) { paleta::ROJO } else { paleta::TINTA });
        }
    }
}

/// LA REGLA DE DOS TIRADORES: elegir un tramo arrastrando, que es como se
/// elige un tramo. Escribir dos números es lo que hace un formulario.
pub struct Regla {
    pub x: f32,
    pub y: f32,
    pub ancho: f32,
    /// el tramo entero que se puede elegir (0 → `total`)
    pub total: f64,
    pub a: f64,
    pub b: f64,
}

pub const REGLA_ALTO: f32 = 30.0;

impl Regla {
    fn px(&self, t: f64) -> f32 {
        self.x + (t / self.total.max(0.001)) as f32 * self.ancho
    }

    pub fn tiempo(&self, px: f32) -> f64 {
        (((px - self.x) / self.ancho.max(1.0)) as f64 * self.total).clamp(0.0, self.total)
    }

    /// qué tirador hay cerca de una x (0 = el de entrada, 1 = el de salida)
    pub fn tirador_en(&self, mx: f32, my: f32) -> Option<u8> {
        if my < self.y - 8.0 || my > self.y + REGLA_ALTO + 8.0 { return None; }
        let (pa, pb) = (self.px(self.a), self.px(self.b));
        if (mx - pa).abs() <= 9.0 { return Some(0); }
        if (mx - pb).abs() <= 9.0 { return Some(1); }
        None
    }

    pub fn dibuja(&self, d: &mut Dibujo, rot: &str, orden: u32) {
        d.texto(self.x, self.y - 14.0, rot, 7.5, paleta::TINTA_TENUE);
        // la cinta entera, y el tramo elegido en rojo encima
        d.rect(self.x, self.y + 10.0, self.ancho, 8.0, [0.80, 0.78, 0.72, 1.0]);
        let (pa, pb) = (self.px(self.a), self.px(self.b));
        d.rect(pa, self.y + 10.0, (pb - pa).max(2.0), 8.0, [0.851, 0.2, 0.145, 0.55]);
        // los dos tiradores, con su agarradero
        for (k, px) in [pa, pb].iter().enumerate() {
            d.rect(px - 3.0, self.y + 2.0, 6.0, 24.0, paleta::ROJO);
            trazo::caja(d, px - 3.0, self.y + 2.0, 6.0, 24.0, 1.0, paleta::TINTA,
                        orden + k as u32);
        }
        let reloj = |t: f64| format!("{}:{:04.1}", (t / 60.0) as u64, t % 60.0);
        d.texto(self.x, self.y + 22.0, &reloj(self.a), 8.0, paleta::TINTA_TENUE);
        let fin = reloj(self.b);
        d.texto(self.x + self.ancho - fin.len() as f32 * 5.4, self.y + 22.0, &fin,
                8.0, paleta::TINTA_TENUE);
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn ops() -> Vec<String> {
        (0..5).map(|i| format!("opción {i}")).collect()
    }

    #[test]
    fn la_lista_se_abre_hacia_arriba_si_no_cabe_abajo() {
        let o = ops();
        let d = Desplegable { x: 10.0, y: 400.0, ancho: 200.0, rotulo: "X",
                              opciones: &o, elegida: 0 };
        let (_, y, _, h) = d.lista(460.0);       // ventana baja: no cabe debajo
        assert!(y + h <= 420.0, "se sale por abajo: y={y} h={h}");
        let (_, y2, _, _) = d.lista(900.0);      // ventana alta: cabe
        assert!(y2 > 400.0, "debería abrirse hacia abajo");
    }

    #[test]
    fn cada_opcion_cae_donde_se_dibuja() {
        let o = ops();
        let d = Desplegable { x: 10.0, y: 40.0, ancho: 200.0, rotulo: "X",
                              opciones: &o, elegida: 2 };
        let (lx, ly, _, _) = d.lista(600.0);
        for i in 0..o.len() {
            let cy = ly + 4.0 + i as f32 * FILA + FILA * 0.5;
            assert_eq!(d.opcion_en(600.0, lx + 20.0, cy), Some(i), "opción {i}");
        }
        // y fuera de la lista, nada
        assert_eq!(d.opcion_en(600.0, lx - 5.0, ly + 10.0), None);
    }

    #[test]
    fn los_tiradores_de_la_regla_se_cogen_donde_se_ven() {
        let r = Regla { x: 20.0, y: 100.0, ancho: 400.0, total: 60.0, a: 15.0, b: 45.0 };
        // el de entrada cae a un cuarto, el de salida a tres cuartos
        assert_eq!(r.tirador_en(120.0, 110.0), Some(0));
        assert_eq!(r.tirador_en(320.0, 110.0), Some(1));
        assert_eq!(r.tirador_en(220.0, 110.0), None);
        // y el tiempo se lee donde se pincha
        assert!((r.tiempo(220.0) - 30.0).abs() < 0.1);
    }
}
