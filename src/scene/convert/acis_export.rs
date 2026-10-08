//! Export a kernel [`Body`] to an exact ACIS `SatDocument`.
//!
//! Analytic surfaces remain analytic instead of becoming facets.

use kernel::acis::append;
use codec::entities::acis::{SabReader, SabWriter, SatDocument, SatToken};
use kernel::brep::Body;

/// Repair kernel's vertex records to the authored ASM genus.
///
/// kernel appends the classic three-token vertex (`$attr $edge $point`).
/// Every authored vertex â€” the Â§20 G-A census across the measured corpus â€”
/// carries the four-token form `vertex $attr $-1 $edge <role> $point`, where
/// the role token marks the vertex's stance in its own edge (0 start, 1 end,
/// 2 both endpoints of a closed edge; codec's primitive builders ship the
/// 0 placeholder). The ASM modeler reads the record positionally: without
/// the role token it consumes the point pointer as the role, the record
/// parse desyncs, and the solid arrives as "Modeling operation error:
/// Data stream is empty" (BricsCAD AUDIT on a constructed cylinder).
///
/// `kernel::acis::lift` reads the point by pointer ordinal, so the
/// repaired document still round-trips through kernel unchanged.
fn repair_vertex_roles(document: &mut SatDocument) {
    for record in &mut document.records {
        if record.entity_type == "vertex"
            && record.tokens.len() == 3
            && matches!(record.tokens[2], SatToken::Pointer(_))
        {
            record.tokens.insert(2, SatToken::Integer(0));
        }
    }
}

/// Returns `None` when the body contains an unsupported record form.
pub fn solid_to_sat(body: &Body) -> Option<SatDocument> {
    let mut document = SatDocument::new();
    append(body, &mut document).ok()?;
    let mut document = SatDocument::parse(&document.to_sat_string()).ok()?;
    repair_vertex_roles(&mut document);
    let valid = |candidate: &SatDocument| {
        let (restored, loss) = kernel::acis::lift(candidate);
        loss.is_empty() && restored.len() == 1 && restored[0].validate().is_empty()
    };
    if !valid(&document) {
        return None;
    }
    let binary = SabWriter::write(&document);
    valid(&SabReader::read(&binary).ok()?).then_some(document)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::geom2d::{Arc, Curve};
    use kernel::space::Plane;

    fn circle_section(z: f64, radius: f64) -> (Plane, Vec<Curve>) {
        let curves = (0..4)
            .map(|part| {
                let start = std::f64::consts::FRAC_PI_2 * part as f64;
                Curve::Arc(Arc {
                    centre: [0.0, 0.0],
                    radius,
                    start_angle: start,
                    end_angle: start + std::f64::consts::FRAC_PI_2,
                })
            })
            .collect();
        (
            Plane::from_axes([0.0, 0.0, z], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            curves,
        )
    }

    #[test]
    fn a_curved_loft_round_trips_through_text_and_binary_acis() {
        let body = kernel::brep::loft(&[
            circle_section(0.0, 5.0),
            circle_section(10.0, 2.0),
        ])
        .unwrap();
        assert!(body.validate().is_empty());

        let mut document = SatDocument::new();
        kernel::acis::append(&body, &mut document).unwrap();
        let text = document.to_sat_string();
        let parsed = SatDocument::parse(&text).unwrap();
        let (restored, text_loss) = kernel::acis::lift(&parsed);
        assert!(text_loss.is_empty(), "{text_loss:?}");
        assert_eq!(restored.len(), 1);
        assert!(restored[0].validate().is_empty());

        let binary = SabWriter::write(&parsed);
        let parsed_binary = SabReader::read(&binary).unwrap();
        let (restored, binary_loss) = kernel::acis::lift(&parsed_binary);
        assert!(binary_loss.is_empty(), "{binary_loss:?}");
        assert_eq!(restored.len(), 1);
        assert!(restored[0].validate().is_empty());
    }

    #[test]
    fn exported_vertices_carry_the_authored_role_token() {
        // The cylinder the CYLINDER command commits (equal radii): the
        // seamed brep whose SAT previously reached the DWG AcDs stream
        // without the vertex role token and failed BricsCAD's AUDIT
        // with "Data stream is empty".
        let body = kernel::brep::make::elliptical_cylinder(
            [0.0, 0.0, 0.0],
            1.0,
            1.0,
            1.0,
        )
        .unwrap();
        assert!(body.validate().is_empty());

        let sat = solid_to_sat(&body).unwrap();
        let vertices: Vec<_> = sat
            .records
            .iter()
            .filter(|record| record.entity_type == "vertex")
            .collect();
        assert!(!vertices.is_empty(), "the seamed cylinder has seam vertices");
        for vertex in &vertices {
            assert_eq!(
                vertex.tokens.len(),
                4,
                "vertex must carry the ASM role token: {:?}",
                vertex.tokens
            );
            assert!(matches!(vertex.tokens[2], SatToken::Integer(_)));
            assert!(matches!(vertex.tokens[3], SatToken::Pointer(_)));
        }

        // The SAB round trip â€” the exact conversion the DWG writer's
        // queue_sab_entry performs â€” must preserve the four-token form.
        let binary = SabWriter::write(&sat);
        let reread = SabReader::read(&binary).unwrap();
        for vertex in reread
            .records
            .iter()
            .filter(|record| record.entity_type == "vertex")
        {
            assert_eq!(
                vertex.tokens.len(),
                4,
                "SAB round trip lost the role token: {:?}",
                vertex.tokens
            );
        }

        // The repaired document still lifts losslessly through kernel.
        let (restored, loss) = kernel::acis::lift(&sat);
        assert!(loss.is_empty(), "{loss:?}");
        assert_eq!(restored.len(), 1);
        assert!(restored[0].validate().is_empty());
    }

    /// Every constructible primitive family, through the same
    /// `solid_model` makers the Model-tab commands commit, must export
    /// to the authored ASM genus: the vertex role token present, the
    /// family's analytic surface preserved, and the whole document
    /// SAB-round-trip valid (the `solid_to_sat` gate). The authored
    /// census behind these forms is the cadcodec gold-harness fixture
    /// corpus (`tests/gold_harness/fixtures/sh_history`).
    #[test]
    fn every_primitive_family_exports_the_authored_record_genus() {
        use crate::scene::model::solid_model as model;

        let families: &[(&str, Option<Body>, &str)] = &[
            ("box", model::box_solid([0.0, 0.0, 0.0], 10.0, 10.0, 10.0), "plane-surface"),
            ("wedge", model::wedge_solid([0.0, 0.0, 0.0], 10.0, 10.0, 10.0), "plane-surface"),
            (
                "cylinder",
                model::cylinder_solid([0.0, 0.0, 0.0], 5.0, 10.0),
                "cone-surface",
            ),
            (
                // A true ellipse is a cone-surface record with a section
                // ratio â€” the authored elliptical-cylinder wire form
                // (CylinderElliptical_2018 in the gold corpus: major
                // basis, ratio 0.6, sine 0 / cosine 1), not a spline.
                "elliptical-cylinder",
                model::elliptical_cylinder_solid([0.0, 0.0, 0.0], 5.0, 3.0, 10.0),
                "cone-surface",
            ),
            (
                "cone",
                model::cone_frustum_solid([0.0, 0.0, 0.0], 5.0, 5.0, 0.0, 10.0),
                "cone-surface",
            ),
            (
                "cone-frustum",
                model::cone_frustum_solid([0.0, 0.0, 0.0], 5.0, 5.0, 2.0, 10.0),
                "cone-surface",
            ),
            ("sphere", model::sphere_solid([0.0, 0.0, 0.0], 5.0), "sphere-surface"),
            ("torus", model::torus_solid([0.0, 0.0, 0.0], 5.0, 1.0), "torus-surface"),
            (
                "pyramid",
                model::pyramid_solid([0.0, 0.0, 0.0], 5.0, 10.0, 4),
                "plane-surface",
            ),
            (
                "pyramid-frustum",
                model::pyramid_frustum_solid([0.0, 0.0, 0.0], 5.0, 2.0, 10.0, 6),
                "plane-surface",
            ),
        ];

        for (family, body, surface_kind) in families {
            let body = body
                .as_ref()
                .unwrap_or_else(|| panic!("{family}: maker returned no body"));
            assert!(body.validate().is_empty(), "{family}: brep invalid");

            let sat = solid_to_sat(body)
                .unwrap_or_else(|| panic!("{family}: export rejected the body"));
            assert!(
                sat.records.iter().any(|r| r.entity_type == *surface_kind),
                "{family}: analytic surface {surface_kind} lost to export"
            );

            let mut vertices = 0;
            for record in &sat.records {
                if record.entity_type == "vertex" {
                    vertices += 1;
                    assert_eq!(
                        record.tokens.len(),
                        4,
                        "{family}: vertex missing the ASM role token: {:?}",
                        record.tokens
                    );
                    assert!(matches!(record.tokens[2], SatToken::Integer(_)));
                    assert!(matches!(record.tokens[3], SatToken::Pointer(_)));
                }
            }
            assert!(vertices > 0, "{family}: no vertices");
        }
    }
}