use super::*;

use crate::app::OpenCADStudio;

/// USERR1 is a user variable: a save stores the annotation scale in
/// CANNOSCALE, not in USERR1, and keeps the value the user gave it.
#[test]
fn save_keeps_userr1() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].scene.document.header.user_real1 = 7.5;
    app.tabs[0].scene.annotation_scale = 0.02;
    app.prepare_snapshot_save(0);
    assert_eq!(app.tabs[0].scene.document.header.user_real1, 7.5);
}
