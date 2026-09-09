//! Export a kernel [`Body`] to an exact ACIS `SatDocument`.
//!
//! Analytic surfaces remain analytic instead of becoming facets.

use cadkernel::acis::append;
use acadrust::entities::acis::{SabReader, SabWriter, SatDocument};
use cadkernel::brep::Body;

/// Returns `None` when the body contains an unsupported record form.
pub fn solid_to_sat(body: &Body) -> Option<SatDocument> {
    let mut document = SatDocument::new();
    append(body, &mut document).ok()?;
    let document = SatDocument::parse(&document.to_sat_string()).ok()?;
    let valid = |candidate: &SatDocument| {
        let (restored, loss) = cadkernel::acis::lift(candidate);
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
    use cadkernel::geom2d::{Arc, Curve};
    use cadkernel::space::Plane;

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
        let body = cadkernel::brep::loft(&[
            circle_section(0.0, 5.0),
            circle_section(10.0, 2.0),
        ])
        .unwrap();
        assert!(body.validate().is_empty());

        let mut document = SatDocument::new();
        cadkernel::acis::append(&body, &mut document).unwrap();
        let text = document.to_sat_string();
        let parsed = SatDocument::parse(&text).unwrap();
        let (restored, text_loss) = cadkernel::acis::lift(&parsed);
        assert!(text_loss.is_empty(), "{text_loss:?}");
        assert_eq!(restored.len(), 1);
        assert!(restored[0].validate().is_empty());

        let binary = SabWriter::write(&parsed);
        let parsed_binary = SabReader::read(&binary).unwrap();
        let (restored, binary_loss) = cadkernel::acis::lift(&parsed_binary);
        assert!(binary_loss.is_empty(), "{binary_loss:?}");
        assert_eq!(restored.len(), 1);
        assert!(restored[0].validate().is_empty());
    }
}
