//! The view scale a set of rasters follows: PDF underlay pages and OLE
//! pictures are drawn near the size they show on screen and, zoomed in, again
//! for the part in view. They are made again once the view has moved to
//! another quarter-octave scale step, or out of the part drawn sharp, and
//! rested there briefly (not on every frame of a zoom).

use std::cell::Cell;
use std::time::Duration;

/// How long the view rests on a new scale step before the rasters follow.
const SETTLE: Duration = Duration::from_millis(150);

/// The quarter-octave step of a view scale (world units per pixel). Rounded
/// up, so the scale the rasters are made for is never finer than the view's.
fn step_of(world_per_pixel: f64) -> i32 {
    (world_per_pixel.log2() * 4.0).ceil() as i32
}

#[derive(Debug, Default)]
pub(crate) struct RasterScale {
    /// The view scale (world units per pixel, on a quarter-octave step) the
    /// rasters are made for.
    pub(crate) world_per_pixel: Option<f64>,
    /// World rectangle the zoomed-in regions are drawn for (the plan view
    /// with a margin).
    pub(crate) view: Option<[f64; 4]>,
    /// Whether any raster drew a zoomed-in region.
    pub(crate) region_used: Cell<bool>,
    /// The layout the scale was taken in.
    layout: String,
    /// A new step and camera generation waiting for the view to rest.
    pending: Option<((i32, u64), iced::time::Instant)>,
}

impl RasterScale {
    fn current_step(&self, layout: &str) -> Option<i32> {
        if self.layout != layout {
            return None;
        }
        self.world_per_pixel
            .map(|w| (w.log2() * 4.0).round() as i32)
    }

    /// A zoomed-in region was drawn and the view `now` has left it.
    fn view_moved(&self, now: Option<[f64; 4]>) -> bool {
        if !self.region_used.get() {
            return false;
        }
        match (self.view, now) {
            (Some(have), Some(now)) => {
                now[0] < have[0] || now[1] < have[1] || now[2] > have[2] || now[3] > have[3]
            }
            _ => true,
        }
    }

    /// Whether rasters made for this scale are out of date for a view of
    /// `layout` at `world_per_pixel` showing `view`.
    pub(crate) fn stale(&self, world_per_pixel: f32, view: Option<[f64; 4]>, layout: &str) -> bool {
        self.current_step(layout) != Some(step_of(f64::from(world_per_pixel)))
            || self.view_moved(view)
    }

    /// Follow the view of `layout` at `world_per_pixel` showing `view`, once
    /// the camera (`camera_generation`) has rested on its step. True when
    /// the scale moved and the rasters are to be made again.
    pub(crate) fn settle(
        &mut self,
        world_per_pixel: f32,
        view: Option<[f64; 4]>,
        layout: &str,
        camera_generation: u64,
        now: iced::time::Instant,
    ) -> bool {
        let step = step_of(f64::from(world_per_pixel));
        if self.current_step(layout) == Some(step) && !self.view_moved(view) {
            self.pending = None;
            return false;
        }
        // Wait for the camera to rest, not only for the scale step to hold.
        let key = (step, camera_generation);
        match self.pending {
            Some((pending, since)) if pending == key => {
                if now.duration_since(since) < SETTLE {
                    return false;
                }
            }
            _ => {
                self.pending = Some((key, now));
                return false;
            }
        }
        self.pending = None;
        self.world_per_pixel = Some(2f64.powf(step as f64 / 4.0));
        self.view = view.map(|[x0, y0, x1, y1]| {
            let (mx, my) = ((x1 - x0) * 0.5, (y1 - y0) * 0.5);
            [x0 - mx, y0 - my, x1 + mx, y1 + my]
        });
        self.region_used.set(false);
        self.layout = layout.to_string();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: Option<[f64; 4]> = Some([0.0, 0.0, 100.0, 100.0]);

    /// The rasters follow a new scale only once the view has rested on it.
    #[test]
    fn the_scale_follows_the_view_once_it_rests() {
        let mut scale = RasterScale::default();
        let t = iced::time::Instant::now();
        assert!(scale.stale(0.1, VIEW, "Model"));
        assert!(!scale.settle(0.1, VIEW, "Model", 1, t), "a new step waits");
        assert!(!scale.settle(0.1, VIEW, "Model", 1, t + SETTLE / 2));
        assert!(
            scale.settle(0.1, VIEW, "Model", 1, t + SETTLE),
            "then it follows"
        );
        assert!(!scale.stale(0.1, VIEW, "Model"));
        // Its scale is the view's, a quarter-octave step coarser at most.
        let wpp = scale.world_per_pixel.unwrap();
        assert!((0.1..0.1 * 2f64.powf(0.25)).contains(&wpp), "{wpp}");
        // The same scale and view: nothing to make again.
        assert!(!scale.settle(0.1, VIEW, "Model", 2, t + SETTLE * 4));
    }

    /// Another layout at the same scale is another view: its rasters follow it.
    #[test]
    fn another_layout_is_another_view() {
        let mut scale = RasterScale::default();
        let t = iced::time::Instant::now();
        scale.settle(0.1, VIEW, "Model", 1, t);
        assert!(scale.settle(0.1, VIEW, "Model", 1, t + SETTLE));
        assert!(scale.stale(0.1, VIEW, "Sheet"));
        scale.settle(0.1, VIEW, "Sheet", 1, t + SETTLE);
        assert!(scale.settle(0.1, VIEW, "Sheet", 1, t + SETTLE * 2));
        assert!(!scale.stale(0.1, VIEW, "Sheet"));
    }
}
