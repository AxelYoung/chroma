use builder::ChromaBuilder;
use renderers::ScalingRenderer;

#[cfg(target_arch="wasm32")]
use wasm_bindgen::prelude::*;

use winit::window::Window;

mod renderers;
mod builder;

pub const WIDTH: u32 = 20;
pub const HEIGHT: u32 = 20;

pub const SCALE: u32 = 4;

pub const TILE_SIZE: u32 = 16;
pub const TILE_DATA_SIZE: usize = (TILE_SIZE * TILE_SIZE * 4) as usize;

pub struct SurfaceSize {
    width: u16,
    height: u16
}

pub struct SurfaceTexture<'win> {
    window: &'win Window,
    size: SurfaceSize
}

impl<'win> SurfaceTexture<'win> {
    pub fn new(width: u16, height: u16, window: &'win Window) -> Self {
        Self {
            window,
            size: SurfaceSize { width, height }
        }
    }
}

pub struct ChromaContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    surface: wgpu::Surface,
    pub texture: wgpu::Texture,
    pub texture_extent: wgpu::Extent3d,
    pub scaling_renderer: ScalingRenderer
}

pub struct Chroma {
    context: ChromaContext,
    surface_size: SurfaceSize,
    alpha_mode: wgpu::CompositeAlphaMode,
    adapter: wgpu::Adapter,
    pixels: Vec<u8>,
    scaling_matrix_inverse: ultraviolet::Mat4
}

impl Chroma {
    pub fn new(width: u16, height: u16, surface_texture: SurfaceTexture<'_>) -> Self {
        ChromaBuilder::new(width, height, surface_texture).build()
    }

    pub fn reconfigure_surface(&self) {
        self.context.surface.configure(
            &self.context.device, 
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                width: self.surface_size.width as u32,
                height: self.surface_size.height as u32,
                present_mode: wgpu::PresentMode::Fifo,
                alpha_mode: self.alpha_mode,
                view_formats: vec![]
            });
    }

    pub fn render(&self) {
        self.render_with(|encoder, render_target, context| {
            context.scaling_renderer.render(encoder, render_target);
        });
    }

    pub fn render_with<F>(&self, render_function: F) 
        where F: FnOnce(
            &mut wgpu::CommandEncoder,
            &wgpu::TextureView,
            &ChromaContext
        ) {
        let frame = self.context.surface.get_current_texture().or_else(|_| {

            self.context.surface.get_current_texture()
        }).unwrap();

        let mut encoder = 
            self.context.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("command_encoder")
                });

        let bytes_per_row = (self.context.texture_extent.width as f32 * 4.0) as u32;
    
        self.context.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.context.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: 0, z: 0 },
                aspect: wgpu::TextureAspect::All
            }, 
            &self.pixels, 
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row as u32),
                rows_per_image: Some(self.context.texture_extent.height as u32)
            }, 
            self.context.texture_extent
        );

        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
    
        (render_function)(&mut encoder, &view, &self.context);

        self.context.queue.submit(Some(encoder.finish()));
        frame.present();
    }

    pub fn frame_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    pub fn frame(&self) -> &[u8] {
        &self.pixels
    }
    
    pub fn sprite_sheet_data(bytes: &[u8]) -> Vec<[u8; TILE_DATA_SIZE]> {
        let sprite_sheet = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).unwrap();
        let rgba_data = sprite_sheet.to_rgba8();
    
        let (width, height) = rgba_data.dimensions();
    
        let x_count = width / TILE_SIZE;
        let y_count = height / TILE_SIZE;
    
        let mut sprite_sheet : Vec<[u8; TILE_DATA_SIZE]> = vec![];
    
        for sprite_y in 0..y_count {
            for sprite_x in 0..x_count {
                let mut pixel_data = [0; TILE_DATA_SIZE];
                let mut is_empty = true;
                for y in 0..TILE_SIZE {
                    for x in 0..TILE_SIZE {
                        let pixel = rgba_data.get_pixel(x + (TILE_SIZE * sprite_x),  y + (TILE_SIZE * sprite_y));
                        for offset in 0..4 {
                            pixel_data[(((y * TILE_SIZE as u32 + x) * 4) + offset) as usize] = pixel[offset as usize];
                        }
                        if pixel[3] != 0 {is_empty = false;}
                    }
                }
                if is_empty {break;}
                else {sprite_sheet.push(pixel_data); }
            }
        }
    
        sprite_sheet
    }
    
    pub fn sprite_data(bytes: &[u8]) -> [u8; TILE_DATA_SIZE] {
        let sprite = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).unwrap();
    
        let rgba_data = sprite.to_rgba8();
    
        let mut pixel_data = [0; TILE_DATA_SIZE];
    
        for y in 0..TILE_SIZE {
            for x in 0..TILE_SIZE {
                let pixel = rgba_data.get_pixel(x, y);
                for offset in 0..4 {
                    pixel_data[(((y * TILE_SIZE as u32 + x) * 4) + offset) as usize] = pixel[offset as usize];
                }
            }
        }
    
        pixel_data
    }
    
    pub fn draw_tiles(&mut self, tilemap: [[u8; 16]; 14], tiles: [[u8; 1024]; 2]) {
        for y in 0..14 {
            for x in 0..16 {
                let index = tilemap[y][x];
                if index == 0 { continue; }
                self.draw_tile(tiles[(index - 1) as usize], x as u32, y as u32);
            }
        }
    }
    
    pub fn draw_tile(&mut self, sprite: [u8; TILE_DATA_SIZE], pos_x: u32, pos_y: u32) {
        self.draw_sprite(sprite, pos_x * TILE_SIZE, pos_y * TILE_SIZE);
    }
    
    pub fn draw_sprite(&mut self, sprite: [u8; TILE_DATA_SIZE], pos_x: u32, pos_y: u32) {
        for y in 0..TILE_SIZE {
            for x in 0..TILE_SIZE {
                let index = ((y * TILE_SIZE) + x) * 4;
                self.draw_pixel(&sprite[index as usize..(index + 4) as usize], x + pos_x, y + pos_y)
            }
        }
    }
    
    pub fn draw_pixel(&mut self, pixel: &[u8], x: u32, y: u32) {
        let index = ((y * WIDTH) + x) * 4;
        if pixel[3] == 0 { return }
        for offset in 0..4 {
            self.pixels[(index + offset) as usize] = pixel[offset as usize];
        }
    }
    
}