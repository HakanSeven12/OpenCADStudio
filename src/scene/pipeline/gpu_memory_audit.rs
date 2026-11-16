use super::super::*;
use crate::perf::audit;
use std::{hint::black_box, time::Instant};

#[allow(dead_code)]
#[derive(Clone)]
struct LegacyWireGpu {
    instance_buffer: wgpu::Buffer,
    first_instance: u32,
    instance_count: u32,
    is_3d_mesh_edge: bool,
    const_bind_group: Option<std::sync::Arc<wgpu::BindGroup>>,
}

fn duration(mut operation: impl FnMut(), repeats: usize) -> f64 {
    let start = Instant::now();
    for _ in 0..repeats {
        operation();
    }
    start.elapsed().as_secs_f64() * 1e6 / repeats as f64
}

#[test]
#[ignore = "release GPU telemetry and metadata performance audit; requires adapter"]
fn performance_audit_gpu() {
    assert!(!cfg!(debug_assertions), "use --release");
    crate::perf::set_ui_enabled(false);
    assert!(!crate::perf::enabled(), "start the audit with PERF unset");
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter =
        pollster::block_on(instance.request_adapter(&Default::default())).expect("GPU adapter");
    let info = adapter.get_info();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("GPU device");
    let buffers: Vec<_> = (0..32)
        .map(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 4096,
                usage: wgpu::BufferUsages::VERTEX,
                mapped_at_creation: false,
            })
        })
        .collect();
    let const_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 4096,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    let layout = wire_gpu::WireConst::bind_group_layout(&device);
    let group = std::sync::Arc::new(device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: const_buffer.as_entire_binding(),
        }],
    }));
    let mut metadata = Vec::new();
    for storage in [false, true] {
        let old: Vec<_> = (0..1000)
            .map(|i| LegacyWireGpu {
                instance_buffer: buffers[i % buffers.len()].clone(),
                first_instance: 0,
                instance_count: 1,
                is_3d_mesh_edge: false,
                const_bind_group: storage.then(|| group.clone()),
            })
            .collect();
        let new: Vec<_> = old
            .iter()
            .map(|wire| WireGpu {
                instance_buffer: wire.instance_buffer.clone(),
                first_instance: 0,
                instance_count: 1,
                is_3d_mesh_edge: false,
                const_bind_group: wire.const_bind_group.clone(),
                const_buffer: storage.then(|| const_buffer.clone()),
            })
            .collect();
        let timing = audit::paired(
            200,
            || {
                duration(
                    || {
                        let copy = black_box(old.clone());
                        drop(copy);
                    },
                    10,
                )
            },
            || {
                duration(
                    || {
                        let copy = black_box(new.clone());
                        drop(copy);
                    },
                    10,
                )
            },
        );
        eprintln!(
            "GPU wire metadata storage={storage}: {:.3} -> {:.3} us / 1000 clone+drop",
            timing.before.median_us, timing.after.median_us
        );
        metadata.push(serde_json::json!({"case":if storage {"wire_storage"} else {"wire_packed"},"records":1000,
            "before_record_bytes":std::mem::size_of::<LegacyWireGpu>(),"after_record_bytes":std::mem::size_of::<WireGpu>(),"timing":timing}));
    }
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let mut pipeline =
        <MultiPipeline as iced::widget::shader::Pipeline>::new(&device, &queue, format);
    let old: Vec<_> = (0..1000)
        .map(|_| pipeline.inners[0].mesh_default_material_bind_group.clone())
        .collect();
    let new: Vec<_> = (0..1000)
        .map(|_| {
            (
                pipeline.inners[0].mesh_default_material_bind_group.clone(),
                pipeline.inners[0].mesh_default_material_memory.clone(),
            )
        })
        .collect();
    let timing = audit::paired(
        200,
        || {
            duration(
                || {
                    let copy = black_box(old.clone());
                    drop(copy);
                },
                10,
            )
        },
        || {
            duration(
                || {
                    let copy = black_box(new.clone());
                    drop(copy);
                },
                10,
            )
        },
    );
    metadata.push(serde_json::json!({"case":"material_metadata","records":1000,
        "before_record_bytes":std::mem::size_of::<wgpu::BindGroup>(),
        "after_record_bytes":std::mem::size_of::<(wgpu::BindGroup,mesh_gpu::MaterialMemory)>(),
        "retained_resources_struct_bytes_per_unique_material":std::mem::size_of::<mesh_gpu::MeshMaterialResources>() + 2 * std::mem::size_of::<usize>(),
        "timing":timing}));
    let mut telemetry = Vec::new();
    let mut idle_predicates = Vec::new();
    for slots in [1, 4, 8] {
        while pipeline.inners.len() < slots {
            pipeline.inners.push(Pipeline::new(&device, &queue, format));
        }
        let idle = audit::paired(
            100,
            || {
                duration(
                    || {
                        for inner in black_box(&pipeline.inners) {
                            let held = inner.performance_audit_legacy_bytes();
                            black_box(
                                held.wire_arena == 0
                                    && held.shadow <= 4
                                    && inner.alloc_size == Size::new(0, 0),
                            );
                        }
                    },
                    1000,
                )
            },
            || {
                duration(
                    || {
                        for inner in black_box(&pipeline.inners) {
                            black_box(
                                inner.wire_arena.is_none()
                                    && inner.wire_arena_mesh.is_none()
                                    && inner.shadow_full.is_none()
                                    && inner.alloc_size == Size::new(0, 0),
                            );
                        }
                    },
                    1000,
                )
            },
        );
        idle_predicates.push(serde_json::json!({"slots":slots,"scope":"empty-arena idle-slot eligibility predicate only; excludes resource destruction","timing":idle}));
        for batches in [0, 1000] {
            let wires = std::sync::Arc::new(
                (0..batches)
                    .map(|i| WireGpu {
                        instance_buffer: buffers[i % buffers.len()].clone(),
                        first_instance: 0,
                        instance_count: 1,
                        is_3d_mesh_edge: false,
                        const_bind_group: Some(group.clone()),
                        const_buffer: Some(const_buffer.clone()),
                    })
                    .collect::<Vec<_>>(),
            );
            for inner in &mut pipeline.inners {
                inner.gpu_wires = wires.clone();
            }
            pipeline.wire_buffer_cache.clear();
            pipeline.wire_buffer_cache.insert(
                1,
                (
                    wires,
                    Default::default(),
                    Default::default(),
                    Default::default(),
                    Default::default(),
                ),
            );
            for enabled in [false, true] {
                crate::perf::set_ui_enabled(enabled);
                performance_audit_legacy_report(&pipeline);
                crate::scene::view::render::performance_audit_report_gpu_live(&pipeline);
                let repeats = if enabled { 10 } else { 1000 };
                let timing = audit::paired(
                    100,
                    || {
                        duration(
                            || performance_audit_legacy_report(black_box(&pipeline)),
                            repeats,
                        )
                    },
                    || {
                        duration(
                            || {
                                crate::scene::view::render::performance_audit_report_gpu_live(
                                    black_box(&pipeline),
                                )
                            },
                            repeats,
                        )
                    },
                );
                eprintln!("GPU telemetry slots={slots} batches={batches} PERF={enabled}: {:.3} -> {:.3} us median", timing.before.median_us, timing.after.median_us);
                telemetry.push(serde_json::json!({"slots":slots,"wire_ranges_per_slot":batches,"perf_enabled":enabled,
                    "old_partial_bytes":pipeline.performance_audit_legacy_bytes().total(),"new_covered_bytes":pipeline.gpu_live_bytes().total(),"timing":timing}));
            }
        }
    }
    crate::perf::set_ui_enabled(false);
    audit::write_report(
        "gpu",
        serde_json::json!({
            "adapter":format!("{} ({:?}, {:?})", info.name, info.backend, info.device_type),
            "scope":"CPU cost of unchanged-state telemetry and bookkeeping clones on actual wgpu handles; baseline code/record layout from f336fbb37; old counter covers fewer categories; excludes GPU frame time, full upload/driver latency and changed-state formatting/logging; metadata maintained even with PERF disabled",
            "metadata":metadata,"telemetry":telemetry,"idle_predicates":idle_predicates,
        }),
    );
}

#[derive(Default, Clone, Copy, PartialEq)]
pub(crate) struct LegacyGpuLiveBytes {
    pub slots: usize,
    pub shadow: u64,
    pub render_targets: u64,
    pub text_atlas: u64,
    pub wire_arena: u64,
    pub block_geometry: u64,
}

impl LegacyGpuLiveBytes {
    pub(crate) fn total(&self) -> u64 {
        self.shadow + self.render_targets + self.text_atlas + self.wire_arena + self.block_geometry
    }
}

impl Pipeline {
    /// This slot's share. `alloc_size` is the real allocation, which is rounded
    /// up from the requested size and grows only.
    fn performance_audit_legacy_bytes(&self) -> LegacyGpuLiveBytes {
        // 4x MSAA colour (4 B/sample) + 4x MSAA depth-stencil (4 B/sample) +
        // one single-sample resolve target.
        const BYTES_PER_PIXEL: u64 = (MSAA_SAMPLES as u64) * 4 + (MSAA_SAMPLES as u64) * 4 + 4;
        let pixels = u64::from(self.alloc_size.width) * u64::from(self.alloc_size.height);
        LegacyGpuLiveBytes {
            slots: 1,
            // Only while this viewport casts shadows; the placeholder that
            // stands in otherwise is 4 bytes.
            shadow: match &self.shadow_full {
                Some(_) => u64::from(SHADOW_MAP_SIZE) * u64::from(SHADOW_MAP_SIZE) * 4,
                None => 4,
            },
            render_targets: pixels * BYTES_PER_PIXEL,
            text_atlas: self
                .text_atlas_gpu
                .as_ref()
                .map(text_gpu::TextAtlasGpu::gpu_bytes)
                .unwrap_or(0),
            wire_arena: self
                .wire_arena
                .as_ref()
                .map(wire_arena::PersistentWireArena::performance_audit_legacy_gpu_bytes)
                .unwrap_or(0)
                + self
                    .wire_arena_mesh
                    .as_ref()
                    .map(wire_arena::PersistentWireArena::performance_audit_legacy_gpu_bytes)
                    .unwrap_or(0),
            block_geometry: 0,
        }
    }
}

impl MultiPipeline {
    pub(crate) fn performance_audit_legacy_bytes(&self) -> LegacyGpuLiveBytes {
        let mut total = LegacyGpuLiveBytes::default();
        // Block arenas belong to the batches, and a cached build hands the same
        // arenas to several slots, so each one is counted once.
        let mut counted = rustc_hash::FxHashSet::default();
        for inner in &self.inners {
            let slot = inner.performance_audit_legacy_bytes();
            total.slots += slot.slots;
            total.shadow += slot.shadow;
            total.render_targets += slot.render_targets;
            total.text_atlas += slot.text_atlas;
            total.wire_arena += slot.wire_arena;
            total.block_geometry +=
                wire_gpu::block_arena_bytes(&inner.gpu_block_wires, &mut counted)
                    + wire_gpu::block_arena_bytes(&inner.gpu_selected_block_wires, &mut counted);
        }
        for entry in self.wire_buffer_cache.values() {
            total.block_geometry += wire_gpu::block_arena_bytes(&entry.1, &mut counted);
        }
        total
    }
}
fn performance_audit_legacy_report(pipeline: &crate::scene::pipeline::MultiPipeline) {
    use std::cell::Cell;
    if !crate::perf::enabled() {
        return;
    }
    thread_local! {
        static LAST: Cell<Option<LegacyGpuLiveBytes>> = const { Cell::new(None) };
    }
    let now = pipeline.performance_audit_legacy_bytes();
    LAST.with(|last| {
        if last.get() == Some(now) {
            return;
        }
        last.set(Some(now));
        const MIB: f64 = 1024.0 * 1024.0;
        let mib = |bytes: u64| bytes as f64 / MIB;
        crate::perf_record!(
            "[perf] gpu-live slots={} total={:.1}MiB shadow={:.1} targets={:.1} \
text_atlas={:.1} wire_arena={:.1} block_geometry={:.1}",
            now.slots,
            mib(now.total()),
            mib(now.shadow),
            mib(now.render_targets),
            mib(now.text_atlas),
            mib(now.wire_arena),
            mib(now.block_geometry),
        );
    });
}
