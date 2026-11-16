//! Explicit UAC restart, with the replacement waiting before single-instance initialization.

use crate::{
    model::{CmdError, Result},
    platform,
};
use std::{
    ffi::{OsStr, OsString},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{
            CloseHandle, ERROR_CANCELLED, ERROR_INVALID_PARAMETER, HANDLE, WAIT_OBJECT_0,
        },
        System::{
            Com::{
                CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
            },
            Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
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

const RESTART: &str = "--elevated-restart";
const DATA_DIR: &str = "--elevation-data-dir";

struct Process(HANDLE);
impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns a non-null process handle and closes it once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Quote one Windows command-line argument, preserving Unicode, embedded quotes and trailing
/// backslashes. ShellExecuteEx receives a parameter string rather than an argument array.
fn quote(value: &OsStr) -> Vec<u16> {
    let mut out = vec![b'"' as u16];
    let mut slashes = 0;
    for c in value.encode_wide() {
        if c == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        if c == b'"' as u16 {
            out.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2 + 1));
        } else {
            out.extend(std::iter::repeat_n(b'\\' as u16, slashes));
        }
        slashes = 0;
        out.push(c);
    }
    out.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    out.push(b'"' as u16);
    out
}

fn handoff(args: &[OsString]) -> Result<Option<(u32, PathBuf)>> {
    if !args.iter().any(|a| a == RESTART) {
        return Ok(None);
    }
    if args.len() != 4 || args[0] != RESTART || args[2] != DATA_DIR {
        return Err(CmdError::invalid("Invalid elevation restart arguments"));
    }
    let pid = args[1]
        .to_str()
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|pid| *pid != 0 && *pid != std::process::id())
        .ok_or_else(|| CmdError::invalid("Invalid elevation restart process"))?;
    let dir = PathBuf::from(&args[3]);
    if !dir.is_absolute() {
        return Err(CmdError::invalid(
            "Elevation data directory must be absolute",
        ));
    }
    Ok(Some((pid, dir)))
}

/// Must run before building Tauri: otherwise the new process hits the existing instance's
/// mutex and quits, leaving no elevated replacement after the original process exits.
pub fn prepare() -> Result<Option<PathBuf>> {
    let Some((pid, dir)) = handoff(&std::env::args_os().skip(1).collect::<Vec<_>>())? else {
        return Ok(None);
    };
    if !platform::self_elevated() {
        return Err(CmdError::new(
            "io",
            "The replacement process was not elevated",
        ));
    }
    wait_for_exit(pid)?;
    Ok(Some(dir))
}

fn wait_for_exit(pid: u32) -> Result<()> {
    // SAFETY: valid access flags; the returned process handle is owned by the RAII wrapper.
    let process = match unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) } {
        Ok(handle) => Process(handle),
        // The original may have exited before the replacement even reached this function.
        Err(e) if e.code() == ERROR_INVALID_PARAMETER.to_hresult() => return Ok(()),
        Err(_) => {
            return Err(CmdError::new(
                "io",
                "Could not wait for the original process",
            ))
        }
    };
    // SAFETY: process.0 stays open until the bounded wait returns.
    if unsafe { WaitForSingleObject(process.0, 30_000) } != WAIT_OBJECT_0 {
        return Err(CmdError::new(
            "io",
            "The original process did not exit in time",
        ));
    }
    Ok(())
}

/// Returns false for UAC cancellation or an already-elevated process. The caller exits the
/// original only after this returns true, so a declined prompt never shuts down Copybook.
pub fn launch(dir: &Path) -> Result<bool> {
    if platform::self_elevated() {
        return Ok(false);
    }
    let exe =
        std::env::current_exe().map_err(|_| CmdError::new("io", "Could not locate Copybook"))?;
    let exe: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut parameters: Vec<u16> = format!("{RESTART} {} {DATA_DIR} ", std::process::id())
        .encode_utf16()
        .collect();
    parameters.extend(quote(dir.as_os_str()));
    parameters.push(0);
    // SAFETY: initialize COM on this worker thread for ShellExecute's shell extensions.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }
        .ok()
        .map_err(|_| CmdError::new("io", "Could not initialize elevation request"))?;
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(exe.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: initialized structure, valid executable and parameter buffers live through the
    // synchronous call. Windows displays UAC; no shell command or arbitrary executable is used.
    let result = unsafe { ShellExecuteExW(&mut info) };
    // SAFETY: balances the successful CoInitializeEx on this same worker thread.
    unsafe {
        CoUninitialize();
    }
    match result {
        Err(e) if e.code() == ERROR_CANCELLED.to_hresult() => Ok(false),
        Err(_) => Err(CmdError::new(
            "io",
            "Could not restart Copybook as administrator",
        )),
        Ok(()) if info.hProcess.is_invalid() => {
            Err(CmdError::new("io", "The replacement did not start"))
        }
        Ok(()) => {
            drop(Process(info.hProcess));
            Ok(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_windows_paths_and_arguments() {
        for (input, expected) in [
            ("", "\"\""),
            (r"C:\用户\Copybook data", "\"C:\\用户\\Copybook data\""),
            ("C:\\space dir\\", "\"C:\\space dir\\\\\""),
            ("a\\\"b", "\"a\\\\\\\"b\""),
        ] {
            assert_eq!(
                String::from_utf16(&quote(OsStr::new(input))).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn validates_handoff_before_waiting() {
        let args = |a: &[&str]| a.iter().map(OsString::from).collect::<Vec<_>>();
        assert!(handoff(&args(&["--autostart"])).unwrap().is_none());
        let parsed = handoff(&args(&[RESTART, "42", DATA_DIR, r"C:\用户\Copybook data"]))
            .unwrap()
            .unwrap();
        assert_eq!(parsed, (42, PathBuf::from(r"C:\用户\Copybook data")));
        for bad in [
            vec![RESTART],
            vec![RESTART, "bad", DATA_DIR, r"C:\data"],
            vec![RESTART, "0", DATA_DIR, r"C:\data"],
            vec![RESTART, "42", DATA_DIR, "relative"],
            vec![RESTART, "42", "--other", r"C:\data"],
            vec![RESTART, "42", DATA_DIR, r"C:\data", "--extra"],
        ] {
            assert!(handoff(&args(&bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn waits_for_an_owned_child_to_exit() {
        use std::{
            os::windows::process::CommandExt,
            process::{Command, Stdio},
        };
        use windows::Win32::System::Threading::CREATE_NO_WINDOW;

        // Reuse the test runner's harmless --list mode to exercise a real Windows process
        // handle. No Copybook instance, UAC dialog or desktop window is launched.
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .creation_flags(CREATE_NO_WINDOW.0)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        wait_for_exit(child.id()).unwrap();
        assert!(child.try_wait().unwrap().is_some());
    }
}
