//! Touch screen gestures on the drawing. One finger needs nothing here: the
//! window moves the cursor with it and the drawing area takes its press and
//! lift as the left button. Two or more fingers become a gesture instead —
//! two drag to pan and pinch to zoom, three drag to orbit; a quick tap with
//! two opens the context menu, with three zooms to the extents, and three
//! swiped up show the isometric view.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use iced::touch;
use iced::{Point, Vector};

/// What a touch event asks the view to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TouchAction {
    /// A second finger came down: drop the press the first one started.
    Cancel,
    Pan(Vector),
    /// Scale the view by `factor` (above 1 zooms in) about `centre`, in
    /// window coordinates.
    Zoom { factor: f32, centre: Point },
    Orbit(Vector),
    Tap(usize),
    SwipeUp,
}

struct Gesture {
    started: Instant,
    most_fingers: usize,
    travel: f32,
    /// How far the fingers moved together, without the jumps of fingers
    /// landing or lifting.
    swept: Vector,
    centre: Point,
    spread: f32,
}

/// A tap is quicker and stiller than this.
const TAP_TIME: Duration = Duration::from_millis(300);
const TAP_TRAVEL: f32 = 12.0;
/// A swipe is a quick long sweep; a slower one is just an orbit.
const SWIPE_TRAVEL: f32 = 80.0;
const SWIPE_TIME: Duration = Duration::from_millis(500);
/// The lift ending a gesture reaches the drawing area as a left release
/// just before or after the gesture ends; this long after, it is let be.
const SETTLE: Duration = Duration::from_millis(150);

#[derive(Default)]
pub(crate) struct TouchGestures {
    fingers: BTreeMap<u64, Point>,
    gesture: Option<Gesture>,
    ended: Option<Instant>,
}

impl TouchGestures {
    /// A gesture is running (or just ended), so the drawing area's own
    /// pointer handling should keep out.
    pub(crate) fn active(&self) -> bool {
        self.gesture.is_some() || self.ended.is_some_and(|ended| ended.elapsed() < SETTLE)
    }

    /// Fingers on the screen.
    pub(crate) fn fingers(&self) -> usize {
        self.fingers.len()
    }

    fn centre_and_spread(&self) -> (Point, f32) {
        let count = self.fingers.len().max(1) as f32;
        let (x, y) = self
            .fingers
            .values()
            .fold((0.0, 0.0), |(x, y), point| (x + point.x, y + point.y));
        let centre = Point::new(x / count, y / count);
        let spread = self
            .fingers
            .values()
            .map(|point| point.distance(centre))
            .sum::<f32>()
            / count;
        (centre, spread)
    }

    pub(crate) fn event(&mut self, event: touch::Event) -> Vec<TouchAction> {
        match event {
            touch::Event::FingerPressed { id, position } => {
                self.fingers.insert(id.0, position);
                if self.fingers.len() < 2 {
                    return Vec::new();
                }
                let (centre, spread) = self.centre_and_spread();
                match &mut self.gesture {
                    Some(gesture) => {
                        gesture.most_fingers = gesture.most_fingers.max(self.fingers.len());
                        gesture.centre = centre;
                        gesture.spread = spread;
                        Vec::new()
                    }
                    None => {
                        self.gesture = Some(Gesture {
                            started: Instant::now(),
                            most_fingers: self.fingers.len(),
                            travel: 0.0,
                            swept: Vector::new(0.0, 0.0),
                            centre,
                            spread,
                        });
                        vec![TouchAction::Cancel]
                    }
                }
            }
            touch::Event::FingerMoved { id, position } => {
                if let Some(finger) = self.fingers.get_mut(&id.0) {
                    *finger = position;
                }
                let count = self.fingers.len();
                let (centre, spread) = self.centre_and_spread();
                let Some(gesture) = self.gesture.as_mut() else {
                    return Vec::new();
                };
                let delta = centre - gesture.centre;
                gesture.travel += (delta.x * delta.x + delta.y * delta.y).sqrt();
                gesture.swept = gesture.swept + delta;
                let mut actions = Vec::new();
                if count == 2 {
                    actions.push(TouchAction::Pan(delta));
                    if gesture.spread > 1.0 && spread > 1.0 {
                        gesture.travel += (spread - gesture.spread).abs();
                        actions.push(TouchAction::Zoom {
                            factor: spread / gesture.spread,
                            centre,
                        });
                    }
                } else if count >= 3 {
                    actions.push(TouchAction::Orbit(delta));
                }
                gesture.centre = centre;
                gesture.spread = spread;
                actions
            }
            touch::Event::FingerLifted { id, .. } | touch::Event::FingerLost { id, .. } => {
                self.fingers.remove(&id.0);
                if !self.fingers.is_empty() {
                    // The rest carry on from where they are, without a jump.
                    let (centre, spread) = self.centre_and_spread();
                    if let Some(gesture) = self.gesture.as_mut() {
                        gesture.centre = centre;
                        gesture.spread = spread;
                    }
                    return Vec::new();
                }
                let Some(gesture) = self.gesture.take() else {
                    return Vec::new();
                };
                self.ended = Some(Instant::now());
                let swept = gesture.swept;
                if gesture.started.elapsed() < TAP_TIME && gesture.travel < TAP_TRAVEL {
                    vec![TouchAction::Tap(gesture.most_fingers)]
                } else if gesture.most_fingers >= 3
                    && gesture.started.elapsed() < SWIPE_TIME
                    && swept.y < -SWIPE_TRAVEL
                    && swept.y.abs() > 2.0 * swept.x.abs()
                {
                    vec![TouchAction::SwipeUp]
                } else {
                    Vec::new()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(id: u64, x: f32, y: f32) -> touch::Event {
        touch::Event::FingerPressed {
            id: touch::Finger(id),
            position: Point::new(x, y),
        }
    }
    fn moved(id: u64, x: f32, y: f32) -> touch::Event {
        touch::Event::FingerMoved {
            id: touch::Finger(id),
            position: Point::new(x, y),
        }
    }
    fn lift(id: u64) -> touch::Event {
        touch::Event::FingerLifted {
            id: touch::Finger(id),
            position: Point::ORIGIN,
        }
    }

    #[test]
    fn two_fingers_pan_pinch_and_tap() {
        let mut touch = TouchGestures::default();
        assert!(touch.event(press(1, 100.0, 100.0)).is_empty());
        assert_eq!(touch.event(press(2, 200.0, 100.0)), vec![TouchAction::Cancel]);
        assert!(touch.active());
        // Both fingers slide right: a pan, no zoom.
        touch.event(moved(1, 110.0, 100.0));
        let actions = touch.event(moved(2, 210.0, 100.0));
        assert!(matches!(actions[0], TouchAction::Pan(v) if (v.x - 5.0).abs() < 1e-3));
        // They spread apart: zoom in.
        let actions = touch.event(moved(2, 260.0, 100.0));
        let zooms_in =
            |a: &TouchAction| matches!(a, TouchAction::Zoom { factor, .. } if *factor > 1.0);
        assert!(actions.iter().any(zooms_in));
        touch.event(lift(1));
        assert!(touch.event(lift(2)).is_empty());

        // A quick still touch with two fingers is a tap.
        touch.event(press(3, 100.0, 100.0));
        touch.event(press(4, 150.0, 100.0));
        touch.event(lift(3));
        assert_eq!(touch.event(lift(4)), vec![TouchAction::Tap(2)]);
    }

    #[test]
    fn three_fingers_orbit_and_swipe_up() {
        let mut touch = TouchGestures::default();
        touch.event(press(1, 100.0, 300.0));
        touch.event(press(2, 150.0, 300.0));
        touch.event(press(3, 200.0, 300.0));
        let mut orbited = false;
        for step in 1..=10 {
            let y = 300.0 - 15.0 * step as f32;
            for (id, x) in [(1, 100.0), (2, 150.0), (3, 200.0)] {
                orbited |= touch
                    .event(moved(id, x, y))
                    .iter()
                    .any(|a| matches!(a, TouchAction::Orbit(_)));
            }
        }
        assert!(orbited);
        touch.event(lift(1));
        touch.event(lift(2));
        assert_eq!(touch.event(lift(3)), vec![TouchAction::SwipeUp]);
    }
}
