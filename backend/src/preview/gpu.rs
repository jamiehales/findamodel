//! Hardware preview renderer built on wgpu (Vulkan, Metal, DX12 or GL), with 4x MSAA.
//! Camera, lighting and coordinate system mirror the frontend viewer exactly.
//!
//! Large meshes are drawn in chunks so no single vertex buffer exceeds device limits;
//! chunks share one depth buffer, so the result is identical to a single draw.

use anyhow::{Context, Result, anyhow};
use bytemuck::{Pod, Zeroable};
use parking_lot::Mutex;
use tracing::info;
use wgpu::util::DeviceExt;

use super::camera::{self, FOV_Y};
use super::encode_png;
use crate::geometry::{Triangle, Vec3};

const LOG_TARGET: &str = "findamodel.Preview";
const SAMPLE_COUNT: u32 = 4;
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const BYTES_PER_VERTEX: u64 = 24;
const MAX_TRIANGLES_PER_CHUNK: u64 = 1_000_000;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [f32; 16],
    camera_pos: [f32; 4],
    albedo: [f32; 4],
}

struct Targets {
    width: u32,
    height: u32,
    msaa_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    resolve: wgpu::Texture,
    resolve_view: wgpu::TextureView,
    readback: wgpu::Buffer,
    padded_bytes_per_row: u32,
}

pub struct GpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    renderer_name: String,
    triangles_per_chunk: u64,
    /// Render targets are reused between renders; the lock also serialises GPU work.
    targets: Mutex<Option<Targets>>,
}

struct Scene {
    view_proj: glam::Mat4,
    eye: Vec3,
}

impl GpuRenderer {
    /// Creates a headless device on a hardware adapter. Software adapters (lavapipe,
    /// llvmpipe, SwiftShader) are rejected because the CPU rasteriser is much faster than
    /// them.
    pub fn new() -> Result<Self> {
        pollster::block_on(Self::new_async(false))
    }

    /// Like [`GpuRenderer::new`], but also accepts software adapters (used by tests).
    pub fn new_allowing_software() -> Result<Self> {
        pollster::block_on(Self::new_async(true))
    }

    async fn new_async(allow_software: bool) -> Result<Self> {
        // Prefer native APIs; only probe GL (EGL) when none are available, since GL probing
        // is slower and noisy in headless containers.
        let mut adapter = None;
        let mut last_error = String::from("no backends tried");
        for backends in [wgpu::Backends::PRIMARY, wgpu::Backends::GL] {
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
                backends,
                flags: wgpu::InstanceFlags::empty(),
                ..Default::default()
            });
            match instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    force_fallback_adapter: false,
                    compatible_surface: None,
                })
                .await
            {
                Ok(a) => {
                    adapter = Some(a);
                    break;
                }
                Err(e) => last_error = e.to_string(),
            }
        }
        let adapter = adapter.ok_or_else(|| anyhow!("no graphics adapter available: {last_error}"))?;

        let info = adapter.get_info();
        let renderer_name = format!("{} ({:?}, {:?})", info.name, info.backend, info.device_type);
        if info.device_type == wgpu::DeviceType::Cpu && !allow_software {
            return Err(anyhow!("only a software adapter is available ({renderer_name})"));
        }
        let limits = adapter.limits();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("findamodel-preview"),
                required_features: wgpu::Features::empty(),
                required_limits: limits.clone(),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
            })
            .await
            .context("creating graphics device")?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("preview-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("preview-uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("preview-layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("preview-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: BYTES_PER_VERTEX,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: SAMPLE_COUNT, ..Default::default() },
            multiview: None,
            cache: None,
        });

        let max_vertices_per_buffer = limits.max_buffer_size / BYTES_PER_VERTEX;
        let triangles_per_chunk = MAX_TRIANGLES_PER_CHUNK.min(max_vertices_per_buffer / 3).max(1);

        info!(target: LOG_TARGET, "GPU preview renderer ready: {renderer_name}");
        Ok(Self {
            device,
            queue,
            pipeline,
            bind_group_layout,
            renderer_name,
            triangles_per_chunk,
            targets: Mutex::new(None),
        })
    }

    pub fn renderer_name(&self) -> &str {
        &self.renderer_name
    }

    /// Renders the model-only and with-supports variants, returning `(model_only, with_supports)` PNGs.
    /// The body is uploaded once and shared by both scenes when it fits in one chunk.
    pub fn render_variant_pair(
        &self,
        body: &[Triangle],
        supports: Option<&[Triangle]>,
        width: u32,
        height: u32,
        body_albedo: Vec3,
        support_albedo: Vec3,
    ) -> Result<(Vec<u8>, Vec<u8>)> {
        let mut targets = self.targets.lock();
        let targets = self.ensure_targets(&mut targets, width, height);
        let supports = supports.filter(|s| !s.is_empty());

        let model_only_scene = self.scene(&[body], width, height);
        let with_supports_scene = match supports {
            Some(s) => self.scene(&[body, s], width, height),
            None => self.scene(&[body], width, height),
        };

        let shared_body = (body.len() as u64 <= self.triangles_per_chunk).then(|| self.vertex_buffer(body));

        self.draw_scene(targets, &model_only_scene, &[(body, body_albedo, shared_body.as_ref())])?;
        let model_only = self.read_png(targets)?;

        let mut draws = vec![(body, body_albedo, shared_body.as_ref())];
        if let Some(s) = supports {
            draws.push((s, support_albedo, None));
        }
        self.draw_scene(targets, &with_supports_scene, &draws)?;
        let with_supports = self.read_png(targets)?;

        Ok((model_only, with_supports))
    }

    /// Renders a single preview; supports (if any) are drawn in their own colour.
    pub fn render(
        &self,
        body: &[Triangle],
        supports: Option<&[Triangle]>,
        width: u32,
        height: u32,
        body_albedo: Vec3,
        support_albedo: Vec3,
    ) -> Result<Vec<u8>> {
        let mut targets = self.targets.lock();
        let targets = self.ensure_targets(&mut targets, width, height);
        let supports = supports.filter(|s| !s.is_empty());

        let scene = match supports {
            Some(s) => self.scene(&[body, s], width, height),
            None => self.scene(&[body], width, height),
        };
        let mut draws = vec![(body, body_albedo, None)];
        if let Some(s) = supports {
            draws.push((s, support_albedo, None));
        }
        self.draw_scene(targets, &scene, &draws)?;
        self.read_png(targets)
    }

    fn scene(&self, sets: &[&[Triangle]], width: u32, height: u32) -> Scene {
        let aspect = width as f32 / height as f32;
        let (centre, eye, dist) = camera::frame(sets.iter().copied(), aspect);
        let view = glam::Mat4::look_at_rh(
            glam::Vec3::new(eye.x, eye.y, eye.z),
            glam::Vec3::new(centre.x, centre.y, centre.z),
            glam::Vec3::Y,
        );
        let proj = glam::Mat4::perspective_rh(FOV_Y, aspect, 0.1, dist * 4.0);
        Scene { view_proj: proj * view, eye }
    }

    fn vertex_data(triangles: &[Triangle]) -> Vec<f32> {
        let mut data = Vec::with_capacity(triangles.len() * 18);
        for tri in triangles {
            let n = tri.preview_normal();
            for v in [tri.v0, tri.v1, tri.v2] {
                data.extend_from_slice(&[v.x, v.y, v.z, n.x, n.y, n.z]);
            }
        }
        data
    }

    fn vertex_buffer(&self, triangles: &[Triangle]) -> wgpu::Buffer {
        self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("preview-vertices"),
            contents: bytemuck::cast_slice(&Self::vertex_data(triangles)),
            usage: wgpu::BufferUsages::VERTEX,
        })
    }

    fn uniform_bind_group(&self, scene: &Scene, albedo: Vec3) -> wgpu::BindGroup {
        let uniforms = Uniforms {
            view_proj: scene.view_proj.to_cols_array(),
            camera_pos: [scene.eye.x, scene.eye.y, scene.eye.z, 1.0],
            albedo: [albedo.x, albedo.y, albedo.z, 1.0],
        };
        let buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("preview-uniforms"),
            contents: bytemuck::bytes_of(&uniforms),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("preview-uniforms"),
            layout: &self.bind_group_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
        })
    }

    /// Clears the targets and draws each triangle set, chunking large sets. Each chunk is
    /// submitted separately so its vertex buffer can be freed before the next upload.
    fn draw_scene(
        &self,
        targets: &Targets,
        scene: &Scene,
        draws: &[(&[Triangle], Vec3, Option<&wgpu::Buffer>)],
    ) -> Result<()> {
        self.draw_scene_chunked(targets, scene, draws, self.triangles_per_chunk as usize)
    }

    fn draw_scene_chunked(
        &self,
        targets: &Targets,
        scene: &Scene,
        draws: &[(&[Triangle], Vec3, Option<&wgpu::Buffer>)],
        triangles_per_chunk: usize,
    ) -> Result<()> {
        let first_pass = std::cell::Cell::new(true);
        let run_pass = |buffer: &wgpu::Buffer, vertex_count: u32, bind_group: &wgpu::BindGroup| {
            let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let (color_load, depth_load) = if first_pass.get() {
                    (wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), wgpu::LoadOp::Clear(1.0))
                } else {
                    (wgpu::LoadOp::Load, wgpu::LoadOp::Load)
                };
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("preview-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &targets.msaa_view,
                        resolve_target: Some(&targets.resolve_view),
                        ops: wgpu::Operations { load: color_load, store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &targets.depth_view,
                        depth_ops: Some(wgpu::Operations { load: depth_load, store: wgpu::StoreOp::Store }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, bind_group, &[]);
                if vertex_count > 0 {
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..vertex_count, 0..1);
                }
            }
            self.queue.submit(Some(encoder.finish()));
            first_pass.set(false);
        };

        for (triangles, albedo, shared) in draws {
            let bind_group = self.uniform_bind_group(scene, *albedo);
            if let Some(buffer) = shared {
                run_pass(buffer, (triangles.len() * 3) as u32, &bind_group);
                continue;
            }
            for chunk in triangles.chunks(triangles_per_chunk.max(1)) {
                let buffer = self.vertex_buffer(chunk);
                run_pass(&buffer, (chunk.len() * 3) as u32, &bind_group);
            }
        }

        if first_pass.get() {
            // Nothing drawn: still clear so the readback is transparent.
            let empty = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: BYTES_PER_VERTEX,
                usage: wgpu::BufferUsages::VERTEX,
                mapped_at_creation: false,
            });
            run_pass(&empty, 0, &self.uniform_bind_group(scene, Vec3::default()));
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn render_chunked_for_test(
        &self,
        body: &[Triangle],
        triangles_per_chunk: usize,
        width: u32,
        height: u32,
        albedo: Vec3,
    ) -> Result<Vec<u8>> {
        let mut targets = self.targets.lock();
        let targets = self.ensure_targets(&mut targets, width, height);
        let scene = self.scene(&[body], width, height);
        self.draw_scene_chunked(targets, &scene, &[(body, albedo, None)], triangles_per_chunk)?;
        self.read_png(targets)
    }

    fn ensure_targets<'a>(&self, slot: &'a mut Option<Targets>, width: u32, height: u32) -> &'a Targets {
        if slot.as_ref().is_some_and(|t| t.width == width && t.height == height) {
            return slot.as_ref().unwrap();
        }

        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let texture = |label, format, samples, usage| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let msaa = texture("preview-msaa", COLOR_FORMAT, SAMPLE_COUNT, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let depth = texture("preview-depth", DEPTH_FORMAT, SAMPLE_COUNT, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let resolve = texture(
            "preview-resolve",
            COLOR_FORMAT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );

        let unpadded = width * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded.div_ceil(align) * align;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("preview-readback"),
            size: (padded_bytes_per_row * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        *slot = Some(Targets {
            width,
            height,
            msaa_view: msaa.create_view(&Default::default()),
            depth_view: depth.create_view(&Default::default()),
            resolve_view: resolve.create_view(&Default::default()),
            resolve,
            readback,
            padded_bytes_per_row,
        });
        slot.as_ref().unwrap()
    }

    fn read_png(&self, targets: &Targets) -> Result<Vec<u8>> {
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &targets.resolve,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &targets.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(targets.padded_bytes_per_row),
                    rows_per_image: Some(targets.height),
                },
            },
            wgpu::Extent3d { width: targets.width, height: targets.height, depth_or_array_layers: 1 },
        );
        self.queue.submit(Some(encoder.finish()));

        let slice = targets.readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device.poll(wgpu::PollType::Wait).map_err(|e| anyhow!("waiting for GPU: {e}"))?;
        rx.recv().context("GPU readback channel closed")?.context("mapping readback buffer")?;

        let row_bytes = (targets.width * 4) as usize;
        let mut pixels = Vec::with_capacity(row_bytes * targets.height as usize);
        {
            let data = slice.get_mapped_range();
            for row in data.chunks(targets.padded_bytes_per_row as usize).take(targets.height as usize) {
                pixels.extend_from_slice(&row[..row_bytes]);
            }
        }
        targets.readback.unmap();

        Ok(encode_png(&pixels, targets.width, targets.height))
    }
}
