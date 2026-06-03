//! The open-documents store: one entry per UI tab.
//!
//! This lives in a `thread_local` because it is shared between the UI callback
//! handlers (in `main`) and the engine event bridge (`bridge`) — both of which
//! run on the Slint event-loop thread. Keeping it thread-local sidesteps the
//! `Send` requirement on the engine sink while staying single-threaded and safe.

use crate::PageItem;
use pdf_engine::DocMeta;
use slint::VecModel;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

/// Sentinel `active-doc-id` meaning "no document open".
pub const NO_DOC: i32 = -1;

/// One open document = one tab.
pub struct Tab {
    pub id: u32,
    pub path: PathBuf,
    pub name: String,
    /// The page model bound to the UI when this tab is active. Kept alive per
    /// tab so already-rendered bitmaps survive tab switches.
    pub pages: Rc<VecModel<PageItem>>,
    /// Small page thumbnails for the navigation panel (one slot per page).
    pub thumbs: Rc<VecModel<PageItem>>,
    pub total: i32,
    /// Properties-panel data (read once on open).
    pub meta: DocMeta,
    pub page_sizes: Vec<(f32, f32)>,
    pub file_size: u64,
    // Per-tab view state, saved when switching away and restored on return.
    pub zoom: f32,
    pub display_mode: i32,
    pub current_page: i32,
    pub scroll_x: f32,
    pub scroll_y: f32,
}

/// All open tabs plus which one is active.
#[derive(Default)]
pub struct DocStore {
    pub tabs: Vec<Tab>,
    pub active: usize, // index into `tabs`; only meaningful when `!tabs.is_empty()`
    next_id: u32,
}

impl DocStore {
    pub fn index_of(&self, id: u32) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }
    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }
    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active)
    }
    /// Allocate the next document id.
    pub fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

thread_local! {
    static STORE: RefCell<DocStore> = RefCell::new(DocStore::default());
}

/// Run `f` with shared access to the store.
pub fn with<R>(f: impl FnOnce(&DocStore) -> R) -> R {
    STORE.with(|s| f(&s.borrow()))
}

/// Run `f` with mutable access to the store.
pub fn with_mut<R>(f: impl FnOnce(&mut DocStore) -> R) -> R {
    STORE.with(|s| f(&mut s.borrow_mut()))
}

/// A fresh, empty page model for a new tab.
pub fn empty_pages() -> Rc<VecModel<PageItem>> {
    Rc::new(VecModel::<PageItem>::default())
}
