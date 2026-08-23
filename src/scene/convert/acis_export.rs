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
