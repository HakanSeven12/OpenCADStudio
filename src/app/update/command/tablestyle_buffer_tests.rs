use super::*;

// #53: New/Copy/Delete must reload Table/MLine buffers (no stale edits
// into the fresh style), and cell text-style typos must not persist.
use crate::app::{OpenCADStudio, StyleKind};

#[test]
fn table_new_reloads_cell_buffers() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.ts_cell_height = "STALE_SENTINEL".to_string();
    let _ = app.style_new(StyleKind::Table);
    assert_ne!(
        app.ts_cell_height, "STALE_SENTINEL",
        "buffers must reload from the fresh style"
    );
}
