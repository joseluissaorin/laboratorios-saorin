fn main() {
    // sólo si se pide la ventana vieja: sin la bandera no hay tauri que
    // preparar (y el taller compila en máquinas donde tauri-build no va)
    #[cfg(feature = "ventana")]
    tauri_build::build();
}
