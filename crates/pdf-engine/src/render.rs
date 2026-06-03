//! Page rasterization.

use pdfium_render::prelude::*;

/// Render one page to raw RGBA8 pixels: `(width, height, bytes)`.
///
/// The page is rendered `target_width` pixels wide; height is derived to
/// preserve the page's aspect ratio (capped to avoid pathological tall pages).
pub(crate) fn render_page_bytes(
    doc: &PdfDocument,
    index: i32,
    target_width: i32,
) -> Result<(u32, u32, Vec<u8>), String> {
    let page = doc
        .pages()
        .get(index)
        .map_err(|e| format!("open page {}: {e:?}", index + 1))?;

    let config = PdfRenderConfig::new()
        .set_target_width(target_width)
        .set_maximum_height(target_width * 4);

    let bitmap = page
        .render_with_config(&config)
        .map_err(|e| format!("render page {}: {e:?}", index + 1))?;

    Ok((
        bitmap.width() as u32,
        bitmap.height() as u32,
        bitmap.as_rgba_bytes(),
    ))
}
