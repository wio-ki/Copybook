use crate::{model::*, platform, settings::Settings, shell, state::AppState, window};
use std::{path::Path, sync::atomic::Ordering};
use tauri::{AppHandle, Manager};
use zeroize::Zeroizing;

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
        let count = a.state::<AppState>().db(|db| db.soft_delete(&ids))?;
        if count > 0 {
            shell::changed(a, "deleted", None);
            let app = a.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    if let Err(error) = app.state::<AppState>().db(|db| db.purge_expired()) {
                        log::warn!("Deletion cleanup failed: {}", error.code);
                    }
                })
                .await;
            });
        }
        Ok(count)
    })
    .await
}

#[tauri::command]
pub async fn list_item_ids(app: AppHandle, query: ListQuery) -> Result<Vec<i64>> {
    blocking(app, move |a| {
        a.state::<AppState>().db(|db| db.matching_ids(&query))
    })
    .await
}
#[tauri::command]
pub async fn undo_delete(app: AppHandle) -> Result<usize> {
    blocking(app, |a| {
        let count = a.state::<AppState>().db(|db| db.undo_delete())?;
        if count > 0 {
            shell::changed(a, "added", None);
        }
        Ok(count)
    })
    .await
}
#[tauri::command]
pub async fn get_merged_text(app: AppHandle, ids: Vec<i64>, separator: String) -> Result<String> {
    blocking(app, move |a| {
        a.state::<AppState>()
            .db(|db| db.merged_text(&ids, &separator))
    })
    .await
}
#[tauri::command]
pub async fn copy_merged(app: AppHandle, ids: Vec<i64>, separator: String) -> Result<()> {
    blocking(app, move |a| crate::paste::copy_merged(a, &ids, &separator)).await
}
#[tauri::command]
pub async fn list_groups(app: AppHandle) -> Result<Vec<String>> {
    blocking(app, |a| a.state::<AppState>().db(|db| db.groups())).await
}
#[tauri::command]
pub async fn create_group(app: AppHandle, name: String) -> Result<()> {
    blocking(app, move |a| {
        a.state::<AppState>().db(|db| db.create_group(name))?;
        shell::changed(a, "updated", None);
        Ok(())
    })
    .await
}
#[tauri::command]
pub async fn create_snippet(
    app: AppHandle,
    group: Option<String>,
    text: String,
) -> Result<ClipItem> {
    blocking(app, move |a| {
        let item = a
            .state::<AppState>()
            .db(|db| db.create_snippet(group, text))?;
        shell::changed(a, "added", Some(item.id));
        Ok(item)
    })
    .await
}
#[tauri::command]
pub async fn edit_snippet(app: AppHandle, id: i64, text: String) -> Result<ClipItem> {
    blocking(app, move |a| {
        let item = a.state::<AppState>().db(|db| db.edit_snippet(id, text))?;
        shell::changed(a, "updated", Some(id));
        Ok(item)
    })
    .await
}
#[tauri::command]
pub async fn move_favorite_item(
    app: AppHandle,
    id: i64,
    direction: FavoriteDirection,
) -> Result<bool> {
    blocking(app, move |a| {
        let moved = a
            .state::<AppState>()
            .db(|db| db.move_favorite(id, direction))?;
        if moved {
            shell::changed(a, "updated", Some(id));
        }
        Ok(moved)
    })
    .await
}
#[tauri::command]
pub async fn set_item_group(app: AppHandle, ids: Vec<i64>, group: Option<String>) -> Result<usize> {
    blocking(app, move |a| {
        let count = a.state::<AppState>().db(|db| db.set_group(&ids, group))?;
        shell::changed(a, "updated", None);
        Ok(count)
    })
    .await
}
#[tauri::command]
pub async fn rename_group(
    app: AppHandle,
    name: String,
    replacement: Option<String>,
) -> Result<usize> {
    blocking(app, move |a| {
        let count = a
            .state::<AppState>()
            .db(|db| db.rename_group(name, replacement))?;
        shell::changed(a, "updated", None);
        Ok(count)
    })
    .await
}
#[tauri::command]
pub async fn webdav_test(app: AppHandle, connection: crate::backup::Connection) -> Result<()> {
    blocking(app, move |_| {
        crate::backup::test_connection(&crate::credentials::resolve_connection(connection)?)
    })
    .await
}
#[tauri::command]
pub async fn webdav_list(
    app: AppHandle,
    connection: crate::backup::Connection,
) -> Result<Vec<crate::backup::BackupEntry>> {
    blocking(app, move |_| {
        crate::backup::list(&crate::credentials::resolve_connection(connection)?)
    })
    .await
}
#[tauri::command]
pub async fn webdav_password_status(
    app: AppHandle,
    connection: crate::backup::Connection,
) -> Result<crate::credentials::PasswordStatus> {
    blocking(app, move |_| crate::credentials::status(&connection)).await
}
#[tauri::command]
pub async fn webdav_save_password(
    app: AppHandle,
    connection: crate::backup::Connection,
    kind: crate::credentials::PasswordKind,
    value: Option<String>,
) -> Result<()> {
    let value = value.map(Zeroizing::new);
    blocking(app, move |_| {
        crate::credentials::save(&connection, kind, value)
    })
    .await
}
#[tauri::command]
pub async fn webdav_backup(
    app: AppHandle,
    connection: crate::backup::Connection,
    password: String,
) -> Result<crate::backup::BackupEntry> {
    let password = Zeroizing::new(password);
    blocking(app, move |a| {
        let _gate = crate::backup::GATE
            .try_lock()
            .map_err(|_| CmdError::invalid("A backup or restore is already running"))?;
        let connection = crate::credentials::resolve_connection(connection)?;
        let password = crate::credentials::resolve_encryption(&connection, password)?;
        let state = a.state::<AppState>();
        let settings = state.settings()?;
        let archive = state.db(|db| crate::backup::snapshot(db, settings))?;
        crate::backup::upload(&connection, &archive, &password)
    })
    .await
}
#[tauri::command]
pub async fn webdav_restore(
    app: AppHandle,
    connection: crate::backup::Connection,
    name: String,
    password: String,
    restore_settings: bool,
    legacy: Option<bool>,
) -> Result<crate::backup::RestoreResult> {
    let password = Zeroizing::new(password);
    blocking(app, move |a| {
        let _gate = crate::backup::GATE
            .try_lock()
            .map_err(|_| CmdError::invalid("A backup or restore is already running"))?;
        let connection = crate::credentials::resolve_connection(connection)?;
        let password = crate::credentials::resolve_encryption(&connection, password)?;
        let archive =
            crate::backup::download(&connection, &name, &password, legacy.unwrap_or(false))?;
        let records = crate::backup::prepare(archive.records)?;
        let mut patch = serde_json::to_value(archive.settings).map_err(|_| CmdError::internal())?;
        let map = patch.as_object_mut().ok_or_else(CmdError::internal)?;
        for key in ["hotkey", "autostart", "runAsAdmin", "winV"] {
            map.remove(key);
        }
        a.state::<AppState>().settings()?.patch(patch.clone())?;
        let mut result = a
            .state::<AppState>()
            .db(|db| db.restore_records(records, archive.folders.unwrap_or_default()))?;
        shell::changed(a, "added", None);
        if restore_settings {
            result.settings_restored = Some(match shell::update(a, patch) {
                Ok(_) => true,
                Err(error) => {
                    log::warn!(
                        "Records restored but preferences could not be applied: {}",
                        error.code
                    );
                    false
                }
            });
        }
        Ok(result)
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
/// User-triggered signed-feed check; no scheduled checks or installer download.
#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<UpdateInfo> {
    crate::update::check(&app).await
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<()> {
    crate::update::install(&app).await
}

#[tauri::command]
pub fn get_update_progress(app: AppHandle) -> Result<UpdateProgress> {
    app.state::<crate::update::State>().progress()
}
#[tauri::command]
pub fn get_app_info(app: AppHandle) -> Result<AppInfo> {
    Ok(AppInfo {
        version: app.package_info().version.to_string(),
        data_dir: app.state::<AppState>().dir.to_string_lossy().into_owned(),
        elevated: platform::self_elevated(),
    })
}

#[tauri::command]
pub async fn restart_as_admin(app: AppHandle) -> Result<bool> {
    let launched = blocking(app.clone(), |a| {
        crate::elevation::launch(&a.state::<AppState>().dir, true)
    })
    .await?;
    if launched {
        app.exit(0);
    }
    Ok(launched)
}
