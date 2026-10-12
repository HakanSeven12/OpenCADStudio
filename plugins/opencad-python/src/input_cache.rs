//! Results of host-driven point/entity picks, retained across Python runs.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use ocs_plugin_api::host::{CommandStep, Handle, InteractiveCommand};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static RESULTS: Mutex<Option<HashMap<u64, (u64, PickResult)>>> = Mutex::new(None);

#[derive(Clone)]
pub(crate) enum PickResult {
    Pending,
    Point([f64; 3]),
    Entity { handle: u64, point: [f64; 3] },
    Cancelled,
}

pub(crate) struct PickCommand {
    id: u64,
    tab_id: u64,
    prompt: String,
    entity: bool,
}

pub(crate) fn begin(tab_id: u64, prompt: String, entity: bool) -> (u64, PickCommand) {
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
}
