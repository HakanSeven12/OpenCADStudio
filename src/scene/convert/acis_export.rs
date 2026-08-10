//! Export a B-rep [`Body`] to an exact ACIS `SatDocument`.
//!
//! A solid built by the Model tab, by EXTRUDE / REVOLVE, or by a boolean
//! carries no ACIS geometry of its own, so other CAD applications drop it on
//! open. Writing the body out as real modeller geometry is what stops that.
//!
//! # Exact, not facetted
//!
//! The kernel keeps analytic surfaces — a cylinder is a `cylinder` record and
//! a sphere a `sphere` — so what lands in the document is the shape itself
//! rather than a triangulation of it. That is the whole reason the Model tab
//! builds bodies at all: a facetted export is a one-way door, and the next
//! application to open the file would have no way back to the circle.
//!
//! The writing is [`acis::append`], which lives with the codec because it has
//! to know both sides. What is left here is the small matter of putting one
//! body into a fresh document.

use acadrust::acis::append;
use acadrust::entities::acis::SatDocument;
use acadrust::kernel::brep::Body;

/// An exact ACIS document holding `body`, or `None` when the kernel has no
/// record form for something in it — a spline curve, at present.
///
/// Refusing is the point: a document missing a face is one that parses into a
/// different solid, and nothing downstream could tell.
pub fn planar_solid_to_sat(body: &Body) -> Option<SatDocument> {
    let mut document = SatDocument::new();
    append(body, &mut document).ok()?;
    Some(document)
}
