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

#[test]
fn ribbon_style_picks_dirty_tab_and_are_undoable() {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;

    let mut ts = document::tables::TextStyle::new("Notes");
    ts.handle = app.tabs[i].scene.document.allocate_handle();
    app.tabs[i].scene.document.text_styles.add(ts).unwrap();
    let ts_handle = app.tabs[i]
        .scene
        .document
        .text_styles
        .iter()
        .find(|s| s.name == "Notes")
        .unwrap()
        .handle;
    let prev_ts_name = app.tabs[i]
        .scene
        .document
        .header
        .current_text_style_name
        .clone();
    let prev_ts_handle = app.tabs[i].scene.document.header.current_text_style_handle;
    app.tabs[i].dirty = false;
    let _ = app.update(Message::RibbonStyleChanged {
        key: StyleKey::TextStyle,
        name: "Notes".to_string(),
    });
    assert!(app.tabs[i].dirty);
    assert_eq!(
        app.tabs[i].scene.document.header.current_text_style_name,
        "Notes"
    );
    assert_eq!(
        app.tabs[i].scene.document.header.current_text_style_handle,
        ts_handle
    );
    app.undo_active_tab();
    assert_eq!(
        app.tabs[i].scene.document.header.current_text_style_name,
        prev_ts_name
    );
    assert_eq!(
        app.tabs[i].scene.document.header.current_text_style_handle,
        prev_ts_handle
    );

    let mut ds = document::tables::DimStyle::new("MetricDim");
    ds.handle = app.tabs[i].scene.document.allocate_handle();
    app.tabs[i].scene.document.dim_styles.add(ds).unwrap();
    let ds_handle = app.tabs[i]
        .scene
        .document
        .dim_styles
        .get("MetricDim")
        .unwrap()
        .handle;
    let prev_ds_name = app.tabs[i]
        .scene
        .document
        .header
        .current_dimstyle_name
        .clone();
    let prev_ds_handle = app.tabs[i].scene.document.header.current_dimstyle_handle;
    app.tabs[i].dirty = false;
    let _ = app.update(Message::RibbonStyleChanged {
        key: StyleKey::DimStyle,
        name: "MetricDim".to_string(),
    });
    assert!(app.tabs[i].dirty);
    assert_eq!(
        app.tabs[i].scene.document.header.current_dimstyle_name,
        "MetricDim"
    );
    assert_eq!(
        app.tabs[i].scene.document.header.current_dimstyle_handle,
        ds_handle
    );
    app.undo_active_tab();
    assert_eq!(
        app.tabs[i].scene.document.header.current_dimstyle_name,
        prev_ds_name
    );
    assert_eq!(
        app.tabs[i].scene.document.header.current_dimstyle_handle,
        prev_ds_handle
    );

    let mut extra = document::objects::TableStyle::standard();
    extra.name = "EXTRA".to_string();
    extra.handle = app.tabs[i].scene.document.allocate_handle();
    app.tabs[i].scene.document.objects.insert(
        extra.handle,
        document::objects::ObjectType::TableStyle(extra),
    );
    let prev_table_style = app.tabs[i]
        .scene
        .document
        .header
        .current_table_style_name
        .clone();
    app.tabs[i].dirty = false;
    let _ = app.update(Message::RibbonStyleChanged {
        key: StyleKey::TableStyle,
        name: "EXTRA".to_string(),
    });
    assert!(app.tabs[i].dirty);
    assert_eq!(
        app.tabs[i].scene.document.header.current_table_style_name,
        "EXTRA"
    );
    app.undo_active_tab();
    assert_eq!(
        app.tabs[i].scene.document.header.current_table_style_name,
        prev_table_style
    );
}

