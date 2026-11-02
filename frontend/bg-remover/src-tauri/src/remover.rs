use anyhow::{Context, Result};
use image::imageops::{self, FilterType};
use image::{GrayImage, Luma};
use ort::{session::Session, value::Tensor};
use std::io::Cursor;
use std::path::Path;

const MODEL_SIZE: u32 = 1024;

pub fn remove_background(input_path: &Path, model_path: &Path) -> Result<Vec<u8>> {
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

    let mut session = Session::builder()?
        .commit_from_file(model_path)
        .context("Could not load the ONNX model")?;
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

    // Encode as PNG in memory instead of writing a file
    let mut png_bytes = Vec::new();
    result.write_to(&mut Cursor::new(&mut png_bytes), image::ImageFormat::Png)?;
    Ok(png_bytes)
}