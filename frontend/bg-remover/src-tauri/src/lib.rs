mod remover;

use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{ipc::Response, State};

/// Remembers the last result so "Save" can write it without sending it back and forth
#[derive(Default)]
struct AppState {
    last_result: Mutex<Option<Vec<u8>>>,
}

#[tauri::command]
async fn remove_background(path: String, state: State<'_, AppState>) -> Result<Response, String> {
    let model_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models/model.onnx");

    // Run the heavy work on a separate thread so the UI doesn't freeze
    let png = tauri::async_runtime::spawn_blocking(move || {
        remover::remove_background(&PathBuf::from(path), &model_path)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("{e:#}"))?;

    *state.last_result.lock().unwrap() = Some(png.clone());
    Ok(Response::new(png))
}

#[tauri::command]
fn save_result(path: String, state: State<'_, AppState>) -> Result<(), String> {
    let guard = state.last_result.lock().unwrap();
    let png = guard.as_ref().ok_or("Nothing to save yet")?;
    std::fs::write(path, png).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![remove_background, save_result])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}