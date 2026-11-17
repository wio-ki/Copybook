//! User-triggered signed updates. The official updater verifies the NSIS payload;
//! our launcher exits through Tauri only after Windows starts the installer.

use crate::{
    model::{CmdError, Result, UpdateInfo, UpdateProgress},
    state::AppState,
};
use std::{
    io::{Read, Write},
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::CloseHandle,
        Storage::FileSystem::FILE_SHARE_READ,
        System::Com::{
            CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
        },
        UI::{
            Shell::{
                ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS,
                SHELLEXECUTEINFOW,
            },
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    },
};

const RELEASES: &str = "https://github.com/wio-ki/Copybook/releases";

#[derive(Default)]
struct Session {
    pending: Option<Update>,
    progress: UpdateProgress,
}

#[derive(Default)]
pub struct State {
    busy: AtomicBool,
    session: Mutex<Session>,
}

struct Operation<'a>(&'a AtomicBool);
impl Drop for Operation<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl State {
    fn acquire(&self) -> Result<Operation<'_>> {
        self.busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| CmdError::invalid("An update operation is already running"))?;
        Ok(Operation(&self.busy))
    }

    pub fn progress(&self) -> Result<UpdateProgress> {
        Ok(self
            .session
            .lock()
            .map_err(|_| CmdError::internal())?
            .progress
            .clone())
    }

    fn report(&self, app: &AppHandle, progress: UpdateProgress) -> Result<()> {
        self.session
            .lock()
            .map_err(|_| CmdError::internal())?
            .progress = progress.clone();
        let _ = app.emit("update://progress", progress);
        Ok(())
    }
}

fn failure(error: impl std::fmt::Display) -> CmdError {
    log::warn!("Update failed: {error}");
    CmdError::new("network", "Could not retrieve or verify the signed update")
}

/// Only the project's official HTTPS installer assets can reach the Windows launcher.
fn official_installer(url: &url::Url, version: &str) -> bool {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return false;
    }
    match url.host_str() {
        Some("github.com") => {
            url.path()
                == format!(
                "/wio-ki/Copybook/releases/download/v{version}/Copybook_{version}_x64-setup.exe"
            )
        }
        Some("api.github.com") => url
            .path()
            .strip_prefix("/repos/wio-ki/Copybook/releases/assets/")
            .is_some_and(|id| !id.is_empty() && id.bytes().all(|c| c.is_ascii_digit())),
        _ => false,
    }
}

pub async fn check(app: &AppHandle) -> Result<UpdateInfo> {
    let state = app.state::<State>();
    let _operation = state.acquire()?;
    state
        .session
        .lock()
        .map_err(|_| CmdError::internal())?
        .pending = None;
    let found = app
        .updater_builder()
        .timeout(Duration::from_secs(25))
        .build()
        .map_err(failure)?
        .check()
        .await
        .map_err(failure)?;
    let current = app.package_info().version.to_string();
    let info = match &found {
        Some(update) => {
            if !official_installer(&update.download_url, &update.version) {
                return Err(failure(
                    "The update does not point to an official Windows installer",
                ));
            }
            UpdateInfo {
                current,
                latest: update.version.clone(),
                newer: true,
                url: format!("{RELEASES}/tag/v{}", update.version),
                installer: Some(update.download_url.to_string()),
            }
        }
        None => UpdateInfo {
            current: current.clone(),
            latest: current,
            newer: false,
            url: format!("{RELEASES}/latest"),
            installer: None,
        },
    };
    state
        .session
        .lock()
        .map_err(|_| CmdError::internal())?
        .pending = found;
    state.report(app, UpdateProgress::default())?;
    Ok(info)
}

/// Build fresh ordinary startup arguments; never replay an old UAC handoff PID.
fn installer_parameters(data_dir: &Path, install_dir: &Path) -> Vec<u16> {
    let mut parameters: Vec<u16> = "/P /UPDATE /R /ARGS --data-dir ".encode_utf16().collect();
    parameters.extend(crate::elevation::quote(data_dir.as_os_str()));
    // NSIS requires /D to be the final, unquoted option, even when the path contains spaces.
    parameters.extend(" /D=".encode_utf16());
    parameters.extend(install_dir.as_os_str().encode_wide());
    parameters.push(0);
    parameters
}

fn stage_installer(dir: &Path, version: &str, bytes: &[u8]) -> Result<PathBuf> {
    // Validate the version before using it in a path, even though the official feed parser
    // already requires a SemVer. Payload authenticity was checked by Update::download.
    if version.is_empty()
        || !version
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b".-+".contains(&c))
        || !bytes.starts_with(b"MZ")
    {
        return Err(CmdError::invalid("Invalid Windows update payload"));
    }
    let dir = dir.join("updates");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("Copybook_{version}_x64-setup.exe"));
    let temp = path.with_extension("part");
    let mut file = std::fs::File::create(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temp, &path)?;
    Ok(path)
}

fn launch_installer(
    path: &Path,
    expected: &[u8],
    data_dir: &Path,
    install_dir: &Path,
) -> Result<()> {
    // Deny writes/deletes while comparing and starting the executable so a verified payload
    // cannot be swapped between staging and CreateProcess inside ShellExecuteExW.
    let mut locked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .open(path)?;
    let mut actual = Vec::new();
    locked.read_to_end(&mut actual)?;
    if actual != expected {
        return Err(CmdError::new("io", "Staged update changed before launch"));
    }
    let file: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let parameters = installer_parameters(data_dir, install_dir);
    // SAFETY: initialize COM on the command worker for ShellExecute shell extensions.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }
        .ok()
        .map_err(|_| CmdError::new("io", "Could not initialize the updater launcher"))?;
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_NO_UI,
        lpVerb: w!("open"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: sized structure and NUL-terminated buffers live through this synchronous call;
    // only the locked, signature-verified installer is executable here. No shell text is run.
    let result = unsafe { ShellExecuteExW(&mut info) };
    // SAFETY: balances successful COM initialization on this worker thread.
    unsafe {
        CoUninitialize();
    }
    result.map_err(|_| CmdError::new("io", "Windows could not start the update installer"))?;
    if info.hProcess.is_invalid() {
        return Err(CmdError::new("io", "The updater process did not start"));
    }
    // SAFETY: ShellExecuteExW transferred ownership of this process handle to us.
    unsafe {
        let _ = CloseHandle(info.hProcess);
    }
    Ok(())
}

pub async fn install(app: &AppHandle) -> Result<()> {
    let state = app.state::<State>();
    let _operation = state.acquire()?;
    let mut update = state
        .session
        .lock()
        .map_err(|_| CmdError::internal())?
        .pending
        .clone()
        .ok_or_else(|| CmdError::invalid("Check for a newer signed update first"))?;
    update.timeout = Some(Duration::from_secs(300));
    state.report(
        app,
        UpdateProgress {
            stage: "downloading",
            version: Some(update.version.clone()),
            downloaded: 0,
            total: None,
        },
    )?;
    let result: Result<()> = async {
        let mut received = 0u64;
        let bytes = update
            .download(
                |chunk, total| {
                    received = received.saturating_add(chunk as u64);
                    let _ = state.report(
                        app,
                        UpdateProgress {
                            stage: "downloading",
                            version: Some(update.version.clone()),
                            downloaded: received,
                            total,
                        },
                    );
                },
                || {},
            )
            .await
            .map_err(failure)?;
        state.report(
            app,
            UpdateProgress {
                stage: "installing",
                version: Some(update.version.clone()),
                downloaded: bytes.len() as u64,
                total: Some(bytes.len() as u64),
            },
        )?;
        let data_dir = app.state::<AppState>().dir.clone();
        let version = update.version.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let exe = std::env::current_exe()?;
            let install_dir = exe.parent().ok_or_else(CmdError::internal)?;
            let path = stage_installer(&data_dir, &version, &bytes)?;
            launch_installer(&path, &bytes, &data_dir, install_dir)
        })
        .await
        .map_err(|_| CmdError::internal())??;
        Ok(())
    }
    .await;
    if let Err(error) = result {
        log::warn!("Update installation failed: {}", error.message);
        let mut progress = state.progress()?;
        progress.stage = "error";
        state.report(app, progress)?;
        return Err(error);
    }
    // Use the normal Tauri Exit path: it stops capture/hooks and releases the instance lock.
    // A failed download, signature check or installer launch never exits the running app.
    app.exit(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permits_only_official_https_installer_assets() {
        for value in ["https://github.com/wio-ki/Copybook/releases/download/v1.3.0/Copybook_1.3.0_x64-setup.exe", "https://api.github.com/repos/wio-ki/Copybook/releases/assets/123"] {
            assert!(official_installer(&url::Url::parse(value).unwrap(), "1.3.0"));
        }
        for value in ["http://api.github.com/repos/wio-ki/Copybook/releases/assets/123", "https://api.github.com/repos/other/Copybook/releases/assets/123", "https://api.github.com/repos/wio-ki/Copybook/releases/assets/../123", "https://github.com/wio-ki/Copybook/releases/download/v1.2.0/Copybook_1.2.0_x64-setup.exe", "https://github.com.evil.test/wio-ki/Copybook/releases/download/v1.3.0/Copybook_1.3.0_x64-setup.exe", "https://user@api.github.com/repos/wio-ki/Copybook/releases/assets/123", "https://api.github.com/repos/wio-ki/Copybook/releases/assets/123?q=x"] {
            assert!(!official_installer(&url::Url::parse(value).unwrap(), "1.3.0"), "{value}");
        }
    }

    #[test]
    fn restart_arguments_are_fresh_and_preserve_unicode_paths() {
        let args = installer_parameters(
            Path::new(r"C:\用户\Copybook data"),
            Path::new(r"C:\Program Files\Copybook"),
        );
        let text = String::from_utf16(&args[..args.len() - 1]).unwrap();
        assert_eq!(text, "/P /UPDATE /R /ARGS --data-dir \"C:\\用户\\Copybook data\" /D=C:\\Program Files\\Copybook");
        assert!(!text.contains("--elevated-restart"));
        assert!(!text.contains("--elevation-data-dir"));
    }

    #[test]
    fn stages_complete_installers_and_rejects_path_traversal() {
        let dir = tempfile::tempdir_in(crate::test_dir()).unwrap();
        let path = stage_installer(dir.path(), "1.3.0", b"MZtest-signed-payload").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"MZtest-signed-payload");
        assert!(stage_installer(dir.path(), "../escape", b"MZ").is_err());
        assert!(stage_installer(dir.path(), "1.3.0", b"unsigned text").is_err());
        assert!(stage_installer(dir.path(), "1.3.0", b"MZreplacement").is_ok());
    }

    #[test]
    fn concurrent_operations_are_rejected_and_errors_can_retry() {
        let state = State::default();
        let operation = state.acquire().unwrap();
        assert!(state.acquire().is_err());
        drop(operation);
        assert!(state.acquire().is_ok());
        assert_eq!(state.progress().unwrap().stage, "idle");
    }
}
