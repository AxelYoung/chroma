use std::{iter, io::BufRead};

use wgpu::util::DeviceExt;
use winit::{
    event::*,
    event_loop::{ControlFlow, EventLoop},
    window::{Window, WindowBuilder},
};

use cgmath::prelude::*;


#[cfg(target_arch="wasm32")]
use wasm_bindgen::prelude::*;

mod texture;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    tex_coords: [f32; 2],
}

impl Vertex {
    fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        use std::mem;
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}

const SPRITE_COUNT: u8 = 5;

const SCREEN_WIDTH: u32 = 128;
const SCREEN_HEIGHT: u32= 112;

const VERTICES: &[Vertex] = &[
    Vertex {
        position: [0.0 / SCREEN_WIDTH as f32 - 2.0, 32.0 / SCREEN_HEIGHT as f32 - 2.0, 0.0],
        tex_coords: [0.0, 0.0],
    }, 
    Vertex {
        position: [0.0 / SCREEN_WIDTH as f32 - 2.0, 0.0 / SCREEN_HEIGHT as f32 - 2.0, 0.0],
        tex_coords: [0.0, 1.0],
    }, 
    Vertex {
        position: [32.0 / SCREEN_WIDTH as f32 - 2.0, 0.0 / SCREEN_HEIGHT as f32 - 2.0, 0.0],
        tex_coords: [1.0 / SPRITE_COUNT as f32, 1.0],
    }, 
    Vertex {
        position: [32.0 / SCREEN_WIDTH as f32 - 2.0, 32.0 / SCREEN_HEIGHT as f32 - 2.0, 0.0],
        tex_coords: [1.0 / SPRITE_COUNT as f32, 0.0],
    }, 
    // -2,-2 to 2,2 => 0,0 to 128, 112
];

const INDICES: &[u16] = &[0, 1, 2, 0, 2, 3];

pub struct Chroma {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pub window_size: winit::dpi::PhysicalSize<u32>,
    render_pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    indices_count: u32,
    diffuse_bind_group: wgpu::BindGroup,
    window: Window,
    surface: wgpu::Surface,
    config: wgpu::SurfaceConfiguration,
    texture: wgpu::Texture,
    texture_view: wgpu::TextureView,
    upscale_pipeline: wgpu::RenderPipeline,
    upscale_bind_group: wgpu::BindGroup,
    upscale_vertex_buffer: wgpu::Buffer,
    clip_rect: (u32, u32, u32, u32),
    instances: Vec<Instance>,
    instance_buffer: wgpu::Buffer,
    update_instance: bool
}

impl Chroma {
    pub async fn new(pixel_width: u32, pixel_height: u32, window: Window) -> Self {
        let window_size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            dx12_shader_compiler: Default::default(),
        });

        let surface = unsafe { instance.create_surface(&window)}.unwrap();
        
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: None,
                    features: wgpu::Features::empty(),
                    limits: if cfg!(target_arch = "wasm32") {
                        wgpu::Limits::downlevel_webgl2_defaults()
                    } else {
                        wgpu::Limits::default()
                    },
                },
                None,
        ).await.unwrap();

        let (render_pipeline, vertex_buffer, index_buffer, indices_count, diffuse_bind_group, texture, texture_view, instance_buffer, instances) = 
        Chroma::create_pixel_renderer(pixel_width, pixel_height, &device, &queue);

        let (config, upscale_pipeline, upscale_vertex_buffer, upscale_bind_group, clip_rect) = 
        Chroma::create_upscale_renderer(&surface, &adapter, &device, window_size, &texture_view, pixel_width, pixel_height);

        Self {
            device,
            queue,
            window_size,
            window,
            surface,
            config,
            
            render_pipeline,
            vertex_buffer,
            index_buffer,
            indices_count,
            diffuse_bind_group,
            texture,
            texture_view,

            upscale_pipeline,
            upscale_bind_group,
            upscale_vertex_buffer,
            clip_rect,
            instance_buffer,
            instances,
            update_instance: false
        }
    }

    pub fn window(&self) -> &Window {
        &self.window
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.window_size = new_size;
            self.surface.configure(&self.device, &self.config);
        }
    }

    pub fn render(&mut self) -> Result<(), wgpu::SurfaceError> {

        if self.update_instance { self.configure_instances(); }

        let mut encoder = self.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder")
            }
        );

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.texture_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.2,
                            b: 0.3,
                            a: 1.0,
                        }),
                        store: true,
                    },
                })],
                depth_stencil_attachment: None,
            });

            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_bind_group(0, &self.diffuse_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

            render_pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

            render_pass.draw_indexed(0..self.indices_count, 0, 0..self.instances.len() as u32);
        }

        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scaling_renderer_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: true
                    }
                })],
                depth_stencil_attachment: None
            });
            
            rpass.set_pipeline(&self.upscale_pipeline);
            rpass.set_bind_group(0, &self.upscale_bind_group, &[]);
            rpass.set_vertex_buffer(0, self.upscale_vertex_buffer.slice(..));
            rpass.set_scissor_rect(self.clip_rect.0, self.clip_rect.1, self.clip_rect.2, self.clip_rect.3);

            rpass.draw(0..3, 0..1);
        }

        self.queue.submit(iter::once(encoder.finish()));
        output.present();

        Ok(())
    }

    fn create_pixel_renderer(width: u32, height: u32, device: &wgpu::Device, queue: &wgpu::Queue) ->
    (wgpu::RenderPipeline, wgpu::Buffer, wgpu::Buffer, u32, wgpu::BindGroup, wgpu::Texture, wgpu::TextureView, wgpu::Buffer, Vec<Instance>) {
        let texture_desc = wgpu::TextureDescriptor {
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            label: None,
            view_formats: &[],
        };
        let texture = device.create_texture(&texture_desc);
        let texture_view = texture.create_view(&Default::default());

        let diffuse_bytes = include_bytes!("../img/sprite_sheet.png");
        let diffuse_texture =
            texture::Texture::from_bytes(&device, &queue, diffuse_bytes, "sprite_sheet.png").unwrap();

        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
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
                label: Some("texture_bind_group_layout"),
            });

        let diffuse_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&diffuse_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&diffuse_texture.sampler),
                },
            ],
            label: Some("diffuse_bind_group"),
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/shader.wgsl").into()),
        });

        let instances = vec![

        ];

        let instance_data = instances.iter().map(Instance::to_raw).collect::<Vec<_>>();
        let instance_buffer = device.create_buffer_init(
                &wgpu::util::BufferInitDescriptor {
                    label: Some("Instance Buffer"),
                    contents: bytemuck::cast_slice(&instance_data),
                    usage: wgpu::BufferUsages::VERTEX,
                }
            );


        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[&texture_bind_group_layout],
                push_constant_ranges: &[],
            });

            let render_pipeline = device.create_render_pipeline(
                &wgpu::RenderPipelineDescriptor {
                label: Some("Render Pipeline"),
                layout: Some(&render_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: "vs_main",
                    buffers: &[Vertex::desc(), InstanceRaw::desc()]
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: "fs_main",
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8UnormSrgb,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL
                    })]
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: Some(wgpu::Face::Back),
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState {
                    count: 1,
                    mask: !0,
                    alpha_to_coverage_enabled: false
                },
                multiview: None
            }
        );

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Index Buffer"),
            contents: bytemuck::cast_slice(INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });

        let indices_count = INDICES.len() as u32;

        (render_pipeline, vertex_buffer, index_buffer, indices_count, diffuse_bind_group, texture, texture_view, instance_buffer, instances)
    }

    fn create_upscale_renderer(surface: &wgpu::Surface, adapter: &wgpu::Adapter, device: &wgpu::Device, window_size: winit::dpi::PhysicalSize<u32>,
    texture_view: &wgpu::TextureView, pixel_width: u32, pixel_height: u32) -> (wgpu::SurfaceConfiguration,
    wgpu::RenderPipeline, wgpu::Buffer, wgpu::BindGroup, (u32, u32, u32, u32)) {
        let surface_capabilities = surface.get_capabilities(&adapter);

        let surface_format = surface_capabilities.formats.iter()
            .copied()
            .filter(|f| f.is_srgb())
            .next()
            .unwrap_or(surface_capabilities.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: window_size.width,
            height: window_size.height,
            present_mode: surface_capabilities.present_modes[0],
            alpha_mode: surface_capabilities.alpha_modes[0],
            view_formats: vec![]
        };

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
            (pixel_width as f32, pixel_height as f32),
            (window_size.width as f32, window_size.height as f32)
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
                    resource: wgpu::BindingResource::TextureView(&texture_view)
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

        surface.configure(&device, &config);

        (config, render_pipeline, vertex_buffer, bind_group, clip_rect)
    }

    pub fn configure_instances(&mut self) {
        let instance_data = self.instances.iter().map(Instance::to_raw).collect::<Vec<_>>();
        self.instance_buffer = self.device.create_buffer_init(
                &wgpu::util::BufferInitDescriptor {
                    label: Some("Instance Buffer"),
                    contents: bytemuck::cast_slice(&instance_data),
                    usage: wgpu::BufferUsages::VERTEX,
                }
            );
        self.update_instance = false;
    }

    pub fn add_tile(&mut self, position: cgmath::Vector2<f32>, index: u32) {
        self.instances.push(
            Instance { 
                position: cgmath::Vector2 {
                    x: position.x * 2.0 / SCREEN_WIDTH as f32,
                    y: position.y * 2.0 / SCREEN_HEIGHT as f32
                },
                uv_offset: cgmath::Vector2 {
                    x: index as f32 / SPRITE_COUNT as f32,
                    y: 0.0
                }
            }
        );
        self.update_instance = true;
    }

    pub fn move_tile(&mut self, position: cgmath::Vector2<f32>, index: u32) {
        self.instances[index as usize].position = cgmath::Vector2 {
            x: position.x * 2.0 / SCREEN_WIDTH as f32,
            y: position.y * 2.0 / SCREEN_HEIGHT as f32
        };
        self.update_instance = true;
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

struct Instance {
    position: cgmath::Vector2<f32>,
    uv_offset: cgmath::Vector2<f32> 
}

impl Instance {
    fn to_raw(&self) -> InstanceRaw {
        InstanceRaw {
            model: [self.position.x, self.position.y, self.uv_offset.x, self.uv_offset.y]
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct InstanceRaw {
    model: [f32; 4],
}

impl InstanceRaw {
    fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        use std::mem;
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<InstanceRaw>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    shader_location: 6,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}
