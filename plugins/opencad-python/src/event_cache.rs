//! Bounded, tab-scoped notifications retained between fresh Python runs.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use ocs_plugin_api::host::HostNotification;

static EVENTS: Mutex<Option<HashMap<u64, TabEvents>>> = Mutex::new(None);
const CAPACITY_PER_TAB: usize = 256;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Event {
    Drawing {
        tab_id: u64,
        version: u64,
    },
    Selection {
        tab_id: u64,
        handles: Vec<u64>,
    },
    Command {
        tab_id: u64,
        command: Option<String>,
    },
    Overflow {
        tab_id: u64,
        dropped: u64,
    },
}

impl Event {
    fn tab_id(&self) -> u64 {
        match self {
            Event::Drawing { tab_id, .. }
            | Event::Selection { tab_id, .. }
            | Event::Command { tab_id, .. }
            | Event::Overflow { tab_id, .. } => *tab_id,
        }
    }
}

#[derive(Default)]
struct TabEvents {
    queue: VecDeque<Event>,
    dropped: u64,
}

pub(crate) fn observe(notification: &HostNotification) {
    if let HostNotification::DocumentTabClosed { tab_id } = notification {
        if let Ok(mut lock) = EVENTS.lock() {
            if let Some(tabs) = lock.as_mut() {
                tabs.remove(tab_id);
            }
        }
        return;
    }
    let event = match notification {
        #[cfg(feature = "experimental-host-model")]
        HostNotification::DrawingChanged { tab_id, epoch } => Event::Drawing {
            tab_id: *tab_id,
            version: *epoch,
        },
        HostNotification::SelectionChangedV4 { tab_id, handles } => Event::Selection {
            tab_id: *tab_id,
            handles: handles.iter().map(|h| h.value()).collect(),
        },
        #[cfg(feature = "experimental-host-model")]
        HostNotification::CommandStateChanged { tab_id, command } => Event::Command {
            tab_id: *tab_id,
            command: command.clone(),
        },
        _ => return,
    };
    if let Ok(mut lock) = EVENTS.lock() {
        let tab = lock
            .get_or_insert_with(HashMap::new)
            .entry(event.tab_id())
            .or_default();
        if tab.queue.len() == CAPACITY_PER_TAB {
            tab.queue.pop_front();
            tab.dropped += 1;
        }
        tab.queue.push_back(event);
    }
}

pub(crate) fn drain_for(tab_id: u64) -> Vec<Event> {
    let Ok(mut lock) = EVENTS.lock() else {
        return Vec::new();
    };
    let Some(tabs) = lock.as_mut() else {
        return Vec::new();
    };
    let Some(tab) = tabs.remove(&tab_id) else {
        return Vec::new();
    };
    let mut events = Vec::with_capacity(tab.queue.len() + usize::from(tab.dropped > 0));
    if tab.dropped > 0 {
        events.push(Event::Overflow {
            tab_id,
            dropped: tab.dropped,
        });
    }
    events.extend(tab.queue);
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polling_one_tab_preserves_other_tabs_events() {
        observe(&HostNotification::DrawingChanged {
            tab_id: 101,
            epoch: 1,
        });
        observe(&HostNotification::DrawingChanged {
            tab_id: 102,
            epoch: 2,
        });
        assert_eq!(
            drain_for(101),
            vec![Event::Drawing {
                tab_id: 101,
                version: 1
            }]
        );
        assert_eq!(
            drain_for(102),
            vec![Event::Drawing {
                tab_id: 102,
                version: 2
            }]
        );
    }

    #[test]
    fn noisy_tab_reports_its_own_overflow_without_evicting_another_tab() {
        observe(&HostNotification::DrawingChanged {
            tab_id: 201,
            epoch: 7,
        });
        for epoch in 0..=CAPACITY_PER_TAB as u64 {
            observe(&HostNotification::DrawingChanged { tab_id: 202, epoch });
        }
        assert_eq!(
            drain_for(201),
            vec![Event::Drawing {
                tab_id: 201,
                version: 7
            }]
        );
        let events = drain_for(202);
        assert_eq!(events.len(), CAPACITY_PER_TAB + 1);
        assert_eq!(
            events[0],
            Event::Overflow {
                tab_id: 202,
                dropped: 1
            }
        );
        assert_eq!(
            events[1],
            Event::Drawing {
                tab_id: 202,
                version: 1
            }
        );
    }

    #[test]
    fn closing_tab_discards_its_pending_events() {
        observe(&HostNotification::DrawingChanged {
            tab_id: 301,
            epoch: 3,
        });
        observe(&HostNotification::DocumentTabClosed { tab_id: 301 });
        assert!(drain_for(301).is_empty());
    }

    #[test]
    fn drawing_selection_and_command_events_keep_delivery_order() {
        use ocs_plugin_api::host::Handle;
        observe(&HostNotification::DrawingChanged {
            tab_id: 401,
            epoch: 9,
        });
        observe(&HostNotification::SelectionChangedV4 {
            tab_id: 401,
            handles: vec![Handle::new(17)],
        });
        observe(&HostNotification::CommandStateChanged {
            tab_id: 401,
            command: Some("LINE".into()),
        });
        observe(&HostNotification::CommandStateChanged {
            tab_id: 401,
            command: None,
        });
        assert_eq!(
            drain_for(401),
            vec![
                Event::Drawing {
                    tab_id: 401,
                    version: 9
                },
                Event::Selection {
                    tab_id: 401,
                    handles: vec![17]
                },
                Event::Command {
                    tab_id: 401,
                    command: Some("LINE".into())
                },
                Event::Command {
                    tab_id: 401,
                    command: None
                },
            ]
        );
    }
}
