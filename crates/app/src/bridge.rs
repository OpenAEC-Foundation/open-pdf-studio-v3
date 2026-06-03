//! The one place the UI and the engine meet: it translates engine [`Event`]s
//! into per-tab Slint model updates, marshalling them onto the Slint event loop.

use crate::{docs, AppWindow, PageItem};
use pdf_engine::Event;
use slint::{Image, Model, Rgba8Pixel, SharedPixelBuffer, SharedString, Weak};

/// Aspect ratio (h/w) fallback, matching the engine's.
const FALLBACK_ASPECT: f32 = 1.4142;

/// Build a Slint image from RGBA bytes (runs on the UI thread).
fn make_image(width: u32, height: u32, rgba: &[u8]) -> Image {
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
    buffer.make_mut_bytes().copy_from_slice(rgba);
    Image::from_rgba8(buffer)
}

/// Returns the engine event sink. It is called on the engine's worker thread and
/// forwards each event onto the Slint event loop via `upgrade_in_event_loop`.
pub fn event_sink(weak: Weak<AppWindow>) -> impl Fn(Event) + Send + 'static {
    move |event| match event {
        Event::Opened { id, name, page_aspects, page_sizes, meta } => {
            let _ = weak.upgrade_in_event_loop(move |ui| {
                let count = page_aspects.len() as i32;

                // Populate this document's page model with sized placeholders.
                let is_active = docs::with_mut(|store| {
                    let Some(idx) = store.index_of(id) else { return false };
                    let tab = &mut store.tabs[idx];
                    let placeholders = |aspects: &[f32]| -> Vec<PageItem> {
                        aspects
                            .iter()
                            .map(|&aspect| PageItem { image: Image::default(), aspect, loaded: false })
                            .collect()
                    };
                    tab.pages.set_vec(placeholders(&page_aspects));
                    tab.thumbs.set_vec(placeholders(&page_aspects));
                    tab.total = count;
                    tab.name = name.clone();
                    tab.meta = meta.clone();
                    tab.page_sizes = page_sizes.clone();
                    idx == store.active
                });

                crate::rebuild_doc_tabs(&ui);

                // Only touch the visible view if the opened doc is the active tab.
                if is_active {
                    ui.set_total_pages(count);
                    ui.set_current_page(1);
                    ui.invoke_scroll_to_y(0.0);
                    ui.set_status(SharedString::from(format!(
                        "{name}  —  {count} pages  (scroll to load)"
                    )));
                    crate::refresh_props(&ui);
                }
            });
        }

        Event::PageRendered { id, index, width, height, rgba } => {
            let _ = weak.upgrade_in_event_loop(move |_ui| {
                docs::with(|store| {
                    let Some(idx) = store.index_of(id) else { return };
                    let model = &store.tabs[idx].pages;
                    // The document may have changed since this was queued; bounds-check.
                    if (index as usize) < model.row_count() {
                        let aspect = if width > 0 {
                            height as f32 / width as f32
                        } else {
                            FALLBACK_ASPECT
                        };
                        model.set_row_data(
                            index as usize,
                            PageItem { image: make_image(width, height, &rgba), aspect, loaded: true },
                        );
                    }
                });
            });
        }

        Event::ThumbRendered { id, index, width, height, rgba } => {
            let _ = weak.upgrade_in_event_loop(move |_ui| {
                docs::with(|store| {
                    let Some(idx) = store.index_of(id) else { return };
                    let model = &store.tabs[idx].thumbs;
                    if (index as usize) < model.row_count() {
                        let aspect = if width > 0 {
                            height as f32 / width as f32
                        } else {
                            FALLBACK_ASPECT
                        };
                        model.set_row_data(
                            index as usize,
                            PageItem { image: make_image(width, height, &rgba), aspect, loaded: true },
                        );
                    }
                });
            });
        }

        Event::Error { message, .. } => {
            let _ = weak.upgrade_in_event_loop(move |ui| ui.set_status(SharedString::from(message)));
        }
    }
}
