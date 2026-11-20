use crate::{
    capture::{Capture, Flags, Shot},
    ocr,
    translate::{Config, Translator},
};
use serde::Serialize;
use std::{
    ffi::c_void,
    mem::size_of,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{sync_channel, Receiver},
        Arc,
    },
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize};
use windows::Win32::{
    Foundation::{HWND, RECT},
    Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS},
    System::Threading::GetCurrentProcessId,
    UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow},
};
use windows_capture::{
    capture::{CaptureControl, GraphicsCaptureApiHandler},
    settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
        MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    },
    window::Window,
};

type CaptureError = Box<dyn std::error::Error + Send + Sync>;

pub struct Session {
    control: CaptureControl<Capture, CaptureError>,
    stop: Arc<AtomicBool>,
}

impl Session {
    pub fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.control.stop();
    }
}

#[derive(Serialize, Clone)]
struct OverlayBlock {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    lines: usize,
    text: String,
    translated: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct OverlayPayload {
    frame_w: u32,
    frame_h: u32,
    blocks: Vec<OverlayBlock>,
}

pub fn start(
    app: AppHandle,
    translator: Arc<Translator>,
    hwnd: isize,
    cfg: Config,
) -> Result<Session, String> {
    let (tx, rx) = sync_channel::<Shot>(1);
    let stop = Arc::new(AtomicBool::new(false));

    let worker_app = app.clone();
    let worker_cfg = cfg.clone();
    thread::spawn(move || run_worker(worker_app, translator, worker_cfg, rx));

    let settings = Settings::new(
        Window::from_raw_hwnd(hwnd as *mut c_void),
        CursorCaptureSettings::WithoutCursor,
        DrawBorderSettings::WithoutBorder,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        Flags {
            on_closed: {
                let app = app.clone();
                Box::new(move || {
                    let _ = app.emit("capture-ended", ());
                })
            },
            tx,
            interval: Duration::from_millis(cfg.interval_ms.max(100)),
            region: cfg.region,
        },
    );
    let control = Capture::start_free_threaded(settings).map_err(|e| format!("could not start capture: {e}"))?;

    let track_app = app.clone();
    let track_stop = stop.clone();
    thread::spawn(move || track_overlay(track_app, hwnd, track_stop));

    Ok(Session { control, stop })
}

fn run_worker(app: AppHandle, translator: Arc<Translator>, cfg: Config, rx: Receiver<Shot>) {
    let engine = match ocr::create_engine(&cfg.source_lang) {
        Ok(e) => e,
        Err(e) => {
            let _ = app.emit(
                "status-error",
                format!("OCR language '{}' is not available ({e}). Install its Windows language pack.", cfg.source_lang),
            );
            return;
        }
    };
    let spaceless = ocr::is_spaceless_script(&cfg.source_lang);

    let mut previous: Vec<(String, f32, f32)> = Vec::new();
    let mut misses = 0u32;

    // The channel closes when capture stops, which ends this loop.
    while let Ok(shot) = rx.recv() {
        let lines = match ocr::recognize(&engine, &shot.data, shot.width, shot.height) {
            Ok(l) => l,
            Err(e) => {
                let _ = app.emit("status-error", format!("OCR failed: {e}"));
                continue;
            }
        };

        let blocks: Vec<_> = ocr::group_lines(lines, spaceless)
            .into_iter()
            .filter(|b| b.text.chars().filter(|c| c.is_alphabetic()).count() >= 2)
            .collect();

        if blocks.is_empty() {
            // Tolerate a single bad frame before clearing, so the overlay does not flicker.
            misses += 1;
            if misses == 2 && !previous.is_empty() {
                previous.clear();
                emit_overlay(&app, OverlayPayload { frame_w: shot.full_width, frame_h: shot.full_height, blocks: vec![] });
            }
            continue;
        }
        misses = 0;

        let signature: Vec<_> = blocks.iter().map(|b| (b.text.clone(), b.x, b.y)).collect();
        let unchanged = signature.len() == previous.len()
            && signature
                .iter()
                .zip(&previous)
                .all(|(a, b)| a.0 == b.0 && (a.1 - b.1).abs() < 6.0 && (a.2 - b.2).abs() < 6.0);
        if unchanged {
            continue;
        }
        previous = signature;

        let handles: Vec<_> = blocks
            .iter()
            .map(|b| {
                let (t, c, text) = (translator.clone(), cfg.clone(), b.text.clone());
                tauri::async_runtime::spawn(async move { t.translate(&c, &text).await })
            })
            .collect();

        let mut out = Vec::with_capacity(blocks.len());
        for (b, handle) in blocks.into_iter().zip(handles) {
            let translated = match tauri::async_runtime::block_on(handle) {
                Ok(Ok(t)) => t,
                Ok(Err(e)) => {
                    let _ = app.emit("status-error", format!("Translation failed: {e}"));
                    continue;
                }
                Err(_) => continue,
            };
            out.push(OverlayBlock {
                x: b.x + shot.offset_x as f32,
                y: b.y + shot.offset_y as f32,
                w: b.w,
                h: b.h,
                lines: b.lines,
                text: b.text,
                translated,
            });
        }
        emit_overlay(&app, OverlayPayload { frame_w: shot.full_width, frame_h: shot.full_height, blocks: out });
    }
}

/// Goes to every window: the overlay draws it, the control panel lists it in the live feed.
fn emit_overlay(app: &AppHandle, payload: OverlayPayload) {
    let _ = app.emit("ocr-result", payload);
}

/// Keeps the transparent overlay on top of the game window and hides it when the game is not in front.
fn track_overlay(app: AppHandle, hwnd: isize, stop: Arc<AtomicBool>) {
    let game = HWND(hwnd as *mut c_void);
    let mut last_rect = (0, 0, 0, 0);
    let mut shown = false;

    while !stop.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(100));
        let Some(overlay) = app.get_webview_window("overlay") else { continue };

        let alive = unsafe { IsWindow(Some(game)).as_bool() };
        let iconic = unsafe { IsIconic(game).as_bool() };
        let foreground = unsafe { GetForegroundWindow() };
        let mut fg_pid = 0u32;
        unsafe { GetWindowThreadProcessId(foreground, Some(&mut fg_pid)) };
        let game_or_us_in_front = foreground == game || fg_pid == unsafe { GetCurrentProcessId() };

        let want_visible = alive && !iconic && game_or_us_in_front;
        let mut rect = RECT::default();
        let got_rect = unsafe {
            DwmGetWindowAttribute(
                game,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                &mut rect as *mut RECT as *mut c_void,
                size_of::<RECT>() as u32,
            )
            .is_ok()
        };

        if want_visible && got_rect {
            let r = (rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top);
            if r != last_rect && r.2 > 0 && r.3 > 0 {
                let _ = overlay.set_position(PhysicalPosition::new(r.0, r.1));
                let _ = overlay.set_size(PhysicalSize::new(r.2 as u32, r.3 as u32));
                last_rect = r;
            }
            if !shown {
                let _ = overlay.show();
                shown = true;
            }
        } else if shown {
            let _ = overlay.hide();
            shown = false;
        }
    }

    if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.hide();
    }
    let _ = app.emit_to("overlay", "ocr-result", OverlayPayload { frame_w: 1, frame_h: 1, blocks: vec![] });
}
