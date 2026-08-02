//! Sólo en Windows: incrusta el icono del taller como recurso del ejecutable.
//! En el Mac y en Linux no hace nada (el .icns lo pone el empaquetado).
fn main() {
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rerun-if-changed=../shell/icons/icon.ico");
        let mut r = winresource::WindowsResource::new();
        r.set_icon("../shell/icons/icon.ico");
        r.set("ProductName", "Laboratorios Saorín");
        r.set("FileDescription", "Laboratorios Saorín — el taller de revelado");
        // que falle el compilador de recursos NO puede tumbar la compilación:
        // sin icono se trabaja, sin binario no.
        if let Err(e) = r.compile() {
            println!("cargo:warning=sin icono incrustado: {e}");
        }
    }
}
