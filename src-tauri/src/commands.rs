use crate::{model::*, platform, settings::Settings, shell, state::AppState, window};
use std::{path::Path, sync::atomic::Ordering};
use tauri::{AppHandle, Manager};

async fn blocking<T: Send + 'static>(
    app: AppHandle,
    f: impl FnOnce(&AppHandle) -> Result<T> + Send + 'static,
) -> Result<T> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>().wait_ready()?;
        f(&app)
    })
    .await
    .map_err(|_| CmdError::internal())?
}
#[tauri::command]
pub async fn list_items(app: AppHandle, query: ListQuery) -> Result<ListResult> {
    blocking(app, move |a| a.state::<AppState>().db(|db| db.list(query))).await
}
#[tauri::command]
pub async fn get_item(app: AppHandle, id: i64) -> Result<ClipItem> {
    blocking(app, move |a| a.state::<AppState>().db(|db| db.get(id))).await
}
#[tauri::command]
pub async fn get_item_content(app: AppHandle, id: i64, max_chars: i64) -> Result<ItemContent> {
    blocking(app, move |a| {
        a.state::<AppState>()
            .db(|db| db.item_content(id, max_chars))
    })
    .await
}
#[tauri::command]
pub async fn copy_item(app: AppHandle, id: i64, plain: bool) -> Result<()> {
    blocking(app, move |a| crate::paste::copy(a, id, plain)).await
}
#[tauri::command]
pub async fn paste_item(app: AppHandle, id: i64, plain: bool) -> Result<PasteOutcome> {
    blocking(app, move |a| crate::paste::paste(a, id, plain)).await
}
#[tauri::command]
pub async fn set_favorite(app: AppHandle, id: i64, value: bool) -> Result<ClipItem> {
    blocking(app, move |a| {
        let item = a
            .state::<AppState>()
            .db(|db| db.mark(id, "favorite", value))?;
        shell::changed(a, "updated", Some(id));
        Ok(item)
    })
    .await
}
#[tauri::command]
pub async fn set_pinned(app: AppHandle, id: i64, value: bool) -> Result<ClipItem> {
    blocking(app, move |a| {
        let item = a
            .state::<AppState>()
            .db(|db| db.mark(id, "pinned", value))?;
        shell::changed(a, "updated", Some(id));
        Ok(item)
    })
    .await
}
#[tauri::command]
pub async fn set_note(app: AppHandle, id: i64, note: Option<String>) -> Result<ClipItem> {
    blocking(app, move |a| {
        let item = a.state::<AppState>().db(|db| db.note(id, note))?;
        shell::changed(a, "updated", Some(id));
        Ok(item)
    })
    .await
}
#[tauri::command]
pub async fn delete_items(app: AppHandle, ids: Vec<i64>) -> Result<usize> {
    blocking(app, move |a| {
        let count = a.state::<AppState>().db(|db| db.delete(&ids))?;
        if count > 0 {
            shell::changed(a, "deleted", None);
        }
        Ok(count)
    })
    .await
}
#[tauri::command]
pub async fn clear_history(app: AppHandle, keep_marked: bool) -> Result<usize> {
    blocking(app, move |a| {
        let count = a.state::<AppState>().db(|db| db.clear(keep_marked))?;
        if count > 0 {
            shell::changed(a, "cleared", None);
        }
        Ok(count)
    })
    .await
}
#[tauri::command]
pub async fn get_stats(app: AppHandle) -> Result<Stats> {
    blocking(app, |a| a.state::<AppState>().db(|db| db.stats())).await
}
#[tauri::command]
pub async fn list_seen_apps(app: AppHandle) -> Result<Vec<SeenApp>> {
    blocking(app, |a| a.state::<AppState>().db(|db| db.seen_apps())).await
}
#[tauri::command]
pub async fn get_settings(app: AppHandle) -> Result<Settings> {
    blocking(app, |a| a.state::<AppState>().settings()).await
}
#[tauri::command]
pub async fn update_settings(app: AppHandle, patch: serde_json::Value) -> Result<Settings> {
    blocking(app, move |a| shell::update(a, patch)).await
}
#[tauri::command]
pub fn get_capture_paused(state: tauri::State<'_, AppState>) -> Result<bool> {
    Ok(state.paused.load(Ordering::Relaxed))
}
#[tauri::command]
pub fn set_capture_paused(app: AppHandle, paused: bool) -> Result<()> {
    shell::set_paused(&app, paused)
}
#[tauri::command]
pub fn hide_panel(app: AppHandle, restore_focus: bool) -> Result<()> {
    window::hide(&app, restore_focus)
}
#[tauri::command]
pub fn set_panel_locked(state: tauri::State<'_, AppState>, locked: bool) -> Result<()> {
    state.panel.lock().map_err(|_| CmdError::internal())?.locked = locked;
    Ok(())
}
#[tauri::command]
pub fn open_settings(app: AppHandle, section: Option<String>) -> Result<()> {
    window::open_settings(&app, section)
}
#[tauri::command]
pub async fn open_url(app: AppHandle, url: String) -> Result<()> {
    blocking(app, move |_| {
        let parsed = url::Url::parse(&url).map_err(|_| CmdError::invalid("Invalid URL"))?;
        if !["http", "https", "mailto"].contains(&parsed.scheme()) {
            return Err(CmdError::invalid("URL scheme is not allowed"));
        }
        tauri_plugin_opener::open_url(url, None::<&str>)
            .map_err(|_| CmdError::new("io", "Could not open URL"))
    })
    .await
}
fn existing(path: &str) -> Result<()> {
    if !Path::new(path).exists() {
        Err(CmdError::new("not_found", "Path does not exist"))
    } else {
        Ok(())
    }
}
#[tauri::command]
pub async fn open_path(app: AppHandle, path: String) -> Result<()> {
    blocking(app, move |_| {
        existing(&path)?;
        tauri_plugin_opener::open_path(path, None::<&str>)
            .map_err(|_| CmdError::new("io", "Could not open path"))
    })
    .await
}
#[tauri::command]
pub async fn reveal_path(app: AppHandle, path: String) -> Result<()> {
    blocking(app, move |_| {
        existing(&path)?;
        tauri_plugin_opener::reveal_item_in_dir(path)
            .map_err(|_| CmdError::new("io", "Could not reveal path"))
    })
    .await
}
#[tauri::command]
pub async fn open_data_dir(app: AppHandle) -> Result<()> {
    blocking(app, |a| {
        tauri_plugin_opener::open_path(&a.state::<AppState>().dir, None::<&str>)
            .map_err(|_| CmdError::new("io", "Could not open data directory"))
    })
    .await
}
#[tauri::command]
pub fn get_app_info(app: AppHandle) -> Result<AppInfo> {
    Ok(AppInfo {
        version: app.package_info().version.to_string(),
        data_dir: app.state::<AppState>().dir.to_string_lossy().into_owned(),
        elevated: platform::self_elevated(),
    })
}
