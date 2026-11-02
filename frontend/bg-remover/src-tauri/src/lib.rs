
mod remover;

use ort::session::Session;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{
    ipc::Response,
    path::BaseDirectory,
    AppHandle,
    Manager,
    State,
};

#[derive(Default)]
struct AppState {
    /// The loaded model. Starts empty and is filled on the first image.
    session: Mutex<Option<Session>>,

    /// The last transparent result.
    last_result: Mutex<Option<Vec<u8>>>,
}

/// Find the ONNX model.
/// Development uses the source directory.
/// Production uses the bundled application resources.
fn find_model(app: &AppHandle) -> Result<PathBuf, String> {

    // Development path
    #[cfg(debug_assertions)]
    {
        let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("models/model.onnx");

        if dev_path.is_file() {
            return Ok(dev_path);
        }
    }

    // Production / installed application path
    let model_path = app
        .path()
        .resolve("models/model.onnx", BaseDirectory::Resource)
        .map_err(|e| format!("Could not resolve model path: {e}"))?;

    if !model_path.is_file() {
        return Err(format!(
            "ONNX model not found at: {}",
            model_path.display()
        ));
    }

    Ok(model_path)
}

#[tauri::command]
async fn remove_background(
    path: String,
    app: AppHandle,
) -> Result<Response, String> {

    // Find model before starting the background task
    let model_path = find_model(&app)?;

    let png = tauri::async_runtime::spawn_blocking(
        move || -> Result<Vec<u8>, String> {

            let state = app.state::<AppState>();

            // Load model only once
            let mut guard = state
                .session
                .lock()
                .map_err(|e| e.to_string())?;

            if guard.is_none() {
                let session = remover::load_session(&model_path)
                    .map_err(|e| format!("{e:#}"))?;

                *guard = Some(session);
            }

            let session = guard
                .as_mut()
                .ok_or("Model not loaded")?;

            let png = remover::remove_background(
                session,
                &PathBuf::from(path),
            )
            .map_err(|e| format!("{e:#}"))?;

            *state.last_result
                .lock()
                .map_err(|e| e.to_string())? = Some(png.clone());

            Ok(png)
        },
    )
    .await
    .map_err(|e| e.to_string())??;

    Ok(Response::new(png))
}

/// Reads a file and sends its raw bytes to the UI.
#[tauri::command]
fn load_file(path: String) -> Result<Response, String> {
    std::fs::read(&path)
        .map(Response::new)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn save_result(
    path: String,
    background: remover::Background,
    state: State<'_, AppState>,
) -> Result<(), String> {

    let guard = state.last_result
        .lock()
        .map_err(|e| e.to_string())?;

    let png = guard
        .as_ref()
        .ok_or("Nothing to save yet")?;

    let out = remover::composite(png, &background)
        .map_err(|e| format!("{e:#}"))?;

    std::fs::write(path, out)
        .map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            remove_background,
            load_file,
            save_result
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
