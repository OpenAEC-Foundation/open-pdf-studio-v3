//! The one place the UI and the engine meet: it translates engine [`Event`]s
//! into Slint model updates, marshalling them onto the Slint event loop.

use crate::{AppWindow, PageItem};
use pdf_engine::Event;
use slint::{
    Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel, Weak,
};
use std::rc::Rc;

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
        Event::Opened { name, page_aspects } => {
            let _ = weak.upgrade_in_event_loop(move |ui| {
                let count = page_aspects.len();
                let items: Vec<PageItem> = page_aspects
                    .into_iter()
                    .map(|aspect| PageItem {
                        image: Image::default(),
                        aspect,
                        loaded: false,
                    })
                    .collect();
                ui.set_pages(ModelRc::from(Rc::new(VecModel::from(items))));
                ui.set_total_pages(count as i32);
                ui.set_current_page(1);
                ui.invoke_scroll_to_y(0.0); // start a freshly opened document at the top
                ui.set_status(SharedString::from(format!(
                    "{name}  —  {count} pages  (scroll to load)"
                )));
            });
        }

        Event::PageRendered { index, width, height, rgba } => {
            let _ = weak.upgrade_in_event_loop(move |ui| {
                let model = ui.get_pages();
                // The document may have changed since this was queued; bounds-check.
                if (index as usize) < model.row_count() {
                    let aspect = if width > 0 {
                        height as f32 / width as f32
                    } else {
                        FALLBACK_ASPECT
                    };
                    model.set_row_data(
                        index as usize,
                        PageItem {
                            image: make_image(width, height, &rgba),
                            aspect,
                            loaded: true,
                        },
                    );
                }
            });
        }

        Event::Error(message) => {
            let _ = weak.upgrade_in_event_loop(move |ui| ui.set_status(SharedString::from(message)));
        }
    }
}
