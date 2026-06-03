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
mod settings;

use pdf_engine::{Engine, BASE_RENDER_WIDTH};
use settings::Settings;
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

/// The sample PDF opened on startup (relative to the working directory).
const PDF_PATH: &str = "temp/samples/2459-TO_Fragmenten.pdf";

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ui = AppWindow::new()?;

    // Spawn the engine; its events are marshalled to the UI by `bridge`.
    let engine = Engine::spawn(bridge::event_sink(ui.as_weak()), BASE_RENDER_WIDTH);

    // Persisted settings (Preferences + recents) and the live Find filter.
    let settings = Rc::new(RefCell::new(Settings::load()));
    let recent_filter = Rc::new(RefCell::new(String::new()));

    // Debounced zoom: pages scale live while zooming; the engine only re-renders
    // once the zoom settles (timer fires), avoiding a redraw on every step.
    let pending_render_width = Rc::new(Cell::new(BASE_RENDER_WIDTH));
    let zoom_timer = Rc::new(Timer::default());

    // "Open…" / Browse → native file dialog → load the chosen file.
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
                {
                    let mut s = settings.borrow_mut();
                    s.push_recent(&path.to_string_lossy());
                    s.save();
                }
                if let Some(ui) = weak.upgrade() {
                    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("document");
                    ui.set_status(SharedString::from(format!("Opening {name} …")));
                    rebuild_recents(&ui, &settings.borrow(), &recent_filter.borrow());
                }
                engine.open(path);
            }
        }
    });

    // Backstage: open a recent file by path.
    ui.on_open_path({
        let engine = engine.clone();
        let weak = ui.as_weak();
        let settings = settings.clone();
        let recent_filter = recent_filter.clone();
        move |path: SharedString| {
            let path_str = path.to_string();
            {
                let mut s = settings.borrow_mut();
                s.push_recent(&path_str);
                s.save();
            }
            if let Some(ui) = weak.upgrade() {
                let name = Path::new(&path_str)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("document");
                ui.set_status(SharedString::from(format!("Opening {name} …")));
                rebuild_recents(&ui, &settings.borrow(), &recent_filter.borrow());
            }
            engine.open(PathBuf::from(path_str));
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

    // Backstage: Exit (persist settings first).
    ui.on_exit_app({
        let settings = settings.clone();
        move || {
            settings.borrow().save();
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
                engine.set_render_width(pending.get());
                if let Some(ui) = weak.upgrade() {
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
            let pages = ui.get_pages();
            let n = pages.row_count();
            if n == 0 {
                ui.set_content_height(0.0);
                return;
            }
            let zoom = ui.get_zoom();

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
                    engine.render(i);
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
                    engine.render(i as i32);
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

    // Open the bundled sample only if it exists (a dev convenience). A clean
    // install has no such file, so it starts with no document open.
    let sample = PathBuf::from(PDF_PATH);
    if sample.exists() {
        {
            let mut s = settings.borrow_mut();
            s.push_recent(&sample.to_string_lossy());
            s.save();
        }
        rebuild_recents(&ui, &settings.borrow(), &recent_filter.borrow());
        engine.open(sample);
        ui.set_status(SharedString::from(format!("Opening {PDF_PATH} …")));
    } else {
        ui.set_status(SharedString::from("No document open — use File ▸ Open to load a PDF."));
    }

    ui.run()?;
    Ok(())
}
