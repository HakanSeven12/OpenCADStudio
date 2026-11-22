//! Results of host-driven point/entity picks, retained across Python runs.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use ocs_plugin_api::host::{CommandStep, Handle, InteractiveCommand, PreviewWire};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static RESULTS: Mutex<Option<HashMap<u64, (u64, PickResult)>>> = Mutex::new(None);

#[derive(Clone)]
pub(crate) enum PickResult {
    Pending,
    Point([f64; 3]),
    Entity { handle: u64, point: [f64; 3] },
    /// A finished [`DragCommand`]: its points in order, the signed number of
    /// array copies (0 for the other previews) and the script's own text.
    Drag {
        points: Vec<[f64; 3]>,
        count: i64,
        state: String,
    },
    Cancelled,
}

pub(crate) struct PickCommand {
    id: u64,
    tab_id: u64,
    prompt: String,
    entity: bool,
}

fn reserve(tab_id: u64) -> u64 {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut lock) = RESULTS.lock() {
        let results = lock.get_or_insert_with(HashMap::new);
        if results.len() >= 1024 {
            if let Some(oldest) = results.keys().min().copied() {
                results.remove(&oldest);
            }
        }
        results.insert(id, (tab_id, PickResult::Pending));
    }
    id
}

pub(crate) fn begin(tab_id: u64, prompt: String, entity: bool) -> (u64, PickCommand) {
    let id = reserve(tab_id);
    (
        id,
        PickCommand {
            id,
            tab_id,
            prompt,
            entity,
        },
    )
}

pub(crate) fn take(tab_id: u64, id: u64) -> Option<PickResult> {
    let mut lock = RESULTS.lock().ok()?;
    let results = lock.as_mut()?;
    let (owner, result) = results.get(&id)?;
    if *owner != tab_id {
        return None;
    }
    if matches!(result, PickResult::Pending) {
        return Some(PickResult::Pending);
    }
    results.remove(&id).map(|(_, result)| result)
}

impl PickCommand {
    fn finish(&self, result: PickResult) {
        if let Ok(mut lock) = RESULTS.lock() {
            lock.get_or_insert_with(HashMap::new)
                .insert(self.id, (self.tab_id, result));
        }
    }
}

impl Drop for PickCommand {
    fn drop(&mut self) {
        if let Ok(mut lock) = RESULTS.lock() {
            if let Some((owner, PickResult::Pending)) = lock.as_mut().and_then(|r| r.get(&self.id))
            {
                if *owner == self.tab_id {
                    lock.as_mut()
                        .unwrap()
                        .insert(self.id, (self.tab_id, PickResult::Cancelled));
                }
            }
        }
    }
}

impl InteractiveCommand for PickCommand {
    fn prompt(&self) -> String {
        self.prompt.clone()
    }
    fn needs_object_pick(&self) -> bool {
        self.entity
    }
    fn on_point(&mut self, point: [f64; 3]) -> CommandStep {
        if self.entity {
            return CommandStep::NeedPoint;
        }
        self.finish(PickResult::Point(point));
        CommandStep::Done
    }
    fn on_object_pick(&mut self, handle: Handle, point: [f64; 3]) -> CommandStep {
        if !self.entity {
            return CommandStep::NeedPoint;
        }
        self.finish(PickResult::Entity {
            handle: handle.value(),
            point,
        });
        CommandStep::Done
    }
    fn on_enter(&mut self) -> CommandStep {
        self.finish(PickResult::Cancelled);
        CommandStep::Cancel
    }
}

/// What a [`DragCommand`] shows while the cursor moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DragPreview {
    /// A line from the last point to the cursor.
    Line,
    /// The wires moved by cursor - first point.
    Move,
    /// Base point, spacing vector, end point: the wires repeated along the
    /// vector as far as the cursor reaches, on either side of the base.
    Array,
}

impl DragPreview {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        match name {
            "line" => Some(Self::Line),
            "move" => Some(Self::Move),
            "array" => Some(Self::Array),
            _ => None,
        }
    }
}

pub(crate) const MAX_DRAG_PROMPTS: usize = 8;
pub(crate) const MAX_DRAG_WIRE_POINTS: usize = 40_000;
pub(crate) const MAX_DRAG_STATE_BYTES: usize = 64 * 1024;
/// Points one cursor move may send back; an array preview stops adding
/// copies there (the result still counts all of them).
const MAX_PREVIEW_POINTS: usize = 20_000;
/// Array copies a drag may ask for, whatever the zoom.
pub(crate) const MAX_ARRAY_COPIES: i64 = 10_000;

/// A multi-point pick with a live preview, started by `ocs.request_drag`.
/// The preview is computed here from the wires the script handed over, so no
/// Python runs while the cursor moves. After the last point the command ends
/// with `resume` (a `PY_RESUME` line) when the script asked to be run again.
pub(crate) struct DragCommand {
    id: u64,
    tab_id: u64,
    prompts: Vec<String>,
    wires: Vec<Vec<[f64; 3]>>,
    preview: DragPreview,
    state: String,
    resume: Option<String>,
    points: Vec<[f64; 3]>,
}

pub(crate) fn begin_drag(
    tab_id: u64,
    prompts: Vec<String>,
    wires: Vec<Vec<[f64; 3]>>,
    preview: DragPreview,
    state: String,
    resume_path: Option<&str>,
) -> (u64, DragCommand) {
    let id = reserve(tab_id);
    let command = DragCommand {
        id,
        tab_id,
        prompts,
        wires,
        preview,
        state,
        resume: resume_path.map(|path| format!("PY_RESUME {id} {path}")),
        points: Vec::new(),
    };
    (id, command)
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add_scaled(a: [f64; 3], v: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] + v[0] * k, a[1] + v[1] * k, a[2] + v[2] * k]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

impl DragCommand {
    /// How far the cursor at `at` reaches along the spacing vector, in whole
    /// and part vectors (negative behind the base), and the vector itself.
    fn array_reach(&self, at: [f64; 3]) -> Option<(f64, [f64; 3])> {
        let (base, vector_end) = (*self.points.first()?, *self.points.get(1)?);
        let vector = sub(vector_end, base);
        let length2 = dot(vector, vector);
        (length2 > 0.0).then(|| (dot(sub(at, base), vector) / length2, vector))
    }

    fn array_count(&self, at: [f64; 3]) -> i64 {
        self.array_reach(at)
            .map(|(reach, _)| (reach.trunc() as i64).clamp(-MAX_ARRAY_COPIES, MAX_ARRAY_COPIES))
            .unwrap_or(0)
    }

    fn moved_wires(&self, by: [f64; 3], out: &mut Vec<PreviewWire>, budget: &mut usize) -> bool {
        for wire in &self.wires {
            if wire.len() > *budget {
                return false;
            }
            *budget -= wire.len();
            out.push(PreviewWire::new(
                wire.iter().map(|p| add_scaled(*p, by, 1.0)).collect(),
            ));
        }
        true
    }

    fn finish(&self, result: PickResult) {
        if let Ok(mut lock) = RESULTS.lock() {
            lock.get_or_insert_with(HashMap::new)
                .insert(self.id, (self.tab_id, result));
        }
    }
}

impl Drop for DragCommand {
    fn drop(&mut self) {
        if let Ok(mut lock) = RESULTS.lock() {
            if let Some(results) = lock.as_mut() {
                if matches!(results.get(&self.id), Some((_, PickResult::Pending))) {
                    results.insert(self.id, (self.tab_id, PickResult::Cancelled));
                }
            }
        }
    }
}

impl InteractiveCommand for DragCommand {
    fn prompt(&self) -> String {
        self.prompts
            .get(self.points.len())
            .or(self.prompts.last())
            .cloned()
            .unwrap_or_default()
    }

    fn on_point(&mut self, point: [f64; 3]) -> CommandStep {
        // An array needs a real spacing vector: the base point again is no vector.
        if self.preview == DragPreview::Array && self.points.len() == 1 && self.points[0] == point {
            return CommandStep::NeedPoint;
        }
        self.points.push(point);
        if self.points.len() < self.prompts.len() {
            return CommandStep::NeedPoint;
        }
        let count = match self.preview {
            DragPreview::Array => self.array_count(point),
            _ => 0,
        };
        let result = PickResult::Drag {
            points: std::mem::take(&mut self.points),
            count,
            state: std::mem::take(&mut self.state),
        };
        self.finish(result);
        match self.resume.take() {
            Some(line) => CommandStep::Dispatch(line),
            None => CommandStep::Done,
        }
    }

    fn on_enter(&mut self) -> CommandStep {
        self.finish(PickResult::Cancelled);
        CommandStep::Cancel
    }

    fn on_cursor_move(&mut self, at: [f64; 3]) -> Vec<PreviewWire> {
        let Some(&first) = self.points.first() else {
            return Vec::new();
        };
        let last = *self.points.last().unwrap_or(&first);
        let mut out = Vec::new();
        let mut budget = MAX_PREVIEW_POINTS;
        match self.preview {
            DragPreview::Line => out.push(PreviewWire::line(last, at, None)),
            DragPreview::Move => {
                out.push(PreviewWire::line(first, at, None));
                self.moved_wires(sub(at, first), &mut out, &mut budget);
            }
            DragPreview::Array => match self.array_reach(at) {
                // The spacing vector is being picked: one copy shows the step.
                None => {
                    out.push(PreviewWire::line(first, at, None));
                    self.moved_wires(sub(at, first), &mut out, &mut budget);
                }
                Some((reach, vector)) => {
                    out.push(PreviewWire::line(first, add_scaled(first, vector, reach), None));
                    let count = self.array_count(at);
                    let side = if count < 0 { -1.0 } else { 1.0 };
                    for step in 1..=count.abs() {
                        let by = add_scaled([0.0; 3], vector, side * step as f64);
                        if !self.moved_wires(by, &mut out, &mut budget) {
                            break;
                        }
                    }
                }
            },
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_and_entity_picks_complete_tokens() {
        let (point_token, mut point) = begin(7, "Point".into(), false);
        assert!(matches!(take(7, point_token), Some(PickResult::Pending)));
        assert!(take(8, point_token).is_none());
        assert!(matches!(point.on_point([1.0, 2.0, 3.0]), CommandStep::Done));
        assert!(take(8, point_token).is_none());
        assert!(matches!(
            take(7, point_token),
            Some(PickResult::Point([1.0, 2.0, 3.0]))
        ));
        let (entity_token, mut entity) = begin(8, "Entity".into(), true);
        assert!(entity.needs_object_pick());
        assert!(matches!(
            entity.on_object_pick(Handle::new(17), [2.0, 0.0, 0.0]),
            CommandStep::Done
        ));
        assert!(matches!(
            take(8, entity_token),
            Some(PickResult::Entity { handle: 17, .. })
        ));
    }

    #[test]
    fn abandoned_pick_reports_cancellation() {
        let (token, command) = begin(7, "Point".into(), false);
        drop(command);
        assert!(take(8, token).is_none());
        assert!(matches!(take(7, token), Some(PickResult::Cancelled)));
    }

    #[test]
    fn enter_cancels_pick_without_returning_a_point() {
        let (token, mut command) = begin(9, "Pick".into(), false);
        assert!(matches!(command.on_enter(), CommandStep::Cancel));
        assert!(matches!(take(9, token), Some(PickResult::Cancelled)));
        drop(command);
        assert!(take(9, token).is_none());
    }

    fn square() -> Vec<Vec<[f64; 3]>> {
        vec![vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 0.0, 0.0]]]
    }

    fn copies(wires: &[PreviewWire]) -> usize {
        wires
            .iter()
            .filter(|wire| matches!(wire, PreviewWire::Polyline { points, .. } if points.len() == 4))
            .count()
    }

    fn starts_at(wires: &[PreviewWire], start: [f64; 3]) -> bool {
        wires.iter().any(|wire| {
            matches!(wire, PreviewWire::Polyline { points, .. } if points.len() == 4 && points[0] == start)
        })
    }

    #[test]
    fn array_drag_previews_one_copy_per_whole_vector_and_resumes_the_script() {
        let prompts = vec!["Base".to_owned(), "Vector".to_owned(), "End".to_owned()];
        let (token, mut drag) = begin_drag(
            3,
            prompts,
            square(),
            DragPreview::Array,
            "handles".to_owned(),
            Some("C:/scripts/array.py"),
        );
        assert_eq!(drag.prompt(), "Base");
        assert!(drag.on_cursor_move([5.0, 5.0, 0.0]).is_empty());
        assert!(matches!(drag.on_point([10.0, 0.0, 0.0]), CommandStep::NeedPoint));
        assert_eq!(drag.prompt(), "Vector");
        // the base point again is not a spacing vector
        assert!(matches!(drag.on_point([10.0, 0.0, 0.0]), CommandStep::NeedPoint));
        assert_eq!(drag.prompt(), "Vector");
        // while the vector is picked: the selection once, at the cursor
        assert_eq!(copies(&drag.on_cursor_move([12.0, 0.0, 0.0])), 1);
        assert!(matches!(drag.on_point([12.0, 0.0, 0.0]), CommandStep::NeedPoint));
        assert_eq!(drag.prompt(), "End");
        // 7.9 units along a 2-unit vector: 3 whole copies, however far off the axis
        assert_eq!(copies(&drag.on_cursor_move([17.9, 40.0, 0.0])), 3);
        assert_eq!(copies(&drag.on_cursor_move([11.9, 0.0, 0.0])), 0);
        // behind the base point the array runs the other way
        let behind = drag.on_cursor_move([5.5, 0.0, 0.0]);
        assert_eq!(copies(&behind), 2);
        assert!(starts_at(&behind, [-4.0, 0.0, 0.0]));
        assert!(matches!(take(3, token), Some(PickResult::Pending)));
        let CommandStep::Dispatch(line) = drag.on_point([5.5, 0.0, 0.0]) else {
            panic!("the last point runs the script again");
        };
        assert_eq!(line, format!("PY_RESUME {token} C:/scripts/array.py"));
        assert!(take(4, token).is_none(), "another tab cannot read it");
        let Some(PickResult::Drag { points, count, state }) = take(3, token) else {
            panic!("the drag result is stored");
        };
        assert_eq!(points.len(), 3);
        assert_eq!(count, -2);
        assert_eq!(state, "handles");
    }

    #[test]
    fn move_drag_follows_the_cursor_and_ends_without_a_script() {
        let prompts = vec!["From".to_owned(), "To".to_owned()];
        let (token, mut drag) =
            begin_drag(5, prompts, square(), DragPreview::Move, String::new(), None);
        assert!(matches!(drag.on_point([1.0, 1.0, 0.0]), CommandStep::NeedPoint));
        assert!(starts_at(&drag.on_cursor_move([4.0, 1.0, 0.0]), [3.0, 0.0, 0.0]));
        assert!(matches!(drag.on_point([4.0, 1.0, 0.0]), CommandStep::Done));
        assert!(matches!(
            take(5, token),
            Some(PickResult::Drag { count: 0, points, .. }) if points == vec![[1.0, 1.0, 0.0], [4.0, 1.0, 0.0]]
        ));
    }

    #[test]
    fn an_abandoned_or_refused_drag_reports_cancellation() {
        let prompts = vec!["A".to_owned(), "B".to_owned()];
        let (token, drag) = begin_drag(
            6,
            prompts.clone(),
            Vec::new(),
            DragPreview::Line,
            String::new(),
            None,
        );
        assert!(matches!(take(6, token), Some(PickResult::Pending)));
        drop(drag);
        assert!(matches!(take(6, token), Some(PickResult::Cancelled)));
        let (token, mut drag) =
            begin_drag(6, prompts, Vec::new(), DragPreview::Line, String::new(), Some("x.py"));
        assert!(matches!(drag.on_point([0.0; 3]), CommandStep::NeedPoint));
        assert_eq!(drag.on_cursor_move([1.0, 0.0, 0.0]).len(), 1);
        assert!(matches!(drag.on_enter(), CommandStep::Cancel));
        assert!(matches!(take(6, token), Some(PickResult::Cancelled)));
    }

    #[test]
    fn a_huge_array_preview_stops_at_the_point_budget_but_counts_every_copy() {
        let prompts = vec!["Base".to_owned(), "Vector".to_owned(), "End".to_owned()];
        let (token, mut drag) =
            begin_drag(7, prompts, square(), DragPreview::Array, String::new(), None);
        drag.on_point([0.0; 3]);
        drag.on_point([1.0, 0.0, 0.0]);
        let shown = copies(&drag.on_cursor_move([1.0e9, 0.0, 0.0]));
        assert_eq!(shown, MAX_PREVIEW_POINTS / 4);
        drag.on_point([1.0e9, 0.0, 0.0]);
        assert!(matches!(
            take(7, token),
            Some(PickResult::Drag { count: MAX_ARRAY_COPIES, .. })
        ));
    }
}
