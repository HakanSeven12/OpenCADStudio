use super::*;

// RED: `Message::LayerToggleVisible` currently carries a panel-row `usize`;
// these tests pass a layer `String` name and fail to compile until the
// #22 migration lands.
use crate::app::{Message, OpenCADStudio};
use crate::ui::window::layers::LayerSortCol;

#[test]
fn ribbon_layer_color_changes_only_named_layer_and_supports_undo() {
    use document::types::Color;
    let mut app = app_with_zulu_alpha();
    let i = app.active_tab;
    let _ = app.update(Message::LayerSelect("ALPHA".into()));
    let selected = app.tabs[i].layers.selected_multi.clone();
    let alpha = app.tabs[i]
        .scene
        .document
        .layers
        .get("ALPHA")
        .unwrap()
        .color;
    let old = app.tabs[i].scene.document.layers.get("ZULU").unwrap().color;
    let creation = app.tabs[i].scene.document.header.current_entity_color;
    let mut line = document::entities::Line::new();
    line.common.color = Color::Index(2);
    line.common.layer = "ZULU".into();
    let handle = app.tabs[i]
        .scene
        .add_entity(document::EntityType::Line(line));
    app.tabs[i].scene.select_entity(handle, true);
    let epoch = app.tabs[i].scene.geometry_epoch;
    let _ = app.update(Message::RibbonLayerColorChanged {
        layer: "ZULU".into(),
        color: Color::Index(1),
    });
    assert_eq!(
        app.tabs[i].scene.document.layers.get("ZULU").unwrap().color,
        Color::Index(1)
    );
    assert_eq!(
        app.tabs[i]
            .scene
            .document
            .layers
            .get("ALPHA")
            .unwrap()
            .color,
        alpha
    );
    assert_eq!(app.tabs[i].layers.selected_multi, selected);
    assert_eq!(
        app.tabs[i]
            .scene
            .document
            .get_entity(handle)
            .unwrap()
            .common()
            .color,
        Color::Index(2)
    );
    assert_eq!(
        app.tabs[i].scene.document.header.current_entity_color,
        creation
    );
    assert!(app.tabs[i].scene.geometry_epoch > epoch);
    assert_eq!(app.tabs[i].recent_colors.first(), Some(&Color::Index(1)));
    app.undo_active_tab();
    assert_eq!(
        app.tabs[i].scene.document.layers.get("ZULU").unwrap().color,
        old
    );
    app.redo_active_tab();
    assert_eq!(
        app.tabs[i].scene.document.layers.get("ZULU").unwrap().color,
        Color::Index(1)
    );
    let history = app.tabs[i].history.undo_stack.len();
    let _ = app.update(Message::RibbonLayerColorChanged {
        layer: "missing".into(),
        color: Color::Index(3),
    });
    let _ = app.update(Message::RibbonLayerColorChanged {
        layer: "ZULU".into(),
        color: Color::ByLayer,
    });
    assert_eq!(app.tabs[i].history.undo_stack.len(), history);
    let _ = app.update(Message::OpenColorWindow(
        crate::app::ColorPickTarget::RibbonLayer("ZULU".into()),
        Color::Index(1),
    ));
    let _ = app.update(Message::ColorWindowPick(
        Color::Rgb {
            r: 12,
            g: 34,
            b: 56,
        },
        Some("TEST$Color".into()),
    ));
    let layer = app.tabs[i].scene.document.layers.get("ZULU").unwrap();
    assert_eq!(
        layer.color,
        Color::Rgb {
            r: 12,
            g: 34,
            b: 56
        }
    );
    assert_eq!(layer.book_name.as_deref(), Some("TEST"));
    assert_eq!(layer.color_name.as_deref(), Some("Color"));
    assert_eq!(
        app.tabs[i]
            .scene
            .document
            .layers
            .get("ALPHA")
            .unwrap()
            .color,
        alpha
    );
}

fn rename_layer(app: &mut OpenCADStudio, old_name: &str, new_name: &str) {
    let i = app.active_tab;
    assert!(
        app.tabs[i]
            .layers
            .layers
            .iter()
            .any(|layer| layer.name == old_name),
        "layer exists in panel"
    );
    app.tabs[i].layers.editing = Some(old_name.to_string());
    app.tabs[i].layers.edit_buf = new_name.to_string();
    let _ = app.on_layer_rename_commit();
}

fn app_with_zulu_alpha() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let _ = app.on_layer_new();
    let first = app.tabs[app.active_tab].layers.edit_buf.clone();
    rename_layer(&mut app, &first, "ZULU");
    let _ = app.on_layer_new();
    let second = app.tabs[app.active_tab].layers.edit_buf.clone();
    rename_layer(&mut app, &second, "ALPHA");
    app
}

#[test]
fn layer_toggle_visible_targets_layer_by_name_after_resort() {
    let mut app = app_with_zulu_alpha();
    // Flip the panel order; a stale row index would now point at the
    // wrong layer, but the name still resolves to ZULU.
    let _ = app.update(Message::LayerSort(LayerSortCol::Name));
    let _ = app.update(Message::LayerToggleVisible("ZULU".to_string()));
    let i = app.active_tab;
    let zulu = app.tabs[i]
        .layers
        .layers
        .iter()
        .find(|l| l.name == "ZULU")
        .expect("ZULU in panel");
    let alpha = app.tabs[i]
        .layers
        .layers
        .iter()
        .find(|l| l.name == "ALPHA")
        .expect("ALPHA in panel");
    assert!(!zulu.visible, "ZULU should be toggled off");
    assert!(alpha.visible, "ALPHA must be untouched");
    assert!(
        app.tabs[i]
            .scene
            .document
            .layers
            .get("ZULU")
            .unwrap()
            .flags
            .off,
        "document ZULU should be off"
    );
}

#[test]
fn layer_rename_commit_follows_name_across_resort() {
    // #44: rename state keyed by name survives a panel re-sort; a stale
    // row index would rename the wrong layer.
    let mut app = app_with_zulu_alpha();
    let i = app.active_tab;
    app.tabs[i].layers.editing = Some("ZULU".to_string());
    app.tabs[i].layers.edit_buf = "ZULU_NEW".to_string();
    let _ = app.update(Message::LayerSort(LayerSortCol::Name));
    let _ = app.update(Message::LayerRenameCommit);
    let names: Vec<String> = app.tabs[i]
        .layers
        .layers
        .iter()
        .map(|l| l.name.clone())
        .collect();
    assert!(
        names.contains(&"ZULU_NEW".to_string()),
        "ZULU must be renamed, got {names:?}"
    );
    assert!(
        !names.contains(&"ZULU".to_string()),
        "old name must be gone, got {names:?}"
    );
    assert!(
        app.tabs[i].scene.document.layers.contains("ZULU_NEW"),
        "document must follow the rename"
    );
}

/// #2610: the current layer can't be frozen, a frozen layer can't be
/// made current, and Defpoints and xref layers can't be deleted.
#[test]
fn layer_manager_guards_current_frozen_and_protected_layers() {
    let mut app = app_with_zulu_alpha();
    let i = app.active_tab;
    let current = app.tabs[i].scene.document.header.current_layer_name.clone();
    let _ = app.update(Message::LayerToggleFreeze(current.clone()));
    assert!(
        !app.tabs[i]
            .scene
            .document
            .layers
            .get(&current)
            .unwrap()
            .is_frozen(),
        "the current layer stays thawed"
    );

    let _ = app.update(Message::LayerToggleFreeze("ZULU".to_string()));
    assert!(app.tabs[i]
        .scene
        .document
        .layers
        .get("ZULU")
        .unwrap()
        .is_frozen());
    let zulu = app.layer_panel_index(i, "ZULU").unwrap();
    app.tabs[i].layers.selected = Some(zulu);
    let _ = app.on_layer_set_current();
    assert_eq!(
        app.tabs[i].scene.document.header.current_layer_name,
        current
    );

    for name in ["Defpoints", "XREF|WALLS"] {
        let mut layer = document::tables::layer::Layer::new(name);
        layer.handle = app.tabs[i].scene.document.allocate_handle();
        let _ = app.tabs[i].scene.document.layers.add(layer);
        app.sync_layer_panel(i);
        let idx = app.layer_panel_index(i, name).unwrap();
        app.tabs[i].layers.selected = Some(idx);
        app.tabs[i].layers.selected_multi = vec![idx];
        let _ = app.on_layer_delete();
        assert!(
            app.tabs[i].scene.document.layers.contains(name),
            "{name} kept"
        );
    }
}

#[test]
fn layer_rename_commit_deleted_layer_is_noop() {
    // Renaming a layer deleted mid-edit must clear state without
    // renaming anything or pushing undo.
    let mut app = app_with_zulu_alpha();
    let i = app.active_tab;
    app.finish_pending_history(i);
    let undo_before = app.tabs[i].history.undo_stack.len();
    app.tabs[i].layers.editing = Some("GONE".to_string());
    app.tabs[i].layers.edit_buf = "NEW".to_string();
    let _ = app.update(Message::LayerRenameCommit);
    assert!(
        app.tabs[i].layers.editing.is_none(),
        "stale edit must clear"
    );
    assert!(
        app.tabs[i].layers.edit_buf.is_empty(),
        "stale edit buffer must clear"
    );
    assert_eq!(
        app.tabs[i].history.undo_stack.len(),
        undo_before,
        "no-op commit must not push undo"
    );
    assert!(
        !app.tabs[i].scene.document.layers.contains("NEW"),
        "nothing must be renamed"
    );
}

#[test]
fn layer_rename_commit_empty_name_is_noop() {
    let mut app = app_with_zulu_alpha();
    let i = app.active_tab;
    app.finish_pending_history(i);
    let undo_before = app.tabs[i].history.undo_stack.len();
    app.tabs[i].layers.editing = Some("ZULU".to_string());
    app.tabs[i].layers.edit_buf = "   ".to_string();
    let _ = app.update(Message::LayerRenameCommit);
    assert!(app.tabs[i].layers.editing.is_none(), "edit must clear");
    assert!(
        app.tabs[i].scene.document.layers.contains("ZULU"),
        "blank commit must not rename"
    );
    assert_eq!(
        app.tabs[i].history.undo_stack.len(),
        undo_before,
        "blank commit must not push undo"
    );
}

#[test]
fn layer_color_picker_toggle_follows_name_across_resort() {
    // Same name-keying guarantee as rename, for the swatch row.
    let mut app = app_with_zulu_alpha();
    let _ = app.update(Message::LayerColorPickerToggle("ZULU".to_string()));
    let _ = app.update(Message::LayerSort(LayerSortCol::Name));
    let i = app.active_tab;
    assert_eq!(
        app.tabs[i].layers.color_picker_row,
        Some("ZULU".to_string()),
        "picker must stay on ZULU across a resort"
    );
    let _ = app.update(Message::LayerColorPickerToggle("ZULU".to_string()));
    assert!(
        app.tabs[i].layers.color_picker_row.is_none(),
        "second toggle must close the picker"
    );
}

#[test]
fn shift_select_skips_filtered_out_rows() {
    // #50: rows hidden by the live filter must not join a shift-range.
    // Sorted panel: A1(0), B9(1), C1(2); filter "1" hides B9.
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    for new in ["C1", "B9", "A1"] {
        let _ = app.on_layer_new();
        let fresh = app.tabs[app.active_tab].layers.edit_buf.clone();
        rename_layer(&mut app, &fresh, new);
    }
    let i = app.active_tab;
    app.tabs[i].layers.filter = "1".to_string();
    let _ = app.update(Message::LayerSelect("A1".to_string()));
    app.shift_down = true;
    let _ = app.update(Message::LayerSelect("C1".to_string()));
    app.shift_down = false;
    let multi = app.tabs[i].layers.selected_multi.clone();
    let names: Vec<String> = multi
        .iter()
        .filter_map(|&x| app.tabs[i].layers.layers.get(x).map(|l| l.name.clone()))
        .collect();
    assert!(
        !names.contains(&"B9".to_string()),
        "hidden row must not join the range, got {names:?}"
    );
    assert!(names.contains(&"A1".to_string()) && names.contains(&"C1".to_string()));
}

#[test]
fn layer_toggle_visible_unknown_name_is_noop() {
    let mut app = app_with_zulu_alpha();
    let i = app.active_tab;
    let before: Vec<(String, bool)> = app.tabs[i]
        .layers
        .layers
        .iter()
        .map(|l| (l.name.clone(), l.visible))
        .collect();
    let _ = app.update(Message::LayerToggleVisible("DELETED".to_string()));
    let after: Vec<(String, bool)> = app.tabs[i]
        .layers
        .layers
        .iter()
        .map(|l| (l.name.clone(), l.visible))
        .collect();
    assert_eq!(before, after, "unknown name must change nothing");
}

fn panel_transparency(app: &OpenCADStudio, name: &str) -> i32 {
    app.tabs[app.active_tab]
        .layers
        .layers
        .iter()
        .find(|l| l.name == name)
        .expect("layer in panel")
        .transparency
}

#[test]
fn layer_transparency_edit_is_one_undo_step_on_enter() {
    // #43 / #2610: typing fills the field only; Enter applies it as one
    // undo step, however many keys were typed.
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.finish_pending_history(i);
    let before = app.tabs[i].history.undo_stack.len();
    for typed in ["5", "50"] {
        let _ = app.update(Message::LayerTransparencyEdit(
            "0".to_string(),
            typed.into(),
        ));
    }
    assert_eq!(
        app.tabs[i].history.undo_stack.len(),
        before,
        "typing alone changes nothing"
    );
    assert_eq!(panel_transparency(&app, "0"), 0);
    let _ = app.update(Message::LayerTransparencyCommit);
    assert_eq!(
        app.tabs[i].history.undo_stack.len(),
        before + 1,
        "one undo step"
    );
    assert_eq!(panel_transparency(&app, "0"), 50);
    assert!(app.tabs[i].layers.transparency_edit.is_none());
    app.undo_active_tab();
    assert_eq!(
        panel_transparency(&app, "0"),
        0,
        "undo must restore transparency"
    );
}

#[test]
fn layer_transparency_out_of_range_says_so_and_keeps_the_value() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.finish_pending_history(i);
    let before = app.tabs[i].history.undo_stack.len();
    for typed in ["95", "x"] {
        app.command_line.last_error = None;
        let _ = app.update(Message::LayerTransparencyEdit(
            "0".to_string(),
            typed.into(),
        ));
        let _ = app.update(Message::LayerTransparencyCommit);
        assert!(
            app.command_line.last_error.is_some(),
            "{typed} is refused with a message"
        );
    }
    assert_eq!(panel_transparency(&app, "0"), 0);
    assert_eq!(app.tabs[i].history.undo_stack.len(), before);
    // Leaving the field applies like Enter; Esc drops what was typed.
    let _ = app.update(Message::LayerTransparencyEdit("0".to_string(), "30".into()));
    let _ = app.update(Message::LayerTransparencyFocus(None));
    assert_eq!(panel_transparency(&app, "0"), 30);
    let _ = app.update(Message::LayerTransparencyEdit("0".to_string(), "60".into()));
    let _ = app.update(Message::ShortcutPressed("ESCAPE".into()));
    assert!(app.tabs[i].layers.transparency_edit.is_none());
    assert_eq!(panel_transparency(&app, "0"), 30);
}

#[test]
fn layer_rename_to_a_taken_name_says_why_and_stays_open() {
    let mut app = app_with_zulu_alpha();
    let i = app.active_tab;
    app.finish_pending_history(i);
    let before = app.tabs[i].history.undo_stack.len();
    let _ = app.update(Message::LayerRenameStart("ZULU".to_string()));
    let _ = app.update(Message::LayerRenameEdit("alpha".to_string()));
    app.command_line.last_error = None;
    let _ = app.update(Message::LayerRenameCommit);
    assert!(app.command_line.last_error.is_some(), "the reason is shown");
    assert_eq!(
        app.tabs[i].layers.editing.as_deref(),
        Some("ZULU"),
        "field stays open"
    );
    assert_eq!(app.tabs[i].layers.edit_buf, "alpha", "with what was typed");
    assert!(app.tabs[i].scene.document.layers.contains("ZULU"));
    assert_eq!(app.tabs[i].history.undo_stack.len(), before, "no undo step");
    // Esc gives up: the old name stays and the field closes.
    let _ = app.update(Message::ShortcutPressed("ESCAPE".into()));
    assert!(app.tabs[i].layers.editing.is_none());
    assert!(app.tabs[i].layers.edit_buf.is_empty());
    // Invalid characters and protected layers are refused the same way.
    for (layer, typed) in [("ZULU", "A*B"), ("0", "ZERO")] {
        let _ = app.update(Message::LayerRenameStart(layer.to_string()));
        let _ = app.update(Message::LayerRenameEdit(typed.to_string()));
        app.command_line.last_error = None;
        let _ = app.update(Message::LayerRenameCommit);
        assert!(
            app.command_line.last_error.is_some(),
            "{layer} → {typed} refused"
        );
        assert!(app.tabs[i].scene.document.layers.contains(layer));
        let _ = app.update(Message::ShortcutPressed("ESCAPE".into()));
    }
    // A click elsewhere after a refused name keeps the old one and moves on.
    let _ = app.update(Message::LayerRenameStart("ZULU".to_string()));
    let _ = app.update(Message::LayerRenameEdit("ALPHA".to_string()));
    let _ = app.update(Message::LayerRenameFocus(None));
    assert!(app.tabs[i].layers.editing.is_none());
    assert!(app.tabs[i].scene.document.layers.contains("ZULU"));
}

#[test]
fn ctrl_click_on_a_layer_name_extends_the_selection() {
    let mut app = app_with_zulu_alpha();
    let i = app.active_tab;
    let _ = app.update(Message::LayerSelect("ALPHA".to_string()));
    app.ctrl_down = true;
    let _ = app.update(Message::LayerRenameStart("ZULU".to_string()));
    app.ctrl_down = false;
    assert!(app.tabs[i].layers.editing.is_none(), "no rename starts");
    let selected: Vec<String> = app.tabs[i]
        .layers
        .selected_multi
        .iter()
        .map(|&idx| app.tabs[i].layers.layers[idx].name.clone())
        .collect();
    assert!(selected.contains(&"ALPHA".to_string()) && selected.contains(&"ZULU".to_string()));
}

fn selected(app: &OpenCADStudio) -> Vec<String> {
    let mut names = app.tabs[app.active_tab].layers.selected_names();
    names.sort();
    names
}

/// #2610: a click on a name selects; the keys then work the table until a
/// click in the drawing.
#[test]
fn layer_table_keys_after_a_click() {
    use crate::app::ArrowKey;
    let mut app = app_with_zulu_alpha();
    let i = app.active_tab;
    // Name order, ascending (the default): 0, ALPHA, ZULU.
    app.show_layers = true;
    app.dock_open_at_default(crate::ui::dock::PanelId::Layers);
    let _ = app.update(Message::LayerSelect("0".into()));
    assert!(app.tabs[i].layers.keys_active);
    assert!(
        app.tabs[i].layers.editing.is_none(),
        "a click doesn't rename"
    );
    let arrow = |direction, extend_selection| Message::ArrowKeyPressed {
        direction,
        shortcut: String::new(),
        extend_selection,
    };
    let _ = app.update(arrow(ArrowKey::Down, false));
    assert_eq!(selected(&app), ["ALPHA"]);
    let _ = app.update(arrow(ArrowKey::Down, true));
    assert_eq!(selected(&app), ["ALPHA", "ZULU"]);
    let _ = app.update(Message::ShortcutPressed("ESCAPE".into()));
    assert!(selected(&app).is_empty());
    let _ = app.update(Message::LayerSelect("ZULU".into()));
    let _ = app.update(Message::ShortcutPressed("CTRL+A".into()));
    assert_eq!(selected(&app).len(), 3);
    let _ = app.update(Message::LayerSelect("ZULU".into()));
    let _ = app.update(Message::ShortcutPressed("F2".into()));
    assert_eq!(app.tabs[i].layers.editing.as_deref(), Some("ZULU"));
    let _ = app.update(Message::ShortcutPressed("ESCAPE".into()));
    // A click in the drawing hands the keys back.
    let _ = app.update(Message::ViewportLeftPress);
    assert!(!app.tabs[i].layers.keys_active);
    let _ = app.update(arrow(ArrowKey::Up, false));
    assert_eq!(
        selected(&app),
        ["ZULU"],
        "the arrow no longer moves the rows"
    );
}

#[test]
fn layer_row_menu_acts_on_the_row_or_the_selection_it_is_in() {
    use crate::ui::window::layers::LayerMenuAction;
    use document::entities::Line;
    use document::types::Vector3;
    let mut app = app_with_zulu_alpha();
    let i = app.active_tab;
    let mut line = Line::from_points(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
    line.common.layer = "ZULU".into();
    let on_zulu = app.tabs[i]
        .scene
        .add_entity(document::EntityType::Line(line));
    let _ = app.tabs[i]
        .scene
        .add_entity(document::EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::new(2.0, 0.0, 0.0),
        )));
    let _ = app.update(Message::LayerSelect("ALPHA".into()));
    // A row outside the selection becomes the selection first.
    let _ = app.update(Message::LayerMenu(
        "ZULU".into(),
        LayerMenuAction::SelectObjects,
    ));
    assert_eq!(selected(&app), ["ZULU"]);
    assert_eq!(app.tabs[i].scene.selected.len(), 1);
    assert!(app.tabs[i].scene.selected.contains(&on_zulu));
    // Select All / Deselect All left the menu (Ctrl+A and Esc do them).
    let _ = app.update(Message::LayerMenu(
        "ALPHA".into(),
        LayerMenuAction::SetCurrent,
    ));
    assert_eq!(
        app.tabs[i].scene.document.header.current_layer_name,
        "ALPHA"
    );
}

#[test]
fn a_new_layer_takes_the_selected_layers_look() {
    let mut app = app_with_zulu_alpha();
    let i = app.active_tab;
    {
        let zulu = app.tabs[i].scene.document.layers.get_mut("ZULU").unwrap();
        zulu.color = document::types::Color::Index(3);
        zulu.line_type = "HIDDEN".into();
        zulu.is_plottable = false;
    }
    app.sync_layer_panel(i);
    let _ = app.update(Message::LayerSelect("ZULU".into()));
    let _ = app.on_layer_new();
    let name = app.tabs[i].layers.edit_buf.clone();
    let layer = app.tabs[i].scene.document.layers.get(&name).unwrap();
    assert_eq!(layer.color, document::types::Color::Index(3));
    assert_eq!(layer.line_type, "HIDDEN");
    assert!(!layer.is_plottable);
}

#[test]
fn a_new_layer_can_start_frozen_in_every_viewport() {
    let mut app = app_with_zulu_alpha();
    let i = app.active_tab;
    let viewport = app.tabs[i]
        .scene
        .add_entity(document::EntityType::Viewport(Box::new(
            document::entities::Viewport::new(),
        )));
    app.tabs[i].layers.selected = None;
    app.tabs[i].layers.selected_multi.clear();
    let _ = app.update(Message::LayerNewVpFrozen);
    let name = app.tabs[i].layers.edit_buf.clone();
    let handle = app.tabs[i].scene.document.layers.get(&name).unwrap().handle;
    let Some(document::EntityType::Viewport(vp)) = app.tabs[i].scene.document.get_entity(viewport)
    else {
        panic!("the viewport stays");
    };
    assert!(vp.frozen_layers.contains(&handle));
    app.undo_active_tab();
    assert!(
        !app.tabs[i].scene.document.layers.contains(&name),
        "one undo step"
    );
}

fn row(app: &OpenCADStudio, name: &str) -> crate::ui::window::layers::Layer {
    app.tabs[app.active_tab]
        .layers
        .layers
        .iter()
        .find(|l| l.name == name)
        .expect("layer in panel")
        .clone()
}

/// #2610: the Objects column counts the layer's objects, block contents
/// apart, and follows the drawing.
#[test]
fn layer_object_counts_follow_the_drawing() {
    use document::entities::Line;
    use document::types::Vector3;
    let mut app = app_with_zulu_alpha();
    let i = app.active_tab;
    app.show_layers = true;
    app.dock_open_at_default(crate::ui::dock::PanelId::Layers);
    let line_on = |layer: &str| {
        let mut line = Line::from_points(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
        line.common.layer = layer.into();
        document::EntityType::Line(line)
    };
    let first = app.tabs[i].scene.add_entity(line_on("ZULU"));
    let _ = app.tabs[i].scene.add_entity(line_on("zulu"));
    // One more inside a block definition.
    let mut block = document::tables::BlockRecord::new("DOOR");
    block.handle = app.tabs[i].scene.document.allocate_handle();
    let owner = block.handle;
    let _ = app.tabs[i].scene.document.block_records.add(block);
    let mut inside = line_on("ZULU");
    inside.common_mut().owner_handle = owner;
    let _ = app.tabs[i].scene.document.add_entity(inside);
    app.tabs[i].scene.bump_geometry();
    let _ = app.update(Message::LayerSort(LayerSortCol::Name));
    assert_eq!(
        (row(&app, "ZULU").objects, row(&app, "ZULU").in_blocks),
        (3, 1)
    );
    assert_eq!(row(&app, "ALPHA").objects, 0);
    let _ = app.tabs[i].scene.document.remove_entity(first);
    app.tabs[i].scene.bump_geometry();
    let _ = app.update(Message::LayerSort(LayerSortCol::Name));
    assert_eq!(
        row(&app, "ZULU").objects,
        2,
        "counted again after the change"
    );
}

fn shown(app: &OpenCADStudio) -> Vec<String> {
    let panel = &app.tabs[app.active_tab].layers;
    let mut names: Vec<String> = panel
        .visible_indices()
        .into_iter()
        .map(|row| panel.layers[row].name.clone())
        .collect();
    names.sort();
    names
}

/// #2610: the filter tree's nodes pick the layers the table shows.
#[test]
fn filter_tree_nodes_choose_the_rows() {
    use crate::ui::window::layers::TreeNode;
    use document::entities::Line;
    use document::types::Vector3;
    let mut app = app_with_zulu_alpha();
    let i = app.active_tab;
    app.show_layers = true;
    app.dock_open_at_default(crate::ui::dock::PanelId::Layers);
    let mut line = Line::from_points(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
    line.common.layer = "ZULU".into();
    let on_zulu = app.tabs[i]
        .scene
        .add_entity(document::EntityType::Line(line));
    app.tabs[i].scene.bump_geometry();
    let mut xref = document::tables::Layer::new("SITE|ROAD");
    xref.handle = app.tabs[i].scene.document.allocate_handle();
    let _ = app.tabs[i].scene.document.layers.add(xref);
    app.sync_layer_panel(i);
    let _ = app.update(Message::LayerShow(TreeNode::Used));
    assert_eq!(shown(&app), ["ZULU"]);
    let _ = app.update(Message::LayerInvert(true));
    assert_eq!(shown(&app), ["0", "ALPHA", "SITE|ROAD"]);
    let _ = app.update(Message::LayerInvert(false));
    let _ = app.update(Message::LayerShow(TreeNode::Unused));
    assert_eq!(shown(&app), ["0", "ALPHA", "SITE|ROAD"]);
    assert!(app.tabs[i]
        .layers
        .tree_nodes()
        .iter()
        .any(|(n, _)| *n == TreeNode::Xref("SITE".into())));
    let _ = app.update(Message::LayerShow(TreeNode::Xref("site".into())));
    assert_eq!(shown(&app), ["SITE|ROAD"]);
    // Layers of the selected objects follow the selection.
    app.tabs[i].scene.select_entity(on_zulu, false);
    let _ = app.update(Message::LayerShow(TreeNode::Selected));
    assert_eq!(shown(&app), ["ZULU"]);
    // A LAYERFILTER filter is a node too.
    let _ = app.run_command_line("LAYERFILTER NEW LETTERS A*,Z*");
    let _ = app.update(Message::LayerShow(TreeNode::Filter("LETTERS".into())));
    assert_eq!(shown(&app), ["ALPHA", "ZULU"]);
    let _ = app.update(Message::LayerShow(TreeNode::All));
    assert_eq!(shown(&app).len(), 4);
}

/// #2610 (J6): layer filters live in the drawing — one undo step to make,
/// undone like anything else; a filter under another sits deeper in the
/// tree and takes only what its parent does; the search text becomes a
/// filter; the ribbon's list can follow the tree.
#[test]
fn layer_filters_in_the_drawing_nested_saved_from_search() {
    use crate::app::filter_dialog::FilterMsg;
    use crate::filter::{Condition, Filter, Item, Match};
    use crate::ui::window::layers::TreeNode;
    let mut app = app_with_zulu_alpha();
    let i = app.active_tab;
    app.show_layers = true;
    app.dock_open_at_default(crate::ui::dock::PanelId::Layers);
    app.finish_pending_history(i);
    let _ = app.run_command_line("LAYERFILTER NEW LETTERS A*,Z*");
    let stored = |app: &OpenCADStudio| {
        crate::app::layer_filter_store::stored_layer_filters(
            &app.tabs[app.active_tab].scene.document,
        )
    };
    assert_eq!(stored(&app).len(), 1, "kept in the drawing");
    app.undo_active_tab();
    assert!(stored(&app).is_empty());
    app.refresh_layer_counts();
    assert!(
        app.tabs[i].layers.filters.is_empty(),
        "undone in the tree too"
    );
    let _ = app.run_command_line("LAYERFILTER NEW LETTERS A*,Z*");
    // ZULUS under LETTERS: Z* within A*,Z*.
    app.filter_open_layers(
        "ZULUS".into(),
        Filter {
            root: Match::All,
            items: vec![Item::Condition(Condition {
                property: Some("name".into()),
                value: "Z*".into(),
                ..Default::default()
            })],
        },
    );
    let _ = app.update(Message::FilterDialog(FilterMsg::LayerParent(
        "LETTERS".into(),
    )));
    let _ = app.update(Message::FilterDialog(FilterMsg::SaveLayerFilter));
    let saved = stored(&app);
    let zulus = saved.iter().find(|f| f.name == "ZULUS").unwrap();
    assert_eq!(zulus.parent.as_deref(), Some("LETTERS"));
    app.refresh_layer_counts();
    let panel = &app.tabs[i].layers;
    let at = panel
        .filters
        .iter()
        .position(|(f, _)| f == "ZULUS")
        .unwrap();
    assert_eq!(panel.filter_depths[at], 1);
    assert_eq!(panel.filters[at].1, ["ZULU"]);
    // The search text as a filter.
    app.tabs[i].layers.filter = "AL".into();
    let _ = app.update(Message::LayerSaveSearch);
    assert!(stored(&app).iter().any(|f| f.name == "AL"));
    assert_eq!(app.tabs[i].layers.view, TreeNode::Filter("AL".into()));
    assert!(app.tabs[i].layers.filter.is_empty());
    // The ribbon follows the tree: ALPHA (and the current layer).
    let _ = app.update(Message::LayerRibbonFollows(true));
    let _ = app.update(Message::LayerShow(TreeNode::Filter("AL".into())));
    let names: Vec<String> = app
        .ribbon
        .layer_infos
        .iter()
        .map(|l| l.name.clone())
        .collect();
    assert!(names.contains(&"ALPHA".to_string()));
    assert!(!names.contains(&"ZULU".to_string()) || app.tabs[i].active_layer == "ZULU");
    let _ = app.update(Message::LayerRibbonFollows(false));
}

/// #2610 (F1): a group with a space in its name is renamed in place and
/// deleted from the tree's menu; a filter is renamed too, and a taken
/// name is refused.
#[test]
fn tree_rename_and_delete_with_spaces() {
    use crate::ui::window::layers::{TreeAction, TreeNode};
    let mut app = app_with_zulu_alpha();
    let i = app.active_tab;
    app.show_layers = true;
    app.dock_open_at_default(crate::ui::dock::PanelId::Layers);
    let _ = app.update(Message::LayerSelect("ZULU".into()));
    let _ = app.update(Message::LayerGroupNew);
    let group = app.tabs[i].layers.groups[0].0.clone();
    assert!(
        group.contains(' '),
        "a default name like `Group 1` has a space"
    );
    let node = TreeNode::Group(group.clone());
    let _ = app.update(Message::LayerTreeAction(node.clone(), TreeAction::Rename));
    assert_eq!(app.tabs[i].layers.tree_rename, Some((node, group.clone())));
    let _ = app.update(Message::LayerTreeRenameEdit("1. csoport".into()));
    let _ = app.update(Message::LayerTreeRenameCommit);
    assert_eq!(app.tabs[i].layers.groups[0].0, "1. csoport");
    assert_eq!(app.tabs[i].layers.groups[0].1, ["ZULU"]);
    let _ = app.update(Message::LayerTreeAction(
        TreeNode::Group("1. csoport".into()),
        TreeAction::Delete,
    ));
    assert!(
        app.tabs[i].layers.groups.is_empty(),
        "deleted despite the space"
    );
    // A filter: renamed; a second one may not take its name.
    let _ = app.run_command_line("LAYERFILTER NEW LETTERS A*,Z*");
    let _ = app.run_command_line("LAYERFILTER NEW OTHERS O*");
    let _ = app.update(Message::LayerTreeAction(
        TreeNode::Filter("LETTERS".into()),
        TreeAction::Rename,
    ));
    let _ = app.update(Message::LayerTreeRenameEdit("Letters A-Z".into()));
    let _ = app.update(Message::LayerTreeRenameCommit);
    assert!(app.layer_filters.iter().any(|f| f.name == "Letters A-Z"));
    let _ = app.update(Message::LayerTreeAction(
        TreeNode::Filter("OTHERS".into()),
        TreeAction::Rename,
    ));
    let _ = app.update(Message::LayerTreeRenameEdit("letters a-z".into()));
    let _ = app.update(Message::LayerTreeRenameCommit);
    assert!(app.layer_filters.iter().any(|f| f.name == "OTHERS"));
}

/// #2610 (F1): the row menu's group ticks add and take out; rows dropped
/// on a group join it; a node's eye lights all its layers; "Unused
/// layers" opens Purge with them ticked.
#[test]
fn groups_from_menu_and_drop_node_eye_and_purge() {
    use crate::ui::window::layers::{LayerMenuAction, TreeAction, TreeNode};
    let mut app = app_with_zulu_alpha();
    let i = app.active_tab;
    app.show_layers = true;
    app.dock_open_at_default(crate::ui::dock::PanelId::Layers);
    let _ = app.run_command_line("LAYERGROUP NEW WALLS ZULU");
    let members = |app: &OpenCADStudio| app.tabs[app.active_tab].layers.groups[0].1.clone();
    // ALPHA in by the menu, out again by its tick.
    let _ = app.update(Message::LayerMenu(
        "ALPHA".into(),
        LayerMenuAction::AddToGroup("WALLS".into()),
    ));
    assert_eq!(members(&app).len(), 2);
    let _ = app.update(Message::LayerMenu(
        "ALPHA".into(),
        LayerMenuAction::AddToGroup("WALLS".into()),
    ));
    assert_eq!(members(&app), ["ZULU"]);
    // A press on ALPHA, released on the group: it joins.
    let _ = app.update(Message::LayerSelect("ALPHA".into()));
    let _ = app.update(Message::LayerDragOver(Some("WALLS".into())));
    assert_eq!(app.tabs[i].layers.drag_over.as_deref(), Some("WALLS"));
    let _ = app.update(Message::LayerDrop("WALLS".into()));
    assert_eq!(members(&app).len(), 2);
    // Released on the row instead: no drop afterwards.
    let _ = app.run_command_line("LAYERGROUP REMOVE WALLS ALPHA");
    let _ = app.update(Message::LayerSelect("ALPHA".into()));
    let _ = app.update(Message::LayerDragEnd);
    let _ = app.update(Message::LayerDrop("WALLS".into()));
    assert_eq!(members(&app), ["ZULU"]);
    // The group's eye lights ZULU only; again, off.
    let node = TreeNode::Group("WALLS".into());
    let _ = app.update(Message::LayerNodeEye(node.clone()));
    assert_eq!(app.tabs[i].layers.eye_layers, ["ZULU"]);
    let _ = app.update(Message::LayerNodeEye(node));
    assert!(app.tabs[i].layers.eye_layers.is_empty());
    // Purge with the unused layers ticked (no objects anywhere).
    let _ = app.update(Message::LayerTreeAction(
        TreeNode::Unused,
        TreeAction::Purge,
    ));
    let state = app.purge_dialog.as_ref().expect("Purge opens");
    let ticked: Vec<&str> = state.items[0]
        .iter()
        .filter(|(_, t)| *t)
        .map(|(n, _)| n.as_str())
        .collect();
    assert!(ticked.contains(&"ZULU") && ticked.contains(&"ALPHA"));
}

/// #2610 (F1): FILTER's "Open in Layer manager" opens the pallet the way
/// LAYERS does, and after closing it LAYERS opens it again (it stayed
/// "open" with no window and could not be opened).
#[test]
fn open_in_layer_manager_then_layers_again() {
    use crate::app::filter_dialog::FilterMsg;
    let mut app = app_with_zulu_alpha();
    app.show_layers = false;
    let _ = app.update(Message::FilterDialog(FilterMsg::Open(None)));
    let _ = app.update(Message::FilterDialog(FilterMsg::ShowLayers));
    assert!(app.show_layers);
    assert!(app.dock_panel_visible(crate::ui::dock::PanelId::Layers));
    let _ = app.update(Message::ToggleLayers);
    assert!(!app.show_layers);
    let _ = app.update(Message::ToggleLayers);
    assert!(app.show_layers);
    assert!(app.dock_panel_visible(crate::ui::dock::PanelId::Layers));
}

/// #2610: the tree as in the mockup: "All" with its count, Groups and
/// Property filters as sections that close, a group deleted from its
/// node's menu, and New Filter starting empty whatever the search says.
#[test]
fn layer_tree_sections_counts_and_group_delete() {
    use crate::ui::window::layers::{TreeAction, TreeNode, TreeRow, TreeSection};
    let mut app = app_with_zulu_alpha();
    let i = app.active_tab;
    app.show_layers = true;
    app.dock_open_at_default(crate::ui::dock::PanelId::Layers);
    let _ = app.run_command_line("LAYERGROUP NEW WALLS ZULU");
    let _ = app.run_command_line("LAYERFILTER NEW LETTERS A*,Z*");
    app.sync_layer_panel(i);
    app.refresh_layer_counts();
    let rows = app.tabs[i].layers.tree_rows();
    let group = TreeRow::Node(TreeNode::Group("WALLS".into()), 2);
    let filter = TreeRow::Node(TreeNode::Filter("LETTERS".into()), 2);
    assert_eq!(rows[0], TreeRow::Section(TreeSection::All, 0));
    assert!(rows.contains(&TreeRow::Section(TreeSection::Groups, 1)));
    assert!(rows.contains(&group));
    assert!(rows.contains(&filter));
    let panel = &app.tabs[i].layers;
    assert_eq!(panel.node_count(&TreeNode::Group("WALLS".into())), 1);
    assert_eq!(panel.node_count(&TreeNode::Filter("LETTERS".into())), 2);
    // A closed section hides its nodes, "All" closed hides the rest.
    let _ = app.update(Message::LayerTreeSection(TreeSection::Groups));
    let rows = app.tabs[i].layers.tree_rows();
    assert!(!rows.contains(&group) && rows.contains(&filter));
    let _ = app.update(Message::LayerTreeSection(TreeSection::All));
    assert_eq!(app.tabs[i].layers.tree_rows().len(), 1);
    let _ = app.update(Message::LayerTreeSection(TreeSection::All));
    let _ = app.update(Message::LayerTreeSection(TreeSection::Groups));
    assert!(app.tabs[i].layers.tree_rows().contains(&group));
    // The group goes from its node's menu; the node shown falls back.
    let _ = app.update(Message::LayerShow(TreeNode::Group("WALLS".into())));
    let _ = app.update(Message::LayerTreeAction(
        TreeNode::Group("WALLS".into()),
        TreeAction::Delete,
    ));
    app.sync_layer_panel(i);
    assert_eq!(app.tabs[i].layers.view, TreeNode::All);
    assert!(app.tabs[i].layers.groups.is_empty());
    assert!(app.tabs[i].layers.layers.iter().any(|l| l.name == "ZULU"));
    // New Filter: no first row from the search text.
    app.tabs[i].layers.filter = "LU".into();
    let _ = app.update(Message::LayerFilterEdit(Some(String::new())));
    let state = app.filter_dialog.as_ref().expect("FILTER over the layers");
    assert_eq!(state.live_count, 3, "an empty row finds every layer");
}

/// #2610: a layer filter made with FILTER's rows over layer properties,
/// then switched as a whole from its tree node.
#[test]
fn layer_filter_from_the_editor_and_the_node_menu() {
    use crate::app::filter_dialog::{FilterMsg, FilterRow, FilterRowEdit};
    use crate::ui::window::layers::{TreeAction, TreeNode};
    let mut app = app_with_zulu_alpha();
    let i = app.active_tab;
    app.show_layers = true;
    app.dock_open_at_default(crate::ui::dock::PanelId::Layers);
    // The search text becomes the first row.
    app.tabs[i].layers.filter = "LU".into();
    let _ = app.update(Message::LayerFilterEdit(None));
    let state = app.filter_dialog.as_ref().expect("FILTER over the layers");
    assert_eq!(state.layer_filter.as_deref(), Some(""));
    assert_eq!(state.live_count, 1, "*LU* finds ZULU");
    assert!(state
        .properties_for(None)
        .iter()
        .any(|p| p.field == "frozen"));
    // Or: not frozen and named A* or Z*.
    let edit = |app: &mut OpenCADStudio, row, e| {
        let _ = app.update(Message::FilterDialog(FilterMsg::Edit(row, e)));
    };
    edit(
        &mut app,
        FilterRow::Root(0),
        FilterRowEdit::Value("A*,Z*".into()),
    );
    assert_eq!(app.filter_dialog.as_ref().unwrap().live_count, 2);
    let _ = app.update(Message::FilterDialog(FilterMsg::LayerName(
        "Letters".into(),
    )));
    let _ = app.update(Message::FilterDialog(FilterMsg::SaveLayerFilter));
    assert!(app.filter_dialog.is_none());
    let node = TreeNode::Filter("Letters".into());
    assert_eq!(app.tabs[i].layers.view, node);
    assert_eq!(app.tabs[i].layers.node_layers(&node).len(), 2);
    // The node's menu freezes both at once, one undo step.
    app.finish_pending_history(i);
    let before = app.tabs[i].history.undo_stack.len();
    let _ = app.update(Message::LayerTreeAction(node.clone(), TreeAction::Freeze));
    assert!(app.tabs[i]
        .scene
        .document
        .layers
        .get("ZULU")
        .unwrap()
        .is_frozen());
    assert!(app.tabs[i]
        .scene
        .document
        .layers
        .get("ALPHA")
        .unwrap()
        .is_frozen());
    assert_eq!(app.tabs[i].history.undo_stack.len(), before + 1);
    // Edit opens it again; Delete removes it, the layers stay.
    let _ = app.update(Message::LayerTreeAction(node.clone(), TreeAction::Edit));
    assert_eq!(
        app.filter_dialog.as_ref().unwrap().layer_filter.as_deref(),
        Some("Letters")
    );
    let _ = app.update(Message::FilterDialog(FilterMsg::Close));
    let _ = app.update(Message::LayerTreeAction(node, TreeAction::Delete));
    assert!(app.layer_filters.is_empty());
    assert_eq!(app.tabs[i].layers.view, TreeNode::All);
}

/// #2610: LAYERP undoes layer-property changes only, newest first, and
/// says so when there is nothing left.
#[test]
fn layerp_steps_back_through_layer_changes() {
    use document::entities::Line;
    use document::types::Vector3;
    let mut app = app_with_zulu_alpha();
    let i = app.active_tab;
    let _ = app.update(Message::LayerToggleFreeze("ZULU".into()));
    let _ = app.update(Message::LayerToggleLock("ALPHA".into()));
    // A drawing change in between is not LAYERP's.
    let line = app.tabs[i]
        .scene
        .add_entity(document::EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::new(1.0, 0.0, 0.0),
        )));
    let _ = app.run_command_line("LAYERP");
    assert!(!app.tabs[i]
        .scene
        .document
        .layers
        .get("ALPHA")
        .unwrap()
        .is_locked());
    assert!(app.tabs[i]
        .scene
        .document
        .layers
        .get("ZULU")
        .unwrap()
        .is_frozen());
    let _ = app.run_command_line("LAYERP");
    assert!(!app.tabs[i]
        .scene
        .document
        .layers
        .get("ZULU")
        .unwrap()
        .is_frozen());
    assert!(app.tabs[i].scene.document.get_entity(line).is_some());
    app.command_line.last_error = None;
    let before = app.command_line.history.len();
    let _ = app.run_command_line("LAYERP");
    assert!(
        app.command_line.history.len() > before,
        "it says there is nothing left"
    );
    // LAYERP itself is one ordinary undo step.
    app.undo_active_tab();
    assert!(app.tabs[i]
        .scene
        .document
        .layers
        .get("ZULU")
        .unwrap()
        .is_frozen());
}

/// #2610: Show lights the selected layers' objects; FILTER's result can
/// open in the manager; a row can start FILTER and take merges.
#[test]
fn layer_manager_and_drawing_work_together() {
    use crate::app::filter_dialog::{FilterMsg, FilterRow, FilterRowEdit};
    use crate::ui::window::layers::{LayerMenuAction, TreeNode};
    use document::entities::{Circle, Line};
    use document::types::Vector3;
    let mut app = app_with_zulu_alpha();
    let i = app.active_tab;
    app.show_layers = true;
    app.dock_open_at_default(crate::ui::dock::PanelId::Layers);
    let mut line = Line::from_points(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
    line.common.layer = "ZULU".into();
    let on_zulu = app.tabs[i]
        .scene
        .add_entity(document::EntityType::Line(line));
    let mut circle = Circle::new();
    circle.common.layer = "ALPHA".into();
    let on_alpha = app.tabs[i]
        .scene
        .add_entity(document::EntityType::Circle(circle));
    app.tabs[i].scene.bump_geometry();

    let _ = app.update(Message::LayerEye("ZULU".into()));
    let lit = app.tabs[i].scene.hover_highlight_handles();
    assert!(lit.contains(&on_zulu) && !lit.contains(&on_alpha));
    assert_eq!(app.tabs[i].layers.highlighted, 1);
    assert!(
        app.tabs[i].scene.selected.is_empty(),
        "showing doesn't select"
    );
    // Two eyes light both layers; the eye of ZULU off leaves ALPHA.
    let _ = app.update(Message::LayerEye("alpha".into()));
    assert_eq!(app.tabs[i].layers.highlighted, 2);
    let _ = app.update(Message::LayerEye("zulu".into()));
    let lit = app.tabs[i].scene.hover_highlight_handles();
    assert!(!lit.contains(&on_zulu) && lit.contains(&on_alpha));
    let _ = app.update(Message::LayerEye("ALPHA".into()));
    assert!(app.tabs[i].scene.hover_highlight_handles().is_empty());

    // A row's "Filter objects..." starts FILTER on its layer.
    let _ = app.update(Message::LayerMenu(
        "ALPHA".into(),
        LayerMenuAction::FilterObjects,
    ));
    assert_eq!(app.filter_dialog.as_ref().unwrap().live_count, 1);
    // FILTER's result opens in the manager as its own node.
    let _ = app.update(Message::FilterDialog(FilterMsg::Edit(
        FilterRow::Root(0),
        FilterRowEdit::Value("*".into()),
    )));
    let _ = app.update(Message::FilterDialog(FilterMsg::ShowLayers));
    assert!(app.filter_dialog.is_none());
    assert_eq!(app.tabs[i].layers.view, TreeNode::Found);
    assert_eq!(
        app.tabs[i].layers.node_layers(&TreeNode::Found),
        ["ALPHA", "ZULU"]
    );

    // Merge: ZULU selected, right-click ALPHA.
    let _ = app.update(Message::LayerShow(TreeNode::All));
    let _ = app.update(Message::LayerSelect("ZULU".into()));
    let _ = app.update(Message::LayerMenu(
        "ALPHA".into(),
        LayerMenuAction::MergeHere,
    ));
    assert!(!app.tabs[i].scene.document.layers.contains("ZULU"));
    assert_eq!(
        app.tabs[i]
            .scene
            .document
            .get_entity(on_zulu)
            .unwrap()
            .common()
            .layer,
        "ALPHA"
    );
}

#[test]
fn layer_description_is_typed_and_applied_in_one_step() {
    let mut app = app_with_zulu_alpha();
    let i = app.active_tab;
    app.finish_pending_history(i);
    let before = app.tabs[i].history.undo_stack.len();
    for typed in ["W", "Walls"] {
        let _ = app.update(Message::LayerDescriptionEdit("ZULU".into(), typed.into()));
    }
    assert_eq!(
        app.tabs[i].history.undo_stack.len(),
        before,
        "typing alone changes nothing"
    );
    let _ = app.update(Message::LayerDescriptionFocus(None));
    assert_eq!(
        app.tabs[i]
            .scene
            .document
            .layers
            .get("ZULU")
            .unwrap()
            .description,
        "Walls"
    );
    assert_eq!(row(&app, "ZULU").description, "Walls");
    assert_eq!(app.tabs[i].history.undo_stack.len(), before + 1);
    let _ = app.update(Message::LayerDescriptionEdit("ZULU".into(), "Doors".into()));
    let _ = app.update(Message::ShortcutPressed("ESCAPE".into()));
    assert_eq!(
        app.tabs[i]
            .scene
            .document
            .layers
            .get("ZULU")
            .unwrap()
            .description,
        "Walls"
    );
    app.undo_active_tab();
    assert!(app.tabs[i]
        .scene
        .document
        .layers
        .get("ZULU")
        .unwrap()
        .description
        .is_empty());
}

#[test]
fn plot_toggle_keeps_the_row_order_like_the_other_toggles() {
    let mut app = app_with_zulu_alpha();
    let i = app.active_tab;
    let _ = app.update(Message::LayerSort(LayerSortCol::Plot));
    let order = |app: &OpenCADStudio| -> Vec<String> {
        app.tabs[i]
            .layers
            .layers
            .iter()
            .map(|l| l.name.clone())
            .collect()
    };
    let before = order(&app);
    let _ = app.update(Message::LayerTogglePlot("ZULU".to_string()));
    assert_eq!(order(&app), before);
    assert!(
        !app.tabs[i]
            .scene
            .document
            .layers
            .get("ZULU")
            .unwrap()
            .is_plottable
    );
}
