mod capture;
mod ocr;
mod session;
mod translate;

use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};
use windows_capture::window::Window;

struct AppState {
    session: Mutex<Option<session::Session>>,
    translator: Arc<translate::Translator>,
}

#[derive(Serialize)]
struct WindowInfo {
    hwnd: isize,
    title: String,
    process: String,
}

#[tauri::command]
fn list_windows() -> Vec<WindowInfo> {
    let own_pid = std::process::id();
    Window::enumerate()
        .unwrap_or_default()
        .into_iter()
        .filter(|w| w.process_id().map(|p| p != own_pid).unwrap_or(true))
        .filter_map(|w| {
            let title = w.title().ok().filter(|t| !t.trim().is_empty())?;
            Some(WindowInfo {
                hwnd: w.as_raw_hwnd() as isize,
                title,
                process: w.process_name().unwrap_or_default(),
            })
        })
        .collect()
}

#[tauri::command]
fn list_ocr_languages() -> Vec<ocr::OcrLang> {
    ocr::available_languages()
}

#[tauri::command]
fn start_capture(
    app: AppHandle,
    state: State<AppState>,
    hwnd: isize,
    config: translate::Config,
) -> Result<(), String> {
    let mut slot = state.session.lock().unwrap();
    if let Some(old) = slot.take() {
        old.stop();
    }
    *slot = Some(session::start(app, state.translator.clone(), hwnd, config)?);
    Ok(())
}

#[tauri::command]
fn stop_capture(state: State<AppState>) {
    if let Some(old) = state.session.lock().unwrap().take() {
        old.stop();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState { session: Mutex::new(None), translator: Arc::new(translate::Translator::new()) })
        .setup(|app| {
            if let Some(overlay) = app.get_webview_window("overlay") {
                // Let mouse clicks pass through the overlay to the game underneath.
                overlay.set_ignore_cursor_events(true)?;
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { .. } = event {
                    window.app_handle().exit(0);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![list_windows, list_ocr_languages, start_capture, stop_capture])
        .run(tauri::generate_context!())
        .expect("error while running Screen Translator");
}
