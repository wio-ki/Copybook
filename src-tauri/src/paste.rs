use crate::{model::*, platform, shell, state::AppState, window};
use sha2::{Digest, Sha256};
use std::{io::Write, path::Path, sync::Mutex};
use tauri::{AppHandle, Manager};

static EXPORT_GATE: Mutex<()> = Mutex::new(());

fn terminal_image_path(
    content: &crate::storage::StoredContent,
    dir: &Path,
) -> Result<Option<String>> {
    if content.item.kind != ClipKind::Image {
        return Ok(None);
    }
    let info = content.item.image.as_ref().ok_or_else(CmdError::internal)?;
    let data = std::fs::read(&info.path)?;
    let export_dir = dir.join("paste-images");
    std::fs::create_dir_all(&export_dir)?;
    let path = export_dir.join(format!("{:x}.png", Sha256::digest(&data)));
    let _gate = EXPORT_GATE.lock().map_err(|_| CmdError::internal())?;
    if std::fs::read(&path).ok().as_deref() != Some(data.as_slice()) {
        // Publish a complete PNG atomically. Concurrent pastes and interrupted writes must
        // never hand a CLI a partially written file; also repair an invalid cached copy.
        let temp = path.with_extension("tmp");
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(&data)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, &path)?;
    }
    // One native absolute path, quoted for spaces/Unicode. Never append Enter/newline:
    // the CLI must attach the image without submitting the user's prompt.
    Ok(Some(format!("\"{}\"", path.display())))
}

pub fn copy(app: &AppHandle, id: i64, plain: bool) -> Result<()> {
    copy_for_target(app, id, plain, false)
}

fn copy_for_target(app: &AppHandle, id: i64, plain: bool, terminal: bool) -> Result<()> {
    let state = app.state::<AppState>();
    let content = state.db(|db| db.content(id))?;
    let owner = app
        .get_webview_window("panel")
        .ok_or_else(CmdError::internal)?
        .hwnd()
        .map_err(|_| CmdError::internal())?;
    let image_path = if terminal {
        terminal_image_path(&content, &state.dir)?
    } else {
        None
    };
    if let Some(path) = image_path.as_deref() {
        state.writer.write_for_paste(
            &content,
            plain,
            platform::hwnd(owner.0 as isize),
            Some(path),
        )?;
    } else {
        state
            .writer
            .write(&content, plain, platform::hwnd(owner.0 as isize))?;
    }
    let move_to_top = state.settings()?.move_to_top;
    state.db(|db| db.bump(id, move_to_top))?;
    shell::changed(app, "bumped", Some(id));
    Ok(())
}
pub fn paste(app: &AppHandle, id: i64, plain: bool) -> Result<PasteOutcome> {
    let state = app.state::<AppState>();
    let settings = state.settings()?;
    let target = state
        .panel
        .lock()
        .map_err(|_| CmdError::internal())?
        .target
        .and_then(|t| platform::target(platform::hwnd(t)));
    copy_for_target(
        app,
        id,
        plain,
        settings.auto_paste && target.is_some_and(|t| platform::terminal(platform::hwnd(t))),
    )?;
    if !settings.auto_paste {
        window::hide(app, true)?;
        return Ok(PasteOutcome {
            pasted: false,
            reason: Some("paste_disabled"),
        });
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{settings::Settings, storage::Storage};

    #[test]
    fn terminal_png_is_quoted_readable_and_survives_history_deletion() {
        let root = tempfile::tempdir_in(crate::test_dir()).unwrap();
        let dir = root.path().join("中文 CLI path with spaces");
        let mut db = Storage::open(&dir).unwrap();
        let pixels = image::RgbaImage::from_pixel(3, 2, image::Rgba([30, 70, 110, 200]));
        let mut capture = Capture::text("");
        capture.kind = ClipKind::Image;
        capture.pixels = Some(pixels.clone());
        let id = db.ingest(capture, &Settings::default()).unwrap().0;
        let content = db.content(id).unwrap();
        let quoted = terminal_image_path(&content, &dir).unwrap().unwrap();
        assert!(quoted.starts_with('"') && quoted.ends_with('"'));
        assert!(!quoted.contains(['\r', '\n']));
        let path = Path::new(&quoted[1..quoted.len() - 1]);
        assert!(path.is_absolute());
        assert_eq!(path.extension().unwrap(), "png");
        assert_eq!(
            terminal_image_path(&content, &dir).unwrap().unwrap(),
            quoted
        );
        std::fs::write(path, b"interrupted old export").unwrap();
        terminal_image_path(&content, &dir).unwrap();
        assert_eq!(image::open(path).unwrap().to_rgba8(), pixels);
        db.delete(&[id]).unwrap();
        assert!(!Path::new(&content.item.image.unwrap().path).exists());
        assert_eq!(image::open(path).unwrap().to_rgba8(), pixels);

        let id = db
            .ingest(Capture::text("text"), &Settings::default())
            .unwrap()
            .0;
        assert!(terminal_image_path(&db.content(id).unwrap(), &dir)
            .unwrap()
            .is_none());
    }
}
