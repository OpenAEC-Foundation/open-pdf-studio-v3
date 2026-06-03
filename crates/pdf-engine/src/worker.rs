//! The engine worker loop. Owns the PDFium engine and the current document, and
//! lives on its own thread because PDFium is single-threaded and not `Send`.

use crate::library::init_pdfium;
use crate::render::render_page_bytes;
use crate::types::{Command, Event};
use pdfium_render::prelude::*;
use std::collections::HashSet;
use std::sync::mpsc::Receiver;

/// Clamp render resolution so extreme zoom can't allocate enormous bitmaps.
const MIN_RENDER_WIDTH: i32 = 300;
// Caps the rasterized page width. Pages stay pin-sharp up to ~400% (base 1100px),
// and beyond that the bitmap is scaled up — this bounds per-page bitmap memory
// (a full A4 at this width is ~110 MB) while still allowing zoom to 800%.
const MAX_RENDER_WIDTH: i32 = 4400;
/// Aspect ratio (h/w) to assume if a page's real size can't be read (≈ A4).
const FALLBACK_ASPECT: f32 = 1.4142;

/// Run the worker until the command channel is closed (all senders dropped).
/// `emit` is called for every [`Event`] produced.
pub(crate) fn run(rx: Receiver<Command>, emit: Box<dyn Fn(Event) + Send>, initial_width: i32) {
    let mut target_width = initial_width;

    let pdfium = match init_pdfium() {
        Ok(p) => p,
        Err(e) => {
            emit(Event::Error(format!("PDFium failed to load: {e}")));
            return;
        }
    };

    // Current document (borrows `pdfium` for the thread's lifetime), its page
    // count, and the set of pages already rendered at the current resolution.
    let mut doc: Option<PdfDocument<'_>> = None;
    let mut total: i32 = 0;
    let mut rendered: HashSet<i32> = HashSet::new();

    while let Ok(cmd) = rx.recv() {
        match cmd {
            Command::Open(path) => match pdfium.load_pdf_from_file(&path, None) {
                Ok(d) => {
                    let count = d.pages().len();

                    // Cheap pass: read page sizes (no rasterization) so the UI
                    // can size placeholders and the scrollbar correctly.
                    let mut page_aspects: Vec<f32> = Vec::with_capacity(count as usize);
                    for i in 0..count {
                        let aspect = match d.pages().get(i) {
                            Ok(p) => {
                                let w = p.width().value;
                                let h = p.height().value;
                                if w > 0.0 { h / w } else { FALLBACK_ASPECT }
                            }
                            Err(_) => FALLBACK_ASPECT,
                        };
                        page_aspects.push(aspect);
                    }

                    let name = path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("document")
                        .to_string();
                    eprintln!("[engine] opened {name}: {count} page(s)");

                    emit(Event::Opened { name, page_aspects });
                    doc = Some(d);
                    total = count;
                    rendered.clear();
                }
                Err(e) => {
                    eprintln!("[engine] open failed {path:?}: {e:?}");
                    emit(Event::Error(format!("Failed to open {}: {e:?}", path.display())));
                }
            },

            Command::SetRenderWidth(w) => {
                let w = w.clamp(MIN_RENDER_WIDTH, MAX_RENDER_WIDTH);
                if w != target_width {
                    target_width = w;
                    rendered.clear(); // re-render on demand at the new resolution
                    eprintln!("[engine] render width -> {target_width}px");
                }
            }

            Command::Render(index) => {
                if index < 0 || index >= total || rendered.contains(&index) {
                    continue;
                }
                let Some(d) = doc.as_ref() else { continue };
                match render_page_bytes(d, index, target_width) {
                    Ok((width, height, rgba)) => {
                        rendered.insert(index);
                        emit(Event::PageRendered { index, width, height, rgba });
                        eprintln!("[engine] rendered page {}", index + 1);
                    }
                    Err(e) => emit(Event::Error(e)),
                }
            }
        }
    }
    eprintln!("[engine] worker exiting");
}
