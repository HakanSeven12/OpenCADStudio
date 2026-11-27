//! A boundary representation tessellated for display at a chosen tolerance.

use ocs_solid::model::{self, Brep};

/// What a display mesh shows: the facet angle, the chordal deflection, the isoline counts, and
/// whether they are a surface's own U/V lines (`Some`, with whether planes get them too) or a
/// solid's (`None`), spaced as the drawing's isoline setting spaces them.
#[derive(Clone, Copy, Debug)]
pub struct DisplayTolerance {
    pub angle: f64,
    pub chord: Option<f64>,
    pub isolines: [usize; 2],
    pub planar_isolines: Option<bool>,
}

/// The display mesh of `brep`.
pub fn tessellate(brep: &Brep, tolerance: DisplayTolerance) -> Option<ocs_solid::display::RecordMesh> {
    let display = ocs_solid::display::Display {
        angle: tolerance.angle,
        chord: tolerance.chord,
        isolines: tolerance.isolines,
        planar_isolines: tolerance.planar_isolines.unwrap_or(false),
        solid_isolines: tolerance.planar_isolines.is_none(),
    };
    ocs_solid::display::mesh(brep, &display, model::size(brep)).ok()
}
