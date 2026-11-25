//! Pure panel-surface event routing (api v29, S04): pointer semantics,
//! drag state machine, and event identity.
//!
//! Host-owned input (finding F2's contract): the host interprets raw
//! pointer input against its own scene projection and emits best-effort
//! semantic notifications to the owning plugin. Moves are COALESCED
//! (absolute positions carry all information; dropped intermediates lose
//! nothing); `DragSettled` and `DragCancelled` are authoritative and
//! never dropped. Exactly one settle per drag. Cancellation restores the
//! pre-drag origin losslessly.
//!
//! Click contract: press begins a POTENTIAL drag (no event yet); the
//! first move emits `DragStarted` (the pre-drag origin) plus a coalesced
//! `DragMoved`; release after moves emits `DragSettled`; release without
//! any move emits `ElementClicked`. A cancelled potential drag emits
//! nothing (silently dropped candidate) — a cancelled STARTED drag emits
//! `DragCancelled` with the origin.
//!
//! Nothing here touches the renderer or the wire: the caller translates
//! [`RoutedEvent`]s into `HostNotification::PanelSurfaceEvent` frames
//! with the router's sequence numbers (the explicit event identity).

use ocs_plugin_api::scene::{StableId, SurfaceEvent, Vec2};

/// One notification the host should send: the panel surface's identity,
/// the explicit event sequence, and the event itself.
#[derive(Debug, Clone, PartialEq)]
pub struct RoutedEvent {
    pub panel_id: String,
    pub sequence: u64,
    pub event: SurfaceEvent,
}

#[derive(Debug)]
struct DragState {
    element: StableId,
    /// The pre-drag origin (scene space) — the lossless-cancel restore.
    origin: Vec2,
    /// Whether `DragStarted` has been emitted for this drag.
    started: bool,
    /// Latest coalesced move position (scene space), if any.
    latest_move: Option<Vec2>,
}

/// The per-surface router. One instance owns ALL panel surfaces: event
/// sequences are per SURFACE (panel), and at most one drag is active at a
/// time (the host has one pointer).
#[derive(Debug, Default)]
pub struct PanelEventRouter {
    sequences: std::collections::BTreeMap<String, u64>,
    drag: Option<(String, DragState)>,
}

/// A pointer transition in scene space, already resolved to the element
/// under the pointer (or `None` when the pointer is over empty canvas).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointer {
    Press,
    Move,
    Release,
    /// Escape / focus loss / teardown: the authoritative cancel.
    Cancel,
}

impl PanelEventRouter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one pointer transition for `panel_id`. Returns the ordered
    /// notifications to send for this transition (often empty, one for
    /// clicks/settles/cancels, two when a drag starts with a move).
    pub fn on_pointer(
        &mut self,
        panel_id: &str,
        pointer: Pointer,
        element: Option<StableId>,
        pos: Vec2,
    ) -> Vec<RoutedEvent> {
        match pointer {
            Pointer::Press => self.on_press(panel_id, element, pos),
            Pointer::Move => self.on_move(panel_id, element, pos),
            Pointer::Release => self.on_release(panel_id, element, pos),
            Pointer::Cancel => self.on_cancel(panel_id),
        }
    }

    fn next_sequence(&mut self, panel_id: &str) -> u64 {
        let entry = self.sequences.entry(panel_id.to_owned()).or_insert(0);
        *entry += 1;
        *entry
    }

    fn notify(&mut self, panel_id: &str, event: SurfaceEvent) -> Vec<RoutedEvent> {
        let sequence = self.next_sequence(panel_id);
        vec![RoutedEvent {
            panel_id: panel_id.to_owned(),
            sequence,
            event,
        }]
    }

    fn on_press(
        &mut self,
        panel_id: &str,
        element: Option<StableId>,
        pos: Vec2,
    ) -> Vec<RoutedEvent> {
        // One pointer: a press anywhere settles (or silently drops) the
        // in-flight drag first — the host has a single pointer, and the
        // settle is authoritative (never lost, never partial).
        let flushed = self.settle_active_drag();
        self.drag = element.map(|element| {
            (
                panel_id.to_owned(),
                DragState {
                    element,
                    origin: pos,
                    started: false,
                    latest_move: None,
                },
            )
        });
        flushed
    }

    fn on_move(
        &mut self,
        panel_id: &str,
        element: Option<StableId>,
        pos: Vec2,
    ) -> Vec<RoutedEvent> {
        // The dragged element stays the drag's subject: moves over other
        // elements do not re-target the drag.
        let _ = element;
        let Some((drag_panel, drag)) = self.drag.as_mut() else {
            return Vec::new();
        };
        if drag_panel != panel_id {
            return Vec::new();
        }
        let subject = drag.element;
        let origin = drag.origin;
        let first_move = !drag.started;
        drag.started = true;
        // Coalesce: the pending move is REPLACED by the latest (absolute
        // positions lose nothing).
        drag.latest_move = Some(pos);
        drop(drag);
        let mut out = Vec::new();
        if first_move {
            out.extend(self.notify(
                panel_id,
                SurfaceEvent::DragStarted {
                    element: subject,
                    origin,
                },
            ));
        }
        out
    }

    fn on_release(
        &mut self,
        panel_id: &str,
        element: Option<StableId>,
        pos: Vec2,
    ) -> Vec<RoutedEvent> {
        let Some((drag_panel, mut drag)) = self.drag.take() else {
            return Vec::new();
        };
        if drag_panel != panel_id {
            self.drag = Some((drag_panel, drag));
            return Vec::new();
        }
        if !drag.started {
            // Never moved: this was a CLICK on the pressed element (the
            // release target wins; it is the element under the pointer).
            let target = element.unwrap_or(drag.element);
            return self.notify(
                panel_id,
                SurfaceEvent::ElementClicked {
                    element: target,
                    pos,
                },
            );
        }
        // The authoritative settle supersedes any coalesced move.
        let _ = drag.latest_move.take();
        self.notify(
            panel_id,
            SurfaceEvent::DragSettled {
                element: drag.element,
                pos,
            },
        )
    }

    fn on_cancel(&mut self, panel_id: &str) -> Vec<RoutedEvent> {
        let Some((drag_panel, drag)) = self.drag.take() else {
            return Vec::new();
        };
        if drag_panel != panel_id {
            self.drag = Some((drag_panel, drag));
            return Vec::new();
        }
        if !drag.started {
            // A cancelled potential drag emitted nothing: silently dropped.
            return Vec::new();
        }
        // Lossless cancellation: the origin rides the event.
        self.notify(
            panel_id,
            SurfaceEvent::DragCancelled {
                element: drag.element,
                origin: drag.origin,
            },
        )
    }

    /// Widget-tier notifications reuse the per-surface sequence for their
    /// explicit identity: activation gestures and lazy-expansion requests
    /// (expanding an `Unloaded` row asks the plugin for a page).
    pub fn on_node_activated(&mut self, panel_id: &str, node: StableId) -> RoutedEvent {
        let sequence = self.next_sequence(panel_id);
        RoutedEvent {
            panel_id: panel_id.to_owned(),
            sequence,
            event: SurfaceEvent::NodeActivated { node },
        }
    }

    /// A committed cell edit on a table surface (api v29, S07): the
    /// explicit event identity is the per-surface sequence, as
    /// everywhere.
    pub fn on_cell_edited(
        &mut self,
        panel_id: &str,
        row: StableId,
        column: StableId,
        value: ocs_plugin_api::tables::CellValue,
    ) -> RoutedEvent {
        let sequence = self.next_sequence(panel_id);
        RoutedEvent {
            panel_id: panel_id.to_owned(),
            sequence,
            event: SurfaceEvent::CellEdited { row, column, value },
        }
    }

    /// The one expansion case that is not host-local: expanding an
    /// `Unloaded` row asks the plugin (the data source) for a page.
    pub fn on_node_expanded(&mut self, panel_id: &str, node: StableId) -> RoutedEvent {
        let sequence = self.next_sequence(panel_id);
        RoutedEvent {
            panel_id: panel_id.to_owned(),
            sequence,
            event: SurfaceEvent::NodeExpanded { node },
        }
    }

    /// Automation-triggered activation of a SCENE element (api v29, S08):
    /// the agent's `plugin_panel_action` routes here, mirroring
    /// `on_node_activated`. The explicit identity is the per-surface
    /// sequence, as everywhere; `pos` is pointer-derived payload, NOT
    /// identity — the automation path anchors it at the element's stored
    /// origin (no pointer exists), so equivalence with a GUI click is
    /// asserted on (panel, sequence, kind, element), never on pos.
    pub fn on_element_clicked(&mut self, panel_id: &str, element: StableId) -> RoutedEvent {
        let sequence = self.next_sequence(panel_id);
        RoutedEvent {
            panel_id: panel_id.to_owned(),
            sequence,
            event: SurfaceEvent::ElementClicked {
                element,
                pos: ocs_plugin_api::scene::Vec2([0.0, 0.0]),
            },
        }
    }

    /// Settle the active drag (whatever panel owns it): a `DragSettled`
    /// at the latest known position when started, or a silent drop when
    /// the drag never began. The single-pointer law: a new press steals
    /// the pointer, and what the preview showed is what settles.
    fn settle_active_drag(&mut self) -> Vec<RoutedEvent> {
        let Some((drag_panel, drag)) = self.drag.take() else {
            return Vec::new();
        };
        if !drag.started {
            return Vec::new();
        }
        let pos = drag.latest_move.unwrap_or(drag.origin);
        self.notify(
            &drag_panel,
            SurfaceEvent::DragSettled {
                element: drag.element,
                pos,
            },
        )
    }

    /// Whether a drag is in flight for `panel_id` (the canvas paints the
    /// host-local preview when true).
    pub fn drag_preview(&self, panel_id: &str) -> Option<(StableId, Vec2, Vec2)> {
        let (drag_panel, drag) = self.drag.as_ref()?;
        if drag_panel != panel_id || !drag.started {
            return None;
        }
        Some((
            drag.element,
            drag.origin,
            drag.latest_move.unwrap_or(drag.origin),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANEL: &str = "acme:graph";
    const EL: StableId = StableId(7);

    fn at(x: f64, y: f64) -> Vec2 {
        Vec2([x, y])
    }

    #[test]
    fn click_without_move_emits_exactly_one_element_clicked() {
        let mut router = PanelEventRouter::new();
        assert!(
            router
                .on_pointer(PANEL, Pointer::Press, Some(EL), at(1.0, 1.0))
                .is_empty()
        );
        let out = router.on_pointer(PANEL, Pointer::Release, Some(EL), at(1.0, 1.0));
        assert_eq!(
            out,
            vec![RoutedEvent {
                panel_id: PANEL.to_owned(),
                sequence: 1,
                event: SurfaceEvent::ElementClicked {
                    element: EL,
                    pos: at(1.0, 1.0)
                }
            }]
        );
    }

    #[test]
    fn drag_emits_started_then_coalesced_moves_then_one_settle() {
        let mut router = PanelEventRouter::new();
        router.on_pointer(PANEL, Pointer::Press, Some(EL), at(0.0, 0.0));
        // Press with no events yet.
        let started_flush = router.on_pointer(PANEL, Pointer::Move, Some(EL), at(1.0, 1.0));
        assert_eq!(started_flush.len(), 1, "DragStarted rides the first move");
        assert!(matches!(
            &started_flush[0].event,
            SurfaceEvent::DragStarted { origin, .. } if *origin == at(0.0, 0.0)
        ));
        // Moves COALESCE: three moves flush only the latest.
        let mut flush = router.on_pointer(PANEL, Pointer::Move, Some(EL), at(2.0, 2.0));
        assert!(flush.is_empty(), "move without started-flush coalesces");
        flush.extend(router.on_pointer(PANEL, Pointer::Move, Some(EL), at(3.0, 3.0)));
        flush.extend(router.on_pointer(PANEL, Pointer::Move, Some(EL), at(4.0, 4.0)));
        assert!(flush.is_empty(), "moves stay coalesced until flushed");
        // The settle supersedes the pending coalesced move and is the
        // single authoritative end.
        let out = router.on_pointer(PANEL, Pointer::Release, Some(EL), at(5.0, 5.0));
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].sequence,
            started_flush[0].sequence + 1,
            "sequences are per-surface monotonic"
        );
        assert!(matches!(
            &out[0].event,
            SurfaceEvent::DragSettled { element, pos }
                if *element == EL && *pos == at(5.0, 5.0)
        ));
    }

    #[test]
    fn cancel_after_started_drag_restores_the_origin_losslessly() {
        let mut router = PanelEventRouter::new();
        router.on_pointer(PANEL, Pointer::Press, Some(EL), at(9.0, 9.0));
        router.on_pointer(PANEL, Pointer::Move, Some(EL), at(10.0, 10.0));
        router.on_pointer(PANEL, Pointer::Move, Some(EL), at(50.0, 50.0));
        let out = router.on_pointer(PANEL, Pointer::Cancel, Some(EL), at(50.0, 50.0));
        assert_eq!(out.len(), 1);
        assert!(matches!(
            &out[0].event,
            SurfaceEvent::DragCancelled { element, origin }
                if *element == EL && *origin == at(9.0, 9.0)
        ));
        // After the cancel the router is idle: moves emit nothing.
        assert!(
            router
                .on_pointer(PANEL, Pointer::Move, Some(EL), at(60.0, 60.0))
                .is_empty()
        );
    }

    #[test]
    fn cancelled_potential_drag_emits_nothing() {
        let mut router = PanelEventRouter::new();
        router.on_pointer(PANEL, Pointer::Press, Some(EL), at(1.0, 1.0));
        assert!(
            router
                .on_pointer(PANEL, Pointer::Cancel, Some(EL), at(1.0, 1.0))
                .is_empty()
        );
        // A fresh press after the cancel: the next click sequence numbers
        // start at 1 (nothing burned by the dropped candidate).
        router.on_pointer(PANEL, Pointer::Press, Some(EL), at(1.0, 1.0));
        let out = router
            .on_pointer(PANEL, Pointer::Release, Some(EL), at(1.0, 1.0))
            .pop()
            .expect("release after re-press");
        assert_eq!(out.sequence, 1);
    }

    #[test]
    fn sequences_are_explicit_identity_across_identical_clicks() {
        let mut router = PanelEventRouter::new();
        for _ in 0..2 {
            router.on_pointer(PANEL, Pointer::Press, Some(EL), at(1.0, 1.0));
            router.on_pointer(PANEL, Pointer::Release, Some(EL), at(1.0, 1.0));
        }
        // Both clicks observed: two events, DISTINCT sequences, identical
        // payloads — equality is not event identity.
        assert_eq!(router.sequences.get(PANEL), Some(&2));
    }

    #[test]
    fn press_during_a_drag_settles_the_old_drag_first() {
        let mut router = PanelEventRouter::new();
        router.on_pointer(PANEL, Pointer::Press, Some(EL), at(0.0, 0.0));
        router.on_pointer(PANEL, Pointer::Move, Some(EL), at(1.0, 1.0));
        // A second press (e.g. another panel's canvas or a second button):
        // the in-flight drag settles authoritatively before anything else.
        let out = router.on_pointer("other:panel", Pointer::Press, Some(EL), at(0.0, 0.0));
        assert_eq!(out.len(), 1);
        assert!(matches!(
            &out[0].event,
            SurfaceEvent::DragSettled { element, pos }
                if *element == EL && *pos == at(1.0, 1.0)
        ));
    }
}
