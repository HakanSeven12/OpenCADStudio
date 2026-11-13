use kurbo::Affine;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeoPlacement {
    pub insertion: [f64; 2],
    pub x_scale: f64,
    pub y_scale: f64,
    pub rotation: f64,
}

fn finite(affine: Affine) -> bool {
    affine.as_coeffs().iter().all(|value| value.is_finite())
}

/// Least-squares PDF-user-space → drawing XY, fitted about the centroid to
/// avoid losing the translation when coordinates are large.
pub(crate) fn fit_affine(points: &[([f64; 2], [f64; 2])]) -> Result<Affine, String> {
    if points.len() < 3 || points.len() > 4096 {
        return Err("GeoPDF needs between 3 and 4096 registration points".to_string());
    }
    if points.iter().any(|(source, target)| {
        source
            .iter()
            .chain(target.iter())
            .any(|value| !value.is_finite())
    }) {
        return Err("GeoPDF registration contains a non-finite coordinate".to_string());
    }
    let count = points.len() as f64;
    let source_center = points.iter().fold([0.0; 2], |mut center, (point, _)| {
        center[0] += point[0] / count;
        center[1] += point[1] / count;
        center
    });
    let target_center = points.iter().fold([0.0; 2], |mut center, (_, point)| {
        center[0] += point[0] / count;
        center[1] += point[1] / count;
        center
    });

    let mut normal = [[0.0; 3]; 3];
    let mut rhs_x = [0.0; 3];
    let mut rhs_y = [0.0; 3];
    let mut output_span = 0.0_f64;
    for (source, target) in points {
        let row = [
            source[0] - source_center[0],
            source[1] - source_center[1],
            1.0,
        ];
        let x = target[0] - target_center[0];
        let y = target[1] - target_center[1];
        output_span = output_span.max(x.abs()).max(y.abs());
        for r in 0..3 {
            for c in 0..3 {
                normal[r][c] += row[r] * row[c];
            }
            rhs_x[r] += row[r] * x;
            rhs_y[r] += row[r] * y;
        }
    }
    let scale = normal
        .iter()
        .flatten()
        .fold(0.0_f64, |scale, value| scale.max(value.abs()));
    if scale <= f64::EPSILON {
        return Err("GeoPDF registration points are coincident".to_string());
    }
    let x = solve_3x3(normal, rhs_x, scale)?;
    let y = solve_3x3(normal, rhs_y, scale)?;
    let affine = Affine::new([
        x[0],
        y[0],
        x[1],
        y[1],
        target_center[0] - x[0] * source_center[0] - x[1] * source_center[1],
        target_center[1] - y[0] * source_center[0] - y[1] * source_center[1],
    ]);
    if !finite(affine) {
        return Err("GeoPDF registration produced a non-finite affine".to_string());
    }

    let tolerance = 1e-9 * output_span.max(1.0) + 1e-6;
    for (source, target) in points {
        let mapped = affine * kurbo::Point::new(source[0], source[1]);
        let residual = (mapped.x - target[0]).hypot(mapped.y - target[1]);
        if !residual.is_finite() || residual > tolerance {
            return Err("GeoPDF registration is not one affine transform".to_string());
        }
    }
    Ok(affine)
}

fn solve_3x3(mut matrix: [[f64; 3]; 3], mut rhs: [f64; 3], scale: f64) -> Result<[f64; 3], String> {
    for column in 0..3 {
        let mut pivot = column;
        for row in column + 1..3 {
            if matrix[row][column].abs() > matrix[pivot][column].abs() {
                pivot = row;
            }
        }
        if matrix[pivot][column].abs() <= scale * 1e-12 {
            return Err("GeoPDF registration points are collinear".to_string());
        }
        if pivot != column {
            matrix.swap(pivot, column);
            rhs.swap(pivot, column);
        }
        for row in column + 1..3 {
            let pivot_row = matrix[column];
            let factor = matrix[row][column] / pivot_row[column];
            for (value, pivot) in matrix[row][column..]
                .iter_mut()
                .zip(pivot_row[column..].iter())
            {
                *value -= factor * pivot;
            }
            rhs[row] -= factor * rhs[column];
        }
    }
    let mut result = [0.0; 3];
    for row in (0..3).rev() {
        let remainder = (row + 1..3)
            .map(|column| matrix[row][column] * result[column])
            .sum::<f64>();
        result[row] = (rhs[row] - remainder) / matrix[row][row];
    }
    Ok(result)
}

/// Convert the fitted PDF-user-space → drawing affine into the existing
/// insertion/X/Y-scale/rotation representation. Mirroring and shear are not
/// silently approximated.
pub(crate) fn decompose(affine: Affine) -> Result<GeoPlacement, String> {
    if !finite(affine) {
        return Err("GeoPDF placement is non-finite".to_string());
    }
    let [a, b, c, d, e, f] = affine.as_coeffs();
    let x_scale = a.hypot(b);
    let second_length = c.hypot(d);
    if x_scale <= 0.0 || second_length <= 0.0 {
        return Err("GeoPDF placement is singular".to_string());
    }
    let orthogonality = (a * c + b * d).abs() / (x_scale * second_length);
    if orthogonality > 1e-9 {
        return Err("GeoPDF placement contains unsupported shear".to_string());
    }
    let rotation = b.atan2(a);
    let (sin, cos) = rotation.sin_cos();
    let y_scale = -c * sin + d * cos;
    if !y_scale.is_finite() || y_scale <= 0.0 {
        return Err("GeoPDF placement is mirrored or degenerate".to_string());
    }
    let reconstructed = [x_scale * cos, x_scale * sin, -y_scale * sin, y_scale * cos];
    let scale = x_scale.max(y_scale).max(1.0);
    if reconstructed
        .into_iter()
        .zip([a, b, c, d])
        .any(|(actual, expected)| (actual - expected).abs() > scale * 1e-8)
    {
        return Err("GeoPDF placement cannot be represented without shear".to_string());
    }
    Ok(GeoPlacement {
        insertion: [e, f],
        x_scale,
        y_scale,
        rotation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-7, "{actual} != {expected}");
    }

    #[test]
    fn fits_large_coordinates_and_rejects_non_affine_points() {
        let transform = Affine::new([2.0, 0.5, -0.25, 3.0, -742_925.271389, -1_043_020.310124]);
        let source = [[0.0, 0.0], [100.0, 0.0], [0.0, 200.0], [100.0, 200.0]];
        let points: Vec<_> = source
            .into_iter()
            .map(|point| {
                let mapped = transform * kurbo::Point::new(point[0], point[1]);
                (point, [mapped.x, mapped.y])
            })
            .collect();
        let fitted = fit_affine(&points).unwrap();
        for (actual, expected) in fitted.as_coeffs().into_iter().zip(transform.as_coeffs()) {
            close(actual, expected);
        }
        let mut bad = points.clone();
        bad[3].1[0] += 1.0;
        assert!(fit_affine(&bad).is_err());
        assert!(fit_affine(&points[..2]).is_err());
        assert!(fit_affine(&[
            ([0.0, 0.0], [0.0, 0.0]),
            ([1.0, 1.0], [1.0, 1.0]),
            ([2.0, 2.0], [2.0, 2.0]),
        ])
        .is_err());
    }

    #[test]
    fn decomposes_rotation_and_nonuniform_scale_but_not_shear_or_reflection() {
        let angle = 0.7_f64;
        let (sin, cos) = angle.sin_cos();
        let placement = decompose(Affine::new([
            2.0 * cos,
            2.0 * sin,
            -3.0 * sin,
            3.0 * cos,
            -1000.0,
            2000.0,
        ]))
        .unwrap();
        close(placement.x_scale, 2.0);
        close(placement.y_scale, 3.0);
        close(placement.rotation, angle);
        assert_eq!(placement.insertion, [-1000.0, 2000.0]);
        assert!(decompose(Affine::new([1.0, 0.0, 0.2, 1.0, 0.0, 0.0])).is_err());
        assert!(decompose(Affine::new([-1.0, 0.0, 0.0, 1.0, 0.0, 0.0])).is_err());
        assert!(decompose(Affine::new([0.0, 0.0, 0.0, 1.0, 0.0, 0.0])).is_err());
    }
}
