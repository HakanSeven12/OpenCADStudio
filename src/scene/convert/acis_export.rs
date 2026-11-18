//! Writing a Solidcore record as an exact ACIS `SatDocument`: analytic surfaces stay analytic
//! instead of becoming facets.

use document::entities::acis::{SabReader, SabWriter, SatDocument};
use ocs_solid::model::Brep;

/// The record as ACIS; `None` when it holds a form ACIS cannot carry, or what is written does
/// not read back as one body, in text and in binary.
pub fn solid_to_sat(body: &Brep) -> Option<SatDocument> {
    let mut document = SatDocument::new();
    ocs_solid::append(body, &mut document).ok()?;
    let document = SatDocument::parse(&document.to_sat_string()).ok()?;
    let valid = |candidate: &SatDocument| {
        let (restored, loss) = ocs_solid::lift(candidate);
        loss.is_empty() && restored.len() == 1
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

    #[test]
    fn a_curved_loft_round_trips_through_text_and_binary_acis() {
        let body = crate::scene::model::solid_model::cone_frustum_solid([0.0; 3], 5.0, 3.0, 2.0, 10.0).unwrap();
        let document = solid_to_sat(&body).expect("written");
        let (restored, loss) = ocs_solid::lift(&document);
        assert!(loss.is_empty(), "{loss:?}");
        let before = ocs_solid::model::volume(&body).unwrap();
        let after = ocs_solid::model::volume(&restored[0].brep).unwrap();
        assert!((before - after).abs() < 1e-6 * before, "{before} {after}");
    }
}
