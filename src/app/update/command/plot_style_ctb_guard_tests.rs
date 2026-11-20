use super::*;

use crate::app::OpenCADStudio;
use document::entities::Line;
use document::types::Vector3;

fn line_handle(app: &mut OpenCADStudio) -> document::Handle {
    let mut line = Line::new();
    line.start = Vector3::ZERO;
    line.end = Vector3::new(1.0, 0.0, 0.0);
    app.commit_entity_handle(document::EntityType::Line(line))
        .expect("line should commit")
}

#[test]
fn ctb_mode_plot_style_choice_pushes_no_undo_entry() {
    let mut app = OpenCADStudio::new_for_test();
    let i = app.active_tab;
    let h = line_handle(&mut app);
    // Mirror `chprop_integration_tests`: seed `source_handles` so
    // `property_target_handles` returns the line.
    app.tabs[i].properties.source_handles = vec![h];
    // CTB mode: the B15 `plot_style` arm is a no-op early return.
    app.tabs[i].scene.document.header.plotstyle_mode = true;
    // Settle any pending snapshot left by the fixture setup.
    app.finish_pending_history(i);
    assert!(
        app.tabs[i].history.pending.is_none(),
        "test setup: no pending snapshot before the choice change"
    );
    let before = app.tabs[i].history.undo_stack.len();
    // Drive the handler directly: `update()` closes the pending
    // transaction at the message boundary (`finish_all_pending_history`),
    // which silently discards the leaked no-op snapshot and would mask
    // the bug. The direct call exposes whether a snapshot was pushed.
    let _ = app.on_prop_geom_choice_changed("plot_style", "ByLayer".to_string());
    assert!(
        app.tabs[i].history.pending.is_none(),
        "CTB-mode plot_style choice is a no-op and must not push an undo snapshot"
    );
    app.finish_pending_history(i);
    assert_eq!(
        app.tabs[i].history.undo_stack.len(),
        before,
        "CTB-mode plot_style choice must not add an undo entry"
    );
}
