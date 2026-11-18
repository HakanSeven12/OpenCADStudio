//! Rigid placement from one, two, or three corresponding point pairs.

use super::Vec3;

/// Edge or center used to align a collection of 2D axis-aligned bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundsAlignment {
    Left,
    HorizontalCenter,
    Right,
    Top,
    VerticalCenter,
    Bottom,
}

/// Translation offsets that align each `[min_x, min_y, max_x, max_y]` box
/// within the combined bounds of the collection.
pub fn align_aabbs_2d(bounds: &[[f64; 4]], alignment: BoundsAlignment) -> Option<Vec<[f64; 2]>> {
    if bounds.len() < 2
        || bounds
            .iter()
            .any(|b| b.iter().any(|value| !value.is_finite()) || b[0] > b[2] || b[1] > b[3])
    {
        return None;
    }

    let min_x = bounds.iter().map(|b| b[0]).fold(f64::INFINITY, f64::min);
    let min_y = bounds.iter().map(|b| b[1]).fold(f64::INFINITY, f64::min);
    let max_x = bounds
        .iter()
        .map(|b| b[2])
        .fold(f64::NEG_INFINITY, f64::max);
    let max_y = bounds
        .iter()
        .map(|b| b[3])
        .fold(f64::NEG_INFINITY, f64::max);
    let center_x = (min_x + max_x) * 0.5;
    let center_y = (min_y + max_y) * 0.5;

    Some(
        bounds
            .iter()
            .map(|b| match alignment {
                BoundsAlignment::Left => [min_x - b[0], 0.0],
                BoundsAlignment::HorizontalCenter => [center_x - (b[0] + b[2]) * 0.5, 0.0],
                BoundsAlignment::Right => [max_x - b[2], 0.0],
                BoundsAlignment::Top => [0.0, max_y - b[3]],
                BoundsAlignment::VerticalCenter => [0.0, center_y - (b[1] + b[3]) * 0.5],
                BoundsAlignment::Bottom => [0.0, min_y - b[1]],
            })
            .collect(),
    )
}

/// Construct a row-major affine matrix from corresponding source and target
/// points. One pair translates; two pairs also rotate and optionally scale;
/// three pairs orient both the baseline and the plane without scaling.
///
/// The first pair is exact. Additional pairs define directions, so unequal
/// point spacing does not introduce shear. Nonfinite coordinates, coincident
/// baselines, and collinear three-point frames are rejected without a matrix.
pub fn align_point_pairs(
    source: &[[f64; 3]],
    target: &[[f64; 3]],
    scale_two_pairs: bool,
) -> Option<[[f64; 4]; 4]> {
    if source.len() != target.len() || !(1..=3).contains(&source.len())
        || source.iter().chain(target).flatten().any(|v| !v.is_finite()) {
        return None;
    }
    let source_origin = Vec3::from(source[0]);
    let target_origin = Vec3::from(target[0]);
    let axes = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)];
    let mut columns = axes;
    if source.len() >= 2 {
        let a = Vec3::from(source[1]) - source_origin;
        let b = Vec3::from(target[1]) - target_origin;
        let a_length = a.length();
        let b_length = b.length();
        if !a_length.is_finite() || !b_length.is_finite() || a_length <= 1e-12 || b_length <= 1e-12 {
            return None;
        }
        let x = a / a_length;
        let u = b / b_length;
        if source.len() == 3 {
            let c = (Vec3::from(source[2]) - source_origin).normalize()?;
            let d = (Vec3::from(target[2]) - target_origin).normalize()?;
            let source_normal = x.cross(c);
            let target_normal = u.cross(d);
            if source_normal.length() <= 1e-12 || target_normal.length() <= 1e-12 {
                return None;
            }
            let z = source_normal.normalize()?;
            let w = target_normal.normalize()?;
            let y = z.cross(x);
            let v = w.cross(u);
            columns = axes.map(|axis| u * x.dot(axis) + v * y.dot(axis) + w * z.dot(axis));
        } else {
            let cosine = x.dot(u).clamp(-1.0, 1.0);
            let cross = x.cross(u);
            let sine = cross.length();
            if sine > 1e-12 {
                let axis = cross / sine;
                columns = axes.map(|basis| basis * cosine + axis.cross(basis) * sine
                    + axis * (axis.dot(basis) * (1.0 - cosine)));
            } else if cosine < 0.0 {
                // At a half-turn the normal is underdetermined. Keep the
                // world vertical axis when possible, otherwise the Y axis.
                let preferred = if x.z.abs() < 0.9 { axes[2] } else { axes[1] };
                let axis = (preferred - x * x.dot(preferred)).normalize()?;
                columns = axes.map(|basis| axis * (2.0 * axis.dot(basis)) - basis);
            }
            if scale_two_pairs {
                let scale = b_length / a_length;
                if !scale.is_finite() { return None; }
                columns = columns.map(|column| column * scale);
            }
        }
    }
    let translation = target_origin - columns[0] * source_origin.x
        - columns[1] * source_origin.y - columns[2] * source_origin.z;
    let matrix = [
        [columns[0].x, columns[1].x, columns[2].x, translation.x],
        [columns[0].y, columns[1].y, columns[2].y, translation.y],
        [columns[0].z, columns[1].z, columns[2].z, translation.z],
        [0.0, 0.0, 0.0, 1.0],
    ];
    matrix.iter().flatten().all(|v| v.is_finite()).then_some(matrix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_pairs_map_a_spatial_frame() {
        let source = [[1.0, 2.0, 3.0], [3.0, 2.0, 3.0], [1.0, 5.0, 3.0]];
        let target = [[7.0, 8.0, 9.0], [7.0, 10.0, 9.0], [4.0, 8.0, 9.0]];
        let matrix = align_point_pairs(&source, &target, true).unwrap();
        for (from, to) in source.into_iter().zip(target) {
            let actual = std::array::from_fn(|row| matrix[row][0] * from[0]
                + matrix[row][1] * from[1] + matrix[row][2] * from[2] + matrix[row][3]);
            assert!(Vec3::from(actual).distance(Vec3::from(to)) < 1e-12);
        }
    }

    #[test]
    fn aligns_bounds_edges_and_centers() {
        let bounds = [[0.0, 0.0, 2.0, 2.0], [5.0, -3.0, 8.0, -2.0]];
        assert_eq!(
            align_aabbs_2d(&bounds, BoundsAlignment::Left).unwrap(),
            vec![[0.0, 0.0], [-5.0, 0.0]]
        );
        assert_eq!(
            align_aabbs_2d(&bounds, BoundsAlignment::Top).unwrap(),
            vec![[0.0, 0.0], [0.0, 4.0]]
        );
        assert_eq!(
            align_aabbs_2d(&bounds, BoundsAlignment::HorizontalCenter).unwrap(),
            vec![[3.0, 0.0], [-2.5, 0.0]]
        );
    }

    #[test]
    fn rejects_unusable_bounds() {
        assert!(align_aabbs_2d(&[[0.0; 4]], BoundsAlignment::Left).is_none());
        assert!(
            align_aabbs_2d(&[[1.0, 0.0, -1.0, 0.0], [0.0; 4]], BoundsAlignment::Left,).is_none()
        );
    }
}
