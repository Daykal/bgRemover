use anyhow::{bail, Context, Result};
use image::imageops::{self, FilterType};
use image::{GrayImage, Luma};
use ort::{session::Session, value::Tensor};
use std::path::PathBuf;

const MODEL_SIZE: u32 = 1024;

/// Build a path relative to the project root (the folder with Cargo.toml)
fn project_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn main() -> Result<()> {
    let input_path = project_path("input/images.jpeg");
    let model_path = project_path("models/model.onnx");
    let output_dir = project_path("output");

    if !model_path.exists() {
        bail!("Model not found at {}", model_path.display());
    }
    std::fs::create_dir_all(&output_dir)?;

    // 1. Load the original image
    let img = image::open(&input_path)
        .with_context(|| format!("Could not open {}", input_path.display()))?;
    let (width, height) = (img.width(), img.height());
    println!("Loaded image: {}x{}", width, height);

    // 2. Resize to what the model expects
    let size = MODEL_SIZE as usize;
    let resized = img
        .resize_exact(MODEL_SIZE, MODEL_SIZE, FilterType::Lanczos3)
        .to_rgb8();

    // 3. Build the input as a flat list in channel-first order (R plane, G plane, B plane)
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

    // 4. Load the model and run it
    let mut session = Session::builder()?
        .commit_from_file(&model_path)
        .context("Could not load the ONNX model")?;

    let tensor = Tensor::from_array(([1_usize, 3, size, size], input))?;
    let outputs = session.run(ort::inputs!["input_image" => tensor])?;

    // 5. Read the result: 320*320 numbers, high = foreground, low = background
     // 5. Read the result and turn it into a mask
     let (_shape, data) = outputs[0].try_extract_tensor::<f32>()?;
     println!("Model returned {} values", data.len());

     let min = data.iter().cloned().fold(f32::INFINITY, f32::min);
     let max = data.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
     let needs_sigmoid = min < 0.0 || max > 1.0;
     println!("Output range: {min} to {max} (sigmoid: {needs_sigmoid})");

     let mut mask = GrayImage::new(MODEL_SIZE, MODEL_SIZE);
     for (i, &value) in data.iter().enumerate() {
         let x = (i % size) as u32;
         let y = (i / size) as u32;
         let p = if needs_sigmoid { 1.0 / (1.0 + (-value).exp()) } else { value };
         mask.put_pixel(x, y, Luma([(p.clamp(0.0, 1.0) * 255.0) as u8]));
     }
     mask.save(output_dir.join("debug_mask.png"))?;

    // 6. Scale the mask back up to the original photo size
    let mask_full = imageops::resize(&mask, width, height, FilterType::Lanczos3);

    // 7. Use the mask as the alpha (transparency) channel of the original photo
    let mut result = img.to_rgba8();
    for (x, y, pixel) in result.enumerate_pixels_mut() {
        pixel[3] = mask_full.get_pixel(x, y)[0];
    }
    let out_path = output_dir.join("photo_nobg.png");
    result.save(&out_path)?;
    println!("Saved {}", out_path.display());

    Ok(())
}