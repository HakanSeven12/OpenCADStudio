use super::*;

/// A first `Visibility Set` row authors the parameter and its initial state.
/// That state has to keep the definition's objects on screen: a state with no
/// objects listed is not "everything visible", it is nothing visible, and the
/// block goes blank the moment the row finishes.
#[test]
fn visibility_set_row_keeps_every_object_visible_in_the_first_state() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i]
        .active_block_edit_session()
        .expect("an editor session is open")
        .br_handle;

    let _ = app.dispatch_command("BPARAMETERSET Visibility Set");
    feed(&mut app, "0,0");
    feed(&mut app, "");

    let parameter = crate::scene::block_visibility::parameter(&app.tabs[i].scene.document, definition)
        .expect("the row authors the visibility parameter");
    assert!(
        !parameter.all_blocks.is_empty(),
        "the state set has to carry the definition's objects before it can show them"
    );
    let state = parameter
        .states
        .iter()
        .find(|state| state.name == "VisibilityState0")
        .expect("the first placement creates its initial state");
    assert_eq!(
        state.visible_blocks.len(),
        parameter.all_blocks.len(),
        "the initial state keeps every object visible"
    );
}
