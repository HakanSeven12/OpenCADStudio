// Saves each fixture entity of the transform test to DXF and to DWG, reads
// the file back, and compares the geometry that `describe` prints.
//
// `KNOWN_DIFFERENCES` lists what a save and reload changes now: the entity,
// the format, and the first field that differs. The test fails when the list
// and the result are not the same, in both directions: a new difference is a
// field that a save loses, and a difference that is gone must leave the list.
// The list records what happens; it does not say that each entry is correct.

use document::{Document, EntityType};
use OpenCADStudio::scene::Scene;

use super::entity_transform_regression::{describe, entities, first_difference};

/// (format, fixture, the first field that differs after a reload).
const KNOWN_DIFFERENCES: &[(&str, &str, &str)] = &[
    // The reader fills the direction that the fixture leaves empty.
    ("dxf", "mtext", "dwg_x_direction"),
    ("dwg", "mtext", "dwg_x_direction"),
    // A 2D polyline is read back from DXF as a different entity type.
    ("dxf", "polyline2d", "(type)"),
    ("dxf", "dim_radius", "base.definition_point"),
    ("dwg", "dim_radius", "base.definition_point"),
    ("dxf", "dim_diameter", "leader_length"),
    ("dwg", "spline", "control_points"),
    ("dwg", "leader", "text_height"),
    ("dxf", "tolerance", "text_height"),
    ("dwg", "tolerance", "text_height"),
    ("dwg", "attribute_definition", "alignment_point"),
];

fn roundtrip(doc: &Document, ext: &str) -> Result<Document, String> {
    let bytes = OpenCADStudio::io::save_to_bytes(doc, ext, doc.version)
        .map_err(|error| format!("save: {error}"))?;
    OpenCADStudio::io::load_bytes(&format!("sweep.{ext}"), bytes)
        .map_err(|error| format!("reload: {error}"))
}

/// The field named on the `expected:` line of a `first_difference` report.
fn differing_field(report: &str) -> String {
    report
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("expected:"))
        .and_then(|rest| rest.trim_start().split(':').next())
        .unwrap_or("(unknown)")
        .to_string()
}

/// The first field that a save and reload in `ext` changes, if one does.
fn difference(entity: &EntityType, ext: &str) -> Option<String> {
    let mut scene = Scene::new();
    let handle = scene.add_entity(entity.clone());
    let stored = scene.document.get_entity(handle).expect("the entity");
    let before = describe(stored);
    let reloaded = match roundtrip(&scene.document, ext) {
        Ok(document) => document,
        Err(error) => return Some(format!("({error})")),
    };
    let Some(after) = reloaded.get_entity(handle) else {
        return Some("(missing)".to_string());
    };
    if std::mem::discriminant(after) != std::mem::discriminant(stored) {
        return Some("(type)".to_string());
    }
    first_difference(&before, &describe(after)).map(|report| differing_field(&report))
}

fn differences() -> Vec<(&'static str, &'static str, String)> {
    let mut found = Vec::new();
    for ext in ["dxf", "dwg"] {
        for (name, entity) in entities() {
            if let Some(field) = difference(&entity, ext) {
                found.push((ext, name, field));
            }
        }
    }
    found
}

#[test]
fn a_save_and_reload_changes_only_the_known_fields() {
    let found = differences();
    let known: Vec<(&str, &str, String)> = KNOWN_DIFFERENCES
        .iter()
        .map(|&(ext, name, field)| (ext, name, field.to_string()))
        .collect();
    let new: Vec<_> = found
        .iter()
        .filter(|entry| !known.contains(entry))
        .collect();
    let gone: Vec<_> = known
        .iter()
        .filter(|entry| !found.contains(entry))
        .collect();
    assert!(
        new.is_empty() && gone.is_empty(),
        "a save and reload now changes: {new:?}\nand no longer changes: {gone:?}\n\
         (update KNOWN_DIFFERENCES when the change is intended)"
    );
}

/// The sweep covers each fixture in both formats, and most of them come
/// back the same: an empty or a broken sweep would pass the test above.
#[test]
fn the_sweep_reads_most_fixtures_back_unchanged() {
    let fixtures = entities().len();
    assert!(fixtures >= 35, "{fixtures} fixtures");
    let changed = differences().len();
    assert!(
        changed * 4 < fixtures * 2,
        "{changed} of {} changed",
        fixtures * 2
    );
}

#[test]
fn the_field_of_a_difference_report_is_found() {
    let report = first_difference("  radius: 1.000000000\n", "  radius: 2.000000000\n")
        .expect("a difference");
    assert_eq!(differing_field(&report), "radius");
}
