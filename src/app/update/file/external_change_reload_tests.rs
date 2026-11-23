use super::*;

use crate::app::{Message, OpenCADStudio, PendingExternalChange, SaveContinuation, SavePurpose};

/// The file the reload is told to read back, gone whatever the test does
/// next — including a failing assert.
struct TempFile(std::path::PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Reloading a drawing that changed outside the app replaces the whole
/// tab, and the drawing read back from disk is a different drawing: the
/// place the replaced one remembered for its next attribute, and the line
/// TEXT would continue below, belong to the document that is gone.
#[test]
fn reloading_forgets_the_places_the_replaced_drawing_remembered() {
    let mut app = OpenCADStudio::new_for_test();
    let reloaded_id = app.push_test_document();
    let kept_id = app.push_test_document();
    let i = app
        .tabs
        .iter()
        .position(|tab| tab.id == reloaded_id)
        .expect("the drawing the reload replaces");
    crate::modules::draw::draw::attdef::session().set_place(reloaded_id, &Default::default());
    crate::modules::annotate::text::session().set_place(reloaded_id, &Default::default());
    crate::modules::draw::draw::attdef::session().set_place(kept_id, &Default::default());
    assert!(
        crate::modules::draw::draw::attdef::session()
            .last_in(reloaded_id)
            .is_some(),
        "the drawing about to be reloaded remembered a place"
    );
    assert!(
        crate::modules::annotate::text::session()
            .last_in(reloaded_id)
            .is_some(),
        "and a line to continue below"
    );
    assert!(
        crate::modules::draw::draw::attdef::session()
            .last_in(kept_id)
            .is_some(),
        "the drawing that stays open remembered its own"
    );
    let temp =
        TempFile(std::env::temp_dir().join(format!("open_cad_studio_reload_{reloaded_id}.dxf")));
    std::fs::write(&temp.0, b"").expect("the file the reload reads back");
    app.pending_external_change = Some(PendingExternalChange {
        tab_id: reloaded_id,
        path: temp.0.clone(),
        version: document::DxfVersion::AC1027,
        purpose: SavePurpose::Manual,
        continuation: SaveContinuation::None,
        set_current_path: true,
    });

    let _ = app.update(Message::ExternalChangeReload);

    assert_ne!(
        app.tabs[i].id, reloaded_id,
        "the reloaded drawing is a fresh tab, not the one that was on disk"
    );
    assert!(
        crate::modules::draw::draw::attdef::session()
            .last_in(reloaded_id)
            .is_none(),
        "the place the replaced drawing remembered went with it"
    );
    assert!(
        crate::modules::annotate::text::session()
            .last_in(reloaded_id)
            .is_none(),
        "and so did the line it remembered"
    );
    assert!(
        app.tabs.iter().any(|tab| tab.id == kept_id),
        "the other drawing is still open"
    );
    assert!(
        crate::modules::draw::draw::attdef::session()
            .last_in(kept_id)
            .is_some(),
        "and keeps the place it remembered"
    );
}
