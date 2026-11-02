use anyhow::{bail, Context, Result};
use image::imageops::{self, FilterType};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use ort::{session::Session, value::Tensor};
use serde::Deserialize;
use std::io::Cursor;
use std::path::Path;

const MODEL_SIZE: u32 = 1024;

/// What the user picked as a background. The UI sends this as JSON,
/// e.g. { "kind": "color", "hex": "#ff0000" }
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Background {
    Transparent,
    Color { hex: String },
    Image { path: String },
}

pub fn load_session(model_path: &Path) -> Result<Session> {
    Session::builder()?
        .commit_from_file(model_path)
        .context("Could not load the ONNX model")
}

pub fn remove_background(session: &mut Session, input_path: &Path) -> Result<Vec<u8>> {
    let img = image::open(input_path)
        .with_context(|| format!("Could not open {}", input_path.display()))?;
    let (width, height) = (img.width(), img.height());

    let size = MODEL_SIZE as usize;
    let resized = img
        .resize_exact(MODEL_SIZE, MODEL_SIZE, FilterType::Lanczos3)
        .to_rgb8();

    let mean = [0.485_f32, 0.456, 0.406];
    let std = [0.229_f32, 0.224, 0.225];
    let mut input = vec![0.0_f32; 3 * size * size];
    for (x, y, pixel) in resized.enumerate_pixels() {
        for c in 0..3 {
            let value = pixel[c] as f32 / 255.0;
            let index = c * size * size + (y as usize) * size + (x as usize);
            input[index] = (value - mean[c]) / std[c];
        }
    }

    let input_name = session.inputs()[0].name().to_string();
    let tensor = Tensor::from_array(([1_usize, 3, size, size], input))?;
    let outputs = session.run(ort::inputs![input_name.as_str() => tensor])?;
    let (_shape, data) = outputs[0].try_extract_tensor::<f32>()?;

    let (min, max) = data
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(mn, mx), &v| {
            (mn.min(v), mx.max(v))
        });
    let needs_sigmoid = min < 0.0 || max > 1.0;

    let mut mask = GrayImage::new(MODEL_SIZE, MODEL_SIZE);
    for (i, &value) in data.iter().enumerate() {
        let x = (i % size) as u32;
        let y = (i / size) as u32;
        let p = if needs_sigmoid {
            1.0 / (1.0 + (-value).exp())
        } else {
            value
        };
        mask.put_pixel(x, y, Luma([(p.clamp(0.0, 1.0) * 255.0) as u8]));
    }

    let mask_full = imageops::resize(&mask, width, height, FilterType::Lanczos3);
    let mut result = img.to_rgba8();
    for (x, y, pixel) in result.enumerate_pixels_mut() {
        pixel[3] = mask_full.get_pixel(x, y)[0];
    }

    encode_png(&result)
}

/// Put the transparent cut-out on top of the chosen background and return PNG bytes
pub fn composite(png: &[u8], background: &Background) -> Result<Vec<u8>> {
    if let Background::Transparent = background {
        return Ok(png.to_vec());
    }

    let fg = image::load_from_memory(png)?.to_rgba8();
    let (w, h) = fg.dimensions();

    let mut canvas: RgbaImage = match background {
        Background::Color { hex } => {
            let [r, g, b] = parse_hex(hex)?;
            RgbaImage::from_pixel(w, h, Rgba([r, g, b, 255]))
        }
        Background::Image { path } => image::open(path)
            .with_context(|| format!("Could not open background {path}"))?
            // fill the whole frame and crop the overflow, like CSS "cover"
            .resize_to_fill(w, h, FilterType::Lanczos3)
            .to_rgba8(),
        Background::Transparent => unreachable!(),
    };

    imageops::overlay(&mut canvas, &fg, 0, 0);
    encode_png(&canvas)
}

fn parse_hex(hex: &str) -> Result<[u8; 3]> {
    let h = hex.trim_start_matches('#');
    if h.len() != 6 {
        bail!("Bad color: {hex}");
    }
    Ok([
        u8::from_str_radix(&h[0..2], 16)?,
        u8::from_str_radix(&h[2..4], 16)?,
        u8::from_str_radix(&h[4..6], 16)?,
    ])
}

fn encode_png(img: &RgbaImage) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    img.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)?;
    Ok(bytes)
}