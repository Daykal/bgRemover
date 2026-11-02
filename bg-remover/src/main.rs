use anyhow::{bail, Context, Result};
use image::imageops::{self, FilterType};
use image::{GrayImage, Luma};
use ort::{session::Session, value::Tensor};
use std::path::PathBuf;

fn project_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn main() -> Result<()> {
    let input_path = project_path("input/images.jpeg");
    let model_path = project_path("models/phone.onnx");
    let output_dir = project_path("output");

    if !model_path.exists() {
        bail!("Model not found at {}", model_path.display());
    }
    std::fs::create_dir_all(&output_dir)?;

    // 1. Load the model FIRST, so we can ask it what size it wants
    let mut session = Session::builder()?
        .commit_from_file(&model_path)
        .context("Could not load the ONNX model")?;

    let input_name = session.inputs()[0].name().to_string();
    let shape = session.inputs()[0]
        .dtype()
        .tensor_shape()
        .context("Model input is not a tensor")?;

    // Shape is [batch, channels, height, width]. A value <= 0 means "any size".
    let model_h = if shape[2] > 0 { shape[2] as u32 } else { 1024 };
    let model_w = if shape[3] > 0 { shape[3] as u32 } else { 1024 };
    println!("Model '{input_name}' wants {model_w}x{model_h}");

    // 2. Load the original image
    let img = image::open(&input_path)
        .with_context(|| format!("Could not open {}", input_path.display()))?;
    let (width, height) = (img.width(), img.height());
    println!("Loaded image: {}x{}", width, height);

    // 3. Resize to the size the model asked for
    let (w, h) = (model_w as usize, model_h as usize);
    let resized = img
        .resize_exact(model_w, model_h, FilterType::Lanczos3)
        .to_rgb8();

    // 4. Normalize and flatten into channel-first format
    let mean = [0.485_f32, 0.456, 0.406];
    let std = [0.229_f32, 0.224, 0.225];
    let mut input = vec![0.0_f32; 3 * h * w];

    for (x, y, pixel) in resized.enumerate_pixels() {
        for c in 0..3 {
            let value = pixel[c] as f32 / 255.0;
            let index = c * h * w + (y as usize) * w + (x as usize);
            input[index] = (value - mean[c]) / std[c];
        }
    }

    // 5. Run the model
    let tensor = Tensor::from_array(([1_usize, 3, h, w], input))?;
    let outputs = session.run(ort::inputs![input_name.as_str() => tensor])?;

    // 6. Read the result and find min/max in a single pass
    let (_shape, data) = outputs[0].try_extract_tensor::<f32>()?;
    println!("Model returned {} values", data.len());

    let (min, max) = data
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(mn, mx), &v| {
            (mn.min(v), mx.max(v))
        });
    let needs_sigmoid = min < 0.0 || max > 1.0;
    println!("Output range: {min} to {max} (sigmoid: {needs_sigmoid})");

    // 7. Build the mask
    let mut mask = GrayImage::new(model_w, model_h);
    for (i, &value) in data.iter().enumerate() {
        let x = (i % w) as u32;
        let y = (i / w) as u32;
        let p = if needs_sigmoid {
            1.0 / (1.0 + (-value).exp())
        } else {
            value
        };
        mask.put_pixel(x, y, Luma([(p.clamp(0.0, 1.0) * 255.0) as u8]));
    }
    mask.save(output_dir.join("debug_mask.png"))?;

    // 8. Scale the mask back to the original photo size and apply it as alpha
    let mask_full = imageops::resize(&mask, width, height, FilterType::Lanczos3);
    let mut result = img.to_rgba8();
    for (x, y, pixel) in result.enumerate_pixels_mut() {
        pixel[3] = mask_full.get_pixel(x, y)[0];
    }

    let out_path = output_dir.join("photo_nobg.png");
    result.save(&out_path)?;
    println!("Saved {}", out_path.display());

    Ok(())
}