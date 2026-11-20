use super::*;
use crate::app::OpenCADStudio;

/// The tally is per thread, so tests running in parallel do not see each
/// other's rebuilds.
fn expensive_runs() -> usize {
    TREE_EXPENSIVE_RUNS.with(std::cell::Cell::get)
}

fn reset_expensive_runs() {
    TREE_EXPENSIVE_RUNS.with(|runs| runs.set(0));
}

fn app_with_layer_panel() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    for k in 0..6u8 {
        let mut layer = document::tables::Layer::new(format!("L{k}"));
        layer.handle = app.tabs[i].scene.document.allocate_handle();
        layer.color = document::types::Color::Index(k + 1);
        let _ = app.tabs[i].scene.document.layers.add(layer);
    }
    for k in 1..6u8 {
        let mut entity = document::EntityType::Line(document::entities::Line::new());
        entity.common_mut().layer = format!("L{k}");
        app.tabs[i].scene.add_entity(entity);
    }
    app.show_layers = true;
    app.dock.float(crate::ui::dock::FloatPanel::new(
        crate::ui::dock::PanelId::Layers,
        50.0,
        50.0,
        1300.0,
        700.0,
    ));
    assert!(
        app.dock_panel_visible(crate::ui::dock::PanelId::Layers),
        "the harness must have the Layers panel showing",
    );
    app
}

/// #11: an idle message (nothing changed since the last refresh) must not
/// rerun the filter-tree rebuild. Before the gate every message reloaded
/// the drawing's filters and Debug-formatted every layer for the key.
#[test]
fn idle_layer_refresh_skips_the_expensive_rebuild() {
    let mut app = app_with_layer_panel();
    app.refresh_layer_counts();
    assert!(
        expensive_runs() > 0,
        "the first refresh must build the tree"
    );
    reset_expensive_runs();
    app.refresh_layer_counts();
    assert_eq!(
        expensive_runs(),
        0,
        "an idle refresh must not rerun the filter-tree rebuild",
    );
}

/// The gate must read layer content, not just `geometry_epoch`: a property
/// change on a layer with no entities bumps no geometry epoch, yet it can
/// move the layer into a property filter.
#[test]
fn a_layer_property_change_still_refreshes_the_tree() {
    let mut app = app_with_layer_panel();
    let i = app.active_tab;
    {
        let mut empty = document::tables::Layer::new("EMPTY");
        empty.handle = app.tabs[i].scene.document.allocate_handle();
        empty.color = document::types::Color::Index(1);
        let _ = app.tabs[i].scene.document.layers.add(empty);
    }
    app.layer_filters = vec![crate::app::layer_filters::LayerFilter {
        name: "REDS".into(),
        predicates: vec![crate::app::layer_filters::LayerPredicate::Color(5)],
        ..Default::default()
    }];
    app.persist_layer_filters(i, "TEST FILTERS");
    app.refresh_layer_counts();
    let in_reds = |app: &OpenCADStudio| {
        app.tabs[i]
            .layers
            .filters
            .iter()
            .find(|(name, _)| name == "REDS")
            .map(|(_, members)| members.iter().any(|n| n == "EMPTY"))
            .unwrap_or(false)
    };
    assert!(!in_reds(&app), "a colour-1 layer is not in the reds filter");
    app.edit_layers(i, "TEST COLOR", &["EMPTY".to_string()], |layer| {
        layer.color = document::types::Color::Index(5);
    });
    app.refresh_layer_counts();
    assert!(
        in_reds(&app),
        "recolouring an empty layer must carry it into the filter",
    );
}

/// `edit_layers` bumps the geometry epoch even when the edit writes back
/// the value the layer already had. The tree shows layer properties, not
/// geometry, so that edit must not rebuild it.
#[test]
fn an_edit_that_leaves_the_tree_inputs_alone_skips_the_rebuild() {
    let mut app = app_with_layer_panel();
    let i = app.active_tab;
    app.refresh_layer_counts();
    assert!(
        expensive_runs() > 0,
        "the first refresh must build the tree"
    );
    reset_expensive_runs();
    app.edit_layers(i, "TEST NOOP", &["L2".to_string()], |layer| {
        let color = layer.color;
        layer.color = color;
    });
    app.refresh_layer_counts();
    assert_eq!(
        expensive_runs(),
        0,
        "an edit that changes nothing the tree shows must not rebuild it",
    );
}

/// The selected-objects row follows the objects' layers, so carrying a
/// selected object to another layer must rebuild the tree.
#[test]
fn moving_a_selected_object_to_another_layer_refreshes_the_tree() {
    let mut app = app_with_layer_panel();
    let i = app.active_tab;
    let mut line = document::entities::Line::new();
    line.common.layer = "L1".into();
    let handle = app.tabs[i]
        .scene
        .add_entity(document::EntityType::Line(line));
    app.tabs[i].scene.selected.insert(handle);
    app.refresh_layer_counts();
    assert!(
        app.tabs[i]
            .layers
            .selected_object_layers
            .contains(&"l1".to_string()),
        "the row must start on l1",
    );
    let entity = app.tabs[i].scene.document.get_entity_mut(handle).unwrap();
    entity.common_mut().layer = "L3".into();
    app.refresh_layer_counts();
    assert_eq!(
        app.tabs[i].layers.selected_object_layers,
        vec!["l3".to_string()],
        "the row must follow the object to l3",
    );
}

/// Selecting one more object on a layer the row already lists leaves the
/// row as it was, so the panel must not rebuild for it.
#[test]
fn selecting_another_object_on_the_same_layer_skips_the_rebuild() {
    let mut app = app_with_layer_panel();
    let i = app.active_tab;
    let mut first = document::entities::Line::new();
    first.common.layer = "L1".into();
    let first = app.tabs[i]
        .scene
        .add_entity(document::EntityType::Line(first));
    let mut second = document::entities::Line::new();
    second.common.layer = "L1".into();
    let second = app.tabs[i]
        .scene
        .add_entity(document::EntityType::Line(second));
    app.tabs[i].scene.selected.insert(first);
    app.refresh_layer_counts();
    assert!(
        expensive_runs() > 0,
        "the first selection must build the tree"
    );
    reset_expensive_runs();
    app.tabs[i].scene.selected.insert(second);
    app.refresh_layer_counts();
    assert_eq!(
        expensive_runs(),
        0,
        "another object on the same layer must not rebuild the tree",
    );
}

/// The drawing's filter table can change behind the session's back: a file
/// just opened into the tab, another part of the app writing it. The panel
/// shows the drawing's filters, so the gate must reload them when that
/// table moves.
#[test]
fn a_drawing_side_filter_change_reloads_the_filters() {
    let mut app = app_with_layer_panel();
    let i = app.active_tab;
    app.layer_filters = vec![crate::app::layer_filters::LayerFilter {
        name: "KEEP".into(),
        pattern: "L*".into(),
        ..Default::default()
    }];
    app.persist_layer_filters(i, "TEST FILTERS");
    app.refresh_layer_counts();
    assert_eq!(app.tabs[i].layers.filters.len(), 1);
    crate::app::layer_filter_store::write_layer_filters(&mut app.tabs[i].scene.document, &[]);
    app.refresh_layer_counts();
    assert!(
        app.tabs[i].layers.filters.is_empty(),
        "the panel must drop a filter the drawing no longer has",
    );
    reset_expensive_runs();
    app.refresh_layer_counts();
    assert_eq!(
        expensive_runs(),
        0,
        "the reload must settle: the next message has nothing to do",
    );
}

/// The filter records the drawing holds, by handle, to see the allocator
/// hand one out again.
fn filter_record_handles(app: &OpenCADStudio, i: usize) -> Vec<u64> {
    let mut handles: Vec<u64> = app.tabs[i]
        .scene
        .document
        .objects
        .iter()
        .filter(|(_, object)| {
            matches!(object,
            document::objects::ObjectType::XRecord(record)
                if record.entries.first().is_some_and(|entry| {
                    entry.code == 1 && entry.value.as_string() == Some("AcLyLayerFilter")
                }))
        })
        .map(|(handle, _)| handle.value())
        .collect();
    handles.sort_unstable();
    handles
}

/// A filter write is an undo step, and undo rewinds the drawing's handle
/// allocator: the next write can hand out the same record handle again
/// with another expression. The panel is not refreshed while it is hidden,
/// so the cached key must follow the records' content, not their handles.
#[test]
fn a_filter_rewritten_after_undo_reloads_the_filters() {
    let mut app = app_with_layer_panel();
    let i = app.active_tab;
    let set = |app: &mut OpenCADStudio, colour: u16, label: &str| {
        app.layer_filters = vec![crate::app::layer_filters::LayerFilter {
            name: "REDS".into(),
            predicates: vec![crate::app::layer_filters::LayerPredicate::Color(colour)],
            ..Default::default()
        }];
        app.persist_layer_filters(i, label);
    };
    {
        let mut red = document::tables::Layer::new("L7");
        red.handle = app.tabs[i].scene.document.allocate_handle();
        red.color = document::types::Color::Index(7);
        let _ = app.tabs[i].scene.document.layers.add(red);
    }
    set(&mut app, 5, "TEST FILTERS 5");
    app.refresh_layer_counts();
    set(&mut app, 7, "TEST FILTERS 7");
    app.refresh_layer_counts();
    let in_reds = |app: &OpenCADStudio, name: &str| {
        app.tabs[i]
            .layers
            .filters
            .iter()
            .find(|(filter, _)| filter == "REDS")
            .map(|(_, members)| members.iter().any(|member| member == name))
            .unwrap_or(false)
    };
    assert!(in_reds(&app, "L7"), "the row follows the colour-7 filter");
    let rewritten = filter_record_handles(&app, i);
    app.show_layers = false;
    let _ = app.update(crate::app::Message::Undo);
    set(&mut app, 8, "TEST FILTERS 8");
    assert_eq!(
        filter_record_handles(&app, i),
        rewritten,
        "the scenario needs undo to rewind the allocator into the same handle",
    );
    app.show_layers = true;
    app.refresh_layer_counts();
    assert!(
        !in_reds(&app, "L7"),
        "a filter rewritten onto a reused record handle must reload",
    );
}
