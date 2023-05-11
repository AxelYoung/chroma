use builder::PixelsBuilder;
use renderers::ScalingRenderer;

use image::Rgba;

#[cfg(target_arch="wasm32")]
use wasm_bindgen::prelude::*;

use image::GenericImageView;
use winit::{
    event::*,
    event_loop::{ControlFlow, EventLoop},
    window::{Window, WindowBuilder}, dpi::PhysicalPosition,
};

use winit::dpi::PhysicalSize;

mod renderers;
mod builder;

pub const WIDTH: u32 = 256;
pub const HEIGHT: u32 = 224;

pub const SCALE: u32 = 4;

pub const TILE_SIZE: u32 = 16;
pub const TILE_DATA_SIZE: usize = (TILE_SIZE * TILE_SIZE * 4) as usize;

const SENTINEL_SPRITE: &[u8] = include_bytes!("img/sentinel.png");
const STONE_SPRITE: &[u8] = include_bytes!("img/stone.png");
const GRASS_SPRITE: &[u8] = include_bytes!("img/grass.png");

const TILEMAP: [[u8; 16]; 14] = [
    [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
    [1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1],
    [1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1],
    [1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1],
    [1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1],
    [1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1],
    [1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1],
    [1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1],
    [1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1],
    [1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1],
    [1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1],
    [1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1],
    [1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1],
    [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]
];

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

pub struct PixelsContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    surface: wgpu::Surface,
    pub texture: wgpu::Texture,
    pub texture_extent: wgpu::Extent3d,
    pub texture_format: wgpu::TextureFormat,
    pub texture_format_size: f32,
    pub scaling_renderer: ScalingRenderer
}

pub struct Pixels {
    context: PixelsContext,
    surface_size: SurfaceSize,
    present_mode: wgpu::PresentMode,
    render_texture_format: wgpu::TextureFormat,
    surface_texture_format: wgpu::TextureFormat,
    blend_state: wgpu::BlendState,
    alpha_mode: wgpu::CompositeAlphaMode,
    adapter: wgpu::Adapter,
    pixels: Vec<u8>,
    scaling_matrix_inverse: ultraviolet::Mat4
}

impl Pixels {

    pub fn new(width: u16, height: u16, surface_texture: SurfaceTexture<'_>) -> Self {
        PixelsBuilder::new(width, height, surface_texture).build()
    }

    pub fn reconfigure_surface(&self) {
        self.context.surface.configure(
            &self.context.device, 
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: self.surface_texture_format,
                width: self.surface_size.width as u32,
                height: self.surface_size.height as u32,
                present_mode: self.present_mode,
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
            &PixelsContext
        ) {
        let frame = self.context.surface.get_current_texture().or_else(|_| {

            self.context.surface.get_current_texture()
        }).unwrap();

        let mut encoder = 
            self.context.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("command_encoder")
                });

        let bytes_per_row = (self.context.texture_extent.width as f32 * self.context.texture_format_size) as u32;
    
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
}

#[cfg_attr(target_arch="wasm32", wasm_bindgen(start))]
pub fn run() {
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            std::panic::set_hook(Box::new(console_error_panic_hook::hook));
            console_log::init_with_level(log::Level::Warn).expect("Couldn't initialize logger");
        } else {
            env_logger::init();
        }
    }    

    let event_loop = EventLoop::new();

    let window = WindowBuilder::new()
        .with_title("Snake")
        .with_inner_size(PhysicalSize { width: WIDTH * SCALE, height: HEIGHT * SCALE})
        .with_resizable(false)
        .build(&event_loop)
        .unwrap();
    
    #[cfg(target_arch = "wasm32")] {
        // Winit prevents sizing with CSS, so we have to set
        // the size manually when on web.
        use winit::dpi::PhysicalSize;
        window.set_inner_size(PhysicalSize::new(WIDTH * SCALE, HEIGHT * SCALE));
        
        use winit::platform::web::WindowExtWebSys;
        web_sys::window()
            .and_then(|win| win.document())
            .and_then(|doc| {
                let dst = doc.get_element_by_id("wasm-example")?;
                let canvas = web_sys::Element::from(window.canvas());
                dst.append_child(&canvas).ok()?;
                Some(())
            })
            .expect("Couldn't append canvas to document body.");
    }

    let mut pixels = {
        let window_size = window.inner_size();
        let surface_texture = SurfaceTexture::new(window_size.width as u16, window_size.height as u16, &window);
        Pixels::new(WIDTH as u16, HEIGHT as u16, surface_texture)
    };

    let stone_data = sprite_data(STONE_SPRITE);
    let grass_data = sprite_data(GRASS_SPRITE);

    let tiles = [stone_data, grass_data];

    let sentinel_data = sprite_sheet_data(SENTINEL_SPRITE);

    event_loop.run(move |event, _, control_flow| match event {
        Event::RedrawRequested(window_id) => {
            
        },
        Event::MainEventsCleared => {
            let screen = pixels.frame_mut();
            draw_tiles(TILEMAP, tiles, screen);
            draw_sprite(sentinel_data[0], WIDTH / 2, HEIGHT / 2, screen);
            pixels.render();
        },
        Event::WindowEvent {
            window_id,
            ref event,
        } => {
            match event {
                WindowEvent::CloseRequested
                | WindowEvent::KeyboardInput {
                    input:
                        KeyboardInput {
                            state: ElementState::Pressed,
                            virtual_keycode: Some(VirtualKeyCode::Escape),
                            ..
                        },
                    ..
                } => *control_flow = ControlFlow::Exit,
                WindowEvent::Resized(physical_size) => {

                },
                WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {

                },
                _ => {}
            }
        },
        _ => {}
    });
}

fn draw_tiles(tilemap: [[u8; 16]; 14], tiles: [[u8; 1024]; 2], screen: &mut [u8]) {
    for y in 0..14 {
        for x in 0..16 {
            let index = tilemap[y][x];
            if index == 0 { continue; }
            draw_tile(tiles[(index - 1) as usize], x as u32, y as u32, screen);
        }
    }
}

fn sprite_sheet_data(bytes: &[u8]) -> Vec<[u8; TILE_DATA_SIZE]> {
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

fn sprite_data(bytes: &[u8]) -> [u8; TILE_DATA_SIZE] {
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

fn draw_tile(sprite: [u8; TILE_DATA_SIZE], pos_x: u32, pos_y: u32, screen: &mut [u8]) {
    draw_sprite(sprite, pos_x * TILE_SIZE, pos_y * TILE_SIZE, screen);
}

fn draw_sprite(sprite: [u8; TILE_DATA_SIZE], pos_x: u32, pos_y: u32, screen: &mut [u8]) {
    for y in 0..TILE_SIZE {
        for x in 0..TILE_SIZE {
            let index = ((y * TILE_SIZE) + x) * 4;
            draw_pixel(&sprite[index as usize..(index + 4) as usize], x + pos_x, y + pos_y, screen)
        }
    }
}

fn draw_pixel(pixel: &[u8], x: u32, y: u32, screen: &mut [u8]) {
    let index = ((y * WIDTH) + x) * 4;
    if pixel[3] == 0 { return }
    for offset in 0..4 {
        screen[(index + offset) as usize] = pixel[offset as usize];
    }
}
