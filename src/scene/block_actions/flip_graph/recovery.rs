//! Recover canonical inputs from native frames by reversing the active Flips.
use super::*;

impl Plan {
    pub(super) fn recover_native(
        &self,
        doc: &Document,
        baseline: &Document,
        def: Handle,
        insert: Handle,
        states: &[bool],
    ) -> Result<BTreeMap<Handle, ParameterFrame>, &'static str> {
        let steps: Vec<_> = self
            .steps
            .iter()
            .zip(states)
            .filter_map(|(step, active)| active.then_some(step))
            .collect();
        let required: BTreeSet<_> = steps
            .iter()
            .flat_map(|step| std::iter::once(step.axis).chain(step.targets.iter().copied()))
            .collect();
        let mut visible = BTreeMap::new();
        for p in &self.parameters {
            if required.contains(&p.handle) && instance::cache(doc, insert, p.id)?.is_none() {
                return Err(FRAME_ERROR);
            }
            visible.insert(p.handle, instance::frame(doc, insert, p)?);
        }
        let graph = chains::Graph::new(baseline, def)?;
        let unknown: BTreeMap<_, _> = steps
            .iter()
            .map(|step| step.axis)
            .filter(|axis| graph.has_unchained_incoming_move(*axis))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .enumerate()
            .map(|(index, axis)| (axis, index))
            .collect();
        let inverse = |offsets: &[f64]| -> Result<BTreeMap<Handle, ParameterFrame>, &'static str> {
            let mut frames = visible.clone();
            for step in steps.iter().rev() {
                let mut axis = frames[&step.axis];
                if let Some(index) = unknown.get(&step.axis) {
                    let shift = normal(axis)? * offsets[*index];
                    axis.base += shift;
                    axis.end += shift;
                }
                for target in &step.targets {
                    let frame = frames.get_mut(target).ok_or(FRAME_ERROR)?;
                    frame.base = Self::reflected(frame.base, axis)?;
                    frame.end = Self::reflected(frame.end, axis)?;
                }
            }
            Ok(frames)
        };
        let active_frames = |frames: &BTreeMap<Handle, ParameterFrame>| -> Result<_, &'static str> {
            let values = frames.iter().map(|(h, f)| (*h, f.end)).collect();
            let bases = frames.iter().map(|(h, f)| (*h, f.base)).collect();
            let autonomous = graph.autonomous(baseline, Some(&values), Some(&bases))?;
            Ok(frames
                .iter()
                .map(|(h, f)| (*h, graph.active_frame(*h, *f, &autonomous)))
                .collect::<BTreeMap<_, _>>())
        };

        // Move transport changes axis positions but never their directions.
        // Each unknown is a shift along the axis normal. With fixed directions,
        // inversion and Move propagation are affine in these shifts.
        let residual = |offsets: &[f64]| -> Result<Vec<f64>, &'static str> {
            let frames = inverse(offsets)?;
            let active = active_frames(&frames)?;
            let mut residuals = vec![0.0; unknown.len()];
            for (axis, index) in &unknown {
                let effective = frames[axis];
                let shift = (active[axis].base - effective.base).dot(normal(effective)?);
                residuals[*index] = offsets[*index] - shift;
            }
            Ok(residuals)
        };
        let offsets = solve_affine(unknown.len(), residual)?;
        let canonical = inverse(&offsets)?;
        for p in self
            .parameters
            .iter()
            .filter(|p| p.linear && required.contains(&p.handle))
        {
            let frame = canonical[&p.handle];
            let direction = (frame.end - frame.base)
                .try_normalize()
                .ok_or(FRAME_ERROR)?;
            let authored = (p.end - p.base).try_normalize().ok_or(FRAME_ERROR)?;
            if direction.distance(authored) > 1e-7 {
                return Err(FRAME_ERROR);
            }
        }
        let replay = self.display_frames(states, &canonical, &active_frames(&canonical)?)?;
        if visible
            .iter()
            .any(|(h, frame)| !near(frame.base, replay[h].base) || !near(frame.end, replay[h].end))
        {
            return Err(FRAME_ERROR);
        }
        Ok(canonical)
    }
}

fn normal(frame: ParameterFrame) -> Result<DVec3, &'static str> {
    DVec3::Z
        .cross(frame.end - frame.base)
        .try_normalize()
        .ok_or(FRAME_ERROR)
}

fn near(a: DVec3, b: DVec3) -> bool {
    a.distance(b) <= 1e-7 + 1e-12 * a.length().max(b.length())
}

/// Sample an affine residual and solve its zero with partial pivoting.
fn solve_affine(
    size: usize,
    residual: impl Fn(&[f64]) -> Result<Vec<f64>, &'static str>,
) -> Result<Vec<f64>, &'static str> {
    if size == 0 {
        return Ok(Vec::new());
    }
    let mut sample = vec![0.0; size];
    let constant = residual(&sample)?;
    let mut matrix = vec![vec![0.0; size + 1]; size];
    for column in 0..size {
        sample[column] = 1.0;
        let values = residual(&sample)?;
        sample[column] = 0.0;
        for row in 0..size {
            matrix[row][column] = values[row] - constant[row];
        }
    }
    for (row, value) in matrix.iter_mut().zip(constant) {
        row[size] = -value;
    }
    for column in 0..size {
        let pivot = (column..size)
            .max_by(|a, b| {
                matrix[*a][column]
                    .abs()
                    .total_cmp(&matrix[*b][column].abs())
            })
            .ok_or(FRAME_ERROR)?;
        matrix.swap(column, pivot);
        let scale = matrix[column][column];
        if !scale.is_finite() || scale.abs() <= 1e-9 {
            return Err(FRAME_ERROR);
        }
        for value in &mut matrix[column][column..] {
            *value /= scale;
        }
        let pivot_row = matrix[column].clone();
        for (index, row) in matrix.iter_mut().enumerate() {
            if index == column {
                continue;
            }
            let scale = row[column];
            for entry in column..=size {
                row[entry] -= scale * pivot_row[entry];
            }
        }
    }
    let result: Vec<_> = matrix.iter().map(|row| row[size]).collect();
    if result.iter().all(|value| value.is_finite()) {
        Ok(result)
    } else {
        Err(FRAME_ERROR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coupled_axis_offsets_require_a_unique_solution() {
        let solved = solve_affine(2, |x| Ok(vec![x[1] - 3.0, 2.0 * x[0] + x[1] - 7.0])).unwrap();
        assert_eq!(solved, [2.0, 3.0]);
        assert_eq!(
            solve_affine(2, |x| Ok(vec![x[0] + x[1], 2.0 * x[0] + 2.0 * x[1]])),
            Err(FRAME_ERROR)
        );
    }
}
