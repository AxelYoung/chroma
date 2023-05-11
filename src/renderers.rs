use wgpu::util::DeviceExt;

use crate::SurfaceSize;

// Scales the render texture to the goal screen size,
pub struct ScalingRenderer {
    // Holds the vertices that will be used to draw the screen quad
    vertex_buffer: wgpu::Buffer,
    // Holds uniform data to be used to draw screen quad
    uniform_buffer: wgpu::Buffer,
    // The bind group, which describes the resources the shader can access
    bind_group: wgpu::BindGroup,
    render_pipeline: wgpu::RenderPipeline,
    // Width of screen
    width: f32,
    // Height of screen
    height: f32,
    clip_rect: (u32, u32, u32, u32)
}

impl ScalingRenderer {
    pub fn new(
        device: &wgpu::Device,
        texture_view: &wgpu::TextureView,
        texture_size: &wgpu::Extent3d,
        surface_size: &SurfaceSize
    ) -> Self {
        let shader = wgpu::include_wgsl!("../shaders/scale.wgsl");
        let module = device.create_shader_module(shader);

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("scaling_renderer"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            lod_min_clamp: 0.0,
            lod_max_clamp: 1.0,
            compare: None,
            anisotropy_clamp: 1,
            border_color: None
        });

        // This is just one giant ass triangle
        // See: https://github.com/parasyte/pixels/issues/180
        let vertex_data: [[f32; 2]; 3] = [
            [-1.0, -1.0],
            [3.0, -1.0],
            [-1.0, 3.0]
        ];

        let vertex_slice = bytemuck::cast_slice(&vertex_data);
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("scaling_renderer_vertex_buffer"),
            contents: vertex_slice,
            usage: wgpu::BufferUsages::VERTEX
        });
        let vertex_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: vertex_slice.len() as u64 / vertex_data.len() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 0,
                shader_location: 0
            }]
        };

        let matrix = ScalingMatrix::new(
            (texture_size.width as f32, texture_size.height as f32),
            (surface_size.width as f32, surface_size.height as f32)
        );

        let transform_bytes = matrix.as_bytes();

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("scaling_renderer_bind_matrix_uniform_buffer"),
            contents: transform_bytes,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scaling_renderer_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { 
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2, 
                        multisampled: false 
                    },
                    count: None
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer { 
                        ty: wgpu::BufferBindingType::Uniform, 
                        has_dynamic_offset: false, 
                        min_binding_size: wgpu::BufferSize::new(transform_bytes.len() as u64) 
                    },
                    count: None
                }
            ]
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scaling_renderer_bind_group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(texture_view)
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler)
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform_buffer.as_entire_binding()
                }
            ]
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scaling_renderer_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[]
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scaling_renderer_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { 
                module: &module, 
                entry_point: "vs_main", 
                buffers: &[vertex_buffer_layout]
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL
                })]
            }),
            multiview: None
        });

        let clip_rect = matrix.clip_rect();

        Self {
            vertex_buffer,
            uniform_buffer,
            bind_group,
            render_pipeline,
            width: texture_size.width as f32,
            height: texture_size.height as f32,
            clip_rect
        }
    }

    pub fn render(&self, encoder: &mut wgpu::CommandEncoder, render_target: &wgpu::TextureView) {
        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scaling_renderer_render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: render_target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                    store: true
                }
            })],
            depth_stencil_attachment: None
        });
        
        rpass.set_pipeline(&self.render_pipeline);
        rpass.set_bind_group(0, &self.bind_group, &[]);
        rpass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        rpass.set_scissor_rect(self.clip_rect.0, self.clip_rect.1, self.clip_rect.2, self.clip_rect.3);

        rpass.draw(0..3, 0..1);
    }
 }

pub struct ScalingMatrix {
    pub transform: ultraviolet::Mat4,
    clip_rect: (u32, u32, u32, u32)
}

impl ScalingMatrix {
    pub fn new(texture_size: (f32, f32), screen_size: (f32, f32)) -> Self {
        let (texture_width, texture_height) = texture_size;
        let (screen_width, screen_height) = screen_size;

        let width_ratio = screen_width / texture_width;
        let height_ratio = screen_height / texture_height;

        let scale = width_ratio.clamp(1.0, height_ratio).floor();

        let scaled_width = scale * texture_width;
        let scaled_height = scale * texture_height;

        // Matrixes, how tf do they work, nobody knows
        let sw = scaled_width / screen_width;
        let sh = scaled_height / screen_height;

        let tx = (screen_width / 2.0).fract() / screen_width;
        let ty = (screen_height / 2.0).fract() / screen_height;

        let transform: [f32; 16] = [
            sw, 0.0, 0.0, 0.0,
            0.0, sh, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            tx, ty,   0.0, 1.0
        ];

        let clip_rect = {
            let scaled_width = scaled_width.min(screen_width);
            let scaled_height = scaled_height.min(screen_height);

            let x = ((screen_width - scaled_width) / 2.0) as u32;
            let y = ((screen_height - scaled_height) / 2.0) as u32;

            (x, y, scaled_width as u32, scaled_height as u32)
        };

        Self {
            transform: ultraviolet::Mat4::from(transform),
            clip_rect
        }
    }

    fn as_bytes(&self) -> &[u8] {
        self.transform.as_byte_slice()
    }

    pub fn clip_rect(&self) -> (u32, u32, u32, u32) {
        self.clip_rect
    }
}