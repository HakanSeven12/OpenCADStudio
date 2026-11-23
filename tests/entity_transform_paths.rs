// A typed transform (MOVE, ROTATE, SCALE, MIRROR) and its matrix
// (`codec_transform`, as ALIGN, WBLOCK and the block base point give it) must
// move each fixture entity of the golden test to the same place. The fields
// that differ today are in `KNOWN_DIFFERENCES`; a new difference fails, and so
// does a listed one that no longer differs. Add a line there only with
// evidence of which value is correct, written in its comment.

use std::collections::{BTreeMap, BTreeSet};

use document::EntityType;
use OpenCADStudio::entities::transform::EntityTransform;
use OpenCADStudio::scene::view::dispatch::apply_transform;
use OpenCADStudio::scene::view::transform::codec_transform;

use super::entity_transform_regression::{describe, entities, first_difference, transforms};

/// (entity, transform, field): the typed transform and its matrix give a
/// different value. Each group has its class: "By design", "Two forms of one
/// value", or "Not examined".
const KNOWN_DIFFERENCES: &[(&str, &str, &str)] = &[
    // Not examined: no reason is known for this difference, and one of the
    // two paths can be wrong. A dimension's insertion point is (0, 0, 0) in
    // the fixtures. The typed translate keeps it there; the matrix moves it
    // by the offset.
    ("dim_aligned", "translate", "base.insertion_point"),
    ("dim_angular2ln", "translate", "base.insertion_point"),
    ("dim_angular3pt", "translate", "base.insertion_point"),
    ("dim_arc_length", "translate", "base.insertion_point"),
    ("dim_diameter", "translate", "base.insertion_point"),
    ("dim_jogged_radius", "translate", "base.insertion_point"),
    ("dim_linear", "translate", "base.insertion_point"),
    ("dim_ordinate", "translate", "base.insertion_point"),
    ("dim_radius", "translate", "base.insertion_point"),
    // Two forms of one value. The fixture has no stored direction, which
    // means the direction of the rotation. The typed translate and the typed
    // mirror keep it empty; the matrix writes that direction.
    ("mtext", "translate", "dwg_x_direction"),
    ("mtext", "mirror", "dwg_x_direction"),
    ("mtext", "mirror_offset", "dwg_x_direction"),
    // By design. The typed mirror in the world XY plane has its own rule for
    // text and for a shape (#3054): the oblique angle changes its sign, and a shape
    // also changes its size, rotation and X scale. The matrix changes only
    // the rotation.
    ("text", "mirror", "oblique_angle"),
    ("attribute_definition", "mirror", "oblique_angle"),
    ("shape", "mirror", "size"),
    ("shape", "mirror", "rotation"),
    ("shape", "mirror", "relative_x_scale"),
    ("shape", "mirror", "oblique_angle"),
    ("text", "mirror_offset", "oblique_angle"),
    ("attribute_definition", "mirror_offset", "oblique_angle"),
    ("shape", "mirror_offset", "size"),
    ("shape", "mirror_offset", "rotation"),
    ("shape", "mirror_offset", "relative_x_scale"),
    ("shape", "mirror_offset", "oblique_angle"),
    // Not examined: no reason is known for this difference, and one of the
    // two paths can be wrong. The typed mirror keeps the angle of a point's
    // X axis; the matrix reflects it.
    ("point", "mirror", "x_axis_angle"),
    ("point", "mirror_offset", "x_axis_angle"),
    // By design, two forms of one curve. The typed mirror keeps the +Z normal
    // and changes the parameters (the comment in `src/entities/ellipse.rs`
    // gives the reason); the matrix turns the normal to -Z.
    ("ellipse", "mirror", "start_parameter"),
    ("ellipse", "mirror", "end_parameter"),
    ("ellipse", "mirror", "normal"),
    ("ellipse", "mirror_offset", "start_parameter"),
    ("ellipse", "mirror_offset", "end_parameter"),
    ("ellipse", "mirror_offset", "normal"),
];

fn transformed(entity: &EntityType, transform: &EntityTransform) -> EntityType {
    let mut copy = entity.clone();
    apply_transform(&mut copy, transform);
    copy
}

/// The name of the field on a line of `describe`.
fn field_name(line: &str) -> &str {
    line.trim_start().split(':').next().unwrap_or(line)
}

type Key = (String, String, String);

/// Each (entity, transform, field) where the two paths differ, with the
/// described line of the typed path and of the matrix path.
fn differences() -> BTreeMap<Key, (String, String)> {
    let mut found = BTreeMap::new();
    for (entity_name, entity) in entities() {
        for (transform_name, transform) in transforms() {
            if matches!(transform, EntityTransform::Affine(_)) {
                continue;
            }
            let matrix = EntityTransform::Affine(codec_transform(&transform));
            let typed = describe(&transformed(&entity, &transform));
            let affine = describe(&transformed(&entity, &matrix));
            let (typed_lines, affine_lines): (Vec<_>, Vec<_>) =
                (typed.lines().collect(), affine.lines().collect());
            assert_eq!(
                typed_lines.len(),
                affine_lines.len(),
                "{entity_name} / {transform_name}: the two paths describe a different set of fields"
            );
            for (typed_line, affine_line) in typed_lines.iter().zip(&affine_lines) {
                if first_difference(typed_line, affine_line).is_some() {
                    found.insert(
                        (
                            entity_name.to_string(),
                            transform_name.to_string(),
                            field_name(typed_line).to_string(),
                        ),
                        (
                            typed_line.trim().to_string(),
                            affine_line.trim().to_string(),
                        ),
                    );
                }
            }
        }
    }
    found
}

#[test]
fn typed_and_matrix_transforms_agree_except_for_the_known_differences() {
    let found = differences();
    let known: BTreeSet<Key> = KNOWN_DIFFERENCES
        .iter()
        .map(|&(entity, transform, field)| {
            (entity.to_string(), transform.to_string(), field.to_string())
        })
        .collect();
    assert_eq!(
        known.len(),
        KNOWN_DIFFERENCES.len(),
        "KNOWN_DIFFERENCES has a line two times"
    );

    let new: String = found
        .iter()
        .filter(|(key, _)| !known.contains(*key))
        .map(|((entity, transform, field), (typed, matrix))| {
            format!(
                "  {entity} / {transform} / {field}\n    typed:  {typed}\n    matrix: {matrix}\n"
            )
        })
        .collect();
    assert!(
        new.is_empty(),
        "A typed transform and its matrix move an entity differently, and KNOWN_DIFFERENCES \
         does not have the difference:\n{new}\
         If your change made this difference, it is a fault in the path that you changed: \
         repair it. Add a line to KNOWN_DIFFERENCES only with evidence of which value is correct \
         (the reference application, or the file format), and write the evidence in the comment \
         of the line. Do not add a line only to make this test pass."
    );

    let gone: String = known
        .iter()
        .filter(|key| !found.contains_key(*key))
        .map(|(entity, transform, field)| format!("  {entity} / {transform} / {field}\n"))
        .collect();
    assert!(
        gone.is_empty(),
        "These lines of KNOWN_DIFFERENCES no longer differ: the two paths agree there now.\n{gone}\
         If your change is meant to do that, remove the lines. If you did not mean to change a \
         transform, find which path changed before you remove a line."
    );
}

/// Rotate, scale and the mirror in a tilted UCS have no line in the list: the
/// two paths agree there for each fixture entity. A line for one of them is
/// refused, so such a difference cannot become a known one. The list is not
/// empty, so the comparison is real.
#[test]
fn the_known_differences_are_only_for_translate_and_mirror() {
    for &(entity, transform, field) in KNOWN_DIFFERENCES {
        assert!(
            matches!(transform, "translate" | "mirror" | "mirror_offset"),
            "KNOWN_DIFFERENCES has a line for `{transform}` ({entity} / {transform} / {field}). \
             The typed transform and its matrix agree for `{transform}` on each fixture entity, \
             so a difference there is a fault in a transform, not a known difference. Repair \
             the transform and remove the line."
        );
    }
    assert!(!KNOWN_DIFFERENCES.is_empty());
}
