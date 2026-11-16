use crate::{
    hook::Hook,
    model::{CmdError, Result},
    platform,
    settings::{hotkey, Settings},
    state::AppState,
    window,
};
use std::sync::{atomic::Ordering, Arc};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_global_shortcut::GlobalShortcutExt;

pub fn queue_toggle(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if handle.try_state::<AppState>().is_some() {
            if let Err(e) = window::toggle(&handle) {
                log::warn!("Panel toggle failed: {}", e.code);
            }
        }
    });
}
pub fn install_hook(app: &AppHandle) -> Result<Hook> {
    let app = app.clone();
    Hook::start(Arc::new(move || queue_toggle(&app)))
}
fn autostart(app: &AppHandle, enabled: bool) -> Result<()> {
    let result = if enabled {
        app.autolaunch().enable()
    } else {
        app.autolaunch().disable()
    };
    result.map_err(|_| CmdError::new("io", "Could not update autostart"))
}
pub fn changed(app: &AppHandle, reason: &str, id: Option<i64>) {
    let _ = app.emit(
        "history://changed",
        serde_json::json!({"reason":reason,"id":id}),
    );
}

pub fn update(app: &AppHandle, patch: serde_json::Value) -> Result<Settings> {
    let state = app.state::<AppState>();
    let _gate = state
        .settings_gate
        .lock()
        .map_err(|_| CmdError::internal())?;
    let old = state.settings()?;
    let new = old.patch(patch)?;
    let old_shortcut = hotkey(&old.hotkey)?.1;
    let new_shortcut = hotkey(&new.hotkey)?.1;
    let hotkey_changed = old.hotkey != new.hotkey;
    if hotkey_changed {
        if let Some(shortcut) = new_shortcut {
            app.global_shortcut()
                .register(shortcut)
                .map_err(|_| CmdError::new("hotkey_conflict", "Shortcut is unavailable"))?;
        }
    }
    let mut new_hook = None;
    let apply = (|| -> Result<()> {
        if old.autostart != new.autostart {
            autostart(app, new.autostart)?;
        }
        if !old.win_v && new.win_v {
            new_hook = Some(install_hook(app)?);
        }
        // Check lock health before committing any persistent setting.
        drop(state.hook.lock().map_err(|_| CmdError::internal())?);
        drop(state.settings.write().map_err(|_| CmdError::internal())?);
        new.save(&state.dir)?;
        if hotkey_changed {
            if let Some(shortcut) = old_shortcut {
                app.global_shortcut()
                    .unregister(shortcut)
                    .map_err(|_| CmdError::new("internal", "Could not release old shortcut"))?;
            }
        }
        Ok(())
    })();
    if let Err(error) = apply {
        if hotkey_changed {
            if let Some(shortcut) = new_shortcut {
                let _ = app.global_shortcut().unregister(shortcut);
            }
        }
        if old.autostart != new.autostart && autostart(app, old.autostart).is_err() {
            log::error!("Autostart rollback failed: io");
        }
        if old.save(&state.dir).is_err() {
            log::error!("Settings rollback failed: io");
        }
        return Err(error);
    }
    if old.win_v != new.win_v {
        *state.hook.lock().map_err(|_| CmdError::internal())? = new_hook;
    }
    *state.settings.write().map_err(|_| CmdError::internal())? = new.clone();
    let cleaned = state.db(|db| db.cleanup(&new));
    match cleaned {
        Ok(n) if n > 0 => changed(app, "cleanup", None),
        Err(e) => log::warn!("Settings cleanup failed: {}", e.code),
        _ => {}
    }
    if refresh_tray(app).is_err() {
        log::warn!("Tray refresh failed: internal");
    }
    let _ = app.emit("settings://changed", &new);
    Ok(new)
}
fn menu(app: &AppHandle) -> Result<Menu<tauri::Wry>> {
    let state = app.state::<AppState>();
    let settings = state.settings()?;
    let zh =
        settings.language == "zh-CN" || (settings.language == "system" && platform::chinese_ui());
    let label = if zh { "打开面板" } else { "Open panel" };
    let label = if settings.hotkey.is_empty() {
        label.to_owned()
    } else {
        format!("{label}	{}", hotkey_label(&settings.hotkey))
    };
    let open = MenuItem::with_id(app, "open", label, true, None::<&str>)
        .map_err(|_| CmdError::internal())?;
    let pause = CheckMenuItem::with_id(
        app,
        "pause",
        if zh { "暂停记录" } else { "Pause capture" },
        true,
        state.paused.load(Ordering::Relaxed),
        None::<&str>,
    )
    .map_err(|_| CmdError::internal())?;
    let preferences = MenuItem::with_id(
        app,
        "settings",
        if zh {
            "偏好设置…"
        } else {
            "Preferences…"
        },
        true,
        None::<&str>,
    )
    .map_err(|_| CmdError::internal())?;
    let separator = PredefinedMenuItem::separator(app).map_err(|_| CmdError::internal())?;
    let quit = MenuItem::with_id(
        app,
        "quit",
        if zh { "退出" } else { "Quit" },
        true,
        None::<&str>,
    )
    .map_err(|_| CmdError::internal())?;
    Menu::with_items(app, &[&open, &pause, &preferences, &separator, &quit])
        .map_err(|_| CmdError::internal())
}
/// "Ctrl+Shift+KeyV" -> "Ctrl+Shift+V", "Super+Backquote" -> "Win+`" (menu text only).
fn hotkey_label(hotkey: &str) -> String {
    hotkey
        .split('+')
        .map(|part| match part {
            "Super" => "Win".to_owned(),
            "Backquote" => "`".to_owned(),
            "Space" => "Space".to_owned(),
            p => p
                .strip_prefix("Key")
                .or_else(|| p.strip_prefix("Digit"))
                .filter(|rest| rest.chars().count() == 1)
                .unwrap_or(p)
                .to_owned(),
        })
        .collect::<Vec<_>>()
        .join("+")
}
fn icon(paused: bool) -> Result<tauri::image::Image<'static>> {
    let bytes = if paused {
        include_bytes!("../icons/tray-paused.png").as_slice()
    } else {
        include_bytes!("../icons/tray.png").as_slice()
    };
    tauri::image::Image::from_bytes(bytes).map_err(|_| CmdError::internal())
}
pub fn create_tray(app: &AppHandle) -> Result<()> {
    TrayIconBuilder::with_id("copybook")
        .icon(icon(false)?)
        .tooltip("Copybook")
        .menu(&menu(app)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => {
                if let Err(e) = window::show(app) {
                    log::warn!("Panel show failed: {}", e.code);
                }
            }
            "pause" => {
                let paused = !app.state::<AppState>().paused.load(Ordering::Relaxed);
                let _ = set_paused(app, paused);
            }
            "settings" => {
                let _ = window::open_settings(app, None);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                queue_toggle(tray.app_handle());
            }
        })
        .build(app)
        .map_err(|_| CmdError::internal())?;
    Ok(())
}
pub fn refresh_tray(app: &AppHandle) -> Result<()> {
    let tray = app.tray_by_id("copybook").ok_or_else(CmdError::internal)?;
    tray.set_menu(Some(menu(app)?))
        .map_err(|_| CmdError::internal())?;
    tray.set_icon(Some(icon(
        app.state::<AppState>().paused.load(Ordering::Relaxed),
    )?))
    .map_err(|_| CmdError::internal())?;
    Ok(())
}
pub fn set_paused(app: &AppHandle, paused: bool) -> Result<()> {
    app.state::<AppState>()
        .paused
        .store(paused, Ordering::Relaxed);
    refresh_tray(app)?;
    app.emit("capture://paused", serde_json::json!({"paused":paused}))
        .map_err(|_| CmdError::internal())?;
    Ok(())
}
pub fn start(app: &AppHandle) -> Result<()> {
    let state = app.state::<AppState>();
    let settings = state.settings()?;
    if let Some(shortcut) = hotkey(&settings.hotkey)?.1 {
        if app.global_shortcut().register(shortcut).is_err() {
            log::warn!("Startup shortcut registration failed: hotkey_conflict");
        }
    }
    if settings.win_v {
        match install_hook(app) {
            Ok(hook) => *state.hook.lock().map_err(|_| CmdError::internal())? = Some(hook),
            Err(e) => log::warn!("Startup hook failed: {}", e.code),
        }
    }
    // Do not rewrite HKCU on every launch; only explicit updates change autostart.
    create_tray(app)
}
