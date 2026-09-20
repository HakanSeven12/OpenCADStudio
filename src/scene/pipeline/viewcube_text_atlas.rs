use cosmic_text::{
    Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache, Weight, Wrap,
};

pub(super) const ATLAS_WIDTH: u32 = 512;
pub(super) const ATLAS_HEIGHT: u32 = 256;
pub(super) const FACE_TILE_COUNT: usize = 6;
pub(super) const CARDINAL_TILE_START: usize = FACE_TILE_COUNT;
pub(super) const TILE_COUNT: usize = FACE_TILE_COUNT + 4;

const FONT_PX: f32 = 32.0;
const LINE_PX: f32 = 40.0;
const TILE_PADDING: i32 = 3;
const TILE_GAP: u32 = 3;
const EMBOLDEN_RADIUS: i32 = 1;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct AtlasTile {
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub aspect: f32,
}

pub(super) struct LabelAtlas {
    pub pixels: Vec<u8>,
    pub tiles: [AtlasTile; TILE_COUNT],
}

struct LabelBitmap {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

pub(super) fn empty_label_atlas() -> LabelAtlas {
    LabelAtlas {
        pixels: vec![0_u8; (ATLAS_WIDTH * ATLAS_HEIGHT) as usize],
        tiles: [AtlasTile::default(); TILE_COUNT],
    }
}

pub(super) fn build_label_atlas(labels: &[String; FACE_TILE_COUNT]) -> Option<LabelAtlas> {
    let (mut font_system, family, weight) = font_system(labels)?;
    let mut swash_cache = SwashCache::new();
    let mut texts: Vec<&str> = labels.iter().map(String::as_str).collect();
    texts.extend(["N", "E", "S", "W"]);
    let bitmaps: Vec<LabelBitmap> = texts
        .into_iter()
        .map(|text| rasterize_label(&mut font_system, &mut swash_cache, text, family, weight))
        .collect();

    Some(pack_bitmaps(&bitmaps))
}

fn rasterize_label(
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    text: &str,
    family: Option<&str>,
    weight: Weight,
) -> LabelBitmap {
    let attrs = match family {
        Some(family) => Attrs::new().family(Family::Name(family)).weight(weight),
        None => Attrs::new().family(Family::SansSerif).weight(weight),
    };
    let mut buffer = Buffer::new(font_system, Metrics::new(FONT_PX, LINE_PX));
    buffer.set_size(font_system, None, None);
    buffer.set_wrap(font_system, Wrap::None);
    buffer.set_text(font_system, text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(font_system, false);

    let logical_width = buffer
        .layout_runs()
        .map(|run| run.line_w)
        .fold(0.0_f32, f32::max)
        .ceil() as i32;
    let mut ink_min = [0_i32, 0_i32];
    let mut ink_max = [logical_width.max(1), LINE_PX.ceil() as i32];
    buffer.draw(
        font_system,
        swash_cache,
        Color::rgb(255, 255, 255),
        |x, y, width, height, color| {
            if color.a() == 0 {
                return;
            }
            ink_min[0] = ink_min[0].min(x);
            ink_min[1] = ink_min[1].min(y);
            ink_max[0] = ink_max[0].max(x + width as i32);
            ink_max[1] = ink_max[1].max(y + height as i32);
        },
    );

    let origin_x = ink_min[0] - TILE_PADDING;
    let origin_y = ink_min[1] - TILE_PADDING;
    let width = (ink_max[0] - ink_min[0] + TILE_PADDING * 2).max(1) as u32;
    let height = (ink_max[1] - ink_min[1] + TILE_PADDING * 2).max(1) as u32;
    let mut pixels = vec![0_u8; (width * height) as usize];
    buffer.draw(
        font_system,
        swash_cache,
        Color::rgb(255, 255, 255),
        |x, y, pixel_width, pixel_height, color| {
            let alpha = color.a();
            if alpha == 0 {
                return;
            }
            for py in 0..pixel_height as i32 {
                for px in 0..pixel_width as i32 {
                    let dst_x = x + px - origin_x;
                    let dst_y = y + py - origin_y;
                    if dst_x < 0 || dst_y < 0 || dst_x >= width as i32 || dst_y >= height as i32 {
                        continue;
                    }
                    let index = dst_y as usize * width as usize + dst_x as usize;
                    let current = pixels[index] as u16;
                    let incoming = alpha as u16;
                    pixels[index] = (incoming + current * (255 - incoming) / 255) as u8;
                }
            }
        },
    );
    let pixels = embolden_alpha(&pixels, width, height);

    LabelBitmap {
        width,
        height,
        pixels,
    }
}

fn embolden_alpha(source: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut output = source.to_vec();
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let alpha = source[y as usize * width as usize + x as usize];
            if alpha == 0 {
                continue;
            }
            for offset_y in -EMBOLDEN_RADIUS..=EMBOLDEN_RADIUS {
                for offset_x in -EMBOLDEN_RADIUS..=EMBOLDEN_RADIUS {
                    let target_x = x + offset_x;
                    let target_y = y + offset_y;
                    if target_x < 0
                        || target_y < 0
                        || target_x >= width as i32
                        || target_y >= height as i32
                    {
                        continue;
                    }
                    let index = target_y as usize * width as usize + target_x as usize;
                    output[index] = output[index].max(alpha);
                }
            }
        }
    }
    output
}

#[cfg(not(target_arch = "wasm32"))]
fn font_system(
    _labels: &[String; FACE_TILE_COUNT],
) -> Option<(FontSystem, Option<&'static str>, Weight)> {
    Some((FontSystem::new(), None, Weight::SEMIBOLD))
}

#[cfg(target_arch = "wasm32")]
fn font_system(
    _labels: &[String; FACE_TILE_COUNT],
) -> Option<(FontSystem, Option<&'static str>, Weight)> {
    let script = crate::scene::text::web_font::primary_script();
    let bytes = crate::scene::text::web_font::loaded(script)?;
    let mut database = fontdb::Database::new();
    database.load_font_data((*bytes).clone());
    database.set_sans_serif_family(script.family());
    Some((
        FontSystem::new_with_locale_and_db("en-US".to_owned(), database),
        Some(script.family()),
        Weight::NORMAL,
    ))
}

fn pack_bitmaps(bitmaps: &[LabelBitmap]) -> LabelAtlas {
    let mut pixels = vec![0_u8; (ATLAS_WIDTH * ATLAS_HEIGHT) as usize];
    let mut tiles = [AtlasTile::default(); TILE_COUNT];
    let mut cursor_x = TILE_GAP;
    let mut cursor_y = TILE_GAP;
    let mut row_height = 0_u32;

    for (index, bitmap) in bitmaps.iter().enumerate().take(TILE_COUNT) {
        if cursor_x + bitmap.width + TILE_GAP > ATLAS_WIDTH {
            cursor_x = TILE_GAP;
            cursor_y += row_height + TILE_GAP;
            row_height = 0;
        }
        if cursor_y + bitmap.height + TILE_GAP > ATLAS_HEIGHT {
            break;
        }

        for row in 0..bitmap.height as usize {
            let src = row * bitmap.width as usize;
            let dst = (cursor_y as usize + row) * ATLAS_WIDTH as usize + cursor_x as usize;
            pixels[dst..dst + bitmap.width as usize]
                .copy_from_slice(&bitmap.pixels[src..src + bitmap.width as usize]);
        }

        tiles[index] = AtlasTile {
            uv_min: [
                cursor_x as f32 / ATLAS_WIDTH as f32,
                cursor_y as f32 / ATLAS_HEIGHT as f32,
            ],
            uv_max: [
                (cursor_x + bitmap.width) as f32 / ATLAS_WIDTH as f32,
                (cursor_y + bitmap.height) as f32 / ATLAS_HEIGHT as f32,
            ],
            aspect: bitmap.width as f32 / bitmap.height as f32,
        };
        cursor_x += bitmap.width + TILE_GAP;
        row_height = row_height.max(bitmap.height);
    }

    LabelAtlas { pixels, tiles }
}
