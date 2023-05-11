use crate::{SurfaceTexture, SurfaceSize, renderers::{ScalingRenderer, ScalingMatrix}, PixelsContext, Pixels};

pub struct PixelsBuilder<'req, 'dev, 'win> {
    request_adapter_options: Option<wgpu::RequestAdapterOptions<'req>>,
    device_descriptor: Option<wgpu::DeviceDescriptor<'dev>>,
    backend: wgpu::Backends,
    width: u16,
    height: u16,
    present_mode: wgpu::PresentMode,
    surface_texture: SurfaceTexture<'win>,
    texture_format: wgpu::TextureFormat,
    render_texture_format: Option<wgpu::TextureFormat>,
    surface_texture_format: Option<wgpu::TextureFormat>,
    clear_color: wgpu::Color,
    blend_state: wgpu::BlendState
}

impl<'req, 'dev, 'win> PixelsBuilder<'req, 'dev, 'win> {
    pub fn new(width: u16, height: u16, surface_texture: SurfaceTexture<'win>) -> Self {
        Self {
            request_adapter_options: None,
            device_descriptor: None,
            backend: wgpu::util::backend_bits_from_env().unwrap_or_else(wgpu::Backends::all),
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            surface_texture,
            texture_format: wgpu::TextureFormat::Rgba8UnormSrgb,
            render_texture_format: None,
            surface_texture_format: None,
            clear_color: wgpu::Color::WHITE,
            blend_state: wgpu::BlendState::ALPHA_BLENDING
        }
    }

    pub fn build(self) -> Pixels {
        pollster::block_on(self.build_impl())
    }

    async fn build_impl(self) -> Pixels {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: self.backend,
            ..Default::default()
        });

        let surface = unsafe { instance.create_surface(self.surface_texture.window)}.unwrap();
        let compatible_surface = Some(&surface);
        let request_adapter_options = &self.request_adapter_options;
        let adapter = match wgpu::util::initialize_adapter_from_env(&instance, self.backend) {
            Some(adapter) => Some(adapter),
            None => {
                instance.request_adapter(&request_adapter_options.as_ref().map_or_else(
                    || wgpu::RequestAdapterOptions {
                        compatible_surface,
                        force_fallback_adapter: false,
                        power_preference:
                        wgpu::util::power_preference_from_env().unwrap_or_default()
                    },
                    |rao| wgpu::RequestAdapterOptions {
                        compatible_surface: rao.compatible_surface.or(compatible_surface),
                        force_fallback_adapter: rao.force_fallback_adapter,
                        power_preference: rao.power_preference
                    })).await
            }
        };

        let adapter = adapter.unwrap();

        let device_descriptor = self.device_descriptor.unwrap_or_else(
            || wgpu::DeviceDescriptor {
                limits: adapter.limits(),
                ..wgpu::DeviceDescriptor::default()
            }
        );

        let (device, queue) = adapter.request_device(&device_descriptor, None).await.unwrap();

        let surface_capabilities = surface.get_capabilities(&adapter);
        let present_mode = self.present_mode;
        let surface_texture_format = self.surface_texture_format.unwrap_or_else(|| {
            *surface_capabilities.formats.iter().find(|format| format.is_srgb()).unwrap()
        });
        let render_texture_format = self.render_texture_format.unwrap_or(surface_texture_format);

        let surface_size = self.surface_texture.size;
        let clear_color = self.clear_color;
        let blend_state = self.blend_state;
        let (scaling_matrix_inverse, texture_extent, texture, scaling_renderer, pixels_buffer_size) =
            create_backing_texture(
                &device,
                self.width,
                self.height,
                self.texture_format,
                &surface_size,
                render_texture_format,
                clear_color,
                blend_state
            );
        
        let mut pixels : Vec<u8> = Vec::with_capacity(pixels_buffer_size);
        pixels.resize_with(pixels_buffer_size, Default::default);

        let alpha_mode = surface_capabilities.alpha_modes[0];

        let context = PixelsContext {
            device,
            queue,
            surface,
            texture,
            texture_extent,
            texture_format: self.texture_format,
            texture_format_size: texture_format_size(self.texture_format),
            scaling_renderer
        };

        let pixels = Pixels {
            context,
            adapter,
            surface_size,
            present_mode,
            render_texture_format,
            surface_texture_format,
            blend_state,
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
    backing_texture_format: wgpu::TextureFormat,
    surface_size: &SurfaceSize,
    render_texture_format: wgpu::TextureFormat,
    clear_color: wgpu::Color,
    blend_state: wgpu::BlendState
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
        format: backing_texture_format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[]
    });
    let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let scaling_renderer = ScalingRenderer::new(
        device,
        &texture_view,
        &texture_extent,
        surface_size,
        render_texture_format,
        clear_color,
        blend_state
    );

    let texture_format_size = texture_format_size(backing_texture_format);
    let pixels_buffer_size = ((width * height) as f32 * texture_format_size) as usize;

    (scaling_matrix_inverse, texture_extent, texture, scaling_renderer, pixels_buffer_size)
}

const fn texture_format_size(texture_format: wgpu::TextureFormat) -> f32 {
    use wgpu::{AstcBlock::*, TextureFormat::*};

    // TODO: Use constant arithmetic when supported.
    // See: https://github.com/rust-lang/rust/issues/57241
    match texture_format {
        // Note that these sizes are typically estimates. For instance, GPU vendors decide whether
        // their implementation uses 5 or 8 bytes per texel for formats like `Depth32PlusStencil8`.
        // In cases where it is unclear, we choose to overestimate.
        //
        // See:
        // - https://gpuweb.github.io/gpuweb/#plain-color-formats
        // - https://gpuweb.github.io/gpuweb/#depth-formats
        // - https://gpuweb.github.io/gpuweb/#packed-formats

        // 8-bit formats, 8 bits per component
        R8Unorm
        | R8Snorm
        | R8Uint
        | R8Sint
        | Stencil8 => 1.0, // 8.0 / 8.0

        // 16-bit formats, 8 bits per component
        R16Uint
        | R16Sint
        | R16Float
        | R16Unorm
        | R16Snorm
        | Rg8Unorm
        | Rg8Snorm
        | Rg8Uint
        | Rg8Sint
        | Rgb9e5Ufloat
        | Depth16Unorm => 2.0, // 16.0 / 8.0

        // 32-bit formats, 8 bits per component
        R32Uint
        | R32Sint
        | R32Float
        | Rg16Uint
        | Rg16Sint
        | Rg16Float
        | Rg16Unorm
        | Rg16Snorm
        | Rgba8Unorm
        | Rgba8UnormSrgb
        | Rgba8Snorm
        | Rgba8Uint
        | Rgba8Sint
        | Bgra8Unorm
        | Bgra8UnormSrgb
        | Rgb10a2Unorm
        | Rg11b10Float
        | Depth32Float
        | Depth24Plus
        | Depth24PlusStencil8 => 4.0, // 32.0 / 8.0

        // 64-bit formats, 8 bits per component
        Rg32Uint
        | Rg32Sint
        | Rg32Float
        | Rgba16Uint
        | Rgba16Sint
        | Rgba16Float
        | Rgba16Unorm
        | Rgba16Snorm
        | Depth32FloatStencil8 => 8.0, // 64.0 / 8.0

        // 128-bit formats, 8 bits per component
        Rgba32Uint
        | Rgba32Sint
        | Rgba32Float => 16.0, // 128.0 / 8.0

        // Compressed formats

        // 4x4 blocks, 8 bytes per block
        Bc1RgbaUnorm
        | Bc1RgbaUnormSrgb
        | Bc4RUnorm
        | Bc4RSnorm
        | Etc2Rgb8Unorm
        | Etc2Rgb8UnormSrgb
        | Etc2Rgb8A1Unorm
        | Etc2Rgb8A1UnormSrgb
        | EacR11Unorm
        | EacR11Snorm => 0.5, // 4.0 * 4.0 / 8.0

        // 4x4 blocks, 16 bytes per block

        // 5x4 blocks, 16 bytes per block
        Astc { block: B5x4, channel: _ } => 1.25, // 5.0 * 4.0 / 16.0

        // 5x5 blocks, 16 bytes per block
        Astc { block: B5x5, channel: _ } => 1.5625, // 5.0 * 5.0 / 16.0

        // 6x5 blocks, 16 bytes per block
        Astc { block: B6x5, channel: _ } => 1.875, // 6.0 * 5.0 / 16.0

        // 6x6 blocks, 16 bytes per block
        Astc { block: B6x6, channel: _ } => 2.25, // 6.0 * 6.0 / 16.0

        // 8x5 blocks, 16 bytes per block
        Astc { block: B8x5, channel: _ } => 2.5, // 8.0 * 5.0 / 16.0

        // 8x6 blocks, 16 bytes per block
        Astc { block: B8x6, channel: _ } => 3.0, // 8.0 * 6.0 / 16.0

        // 8x8 blocks, 16 bytes per block
        Astc { block: B8x8, channel: _ } => 4.0, // 8.0 * 8.0 / 16.0

        // 10x5 blocks, 16 bytes per block
        Astc { block: B10x5, channel: _ } => 3.125, // 10.0 * 5.0 / 16.0

        // 10x6 blocks, 16 bytes per block
        Astc { block: B10x6, channel: _ } => 3.75, // 10.0 * 6.0 / 16.0

        // 10x8 blocks, 16 bytes per block
        Astc { block: B10x8, channel: _ } => 5.0, // 10.0 * 8.0 / 16.0

        // 10x10 blocks, 16 bytes per block
        Astc { block: B10x10, channel: _ } => 6.25, // 10.0 * 10.0 / 16.0

        // 12x10 blocks, 16 bytes per block
        Astc { block: B12x10, channel: _ } => 7.5, // 12.0 * 10.0 / 16.0

        // 12x12 blocks, 16 bytes per block
        Astc { block: B12x12, channel: _ } => 9.0, // 12.0 * 12.0 / 16.0
        _ => 1.0,
    }
}