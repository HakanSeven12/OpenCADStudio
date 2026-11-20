use super::*;
use crate::entities::traits::Grippable;
use document::{entities::Table, types::Vector3, Document, EntityType};

fn table(doc: &Document, handle: Handle) -> &Table {
    let Some(EntityType::Table(table)) = doc.get_entity(handle) else {
        panic!("TABLE")
    };
    table
}

fn fixture() -> (OpenCADStudio, Handle) {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.snapper.snap_enabled = false;
    app.snapper.grid_snap_on = false;
    app.snapper.otrack_enabled = false;
    app.ortho_mode = false;
    app.polar_mode = false;
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    let mut source = Table::new(Vector3::ZERO, 3, 4);
    for column in &mut source.columns {
        column.width = 20.0;
    }
    for row in &mut source.rows {
        row.height = 10.0;
    }
    for column in 0..4 {
        source.set_cell_text(2, column, &format!("C{column}"));
    }
    let handle = app.tabs[i]
        .scene
        .add_entity(EntityType::Table(Box::new(source)));
    assert!(table(&app.tabs[i].scene.document, handle)
        .block_record_handle
        .is_some());
    (app, handle)
}

// Resolve by name as well: rebuilt tables can keep drawing directly from cells.
fn picture(doc: &Document, handle: Handle) -> (Handle, String, f64, Vec<f64>) {
    let t = table(doc, handle);
    let record = t
        .block_record_handle
        .and_then(|h| doc.block_records.iter().find(|r| r.handle == h))
        .or_else(|| doc.block_records.get(&t.block_name))
        .expect("picture block");
    let mut right: f64 = 0.0;
    let mut texts = vec![f64::NAN; 4];
    for h in &record.entity_handles {
        match doc.get_entity(*h) {
            Some(EntityType::Line(line)) => {
                right = right.max(line.start.x).max(line.end.x);
            }
            Some(EntityType::MText(text)) => {
                for c in 0..4 {
                    if text.value == format!("C{c}") {
                        texts[c] = text.insertion_point.x;
                    }
                }
            }
            _ => {}
        }
    }
    (record.handle, record.name.clone(), right, texts)
}

fn assert_picture(doc: &Document, handle: Handle, width: f64) {
    let p = picture(doc, handle);
    assert!(
        (p.2 - width).abs() < 1e-3,
        "stale picture extent: {} != {width}",
        p.2
    );
    for c in 0..4 {
        assert!(
            (p.3[c] - p.3[0] - c as f64 * width / 4.0).abs() < 1e-3,
            "stale picture text position: {:?}",
            p.3
        );
    }
}

fn drag(app: &mut OpenCADStudio, handle: Handle, id: usize, delta: glam::DVec3) {
    let i = app.active_tab;
    let t = table(&app.tabs[i].scene.document, handle);
    let range_grips = crate::entities::table::prop_cell_range()
        .map(|range| crate::entities::table::cell_range_grips(t, range))
        .unwrap_or_default();
    let origin = t
        .grips()
        .into_iter()
        .chain(range_grips)
        .find(|g| g.id == id)
        .unwrap()
        .world;
    app.tabs[i].active_grip = Some(GripEdit::single(handle, id, false, origin));
    let cursor = app.tabs[i]
        .scene
        .camera
        .borrow()
        .project(
            origin + delta,
            iced::Rectangle::with_size(iced::Size::new(800.0, 600.0)),
        )
        .unwrap();
    let _ = app.on_viewport_move(Point::new(cursor.x, cursor.y));
}

/// A formula cell's picture MTEXT must carry the computed value (what the
/// screen shows), not the raw "=A1+C1" text a program drawing the picture
/// would then display as-is.
#[test]
fn table_picture_formula_cell_shows_computed_value_not_raw_text() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    let mut source = Table::new(Vector3::ZERO, 1, 3);
    for column in &mut source.columns {
        column.width = 20.0;
    }
    for row in &mut source.rows {
        row.height = 10.0;
    }
    source.set_cell_text(0, 0, "2"); // A1
    source.set_cell_text(0, 2, "3"); // C1
    source.set_cell_text(0, 1, "=A1+C1"); // B1
    let handle = app.tabs[i]
        .scene
        .add_entity(EntityType::Table(Box::new(source)));
    let doc = &app.tabs[i].scene.document;
    let t = table(doc, handle);
    assert!(t.block_record_handle.is_some(), "no picture block");
    let record = t
        .block_record_handle
        .and_then(|h| doc.block_records.iter().find(|r| r.handle == h))
        .or_else(|| doc.block_records.get(&t.block_name))
        .expect("picture block");
    let values: Vec<String> = record
        .entity_handles
        .iter()
        .filter_map(|h| match doc.get_entity(*h) {
            Some(EntityType::MText(text)) => Some(text.value.clone()),
            _ => None,
        })
        .collect();
    assert!(
        !values.iter().any(|v| v == "=A1+C1"),
        "the picture stores the raw formula text: {values:?}"
    );
    assert!(
        values.iter().any(|v| v == "5"),
        "the picture should show B1's computed value 5: {values:?}"
    );
}

#[test]
fn table_grip_picture_width_preview_commit_and_cancel() {
    let (mut app, h) = fixture();
    let i = app.active_tab;
    let before = picture(&app.tabs[i].scene.document, h);
    let old_members = app.tabs[i]
        .scene
        .document
        .block_records
        .get(&before.1)
        .unwrap()
        .entity_handles
        .clone();
    // All layout grips use the same invalidation, and Esc never touches the block.
    for id in [1, 2, 3, 100, 102] {
        drag(&mut app, h, id, glam::DVec3::new(40.0, -10.0, 0.0));
        let wires = app.tabs[i].scene.grip_wire_models_for(&[h]);
        let xs: Vec<_> = wires
            .iter()
            .flat_map(|w| w.points.iter().map(|p| p[0] as f64))
            .collect();
        let width = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - xs.iter().copied().fold(f64::INFINITY, f64::min);
        assert!(
            (width - table(&app.tabs[i].scene.document, h).total_width()).abs() < 0.01,
            "preview has stale width: {width}"
        );
        assert!(app.cancel_active_grip_edit());
        assert_eq!(picture(&app.tabs[i].scene.document, h), before);
        assert_eq!(table(&app.tabs[i].scene.document, h).total_width(), 80.0);
    }
    drag(&mut app, h, 1, glam::DVec3::new(40.0, 0.0, 0.0));
    let _ = app.commit_active_grip_edit();
    let doc = &app.tabs[i].scene.document;
    assert_picture(doc, h, 120.0);
    assert_eq!(
        picture(doc, h).0,
        before.0,
        "reuse the unshared picture record"
    );
    assert!(
        old_members.iter().all(|h| doc.get_entity(*h).is_none()),
        "orphaned picture entities"
    );
    // A legacy second table using that name must retain its original picture.
    let (mut app, h) = fixture();
    let i = app.active_tab;
    let mut shared = app.tabs[i].scene.document.get_entity(h).unwrap().clone();
    shared.common_mut().handle = Handle::NULL;
    let other = app.tabs[i].scene.document.add_entity(shared).unwrap();
    let original = picture(&app.tabs[i].scene.document, other);
    drag(&mut app, h, 1, glam::DVec3::new(40.0, 0.0, 0.0));
    let _ = app.commit_active_grip_edit();
    assert_picture(&app.tabs[i].scene.document, h, 120.0);
    assert_eq!(picture(&app.tabs[i].scene.document, other), original);
    assert_ne!(picture(&app.tabs[i].scene.document, h).0, original.0);
    app.undo_steps(1);
    assert_eq!(picture(&app.tabs[i].scene.document, h), original);
    assert_eq!(picture(&app.tabs[i].scene.document, other), original);
}

#[test]
fn table_grip_picture_width_survives_dwg_and_dxf() {
    let (mut app, h) = fixture();
    let i = app.active_tab;
    drag(&mut app, h, 1, glam::DVec3::new(40.0, 0.0, 0.0));
    let _ = app.commit_active_grip_edit();
    for ext in ["dwg", "dxf"] {
        let doc = &app.tabs[i].scene.document;
        let bytes = crate::io::save_to_bytes(doc, ext, doc.version).unwrap();
        let saved = crate::io::load_bytes(&format!("table-grip.{ext}"), bytes).unwrap();
        let h = saved
            .entities()
            .find_map(|e| matches!(e, EntityType::Table(_)).then_some(e.common().handle))
            .unwrap();
        assert!((table(&saved, h).total_width() - 120.0).abs() < 1e-3);
        assert_picture(&saved, h, 120.0);
    }
}

#[test]
fn table_grip_picture_move_retains_handle_after_resize() {
    let (mut app, h) = fixture();
    let i = app.active_tab;
    let original = picture(&app.tabs[i].scene.document, h);
    drag(&mut app, h, 0, glam::DVec3::new(5.0, 7.0, 0.0));
    let _ = app.commit_active_grip_edit();
    assert_eq!(
        table(&app.tabs[i].scene.document, h).block_record_handle,
        Some(original.0)
    );
    assert_eq!(picture(&app.tabs[i].scene.document, h), original);
    drag(&mut app, h, 1, glam::DVec3::new(40.0, 0.0, 0.0));
    let _ = app.commit_active_grip_edit();
    assert_picture(&app.tabs[i].scene.document, h, 120.0);
    let before = picture(&app.tabs[i].scene.document, h);
    let handle_before = table(&app.tabs[i].scene.document, h).block_record_handle;
    drag(&mut app, h, 0, glam::DVec3::new(5.0, 7.0, 0.0));
    let _ = app.commit_active_grip_edit();
    assert_eq!(
        table(&app.tabs[i].scene.document, h).block_record_handle,
        handle_before
    );
    assert_eq!(picture(&app.tabs[i].scene.document, h), before);
}

#[test]
fn table_grip_picture_width_undo_restores_old_picture() {
    let (mut app, h) = fixture();
    let i = app.active_tab;
    let before = picture(&app.tabs[i].scene.document, h);
    let depth = app.tabs[i].history.undo_stack.len();
    drag(&mut app, h, 1, glam::DVec3::new(40.0, 0.0, 0.0));
    let _ = app.commit_active_grip_edit();
    assert_picture(&app.tabs[i].scene.document, h, 120.0);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    app.undo_steps(1);
    assert_eq!(table(&app.tabs[i].scene.document, h).total_width(), 80.0);
    assert_eq!(picture(&app.tabs[i].scene.document, h), before);
    app.redo_steps(1);
    assert_picture(&app.tabs[i].scene.document, h, 120.0);
}

/// Dragging a picked cell's fill diamond reaches the cell under the cursor
/// whatever drawing aids are on: snaps on the table's own lines, tracking or
/// polar must not pull it to the far edge of the table.
#[test]
fn table_fill_drag_reaches_the_cell_under_the_cursor_with_drawing_aids_on() {
    let fill = crate::entities::table::CELL_GRIP_FILL;
    for aid in ["osnap", "ortho", "polar", "otrack", "grid"] {
        let (mut app, h) = fixture();
        let i = app.active_tab;
        match aid {
            "osnap" => app.snapper.snap_enabled = true,
            "ortho" => app.ortho_mode = true,
            "polar" => app.polar_mode = true,
            "otrack" => {
                app.snapper.snap_enabled = true;
                app.snapper.otrack_enabled = true;
            }
            _ => app.snapper.grid_snap_on = true,
        }
        crate::entities::table::set_prop_cell_range(Some((0, 0, 0, 0)));
        // The diamond sits at the picked cell's lower right corner (20, -10);
        // the cursor goes inside cell (1, 1), next to the row line at y = -20
        // and the column line at x = 20, both of which run to the table edge.
        drag(&mut app, h, fill, glam::DVec3::new(3.0, -9.0, 0.0));
        let cells = app.tabs[i].scene.table_cells;
        crate::entities::table::set_prop_cell_range(None);
        let cells = cells.unwrap_or_else(|| panic!("{aid}: no fill span shown"));
        assert_eq!(
            (cells.anchor, cells.end),
            ((0, 0), (1, 0)),
            "{aid}: the fill must stop at the row under the cursor"
        );
    }
}

/// Dragging a formula cell's fill diamond copies the formula with its cell
/// references moved, as a spreadsheet does (D1 =SUM(A1:C1) filled down
/// gives D2 =SUM(A2:C2) and D3 =SUM(A3:C3)), each a formula field showing
/// its own result; any other field is copied as a field. One undo takes
/// the fill back.
#[test]
fn table_fill_copies_formulas_with_moved_references_and_fields() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.snapper.snap_enabled = false;
    app.snapper.grid_snap_on = false;
    app.snapper.otrack_enabled = false;
    app.ortho_mode = false;
    app.polar_mode = false;
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    let mut source = Table::new(Vector3::ZERO, 3, 5);
    for column in &mut source.columns {
        column.width = 20.0;
    }
    for row in &mut source.rows {
        row.height = 10.0;
    }
    for (row, values) in [["1", "2", "3"], ["10", "20", "30"], ["100", "200", "300"]]
        .iter()
        .enumerate()
    {
        for (column, value) in values.iter().enumerate() {
            source.set_cell_text(row, column, value);
        }
    }
    source.set_cell_text(0, 3, "=SUM(A1:C1)");
    let h = app.tabs[i]
        .scene
        .add_entity(EntityType::Table(Box::new(source)));
    let document = &mut app.tabs[i].scene.document;
    assert!(crate::entities::table::typed_formula_to_field(
        document, h, 0, 3
    ));
    document
        .set_table_cell_field(
            h,
            0,
            4,
            "%<\\_FldIdx 0>%",
            vec![document::fields::NewField::new("\\AcVar Author", "someone")],
        )
        .expect("field cell");
    let formula = |app: &OpenCADStudio, row: usize, column: usize| {
        let document = &app.tabs[app.active_tab].scene.document;
        crate::entities::table::cell_formula_text(document, table(document, h), row, column)
    };
    let field_code = |app: &OpenCADStudio, row: usize, column: usize| {
        use document::objects::ObjectType;
        let document = &app.tabs[app.active_tab].scene.document;
        let container = table(document, h)
            .cell(row, column)?
            .contents
            .first()?
            .field_handle?;
        let Some(ObjectType::Field(container)) = document.objects.get(&container) else {
            return None;
        };
        let [child] = container.child_fields.as_slice() else {
            return None;
        };
        match document.objects.get(child) {
            Some(ObjectType::Field(child)) => Some(child.code.clone()),
            _ => None,
        }
    };
    let shown = |app: &OpenCADStudio, row: usize, column: usize| {
        table(&app.tabs[app.active_tab].scene.document, h)
            .cell(row, column)
            .unwrap()
            .text_value()
            .to_string()
    };
    assert_eq!(formula(&app, 0, 3).as_deref(), Some("=SUM(A1:C1)"));
    let fill = crate::entities::table::CELL_GRIP_FILL;
    // D1:E1 picked; the diamond sits at E1's lower right corner (100, -10);
    // the cursor goes into E3.
    crate::entities::table::set_prop_cell_range(Some((0, 3, 0, 4)));
    drag(&mut app, h, fill, glam::DVec3::new(-3.0, -16.0, 0.0));
    let cells = app.tabs[i].scene.table_cells;
    let _ = app.commit_active_grip_edit();
    crate::entities::table::set_prop_cell_range(None);
    let cells = cells.expect("fill span shown");
    assert_eq!((cells.anchor, cells.end), ((0, 3), (2, 4)));
    assert_eq!(formula(&app, 1, 3).as_deref(), Some("=SUM(A2:C2)"));
    assert_eq!(formula(&app, 2, 3).as_deref(), Some("=SUM(A3:C3)"));
    assert_eq!(shown(&app, 1, 3), "60");
    assert_eq!(shown(&app, 2, 3), "600");
    assert_eq!(field_code(&app, 1, 4).as_deref(), Some("\\AcVar Author"));
    assert_eq!(field_code(&app, 2, 4).as_deref(), Some("\\AcVar Author"));
    assert_eq!(shown(&app, 2, 4), "someone");
    // The source cells keep their own formula and field.
    assert_eq!(formula(&app, 0, 3).as_deref(), Some("=SUM(A1:C1)"));
    assert_eq!(field_code(&app, 0, 4).as_deref(), Some("\\AcVar Author"));
    app.undo_steps(1);
    assert_eq!(formula(&app, 1, 3), None);
    assert_eq!(field_code(&app, 2, 4), None);
    assert_eq!(shown(&app, 1, 3), "");
    assert_eq!(formula(&app, 0, 3).as_deref(), Some("=SUM(A1:C1)"));
}
