// A transform has two paths through the code. MOVE, ROTATE, SCALE and MIRROR
// give a typed `EntityTransform`. ALIGN, WBLOCK, the block base point and
// QDIM give the motion as a matrix (`EntityTransform::Affine`). The two paths
// must move an entity to the same place.
//
// This test applies each typed transform of the golden test to each of its
// fixture entities, applies the matrix of that transform
// (`codec_transform`), and compares each described field. The fields that
// differ today are in `KNOWN_DIFFERENCES`. A new difference fails the test,
// and so does a line of the list that no longer differs.

use std::collections::BTreeSet;

use document::EntityType;
use OpenCADStudio::entities::transform::EntityTransform;
use OpenCADStudio::scene::view::dispatch::apply_transform;
use OpenCADStudio::scene::view::transform::codec_transform;

use super::entity_transform_regression::{describe, entities, first_difference, transforms};

/// (entity, transform, field): the typed transform and its matrix give a
/// different value.
const KNOWN_DIFFERENCES: &[(&str, &str, &str)] = &[
    // A dimension's insertion point is (0, 0, 0) in the fixtures. The typed
    // translate keeps it there; the matrix moves it by the offset.
    ("dim_aligned", "translate", "base.insertion_point"),
    ("dim_angular2ln", "translate", "base.insertion_point"),
    ("dim_angular3pt", "translate", "base.insertion_point"),
    ("dim_arc_length", "translate", "base.insertion_point"),
    ("dim_diameter", "translate", "base.insertion_point"),
    ("dim_jogged_radius", "translate", "base.insertion_point"),
    ("dim_linear", "translate", "base.insertion_point"),
    ("dim_ordinate", "translate", "base.insertion_point"),
    ("dim_radius", "translate", "base.insertion_point"),
    // The fixture has no stored direction. The typed translate and the typed
    // mirror keep it empty; the matrix writes the direction of the rotation.
    ("mtext", "translate", "dwg_x_direction"),
    ("mtext", "mirror", "dwg_x_direction"),
    ("mtext", "mirror_offset", "dwg_x_direction"),
    // The typed mirror in the world XY plane has its own rule for text and
    // for a shape (#3054): the oblique angle changes its sign, and a shape
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
    // The typed mirror keeps the angle of a point's X axis; the matrix
    // reflects it.
    ("point", "mirror", "x_axis_angle"),
    ("point", "mirror_offset", "x_axis_angle"),
    // Two forms of the mirrored ellipse: the typed mirror keeps the +Z normal
    // and changes the parameters; the matrix turns the normal to -Z.
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

/// Each (entity, transform, field) where the two paths differ.
fn differences() -> BTreeSet<(String, String, String)> {
    let mut found = BTreeSet::new();
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
                    found.insert((
                        entity_name.to_string(),
                        transform_name.to_string(),
                        field_name(typed_line).to_string(),
                    ));
                }
            }
        }
    }
    found
}

#[test]
fn typed_and_matrix_transforms_agree_except_for_the_known_differences() {
    let found = differences();
    let known: BTreeSet<(String, String, String)> = KNOWN_DIFFERENCES
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

    let new: Vec<_> = found.difference(&known).collect();
    let gone: Vec<_> = known.difference(&found).collect();
    assert!(
        new.is_empty(),
        "a typed transform and its matrix give a different value, and the difference is not in \
         KNOWN_DIFFERENCES. Repair the path that is wrong, or add the line with its reason:\n{new:#?}"
    );
    assert!(
        gone.is_empty(),
        "these lines of KNOWN_DIFFERENCES no longer differ. Remove them:\n{gone:#?}"
    );
}

/// The comparison is real: rotate and scale have no line in the list, so the
/// two paths agree there for each fixture entity, and the list is not empty.
#[test]
fn the_known_differences_are_only_for_translate_and_mirror() {
    for &(entity, transform, field) in KNOWN_DIFFERENCES {
        assert!(
            matches!(transform, "translate" | "mirror" | "mirror_offset"),
            "{entity} / {transform} / {field}"
        );
    }
    assert!(!KNOWN_DIFFERENCES.is_empty());
}
