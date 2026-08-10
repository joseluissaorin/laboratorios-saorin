//! EL OÍDO, APAGADO. Este taller se compiló sin `oido` (whisper.cpp pide
//! libclang para generar sus enlaces y no siempre está). Todo lo demás
//! funciona; los subtítulos automáticos lo dicen en vez de fallar raro.

use std::path::{Path, PathBuf};

pub struct Trozo { pub t0: f64, pub t1: f64, pub texto: String }
pub struct Trabajo {
    pub fichero: PathBuf, pub t_in: f64, pub t_out: f64,
    pub desde: f64, pub velocidad: f64,
}

const SIN: &str = "este taller se compiló sin el oído (falta LLVM/libclang \
                   al compilar): los subtítulos automáticos no están";

pub fn modelo(_taller: &Path, _cual: usize, _aviso: &dyn Fn(&str)) -> Result<PathBuf, String> {
    Err(SIN.into())
}
pub struct Palabra { pub t0: f64, pub t1: f64, pub txt: String, pub corte: bool }
pub fn palabras_json(_p: &[Palabra]) -> String { "{\"palabras\":[]}".into() }
pub fn escucha(_m: &Path, _ff: &str, _media: &Path, _idioma: &str, _largo: i32,
               _aviso: &dyn Fn(&str)) -> Result<(Vec<Trozo>, Vec<Palabra>), String> { Err(SIN.into()) }
pub fn escucha_bobina(_m: &Path, _ff: &str, _t: &[Trabajo], _idioma: &str, _largo: i32,
                      _aviso: &dyn Fn(&str)) -> Result<(Vec<Trozo>, Vec<Palabra>), String> { Err(SIN.into()) }
pub const LARGO_PIE: i32 = 84;
pub fn srt(_t: &[Trozo]) -> String { String::new() }
pub fn el_de_esta_maquina() -> usize { 0 }
