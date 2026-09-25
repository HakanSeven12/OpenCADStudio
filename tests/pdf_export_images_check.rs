//! #1266: decoded IMAGE/OLE content must survive the shared PDF/preview/print path.
#![cfg(not(target_arch = "wasm32"))]

use codec::entities::{Insert, Viewport, Wipeout};
use codec::types::{Vector2, Vector3};
use codec::EntityType;
use std::sync::Arc;
use OpenCADStudio::io::pdf_export::{
    export_pdf, export_pdf_pages, PdfPageInput, PdfPlotOptions, PlotContent, PlotGroupSplits,
    PlotImage, PlotWire,
};
use OpenCADStudio::scene::model::image_model::{ImageModel, ImageQuadVertex};
use OpenCADStudio::scene::{Scene, WireModel};

fn embedded_scene() -> Scene {
    let mut scene = Scene::new();
    scene.document = OpenCADStudio::io::load_bytes(
        "embedded-image-print.dxf",
        include_bytes!("fixtures/embedded-image-print.dxf").to_vec(),
    )
    .expect("synthetic embedded bitmap DXF");
    scene.images =
        OpenCADStudio::scene::build_derived_caches_with_progress(&scene.document, &|_| {}, None)
            .images;
    scene.set_current_layout("Layout1".into());
    scene
}

fn page(images: Vec<PlotImage>) -> PdfPageInput {
    PdfPageInput {
        content: PlotContent {
            images,
            ..Default::default()
        },
        paper_w: 100.0,
        paper_h: 100.0,
        offset_x: 0.0,
        offset_y: 0.0,
        rotation_deg: 0,
        scale: 1.0,
        clip: None,
        options: PdfPlotOptions::default(),
        plot_style: None,
    }
}

fn export(pages: &[PdfPageInput]) -> Vec<u8> {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "ocs1266-{}-{}.pdf",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    if let [page] = pages {
        export_pdf(page, &path).expect("export bitmap PDF");
    } else {
        export_pdf_pages(pages, &path, None).expect("export bitmap PDF pages");
    }
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    bytes
}

struct Raster {
    pixels: Vec<u8>,
    width: usize,
    height: usize,
    scale: f32,
}
impl Raster {
    fn from_pdf(bytes: &[u8], page: usize) -> Self {
        Self::at_scale(bytes, page, 2.0)
    }

    fn at_scale(bytes: &[u8], page: usize, scale: f32) -> Self {
        let pdf = hayro::hayro_syntax::Pdf::new(Arc::new(bytes.to_vec())).unwrap();
        let pixmap = hayro::render(
            &pdf.pages()[page],
            &hayro::RenderCache::new(),
            &hayro::hayro_interpret::InterpreterSettings::default(),
            &hayro::RenderSettings {
                x_scale: scale,
                y_scale: scale,
                bg_color: hayro::vello_cpu::color::palette::css::WHITE,
                ..Default::default()
            },
        );
        Self {
            pixels: pixmap.data_as_u8_slice().to_vec(),
            width: u32::from(pixmap.width()) as usize,
            height: u32::from(pixmap.height()) as usize,
            scale,
        }
    }

    fn assert_color(&self, x_mm: f32, y_mm: f32, expected: [u8; 3]) {
        let x = (x_mm * 72.0 / 25.4 * self.scale) as usize;
        let y = self.height - 1 - (y_mm * 72.0 / 25.4 * self.scale) as usize;
        let actual = &self.pixels[(y * self.width + x) * 4..][..3];
        assert!(
            actual
                .iter()
                .zip(expected)
                .all(|(&a, b)| a.abs_diff(b) < 25),
            "at ({x_mm}, {y_mm}): expected {expected:?}, got {actual:?}"
        );
    }
}

fn image() -> PlotImage {
    let corners = [
        [10.0, 10.0, 0.0],
        [50.0, 10.0, 0.0],
        [50.0, 50.0, 0.0],
        [10.0, 50.0, 0.0],
    ];
    let uv = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    // Four distinct quadrants detect flips, reflections, and diagonal seams.
    let pixels = (0..32)
        .flat_map(|y| {
            (0..32).flat_map(move |x| match (x < 16, y < 16) {
                (true, true) => [255, 0, 0, 255],
                (false, true) => [0, 0, 255, 255],
                (true, false) => [0, 255, 0, 255],
                (false, false) => [255, 255, 0, 255],
            })
        })
        .collect();
    PlotImage {
        image: ImageModel {
            render_instance: None,
            file_path: "synthetic".into(),
            pixels: Arc::new(pixels),
            width: 32,
            height: 32,
            opacity: 1.0,
            corners,
            corners_low: [[0.0; 3]; 4],
            draw_depth: 0.5,
            verts: [0, 1, 2, 0, 2, 3]
                .into_iter()
                .map(|i| ImageQuadVertex {
                    pos: corners[i],
                    pos_low: [0.0; 3],
                    uv: uv[i],
                })
                .collect(),
        },
        clips: Vec::new(),
    }
}

#[test]
fn embedded_ole_inside_insert_reaches_rendered_pdf() {
    let scene = embedded_scene();
    let images = scene.paper_plot_images();
    assert_eq!(images.len(), 1);
    assert_eq!((images[0].image.width, images[0].image.height), (64, 32));
    assert!(scene
        .images
        .values()
        .any(|source| Arc::ptr_eq(&source.pixels, &images[0].image.pixels)));
    let mut input = page(images);
    input.scale = 10.0;
    let raster = Raster::from_pdf(&export(&[input]), 0);
    raster.assert_color(23.0, 42.0, [255, 0, 0]);
    raster.assert_color(37.0, 48.0, [0, 0, 255]);
    raster.assert_color(25.0, 47.5, [0, 255, 0]);
    raster.assert_color(45.0, 45.0, [255, 255, 255]);
}

#[test]
fn rotated_nonuniform_insert_preserves_pixel_orientation() {
    let mut scene = embedded_scene();
    let handle = scene
        .document
        .entities()
        .find_map(|e| match e {
            EntityType::Insert(i) if i.block_name == "SyntheticImage" => Some(i.common.handle),
            _ => None,
        })
        .unwrap();
    let Some(EntityType::Insert(insert)) = scene.document.get_entity_mut(handle) else {
        panic!()
    };
    insert.insert_point = Vector3::new(60.0, 20.0, 0.0);
    insert.rotation = std::f64::consts::FRAC_PI_2;
    insert.set_x_scale(20.0);
    insert.set_y_scale(10.0);
    let raster = Raster::from_pdf(&export(&[page(scene.paper_plot_images())]), 0);
    raster.assert_color(58.0, 26.0, [255, 0, 0]);
    raster.assert_color(52.0, 54.0, [0, 0, 255]);
    raster.assert_color(52.5, 30.0, [0, 255, 0]);
    raster.assert_color(65.0, 40.0, [255, 255, 255]);
}

#[test]
fn clipping_page_transform_and_shared_resources_survive_serialization() {
    let mut first = image();
    // A visible-region triangle (IMAGE clip), plus inherited and viewport clips.
    first.image.verts.truncate(3);
    first
        .clips
        .push(vec![[15.0, 5.0], [55.0, 5.0], [55.0, 45.0], [15.0, 45.0]]);
    first
        .clips
        .push(vec![[5.0, 15.0], [45.0, 15.0], [45.0, 55.0], [5.0, 55.0]]);
    let mut second = page(vec![first.clone()]);
    second.rotation_deg = 90;
    second.scale = 0.5;
    second.offset_x = 10.0;
    second.offset_y = 10.0;
    let bytes = export(&[page(vec![first]), second]);
    let parsed = printpdf::PdfDocument::parse(
        &bytes,
        &printpdf::PdfParseOptions::default(),
        &mut Vec::new(),
    )
    .unwrap();
    let uses: Vec<_> = parsed
        .pages
        .iter()
        .flat_map(|p| p.ops.iter())
        .filter_map(|op| {
            if let printpdf::Op::UseXobject { id, .. } = op {
                Some(id)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        uses.len(),
        2,
        "one image draw per affine quad, without a diagonal seam"
    );
    assert_eq!(uses[0], uses[1], "pages should share one image resource");
    let raster = Raster::from_pdf(&bytes, 0);
    raster.assert_color(40.0, 20.0, [255, 255, 0]);
    for (x, y) in [(12.0, 20.0), (48.0, 20.0), (20.0, 40.0), (40.0, 12.0)] {
        raster.assert_color(x, y, [255, 255, 255]);
    }
    Raster::from_pdf(&bytes, 1).assert_color(85.0, 25.0, [255, 255, 0]);
}

#[test]
fn image_alpha_and_plot_transparency_are_preserved() {
    let mut plot = image();
    plot.image.opacity = 0.5;
    // Intrinsic transparent texels remain transparent with either plot setting.
    let pixels = Arc::make_mut(&mut plot.image.pixels);
    for y in 16..32 {
        for x in 0..16 {
            pixels[(y * 32 + x) * 4 + 3] = 0;
        }
    }
    let opaque = page(vec![plot.clone()]);
    let mut transparent = page(vec![plot]);
    transparent.options.transparency = true;
    let bytes = export(&[opaque, transparent]);
    let off = Raster::from_pdf(&bytes, 0);
    off.assert_color(20.0, 40.0, [255, 0, 0]);
    off.assert_color(20.0, 20.0, [255, 255, 255]);
    let on = Raster::from_pdf(&bytes, 1);
    on.assert_color(20.0, 40.0, [255, 128, 128]);
    on.assert_color(20.0, 20.0, [255, 255, 255]);
}

#[test]
fn depth_and_paper_model_groups_order_images_with_wipeouts_and_wires() {
    let mut scene = Scene::new();
    let wipeout = Wipeout::polygonal(
        &[
            Vector2::new(10.0, 30.0),
            Vector2::new(30.0, 30.0),
            Vector2::new(30.0, 50.0),
            Vector2::new(10.0, 50.0),
        ],
        0.0,
    );
    scene.add_entity(EntityType::Wipeout(wipeout));
    let mut input = page(vec![image()]);
    input.content.wipeouts = scene.paper_plot_wipeouts().as_ref().clone();
    assert_eq!(input.content.wipeouts.len(), 1);
    input.content.wipeouts[0].draw_depth = 0.8;
    let mut wire = WireModel::solid(
        "over image".into(),
        vec![[35.0, 15.0, 0.0], [45.0, 15.0, 0.0]],
        [0.0, 0.0, 0.0, 1.0],
        false,
    );
    wire.line_weight_px = 8.0;
    input.content.wires = Arc::new(vec![PlotWire {
        wire,
        draw_depth: 0.9,
    }]);
    let raster = Raster::from_pdf(&export(&[input]), 0);
    raster.assert_color(20.0, 40.0, [255, 255, 255]);
    raster.assert_color(40.0, 40.0, [0, 0, 255]);
    raster.assert_color(40.0, 15.0, [0, 0, 0]);

    // First group wins by depth internally, second group still paints last.
    let mut input = page(vec![image()]);
    input.content.wipeouts = scene.paper_plot_wipeouts().as_ref().clone();
    input.content.wipeouts[0].draw_depth = 0.9;
    input.content.group_splits = PlotGroupSplits {
        wipeouts: 1,
        ..Default::default()
    };
    Raster::from_pdf(&export(&[input]), 0).assert_color(20.0, 40.0, [255, 0, 0]);
}

#[test]
fn viewport_projection_clips_images_and_respects_frozen_and_nonplot_layers() {
    let mut scene = embedded_scene();
    scene.set_current_layout("Model".into());
    scene.ensure_layer("Bitmap");
    let mut insert = Insert::new("SyntheticImage", Vector3::new(0.0, 0.0, 0.0));
    insert.common.layer = "Bitmap".into();
    insert.set_x_scale(20.0);
    insert.set_y_scale(20.0);
    scene.add_entity(EntityType::Insert(insert));
    scene.set_current_layout("Layout1".into());
    let mut viewport = Viewport::new();
    viewport.center = Vector3::new(50.0, 50.0, 0.0);
    viewport.width = 20.0;
    viewport.height = 20.0;
    viewport.view_height = 20.0;
    viewport.view_center = Vector3::new(20.0, 10.0, 0.0);
    viewport.id = 2;
    viewport.status.is_on = true;
    let handle = scene.add_entity(EntityType::Viewport(viewport));
    let (_, _, _, images) = scene.viewport_plot_fills();
    assert_eq!(images.len(), 1);
    let raster = Raster::from_pdf(&export(&[page(images)]), 0);
    raster.assert_color(42.0, 43.0, [255, 0, 0]);
    raster.assert_color(58.0, 57.0, [0, 0, 255]);
    raster.assert_color(35.0, 45.0, [255, 255, 255]);
    let layer = scene.document.layers.get("Bitmap").unwrap().handle;
    let Some(EntityType::Viewport(vp)) = scene.document.get_entity_mut(handle) else {
        panic!()
    };
    vp.frozen_layers.push(layer);
    assert!(scene.viewport_plot_fills().3.is_empty());
    let Some(EntityType::Viewport(vp)) = scene.document.get_entity_mut(handle) else {
        panic!()
    };
    vp.frozen_layers.clear();
    scene
        .document
        .layers
        .get_mut("Bitmap")
        .unwrap()
        .is_plottable = false;
    assert!(scene.viewport_plot_fills().3.is_empty());
}

#[test]
fn linked_image_uses_decoded_pixels_and_its_image_clip_without_reopening() {
    use codec::entities::{ClipBoundary, RasterImage};
    let path = std::env::temp_dir().join(format!("ocs1266-linked-{}.png", std::process::id()));
    let source = image();
    image::save_buffer(&path, &source.image.pixels, 32, 32, image::ColorType::Rgba8).unwrap();
    let mut raster = RasterImage::new(
        path.to_str().unwrap(),
        Vector3::new(10.0, 10.0, 0.0),
        32.0,
        32.0,
    );
    raster.u_vector = Vector3::new(1.25, 0.0, 0.0);
    raster.v_vector = Vector3::new(0.0, 1.25, 0.0);
    raster.clipping_enabled = true;
    raster.clip_boundary =
        ClipBoundary::rectangular(Vector2::new(0.0, 0.0), Vector2::new(16.0, 32.0));
    let mut scene = Scene::new();
    let handle = scene.add_entity(EntityType::RasterImage(raster));
    scene.populate_images_from_document();
    std::fs::remove_file(path).unwrap();
    let images = scene.paper_plot_images();
    assert_eq!(images.len(), 1);
    assert!(Arc::ptr_eq(
        &images[0].image.pixels,
        &scene.images[&handle].pixels
    ));
    let rendered = Raster::from_pdf(&export(&[page(images)]), 0);
    rendered.assert_color(20.0, 40.0, [255, 0, 0]);
    rendered.assert_color(20.0, 20.0, [0, 255, 0]);
    rendered.assert_color(40.0, 40.0, [255, 255, 255]);
}

#[test]
fn equal_sized_sources_remain_distinct_across_pages() {
    let pages = {
        let red = image();
        let mut cyan = image();
        Arc::make_mut(&mut cyan.image.pixels)
            .chunks_exact_mut(4)
            .for_each(|pixel| pixel.copy_from_slice(&[0, 255, 255, 255]));
        assert!(!Arc::ptr_eq(&red.image.pixels, &cyan.image.pixels));
        vec![
            page(vec![red.clone()]),
            page(vec![cyan.clone()]),
            page(vec![red]),
            page(vec![cyan]),
        ]
    };
    let bytes = export(&pages);
    let parsed = printpdf::PdfDocument::parse(
        &bytes,
        &printpdf::PdfParseOptions::default(),
        &mut Vec::new(),
    )
    .unwrap();
    let uses: Vec<_> = parsed
        .pages
        .iter()
        .flat_map(|page| &page.ops)
        .filter_map(|op| {
            if let printpdf::Op::UseXobject { id, .. } = op {
                Some(id)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(uses.len(), 4);
    assert_ne!(uses[0], uses[1]);
    assert_eq!(uses[0], uses[2]);
    assert_eq!(uses[1], uses[3]);
    for (index, expected) in [[255, 0, 0], [0, 255, 255], [255, 0, 0], [0, 255, 255]]
        .into_iter()
        .enumerate()
    {
        Raster::from_pdf(&bytes, index).assert_color(20.0, 40.0, expected);
    }
}

#[test]
fn affine_triangle_clip_has_no_shared_edge_seam() {
    let mut plot = image();
    Arc::make_mut(&mut plot.image.pixels)
        .chunks_exact_mut(4)
        .for_each(|pixel| pixel.copy_from_slice(&[0, 0, 0, 128]));
    let mut sheared = plot.clone();
    let transform = |[x, y, z]: [f32; 3]| [x + 0.27 * y + 5.0, 0.19 * x + y + 3.0, z];
    sheared
        .image
        .corners
        .iter_mut()
        .for_each(|p| *p = transform(*p));
    sheared
        .image
        .verts
        .iter_mut()
        .for_each(|v| v.pos = transform(v.pos));
    let pages = [page(vec![plot]), page(vec![sheared])];
    let bytes = export(&pages);
    for (index, page) in pages.iter().enumerate() {
        let raster = Raster::at_scale(&bytes, index, 8.0);
        let corners = page.content.images[0].image.corners;
        let u = [corners[1][0] - corners[0][0], corners[1][1] - corners[0][1]];
        let v = [corners[3][0] - corners[0][0], corners[3][1] - corners[0][1]];
        let determinant = u[0] * v[1] - u[1] * v[0];
        let mut checked = 0;
        for y in 0..raster.height {
            for x in 0..raster.width {
                let dx = (x as f32 + 0.5) / raster.scale * 25.4 / 72.0 - corners[0][0];
                let dy = (raster.height as f32 - y as f32 - 0.5) / raster.scale * 25.4 / 72.0
                    - corners[0][1];
                let a = (dx * v[1] - dy * v[0]) / determinant;
                let b = (u[0] * dy - u[1] * dx) / determinant;
                if (0.15..0.85).contains(&a) && (0.15..0.85).contains(&b) {
                    let rgb = &raster.pixels[(y * raster.width + x) * 4..][..3];
                    assert!(rgb.iter().all(|&c| c.abs_diff(127) <= 2),
                    "page {index}, pixel ({x}, {y}): shared-edge gap or double coverage: {rgb:?}");
                    checked += 1;
                }
            }
        }
        assert!(checked > 100_000);
    }
}

#[test]
fn malformed_bitmap_reports_an_error_without_replacing_the_pdf() {
    let path = std::env::temp_dir().join(format!("ocs1266-invalid-{}.pdf", std::process::id()));
    std::fs::write(&path, b"existing PDF").unwrap();
    let mut pixels = image();
    pixels.image.width += 1;
    let mut coordinates = image();
    coordinates.image.corners[0][0] = f32::NAN;
    let mut mapping = image();
    mapping.image.corners[2][0] += 10.0;
    mapping
        .image
        .verts
        .iter_mut()
        .for_each(|vertex| vertex.uv = [0.0, 0.0]);
    for (plot, reason) in [
        (pixels, "dimensions"),
        (coordinates, "coordinates"),
        (mapping, "texture mapping"),
    ] {
        let error = export_pdf(&page(vec![plot]), &path).unwrap_err();
        assert!(
            error.contains("Page 1") && error.contains(reason),
            "{error}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"existing PDF");
    }
    std::fs::remove_file(path).unwrap();
}
