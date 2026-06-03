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
//! engine.open("document.pdf".into());
//! engine.render(0);
//! ```

mod library;
mod render;
mod types;
mod worker;

pub use types::Event;

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

    /// Open a document (emits [`Event::Opened`] or [`Event::Error`]).
    pub fn open(&self, path: PathBuf) {
        let _ = self.tx.send(Command::Open(path));
    }

    /// Request a page render (emits [`Event::PageRendered`]). Already-rendered
    /// pages at the current resolution are ignored.
    pub fn render(&self, index: i32) {
        let _ = self.tx.send(Command::Render(index));
    }

    /// Set the render resolution (page width in px); subsequent renders use it.
    pub fn set_render_width(&self, width: i32) {
        let _ = self.tx.send(Command::SetRenderWidth(width));
    }
}
