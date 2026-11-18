//! A PDF whose page tree refers back to itself must not stop the app.
//!
//! `tests/fixtures/pdf/inkscape.pdf` with one byte of its compressed
//! cross-reference stream changed (offset 18,657, `1` to `0`) points a
//! `/Kids` entry back up the page tree. hayro-syntax 0.7 followed `/Kids`
//! without a record of the nodes it had seen, so the read never ended and
//! overflowed the stack. A stack overflow stops the process on every thread,
//! also on a worker under `catch_unwind`, so every PDF read path (IMPORT,
//! PDFIMPORT, PDFATTACH, the thumbnails) stopped the app with this file.
//! Without the fix, this test stops the whole test binary.
#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;
use OpenCADStudio::scene::model::{pdf_raster, pdf_vector};

const OFFSET: usize = 18_657;

fn damaged() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/pdf/inkscape.pdf"
    );
    let mut bytes = std::fs::read(path).expect("the inkscape.pdf fixture");
    assert_eq!(
        bytes[OFFSET], b'1',
        "the fixture changed; find the byte again"
    );
    bytes[OFFSET] = b'0';
    bytes
}

/// Each read the PDF commands do on the file ends. The page tree is read
/// without the loop: one page of 8 × 6 in, as in the undamaged file.
#[test]
fn a_page_tree_that_refers_back_to_itself_is_read_to_an_end() {
    let path = "page-tree-cycle.pdf";
    pdf_raster::register_source(path, Arc::new(damaged()));
    assert_eq!(pdf_raster::page_count(path), Some(1));
    assert_eq!(pdf_raster::page_size_inches(path, "1"), Some((8.0, 6.0)));
    assert!(pdf_raster::rasterize_page_at_dpi_uncached(path, "1", 8.0).is_some());
    assert!(pdf_vector::page_content_by_layer(path, "1", &[]).is_some());
}
