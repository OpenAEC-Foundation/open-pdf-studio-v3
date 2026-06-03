// Open PDF Studio — application entry point.
//
// This binary is the *app* layer: it owns the Slint window, wires UI callbacks
// to the engine, bridges engine events back to the UI, and persists settings.
// All PDF work lives in the `pdf-engine` crate, which has no UI dependency.

// Use the Windows GUI subsystem in release builds so launching the app does
// not spawn a console window (debug builds keep the console for logs/panics).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

mod bridge;
mod docs;
mod settings;

use docs::{Tab, NO_DOC};
use pdf_engine::{Engine, BASE_RENDER_WIDTH};
use settings::Settings;
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

// Page-geometry constants — must match `base-width` and the +16px row gap in
// app.slint, so page offsets computed here line up with the on-screen layout.
const BASE_WIDTH: f32 = 1100.0;
const ROW_GAP: f32 = 16.0;
const FALLBACK_ASPECT: f32 = 1.4142;

/// Cumulative top offset (px) of page `index` (0-based), given the current zoom.
fn page_offset(pages: &slint::ModelRc<PageItem>, index: usize, zoom: f32) -> f32 {
    (0..index)
        .map(|i| {
            let aspect = pages.row_data(i).map(|p| p.aspect).unwrap_or(FALLBACK_ASPECT);
            BASE_WIDTH * zoom * aspect + ROW_GAP
        })
        .sum()
}

/// Rebuild the Slint recent-files model from settings, applying `filter`.
fn rebuild_recents(ui: &AppWindow, settings: &Settings, filter: &str) {
    let needle = filter.to_lowercase();
    let items: Vec<RecentFile> = settings
        .recent_files
        .iter()
        .filter(|p| needle.is_empty() || p.to_lowercase().contains(&needle))
        .map(|p| RecentFile {
            name: SharedString::from(
                Path::new(p).file_name().and_then(|s| s.to_str()).unwrap_or("(unknown)"),
            ),
            path: SharedString::from(p.as_str()),
        })
        .collect();
    ui.set_recent_files(ModelRc::from(Rc::new(VecModel::from(items))));
}

/// Rebuild the document-tab strip from the open-documents store.
pub(crate) fn rebuild_doc_tabs(ui: &AppWindow) {
    let items: Vec<DocTabItem> = docs::with(|s| {
        s.tabs
            .iter()
            .enumerate()
            .map(|(i, t)| DocTabItem {
                id: t.id as i32,
                name: SharedString::from(t.name.as_str()),
                active: i == s.active,
            })
            .collect()
    });
    ui.set_open_docs(ModelRc::from(Rc::new(VecModel::from(items))));
}

/// Human-readable file size.
fn fmt_size(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1_048_576.0 {
        format!("{:.1} MB", b / 1_048_576.0)
    } else if b >= 1024.0 {
        format!("{:.0} KB", b / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

/// Reformat a PDF date (`D:YYYYMMDDHHmmSS…`) as `YYYY-MM-DD HH:MM`; pass through
/// anything that doesn't match.
fn fmt_pdf_date(s: &str) -> String {
    let t = s.strip_prefix("D:").unwrap_or(s);
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 12 {
        format!("{}-{}-{} {}:{}", &digits[0..4], &digits[4..6], &digits[6..8], &digits[8..10], &digits[10..12])
    } else if digits.len() >= 8 {
        format!("{}-{}-{}", &digits[0..4], &digits[4..6], &digits[6..8])
    } else {
        s.to_string()
    }
}

/// Friendly paper-size name for a page (in points), if recognised.
fn paper_name(w: f32, h: f32) -> Option<&'static str> {
    let m = |a: f32, b: f32| {
        ((w - a).abs() < 3.0 && (h - b).abs() < 3.0) || ((w - b).abs() < 3.0 && (h - a).abs() < 3.0)
    };
    if m(595.0, 842.0) {
        Some("A4")
    } else if m(612.0, 792.0) {
        Some("Letter")
    } else if m(612.0, 1008.0) {
        Some("Legal")
    } else if m(842.0, 1191.0) {
        Some("A3")
    } else {
        None
    }
}

/// Set the Properties "Page size" field from the active tab's current page.
fn set_page_size(ui: &AppWindow) {
    let size = docs::with(|st| {
        st.active_tab().and_then(|t| {
            if t.page_sizes.is_empty() {
                return None;
            }
            let i = (ui.get_current_page() - 1).clamp(0, t.page_sizes.len() as i32 - 1) as usize;
            t.page_sizes.get(i).copied()
        })
    });
    // PDF points → millimetres (1 pt = 1/72 in = 25.4/72 mm).
    let text = match size {
        Some((w, h)) if w > 0.0 && h > 0.0 => {
            let mm = 25.4 / 72.0;
            let name = paper_name(w, h).map(|n| format!("  ({n})")).unwrap_or_default();
            format!("{:.1} × {:.1} mm{}", w * mm, h * mm, name)
        }
        _ => "".to_string(),
    };
    ui.set_prop_page_size(SharedString::from(text));
}

/// Rebuild the Properties panel from the active tab (or clear it if none).
pub(crate) fn refresh_props(ui: &AppWindow) {
    let props = docs::with(|st| {
        st.active_tab().map(|t| DocProps {
            file_name: SharedString::from(t.name.as_str()),
            file_path: SharedString::from(t.path.to_string_lossy().to_string()),
            file_size: SharedString::from(fmt_size(t.file_size)),
            title: SharedString::from(t.meta.title.as_str()),
            author: SharedString::from(t.meta.author.as_str()),
            subject: SharedString::from(t.meta.subject.as_str()),
            keywords: SharedString::from(t.meta.keywords.as_str()),
            creator: SharedString::from(t.meta.creator.as_str()),
            producer: SharedString::from(t.meta.producer.as_str()),
            created: SharedString::from(fmt_pdf_date(&t.meta.created)),
            modified: SharedString::from(fmt_pdf_date(&t.meta.modified)),
            version: SharedString::from(t.meta.version.as_str()),
            pages: SharedString::from(t.total.to_string()),
        })
    });
    ui.set_props(props.unwrap_or_default());
    set_page_size(ui);
}

/// Persist the current window size (in logical px) so it can be restored next launch.
fn save_window_size(ui: &AppWindow, settings: &Rc<RefCell<Settings>>) {
    let sz = ui.window().size();
    let scale = ui.window().scale_factor();
    if scale > 0.0 && sz.width > 0 && sz.height > 0 {
        let mut s = settings.borrow_mut();
        s.window_w = sz.width as f32 / scale;
        s.window_h = sz.height as f32 / scale;
        s.save();
    }
}

/// Save the live UI view (zoom / scroll / page / mode) into the active tab, so
/// it can be restored when the user switches back to it.
fn save_active_view(ui: &AppWindow) {
    docs::with_mut(|s| {
        if let Some(t) = s.active_tab_mut() {
            t.zoom = ui.get_zoom();
            t.display_mode = ui.get_display_mode();
            t.current_page = ui.get_current_page();
            t.scroll_x = ui.get_scroll_x();
            t.scroll_y = ui.get_scroll_y();
        }
    });
}

/// Push the active tab's stored view into the UI and render it. When there is no
/// active tab, switch to the empty "no document" state.
fn apply_active_tab(ui: &AppWindow, engine: &Engine) {
    let info = docs::with(|s| {
        s.active_tab().map(|t| {
            (t.id, t.pages.clone(), t.thumbs.clone(), t.total, t.zoom, t.display_mode, t.current_page, t.scroll_x, t.scroll_y)
        })
    });

    if let Some((id, pages, thumbs, total, zoom, mode, cur, scroll_x, scroll_y)) = info {
        ui.set_active_doc_id(id as i32);
        ui.set_display_mode(mode);
        ui.set_zoom(zoom);
        ui.set_pages(ModelRc::from(pages));
        ui.set_thumbs(ModelRc::from(thumbs));
        ui.set_total_pages(total);
        ui.set_current_page(cur);
        engine.set_render_width(id, (BASE_WIDTH * zoom).round() as i32);
        ui.invoke_update_render();
        ui.invoke_restore_scroll(scroll_x, scroll_y);
        if total > 0 {
            let name = docs::with(|s| s.active_tab().map(|t| t.name.clone()).unwrap_or_default());
            ui.set_status(SharedString::from(format!("{name}  —  {total} pages")));
        }
    } else {
        ui.set_active_doc_id(NO_DOC);
        ui.set_pages(ModelRc::from(Rc::new(VecModel::<PageItem>::default())));
        ui.set_thumbs(ModelRc::from(Rc::new(VecModel::<PageItem>::default())));
        ui.set_total_pages(0);
        ui.set_current_page(1);
        ui.set_status(SharedString::from("No document open — use File ▸ Open to load a PDF."));
    }
    rebuild_doc_tabs(ui);
    refresh_props(ui);
}

/// Open `path` in a new tab and make it the active document.
fn open_document(
    ui: &AppWindow,
    engine: &Engine,
    settings: &Rc<RefCell<Settings>>,
    recent_filter: &Rc<RefCell<String>>,
    path: PathBuf,
) {
    // If the file is already open, just switch to its tab (no duplicates).
    if let Some(id) = docs::with(|s| s.tabs.iter().find(|t| t.path == path).map(|t| t.id)) {
        select_tab(ui, engine, id);
        return;
    }

    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("document").to_string();
    let default_mode = ui.get_display_mode();

    // Preserve the outgoing tab's view before focus moves to the new one.
    save_active_view(ui);

    let id = docs::with_mut(|s| {
        let id = s.alloc_id();
        s.tabs.push(Tab {
            id,
            path: path.clone(),
            name: name.clone(),
            pages: docs::empty_pages(),
            thumbs: docs::empty_pages(),
            total: 0,
            meta: Default::default(),
            page_sizes: Vec::new(),
            file_size: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
            zoom: 1.0,
            display_mode: default_mode,
            current_page: 1,
            scroll_x: 0.0,
            scroll_y: 0.0,
        });
        s.active = s.tabs.len() - 1;
        id
    });

    apply_active_tab(ui, engine);
    ui.set_status(SharedString::from(format!("Opening {name} …")));

    {
        let mut st = settings.borrow_mut();
        st.push_recent(&path.to_string_lossy());
        st.save();
    }
    rebuild_recents(ui, &settings.borrow(), &recent_filter.borrow());

    engine.open(id, path);
}

/// Make the tab with `id` active (saving the current tab's view first).
fn select_tab(ui: &AppWindow, engine: &Engine, id: u32) {
    let need = docs::with(|s| match s.index_of(id) {
        Some(i) => i != s.active,
        None => false,
    });
    if !need {
        return;
    }
    save_active_view(ui);
    docs::with_mut(|s| {
        if let Some(i) = s.index_of(id) {
            s.active = i;
        }
    });
    apply_active_tab(ui, engine);
}

/// Close the tab with `id`, freeing its document, and activate a neighbour (or
/// the empty state if it was the last tab).
fn close_tab(ui: &AppWindow, engine: &Engine, id: u32) {
    engine.close(id);
    let removed_active = docs::with_mut(|s| {
        let Some(idx) = s.index_of(id) else { return false };
        let was_active = idx == s.active;
        s.tabs.remove(idx);
        if s.tabs.is_empty() {
            s.active = 0;
        } else if idx < s.active || s.active >= s.tabs.len() {
            s.active = s.active.saturating_sub(1).min(s.tabs.len() - 1);
        }
        was_active
    });

    if removed_active {
        apply_active_tab(ui, engine);
    } else {
        rebuild_doc_tabs(ui);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ui = AppWindow::new()?;
    ui.set_app_version(SharedString::from(env!("CARGO_PKG_VERSION")));

    // Spawn the engine; its events are marshalled to the UI by `bridge`.
    let engine = Engine::spawn(bridge::event_sink(ui.as_weak()), BASE_RENDER_WIDTH);

    // Persisted settings (Preferences + recents) and the live Find filter.
    let settings = Rc::new(RefCell::new(Settings::load()));
    let recent_filter = Rc::new(RefCell::new(String::new()));

    // Restore the last window size (logical px) if we have one saved.
    {
        let s = settings.borrow();
        if s.window_w >= 200.0 && s.window_h >= 200.0 {
            ui.window().set_size(slint::LogicalSize::new(s.window_w, s.window_h));
        }
    }

    // Save the window size when the OS close button is used.
    ui.window().on_close_requested({
        let weak = ui.as_weak();
        let settings = settings.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                save_window_size(&ui, &settings);
            }
            let _ = slint::quit_event_loop();
            slint::CloseRequestResponse::HideWindow
        }
    });

    // Debounced zoom: pages scale live while zooming; the engine only re-renders
    // once the zoom settles (timer fires), avoiding a redraw on every step.
    let pending_render_width = Rc::new(Cell::new(BASE_RENDER_WIDTH));
    let zoom_timer = Rc::new(Timer::default());

    // "Open…" / Browse / "+" → native file dialog → load the chosen file (new tab).
    ui.on_open_file({
        let engine = engine.clone();
        let weak = ui.as_weak();
        let settings = settings.clone();
        let recent_filter = recent_filter.clone();
        move || {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("PDF documents", &["pdf"])
                .set_title("Open a PDF")
                .pick_file()
            {
                if let Some(ui) = weak.upgrade() {
                    open_document(&ui, &engine, &settings, &recent_filter, path);
                }
            }
        }
    });

    // Backstage: open a recent file by path (new tab).
    ui.on_open_path({
        let engine = engine.clone();
        let weak = ui.as_weak();
        let settings = settings.clone();
        let recent_filter = recent_filter.clone();
        move |path: SharedString| {
            if let Some(ui) = weak.upgrade() {
                open_document(&ui, &engine, &settings, &recent_filter, PathBuf::from(path.to_string()));
            }
        }
    });

    // Document tabs: switch / close.
    ui.on_select_doc({
        let engine = engine.clone();
        let weak = ui.as_weak();
        move |id: i32| {
            if id >= 0 {
                if let Some(ui) = weak.upgrade() {
                    select_tab(&ui, &engine, id as u32);
                }
            }
        }
    });
    ui.on_close_doc({
        let engine = engine.clone();
        let weak = ui.as_weak();
        move |id: i32| {
            if id >= 0 {
                if let Some(ui) = weak.upgrade() {
                    close_tab(&ui, &engine, id as u32);
                }
            }
        }
    });

    // Thumbnail panel: a thumbnail slot scrolled into view → render it (lazy).
    ui.on_request_thumb({
        let engine = engine.clone();
        let weak = ui.as_weak();
        move |index: i32| {
            if let Some(ui) = weak.upgrade() {
                let id = ui.get_active_doc_id();
                if id >= 0 && index >= 0 {
                    engine.render_thumb(id as u32, index);
                }
            }
        }
    });

    // Backstage: remove a recent entry.
    ui.on_remove_recent({
        let weak = ui.as_weak();
        let settings = settings.clone();
        let recent_filter = recent_filter.clone();
        move |path: SharedString| {
            {
                let mut s = settings.borrow_mut();
                s.remove_recent(path.as_str());
                s.save();
            }
            if let Some(ui) = weak.upgrade() {
                rebuild_recents(&ui, &settings.borrow(), &recent_filter.borrow());
            }
        }
    });

    // Backstage: filter the recents list (Find box).
    ui.on_filter_recents({
        let weak = ui.as_weak();
        let settings = settings.clone();
        let recent_filter = recent_filter.clone();
        move |text: SharedString| {
            *recent_filter.borrow_mut() = text.to_string();
            if let Some(ui) = weak.upgrade() {
                rebuild_recents(&ui, &settings.borrow(), &recent_filter.borrow());
            }
        }
    });

    // Preferences: persist the current values to disk.
    ui.on_save_prefs({
        let weak = ui.as_weak();
        let settings = settings.clone();
        move || {
            let Some(ui) = weak.upgrade() else { return };
            {
                let mut s = settings.borrow_mut();
                s.author_name = ui.get_author_name().to_string();
                s.restore_session = ui.get_restore_session();
                s.interface_language = ui.get_pref_language().to_string();
                s.theme = ui.get_pref_theme().to_string();
                s.save();
            }
            ui.set_status(SharedString::from("Preferences saved"));
        }
    });

    // Backstage: Exit (persist settings + window size first).
    ui.on_exit_app({
        let weak = ui.as_weak();
        let settings = settings.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                save_window_size(&ui, &settings);
            }
            let _ = slint::quit_event_loop();
        }
    });

    // Zoom changed → scale live now; debounce the actual re-render so it happens
    // once, at the final resolution, ~180 ms after zooming stops.
    ui.on_zoom_changed({
        let engine = engine.clone();
        let weak = ui.as_weak();
        let pending = pending_render_width.clone();
        let timer = zoom_timer.clone();
        move |width: f32| {
            pending.set(width.round() as i32);
            let engine = engine.clone();
            let weak = weak.clone();
            let pending = pending.clone();
            timer.start(TimerMode::SingleShot, Duration::from_millis(180), move || {
                if let Some(ui) = weak.upgrade() {
                    let id = ui.get_active_doc_id();
                    if id >= 0 {
                        engine.set_render_width(id as u32, pending.get());
                    }
                    ui.invoke_update_render();
                }
            });
        }
    });

    // Ribbon commands not yet implemented → surface a hint in the status bar.
    ui.on_action_todo({
        let weak = ui.as_weak();
        move |name: SharedString| {
            if let Some(ui) = weak.upgrade() {
                ui.set_status(SharedString::from(format!("{name} — not implemented yet")));
            }
        }
    });

    // Page navigation: jump the scroll position to a 1-based page.
    ui.on_goto_page({
        let weak = ui.as_weak();
        move |page: f32| {
            let Some(ui) = weak.upgrade() else { return };
            let pages = ui.get_pages();
            let n = pages.row_count() as i32;
            if n == 0 {
                return;
            }
            let page = (page.round() as i32).clamp(1, n);
            ui.set_current_page(page);
            if ui.get_display_mode() == 1 {
                // Continuous: scroll to the page's offset.
                let offset = page_offset(&pages, (page - 1) as usize, ui.get_zoom());
                ui.invoke_scroll_to_y(offset);
            } else {
                // Single: show the new page from the top (rendered via update-render).
                ui.invoke_scroll_to_y(0.0);
            }
        }
    });

    // Scroll/zoom/open changed the viewport → update the current page and render
    // the pages now intersecting the (buffered) visible band. The engine dedupes.
    ui.on_update_render({
        let weak = ui.as_weak();
        let engine = engine.clone();
        move || {
            let Some(ui) = weak.upgrade() else { return };
            let id = ui.get_active_doc_id();
            if id < 0 {
                ui.set_content_height(0.0);
                return;
            }
            let id = id as u32;
            let pages = ui.get_pages();
            let n = pages.row_count();
            if n == 0 {
                ui.set_content_height(0.0);
                return;
            }
            let zoom = ui.get_zoom();
            set_page_size(&ui); // keep the Properties "Page size" current

            // Total content height for the scroller — set explicitly so the
            // Flickable detects overflow regardless of the content variant.
            let content_h: f32 = if ui.get_display_mode() == 0 {
                let cur = (ui.get_current_page() - 1).clamp(0, n as i32 - 1) as usize;
                let a = pages.row_data(cur).map(|p| p.aspect).unwrap_or(FALLBACK_ASPECT);
                BASE_WIDTH * zoom * a + ROW_GAP
            } else {
                (0..n)
                    .map(|i| {
                        BASE_WIDTH * zoom * pages.row_data(i).map(|p| p.aspect).unwrap_or(FALLBACK_ASPECT)
                            + ROW_GAP
                    })
                    .sum()
            };
            ui.set_content_height(content_h);

            // Single mode: render just the current page (+ prefetch neighbours).
            if ui.get_display_mode() == 0 {
                let cur = (ui.get_current_page() - 1).clamp(0, n as i32 - 1);
                for i in (cur - 1).max(0)..=(cur + 1).min(n as i32 - 1) {
                    engine.render(id, i);
                }
                return;
            }

            // Continuous mode: render the visible band and track the top page.
            let scroll = ui.get_scroll_y();
            let view_h = ui.get_view_height();
            let top_edge = scroll - 300.0;
            let bottom_edge = scroll + view_h + 600.0;

            let mut y = 0.0f32;
            let mut current = 1i32;
            for i in 0..n {
                let aspect = pages.row_data(i).map(|p| p.aspect).unwrap_or(FALLBACK_ASPECT);
                let h = BASE_WIDTH * zoom * aspect + ROW_GAP;
                if y <= scroll + 1.0 {
                    current = i as i32 + 1;
                }
                if y + h >= top_edge && y <= bottom_edge {
                    engine.render(id, i as i32);
                }
                y += h;
                if y > bottom_edge {
                    break;
                }
            }
            ui.set_current_page(current);
        }
    });

    // Apply loaded settings to the UI.
    {
        let s = settings.borrow();
        ui.set_author_name(s.author_name.clone().into());
        ui.set_restore_session(s.restore_session);
        ui.set_pref_language(s.interface_language.clone().into());
        ui.set_pref_theme(s.theme.clone().into());
    }

    // Restore the recents list from settings.
    rebuild_recents(&ui, &settings.borrow(), &recent_filter.borrow());

    // Open a file passed on the command line (e.g. when launched via the PDF
    // file association); otherwise start with no document open. The bundled
    // sample is no longer auto-loaded.
    match std::env::args_os().nth(1).map(PathBuf::from).filter(|p| p.exists()) {
        Some(path) => open_document(&ui, &engine, &settings, &recent_filter, path),
        None => {
            ui.set_active_doc_id(NO_DOC);
            ui.set_status(SharedString::from("No document open — use File ▸ Open to load a PDF."));
        }
    }

    ui.run()?;
    Ok(())
}
