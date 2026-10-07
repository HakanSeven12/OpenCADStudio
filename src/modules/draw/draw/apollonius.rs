// Apollonius Circle Solvers for Fluid Tangent Arcs.
//
// Solves:
// - CCP (Curve-Curve-Point): Circle passing through point P and tangent to two circles.
// - PPC (Point-Point-Curve): Circle passing through points P1, P2 and tangent to a circle.
//
// Both use Circle Inversion centered at the pass-through point, mapping circles to
// circles/lines and reducing the problem to basic bitangent or point-tangent lines,
// then inverting back. Closed-form, O(1), and numerically stable.

use glam::DVec2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Circle2D {
    pub center: DVec2,
    pub radius: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct ApolloniusSolution {
    pub center: DVec2,
    pub radius: f64,
    /// Tangent contact point on the first curve.
    pub tangent1: DVec2,
    /// Tangent contact point on the second curve (for CCP) or None (for PPC).
    pub tangent2: Option<DVec2>,
}

/// Invert a circle about origin with inversion radius k.
/// If circle does not pass through origin, returns inverted Circle2D.
fn invert_circle(c: Circle2D, k: f64) -> Option<Circle2D> {
    let d2 = c.center.length_squared();
    let denom = d2 - c.radius * c.radius;
    if denom.abs() < 1e-9 {
        // Circle passes through origin -> inverts to a straight line
        return None;
    }
    let scale = (k * k) / denom;
    Some(Circle2D {
        center: c.center * scale,
        radius: (scale * c.radius).abs(),
    })
}

/// Invert a line `n.x * x + n.y * y + c = 0` (where |n| = 1) about origin with radius k.
/// Line inverts to a circle through origin.
fn invert_line_to_circle(n: DVec2, c: f64, k: f64) -> Option<Circle2D> {
    if c.abs() < 1e-9 {
        // Line passes through origin -> inverts to itself
        return None;
    }
    let radius = (k * k) / (2.0 * c.abs());
    let center = -n * (k * k / (2.0 * c));
    Some(Circle2D { center, radius })
}

/// Find the contact point on `original` closest to tangency with `sol`.
fn compute_contact_point(original: Circle2D, sol: Circle2D) -> DVec2 {
    let dir = sol.center - original.center;
    let dist = dir.length();
    if dist < 1e-9 {
        return original.center + DVec2::new(original.radius, 0.0);
    }
    let u = dir / dist;
    // Choose sign + or - depending on whether sol is external or enclosing
    let p_plus = original.center + u * original.radius;
    let p_minus = original.center - u * original.radius;
    if ((p_plus - sol.center).length() - sol.radius).abs() < 1e-3 {
        p_plus
    } else {
        p_minus
    }
}

/// Bitangent lines between two circles: returns line normals `n` (|n| = 1) and constants `c`
/// such that `n . x + c = 0`.
fn bitangent_lines(c1: Circle2D, c2: Circle2D) -> Vec<(DVec2, f64)> {
    let mut lines = Vec::with_capacity(4);
    let d_vec = c2.center - c1.center;
    let d = d_vec.length();
    if d < 1e-9 {
        return lines;
    }
    let u = d_vec / d;
    let v = DVec2::new(-u.y, u.x);

    // 1. External bitangents (r_diff = r1 - r2)
    let r_diff = c1.radius - c2.radius;
    if d >= r_diff.abs() - 1e-9 {
        let sin_a = (r_diff / d).clamp(-1.0, 1.0);
        let cos_a = (1.0 - sin_a * sin_a).max(0.0).sqrt();
        for sign in [-1.0, 1.0] {
            let n = u * sin_a + v * (cos_a * sign);
            let c = -(n.dot(c1.center) + c1.radius);
            lines.push((n, c));
        }
    }

    // 2. Internal bitangents (r_sum = r1 + r2)
    let r_sum = c1.radius + c2.radius;
    if d >= r_sum - 1e-9 {
        let sin_a = (r_sum / d).clamp(-1.0, 1.0);
        let cos_a = (1.0 - sin_a * sin_a).max(0.0).sqrt();
        for sign in [-1.0, 1.0] {
            let n = u * sin_a + v * (cos_a * sign);
            let c = -(n.dot(c1.center) + c1.radius);
            lines.push((n, c));
        }
    }

    lines
}

/// Tangent lines from a point to a circle: returns lines `n . x + c = 0`.
fn point_circle_tangent_lines(pt: DVec2, circle: Circle2D) -> Vec<(DVec2, f64)> {
    let mut lines = Vec::with_capacity(2);
    let d_vec = circle.center - pt;
    let d = d_vec.length();
    if d < circle.radius - 1e-9 {
        return lines;
    }
    if d < 1e-9 {
        return lines;
    }
    let u = d_vec / d;
    let v = DVec2::new(-u.y, u.x);

    let sin_a = (circle.radius / d).clamp(-1.0, 1.0);
    let cos_a = (1.0 - sin_a * sin_a).max(0.0).sqrt();

    for sign in [-1.0, 1.0] {
        let n = u * sin_a + v * (cos_a * sign);
        let c = -n.dot(pt);
        lines.push((n, c));
    }

    lines
}

/// Solve Apollonius CCP: circle passing through point `p` and tangent to circles `c1` and `c2`.
pub fn solve_ccp(c1: Circle2D, c2: Circle2D, p: DVec2) -> Vec<ApolloniusSolution> {
    let k = 100.0; // Inversion radius
    // Translate origin to p
    let c1_local = Circle2D {
        center: c1.center - p,
        radius: c1.radius,
    };
    let c2_local = Circle2D {
        center: c2.center - p,
        radius: c2.radius,
    };

    let Some(c1_inv) = invert_circle(c1_local, k) else {
        return Vec::new();
    };
    let Some(c2_inv) = invert_circle(c2_local, k) else {
        return Vec::new();
    };

    let lines = bitangent_lines(c1_inv, c2_inv);
    let mut solutions = Vec::new();

    for (n, c) in lines {
        if let Some(sol_local) = invert_line_to_circle(n, c, k) {
            let sol = Circle2D {
                center: sol_local.center + p,
                radius: sol_local.radius,
            };
            // Verify sol is actually tangent to c1 and c2 within tolerance
            let d1 = (sol.center - c1.center).length();
            let d2 = (sol.center - c2.center).length();
            let tan1_valid = ((d1 - (sol.radius + c1.radius)).abs() < 1e-4)
                || ((d1 - (sol.radius - c1.radius).abs()).abs() < 1e-4);
            let tan2_valid = ((d2 - (sol.radius + c2.radius)).abs() < 1e-4)
                || ((d2 - (sol.radius - c2.radius).abs()).abs() < 1e-4);

            if tan1_valid && tan2_valid {
                let tangent1 = compute_contact_point(c1, sol);
                let tangent2 = compute_contact_point(c2, sol);
                solutions.push(ApolloniusSolution {
                    center: sol.center,
                    radius: sol.radius,
                    tangent1,
                    tangent2: Some(tangent2),
                });
            }
        }
    }

    solutions
}

/// Solve Apollonius PPC: circle passing through points `p1` and `p2`, and tangent to circle `c1`.
pub fn solve_ppc(c1: Circle2D, p1: DVec2, p2: DVec2) -> Vec<ApolloniusSolution> {
    if (p1 - p2).length_squared() < 1e-12 {
        return Vec::new();
    }
    let k = 100.0;
    // Translate origin to p1
    let c1_local = Circle2D {
        center: c1.center - p1,
        radius: c1.radius,
    };
    let p2_local = p2 - p1;

    let Some(c1_inv) = invert_circle(c1_local, k) else {
        return Vec::new();
    };
    let p2_inv_len2 = p2_local.length_squared();
    if p2_inv_len2 < 1e-12 {
        return Vec::new();
    }
    let p2_inv = p2_local * (k * k / p2_inv_len2);

    let lines = point_circle_tangent_lines(p2_inv, c1_inv);
    let mut solutions = Vec::new();

    for (n, c) in lines {
        if let Some(sol_local) = invert_line_to_circle(n, c, k) {
            let sol = Circle2D {
                center: sol_local.center + p1,
                radius: sol_local.radius,
            };
            let d1 = (sol.center - c1.center).length();
            let tan1_valid = ((d1 - (sol.radius + c1.radius)).abs() < 1e-4)
                || ((d1 - (sol.radius - c1.radius).abs()).abs() < 1e-4);
            let passes_p2 = ((sol.center - p2).length() - sol.radius).abs() < 1e-4;

            if tan1_valid && passes_p2 {
                let tangent1 = compute_contact_point(c1, sol);
                solutions.push(ApolloniusSolution {
                    center: sol.center,
                    radius: sol.radius,
                    tangent1,
                    tangent2: None,
                });
            }
        }
    }

    solutions
}

/// Choose the best CCP candidate closest to pick hints `hint1` and `hint2`.
pub fn best_ccp(
    candidates: &[ApolloniusSolution],
    hint1: DVec2,
    hint2: DVec2,
) -> Option<ApolloniusSolution> {
    candidates
        .iter()
        .filter(|sol| sol.radius.is_finite() && sol.radius > 1e-4)
        .min_by(|a, b| {
            let score = |s: &ApolloniusSolution| -> f64 {
                let d1 = (s.tangent1 - hint1).length();
                let d2 = s.tangent2.map_or(0.0, |t| (t - hint2).length());
                let r_pen = if s.radius > 50000.0 { 10000.0 } else { 0.0 };
                d1 + d2 + r_pen
            };
            score(a).total_cmp(&score(b))
        })
        .copied()
}

/// Choose the best PPC candidate closest to pick hint `hint1`.
pub fn best_ppc(
    candidates: &[ApolloniusSolution],
    hint1: DVec2,
) -> Option<ApolloniusSolution> {
    candidates
        .iter()
        .filter(|sol| sol.radius.is_finite() && sol.radius > 1e-4)
        .min_by(|a, b| {
            let score = |s: &ApolloniusSolution| -> f64 {
                let d1 = (s.tangent1 - hint1).length();
                let r_pen = if s.radius > 50000.0 { 10000.0 } else { 0.0 };
                d1 + r_pen
            };
            score(a).total_cmp(&score(b))
        })
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ccp_two_equal_circles() {
        // Two circles of radius 10 at (-20, 0) and (20, 0).
        // Middle point P at (0, 10).
        // A circle through (0, 10) tangent to both circles at their top (+Y).
        let c1 = Circle2D {
            center: DVec2::new(-20.0, 0.0),
            radius: 10.0,
        };
        let c2 = Circle2D {
            center: DVec2::new(20.0, 0.0),
            radius: 10.0,
        };
        let p = DVec2::new(0.0, 10.0);

        let sols = solve_ccp(c1, c2, p);
        assert!(!sols.is_empty(), "Should find CCP solutions");

        let best = best_ccp(&sols, DVec2::new(-20.0, 10.0), DVec2::new(20.0, 10.0)).unwrap();
        // Check that distance to c1 center is radius +- 10
        let d1 = (best.center - c1.center).length();
        assert!((d1 - (best.radius + c1.radius)).abs() < 1e-3 || (d1 - (best.radius - c1.radius).abs()).abs() < 1e-3);
        // Check that point P is on the circle
        assert!(((best.center - p).length() - best.radius).abs() < 1e-3);
    }

    #[test]
    fn test_ppc_circle_and_two_points() {
        // Circle of radius 10 at (0, 0).
        // Points at (0, 20) and (20, 20).
        let c = Circle2D {
            center: DVec2::new(0.0, 0.0),
            radius: 10.0,
        };
        let p1 = DVec2::new(0.0, 20.0);
        let p2 = DVec2::new(20.0, 20.0);

        let sols = solve_ppc(c, p1, p2);
        assert!(!sols.is_empty(), "Should find PPC solutions");

        let best = best_ppc(&sols, DVec2::new(0.0, 10.0)).unwrap();
        assert!(((best.center - p1).length() - best.radius).abs() < 1e-3);
        assert!(((best.center - p2).length() - best.radius).abs() < 1e-3);
    }
}
