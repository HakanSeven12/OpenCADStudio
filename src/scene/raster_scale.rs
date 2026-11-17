//! The view scale a set of rasters follows: PDF underlay pages and OLE
//! pictures are drawn near the size they show on screen and, zoomed in, again
//! for the part in view. They are made again once the view has moved to
//! another quarter-octave scale step, or out of the part drawn sharp, and
//! rested there briefly (not on every frame of a zoom).

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
