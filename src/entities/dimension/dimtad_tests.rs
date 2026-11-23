// Tests for dimtad.
use super::*;
use crate::t;

use super::text_on_dim_line;

fn v(x: f64, y: f64) -> Vector3 {
    Vector3::new(x, y, 0.0)
}

// DIMTAD=Above must place text on the geometric "up" side of a horizontal
// dimension whichever way the dimension runs; DIMTAD=Below flips it. (#144)
#[test]
fn above_is_up_regardless_of_direction() {
    let (first, second, defpt) = (v(0.0, 10.0), v(20.0, 10.0), v(0.0, 0.0));
    let perp = 5.0;
    let fwd = text_on_dim_line(
        first, second, defpt, 1.0, 0.0, 0, perp, 0.0, 0.0, 1.0, false, 3, 1, false,
    );
    let rev = text_on_dim_line(
        first, second, defpt, -1.0, 0.0, 0, perp, 0.0, 0.0, 1.0, false, 3, 1, false,
    );
    assert!(fwd.y > 0.0, "above must be +Y, got {}", fwd.y);
    assert!(rev.y > 0.0, "above must be +Y even reversed, got {}", rev.y);
    let below = text_on_dim_line(
        first, second, defpt, 1.0, 0.0, 0, perp, 0.0, 0.0, 1.0, false, 3, 4, false,
    );
    assert!(below.y < 0.0, "below must be -Y, got {}", below.y);
}

// DIMTAD=Outside (2) stays on the side farthest from the measured points,
// whichever side of the geometry the dimension line is on.
#[test]
fn outside_is_away_from_object() {
    let perp = 5.0;
    // Object at y=0, dim line above it (y=10): away → further above.
    let above = text_on_dim_line(
        v(0.0, 0.0),
        v(20.0, 0.0),
        v(0.0, 10.0),
        1.0,
        0.0,
        0,
        perp,
        0.0,
        0.0,
        1.0,
        false,
        3,
        2,
        false,
    );
    assert!(
        above.y > 10.0,
        "outside must clear the object side, got {}",
        above.y
    );
    // Object at y=0, dim line below it (y=-10): away → further below.
    let below = text_on_dim_line(
        v(0.0, 0.0),
        v(20.0, 0.0),
        v(0.0, -10.0),
        1.0,
        0.0,
        0,
        perp,
        0.0,
        0.0,
        1.0,
        false,
        3,
        2,
        false,
    );
    assert!(
        below.y < -10.0,
        "outside must clear the object side, got {}",
        below.y
    );
}
