// How finely a curve is sampled, for the whole of one render frame.
//
// Every curve in a drawing is tessellated to a chord height — how far the
// polyline drawn for it may sit from the curve itself — and the right value
// depends on the zoom. At a metre per pixel a millimetre of error is
// invisible; zoomed to a millimetre per pixel it is the whole picture.
//
// The Scene sets this once per frame from `world_per_pixel`, targeting about
// half a pixel. Every tessellation inside that frame reads the same atomic,
// including the ones running on rayon workers, which is why it is a global
// rather than a parameter: threading a tolerance through every entity
// converter's signature would touch every one of them to say the same thing.
//
// Zero means "no frame is being drawn" — a load, a snap, a hit test — and the
// floor is used instead. That is what `BlockCache::build` expects.

use std::sync::atomic::{AtomicU64, Ordering};

/// The finest a curve is ever sampled, in world units, and the value used
/// when no frame has set one.
const CURVE_TOL: f64 = 0.005;

static CURVE_TOL_BITS: AtomicU64 = AtomicU64::new(0);

/// Sets the per-frame curve tolerance. `None` — or anything not finite and
/// positive — reverts to the floor.
pub fn set_curve_tol_override(tol: Option<f64>) {
    let bits = match tol {
        Some(value) if value > 0.0 && value.is_finite() => value.to_bits(),
        _ => 0,
    };
    CURVE_TOL_BITS.store(bits, Ordering::Relaxed);
}

/// The tolerance to sample a curve of a given size at.
///
/// A chord tolerance is an absolute length, so a fixed floor carries an
/// assumption about the drawing's units. Five thousandths is a fine sampling
/// for a part measured in millimetres and a coarse one for a building
/// measured in metres — it turns a ten-centimetre pipe into a decagon and a
/// five-centimetre bolt into a heptagon, and zooming in cannot recover them,
/// because the floor is what stops the frame asking for better.
///
/// Bounded against the curve's own size instead, which carries no unit. The
/// two ends are worth stating as segment counts, since that is what they
/// really are: a circle gets at least about fifty sides and never more than
/// about a hundred and sixty, wherever the camera is and whatever the drawing
/// is measured in.
pub(crate) fn curve_tol_for(size: f64) -> f64 {
    if !(size > 0.0) || !size.is_finite() {
        return current_curve_tol();
    }
    // The frame's own request, before the absolute floor — that floor is the
    // very thing being replaced here, and letting it through first would pin
    // every curve to it however close the camera came.
    let bits = CURVE_TOL_BITS.load(Ordering::Relaxed);
    let asked = if bits == 0 {
        // Nothing is being drawn — a load, a snap, a hit test. Ask for the
        // middle of the range, which is smooth without being extravagant.
        size / 1_500.0
    } else {
        f64::from_bits(bits)
    };
    asked.clamp(size / 5_000.0, size / 500.0)
}

/// The tolerance to sample at, never below the floor — so zooming a long way
/// in cannot ask for a sampling finer than the baseline quality.
pub(crate) fn current_curve_tol() -> f64 {
    let bits = CURVE_TOL_BITS.load(Ordering::Relaxed);
    if bits == 0 {
        CURVE_TOL
    } else {
        f64::from_bits(bits).max(CURVE_TOL)
    }
}

/// `Some(tol)` only while a frame's override is in force — that is, while
/// something is being drawn rather than loaded, snapped or hit-tested. Hatch
/// boundaries use it to decide whether zoom-adaptive sampling applies at all.
pub(crate) fn active_curve_tol() -> Option<f64> {
    let bits = CURVE_TOL_BITS.load(Ordering::Relaxed);
    (bits != 0).then(|| f64::from_bits(bits).max(CURVE_TOL))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How many chords a circle of `radius` needs at chord height `tol`.
    ///
    /// From the sagitta: a chord subtending `θ` departs from the arc by
    /// `r(1 − cos(θ/2))` in the middle.
    fn sides(radius: f64, tol: f64) -> f64 {
        let step = 2.0 * (1.0 - tol / radius).clamp(-1.0, 1.0).acos();
        std::f64::consts::TAU / step
    }

    #[test]
    fn a_circle_gets_the_same_number_of_sides_whatever_it_is_measured_in() {
        // The fault: the floor was an absolute length, so the same circle drawn
        // in millimetres and in metres came out smooth and faceted. Ten
        // centimetres of pipe had ten sides.
        set_curve_tol_override(None);
        let in_millimetres = sides(100.0, curve_tol_for(100.0));
        let in_metres = sides(0.1, curve_tol_for(0.1));
        assert!(
            (in_millimetres - in_metres).abs() < 1.0,
            "{in_millimetres} vs {in_metres}"
        );
        assert!(in_metres > 40.0, "a circle should not read as a decagon: {in_metres}");
    }

    #[test]
    fn zooming_in_buys_detail_and_zooming_out_does_not_cost_it() {
        // Between the two bounds the frame decides, so a curve fills in as the
        // camera closes on it.
        set_curve_tol_override(Some(0.01));
        let far = sides(1.0, curve_tol_for(1.0));
        set_curve_tol_override(Some(0.0001));
        let near = sides(1.0, curve_tol_for(1.0));
        assert!(near > far, "{near} vs {far}");
        // And neither end runs away: a circle is never a polygon and never a
        // thousand-sided one that costs more than it shows.
        set_curve_tol_override(Some(1e-12));
        assert!(sides(1.0, curve_tol_for(1.0)) < 200.0);
        set_curve_tol_override(Some(1e9));
        assert!(sides(1.0, curve_tol_for(1.0)) > 40.0);
        set_curve_tol_override(None);
    }

    #[test]
    fn a_curve_with_no_size_is_left_to_the_frame() {
        // A straight run needs its two ends; there is no curvature to sample.
        set_curve_tol_override(Some(0.02));
        assert_eq!(curve_tol_for(0.0), current_curve_tol());
        assert_eq!(curve_tol_for(f64::NAN), current_curve_tol());
        set_curve_tol_override(None);
    }
}
