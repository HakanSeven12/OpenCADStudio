//! A layout can hold viewports onto the model and no sheet viewport yet: a
//! script, a plugin or an automation request added them before the layout
//! was first opened. Saved and opened again, they are still the viewports
//! that were drawn.
//!
//! A drawing file numbers a layout's viewports in their order, and the first
//! is the sheet's own. Saved without one, the first viewport onto the model
//! came back as the sheet and was sized to the page when the layout opened.

use crate::scene::Scene;
use document::entities::{Line, Viewport};
use document::objects::ObjectType;
use document::types::Vector3;
use document::{Document, EntityType};

/// Where a viewport sits on the sheet: its two corners.
type Corners = [f64; 4];

const FIRST: Corners = [20.0, 20.0, 100.0, 90.0];
const SECOND: Corners = [120.0, 20.0, 200.0, 90.0];

/// A viewport onto the model, numbered as MVIEW numbers one.
fn viewport_at(corners: Corners, id: i16) -> EntityType {
    let [x0, y0, x1, y1] = corners;
    let mut viewport = Viewport::new();
    viewport.center = Vector3::new((x0 + x1) / 2.0, (y0 + y1) / 2.0, 0.0);
    viewport.width = x1 - x0;
    viewport.height = y1 - y0;
    viewport.view_height = 50.0;
    viewport.id = id;
    EntityType::Viewport(Box::new(viewport))
}

/// The paper layout a new drawing starts with. It holds no viewport until it
/// is opened for the first time.
fn sheet_name(document: &Document) -> String {
    document
        .objects
        .values()
        .find_map(|object| match object {
            ObjectType::Layout(layout) if !layout.name.eq_ignore_ascii_case("Model") => {
                Some(layout.name.clone())
            }
            _ => None,
        })
        .expect("a paper layout")
}

/// A new drawing with a line in the model; `viewports` are added to its
/// paper layout, which was never opened, numbered from 2.
fn drawing_with(viewports: &[Corners]) -> Scene {
    let mut scene = Scene::new();
    scene
        .document
        .add_entity(EntityType::Line(Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(100.0, 50.0, 0.0),
        )))
        .unwrap();
    let sheet = sheet_name(&scene.document);
    for (index, corners) in viewports.iter().enumerate() {
        scene
            .document
            .add_entity_to_layout(viewport_at(*corners, index as i16 + 2), &sheet)
            .expect("the viewport is added");
    }
    scene
}

/// The scene's drawing saved in `format` and opened the way the application
/// opens a file.
fn reopened(scene: &Scene, format: &str) -> Document {
    let bytes = crate::io::save_to_bytes(&scene.document, format, scene.document.version)
        .expect("save the drawing");
    crate::io::load_bytes_finalized(std::path::Path::new(&format!("saved.{format}")), bytes)
        .expect("open the saved drawing")
}

/// The corners of the layout's viewports onto the model, in the layout's
/// order, and whether the layout has a sheet viewport.
fn layout_viewports(document: &Document) -> (Vec<Corners>, bool) {
    let name = sheet_name(document);
    let layout = document
        .objects
        .values()
        .find_map(|object| match object {
            ObjectType::Layout(layout) if layout.name == name => Some(layout.clone()),
            _ => None,
        })
        .expect("the layout");
    let sheet = Scene::layout_sheet_viewport_handle(document, &layout);
    let mut onto_the_model: Vec<Corners> = document
        .entities()
        .filter_map(|entity| match entity {
            EntityType::Viewport(viewport)
                if viewport.common.owner_handle == layout.block_record
                    && viewport.common.handle != sheet =>
            {
                Some([
                    viewport.center.x - viewport.width / 2.0,
                    viewport.center.y - viewport.height / 2.0,
                    viewport.center.x + viewport.width / 2.0,
                    viewport.center.y + viewport.height / 2.0,
                ])
            }
            _ => None,
        })
        .map(|corners| corners.map(|value| (value * 1.0e6).round() / 1.0e6))
        .collect();
    onto_the_model.sort_by(|a, b| a[0].total_cmp(&b[0]));
    (onto_the_model, sheet.is_valid())
}

#[test]
fn before_it_is_saved_the_layout_has_two_viewports_and_no_sheet() {
    let scene = drawing_with(&[FIRST, SECOND]);
    assert_eq!(
        layout_viewports(&scene.document),
        (vec![FIRST, SECOND], false)
    );
}

#[test]
fn viewports_of_a_layout_without_a_sheet_are_the_same_after_a_dwg_save() {
    let scene = drawing_with(&[FIRST, SECOND]);
    let (viewports, has_sheet) = layout_viewports(&reopened(&scene, "dwg"));
    assert_eq!(
        viewports,
        vec![FIRST, SECOND],
        "the viewports onto the model"
    );
    assert!(has_sheet, "the saved layout has its sheet viewport");
}

#[test]
fn viewports_of_a_layout_without_a_sheet_are_the_same_after_a_dxf_save() {
    let scene = drawing_with(&[FIRST, SECOND]);
    let (viewports, _) = layout_viewports(&reopened(&scene, "dxf"));
    assert_eq!(
        viewports,
        vec![FIRST, SECOND],
        "the viewports onto the model"
    );
}

#[test]
fn a_single_viewport_is_not_taken_for_the_sheet_after_a_dwg_save() {
    let scene = drawing_with(&[FIRST]);
    let (viewports, has_sheet) = layout_viewports(&reopened(&scene, "dwg"));
    assert_eq!(viewports, vec![FIRST]);
    assert!(has_sheet);
}

#[test]
fn saving_leaves_the_open_drawing_as_it_was() {
    let scene = drawing_with(&[FIRST, SECOND]);
    let _ = reopened(&scene, "dwg");
    assert_eq!(
        layout_viewports(&scene.document),
        (vec![FIRST, SECOND], false),
        "the sheet is given to the saved copy only"
    );
}

#[test]
fn a_layout_that_holds_no_viewport_is_saved_without_one() {
    let scene = drawing_with(&[]);
    assert_eq!(
        layout_viewports(&reopened(&scene, "dwg")),
        (Vec::new(), false)
    );
}

#[test]
fn a_layout_opened_after_its_viewports_were_added_is_saved_as_it_is() {
    // Opening the layout gives it its sheet, after the viewports it held.
    for format in ["dwg", "dxf"] {
        let mut scene = drawing_with(&[FIRST, SECOND]);
        let sheet = sheet_name(&scene.document);
        assert!(scene.ensure_sheet_viewport(&sheet));
        let count = |document: &Document| {
            document
                .entities()
                .filter(|entity| matches!(entity, EntityType::Viewport(_)))
                .count()
        };
        assert_eq!(count(&scene.document), 3);
        let opened = reopened(&scene, format);
        assert_eq!(count(&opened), 3, "{format}: no second sheet is added");
        assert_eq!(
            layout_viewports(&opened),
            (vec![FIRST, SECOND], true),
            "{format}"
        );
    }
}
