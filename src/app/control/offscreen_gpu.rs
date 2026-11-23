//! Headless `wgpu` off-screen rendering worker for WYSIWYG `capture` (`renderer: "gpu"` / `"auto"`).
//!
//! Runs all `wgpu::Device` / `MultiPipeline` work on one dedicated native worker thread so:
//! 1. Thread-local raster tile caches (`image_gpu::TILES`) stay completely isolated from the GUI thread.
//! 2. Parallel off-screen captures and tests serialize safely on one adapter/device.
//! 3. Shader pipeline compilation amortizes once per process.

use crate::scene::pipeline::MultiPipeline;
use crate::scene::view::render::Primitive;
use iced::wgpu;
use std::sync::{mpsc, OnceLock};

pub(super) struct GpuRenderOutput {
    pub image: image::RgbaImage,
    pub adapter_name: String,
    pub software: bool,
}

struct GpuJob {
    primitive: Primitive,
    width: u32,
    height: u32,
    clear_color: [f32; 4],
    reply: mpsc::Sender<Result<GpuRenderOutput, String>>,
}

struct OffscreenGpuClient {
    tx: mpsc::Sender<GpuJob>,
}

struct HeadlessGpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: MultiPipeline,
    adapter_name: String,
    software: bool,
}

static GPU_CLIENT: OnceLock<Result<OffscreenGpuClient, String>> = OnceLock::new();

fn try_create_device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue), String> {
    let limits_candidates = [
        wgpu::Limits::default().using_resolution(adapter.limits()),
        wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
    ];
    let mut last_err = String::from("No compatible wgpu device limits found");
    for required_limits in limits_candidates {
        match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("ocs-offscreen-capture"),
            required_features: wgpu::Features::empty(),
            required_limits,
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
        })) {
            Ok(pair) => return Ok(pair),
            Err(e) => last_err = e.to_string(),
        }
    }
    Err(last_err)
}

fn acquire_headless_context() -> Result<HeadlessGpuContext, String> {
    let backends = wgpu::Backends::DX12 | wgpu::Backends::VULKAN | wgpu::Backends::GL | wgpu::Backends::METAL;
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = backends;
    let instance = wgpu::Instance::new(desc);

    let mut candidates: Vec<wgpu::Adapter> = Vec::new();
    if let Ok(hw) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) {
        candidates.push(hw);
    }
    if let Ok(sw) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::None,
        force_fallback_adapter: true,
        compatible_surface: None,
    })) {
        let already = candidates
            .iter()
            .any(|a| a.get_info() == sw.get_info());
        if !already {
            candidates.push(sw);
        }
    }
    if candidates.is_empty() {
        for adapter in pollster::block_on(instance.enumerate_adapters(backends)) {
            candidates.push(adapter);
        }
    }
    if candidates.is_empty() {
        return Err("No hardware or software wgpu adapter available".to_string());
    }

    let mut errors = Vec::new();
    for adapter in candidates {
        let info = adapter.get_info();
        let software = matches!(info.device_type, wgpu::DeviceType::Cpu)
            || info.name.to_ascii_lowercase().contains("microsoft basic render")
            || info.name.to_ascii_lowercase().contains("llvmpipe")
            || info.name.to_ascii_lowercase().contains("lavapipe")
            || info.name.to_ascii_lowercase().contains("swiftshader");
        match try_create_device(&adapter) {
            Ok((device, queue)) => {
                // OCS shaders emit sRGB-encoded floats directly (GAMMA_CORRECTION=false),
                // so target format MUST be non-sRGB Rgba8Unorm.
                let pipeline =
                    MultiPipeline::new_headless(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
                return Ok(HeadlessGpuContext {
                    device,
                    queue,
                    pipeline,
                    adapter_name: info.name,
                    software,
                });
            }
            Err(err) => errors.push(format!("{}: {err}", info.name)),
        }
    }
    Err(format!(
        "Failed to initialize headless wgpu device ({})",
        errors.join("; ")
    ))
}

fn execute_job(ctx: &mut HeadlessGpuContext, job: &GpuJob) -> Result<GpuRenderOutput, String> {
    let (width, height) = (job.width, job.height);
    if width == 0 || height == 0 {
        return Err("Off-screen capture dimensions must be non-zero".to_string());
    }

    let error_scope = ctx
        .device
        .push_error_scope(wgpu::ErrorFilter::Validation);

    let extent = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = ctx.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ocs-offscreen-color"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());

    let bounds = iced::Rectangle {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    };
    let clip = iced::Rectangle {
        x: 0,
        y: 0,
        width,
        height,
    };

    job.primitive
        .prepare_offscreen(&mut ctx.pipeline, &ctx.device, &ctx.queue, &bounds);

    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ocs-offscreen-encoder"),
        });

    {
        let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ocs-offscreen-clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: f64::from(job.clear_color[0]),
                        g: f64::from(job.clear_color[1]),
                        b: f64::from(job.clear_color[2]),
                        a: f64::from(job.clear_color[3]),
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }

    job.primitive
        .render_offscreen(&ctx.pipeline, &mut encoder, &view, &clip);

    let unpadded_bpr = width
        .checked_mul(4)
        .ok_or_else(|| "Image byte width overflow".to_string())?;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bpr = unpadded_bpr.div_ceil(align) * align;
    let buffer_size = u64::from(padded_bpr) * u64::from(height);

    let readback = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ocs-offscreen-readback"),
        size: buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bpr),
                rows_per_image: Some(height),
            },
        },
        extent,
    );

    ctx.queue.submit([encoder.finish()]);

    let (map_tx, map_rx) = mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |res| {
            let _ = map_tx.send(res);
        });
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| format!("wgpu device poll failed: {e}"))?;
    map_rx
        .recv()
        .map_err(|e| format!("wgpu readback channel failed: {e}"))?
        .map_err(|e| format!("wgpu buffer map failed: {e}"))?;

    let packed = {
        let view = readback.slice(..).get_mapped_range();
        let row_bytes = unpadded_bpr as usize;
        let stride = padded_bpr as usize;
        let mut out = vec![0u8; row_bytes * height as usize];
        for row in 0..height as usize {
            let src_start = row * stride;
            let dst_start = row * row_bytes;
            out[dst_start..dst_start + row_bytes]
                .copy_from_slice(&view[src_start..src_start + row_bytes]);
        }
        out
    };
    readback.unmap();

    if let Some(err) = pollster::block_on(error_scope.pop()) {
        return Err(format!("wgpu validation error: {err}"));
    }

    let image = image::RgbaImage::from_raw(width, height, packed)
        .ok_or_else(|| "Rendered GPU image buffer size mismatch".to_string())?;
    Ok(GpuRenderOutput {
        image,
        adapter_name: ctx.adapter_name.clone(),
        software: ctx.software,
    })
}

fn gpu_client() -> Result<&'static OffscreenGpuClient, String> {
    let res = GPU_CLIENT.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<GpuJob>();
        let (init_tx, init_rx) = mpsc::channel::<Result<(), String>>();
        std::thread::Builder::new()
            .name("ocs-offscreen-wgpu".to_string())
            .spawn(move || {
                let mut ctx = match acquire_headless_context() {
                    Ok(ctx) => {
                        let _ = init_tx.send(Ok(()));
                        ctx
                    }
                    Err(err) => {
                        let _ = init_tx.send(Err(err));
                        return;
                    }
                };
                while let Ok(job) = rx.recv() {
                    let result = execute_job(&mut ctx, &job);
                    let _ = job.reply.send(result);
                }
            })
            .map_err(|e| format!("Failed to spawn headless wgpu worker thread: {e}"))?;
        init_rx
            .recv()
            .map_err(|e| format!("Headless wgpu worker exited before initialization: {e}"))??;
        Ok(OffscreenGpuClient { tx })
    });
    res.as_ref().map_err(|e| e.clone())
}

pub(super) fn gpu_available() -> bool {
    gpu_client().is_ok()
}

pub(super) fn gpu_unavailable_reason() -> String {
    match gpu_client() {
        Ok(_) => String::new(),
        Err(err) => err,
    }
}

pub(super) fn render_primitive_rgba(
    primitive: Primitive,
    width: u32,
    height: u32,
    clear_color: [f32; 4],
) -> Result<GpuRenderOutput, String> {
    let client = gpu_client()?;
    let (reply_tx, reply_rx) = mpsc::channel();
    client
        .tx
        .send(GpuJob {
            primitive,
            width,
            height,
            clear_color,
            reply: reply_tx,
        })
        .map_err(|_| "Headless wgpu worker thread stopped unexpectedly".to_string())?;
    reply_rx
        .recv()
        .map_err(|_| "Headless wgpu worker closed reply channel".to_string())?
}
