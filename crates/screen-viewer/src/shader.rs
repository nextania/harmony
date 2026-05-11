use std::fmt::Debug;
use std::fmt::Formatter;
use std::sync::Arc;

use arc_swap::ArcSwap;
use iced::Rectangle;
use iced::wgpu;

use iced::widget::shader::Pipeline;
use iced::widget::shader::Primitive;
use iced::widget::shader::Program;
use iced::widget::shader::Viewport;
use tracing::debug;
use wgpu_capture::{CaptureFrame, WgpuImporter};

pub struct ScreenProgram {
    frame: Arc<ArcSwap<Option<CaptureFrame>>>,
}

impl ScreenProgram {
    pub fn new(frame: Arc<ArcSwap<Option<CaptureFrame>>>) -> Self {
        ScreenProgram { frame }
    }
}

impl<Message> Program<Message> for ScreenProgram {
    type State = ();
    type Primitive = ScreenPrimitive;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: iced::mouse::Cursor,
        _bounds: Rectangle,
    ) -> Self::Primitive {
        ScreenPrimitive {
            frame: Arc::clone(&self.frame),
        }
    }
}

pub struct ScreenPrimitive {
    pub frame: Arc<ArcSwap<Option<CaptureFrame>>>,
}

impl Debug for ScreenPrimitive {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScreenPrimitive").finish_non_exhaustive()
    }
}

impl Primitive for ScreenPrimitive {
    type Pipeline = ScreenPipeline;

    fn prepare(
        &self,
        pipeline: &mut ScreenPipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _bounds: &Rectangle,
        _viewport: &Viewport,
    ) {
        let maybe_frame = self.frame.load_full();

        let capture_frame = match maybe_frame.as_ref().as_ref() {
            Some(f) => f,
            None => return,
        };

        let frame_id = capture_frame.frame_id();

        // Same CaptureFrame clone as last prepare — nothing to do.
        if pipeline.last_imported_frame_id == Some(frame_id) {
            return;
        }

        let desc = wgpu::TextureDescriptor {
            label: Some("capture_frame"),
            size: wgpu::Extent3d {
                width: capture_frame.width(),
                height: capture_frame.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        };

        match pipeline
            .importer
            .import(capture_frame, device, queue, &desc)
        {
            Ok(texture) => {
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("capture_bg"),
                    layout: &pipeline.bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&pipeline.sampler),
                        },
                    ],
                });
                pipeline.current_view = Some(view);
                pipeline.current_bind_group = Some(bind_group);
                pipeline.current_texture = Some(texture);
                pipeline.last_imported_frame_id = Some(frame_id);
            }
            Err(e) => {
                debug!("import frame: {e}");
            }
        }
    }

    fn render(
        &self,
        pipeline: &ScreenPipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        _clip_bounds: &Rectangle<u32>,
    ) {
        let bind_group = match pipeline.current_bind_group.as_ref() {
            Some(bg) => bg,
            None => return,
        };

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("screen_viewer_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        render_pass.set_pipeline(&pipeline.render_pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        render_pass.draw(0..6, 0..1);
    }
}

pub struct ScreenPipeline {
    importer: WgpuImporter,
    render_pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    current_bind_group: Option<wgpu::BindGroup>,
    current_view: Option<wgpu::TextureView>,
    current_texture: Option<wgpu::Texture>,
    last_imported_frame_id: Option<usize>,
}

impl Pipeline for ScreenPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let importer = WgpuImporter::new(device).expect("WgpuImporter::new");

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("screen_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("screen_pl"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("screen_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_WGSL.into()),
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("screen_rp"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("screen_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        ScreenPipeline {
            importer,
            render_pipeline,
            bind_group_layout,
            sampler,
            current_bind_group: None,
            current_view: None,
            current_texture: None,
            last_imported_frame_id: None,
        }
    }
}

const SHADER_WGSL: &str = r#"
@group(0) @binding(0) var t_capture: texture_2d<f32>;
@group(0) @binding(1) var s_capture: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(-1.0,  1.0),
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>( 1.0,  1.0),
    );
    var uvs = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(1.0, 0.0),
    );
    var out: VertexOutput;
    out.position = vec4<f32>(positions[idx], 0.0, 1.0);
    out.uv = uvs[idx];
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(t_capture, s_capture, in.uv);
    // IMPORTANT: we must override the alpha to 1.0, otherwise the output will be transparent
    return vec4<f32>(color.rgb, 1.0);
}
"#;
