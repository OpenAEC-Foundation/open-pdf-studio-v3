//! Locating and loading the PDFium dynamic library.

use pdfium_render::prelude::*;
use std::path::PathBuf;

/// Find `pdfium.dll` (or the platform equivalent) in the current directory or
/// next to the running binary. The crate does not bundle PDFium, so it must be
/// shipped alongside the application.
fn locate_pdfium() -> Option<PathBuf> {
    let lib_name = Pdfium::pdfium_platform_library_name(); // pdfium.dll / libpdfium.so / .dylib

    // Search the working dir (dev) and the locations the various installers place
    // bundled resources relative to the executable:
    //   Windows (NSIS/WiX): next to the .exe
    //   macOS (.app):       Contents/MacOS/../Resources
    //   Linux (deb):        /usr/bin/.. -> /usr/lib/<app>
    let mut dirs: Vec<PathBuf> = vec![PathBuf::from(".")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for rel in [
                "",
                "pdfium-lib",
                "../Resources",
                "../Resources/pdfium-lib",
                "../lib",
                "../lib/open-pdf-studio3",
                "../lib/open-pdf-studio3/pdfium-lib",
            ] {
                dirs.push(if rel.is_empty() { dir.to_path_buf() } else { dir.join(rel) });
            }
        }
    }

    dirs.into_iter()
        .map(|d| d.join(&lib_name))
        .find(|p| p.exists())
}

/// Load PDFium and wrap it in a `Pdfium` handle, falling back to a
/// system-installed library if no local copy is found.
pub(crate) fn init_pdfium() -> Result<Pdfium, String> {
    let bindings = match locate_pdfium() {
        Some(path) => Pdfium::bind_to_library(&path)
            .map_err(|e| format!("Failed to load PDFium from {path:?}: {e:?}"))?,
        None => Pdfium::bind_to_system_library()
            .map_err(|e| format!("pdfium library not found (no local dll, no system lib): {e:?}"))?,
    };
    Ok(Pdfium::new(bindings))
}
