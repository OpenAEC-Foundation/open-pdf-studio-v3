//! The engine's message types. These are deliberately plain data with no UI
//! dependency, so the engine stays decoupled from whatever front-end drives it.
//!
//! The engine is multi-document: every command and event carries a `doc_id`
//! (assigned by the caller) so a single worker thread can hold several open
//! documents at once (one per UI tab).

use std::path::PathBuf;

/// Read-only document metadata (from the PDF's info dictionary), shown in the
/// Properties panel. Empty strings mean the field is absent.
#[derive(Default, Clone)]
pub struct DocMeta {
    pub title: String,
    pub author: String,
    pub subject: String,
    pub keywords: String,
    pub creator: String,
    pub producer: String,
    pub created: String,
    pub modified: String,
    pub version: String,
}

/// Commands sent from the application to the engine worker.
///
/// Not public: callers use the typed methods on [`crate::Engine`] instead.
pub(crate) enum Command {
    /// Open a document under `id`; emits [`Event::Opened`] (or [`Event::Error`]).
    Open { id: u32, path: PathBuf },
    /// Render a single page of document `id`; emits [`Event::PageRendered`].
    Render { id: u32, index: i32 },
    /// Render a small thumbnail of one page; emits [`Event::ThumbRendered`].
    RenderThumb { id: u32, index: i32 },
    /// Change the render resolution (page width px) for future renders of `id`.
    SetRenderWidth { id: u32, width: i32 },
    /// Close document `id` and free its memory.
    Close { id: u32 },
}

/// Events emitted by the engine. Delivered on the engine's worker thread — the
/// front-end is responsible for marshalling them to its own thread if needed.
pub enum Event {
    /// A document opened successfully.
    Opened {
        /// The document id this event refers to.
        id: u32,
        /// Display name (file name).
        name: String,
        /// One aspect ratio (height / width) per page, in page order. Lets the
        /// UI lay out correctly-sized placeholders before pages are rendered.
        page_aspects: Vec<f32>,
        /// Each page's size in PDF points (width, height), in page order.
        page_sizes: Vec<(f32, f32)>,
        /// Document metadata (for the Properties panel).
        meta: DocMeta,
    },
    /// A page finished rendering to an RGBA8 bitmap.
    PageRendered {
        id: u32,
        index: i32,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    /// A page thumbnail finished rendering to an RGBA8 bitmap.
    ThumbRendered {
        id: u32,
        index: i32,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    /// A non-fatal error to surface to the user (`id` names the document, if any).
    Error { id: Option<u32>, message: String },
}
