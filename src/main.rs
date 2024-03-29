// #![allow(unreachable_code)]
// #![allow(unused)]
// #![allow(dead_code)]

use std::time::Duration;
use std::fs::OpenOptions;

extern crate sdl2;

use sdl2::Sdl;
use sdl2::pixels::{PixelFormatEnum, Color};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::video::Window;
use sdl2::render::{Texture, TextureCreator, TextureAccess};

mod decoder;

use decoder::{ImageDecoder, QOIHeader};

fn create_window(sdl: &Sdl) -> Window {
    let video_subsystem = sdl.video().unwrap();

    video_subsystem.window("QOI Viewer", 1600, 900)
        .position_centered()
        .maximized()
        .resizable()
        .build()
        .unwrap()
}

fn gen_texture<'a, T: 'a>(crt: &'a TextureCreator<T>) -> Texture<'a> {
    let path = "qoi_test_images/kodim23.qoi";

    let file = OpenOptions::new()
        .read(true)
        .open(path)
        .expect(format!("Failed to open file: \"{}\"", path).as_str());

    let dec = ImageDecoder::new(file).unwrap();

    let &QOIHeader { width, height, channels, .. } = dec.header();

    let pixels = dec
        .chunks_iter()
        .map(Result::unwrap)
        .flat_map(|p| if channels == 3 {
            p.to_channels3_iter()
        } else {
            p.to_channels4_iter()
        })
        .collect::<Vec<_>>();

    let format = if channels == 3 {
        PixelFormatEnum::RGB24
    } else {
        PixelFormatEnum::RGBA32
    };

    let mut tex = crt
        .create_texture(
            format,
            TextureAccess::Static,
            width, height)
        .expect("Failed to create texture");

    tex.update(None, &pixels[..], (width as usize) * (channels as usize)).unwrap();

    tex
} 

pub fn main() {
    let sdl_context = sdl2::init().unwrap();

    let mut canvas = create_window(&sdl_context)
        .into_canvas()
        .build()
        .unwrap();

    canvas.set_draw_color(Color::RGB(0, 255, 0));

    let mut event_pump = sdl_context.event_pump().unwrap();
    let mut running = true;

    let crt = canvas.texture_creator();
    let texture = gen_texture(&crt);

    while running {
        canvas.clear();
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit {..} |
                Event::KeyDown { keycode: Some(Keycode::Escape | Keycode::Q), .. } => {
                    running = false;
                    break;
                },
                _ => {}
            }
        }

        canvas.copy(&texture, None, None).unwrap();

        canvas.present();
        ::std::thread::sleep(Duration::new(0, 1_000_000_000u32 / 60));
    }
}

