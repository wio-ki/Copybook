use crate::{model::*, platform, shell, state::AppState, window};
use tauri::{AppHandle, Manager};

pub fn copy(app: &AppHandle, id: i64, plain: bool) -> Result<()> {
    let state = app.state::<AppState>();
    let content = state.db(|db| db.content(id))?;
    let owner = app
        .get_webview_window("panel")
        .ok_or_else(CmdError::internal)?
        .hwnd()
        .map_err(|_| CmdError::internal())?;
    state
        .writer
        .write(&content, plain, platform::hwnd(owner.0 as isize))?;
    let move_to_top = state.settings()?.move_to_top;
    state.db(|db| db.bump(id, move_to_top))?;
    shell::changed(app, "bumped", Some(id));
    Ok(())
}
pub fn paste(app: &AppHandle, id: i64, plain: bool) -> Result<PasteOutcome> {
    copy(app, id, plain)?;
    let state = app.state::<AppState>();
    let settings = state.settings()?;
    if !settings.auto_paste {
        window::hide(app, true)?;
        return Ok(PasteOutcome {
            pasted: false,
            reason: Some("paste_disabled"),
        });
    }
    let target = state
        .panel
        .lock()
        .map_err(|_| CmdError::internal())?
        .target
        .and_then(|t| platform::target(platform::hwnd(t)));
    let Some(target) = target else {
        window::hide(app, false)?;
        return Ok(PasteOutcome {
            pasted: false,
            reason: Some("no_target"),
        });
    };
    let target = platform::hwnd(target);
    if !platform::self_elevated() && platform::elevated(platform::pid(target)) {
        return Ok(PasteOutcome {
            pasted: false,
            reason: Some("elevated_target"),
        });
    }
    // Hand the foreground over while the panel still owns it; once hidden, Windows may refuse.
    platform::force_foreground(target);
    window::hide(app, false)?;
    platform::paste_to(target)?;
    Ok(PasteOutcome {
        pasted: true,
        reason: None,
    })
}
