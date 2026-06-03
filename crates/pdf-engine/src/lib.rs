//! UI-agnostic PDF engine for Open PDF Studio.
//!
//! Wraps Google's PDFium (via `pdfium-render`) on a dedicated worker thread and
//! exposes a small message-passing API. The engine knows nothing about the GUI:
//! it accepts [`Command`]s (through typed [`Engine`] methods) and produces
//! [`Event`]s (delivered to a sink callback). This keeps the rendering core
//! reusable and independently testable as the application grows.
//!
//! ```no_run
//! let engine = pdf_engine::Engine::spawn(
//!     |event| { /* handle event on the worker thread */ },
//!     pdf_engine::BASE_RENDER_WIDTH,
//! );
//! engine.open(0, "document.pdf".into());
//! engine.render(0, 0);
//! ```

mod library;
mod render;
mod types;
mod worker;

pub use types::{DocMeta, Event};

use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::thread;
use types::Command;

/// Render width (px) at 100% zoom. Keep in sync with `base-width` in app.slint.
pub const BASE_RENDER_WIDTH: i32 = 1100;

/// Handle to the background PDF engine. Cheap to clone; all clones drive the
/// same worker thread. The worker shuts down when the last handle is dropped.
#[derive(Clone)]
pub struct Engine {
    tx: Sender<Command>,
}

impl Engine {
    /// Spawn the engine worker thread.
    ///
    /// `sink` is invoked for every [`Event`] **on the worker thread**; the
    /// caller is responsible for marshalling to its own (e.g. UI) thread.
    /// `base_render_width` is the initial render resolution.
    pub fn spawn(sink: impl Fn(Event) + Send + 'static, base_render_width: i32) -> Engine {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || worker::run(rx, Box::new(sink), base_render_width));
        Engine { tx }
    }

    /// Open a document under `id` (emits [`Event::Opened`] or [`Event::Error`]).
    pub fn open(&self, id: u32, path: PathBuf) {
        let _ = self.tx.send(Command::Open { id, path });
    }

    /// Request a page render for document `id` (emits [`Event::PageRendered`]).
    /// Already-rendered pages at the document's current resolution are ignored.
    pub fn render(&self, id: u32, index: i32) {
        let _ = self.tx.send(Command::Render { id, index });
    }

    /// Request a small thumbnail render for document `id`, page `index` (emits
    /// [`Event::ThumbRendered`]). Thumbnails are a fixed small size, independent
    /// of zoom, and cached per document.
    pub fn render_thumb(&self, id: u32, index: i32) {
        let _ = self.tx.send(Command::RenderThumb { id, index });
    }

    /// Set the render resolution (page width in px) for document `id`; subsequent
    /// renders of that document use it. Changing it re-renders on demand.
    pub fn set_render_width(&self, id: u32, width: i32) {
        let _ = self.tx.send(Command::SetRenderWidth { id, width });
    }

    /// Close document `id`, freeing its memory.
    pub fn close(&self, id: u32) {
        let _ = self.tx.send(Command::Close { id });
    }
}
