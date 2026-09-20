// The editable "Dim line color" property writes an ACAD_DSTYLE dimension-style
// override (code 176, an ACI index) on the leader / dimension. That override
// must survive a full DWG *and* DXF save/reload — otherwise the picked colour
// is silently lost, which is exactly why the row was kept read-only before.
// This is the regression guard for that persistence.

use acadrust::entities::{Dimension, DimensionLinear, Leader, Line};
use acadrust::tables::BlockRecord;
use acadrust::types::{Color, Vector3};
use acadrust::xdata::XDataValue;
use acadrust::{CadDocument, EntityType, Handle};
use OpenCADStudio::entities::dim_override as dov;
use OpenCADStudio::scene::Scene;

fn roundtrip(doc: &CadDocument, ext: &str) -> CadDocument {
    let bytes = OpenCADStudio::io::save_to_bytes(doc, ext, doc.version)
        .unwrap_or_else(|e| panic!("save to {ext}: {e}"));
    OpenCADStudio::io::load_bytes(&format!("rt.{ext}"), bytes)
        .unwrap_or_else(|e| panic!("reload {ext}: {e}"))
}

fn leader_color(doc: &CadDocument) -> Option<Color> {
    doc.entities()
        .find_map(|e| match e {
            EntityType::Leader(l) => Some(dov::color(&l.common.extended_data, dov::DIMCLRD)),
            _ => None,
        })
        .flatten()
}

fn dim_color(doc: &CadDocument) -> Option<Color> {
    doc.entities()
        .find_map(|e| match e {
            EntityType::Dimension(d) => {
                Some(dov::color(&d.base().common.extended_data, dov::DIMCLRD))
            }
            _ => None,
        })
        .flatten()
}

fn leader_scene(aci: i16) -> Scene {
    let mut scene = Scene::new();
    let mut ld = Leader::new();
    ld.vertices = vec![Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 5.0, 0.0)];
    let h = scene.add_entity(EntityType::Leader(ld));
    dov::set(
        &mut scene.document,
        h,
        dov::DIMCLRD,
        Some(XDataValue::Integer16(aci)),
    );
    scene
}

fn dimension_scene(aci: i16) -> Scene {
    let mut scene = Scene::new();

    // A baked *D0 block holding one line, so the dimension writes cleanly.
    let br_h = Handle::new(scene.document.next_handle());
    let mut br = BlockRecord::new("*D0");
    br.handle = br_h;
    scene.document.block_records.add(br).unwrap();
    let mut sub = Line::new();
    sub.start = Vector3::new(0.0, 0.0, 0.0);
    sub.end = Vector3::new(10.0, 0.0, 0.0);
    let mut sub_e = EntityType::Line(sub);
    sub_e.common_mut().owner_handle = br_h;
    scene.document.add_entity(sub_e).unwrap();

    let mut dim = DimensionLinear::new(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
    dim.base.block_name = "*D0".to_string();
    let h = scene.add_entity(EntityType::Dimension(Dimension::Linear(dim)));
    dov::set(
        &mut scene.document,
        h,
        dov::DIMCLRD,
        Some(XDataValue::Integer16(aci)),
    );
    scene
}

#[test]
fn leader_dim_line_color_survives_dwg_and_dxf() {
    for ext in ["dwg", "dxf"] {
        // ACI 1 (red): an indexed override.
        let scene = leader_scene(1);
        assert_eq!(
            leader_color(&scene.document),
            Some(Color::Index(1)),
            "override not applied pre-save ({ext})"
        );
        let re = roundtrip(&scene.document, ext);
        assert_eq!(
            leader_color(&re),
            Some(Color::Index(1)),
            "leader dim-line colour (ACI 1) lost across {ext} round-trip"
        );

        // ByLayer (256): an explicit override that must persist as ByLayer, not
        // collapse to "no override".
        let scene = leader_scene(256);
        let re = roundtrip(&scene.document, ext);
        assert_eq!(
            leader_color(&re),
            Some(Color::ByLayer),
            "leader dim-line colour (ByLayer) lost across {ext} round-trip"
        );
    }
}

#[test]
fn dimension_dim_line_color_survives_dwg_and_dxf() {
    for ext in ["dwg", "dxf"] {
        let scene = dimension_scene(3);
        assert_eq!(
            dim_color(&scene.document),
            Some(Color::Index(3)),
            "override not applied pre-save ({ext})"
        );
        let re = roundtrip(&scene.document, ext);
        assert_eq!(
            dim_color(&re),
            Some(Color::Index(3)),
            "dimension dim-line colour (ACI 3) lost across {ext} round-trip"
        );
    }
}
