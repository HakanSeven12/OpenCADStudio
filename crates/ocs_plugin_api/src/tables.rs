//! The typed-table surface data model (api v29, S07): tables with STABLE
//! row keys, TYPED SCALAR cells, and atomic cell/row patches.
//!
//! Ownership split (the scene/widget tier law): the plugin owns the
//! semantics; the host owns selection, scroll and the editing session
//! (host-local transient state), rendering natively. The wire carries
//! only this serializable vocabulary.
//!
//! Finding F4's table requirements, minimally: STABLE row keys survive
//! edits and paging; CELL patches update one cell without republishing
//! the table; row patches add/remove whole rows; a page is a batch of
//! `InsertRow` ops positioned after an existing row. Finding-scope
//! exclusions honored: no shared-table reference machinery, no
//! non-scalar presentation cells (a concrete example will introduce
//! them).
//!
//! Atomic everywhere: a patch stages a candidate and swaps only on full
//! success — a failing op never leaves a partial table.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

use crate::scene::StableId;

/// A table surface's own revision — independent of scene/widget
/// revisions (finding F1).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct TableRevision(pub u64);

/// Maximum rows in one table surface (mirrors the widget budget).
pub const MAX_TABLE_ROWS: usize = 10_000;

/// Maximum ops in one table patch (mirrors the scene patch budget).
pub const MAX_TABLE_OPS: usize = 10_000;

/// A typed scalar cell value (finding-scope exclusion: non-scalar
/// presentation types wait for a concrete example). Numbers must be
/// finite; text is bounded by the dialog text budget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CellValue {
    Text(String),
    Number(f64),
    Bool(bool),
}

/// One column: a stable key (the cell index is DERIVED from the column
/// order, never stored per cell) and a display label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableColumn {
    pub id: StableId,
    pub label: String,
}

/// Why a table patch refused. Codes append at the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableError {
    /// The patch's base revision is not the table's current revision.
    StaleBase,
    /// The patch carries more ops than [`MAX_TABLE_OPS`].
    PatchTooLarge,
    /// The table would exceed [`MAX_TABLE_ROWS`].
    TableTooLarge,
    /// The op names a row that is not in the table.
    UnknownRow,
    /// The op names a column that is not in the table.
    UnknownColumn,
    /// A row id collides with an existing row (or appears twice).
    DuplicateRow,
    /// A cell count does not match the column count.
    ArityMismatch,
    /// A number cell is not finite (NaN/inf are rejected).
    NonFiniteNumber,
    /// A text cell exceeds the text budget.
    TextTooLong,
}

impl core::fmt::Display for TableError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TableError::StaleBase => write!(f, "patch base is not the current table revision"),
            TableError::PatchTooLarge => write!(f, "table patch carries too many ops"),
            TableError::TableTooLarge => write!(f, "table row budget exceeded"),
            TableError::UnknownRow => write!(f, "table row is not in the surface"),
            TableError::UnknownColumn => write!(f, "table column is not in the surface"),
            TableError::DuplicateRow => write!(f, "table row id collides"),
            TableError::ArityMismatch => write!(f, "row cells do not match the column count"),
            TableError::NonFiniteNumber => write!(f, "number cell is not finite"),
            TableError::TextTooLong => write!(f, "text cell exceeds the text budget"),
        }
    }
}

/// The stored table surface: columns (ordered, stable keys), rows keyed
/// by stable id, and the display order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TableSurface {
    pub revision: TableRevision,
    pub columns: Vec<TableColumn>,
    pub rows: BTreeMap<StableId, Vec<CellValue>>,
    pub order: Vec<StableId>,
}

impl TableSurface {
    /// A fresh, empty table with `columns` (labels bounded).
    pub fn new(revision: TableRevision, columns: Vec<TableColumn>) -> Self {
        Self {
            revision,
            columns,
            rows: BTreeMap::new(),
            order: Vec::new(),
        }
    }

    fn check_cells(&self, cells: &[CellValue]) -> Result<(), TableError> {
        if cells.len() != self.columns.len() {
            return Err(TableError::ArityMismatch);
        }
        for cell in cells {
            match cell {
                CellValue::Number(n) if !n.is_finite() => return Err(TableError::NonFiniteNumber),
                CellValue::Text(t) if t.len() > crate::scene::MAX_TEXT_BYTES => {
                    return Err(TableError::TextTooLong);
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Validate the whole table: budgets, arity, and every row reachable
    /// from the display order exactly once.
    pub fn validate(&self) -> Result<(), TableError> {
        if self.rows.len() > MAX_TABLE_ROWS {
            return Err(TableError::TableTooLarge);
        }
        for cells in self.rows.values() {
            self.check_cells(cells)?;
        }
        let mut seen = HashSet::new();
        for id in &self.order {
            if !seen.insert(*id) {
                return Err(TableError::DuplicateRow);
            }
            if !self.rows.contains_key(id) {
                return Err(TableError::UnknownRow);
            }
        }
        if seen.len() != self.rows.len() {
            return Err(TableError::UnknownRow);
        }
        Ok(())
    }

    /// Apply a patch atomically: on any refusal the table is
    /// byte-for-byte unchanged; on success the revision advances by one
    /// and the whole table re-validates.
    pub fn apply_patch(&mut self, patch: &TablePatch) -> Result<TableRevision, TableError> {
        if patch.base_revision != self.revision {
            return Err(TableError::StaleBase);
        }
        if patch.ops.len() > MAX_TABLE_OPS {
            return Err(TableError::PatchTooLarge);
        }
        let mut candidate = self.clone();
        for op in &patch.ops {
            apply_op(&mut candidate, op)?;
        }
        candidate.revision = TableRevision(self.revision.0 + 1);
        candidate.validate()?;
        *self = candidate;
        Ok(self.revision)
    }
}

/// One atomic table update op. New ops append at the end; never renumber.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TableOp {
    /// Update ONE cell: the single-cell edit path (the proof obligation
    /// for finding F4 — a cell update never republishes the table).
    SetCell {
        row: StableId,
        column: StableId,
        value: CellValue,
    },
    /// Insert a whole row (a PAGE is a positioned batch of these).
    InsertRow {
        after: Option<StableId>,
        row: StableId,
        cells: Vec<CellValue>,
    },
    /// Remove a row by its stable key.
    RemoveRow { row: StableId },
}

/// A revision-checked table patch: ops apply atomically against the
/// table whose revision equals `base_revision`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TablePatch {
    pub base_revision: TableRevision,
    pub ops: Vec<TableOp>,
}

fn apply_op(table: &mut TableSurface, op: &TableOp) -> Result<(), TableError> {
    match op {
        TableOp::SetCell { row, column, value } => {
            let Some(cells) = table.rows.get_mut(row) else {
                return Err(TableError::UnknownRow);
            };
            let Some(index) = table.columns.iter().position(|c| c.id == *column) else {
                return Err(TableError::UnknownColumn);
            };
            match value {
                CellValue::Number(n) if !n.is_finite() => return Err(TableError::NonFiniteNumber),
                CellValue::Text(t) if t.len() > crate::scene::MAX_TEXT_BYTES => {
                    return Err(TableError::TextTooLong);
                }
                _ => {}
            }
            cells[index] = value.clone();
        }
        TableOp::InsertRow { after, row, cells } => {
            if table.rows.contains_key(row) {
                return Err(TableError::DuplicateRow);
            }
            table.check_cells(cells)?;
            let position = match after {
                None => table.order.len(),
                Some(after_id) => {
                    table
                        .order
                        .iter()
                        .position(|id| id == after_id)
                        .ok_or(TableError::UnknownRow)?
                        + 1
                }
            };
            if table.rows.len() + 1 > MAX_TABLE_ROWS {
                return Err(TableError::TableTooLarge);
            }
            table.order.insert(position, *row);
            table.rows.insert(*row, cells.clone());
        }
        TableOp::RemoveRow { row } => {
            let Some(position) = table.order.iter().position(|id| id == row) else {
                return Err(TableError::UnknownRow);
            };
            table.order.remove(position);
            table.rows.remove(row);
        }
    }
    Ok(())
}

/// Why a table-tier wire request refused. The table tier carries its own
/// rejection type so its independent revision space (finding F1) is
/// type-safe end to end; codes append at the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableRejectCode {
    /// Nothing is attached for this panel (no host attach, or the panel
    /// is not registered by the authenticated plugin), or the table was
    /// never published so it cannot be patched.
    Unattached,
    /// The surface is owned by a different verified plugin.
    OwnedByOther,
    /// The request is structurally invalid; refused without any state
    /// change.
    InvalidSurface,
    /// The request's revision is not usable against the stored head.
    NonMonotonicRevision,
}

/// Typed rejection of a table-tier request: the machine-readable code
/// plus the stored head when the refusal is revision-related.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TableReject {
    pub code: TableRejectCode,
    /// The stored head when the refusal is revision-related; `None`
    /// otherwise.
    pub current_revision: Option<TableRevision>,
}

impl From<crate::scene::SurfaceRejectCode> for TableRejectCode {
    fn from(code: crate::scene::SurfaceRejectCode) -> Self {
        match code {
            crate::scene::SurfaceRejectCode::Unattached => TableRejectCode::Unattached,
            crate::scene::SurfaceRejectCode::OwnedByOther => TableRejectCode::OwnedByOther,
            _ => TableRejectCode::InvalidSurface,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROW_A: StableId = StableId(10);
    const ROW_B: StableId = StableId(11);
    const COL_NAME: StableId = StableId(1);
    const COL_LEN: StableId = StableId(2);

    fn table() -> TableSurface {
        let mut t = TableSurface::new(
            TableRevision(1),
            vec![
                TableColumn {
                    id: COL_NAME,
                    label: "name".to_owned(),
                },
                TableColumn {
                    id: COL_LEN,
                    label: "len".to_owned(),
                },
            ],
        );
        t.apply_patch(&TablePatch {
            base_revision: TableRevision(1),
            ops: vec![TableOp::InsertRow {
                after: None,
                row: ROW_A,
                cells: vec![CellValue::Text("alpha".to_owned()), CellValue::Number(1.0)],
            }],
        })
        .expect("seed row");
        t
    }

    #[test]
    fn single_cell_update_changes_one_cell_and_bumps_one_revision() {
        let mut t = table();
        let before = t.clone();
        let revision = t
            .apply_patch(&TablePatch {
                base_revision: TableRevision(2),
                ops: vec![TableOp::SetCell {
                    row: ROW_A,
                    column: COL_LEN,
                    value: CellValue::Number(42.0),
                }],
            })
            .expect("cell update");
        assert_eq!(revision, TableRevision(3));
        assert_eq!(
            t.rows.get(&ROW_A).unwrap()[1],
            CellValue::Number(42.0),
            "exactly the addressed cell changed"
        );
        assert_eq!(
            t.rows.get(&ROW_A).unwrap()[0],
            CellValue::Text("alpha".to_owned())
        );
        assert_eq!(t.order, vec![ROW_A]);
        assert_ne!(t, before, "the revision moved");
    }

    #[test]
    fn atomic_patch_refuses_without_partial_mutation() {
        let mut t = table();
        let patch = TablePatch {
            base_revision: TableRevision(2),
            ops: vec![
                TableOp::SetCell {
                    row: ROW_A,
                    column: COL_LEN,
                    value: CellValue::Number(7.0),
                },
                TableOp::RemoveRow { row: ROW_B }, // unknown row: fails
            ],
        };
        assert_eq!(t.apply_patch(&patch), Err(TableError::UnknownRow));
        assert_eq!(
            t.rows.get(&ROW_A).unwrap()[1],
            CellValue::Number(1.0),
            "the valid op must not survive a failed patch"
        );
    }

    #[test]
    fn page_install_is_a_positioned_row_batch() {
        let mut t = table();
        let patch = TablePatch {
            base_revision: TableRevision(2),
            ops: vec![
                TableOp::InsertRow {
                    after: Some(ROW_A),
                    row: ROW_B,
                    cells: vec![CellValue::Text("beta".to_owned()), CellValue::Number(2.0)],
                },
                TableOp::InsertRow {
                    after: Some(ROW_B),
                    row: StableId(12),
                    cells: vec![CellValue::Text("gamma".to_owned()), CellValue::Bool(true)],
                },
            ],
        };
        t.apply_patch(&patch).expect("page");
        assert_eq!(t.order, vec![ROW_A, ROW_B, StableId(12)]);
    }

    #[test]
    fn stale_base_and_arity_and_finiteness_are_refused() {
        let mut t = table();
        let stale = TablePatch {
            base_revision: TableRevision(9),
            ops: vec![TableOp::RemoveRow { row: ROW_A }],
        };
        assert_eq!(t.apply_patch(&stale), Err(TableError::StaleBase));

        let arity = TablePatch {
            base_revision: TableRevision(2),
            ops: vec![TableOp::InsertRow {
                after: None,
                row: ROW_B,
                cells: vec![CellValue::Bool(false)],
            }],
        };
        assert_eq!(t.apply_patch(&arity), Err(TableError::ArityMismatch));

        let nonfinite = TablePatch {
            base_revision: TableRevision(2),
            ops: vec![TableOp::SetCell {
                row: ROW_A,
                column: COL_LEN,
                value: CellValue::Number(f64::NAN),
            }],
        };
        assert_eq!(t.apply_patch(&nonfinite), Err(TableError::NonFiniteNumber));
        assert_eq!(t.rows.get(&ROW_A).unwrap()[1], CellValue::Number(1.0));
    }
}
