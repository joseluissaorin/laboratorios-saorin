//! ARRASTRAR FUERA DE LA VENTANA (NORTE §7.16): coger el máster de la cuerda
//! de secado y soltarlo en el Finder / el Explorador / otra app.
//!
//! No es un truco de portapapeles: es el arrastre del sistema de verdad
//! (NSDraggingSession en macOS, DoDragDrop en Windows), con su fantasma y su
//! destino real — lo que el autor pidió explícitamente.

use std::path::Path;

/// arranca el arrastre del fichero desde la ventana. Devuelve false si el
/// sistema no lo aceptó (entonces el llamante puede caer al portapapeles).
#[cfg(target_os = "macos")]
pub fn arrastra(ventana: &winit::window::Window, ruta: &Path, mini: Option<&[u8]>) -> bool {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send, sel};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = ventana.window_handle() else { return false };
    let RawWindowHandle::AppKit(ak) = handle.as_raw() else { return false };
    let view: *mut AnyObject = ak.ns_view.as_ptr().cast();
    if view.is_null() {
        return false;
    }
    let ruta = ruta.to_string_lossy().to_string();

    unsafe {
        // NSApp.currentEvent: el arrastre del sistema necesita el evento de
        // ratón que lo origina (estamos dentro del handler del clic)
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        if app.is_null() {
            return false;
        }
        let evento: *mut AnyObject = msg_send![app, currentEvent];
        if evento.is_null() {
            if std::env::var("FL_CRONO").is_ok() { eprintln!("  arrastre fuera: sin NSEvent"); }
            return false;
        }
        if std::env::var("FL_CRONO").is_ok() {
            let tipo: usize = msg_send![evento, type];
            eprintln!("  arrastre fuera: NSEvent tipo {tipo}");
        }

        // NSURL fileURLWithPath:
        let ns_ruta: Retained<AnyObject> = {
            let s: *mut AnyObject = msg_send![class!(NSString), alloc];
            let bytes = ruta.as_bytes();
            let s: *mut AnyObject = msg_send![s,
                initWithBytes: bytes.as_ptr() as *const std::ffi::c_void,
                length: bytes.len(),
                encoding: 4usize];       // NSUTF8StringEncoding
            match Retained::from_raw(s) {
                Some(r) => r,
                None => return false,
            }
        };
        let url: *mut AnyObject = msg_send![class!(NSURL), fileURLWithPath: &*ns_ruta];
        if url.is_null() {
            return false;
        }

        // el elemento arrastrable lleva la URL en su pasteboard item
        let item: *mut AnyObject = msg_send![class!(NSDraggingItem), alloc];
        let item: *mut AnyObject = msg_send![item, initWithPasteboardWriter: url];
        if item.is_null() {
            return false;
        }
        let item = match Retained::<AnyObject>::from_raw(item) {
            Some(r) => r,
            None => return false,
        };

        // el fantasma que sigue al ratón: la miniatura del render si la hay,
        // y si no el icono del propio fichero (NSWorkspace)
        let imagen: *mut AnyObject = match mini.and_then(imagen_desde_png) {
            Some(img) => Retained::into_raw(img),
            None => {
                let ws: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
                msg_send![ws, iconForFile: &*ns_ruta]
            }
        };
        if imagen.is_null() {
            return false;
        }
        let tam: NSSize = msg_send![imagen, size];
        // el fantasma nace bajo el cursor
        let punto: NSPoint = msg_send![evento, locationInWindow];
        let punto: NSPoint = msg_send![view, convertPoint: punto, fromView: std::ptr::null::<AnyObject>()];
        let marco = NSRect {
            origin: NSPoint { x: punto.x - tam.width / 2.0, y: punto.y - tam.height / 2.0 },
            size: tam,
        };
        let _: () = msg_send![&*item, setDraggingFrame: marco, contents: imagen];

        // la sesión de arrastre: la propia vista hace de source (implementa
        // NSDraggingSource desde macOS 10.7 con el comportamiento por defecto)
        let arr: *mut AnyObject = msg_send![class!(NSArray), arrayWithObject: &*item];
        let responde: bool = msg_send![view, respondsToSelector: sel!(beginDraggingSessionWithItems:event:source:)];
        if !responde {
            return false;
        }
        let sesion: *mut AnyObject = msg_send![view,
            beginDraggingSessionWithItems: arr,
            event: evento,
            source: view];
        if std::env::var("FL_CRONO").is_ok() && !sesion.is_null() {
            // ¿qué lleva de verdad el arrastre? (los tipos del pasteboard)
            let pb: *mut AnyObject = msg_send![sesion, draggingPasteboard];
            if !pb.is_null() {
                let tipos: *mut AnyObject = msg_send![pb, types];
                let n: usize = msg_send![tipos, count];
                eprint!("  arrastre fuera: sesión sí, {n} tipo(s):");
                for i in 0..n.min(4) {
                    let t: *mut AnyObject = msg_send![tipos, objectAtIndex: i];
                    let c: *const std::os::raw::c_char = msg_send![t, UTF8String];
                    if !c.is_null() {
                        let cs = std::ffi::CStr::from_ptr(c).to_string_lossy().to_string();
                        eprint!(" {cs}");
                    }
                }
                let url_str: *mut AnyObject = msg_send![pb,
                    stringForType: &*nsstr("public.file-url")];
                if !url_str.is_null() {
                    let c: *const std::os::raw::c_char = msg_send![url_str, UTF8String];
                    if !c.is_null() {
                        eprint!(" → {}", std::ffi::CStr::from_ptr(c).to_string_lossy());
                    }
                }
                eprintln!();
            }
        } else if std::env::var("FL_CRONO").is_ok() {
            eprintln!("  arrastre fuera: sesión NO");
        }
        !sesion.is_null()
    }
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct NSPoint { x: f64, y: f64 }
#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct NSSize { width: f64, height: f64 }
#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct NSRect { origin: NSPoint, size: NSSize }
#[cfg(target_os = "macos")]
unsafe impl objc2::Encode for NSPoint {
    const ENCODING: objc2::Encoding =
        objc2::Encoding::Struct("CGPoint", &[objc2::Encoding::Double, objc2::Encoding::Double]);
}
#[cfg(target_os = "macos")]
unsafe impl objc2::Encode for NSSize {
    const ENCODING: objc2::Encoding =
        objc2::Encoding::Struct("CGSize", &[objc2::Encoding::Double, objc2::Encoding::Double]);
}
#[cfg(target_os = "macos")]
unsafe impl objc2::Encode for NSRect {
    const ENCODING: objc2::Encoding =
        objc2::Encoding::Struct("CGRect", &[NSPoint::ENCODING, NSSize::ENCODING]);
}

/// una NSString desde &str (para consultar el pasteboard)
#[cfg(target_os = "macos")]
fn nsstr(t: &str) -> objc2::rc::Retained<objc2::runtime::AnyObject> {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    unsafe {
        let s: *mut AnyObject = msg_send![class!(NSString), alloc];
        let s: *mut AnyObject = msg_send![s,
            initWithBytes: t.as_ptr() as *const std::ffi::c_void,
            length: t.len(),
            encoding: 4usize];
        Retained::from_raw(s).expect("NSString")
    }
}

/// el fantasma: una NSImage hecha con los bytes PNG de la miniatura
#[cfg(target_os = "macos")]
fn imagen_desde_png(png: &[u8]) -> Option<objc2::rc::Retained<objc2::runtime::AnyObject>> {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    unsafe {
        let datos: *mut AnyObject = msg_send![class!(NSData), alloc];
        let datos: *mut AnyObject = msg_send![datos,
            initWithBytes: png.as_ptr() as *const std::ffi::c_void,
            length: png.len()];
        let datos = Retained::<AnyObject>::from_raw(datos)?;
        let img: *mut AnyObject = msg_send![class!(NSImage), alloc];
        let img: *mut AnyObject = msg_send![img, initWithData: &*datos];
        Retained::from_raw(img)
    }
}

// ─────────────────────────── Windows: DoDragDrop ───────────────────────────

#[cfg(target_os = "windows")]
pub fn arrastra(_ventana: &winit::window::Window, ruta: &Path, _mini: Option<&[u8]>) -> bool {
    win::arrastra(ruta)
}

#[cfg(target_os = "windows")]
mod win {
    use std::path::Path;
    use windows::core::{implement, Result, HRESULT};
    use windows::Win32::Foundation::{DV_E_FORMATETC, E_NOTIMPL, HGLOBAL, POINT, S_OK};
    use windows::Win32::System::Com::{IAdviseSink, IDataObject, IDataObject_Impl,
                                      IEnumFORMATETC, IEnumSTATDATA, FORMATETC, STGMEDIUM,
                                      TYMED_HGLOBAL};
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::System::Ole::{DoDragDrop, IDropSource, IDropSource_Impl,
                                      DROPEFFECT, DROPEFFECT_COPY};
    use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
    use windows::Win32::UI::Shell::DROPFILES;

    /// CF_HDROP: el formato de portapapeles de «lista de ficheros» (winuser.h)
    const CF_HDROP: u16 = 15;

    // los HRESULT del arrastre (no están expuestos como constantes en 0.58)
    const DRAGDROP_S_DROP: HRESULT = HRESULT(0x0004_0100u32 as i32);
    const DRAGDROP_S_CANCEL: HRESULT = HRESULT(0x0004_0101u32 as i32);
    const DRAGDROP_S_USEDEFAULTCURSORS: HRESULT = HRESULT(0x0004_0102u32 as i32);
    const MK_LBUTTON: u32 = 0x0001;

    /// el objeto de datos del arrastre: un CF_HDROP con la ruta del máster
    #[implement(IDataObject)]
    struct Datos {
        medio: isize,   // el HGLOBAL, como entero (HGLOBAL no es Send/Sync)
    }

    impl IDataObject_Impl for Datos_Impl {
        fn GetData(&self, f: *const FORMATETC) -> Result<STGMEDIUM> {
            unsafe {
                let f = &*f;
                if f.cfFormat != CF_HDROP || (f.tymed & TYMED_HGLOBAL.0 as u32) == 0 {
                    return Err(DV_E_FORMATETC.into());
                }
                let mut m = STGMEDIUM::default();
                m.tymed = TYMED_HGLOBAL.0 as u32;
                m.u.hGlobal = HGLOBAL(self.medio as *mut std::ffi::c_void);
                Ok(m)
            }
        }
        fn GetDataHere(&self, _f: *const FORMATETC, _m: *mut STGMEDIUM) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn QueryGetData(&self, f: *const FORMATETC) -> HRESULT {
            unsafe {
                let f = &*f;
                if f.cfFormat == CF_HDROP && (f.tymed & TYMED_HGLOBAL.0 as u32) != 0 {
                    S_OK
                } else {
                    DV_E_FORMATETC
                }
            }
        }
        fn GetCanonicalFormatEtc(&self, _i: *const FORMATETC, _o: *mut FORMATETC) -> HRESULT {
            E_NOTIMPL
        }
        fn SetData(&self, _f: *const FORMATETC, _m: *const STGMEDIUM, _r: windows::Win32::Foundation::BOOL) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn EnumFormatEtc(&self, _d: u32) -> Result<IEnumFORMATETC> {
            Err(E_NOTIMPL.into())
        }
        fn DAdvise(&self, _f: *const FORMATETC, _a: u32, _s: Option<&IAdviseSink>) -> Result<u32> {
            Err(E_NOTIMPL.into())
        }
        fn DUnadvise(&self, _c: u32) -> Result<()> { Err(E_NOTIMPL.into()) }
        fn EnumDAdvise(&self) -> Result<IEnumSTATDATA> { Err(E_NOTIMPL.into()) }
    }

    /// la fuente del arrastre: acaba al soltar el botón, se cancela con Esc
    #[implement(IDropSource)]
    struct Fuente;

    impl IDropSource_Impl for Fuente_Impl {
        fn QueryContinueDrag(&self, esc: windows::Win32::Foundation::BOOL,
                             teclas: MODIFIERKEYS_FLAGS) -> HRESULT {
            if esc.as_bool() {
                return DRAGDROP_S_CANCEL;
            }
            if (teclas.0 & MK_LBUTTON) == 0 {
                return DRAGDROP_S_DROP;
            }
            S_OK
        }
        fn GiveFeedback(&self, _e: DROPEFFECT) -> HRESULT {
            DRAGDROP_S_USEDEFAULTCURSORS
        }
    }

    pub fn arrastra(ruta: &Path) -> bool {
        unsafe {
            // CF_HDROP: cabecera DROPFILES + la ruta en UTF-16 con doble NUL
            let mut ancho: Vec<u16> = ruta.to_string_lossy().encode_utf16().collect();
            ancho.push(0);
            ancho.push(0);
            let cab = std::mem::size_of::<DROPFILES>();
            let bytes = cab + ancho.len() * 2;
            let Ok(h) = GlobalAlloc(GMEM_MOVEABLE, bytes) else { return false };
            let p = GlobalLock(h) as *mut u8;
            if p.is_null() {
                return false;
            }
            std::ptr::write_bytes(p, 0, bytes);
            let df = p as *mut DROPFILES;
            (*df).pFiles = cab as u32;
            (*df).fWide = true.into();
            (*df).pt = POINT { x: 0, y: 0 };
            std::ptr::copy_nonoverlapping(ancho.as_ptr(), p.add(cab) as *mut u16, ancho.len());
            let _ = GlobalUnlock(h);

            let datos: IDataObject = Datos { medio: h.0 as isize }.into();
            let fuente: IDropSource = Fuente.into();
            let mut efecto = DROPEFFECT::default();
            let r = DoDragDrop(&datos, &fuente, DROPEFFECT_COPY, &mut efecto);
            r == DRAGDROP_S_DROP
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn arrastra(_v: &winit::window::Window, _r: &Path, _m: Option<&[u8]>) -> bool {
    false
}
