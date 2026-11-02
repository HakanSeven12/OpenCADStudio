use super::*;
use crate::entities::traits::Grippable;
use codec::{entities::Table, types::Vector3, Document, EntityType};

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
    let origin = table(&app.tabs[i].scene.document, handle)
        .grips()
        .into_iter()
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
