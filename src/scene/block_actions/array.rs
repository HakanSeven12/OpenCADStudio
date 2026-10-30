//! Bounded array planning shared by committed and preview evaluation.
use super::*;

/// Aggregate generated top-level entities per definition evaluation. Check this
/// before allocating handles or copying geometry, including across actions.
pub(super) const MAX_GENERATED_ENTITIES: usize = 100_000;
pub(super) const LIMIT_ERROR: &str =
    "Block evaluation would generate more than 100000 array objects. Reduce the distance or increase the spacing.";

#[derive(Debug)]
pub(super) struct Plan {
    columns: usize,
    cells: usize,
    column_step: DVec3,
    row_step: DVec3,
}

impl Plan {
    pub(super) fn new(
        parameter: &Parameter,
        base: DVec3,
        end: DVec3,
        action: &BlockArrayAction,
        targets: usize,
        remaining: &mut usize,
    ) -> Result<Self, &'static str> {
        if !base.is_finite() || !end.is_finite() {
            return Err("The parameter value produces non-finite geometry.");
        }
        let mut result = Self {
            columns: 1,
            cells: 1,
            column_step: DVec3::ZERO,
            row_step: DVec3::ZERO,
        };
        // Self-grip dependencies do not produce array copies.
        if targets == 0 {
            return Ok(result);
        }
        let count = |span: f64, spacing: f64| -> Result<usize, &'static str> {
            let cells = ((span + 1e-9) / spacing).floor() + 1.;
            if !span.is_finite()
                || !cells.is_finite()
                || cells < 1.
                || cells > (MAX_GENERATED_ENTITIES + 1) as f64
            {
                return Err(LIMIT_ERROR);
            }
            Ok(cells as usize)
        };
        let (columns, rows) = if parameter.xy {
            let delta = end - base;
            result.column_step =
                DVec3::X * action.column_offset * if end.x >= base.x { 1. } else { -1. };
            result.row_step = DVec3::Y * action.row_offset * if end.y >= base.y { 1. } else { -1. };
            (
                count(delta.x.abs(), action.column_offset)?,
                count(delta.y.abs(), action.row_offset)?,
            )
        } else {
            let (span, axis) = if parameter.linear {
                let axis = (parameter.end - parameter.base).normalize();
                ((end - base).dot(axis).max(0.), axis)
            } else {
                let delta = end - base;
                let span = delta.length();
                let axis = if span > 1e-9 {
                    delta / span
                } else {
                    (parameter.end - parameter.base).normalize()
                };
                (span, axis)
            };
            result.column_step = axis * action.column_offset;
            (count(span, action.column_offset)?, 1)
        };
        let cells = columns.checked_mul(rows).ok_or(LIMIT_ERROR)?;
        let generated = (cells - 1).checked_mul(targets).ok_or(LIMIT_ERROR)?;
        let budget = remaining.checked_sub(generated).ok_or(LIMIT_ERROR)?;
        if !result.column_step.is_finite() || !result.row_step.is_finite() {
            return Err("The parameter value produces non-finite geometry.");
        }
        result.columns = columns;
        result.cells = cells;
        *remaining = budget;
        Ok(result)
    }

    /// Row-major native ordering; the source cell is excluded. Constant memory
    /// keeps preview planning independent of the number of generated copies.
    pub(super) fn displacements(&self) -> impl Iterator<Item = (usize, DVec3)> + '_ {
        (1..self.cells).map(|index| {
            let column = index % self.columns;
            let row = index / self.columns;
            (
                index,
                self.column_step * column as f64 + self.row_step * row as f64,
            )
        })
    }
}

#[cfg(test)]
mod tests;
