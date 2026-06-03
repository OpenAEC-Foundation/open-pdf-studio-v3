//! The engine's message types. These are deliberately plain data with no UI
//! dependency, so the engine stays decoupled from whatever front-end drives it.

use std::path::PathBuf;

/// Commands sent from the application to the engine worker.
///
/// Not public: callers use the typed methods on [`crate::Engine`] instead.
pub(crate) enum Command {
    /// Open a document; emits [`Event::Opened`] (or [`Event::Error`]).
    Open(PathBuf),
    /// Render a single page; emits [`Event::PageRendered`] on success.
    Render(i32),
    /// Change the render resolution (page width in px) for future renders.
    SetRenderWidth(i32),
}

/// Events emitted by the engine. Delivered on the engine's worker thread — the
/// front-end is responsible for marshalling them to its own thread if needed.
pub enum Event {
    /// A document opened successfully.
    Opened {
        /// Display name (file name).
        name: String,
        /// One aspect ratio (height / width) per page, in page order. Lets the
        /// UI lay out correctly-sized placeholders before pages are rendered.
        page_aspects: Vec<f32>,
    },
    /// A page finished rendering to an RGBA8 bitmap.
    PageRendered {
        index: i32,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    /// A non-fatal error to surface to the user.
    Error(String),
}
