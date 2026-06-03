# Open PDF Studio

A minimal PDF viewer demonstrating the **native Rust UI (Slint) + PDFium engine** stack.

- **Engine:** [PDFium](https://pdfium.googlesource.com/pdfium/) (Google's C++ PDF engine, the one in Chrome) via the [`pdfium-render`](https://crates.io/crates/pdfium-render) crate. Parses the PDF and rasterizes each page to an RGBA bitmap.
- **UI:** [Slint](https://slint.dev) — a native, GPU-accelerated Rust GUI (no browser/Electron). Displays the bitmap and drives Prev/Next navigation.

```
┌─────────────────────────────────────────────┐
│  UI            ui/app.slint  (Slint)          │  toolbar + page view
│  Core logic    src/main.rs   (Rust)           │  state, navigation
│  Engine        pdfium.dll    (PDFium, C++)    │  parse + render page → bitmap
└─────────────────────────────────────────────┘
```

## Run

```sh
cargo run
```

It opens `2459-TO_Fragmenten.pdf` from the project folder and shows page 1.
Use **Previous / Next** to page through the document.

## Requirements

- Rust (stable).
- `pdfium.dll` in the project root (already included). It is **not** bundled by the
  crate; prebuilt binaries come from
  [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries).
  At startup the app looks for it next to the executable and in the current directory.

## Where to take it next

- Continuous scroll of all pages (Slint `ListView` / `Flickable`) instead of one page at a time.
- Zoom (re-render at a higher `TARGET_WIDTH`) and fit-to-width/page modes.
- Open-file dialog instead of the hard-coded `PDF_PATH`.
- Render pages on a background thread (PDFium is single-threaded — gate behind a worker + channel) to keep the UI responsive on large pages.
- Text selection / search via `pdfium-render`'s text extraction APIs.
