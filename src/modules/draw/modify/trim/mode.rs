use super::*;

// TrimCommand
// ══════════════════════════════════════════════════════════════════════════

// ── TRIM / EXTEND option machinery (#336) ─────────────────────────────────

/// Sub-mode of the TRIM / EXTEND commands (#336). `Pick` is the quick mode;
/// the others are entered through the option keywords.
pub(crate) enum TrimMode {
    Pick,
    /// Collecting cutting/boundary edges; Enter returns to Pick.
    SelectEdges,
    /// Collecting fence points; Enter runs the fence pass.
    Fence(FencePick),
    /// Quick mode: an empty click placed the fence's first point; the next
    /// point runs the fence pass.
    QuickFence(FencePick),
    /// Crossing: waiting for the first rectangle corner.
    CrossFirst,
    /// Crossing: first corner picked, waiting for the second.
    CrossSecond([f64; 2]),
    /// Erase mode: each pick deletes the object; Enter returns to Pick.
    Erase,
    /// mOde: choosing between Quick and Standard (TRIMEXTENDMODE).
    ChooseMode,
}

impl TrimMode {
    /// A fence or crossing-box point: the bare cursor, no object snap.
    pub(crate) fn picks_selection_path(&self) -> bool {
        matches!(
            self,
            TrimMode::Fence(_)
                | TrimMode::QuickFence(_)
                | TrimMode::CrossFirst
                | TrimMode::CrossSecond(_)
        )
    }
}

/// Quick mode: an empty click in Pick starts a two-point fence, so the next
/// click trims or extends everything the dashed line crosses. Standard mode
/// keeps the crossing box.

pub(crate) fn quick_fence_start(mode: &mut TrimMode, quick: bool, pt: DVec3) -> Option<CmdResult> {
    if !quick || !matches!(mode, TrimMode::Pick) {
        return None;
    }
    let mut pick = FencePick::fence();
    pick.push([pt.x, pt.y]);
    *mode = TrimMode::QuickFence(pick);
    Some(CmdResult::NeedPoint)
}

/// TRIMEXTENDMODE: 1 (Quick) starts TRIM and EXTEND ready to pick, every
/// object a cutting or boundary edge; 0 (Standard) starts them selecting
/// those edges first.
static QUICK_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn quick_mode() -> bool {
    QUICK_MODE.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_quick_mode(quick: bool) {
    QUICK_MODE.store(quick, std::sync::atomic::Ordering::Relaxed);
}

/// The mOde prompt, offering the current mode as the default.

pub(crate) fn mode_prompt(command: &str) -> String {
    let current = if quick_mode() { "Quick" } else { "Standard" };
    crate::tf!("{command}  Enter a trim mode option [Quick/Standard] <{current}>:").into_owned()
}

pub(crate) fn mode_options() -> Vec<crate::command::CmdOption> {
    use crate::command::CmdOption;
    vec![
        CmdOption::new("Quick", "Q"),
        CmdOption::new("Standard", "S"),
    ]
}

/// The mOde option and its answer, shared by TRIM and EXTEND. Returns the
/// step's result and whether the edges must be rebuilt, or `None` for input
/// that is not about the mode.

pub(crate) fn choose_mode(
    mode: &mut TrimMode,
    edge_set: &mut Vec<Handle>,
    text: &str,
) -> Option<(CmdResult, bool)> {
    let word = text.trim().to_uppercase();
    match (&*mode, word.as_str()) {
        (TrimMode::Pick, "O" | "MODE") => *mode = TrimMode::ChooseMode,
        (TrimMode::ChooseMode, "Q" | "QUICK") => {
            set_quick_mode(true);
            edge_set.clear();
            *mode = TrimMode::Pick;
            return Some((CmdResult::NeedPoint, true));
        }
        (TrimMode::ChooseMode, "S" | "STANDARD") => {
            set_quick_mode(false);
            edge_set.clear();
            *mode = TrimMode::SelectEdges;
        }
        (TrimMode::ChooseMode, _) => {
            let message = crate::t!("Invalid option keyword.").into_owned();
            return Some((CmdResult::ReportError(message), false));
        }
        _ => return None,
    }
    Some((CmdResult::NeedPoint, false))
}

/// The first step: picking in Quick mode or when edges were preselected,
/// selecting edges in Standard mode.

pub(crate) fn starting_mode(initial_edges: &[Handle]) -> TrimMode {
    if quick_mode() || !initial_edges.is_empty() {
        TrimMode::Pick
    } else {
        TrimMode::SelectEdges
    }
}

/// A TRIM/EXTEND working copy: the objects, their index and the boundaries
/// cut against.
pub(crate) struct Snapshot<'a> {
    pub(crate) all: &'a mut Vec<EntityType>,
    pub(crate) index: &'a mut ModifyEntityIndex,
    pub(crate) geos: &'a mut Vec<Geo>,
    pub(crate) edge_set: &'a mut Vec<Handle>,
    pub(crate) implied_edges: bool,
}

/// Apply one confirmed replacement to a command's entity snapshot: the old
/// entry goes and its pieces join under the handles the document assigned.
/// Edits stay pending until this callback, so a rejected edit (locked layer)
/// leaves the snapshot untouched. Only the replaced object's entries change;
/// rebuilding every boundary and the index per pick made each click cost the
/// whole drawing (#1978). Returns whether the boundaries need a full rebuild
/// (the last picked cutting edge went, so every object cuts again).

pub(crate) fn apply_pending_replacement(
    snapshot: Snapshot,
    pending: &mut Vec<(Handle, Vec<EntityType>)>,
    old: Handle,
    new_handles: &[Handle],
) -> bool {
    let Some(index) = pending.iter().position(|(handle, _)| *handle == old) else {
        return false;
    };
    let (_, pieces) = pending.swap_remove(index);
    snapshot.index.remove(snapshot.all, old);
    snapshot.geos.retain(|geo| geo.handle() != old);
    let had_edges = !snapshot.edge_set.is_empty();
    snapshot.edge_set.retain(|handle| *handle != old);
    // Pieces never join a picked edge set; with none, every object cuts.
    let all_cut = snapshot.edge_set.is_empty();
    for (mut piece, &handle) in pieces.into_iter().zip(new_handles) {
        piece.as_entity_mut().set_handle(handle);
        if all_cut {
            let mut geos = build_geos(std::slice::from_ref(&piece));
            if snapshot.implied_edges {
                imply_edge_geos(&mut geos);
            }
            snapshot.geos.extend(geos);
        }
        snapshot.index.push(snapshot.all, piece);
    }
    had_edges && all_cut
}
