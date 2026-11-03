use super::*;
use crate::command::CadCommand;
use crate::modules::draw::modify::{
    copy::CopyCommand, mirror::MirrorCommand, rotate::RotateCommand, scale::ScaleCommand,
    translate::MoveCommand,
};
use codec::entities::{Insert, Line};
use codec::types::Vector3;
use glam::DVec3;

fn repeated_blocks(nested: bool) -> (Scene, Vec<Handle>) {
    let mut scene = Scene::new();
    scene
        .define_block_from_owned_entities(
            vec![EntityType::Line(Line::from_points(
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::new(10.0, 5.0, 0.0),
            ))],
            "Leaf",
            DVec3::ZERO,
        )
        .unwrap();
    let name = if nested {
        scene
            .define_block_from_owned_entities(
                vec![
                    EntityType::Insert(Insert::new("Leaf", Vector3::ZERO)),
                    EntityType::Insert(Insert::new("Leaf", Vector3::new(20.0, 0.0, 0.0))),
                ],
                "Parent",
                DVec3::ZERO,
            )
            .unwrap();
        "Parent"
    } else {
        "Leaf"
    };
    let handles = [0.0, 50.0]
        .into_iter()
        .map(|x| {
            scene.add_entity(EntityType::Insert(Insert::new(
                name,
                Vector3::new(x, 0.0, 0.0),
            )))
        })
        .collect();
    (scene, handles)
}

fn assert_geometry(wires: &[WireModel], expected: &[[f64; 3]]) {
    // Inspect the geometry the renderer actually receives, including any
    // prototype-backed copies, rather than only the preview's outer wires.
    let wires = crate::scene::model::instance_model::expand_stubs(wires);
    let points: Vec<_> = wires
        .iter()
        .filter(|wire| wire.name.starts_with("preview_"))
        .flat_map(|wire| {
            assert_eq!(wire.color, WireModel::CYAN);
            (0..wire.points.len())
                .filter(|&i| wire.points[i].iter().all(|v| v.is_finite()))
                .map(|i| wire.point_world(i, 100.0).to_array())
        })
        .collect();
    assert_eq!(points.len(), expected.len(), "{points:?}");
    for point in expected {
        assert!(
            points
                .iter()
                .any(|actual| { actual.iter().zip(point).all(|(a, b)| (a - b).abs() < 1e-4) }),
            "missing {point:?} in {points:?}"
        );
    }
}

#[test]
fn repeated_block_edit_previews_follow_every_command_after_cache_warmup() {
    for nested in [false, true] {
        let (scene, handles) = repeated_blocks(nested);
        let starts: Vec<f64> = if nested {
            vec![0.0, 20.0, 50.0, 70.0]
        } else {
            vec![0.0, 50.0]
        };
        // The cold expansion and several warm expansions must give the same
        // complete preview, including when multiple references are selected.
        for attempt in 0..3 {
            for selection in [&handles[..1], &handles[..]] {
                let selected_starts = &starts[..starts.len() * selection.len() / handles.len()];
                let expected = |map: fn(f64, f64) -> [f64; 3]| {
                    selected_starts
                        .iter()
                        .flat_map(|&x| [map(x, 0.0), map(x + 10.0, 5.0)])
                        .collect::<Vec<_>>()
                };
                let wires = scene.wire_models_for(selection);
                let mut commands: Vec<(Box<dyn CadCommand>, DVec3, Vec<[f64; 3]>)> = vec![
                    (
                        Box::new(MoveCommand::new(selection.to_vec(), wires.clone())),
                        DVec3::new(30.0, 20.0, 0.0),
                        expected(|x, y| [x + 30.0, y + 20.0, 0.0]),
                    ),
                    (
                        Box::new(CopyCommand::new(selection.to_vec(), wires.clone())),
                        DVec3::new(30.0, 20.0, 0.0),
                        expected(|x, y| [x + 30.0, y + 20.0, 0.0]),
                    ),
                    (
                        Box::new(RotateCommand::new(selection.to_vec(), wires.clone())),
                        DVec3::Y,
                        expected(|x, y| [-y, x, 0.0]),
                    ),
                    (
                        Box::new(ScaleCommand::new(selection.to_vec(), wires)),
                        DVec3::new(2.0, 0.0, 0.0),
                        expected(|x, y| [2.0 * x, 2.0 * y, 0.0]),
                    ),
                ];
                let (plain, texts) = scene.mirror_preview_parts(selection);
                commands.push((
                    Box::new(MirrorCommand::new(selection.to_vec(), plain, texts, false)),
                    DVec3::Y,
                    expected(|x, y| [-x, y, 0.0]),
                ));
                for (mut command, cursor, expected) in commands {
                    command.on_point(DVec3::ZERO);
                    let preview = command.on_preview_wires(cursor);
                    eprintln!(
                        "nested={nested} attempt={attempt} command={}",
                        command.name()
                    );
                    assert_geometry(&preview, &expected);
                    // Transformed copies no longer share translation-only GPU
                    // identities: rotating several references must not regroup
                    // them using their original, unrotated placements.
                    if attempt == 2 {
                        assert!(preview.iter().all(|wire| wire.render_instance.is_none()));
                    }
                }
            }
        }
    }
}

#[test]
fn nested_block_insertion_preview_materializes_cached_geometry() {
    let (scene, _) = repeated_blocks(true);
    for _ in 0..3 {
        let wires = scene.block_preview_wires("Parent");
        let preview: Vec<_> = wires
            .iter()
            .map(|wire| wire.mapped(|point| point + DVec3::new(30.0, 20.0, 0.0)))
            .collect();
        assert_geometry(
            &preview,
            &[
                [30.0, 20.0, 0.0],
                [40.0, 25.0, 0.0],
                [50.0, 20.0, 0.0],
                [60.0, 25.0, 0.0],
            ],
        );
        assert!(preview.iter().all(|wire| wire.render_instance.is_none()));
    }
}

#[test]
fn clipboard_block_previews_follow_cursor_after_cache_warmup() {
    use crate::modules::draw::clipboard::paste::PasteCommand;

    for nested in [false, true] {
        let (scene, handles) = repeated_blocks(nested);
        // Warm the same block cache used when the original drawing is rendered.
        scene.wire_models_for(&handles);
        for selection in [&handles[..1], &handles[..]] {
            let entities: Vec<_> = selection
                .iter()
                .map(|handle| scene.document.get_entity(*handle).unwrap().clone())
                .collect();
            let base = DVec3::new(-10.0, -20.0, 0.0);
            let starts: Vec<f64> = selection
                .iter()
                .enumerate()
                .flat_map(|(index, _)| {
                    let x = index as f64 * 50.0;
                    if nested {
                        vec![x, x + 20.0]
                    } else {
                        vec![x]
                    }
                })
                .collect();
            for _ in 0..3 {
                let wires = scene.wires_for_entities(&entities);
                let mut command = PasteCommand::new(wires, base.as_vec3());
                for cursor in [DVec3::new(30.0, 20.0, 0.0), DVec3::new(-50.0, 10.0, 0.0)] {
                    let delta = cursor - base;
                    let expected: Vec<_> = starts
                        .iter()
                        .flat_map(|&x| {
                            [
                                (DVec3::new(x, 0.0, 0.0) + delta).to_array(),
                                (DVec3::new(x + 10.0, 5.0, 0.0) + delta).to_array(),
                            ]
                        })
                        .collect();
                    let preview = command.on_preview_wires(cursor);
                    assert_geometry(&preview, &expected);
                    assert!(preview.iter().all(|wire| wire.stub.is_none()));
                    assert!(preview.iter().all(|wire| wire.render_instance.is_none()));
                }
                assert!(matches!(
                    command.on_point(DVec3::new(30.0, 20.0, 0.0)),
                    crate::command::CmdResult::PasteClipboard { base_pt }
                        if base_pt == DVec3::new(30.0, 20.0, 0.0)
                ));
            }
        }
    }
}

#[test]
fn large_nested_clipboard_uses_a_complete_bounding_box_preview() {
    use crate::modules::draw::clipboard::paste::PasteCommand;

    let (scene, handles) = repeated_blocks(true);
    // Each Parent contains two leaf wires. Expanding cached copies must happen
    // before the paste command chooses its bounded-cost preview for >20k wires.
    let entity = scene.document.get_entity(handles[0]).unwrap().clone();
    let entities = vec![entity; 10_001];
    let mut command = PasteCommand::new(scene.wires_for_entities(&entities), glam::Vec3::ZERO);
    let preview = command.on_preview_wires(DVec3::new(100.0, 80.0, 0.0));
    assert_eq!(preview.len(), 1);
    assert_eq!(preview[0].name, "preview_paste_bbox");
    assert_geometry(
        &preview,
        &[
            [100.0, 80.0, 0.0],
            [130.0, 80.0, 0.0],
            [130.0, 85.0, 0.0],
            [100.0, 85.0, 0.0],
            [100.0, 80.0, 0.0],
        ],
    );
}

#[test]
fn paste_as_block_and_xref_previews_follow_cursor_after_cache_warmup() {
    use crate::modules::insert::{insert_block::InsertBlockCommand, xattach::XAttachCommand};

    let (scene, handles) = repeated_blocks(true);
    scene.wire_models_for(&handles);
    let entities: Vec<_> = handles
        .iter()
        .map(|handle| scene.document.get_entity(*handle).unwrap().clone())
        .collect();
    for _ in 0..3 {
        let wires = scene.wires_for_entities(&entities);
        let mut commands: Vec<Box<dyn CadCommand>> = vec![
            Box::new(InsertBlockCommand::new_for_block(
                "PastedBlock".into(),
                wires.clone(),
                DVec3::ZERO,
            )),
            Box::new(XAttachCommand::with_path("reference.dxf".into()).with_preview(wires, 1.0)),
        ];
        for command in &mut commands {
            for cursor in [DVec3::new(30.0, 20.0, 0.0), DVec3::new(-10.0, 50.0, 0.0)] {
                let expected: Vec<_> = [0.0, 20.0, 50.0, 70.0]
                    .into_iter()
                    .flat_map(|x| {
                        [
                            (DVec3::new(x, 0.0, 0.0) + cursor).to_array(),
                            (DVec3::new(x + 10.0, 5.0, 0.0) + cursor).to_array(),
                        ]
                    })
                    .collect();
                let preview = command.on_preview_wires(cursor);
                assert_geometry(&preview, &expected);
                assert!(preview.iter().all(|wire| wire.stub.is_none()));
                assert!(preview.iter().all(|wire| wire.render_instance.is_none()));
            }
        }
    }
}

#[test]
#[ignore = "requires a GPU adapter"]
fn repeated_block_previews_render_at_the_transformed_positions() {
    use crate::scene::pipeline::{Pipeline, Uniforms};
    use iced::futures::executor::block_on;
    use iced::{wgpu, Rectangle, Size};

    const SIZE: u32 = 256;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&Default::default())).expect("GPU adapter");
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("GPU device");
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut pipeline = Pipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
    pipeline.ensure_depth_texture(&device, Size::new(SIZE, SIZE));
    let bounds = Rectangle::with_size(Size::new(SIZE as f32, SIZE as f32));
    let mut uniforms = Uniforms::new(&Camera::default(), bounds, false);
    uniforms.view_rot = glam::Mat4::from_scale(glam::Vec3::splat(0.01));
    uniforms.eye_high = [0.0; 3];
    uniforms.eye_low = [0.0; 3];
    pipeline.upload_uniforms(&device, &queue, &uniforms);
    let extent = wgpu::Extent3d {
        width: SIZE,
        height: SIZE,
        depth_or_array_layers: 1,
    };
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("block transform preview regression"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("block preview pixels"),
        size: (SIZE * SIZE * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let rect = Rectangle {
        x: 0,
        y: 0,
        width: SIZE,
        height: SIZE,
    };
    for bg in [[0.04, 0.04, 0.04, 1.0], [1.0; 4]] {
        let (mut scene, handles) = repeated_blocks(true);
        scene.bg_color = bg;
        for _ in 0..3 {
            for mode in ["MOVE", "ROTATE", "PASTECLIP", "PASTEBLOCK", "XATTACH"] {
                use crate::modules::draw::clipboard::paste::PasteCommand;
                use crate::modules::insert::{
                    insert_block::InsertBlockCommand, xattach::XAttachCommand,
                };

                let rotate = mode == "ROTATE";
                let mut command: Box<dyn CadCommand> = match mode {
                    "MOVE" => Box::new(MoveCommand::new(
                        handles.clone(),
                        scene.wire_models_for(&handles),
                    )),
                    "ROTATE" => Box::new(RotateCommand::new(
                        handles.clone(),
                        scene.wire_models_for(&handles),
                    )),
                    _ => {
                        let entities: Vec<_> = handles
                            .iter()
                            .map(|handle| scene.document.get_entity(*handle).unwrap().clone())
                            .collect();
                        let wires = scene.wires_for_entities(&entities);
                        match mode {
                            "PASTECLIP" => Box::new(PasteCommand::new(wires, glam::Vec3::ZERO)),
                            "PASTEBLOCK" => Box::new(InsertBlockCommand::new_for_block(
                                "PastedBlock".into(),
                                wires,
                                DVec3::ZERO,
                            )),
                            _ => Box::new(
                                XAttachCommand::with_path("reference.dxf".into())
                                    .with_preview(wires, 1.0),
                            ),
                        }
                    }
                };
                if matches!(mode, "MOVE" | "ROTATE") {
                    command.on_point(DVec3::ZERO);
                }
                let cursor = if rotate {
                    DVec3::Y
                } else {
                    DVec3::new(10.0, 20.0, 0.0)
                };
                let wires = command.on_preview_wires(cursor);
                pipeline.upload_preview_wires(&device, &queue, &wires, &Default::default());
                let mut encoder = device.create_command_encoder(&Default::default());
                pipeline.render(
                    &mut encoder,
                    &view,
                    bounds,
                    rect,
                    rect,
                    bg,
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
                    extent,
                );
                queue.submit([encoder.finish()]);
                let (tx, rx) = std::sync::mpsc::channel();
                readback
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |result| {
                        tx.send(result).unwrap();
                    });
                device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                rx.recv().unwrap().unwrap();
                {
                    let bytes = readback.slice(..).get_mapped_range();
                    for midpoint in [5.0, 25.0, 55.0, 75.0] {
                        let (x, y) = if rotate {
                            (-2.5, midpoint)
                        } else {
                            (midpoint + 10.0, 22.5)
                        };
                        let col = ((x * 0.01 + 1.0) * 0.5 * SIZE as f64) as usize;
                        let row = ((1.0 - y * 0.01) * 0.5 * SIZE as f64) as usize;
                        let cyan = (row - 3..=row + 3).any(|r| {
                            (col - 3..=col + 3).any(|c| {
                                let pixel = &bytes[(r * SIZE as usize + c) * 4..][..4];
                                i16::from(pixel[2]) > i16::from(pixel[0]) + 30
                                    && i16::from(pixel[1]) > i16::from(pixel[0]) + 30
                            })
                        });
                        assert!(
                            cyan,
                            "missing {} preview at ({x}, {y}), bg={bg:?}",
                            command.name()
                        );
                    }
                }
                readback.unmap();
            }
        }
    }
    assert!(block_on(validation.pop()).is_none());
}
