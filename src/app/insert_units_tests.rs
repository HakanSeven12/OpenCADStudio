//! Placing a block whose units differ from the drawing's INSUNITS: the
//! reference is converted when it is added to the drawing, so what the
//! placement shows and where the picked base point lands have to use the
//! converted size too.
use super::OpenCADStudio;
use document::types::Vector3;
use document::EntityType;
use glam::DVec3;

const MILLIMETERS: i16 = 4;
const METERS: i16 = 6;

/// A millimetre drawing holding `METRE_LINE`: a line from 0,0 to 10,0 in a
/// block whose units are metres.
fn drawing() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.document.header.insertion_units = MILLIMETERS;
    app.tabs[i].scene.define_block_raw(
        "METRE_LINE",
        Vector3::ZERO,
        vec![EntityType::Line(document::entities::Line::from_points(
            Vector3::ZERO,
            Vector3::new(10.0, 0.0, 0.0),
        ))],
    );
    app.tabs[i]
        .scene
        .document
        .block_records
        .get_mut("METRE_LINE")
        .expect("the block is defined")
        .units = METERS;
    app
}

fn placed(app: &OpenCADStudio) -> document::entities::Insert {
    let i = app.active_tab;
    app.tabs[i]
        .scene
        .document
        .entities()
        .find_map(|entity| match entity {
            EntityType::Insert(insert) if insert.block_name == "METRE_LINE" => {
                Some(insert.clone())
            }
            _ => None,
        })
        .expect("a reference of METRE_LINE was placed")
}

#[test]
fn placing_a_block_with_units_previews_it_at_its_converted_size() {
    let mut app = drawing();
    let i = app.active_tab;
    let _ = app.run_command_line("-INSERT METRE_LINE");
    app.refresh_command_point_pick_context(i);
    let preview = app.tabs[i]
        .active_cmd
        .as_mut()
        .expect("-INSERT asks for the insertion point")
        .on_preview_wires(DVec3::ZERO);
    let xs: Vec<f64> = preview
        .iter()
        .flat_map(|wire| {
            wire.points.iter().enumerate().map(|(n, p)| {
                p[0] as f64 + wire.points_low.get(n).unwrap_or([0.0; 3])[0] as f64
            })
        })
        .collect();
    let length = xs.iter().cloned().fold(f64::MIN, f64::max)
        - xs.iter().cloned().fold(f64::MAX, f64::min);

    let _ = app.feed_command(crate::command::StepInput::Point(DVec3::ZERO));
    for _ in 0..3 {
        let _ = app.feed_command(crate::command::StepInput::Enter);
    }
    let insert = placed(&app);
    assert!(
        (insert.x_scale() - 1000.0).abs() < 1e-9,
        "the reference is converted: scale {}",
        insert.x_scale()
    );
    assert!(
        (length - 10_000.0).abs() < 1e-3,
        "the 10 m line is previewed {length} long; it lands 10,000 long"
    );
}

#[test]
fn a_block_with_units_lands_its_picked_base_point_on_the_insertion_point() {
    let mut app = drawing();
    // Basepoint: the block's 10,0 (the line's end) is the point that is
    // placed, at 0,0.
    let _ = app.run_command_line("-INSERT METRE_LINE B 10,0 0,0 1 1 0");
    let insert = placed(&app);
    let landed = insert.insert_point.x + insert.x_scale() * 10.0;
    assert!(
        (insert.x_scale() - 1000.0).abs() < 1e-9,
        "the reference is converted: scale {}",
        insert.x_scale()
    );
    assert!(
        landed.abs() < 1e-6,
        "the picked base point lands at x = {landed}, not on the insertion point 0 \
         (reference at {:?}, scale {})",
        insert.insert_point,
        insert.x_scale()
    );
}
