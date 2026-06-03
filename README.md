# Open PDF Studio

A fast, native PDF viewer built on a **Rust + Slint + PDFium** stack — no browser, no Electron.

- **Engine:** [PDFium](https://pdfium.googlesource.com/pdfium/) (Google's C++ PDF engine, the one in Chrome) via the [`pdfium-render`](https://crates.io/crates/pdfium-render) crate — parses PDFs and rasterizes pages to RGBA bitmaps.
- **UI:** [Slint](https://slint.dev) — a native, GPU-accelerated Rust GUI, authored in Slint's declarative markup and compiled to Rust.
- **Core:** Rust, with the PDF engine on a dedicated worker thread (PDFium is single-threaded) talking to the UI by message passing.

## Features

- **Multiple documents** — browser-style tab strip; each tab keeps its own pages, zoom, scroll, current page and page-display mode.
- **Continuous & single-page** modes, lazy page rendering, zoom 10–800 % (Ctrl+wheel zooms to the cursor), middle-button pan.
- **Left panel** — page thumbnails (rendered on demand), collapsible.
- **Right panel** — read-only document **Properties** (file info, metadata, page size), collapsible.
- **Ribbon UI** (Home / Comments / View …), a File backstage with recent files, and a movable Preferences dialog with persisted settings.
- Remembers its **window size**; opens a PDF passed on the command line (PDF file association).

## Project layout

A Cargo workspace:

```
crates/
  pdf-engine/   UI-agnostic core: PDFium on a worker thread, Command/Event API
    src/{lib.rs, types.rs, worker.rs, render.rs, library.rs}
  app/          the Slint application
    src/{main.rs, bridge.rs, docs.rs, settings.rs}   # logic, engine↔UI bridge, tab store, settings
    ui/*.slint                                       # UI (compiled to Rust at build time)
    icons/                                           # app icon set (ico/icns/png)
.github/workflows/release.yml                        # cross-platform installer CI
```

Data flow: `app` sends `Command`s to the engine (open / render / thumbnail / close, per document id); the engine emits `Event`s on its worker thread; `bridge.rs` marshals them onto the Slint event loop and updates the per-tab models held in `docs.rs`.

## Build & run

```sh
cargo run -p open-pdf-studio3
```

Starts with no document open — use **File ▸ Open** or the **+** tab to open a PDF (or pass a path: `cargo run -p open-pdf-studio3 -- some.pdf`).

### Requirements

- Rust (stable).
- `pdfium.dll` (the PDFium engine) next to the binary or in the working directory. A copy is committed at the repo root for dev; prebuilt binaries come from [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries). The crate does **not** bundle it.

## Releases / installers

Pushing a `v*` tag runs `.github/workflows/release.yml`, which builds per-OS installers with [`cargo-packager`](https://crates.io/crates/cargo-packager) and publishes them to the [Releases page](https://github.com/OpenAEC-Foundation/open-pdf-studio-v3/releases):

- **Windows** — NSIS installer (`…-setup.exe`), code-signed with **Azure Trusted Signing**, registered as a `.pdf` handler.
- **macOS** — `.dmg` (Apple Silicon).
- **Linux** — `.deb`.

The per-platform PDFium is fetched at build time. App/installer version comes from `crates/app/Cargo.toml`.
