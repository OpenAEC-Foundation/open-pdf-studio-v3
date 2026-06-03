//! The engine worker loop. Owns the PDFium engine and every open document, and
//! lives on its own thread because PDFium is single-threaded and not `Send`.
//!
//! A single PDFium instance holds many `PdfDocument`s keyed by a caller-assigned
//! `doc_id`, so the app can have several files open at once (one per tab).

use crate::library::init_pdfium;
use crate::render::render_page_bytes;
use crate::types::{Command, DocMeta, Event};
use pdfium_render::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Receiver;

/// A PDF version enum → display string ("1.7", "2.0", …).
fn version_string(v: PdfDocumentVersion) -> String {
    use PdfDocumentVersion::*;
    match v {
        Pdf1_0 => "1.0",
        Pdf1_1 => "1.1",
        Pdf1_2 => "1.2",
        Pdf1_3 => "1.3",
        Pdf1_4 => "1.4",
        Pdf1_5 => "1.5",
        Pdf1_6 => "1.6",
        Pdf1_7 => "1.7",
        Pdf2_0 => "2.0",
        _ => "",
    }
    .to_string()
}

/// Clamp render resolution so extreme zoom can't allocate enormous bitmaps.
const MIN_RENDER_WIDTH: i32 = 300;
// Caps the rasterized page width. Pages stay pin-sharp up to ~400% (base 1100px),
// and beyond that the bitmap is scaled up — this bounds per-page bitmap memory
// (a full A4 at this width is ~110 MB) while still allowing zoom to 800%.
const MAX_RENDER_WIDTH: i32 = 4400;
/// Aspect ratio (h/w) to assume if a page's real size can't be read (≈ A4).
const FALLBACK_ASPECT: f32 = 1.4142;
/// Fixed width (px) for navigation-panel thumbnails (independent of zoom).
const THUMB_WIDTH: i32 = 220;

/// One open document plus the per-document render state.
struct DocEntry<'a> {
    doc: PdfDocument<'a>,
    total: i32,
    /// Render width (px) for this document; tracks its tab's zoom.
    width: i32,
    /// Pages already rendered at the current `width`.
    rendered: HashSet<i32>,
    /// Pages whose thumbnail has been rendered (fixed size, never invalidated).
    thumb_rendered: HashSet<i32>,
}

/// Run the worker until the command channel is closed (all senders dropped).
/// `emit` is called for every [`Event`] produced. `default_width` is the render
/// width a freshly opened document starts at (i.e. 100% zoom).
pub(crate) fn run(rx: Receiver<Command>, emit: Box<dyn Fn(Event) + Send>, default_width: i32) {
    let pdfium = match init_pdfium() {
        Ok(p) => p,
        Err(e) => {
            emit(Event::Error { id: None, message: format!("PDFium failed to load: {e}") });
            return;
        }
    };

    // All open documents, keyed by the caller's doc id. Each borrows `pdfium`
    // (shared, immutable) for the worker thread's lifetime.
    let mut docs: HashMap<u32, DocEntry> = HashMap::new();

    while let Ok(cmd) = rx.recv() {
        match cmd {
            Command::Open { id, path } => match pdfium.load_pdf_from_file(&path, None) {
                Ok(d) => {
                    let count = d.pages().len();

                    // Cheap pass: read page sizes (no rasterization) so the UI
                    // can size placeholders/scrollbar and show page dimensions.
                    let mut page_aspects: Vec<f32> = Vec::with_capacity(count as usize);
                    let mut page_sizes: Vec<(f32, f32)> = Vec::with_capacity(count as usize);
                    for i in 0..count {
                        let (w, h) = match d.pages().get(i) {
                            Ok(p) => (p.width().value, p.height().value),
                            Err(_) => (0.0, 0.0),
                        };
                        page_aspects.push(if w > 0.0 { h / w } else { FALLBACK_ASPECT });
                        page_sizes.push((w, h));
                    }

                    // Document metadata for the Properties panel.
                    let md = d.metadata();
                    let tag = |t| md.get(t).map(|v| v.value().to_string()).unwrap_or_default();
                    let meta = DocMeta {
                        title: tag(PdfDocumentMetadataTagType::Title),
                        author: tag(PdfDocumentMetadataTagType::Author),
                        subject: tag(PdfDocumentMetadataTagType::Subject),
                        keywords: tag(PdfDocumentMetadataTagType::Keywords),
                        creator: tag(PdfDocumentMetadataTagType::Creator),
                        producer: tag(PdfDocumentMetadataTagType::Producer),
                        created: tag(PdfDocumentMetadataTagType::CreationDate),
                        modified: tag(PdfDocumentMetadataTagType::ModificationDate),
                        version: version_string(d.version()),
                    };

                    let name = path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("document")
                        .to_string();
                    eprintln!("[engine] opened #{id} {name}: {count} page(s)");

                    emit(Event::Opened { id, name, page_aspects, page_sizes, meta });
                    docs.insert(
                        id,
                        DocEntry {
                            doc: d,
                            total: count,
                            width: default_width,
                            rendered: HashSet::new(),
                            thumb_rendered: HashSet::new(),
                        },
                    );
                }
                Err(e) => {
                    eprintln!("[engine] open failed {path:?}: {e:?}");
                    emit(Event::Error {
                        id: Some(id),
                        message: format!("Failed to open {}: {e:?}", path.display()),
                    });
                }
            },

            Command::SetRenderWidth { id, width } => {
                let w = width.clamp(MIN_RENDER_WIDTH, MAX_RENDER_WIDTH);
                if let Some(entry) = docs.get_mut(&id) {
                    if w != entry.width {
                        entry.width = w;
                        entry.rendered.clear(); // re-render on demand at the new resolution
                    }
                }
            }

            Command::Render { id, index } => {
                let Some(entry) = docs.get_mut(&id) else { continue };
                if index < 0 || index >= entry.total || entry.rendered.contains(&index) {
                    continue;
                }
                match render_page_bytes(&entry.doc, index, entry.width) {
                    Ok((width, height, rgba)) => {
                        entry.rendered.insert(index);
                        emit(Event::PageRendered { id, index, width, height, rgba });
                    }
                    Err(e) => emit(Event::Error { id: Some(id), message: e }),
                }
            }

            Command::RenderThumb { id, index } => {
                let Some(entry) = docs.get_mut(&id) else { continue };
                if index < 0 || index >= entry.total || entry.thumb_rendered.contains(&index) {
                    continue;
                }
                match render_page_bytes(&entry.doc, index, THUMB_WIDTH) {
                    Ok((width, height, rgba)) => {
                        entry.thumb_rendered.insert(index);
                        emit(Event::ThumbRendered { id, index, width, height, rgba });
                    }
                    Err(e) => emit(Event::Error { id: Some(id), message: e }),
                }
            }

            Command::Close { id } => {
                docs.remove(&id);
                eprintln!("[engine] closed #{id}");
            }
        }
    }
    eprintln!("[engine] worker exiting");
}
