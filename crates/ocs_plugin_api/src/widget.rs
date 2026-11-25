//! The widget surface data model (api v29, S05): semantic trees and
//! lists the host renders with NATIVE widgets.
//!
//! Ownership split mirrors the scene tier: the plugin owns the semantics
//! (what the rows mean); the host owns selection, expansion, scroll and
//! focus (host-local transient state), and renders the structure with its
//! own widgets. Plugin and host share nothing but this serializable
//! vocabulary.
//!
//! Layout is FLAT by design (finding F4): a surface holds every row in a
//! bounded map plus the display order of roots and of each loaded child
//! list. Children a plugin has not pushed yet are `Unloaded` — expanding
//! such a row asks the plugin for a keyed page, which installs
//! atomically; installing another page never replaces the whole surface.
//! Rows carry no nested subtrees, so pages stay small and the model has
//! no recursive traversal costs.
//!
//! Bounded everywhere: surface node budget, per-page row budget, and the
//! dialog text budget for labels. Deserialization of the whole surface is
//! likewise bounded before it can reach the store (same law as scenes).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

use crate::scene::StableId;

/// Maximum rows in one widget surface, bounding allocation and CPU per
/// surface (mirrors [`crate::scene::MAX_SCENE_NODES`]'s spirit).
pub const MAX_WIDGET_NODES: usize = 10_000;

/// Maximum rows in ONE page install: a page is a bounded window, not a
/// whole subtree. Plugins that need more rows install more pages.
pub const MAX_PAGE_ROWS: usize = 1_000;

/// A widget surface's own revision — INDEPENDENT of any scene revision
/// (finding F1: scene and widget surfaces advance separately).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct WidgetRevision(pub u64);

/// One row's child state: `Loaded` carries the ordered child ids actually
/// present in this surface; `Unloaded` marks children that exist but must
/// be paged in on expansion; `Leaf` cannot expand at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WidgetChildren {
    Leaf,
    Unloaded,
    Loaded(Vec<StableId>),
}

/// One row: its display label and its child state. Identity is the
/// plugin-assigned [`StableId`] (the map key, not repeated here).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WidgetRow {
    pub label: String,
    pub children: WidgetChildren,
    /// When set, the row is a button named by this action. Activating it
    /// sends `SurfaceEvent::NodeActivated` for the row; the plugin maps the
    /// row to its action. A button has no children.
    #[serde(default)]
    pub action: Option<String>,
}

/// A keyed page of rows (finding F4): installs under `parent`
/// (`None` = roots), optionally positioned after an existing sibling, and
/// optionally continues from `next` — the cursor the NEXT page must name
/// in `after`. Revision-checked and atomic like the scene patch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WidgetPage {
    pub base_revision: WidgetRevision,
    pub parent: Option<StableId>,
    /// Install position: after this existing sibling of the new rows
    /// (`None` = append at the end of the parent's list).
    pub after: Option<StableId>,
    /// The rows to install (bounded by [`MAX_PAGE_ROWS`]); ids must be new.
    pub rows: Vec<(StableId, WidgetRow)>,
    /// When set, more rows exist after the last installed sibling; the
    /// NEXT page names this cursor in `after`.
    pub next: Option<StableId>,
}

/// Why a widget page install refused. Codes append at the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetError {
    /// The page's `base_revision` is not the surface's current revision.
    StaleBase,
    /// The page carries more rows than [`MAX_PAGE_ROWS`].
    PageTooLarge,
    /// The surface would exceed [`MAX_WIDGET_NODES`].
    SurfaceTooLarge,
    /// The page names a parent that is not in the surface.
    UnknownParent,
    /// The page names a parent that cannot have children (`Leaf`).
    ParentCannotExpand,
    /// The page positions itself after a row that is not a current child
    /// of the parent.
    UnknownAfter,
    /// A structural walk named a row that is not in the surface.
    UnknownElement(StableId),
    /// A row id collides with an existing row (or appears twice).
    DuplicateRow,
    /// A row's label exceeds the text budget.
    LabelTooLong,
    /// A button row's action name exceeds the text budget.
    ActionTooLong,
    /// A button row (an `action` is set) also declared children — buttons
    /// are leaf rows.
    ActionWithChildren,
    /// The resulting surface would hold rows unreachable from the roots.
    OrphanRows,
}

impl core::fmt::Display for WidgetError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            WidgetError::StaleBase => write!(f, "page base is not the current surface revision"),
            WidgetError::PageTooLarge => write!(f, "page carries too many rows"),
            WidgetError::SurfaceTooLarge => write!(f, "surface node budget exceeded"),
            WidgetError::UnknownParent => write!(f, "page parent is not in the surface"),
            WidgetError::ParentCannotExpand => write!(f, "page parent cannot have children"),
            WidgetError::UnknownAfter => write!(f, "page after-cursor is not a current child"),
            WidgetError::UnknownElement(id) => {
                write!(f, "row id {} is not in the surface", id.0)
            }
            WidgetError::DuplicateRow => write!(f, "page row id collides with an existing row"),
            WidgetError::LabelTooLong => write!(f, "row label exceeds the text budget"),
            WidgetError::ActionTooLong => write!(f, "button action name exceeds the text budget"),
            WidgetError::ActionWithChildren => {
                write!(f, "button rows cannot also declare children")
            }
            WidgetError::OrphanRows => write!(f, "page would orphan rows from the roots"),
        }
    }
}

/// The stored widget surface: every row in a bounded map, display order
/// for the roots and for each loaded child list.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WidgetSurface {
    pub revision: WidgetRevision,
    pub rows: BTreeMap<StableId, WidgetRow>,
    pub roots: Vec<StableId>,
}

impl WidgetSurface {
    /// A fresh, empty surface at `revision`.
    pub fn new(revision: WidgetRevision) -> Self {
        Self {
            revision,
            rows: BTreeMap::new(),
            roots: Vec::new(),
        }
    }

    /// Validate the whole surface: bounded row count, bounded labels, and
    /// every row reachable from the roots exactly once (reachability
    /// forces acyclicity and no orphans).
    pub fn validate(&self) -> Result<(), WidgetError> {
        if self.rows.len() > MAX_WIDGET_NODES {
            return Err(WidgetError::SurfaceTooLarge);
        }
        for row in self.rows.values() {
            if row.label.len() > crate::scene::MAX_TEXT_BYTES {
                return Err(WidgetError::LabelTooLong);
            }
            if let Some(action) = &row.action {
                if action.len() > crate::scene::MAX_TEXT_BYTES {
                    return Err(WidgetError::ActionTooLong);
                }
                if !matches!(row.children, WidgetChildren::Leaf) {
                    return Err(WidgetError::ActionWithChildren);
                }
            }
        }
        let mut seen = HashSet::new();
        let mut stack: Vec<StableId> = self.roots.iter().copied().collect();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                return Err(WidgetError::OrphanRows);
            }
            let Some(row) = self.rows.get(&id) else {
                return Err(WidgetError::UnknownElement(id));
            };
            if let WidgetChildren::Loaded(children) = &row.children {
                for child in children {
                    stack.push(*child);
                }
            }
        }
        if seen.len() != self.rows.len() {
            return Err(WidgetError::OrphanRows);
        }
        Ok(())
    }

    /// Apply a page atomically: on any refusal the surface is
    /// byte-for-byte unchanged; on success the revision advances by one
    /// and the whole surface re-validates.
    pub fn apply_page(&mut self, page: &WidgetPage) -> Result<WidgetRevision, WidgetError> {
        if page.base_revision != self.revision {
            return Err(WidgetError::StaleBase);
        }
        if page.rows.len() > MAX_PAGE_ROWS {
            return Err(WidgetError::PageTooLarge);
        }
        // All-or-nothing: stage the candidate, swap only on full success.
        let mut candidate = self.clone();
        // Ids must be new and labels bounded (checked while inserting).
        let mut new_ids = Vec::with_capacity(page.rows.len());
        for (id, row) in &page.rows {
            if candidate.rows.contains_key(id) || new_ids.contains(id) {
                return Err(WidgetError::DuplicateRow);
            }
            if row.label.len() > crate::scene::MAX_TEXT_BYTES {
                return Err(WidgetError::LabelTooLong);
            }
            new_ids.push(*id);
        }
        // Attachment: parent must exist and be expandable; `after` must be
        // a current child of the parent.
        let child_list: &mut Vec<StableId> = match page.parent {
            None => &mut candidate.roots,
            Some(parent_id) => {
                let Some(parent) = candidate.rows.get_mut(&parent_id) else {
                    return Err(WidgetError::UnknownParent);
                };
                match &mut parent.children {
                    WidgetChildren::Leaf => return Err(WidgetError::ParentCannotExpand),
                    WidgetChildren::Unloaded => {
                        parent.children = WidgetChildren::Loaded(Vec::new());
                        match &mut parent.children {
                            WidgetChildren::Loaded(children) => children,
                            _ => unreachable!("just set Loaded"),
                        }
                    }
                    WidgetChildren::Loaded(children) => children,
                }
            }
        };
        let insert_at = match page.after {
            None => child_list.len(),
            Some(after_id) => {
                let position = child_list
                    .iter()
                    .position(|child| *child == after_id)
                    .ok_or(WidgetError::UnknownAfter)?;
                position + 1
            }
        };
        for (index, id) in new_ids.iter().enumerate() {
            child_list.insert(insert_at + index, *id);
        }
        for (id, row) in &page.rows {
            candidate.rows.insert(*id, row.clone());
        }
        // The page's own `next` cursor stays a promise: nothing to install
        // now, but the row list must still end consistently (the next page
        // positions AFTER the last installed row).
        let _ = page.next;
        candidate.revision = WidgetRevision(self.revision.0 + 1);
        candidate.validate()?;
        *self = candidate;
        Ok(self.revision)
    }
}

/// Why a widget-tier wire request refused. The widget tier carries its
/// own rejection type so its independent revision space (finding F1) is
/// type-safe end to end; codes append at the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetRejectCode {
    /// Nothing is attached for this panel (no host attach, or the panel
    /// is not registered by the authenticated plugin), or the surface
    /// was never published so it cannot be paged.
    Unattached,
    /// The surface is owned by a different verified plugin.
    OwnedByOther,
    /// The request is structurally invalid; refused without any state
    /// change.
    InvalidSurface,
    /// The request's revision is not usable against the stored head.
    NonMonotonicRevision,
}

/// Typed rejection of a widget-tier request: the machine-readable code
/// plus the stored head when the refusal is revision-related.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WidgetReject {
    pub code: WidgetRejectCode,
    /// The stored head when the refusal is revision-related; `None`
    /// otherwise.
    pub current_revision: Option<WidgetRevision>,
}

impl From<crate::scene::SurfaceRejectCode> for WidgetRejectCode {
    fn from(code: crate::scene::SurfaceRejectCode) -> Self {
        match code {
            crate::scene::SurfaceRejectCode::Unattached => WidgetRejectCode::Unattached,
            crate::scene::SurfaceRejectCode::OwnedByOther => WidgetRejectCode::OwnedByOther,
            _ => WidgetRejectCode::InvalidSurface,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARENT: StableId = StableId(1);
    const A: StableId = StableId(10);
    const B: StableId = StableId(11);

    fn row(label: &str, children: WidgetChildren) -> WidgetRow {
        WidgetRow {
            label: label.to_owned(),
            children,
            action: None,
        }
    }

    #[test]
    fn button_rows_are_leaf_only_and_bounded() {
        let mut surface = WidgetSurface::new(WidgetRevision(1));
        surface.rows.insert(
            StableId(1),
            WidgetRow {
                label: "Run".to_owned(),
                children: WidgetChildren::Leaf,
                action: Some("run".to_owned()),
            },
        );
        surface.roots = vec![StableId(1)];
        assert_eq!(surface.validate(), Ok(()));

        // A button WITH children is refused.
        surface.rows.insert(
            StableId(1),
            WidgetRow {
                label: "Run".to_owned(),
                children: WidgetChildren::Unloaded,
                action: Some("run".to_owned()),
            },
        );
        assert_eq!(surface.validate(), Err(WidgetError::ActionWithChildren));

        // An over-budget action name is refused.
        surface.rows.insert(
            StableId(1),
            WidgetRow {
                label: "Run".to_owned(),
                children: WidgetChildren::Leaf,
                action: Some("x".repeat(crate::scene::MAX_TEXT_BYTES + 1)),
            },
        );
        assert_eq!(surface.validate(), Err(WidgetError::ActionTooLong));
    }

    fn surface_with_parent() -> WidgetSurface {
        let mut surface = WidgetSurface::new(WidgetRevision(3));
        surface
            .rows
            .insert(PARENT, row("parent", WidgetChildren::Unloaded));
        surface.roots = vec![PARENT];
        surface
    }

    #[test]
    fn lazy_expansion_installs_a_page_under_an_unloaded_parent() {
        let mut surface = surface_with_parent();
        let page = WidgetPage {
            base_revision: WidgetRevision(3),
            parent: Some(PARENT),
            after: None,
            rows: vec![(A, row("alpha", WidgetChildren::Leaf))],
            next: None,
        };
        assert_eq!(surface.apply_page(&page), Ok(WidgetRevision(4)));
        assert_eq!(
            surface.rows.get(&PARENT).unwrap().children,
            WidgetChildren::Loaded(vec![A])
        );
    }

    #[test]
    fn after_cursor_positions_rows_and_next_continues() {
        let mut surface = surface_with_parent();
        let page = WidgetPage {
            base_revision: WidgetRevision(3),
            parent: Some(PARENT),
            after: None,
            rows: vec![(A, row("alpha", WidgetChildren::Leaf))],
            next: Some(A),
        };
        surface.apply_page(&page).expect("first page");
        let page = WidgetPage {
            base_revision: WidgetRevision(4),
            parent: Some(PARENT),
            after: Some(A),
            rows: vec![(B, row("beta", WidgetChildren::Leaf))],
            next: None,
        };
        surface.apply_page(&page).expect("second page");
        assert_eq!(
            surface.rows.get(&PARENT).unwrap().children,
            WidgetChildren::Loaded(vec![A, B])
        );
    }

    #[test]
    fn stale_base_refuses_with_no_mutation() {
        let mut surface = surface_with_parent();
        let page = WidgetPage {
            base_revision: WidgetRevision(2),
            parent: Some(PARENT),
            after: None,
            rows: vec![(A, row("alpha", WidgetChildren::Leaf))],
            next: None,
        };
        assert_eq!(surface.apply_page(&page), Err(WidgetError::StaleBase));
        assert!(matches!(
            surface.rows.get(&PARENT).unwrap().children,
            WidgetChildren::Unloaded
        ));
        assert_eq!(surface.revision, WidgetRevision(3));
    }

    #[test]
    fn atomic_patch_refuses_without_partial_install() {
        let mut surface = surface_with_parent();
        // Two valid rows and a duplicate id in the SAME page: nothing may
        // install.
        let page = WidgetPage {
            base_revision: WidgetRevision(3),
            parent: Some(PARENT),
            after: None,
            rows: vec![
                (A, row("alpha", WidgetChildren::Leaf)),
                (A, row("alpha-again", WidgetChildren::Leaf)),
            ],
            next: None,
        };
        assert_eq!(surface.apply_page(&page), Err(WidgetError::DuplicateRow));
        assert_eq!(surface.rows.len(), 1, "no row installed");
        assert!(matches!(
            surface.rows.get(&PARENT).unwrap().children,
            WidgetChildren::Unloaded
        ));
    }

    #[test]
    fn leaf_parent_and_unknown_parent_refuse() {
        let mut surface = WidgetSurface::new(WidgetRevision(1));
        surface
            .rows
            .insert(PARENT, row("leaf", WidgetChildren::Leaf));
        surface.roots = vec![PARENT];
        let page = WidgetPage {
            base_revision: WidgetRevision(1),
            parent: Some(PARENT),
            after: None,
            rows: vec![(A, row("alpha", WidgetChildren::Leaf))],
            next: None,
        };
        assert_eq!(
            surface.apply_page(&page),
            Err(WidgetError::ParentCannotExpand)
        );

        let page = WidgetPage {
            base_revision: WidgetRevision(1),
            parent: Some(StableId(999)),
            after: None,
            rows: vec![(A, row("alpha", WidgetChildren::Leaf))],
            next: None,
        };
        assert_eq!(surface.apply_page(&page), Err(WidgetError::UnknownParent));
    }

    #[test]
    fn unknown_after_cursor_refuses() {
        let mut surface = surface_with_parent();
        let page = WidgetPage {
            base_revision: WidgetRevision(3),
            parent: Some(PARENT),
            after: Some(B),
            rows: vec![(A, row("alpha", WidgetChildren::Leaf))],
            next: None,
        };
        assert_eq!(surface.apply_page(&page), Err(WidgetError::UnknownAfter));
    }

    #[test]
    fn page_row_budget_is_enforced() {
        let mut surface = surface_with_parent();
        let rows: Vec<_> = (0..=MAX_PAGE_ROWS)
            .map(|i| (StableId(100 + i as u64), row("r", WidgetChildren::Leaf)))
            .collect();
        let page = WidgetPage {
            base_revision: WidgetRevision(3),
            parent: Some(PARENT),
            after: None,
            rows,
            next: None,
        };
        assert_eq!(surface.apply_page(&page), Err(WidgetError::PageTooLarge));
    }

    #[test]
    fn orphan_rows_are_rejected_by_validate() {
        let mut surface = surface_with_parent();
        // A second root whose child list references a missing row.
        surface.roots = vec![PARENT, StableId(50)];
        surface.rows.insert(
            StableId(50),
            row("bad", WidgetChildren::Loaded(vec![StableId(51)])),
        );
        assert_eq!(
            surface.validate(),
            Err(WidgetError::UnknownElement(StableId(51)))
        );
    }
}
