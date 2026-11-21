//! The raster images of a DWF sheet, drawn together into one raster: an
//! underlay shows them as one picture over the box they lie in.

use std::ops::Range;
use std::sync::Arc;

use super::model::{Sheet, SheetImage};

/// The most pixels the images of a sheet are plotted with.
pub const PRINTED_IMAGE_PIXELS: f64 = (1u64 << 26) as f64;

/// The box the sheet's images lie in: min x, min y, max x, max y in sheet
/// units. `None` for a sheet without images.
pub fn images_box(sheet: &Sheet) -> Option<[f64; 4]> {
    sheet.images.iter().map(|image| image.rect).reduce(|a, b| {
        [
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[2].max(b[2]),
            a[3].max(b[3]),
        ]
    })
}

/// How fine the sheet's images are: the most pixels to the sheet unit any
/// of them has along x, and along y.
pub fn images_density(sheet: &Sheet) -> [f64; 2] {
    sheet.images.iter().fold([0.0; 2], |most, image| {
        let [x0, y0, x1, y1] = image.rect;
        if x1 > x0 && y1 > y0 {
            [
                most[0].max(image.width as f64 / (x1 - x0)),
                most[1].max(image.height as f64 / (y1 - y0)),
            ]
        } else {
            most
        }
    })
}

/// The sheet's images over `region` (min x, min y, max x, max y in sheet
/// units) as `width` by `height` pixels: RGBA with straight alpha, rows from
/// the top, clear where no image lies. An image is drawn over the ones the
/// sheet draws before it.
///
/// A pixel of the raster takes the image's pixel under its centre, or the
/// mean of the image's pixels it covers when it covers several. An image
/// alone, asked for over its own box at its own size, is its own pixels.
pub fn draw_images(sheet: &Sheet, region: [f64; 4], width: u32, height: u32) -> Arc<Vec<u8>> {
    if let [image] = sheet.images.as_slice() {
        if image.rect == region && (image.width, image.height) == (width, height) {
            return Arc::clone(&image.pixels);
        }
    }
    let (w, h) = (width as usize, height as usize);
    let mut raster = vec![0u8; w * h * 4];
    // The sheet units a pixel of the raster covers.
    let (dx, dy) = (
        (region[2] - region[0]) / w as f64,
        (region[3] - region[1]) / h as f64,
    );
    for image in &sheet.images {
        let [x0, y0, x1, y1] = image.rect;
        let (iw, ih) = (image.width as usize, image.height as usize);
        let whole = image.pixels.len() >= iw * ih * 4 && iw > 0 && ih > 0;
        if !(whole && x1 > x0 && y1 > y0 && dx > 0.0 && dy > 0.0) {
            continue;
        }
        // The image's pixels a pixel of the raster covers, along each axis.
        let (sx, sy) = (dx * iw as f64 / (x1 - x0), dy * ih as f64 / (y1 - y0));
        // Where the image starts in the raster, in the raster's pixels.
        let (left, top) = ((x0 - region[0]) / dx, (region[3] - y1) / dy);
        let columns = centred(left, (x1 - region[0]) / dx, w);
        for row in centred(top, (region[3] - y0) / dy, h) {
            let rows = covered((row as f64 + 0.5 - top) * sy, sy, ih);
            for column in columns.clone() {
                let across = covered((column as f64 + 0.5 - left) * sx, sx, iw);
                let pixel = mean(image, across, rows.clone());
                over(&mut raster[(row * w + column) * 4..][..4], pixel);
            }
        }
    }
    Arc::new(raster)
}

/// The cells, of `count`, whose centres lie from `low` up to `high`.
fn centred(low: f64, high: f64, count: usize) -> Range<usize> {
    let end = ((high - 0.5).ceil().max(0.0) as usize).min(count);
    ((low - 0.5).ceil().max(0.0) as usize).min(end)..end
}

/// The pixels, of `count`, that a cell `step` of them wide and centred at
/// `centre` covers: those whose centres it holds, or the one under its own
/// centre when it holds none.
fn covered(centre: f64, step: f64, count: usize) -> Range<usize> {
    let held = centred(centre - step / 2.0, centre + step / 2.0, count);
    if held.is_empty() {
        let under = (centre.floor().max(0.0) as usize).min(count - 1);
        under..under + 1
    } else {
        held
    }
}

/// The mean of the image's pixels in the columns `across` of the rows
/// `rows`, each weighed by its alpha.
fn mean(image: &SheetImage, across: Range<usize>, rows: Range<usize>) -> [u8; 4] {
    let at = |column: usize, row: usize| {
        let i = (row * image.width as usize + column) * 4;
        [
            image.pixels[i],
            image.pixels[i + 1],
            image.pixels[i + 2],
            image.pixels[i + 3],
        ]
    };
    if across.len() == 1 && rows.len() == 1 {
        return at(across.start, rows.start);
    }
    let count = (across.len() * rows.len()) as u64;
    let (mut sum, mut alpha) = ([0u64; 3], 0u64);
    for row in rows {
        for column in across.clone() {
            let pixel = at(column, row);
            alpha += pixel[3] as u64;
            for (total, channel) in sum.iter_mut().zip(pixel) {
                *total += channel as u64 * pixel[3] as u64;
            }
        }
    }
    if alpha == 0 {
        return [0; 4];
    }
    let colour = sum.map(|total| ((total + alpha / 2) / alpha) as u8);
    [
        colour[0],
        colour[1],
        colour[2],
        ((alpha + count / 2) / count) as u8,
    ]
}

/// Draws `pixel` over the pixel `under` (both straight alpha).
fn over(under: &mut [u8], pixel: [u8; 4]) {
    match (pixel[3] as u32, under[3] as u32) {
        (0, _) => {}
        (255, _) | (_, 0) => under.copy_from_slice(&pixel),
        (alpha, below) => {
            // What still shows of the pixel below, and the two together,
            // both in 255ths of a 255th.
            let rest = below * (255 - alpha);
            let total = alpha * 255 + rest;
            for (channel, colour) in under.iter_mut().zip(pixel).take(3) {
                *channel = ((colour as u32 * alpha * 255 + *channel as u32 * rest + total / 2)
                    / total) as u8;
            }
            under[3] = ((total + 127) / 255) as u8;
        }
    }
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
