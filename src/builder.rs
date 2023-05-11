use crate::{SurfaceTexture, SurfaceSize, renderers::{ScalingRenderer, ScalingMatrix}, ChromaContext, Chroma};

pub struct ChromaBuilder<'win> {
    width: u16,
    height: u16,
    surface_texture: SurfaceTexture<'win>
}

impl<'win> ChromaBuilder<'win> {
    pub fn new(width: u16, height: u16, surface_texture: SurfaceTexture<'win>) -> Self {
        Self {
            width,
            height,
            surface_texture,
        }
    }

    pub fn build(self) -> Chroma {
        pollster::block_on(self.build_impl())
    }

    async fn build_impl(self) -> Chroma {
        // 'instance' created to handle GPU
        // 'Backends::all()' = Vulkan, Metal, DX12, and WebGPU
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            dx12_shader_compiler: Default::default()
        });

        // The 'surface' is the part of the window we draw to
        // 'surface' takes a reference to 'window', so 'window' needs to remain alive as long as 'surface'
        // Since 'State' owns 'window' and 'surface', this should be safe
        let surface = unsafe { instance.create_surface(self.surface_texture.window)}.unwrap();

        // The 'adapter' is a handle to the GPU, can get information on name and backend
        let adapter = instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false
            }
        ).await.unwrap();

        let (device, queue) = adapter.request_device(
            &wgpu::DeviceDescriptor {
                features: wgpu::Features::empty(),
                limits: wgpu::Limits::default(),
                label: None
            }, 
            None
        ).await.unwrap();

        let surface_capabilities = surface.get_capabilities(&adapter);

        let surface_size = self.surface_texture.size;

        let (scaling_matrix_inverse, texture_extent, texture, scaling_renderer, pixels_buffer_size) =
            create_backing_texture(
                &device,
                self.width,
                self.height,
                &surface_size
            );
        
        let mut pixels : Vec<u8> = Vec::with_capacity(pixels_buffer_size);
        pixels.resize_with(pixels_buffer_size, Default::default);

        let alpha_mode = surface_capabilities.alpha_modes[0];

        let context = ChromaContext {
            device,
            queue,
            surface,
            texture,
            texture_extent,
            scaling_renderer
        };

        let pixels = Chroma {
            context,
            adapter,
            surface_size,
            pixels,
            scaling_matrix_inverse,
            alpha_mode
        };
        pixels.reconfigure_surface();
        
        pixels
    }
}

pub fn create_backing_texture(
    device: &wgpu::Device,
    width: u16,
    height: u16,
    surface_size: &SurfaceSize
) -> (
    ultraviolet::Mat4,
    wgpu::Extent3d,
    wgpu::Texture,
    ScalingRenderer,
    usize
) {
    let scaling_matrix_inverse = ScalingMatrix::new(
        (width as f32, height as f32),
        (surface_size.width as f32, surface_size.height as f32)
    ).transform.inversed();

    let texture_extent = wgpu::Extent3d {
        width: width as u32,
        height: height as u32,
        depth_or_array_layers: 1
    };

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("pixels_source_texture"),
        size: texture_extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[]
    });
    let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let scaling_renderer = ScalingRenderer::new(
        device,
        &texture_view,
        &texture_extent,
        surface_size
    );

    let pixels_buffer_size = ((width * height) as f32 * 4.0) as usize;

    (scaling_matrix_inverse, texture_extent, texture, scaling_renderer, pixels_buffer_size)
}