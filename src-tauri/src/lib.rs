mod clipboard;
mod commands;
mod detect;
mod formats;
mod hook;
pub mod model;
mod paste;
mod platform;
mod search;
mod settings;
mod shell;
mod state;
mod storage;
mod update;
mod window;

use model::{CmdError, Result};
use state::AppState;
use std::sync::{atomic::AtomicBool, Arc, Mutex, RwLock};
use tauri::Manager;

fn initialize(app: &tauri::AppHandle) -> Result<()> {
    let state = app.state::<AppState>();
    let (settings, first) = settings::Settings::load(&state.dir)?;
    let mut db = storage::Storage::open(&state.dir)?;
    let removed = db.cleanup(&settings)?;
    *state.storage.lock().map_err(|_| CmdError::internal())? = Some(db);
    *state.settings.write().map_err(|_| CmdError::internal())? = settings;
    let saved = std::fs::read(state.dir.join("state.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    state.panel.lock().map_err(|_| CmdError::internal())?.saved = saved;
    let handle = app.clone();
    let changed = Arc::new(move |reason, id| shell::changed(&handle, reason, id));
    let pipeline = clipboard::Pipeline::start(
        state.dir.clone(),
        state.storage.clone(),
        state.settings.clone(),
        state.paused.clone(),
        state.writer.clone(),
        changed,
    )?;
    *state.pipeline.lock().map_err(|_| CmdError::internal())? = Some(pipeline);
    if removed > 0 {
        shell::changed(app, "cleanup", None);
    }
    let handle = app.clone();
    app.run_on_main_thread(move || {
        if let Err(e) = shell::start(&handle) {
            log::error!("Shell startup failed: {}", e.code);
        }
        if first && !std::env::args().any(|a| a == "--autostart") {
            let _ = window::open_settings(&handle, None);
        }
        log::info!("Backend ready");
    })
    .map_err(|_| CmdError::internal())?;
    Ok(())
}

pub fn run() {
    let override_dir = match std::env::var_os("COPYBOOK_DATA_DIR") {
        Some(dir) => match std::path::absolute(std::path::PathBuf::from(dir)) {
            Ok(dir) => Some(dir),
            Err(_) => {
                eprintln!("Invalid data directory: io");
                return;
            }
        },
        None => None,
    };
    let mut context = tauri::generate_context!();
    // A data override also isolates WebView2 and logs during local smoke tests.
    if let Some(dir) = &override_dir {
        for window in &mut context.config_mut().app.windows {
            window.data_directory = Some(dir.join("webview"));
        }
    }
    let mut logger = tauri_plugin_log::Builder::new().level(log::LevelFilter::Info);
    if let Some(dir) = &override_dir {
        logger = logger.clear_targets().target(tauri_plugin_log::Target::new(
            tauri_plugin_log::TargetKind::Folder {
                path: dir.join("logs"),
                file_name: Some("copybook".into()),
            },
        ));
    }
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            let app = app.clone();
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                if handle.try_state::<AppState>().is_some() {
                    let _ = window::show(&handle);
                }
            });
        }))
        .plugin(logger.build())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .arg("--autostart")
                .build(),
        )
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _, event| {
                    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        shell::queue_toggle(app);
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            commands::list_items,
            commands::get_item,
            commands::get_item_content,
            commands::paste_item,
            commands::copy_item,
            commands::set_favorite,
            commands::set_pinned,
            commands::set_note,
            commands::delete_items,
            commands::clear_history,
            commands::get_stats,
            commands::list_seen_apps,
            commands::get_settings,
            commands::update_settings,
            commands::get_capture_paused,
            commands::set_capture_paused,
            commands::hide_panel,
            commands::set_panel_locked,
            commands::open_settings,
            commands::open_url,
            commands::open_path,
            commands::reveal_path,
            commands::open_data_dir,
            commands::get_app_info,
            commands::check_update
        ])
        .setup(move |app| {
            let dir = match &override_dir {
                Some(dir) => std::path::absolute(dir)?,
                None => app.path().app_data_dir()?,
            };
            let scope = app.asset_protocol_scope();
            scope.allow_directory(dir.join("images"), true)?;
            scope.allow_directory(dir.join("icons"), true)?;
            app.manage(AppState {
                dir,
                storage: Arc::new(Mutex::new(None)),
                settings: Arc::new(RwLock::new(settings::Settings::default())),
                settings_gate: Mutex::new(()),
                paused: Arc::new(AtomicBool::new(false)),
                writer: Arc::new(clipboard::Writer::default()),
                panel: Mutex::new(window::PanelState::default()),
                hook: Mutex::new(None),
                pipeline: Mutex::new(None),
                initialization: (Mutex::new(None), std::sync::Condvar::new()),
            });
            window::install(app.handle())?;
            let handle = app.handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                let result = initialize(&handle);
                handle.state::<AppState>().initialized(result.clone());
                if let Err(e) = result {
                    log::error!("Backend startup failed: {}", e.code);
                    let app = handle.clone();
                    // Keep a tray and Quit action available even if the data directory failed.
                    let _ = handle.run_on_main_thread(move || {
                        let _ = shell::start(&app);
                    });
                }
            });
            Ok(())
        })
        .build(context);
    match result {
        Ok(app) => app.run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(mut hook) = state.hook.lock() {
                        hook.take();
                    }
                    if let Ok(mut pipeline) = state.pipeline.lock() {
                        pipeline.take();
                    }
                }
            }
        }),
        Err(_) => log::error!("Application startup failed: internal"),
    }
}

#[cfg(test)]
pub fn test_dir() -> std::path::PathBuf {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.review/tests");
    std::fs::create_dir_all(&path).unwrap();
    path
}
