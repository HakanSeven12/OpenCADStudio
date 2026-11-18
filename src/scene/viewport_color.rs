/// The inactive viewport is dimmed on screen. Plot output retains the source RGBA.
pub(super) fn projected_color([r, g, b, a]: [f32; 4], plot: bool) -> [f32; 4] {
    if plot {
        [r, g, b, a]
    } else {
        [r * 0.80, g * 0.80, b * 0.80, a * 0.85]
    }
}

#[cfg(test)]
mod tests {
    use super::projected_color;

    #[test]
    fn plot_preserves_truecolor_and_transparency() {
        for color in [[1.0, 1.0, 0.0, 1.0], [1.0, 223.0 / 255.0, 0.0, 0.4]] {
            assert_eq!(projected_color(color, true), color);
        }
    }

    #[test]
    fn screen_retains_inactive_viewport_dimming() {
        assert_eq!(
            projected_color([1.0, 0.5, 0.0, 1.0], false),
            [0.8, 0.4, 0.0, 0.85]
        );
    }
}
