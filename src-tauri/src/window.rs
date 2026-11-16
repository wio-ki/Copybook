use crate::{
    model::{CmdError, Result},
    platform,
    settings::atomic_json,
    state::AppState,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow, WindowEvent,
};
use windows::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    UI::{
        HiDpi::*,
        Shell::{DefSubclassProc, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};

static IN_SIZE_MOVE: AtomicBool = AtomicBool::new(false);
#[derive(Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: Option<f64>,
    pub height: Option<f64>,
}
#[derive(Default)]
pub struct PanelState {
    pub target: Option<isize>,
    pub locked: bool,
    pub shown: Option<Instant>,
    pub saved: WindowState,
    save_queued: bool,
}
fn panel(app: &AppHandle) -> Result<WebviewWindow> {
    app.get_webview_window("panel")
        .ok_or_else(CmdError::internal)
}
unsafe extern "system" fn subclass(
    window: HWND,
    message: u32,
    w: WPARAM,
    l: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if message == WM_ENTERSIZEMOVE {
        IN_SIZE_MOVE.store(true, Ordering::Relaxed);
    }
    if message == WM_EXITSIZEMOVE {
        IN_SIZE_MOVE.store(false, Ordering::Relaxed);
    }
    // SAFETY: forwards unchanged subclass messages to the system's remaining window procedures.
    unsafe { DefSubclassProc(window, message, w, l) }
}
pub fn install(app: &AppHandle) -> Result<()> {
    let panel = panel(app)?;
    let hwnd = panel.hwnd().map_err(|_| CmdError::internal())?;
    // SAFETY: setup executes on the window's owning thread; callback is static with no userdata.
    if !unsafe { SetWindowSubclass(platform::hwnd(hwnd.0 as isize), Some(subclass), 1, 0) }
        .as_bool()
    {
        return Err(CmdError::internal());
    }
    let handle = app.clone();
    panel.on_window_event(move |event| {
        let Some(state) = handle.try_state::<AppState>() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = hide(&handle, true);
            }
            WindowEvent::Focused(false) => {
                let should_hide = state.settings().is_ok_and(|s| s.hide_on_blur)
                    && !IN_SIZE_MOVE.load(Ordering::Relaxed)
                    && state.panel.lock().is_ok_and(|p| {
                        !p.locked
                            && p.shown
                                .is_some_and(|t| t.elapsed() >= Duration::from_millis(150))
                    });
                if should_hide {
                    let _ = hide(&handle, false);
                }
            }
            WindowEvent::Moved(_) | WindowEvent::Resized(_) => persist(&handle),
            _ => {}
        }
    });
    if let Some(settings) = app.get_webview_window("settings") {
        settings.on_window_event({
            let app = app.clone();
            move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    if let Some(w) = app.get_webview_window("settings") {
                        let _ = w.hide();
                    }
                }
            }
        });
    }
    Ok(())
}
fn persist(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let Ok(panel) = panel(app) else {
        return;
    };
    if !panel.is_visible().unwrap_or(false) {
        return;
    }
    let (Ok(position), Ok(size), Ok(scale)) = (
        panel.outer_position(),
        panel.inner_size(),
        panel.scale_factor(),
    ) else {
        return;
    };
    let saved = WindowState {
        x: Some(position.x),
        y: Some(position.y),
        width: Some(f64::from(size.width) / scale),
        height: Some(f64::from(size.height) / scale),
    };
    let Ok(mut p) = state.panel.lock() else {
        return;
    };
    p.saved = saved;
    if p.save_queued {
        return;
    }
    p.save_queued = true;
    drop(p);
    // One writer coalesces resize/move events and always persists the latest snapshot.
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        loop {
            std::thread::sleep(Duration::from_millis(100));
            let saved = match state.panel.lock() {
                Ok(p) => p.saved.clone(),
                Err(_) => return,
            };
            if atomic_json(&state.dir.join("state.json"), &saved).is_err() {
                log::warn!("Window state write failed: io");
            }
            let Ok(mut p) = state.panel.lock() else {
                return;
            };
            if p.saved == saved {
                p.save_queued = false;
                break;
            }
        }
    });
}
pub fn toggle(app: &AppHandle) -> Result<()> {
    let panel = panel(app)?;
    if panel.is_visible().unwrap_or(false) && panel.is_focused().unwrap_or(false) {
        return hide(app, true);
    }
    show(app)
}
pub fn show(app: &AppHandle) -> Result<()> {
    let state = app.state::<AppState>();
    let settings = state.settings()?;
    let panel = panel(app)?;
    let current = platform::foreground();
    {
        let mut p = state.panel.lock().map_err(|_| CmdError::internal())?;
        // Our own window in front (e.g. settings) means there is nothing to paste into.
        p.target = platform::target(current);
    }
    position(app, &panel, &settings.panel_position)?;
    let locked = {
        let mut p = state.panel.lock().map_err(|_| CmdError::internal())?;
        p.shown = Some(Instant::now());
        p.locked
    };
    panel.show().map_err(|_| CmdError::internal())?;
    let hwnd = panel.hwnd().map_err(|_| CmdError::internal())?;
    if !platform::force_foreground(platform::hwnd(hwnd.0 as isize)) {
        return Err(CmdError::new("internal", "Panel did not receive focus"));
    }
    panel.set_focus().map_err(|_| CmdError::internal())?;
    app.emit("panel://shown", serde_json::json!({"locked":locked}))
        .map_err(|_| CmdError::internal())?;
    Ok(())
}
pub fn hide(app: &AppHandle, restore: bool) -> Result<()> {
    let panel = panel(app)?;
    panel.hide().map_err(|_| CmdError::internal())?;
    let state = app.state::<AppState>();
    let target = {
        let mut p = state.panel.lock().map_err(|_| CmdError::internal())?;
        p.shown = None;
        p.target
    };
    app.emit("panel://hidden", serde_json::json!({}))
        .map_err(|_| CmdError::internal())?;
    if restore {
        if let Some(target) = target.and_then(|t| platform::target(platform::hwnd(t))) {
            platform::force_foreground(platform::hwnd(target));
        }
    }
    Ok(())
}
fn position(app: &AppHandle, panel: &WebviewWindow, mode: &str) -> Result<()> {
    let state = app.state::<AppState>();
    let saved = state
        .panel
        .lock()
        .map_err(|_| CmdError::internal())?
        .saved
        .clone();
    // SAFETY: APIs write into initialized structs; monitor handles are queried on demand.
    let (cursor, work, scale, remember_valid) = unsafe {
        let mut cursor = POINT::default();
        GetCursorPos(&mut cursor)?;
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return Err(CmdError::internal());
        }
        let mut x = 96;
        let mut y = 96;
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y);
        let valid = match (saved.x, saved.y) {
            (Some(x), Some(y)) => {
                !MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONULL).is_invalid()
            }
            _ => false,
        };
        (cursor, info.rcWork, f64::from(x) / 96.0, valid)
    };
    let width = (saved
        .width
        .filter(|n| n.is_finite() && *n >= 320.0)
        .unwrap_or(384.0)
        * scale)
        .round()
        .min(f64::from(work.right - work.left))
        .max(1.0) as u32;
    let height = (saved
        .height
        .filter(|n| n.is_finite() && *n >= 420.0)
        .unwrap_or(576.0)
        * scale)
        .round()
        .min(f64::from(work.bottom - work.top))
        .max(1.0) as u32;
    panel
        .set_size(PhysicalSize::new(width, height))
        .map_err(|_| CmdError::internal())?;
    let (w, h) = (width as i32, height as i32);
    let gap = (8.0 * scale).round() as i32;
    let cursor_position = || {
        (
            if cursor.x + gap + w > work.right {
                cursor.x - w - gap
            } else {
                cursor.x + gap
            },
            if cursor.y + gap + h > work.bottom {
                cursor.y - h - gap
            } else {
                cursor.y + gap
            },
        )
    };
    let (x, y) = match mode {
        "center" => (
            work.left + (work.right - work.left - w) / 2,
            work.top + (work.bottom - work.top - h) / 2,
        ),
        "remember" if remember_valid => (saved.x.unwrap_or(cursor.x), saved.y.unwrap_or(cursor.y)),
        _ => cursor_position(),
    };
    panel
        .set_position(PhysicalPosition::new(
            x.clamp(work.left, (work.right - w).max(work.left)),
            y.clamp(work.top, (work.bottom - h).max(work.top)),
        ))
        .map_err(|_| CmdError::internal())?;
    Ok(())
}
pub fn open_settings(app: &AppHandle, section: Option<String>) -> Result<()> {
    if section
        .as_ref()
        .is_some_and(|s| !["general", "capture", "paste", "data", "about"].contains(&s.as_str()))
    {
        return Err(CmdError::invalid("Unknown settings section"));
    }
    let window = app
        .get_webview_window("settings")
        .ok_or_else(CmdError::internal)?;
    window.unminimize().map_err(|_| CmdError::internal())?;
    window.show().map_err(|_| CmdError::internal())?;
    let hwnd = window.hwnd().map_err(|_| CmdError::internal())?;
    platform::force_foreground(platform::hwnd(hwnd.0 as isize));
    window.set_focus().map_err(|_| CmdError::internal())?;
    if let Some(section) = section {
        app.emit(
            "settings://navigate",
            serde_json::json!({"section":section}),
        )
        .map_err(|_| CmdError::internal())?;
    }
    Ok(())
}
