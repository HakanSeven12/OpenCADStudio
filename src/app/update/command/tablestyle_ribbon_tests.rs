use super::*;

// #45: picking a table style in the ribbon must write the document header
// (like Text/Dim/MLeader), not just the ribbon mirror.
use crate::app::OpenCADStudio;
use crate::modules::StyleKey;

#[test]
fn tablestyle_ribbon_pick_writes_header_and_dirties() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    let mut extra = document::objects::TableStyle::standard();
    extra.name = "EXTRA".to_string();
    extra.handle = app.tabs[i].scene.document.allocate_handle();
    app.tabs[i].scene.document.objects.insert(
        extra.handle,
        document::objects::ObjectType::TableStyle(extra),
    );
    app.tabs[i].dirty = false;
    let _ = app.on_ribbon_style_changed(StyleKey::TableStyle, "EXTRA".to_string());
    assert_eq!(
        app.tabs[i].scene.document.header.current_table_style_name, "EXTRA",
        "ribbon pick must reach the document header"
    );
    assert!(
        app.tabs[i].dirty,
        "ribbon pick must dirty the tab like its siblings"
    );
}

#[test]
fn tablestyle_pick_flows_into_table_insert() {
    // End-to-end: ribbon pick → header → insert dialog defaults to the pick.
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    let mut extra = document::objects::TableStyle::standard();
    extra.name = "EXTRA".to_string();
    extra.handle = app.tabs[i].scene.document.allocate_handle();
    app.tabs[i].scene.document.objects.insert(
        extra.handle,
        document::objects::ObjectType::TableStyle(extra),
    );
    let _ = app.on_ribbon_style_changed(StyleKey::TableStyle, "EXTRA".to_string());
    app.open_table_insert();
    assert_eq!(
        app.table_insert.style, "EXTRA",
        "insert dialog must default to the ribbon-picked style"
    );
}
