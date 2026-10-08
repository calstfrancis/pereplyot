use std::path::PathBuf;
use std::sync::OnceLock;

use pdfium_render::prelude::Pdfium;

use crate::pdfium_lock::LockedBindings;

static PDFIUM: OnceLock<Result<Pdfium, String>> = OnceLock::new();

/// The process's PDFium, bound once. Unlike `fond_doc::bind_pdfium`, every call through it holds
/// one lock, so the render thread and the GTK thread can both use it safely.
pub fn get() -> Result<&'static Pdfium, String> {
    PDFIUM
        .get_or_init(|| {
            let bindings = match std::env::var("PDFIUM_LIB_PATH") {
                Ok(val) => {
                    let given = PathBuf::from(&val);
                    let lib = if given.is_dir() {
                        Pdfium::pdfium_platform_library_name_at_path(&given)
                    } else {
                        given
                    };
                    Pdfium::bind_to_library(&lib).map_err(|e| format!("{}: {e}", lib.display()))?
                }
                Err(_) => Pdfium::bind_to_system_library().map_err(|e| e.to_string())?,
            };
            Ok(Pdfium::new(Box::new(LockedBindings(bindings))))
        })
        .as_ref()
        .map_err(|e| e.clone())
}
