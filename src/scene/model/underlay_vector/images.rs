//! The raster images of a DWF sheet, drawn together into one raster: an
//! underlay shows them as one picture over the box they lie in.

use std::sync::Arc;

use super::model::Sheet;

/// The box the sheet's images lie in: min x, min y, max x, max y in sheet
/// units. `None` for a sheet without images.
pub fn images_box(_sheet: &Sheet) -> Option<[f64; 4]> {
    None
}

/// How fine the sheet's images are: the most pixels to the sheet unit any
/// of them has along x, and along y.
pub fn images_density(_sheet: &Sheet) -> [f64; 2] {
    [0.0; 2]
}

/// The sheet's images over `region` (min x, min y, max x, max y in sheet
/// units) as `width` by `height` pixels: RGBA with straight alpha, rows from
/// the top, clear where no image lies. An image is drawn over the ones the
/// sheet draws before it.
pub fn draw_images(_sheet: &Sheet, _region: [f64; 4], width: u32, height: u32) -> Arc<Vec<u8>> {
    Arc::new(vec![0; width as usize * height as usize * 4])
}

#[cfg(test)]
mod tests {
    use super::super::model::SheetImage;
    use super::*;

    const RED: [u8; 4] = [255, 0, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const CLEAR: [u8; 4] = [0, 0, 0, 0];

    /// An image over `rect` whose pixels are `rows`, from the top.
    fn image(rect: [f64; 4], rows: &[&[[u8; 4]]]) -> SheetImage {
        SheetImage {
            rect,
            width: rows[0].len() as u32,
            height: rows.len() as u32,
            pixels: Arc::new(
                rows.iter()
                    .flat_map(|row| row.iter().flatten().copied())
                    .collect(),
            ),
            after: 0,
        }
    }

    fn sheet(images: Vec<SheetImage>) -> Sheet {
        Sheet {
            rect: [0.0, 0.0, 100.0, 100.0],
            images,
            ..Default::default()
        }
    }

    /// The pixels of a raster, one after the other.
    fn pixels(raster: &[u8]) -> Vec<[u8; 4]> {
        raster
            .chunks_exact(4)
            .map(|p| [p[0], p[1], p[2], p[3]])
            .collect()
    }

    #[test]
    fn the_box_holds_every_image() {
        let sheet = sheet(vec![
            image([10.0, 20.0, 30.0, 40.0], &[&[RED]]),
            image([50.0, 5.0, 60.0, 25.0], &[&[BLUE]]),
        ]);

        assert_eq!(images_box(&sheet), Some([10.0, 5.0, 60.0, 40.0]));
        assert_eq!(images_box(&Sheet::default()), None);
    }

    #[test]
    fn the_density_is_that_of_the_finest_image_along_each_axis() {
        // Four pixels across five units, one up ten; one across ten, two up
        // five.
        let sheet = sheet(vec![
            image([50.0, 0.0, 55.0, 10.0], &[&[BLUE, BLUE, BLUE, BLUE]]),
            image([0.0, 0.0, 10.0, 5.0], &[&[RED], &[RED]]),
        ]);

        assert_eq!(images_density(&sheet), [0.8, 0.4]);
    }

    #[test]
    fn an_image_alone_at_its_own_size_keeps_its_pixels() {
        let sheet = sheet(vec![image(
            [10.0, 20.0, 30.0, 40.0],
            &[&[RED, BLUE], &[BLUE, RED]],
        )]);

        let raster = draw_images(&sheet, [10.0, 20.0, 30.0, 40.0], 2, 2);

        assert!(
            Arc::ptr_eq(&raster, &sheet.images[0].pixels),
            "the image's own pixels, not a copy"
        );
    }

    #[test]
    fn images_are_drawn_where_they_lie_and_the_rest_is_clear() {
        let sheet = sheet(vec![
            image([0.0, 0.0, 10.0, 10.0], &[&[RED, RED], &[RED, RED]]),
            image([20.0, 0.0, 30.0, 10.0], &[&[BLUE, BLUE], &[BLUE, BLUE]]),
        ]);

        let raster = draw_images(&sheet, [0.0, 0.0, 30.0, 10.0], 6, 2);

        assert_eq!(
            pixels(&raster),
            [
                RED, RED, CLEAR, CLEAR, BLUE, BLUE, //
                RED, RED, CLEAR, CLEAR, BLUE, BLUE,
            ]
        );
    }

    #[test]
    fn rows_run_from_the_top() {
        // Red over blue: the upper half of the image is red.
        let sheet = sheet(vec![image([0.0, 0.0, 10.0, 20.0], &[&[RED], &[BLUE]])]);

        assert_eq!(
            pixels(&draw_images(&sheet, [0.0, 10.0, 10.0, 20.0], 1, 1)),
            [RED]
        );
        assert_eq!(
            pixels(&draw_images(&sheet, [0.0, 0.0, 10.0, 10.0], 1, 1)),
            [BLUE]
        );
    }

    #[test]
    fn a_later_image_is_drawn_over_an_earlier_one() {
        let half_blue = [0, 0, 255, 128];
        let sheet = sheet(vec![
            image([0.0, 0.0, 20.0, 10.0], &[&[RED, RED]]),
            image([10.0, 0.0, 30.0, 10.0], &[&[BLUE, half_blue]]),
            image([20.0, 0.0, 30.0, 10.0], &[&[CLEAR]]),
        ]);

        let raster = pixels(&draw_images(&sheet, [0.0, 0.0, 40.0, 10.0], 4, 1));

        assert_eq!(raster[0], RED);
        assert_eq!(raster[1], BLUE, "the opaque pixel hides the red one");
        assert_eq!(raster[2], half_blue, "a clear pixel changes nothing");
        assert_eq!(raster[3], CLEAR);
    }

    #[test]
    fn a_see_through_pixel_lets_the_image_under_it_show() {
        let sheet = sheet(vec![
            image([0.0, 0.0, 10.0, 10.0], &[&[RED]]),
            image([0.0, 0.0, 10.0, 10.0], &[&[[0, 0, 255, 128]]]),
        ]);

        let raster = pixels(&draw_images(&sheet, [0.0, 0.0, 10.0, 10.0], 1, 1));

        let [r, g, b, a] = raster[0];
        assert!(
            (126..=128).contains(&r) && g == 0 && (127..=129).contains(&b) && a == 255,
            "{:?}",
            raster[0]
        );
    }

    #[test]
    fn an_image_drawn_smaller_takes_the_mean_of_its_pixels() {
        let sheet = sheet(vec![image(
            [0.0, 0.0, 20.0, 10.0],
            &[&[RED, RED, BLUE, BLUE]],
        )]);

        let raster = pixels(&draw_images(&sheet, [0.0, 0.0, 20.0, 10.0], 1, 1));

        let [r, g, b, a] = raster[0];
        assert!(
            (127..=128).contains(&r) && g == 0 && (127..=128).contains(&b) && a == 255,
            "{:?}",
            raster[0]
        );
    }

    #[test]
    fn an_image_drawn_larger_repeats_its_pixels() {
        let sheet = sheet(vec![image([0.0, 0.0, 20.0, 10.0], &[&[RED, BLUE]])]);

        let raster = pixels(&draw_images(&sheet, [0.0, 0.0, 20.0, 10.0], 4, 2));

        assert_eq!(raster, [RED, RED, BLUE, BLUE, RED, RED, BLUE, BLUE]);
    }
}
