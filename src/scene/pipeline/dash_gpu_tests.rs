//! Pixels of the dash layout through the real wire pipelines, storage and
//! packed, plain and instanced: the screen must draw what the plot cuts.
use super::*;
use crate::io::pdf_export::dash_ranges;
use crate::scene::model::instance_model::RenderInstance;
use crate::scene::model::wire_model::DashClosure;
use crate::scene::view::camera::Camera;
use iced::futures::executor::block_on;

const SIZE: u32 = 1024;
/// Pixels either side of an expected dash end that are left unchecked: the
/// round cap and the line's antialiasing live there.
const MARGIN: f32 = 4.0;

fn dashed(name: u32, y: f32, xs: &[f32], pattern_length: f32, pattern: [f32; 8]) -> WireModel {
    let points = xs
        .iter()
        .map(|&x| {
            if x.is_nan() {
                [f32::NAN; 3]
            } else {
                [x, y, 0.5]
            }
        })
        .collect();
    let mut wire = WireModel::solid(name.to_string(), points, [1.0; 4], false);
    wire.pattern_length = pattern_length;
    wire.pattern = pattern;
    wire.plinegen = !xs.iter().any(|x| x.is_nan());
    wire
}

fn to_px(ndc: f32) -> f32 {
    (ndc * 0.5 + 0.5) * SIZE as f32
}

/// The pixel columns of `wire` the plot strokes, as [from, to].
fn plotted_spans(wire: &WireModel, scale: f32) -> Vec<[f32; 2]> {
    let mut scaled = wire.clone();
    scaled.pattern_length *= scale;
    scaled.pattern = scaled.pattern.map(|value| value * scale);
    let dashes = crate::scene::pipeline::wire_gpu::wire_distances(&scaled);
    let mut spans: Vec<[f32; 2]> = Vec::new();
    for index in 0..scaled.points.len() - 1 {
        let (a, b) = (scaled.points[index][0], scaled.points[index + 1][0]);
        if !a.is_finite() || !b.is_finite() {
            continue;
        }
        for [from, to] in dash_ranges(
            dashes.dists[index],
            dashes.dists[index + 1],
            dashes.run_totals[index],
            scaled.pattern_length,
            &scaled.pattern,
        ) {
            let span = [to_px(a + (b - a) * from), to_px(a + (b - a) * to)];
            match spans.last_mut() {
                Some(last) if span[0] - last[1] < 0.5 => last[1] = span[1],
                _ => spans.push(span),
            }
        }
    }
    spans
}

fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &mut Pipeline,
    block_geometry: &mut wire_gpu::BlockGeometryCache,
    uniforms: &Uniforms,
    wires: &[WireModel],
) -> Vec<u8> {
    let size = wgpu::Extent3d {
        width: SIZE,
        height: SIZE,
        depth_or_array_layers: 1,
    };
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("dash pixels"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("dash readback"),
        size: (SIZE * SIZE * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    pipeline.ensure_depth_texture(device, Size::new(SIZE, SIZE));
    pipeline.upload_uniforms(device, queue, uniforms);
    let (plain, blocks, _, circles, ellipses) =
        pipeline.build_wire_buffers(device, queue, wires, &Default::default(), block_geometry);
    pipeline.gpu_wires = plain;
    pipeline.gpu_block_wires = blocks;
    pipeline.gpu_circles = circles;
    pipeline.gpu_ellipses = ellipses;
    let mut encoder = device.create_command_encoder(&Default::default());
    let rect = Rectangle {
        x: 0,
        y: 0,
        width: SIZE,
        height: SIZE,
    };
    pipeline.render(
        &mut encoder,
        &view,
        Rectangle::with_size(Size::new(SIZE as f32, SIZE as f32)),
        rect,
        rect,
        [0., 0., 0., 1.],
        false,
        false,
        false,
    );
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIZE * 4),
                rows_per_image: Some(SIZE),
            },
        },
        size,
    );
    queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        tx.send(r).unwrap();
    });
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();
    let bytes = readback.slice(..).get_mapped_range().to_vec();
    readback.unmap();
    bytes
}

/// Whether column `x` holds a lit pixel near the row of NDC height `y`.
fn lit(bytes: &[u8], x: usize, y: f32) -> bool {
    let row = ((1.0 - y) * 0.5 * SIZE as f32) as usize;
    (row - 3..=row + 3).any(|r| {
        let pixel = &bytes[(r * SIZE as usize + x) * 4..][..3];
        pixel.iter().any(|&channel| channel > 60)
    })
}

/// What differs between the row's pixels and the plot's dashes.
fn mismatches(bytes: &[u8], wire: &WireModel, scale: f32) -> Vec<String> {
    let y = wire.points.iter().find(|p| p[0].is_finite()).unwrap()[1];
    let spans = plotted_spans(wire, scale);
    let xs = wire
        .points
        .iter()
        .filter(|p| p[0].is_finite())
        .map(|p| p[0]);
    let start = to_px(xs.clone().fold(f32::MAX, f32::min));
    let end = to_px(xs.fold(f32::MIN, f32::max));
    let mut wrong = Vec::new();
    // The cap past a path's end is not checked: a closed path's pattern resumes
    // there, on the dash it started with.
    for column in start.ceil() as usize..end.floor() as usize - 1 {
        let at = column as f32 + 0.5;
        let inside = spans
            .iter()
            .any(|span| at >= span[0] + MARGIN && at <= span[1] - MARGIN);
        let outside = spans
            .iter()
            .all(|span| at <= span[0] - MARGIN || at >= span[1] + MARGIN);
        let on = lit(bytes, column, y);
        if (inside && !on) || (outside && on) {
            wrong.push(format!("{column}:{}", if on { "lit" } else { "dark" }));
        }
    }
    if wrong.is_empty() {
        return wrong;
    }
    let row: Vec<usize> = (start as usize..=end as usize)
        .filter(|&column| lit(bytes, column, y))
        .collect();
    let mut drawn: Vec<[usize; 2]> = Vec::new();
    for column in row {
        match drawn.last_mut() {
            Some(last) if column == last[1] + 1 => last[1] = column,
            _ => drawn.push([column, column]),
        }
    }
    vec![format!(
        "wire {} expected {spans:?}, drew {drawn:?}; first wrong {:?}",
        wire.name,
        &wrong[..wrong.len().min(6)]
    )]
}

/// One of each dash layout, on its own row.
fn layouts() -> Vec<WireModel> {
    const DASH: [f32; 8] = [0.1, -0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    const DOTTED: [f32; 8] = [0.1, -0.05, 0.02, -0.05, 0.0, 0.0, 0.0, 0.0];
    let row = |index: u32| 0.9 - index as f32 * 0.11;
    let mut wires = Vec::new();
    // "A"-aligned: one whole run, end dashes taking up the rest.
    wires.push(dashed(1, row(0), &[-0.9, 0.87], 0.2, DASH));
    wires.push(dashed(
        2,
        row(1),
        &[-0.9, -0.2, 0.87],
        0.2,
        DOTTED.map(|v| v),
    ));
    wires.last_mut().unwrap().pattern_length = 0.22;
    // Shorter than a period: solid.
    wires.push(dashed(3, row(2), &[-0.9, -0.75], 0.2, DASH));
    // PLINEGEN off: each run aligns on its own, long and short.
    wires.push(dashed(
        4,
        row(3),
        &[-0.9, -0.33, f32::NAN, -0.33, -0.25, f32::NAN, -0.25, 0.87],
        0.2,
        DASH,
    ));
    // A closed curve: whole periods, stretched, from its first point.
    let mut curve = dashed(5, row(4), &[-0.9, -0.4, 0.1, 0.87], 0.2, DASH);
    curve.dash_closure = DashClosure::Curve;
    wires.push(curve);
    // A tie rounds down, and a single period still shows two.
    let mut tie = dashed(6, row(5), &[-0.9, 0.2], 0.2, DASH);
    tie.points[1][0] = -0.9 + 0.2 * 5.5;
    tie.dash_closure = DashClosure::Curve;
    wires.push(tie);
    let mut one = dashed(
        7,
        row(6),
        &[-0.9, -0.3],
        0.6,
        [0.3, -0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    );
    one.dash_closure = DashClosure::Curve;
    wires.push(one);
    // A closed polyline: the pattern starts at the second vertex.
    let mut polyline = dashed(8, row(7), &[-0.9, -0.53, 0.1, 0.87], 0.2, DASH);
    polyline.dash_closure = DashClosure::Polyline { start: 1 };
    wires.push(polyline);
    // Closed and shorter than a period: solid.
    let mut short = dashed(9, row(8), &[-0.9, -0.75], 0.2, DASH);
    short.dash_closure = DashClosure::Curve;
    wires.push(short);
    // A DGN line style: from the start vertex, not aligned.
    let mut from_start = dashed(10, row(9), &[-0.9, 0.83], 0.2, DASH);
    from_start.dash_from_start = true;
    wires.push(from_start);
    // A wide band keeps its widths in the slots a thin wire reuses.
    let mut band = dashed(11, row(10), &[-0.9, 0.87], 0.2, DASH);
    band.world_width = 0.01;
    wires.push(band);
    // The instanced path, whose batched children are runs of their own.
    for (index, source) in [(11_u32, 3_usize), (12, 7), (13, 0), (14, 10)] {
        let mut instanced = wires[source].clone();
        for point in &mut instanced.points {
            if point[0].is_finite() {
                point[1] = row(index);
            }
        }
        instanced.name = (index + 90).to_string();
        instanced.render_instance = Some(RenderInstance {
            source_id: index as u64 + 90,
            translation: [0.0, 0.0, 0.0],
        });
        wires.push(instanced);
    }
    wires
}

/// A tapered band, plain or instanced: its height in pixels a quarter and three
/// quarters along.
fn taper_heights(bytes: &[u8], y: f32) -> [usize; 2] {
    [to_px(-0.5), to_px(0.5)].map(|x| {
        let column = x as usize;
        let row = ((1.0 - y) * 0.5 * SIZE as f32) as usize;
        (row - 30..=row + 30)
            .filter(|r| {
                bytes[(r * SIZE as usize + column) * 4..][..3]
                    .iter()
                    .any(|&channel| channel > 60)
            })
            .count()
    })
}

fn tapered(name: u32, y: f32, instanced: bool) -> WireModel {
    let mut wire = dashed(name, y, &[-1.0, 1.0], 0.0, [0.0; 8]);
    wire.world_width = 0.04;
    wire.taper_widths = vec![0.02, 0.04];
    if instanced {
        wire.render_instance = Some(RenderInstance {
            source_id: name as u64,
            translation: [0.0, 0.0, 0.0],
        });
    }
    wire
}

#[test]
#[ignore = "requires a GPU adapter"]
fn the_screen_draws_the_dashes_the_plot_cuts() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&Default::default())).expect("GPU adapter");
    println!("adapter: {:?}", adapter.get_info().name);
    let bounds = Rectangle::with_size(Size::new(SIZE as f32, SIZE as f32));
    let mut failures = Vec::new();
    for packed in [false, true] {
        let mut limits = adapter.limits();
        if packed {
            limits.max_storage_buffers_per_shader_stage = 0;
        }
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: limits,
            ..Default::default()
        }))
        .expect("GPU device");
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut pipeline = Pipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let mut block_geometry = wire_gpu::BlockGeometryCache::default();
        let mut wires = layouts();
        let (plain_taper, instanced_taper) = (-0.76, -0.88);
        let dashed_rows = wires.len();
        wires.push(tapered(20, plain_taper, false));
        wires.push(tapered(21, instanced_taper, true));
        for scale in [1.0_f32, 0.5] {
            let mut uniforms = Uniforms::new(&Camera::default(), bounds, false);
            uniforms.view_rot = glam::Mat4::IDENTITY;
            uniforms.eye_high = [0.; 3];
            uniforms.eye_low = [0.; 3];
            // The cap's distance reads this: drawn in NDC, a pixel is this wide.
            uniforms.world_per_pixel = 2.0 / SIZE as f32;
            uniforms.linetype_scale = scale;
            let bytes = render(
                &device,
                &queue,
                &mut pipeline,
                &mut block_geometry,
                &uniforms,
                &wires,
            );
            if let Ok(path) = std::env::var("OCS_DASH_TEST_IMAGES") {
                let path = std::path::Path::new(&path);
                std::fs::create_dir_all(path).unwrap();
                image::save_buffer(
                    path.join(format!(
                        "dash-{}-{scale}.png",
                        if packed { "packed" } else { "storage" }
                    )),
                    &bytes,
                    SIZE,
                    SIZE,
                    image::ColorType::Rgba8,
                )
                .unwrap();
            }
            for y in [plain_taper, instanced_taper] {
                let [thin, thick] = taper_heights(&bytes, y);
                // Widths of 0.025 and 0.035 at those points, 512 px a unit.
                if thin.abs_diff(13) > 3 || thick.abs_diff(18) > 3 {
                    failures.push(format!(
                        "packed={packed} taper at {y}: {thin} and {thick} px"
                    ));
                }
            }
            for wire in &wires[..dashed_rows] {
                for problem in mismatches(&bytes, wire, scale) {
                    failures.push(format!("packed={packed} scale={scale} {problem}"));
                }
            }
        }
        assert!(
            block_on(validation.pop()).is_none(),
            "GPU validation failed"
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
