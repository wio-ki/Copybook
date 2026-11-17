use crate::model::{CmdError, Result, SourceApp};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    time::{Duration, Instant},
};
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        Security::*,
        Storage::FileSystem::*,
        System::Threading::*,
        UI::{Input::KeyboardAndMouse::*, Shell::ExtractIconExW, WindowsAndMessaging::*},
    },
};

/// An unassigned virtual key, injected only to release the foreground lock or to break the
/// "Win was tapped alone" gesture. Unlike F24 it is not a real function key, so no other
/// program reacts to it.
pub const VK_PLACEHOLDER: VIRTUAL_KEY = VIRTUAL_KEY(0xE8);
/// True when the image matches one of the screen sizes exactly, the shape of a raw screenshot
/// of the whole display.
pub fn full_screen(pixels: &image::RgbaImage) -> bool {
    let (w, h) = (
        i32::try_from(pixels.width()).unwrap_or(i32::MAX),
        i32::try_from(pixels.height()).unwrap_or(i32::MAX),
    );
    // SAFETY: querying the display metrics has no side effects and no arguments.
    let virtual_size = unsafe {
        (
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    // SAFETY: as above, indexes are the documented metric constants.
    let primary = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    (w, h) == virtual_size || (w, h) == primary
}
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
pub fn hwnd(value: isize) -> HWND {
    HWND(value as *mut _)
}
pub fn pid(window: HWND) -> u32 {
    let mut pid = 0;
    // SAFETY: the API accepts stale/null HWNDs and writes into a live u32.
    unsafe {
        GetWindowThreadProcessId(window, Some(&mut pid));
    }
    pid
}
/// The recipient before the history panel takes focus. HWNDs alone can be recycled by
/// Windows, so keep the owning process/thread as well as the original input control.
#[derive(Clone, Copy, Debug)]
pub struct PasteTarget {
    pub window: isize,
    focus: Option<isize>,
    process: u32,
    thread: u32,
}
impl PasteTarget {
    pub fn valid(self) -> bool {
        let window = hwnd(self.window);
        // SAFETY: these queries accept stale handles and never dereference caller memory.
        unsafe {
            IsWindow(Some(window)).as_bool()
                && pid(window) == self.process
                && GetWindowThreadProcessId(window, None) == self.thread
        }
    }
    fn saved_focus(self) -> Option<HWND> {
        self.focus
            .map(hwnd)
            .filter(|focus| belongs_to(hwnd(self.window), *focus))
    }
    pub fn activate(self) -> bool {
        if !self.valid() || !force_foreground(hwnd(self.window)) {
            return false;
        }
        self.restore_focus();
        foreground() == hwnd(self.window)
    }
    fn restore_focus(self) {
        let Some(focus) = self.saved_focus() else {
            return;
        };
        // SAFETY: the saved child is validated against its live parent. Ensure this worker
        // has a message queue before attaching, and always detach before returning.
        unsafe {
            let current = GetCurrentThreadId();
            let other = GetWindowThreadProcessId(focus, None);
            let mut message = MSG::default();
            let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
            let attached =
                other != 0 && current != other && AttachThreadInput(current, other, true).as_bool();
            if current == other || attached {
                let _ = SetFocus(Some(focus));
            }
            if attached {
                let _ = AttachThreadInput(current, other, false);
            }
        }
    }
    fn focus_ready(self) -> bool {
        if !self.valid() || foreground() != hwnd(self.window) {
            return false;
        }
        let Some(expected) = self.saved_focus() else {
            return true;
        };
        gui_focus(self.thread).is_some_and(|focus| focus == expected || belongs_to(expected, focus))
    }
}
fn belongs_to(window: HWND, child: HWND) -> bool {
    // SAFETY: IsWindow/IsChild accept null and stale HWNDs.
    unsafe {
        IsWindow(Some(child)).as_bool() && (child == window || IsChild(window, child).as_bool())
    }
}
fn gui_focus(thread: u32) -> Option<HWND> {
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: info is initialized and sized for the documented Win32 structure.
    unsafe {
        GetGUIThreadInfo(thread, &mut info).ok()?;
    }
    (!info.hwndFocus.0.is_null()).then_some(info.hwndFocus)
}
pub fn target(window: HWND, excluded: HWND) -> Option<PasteTarget> {
    // SAFETY: all queried handles are checked with IsWindow; APIs accept null/stale handles.
    unsafe {
        if !IsWindow(Some(window)).as_bool() {
            return None;
        }
        // Owned dialogs are independent paste recipients. GA_ROOTOWNER would instead
        // activate their (often disabled) owner and lose the dialog's input control.
        let root = GetAncestor(window, GA_ROOT);
        if root.0.is_null()
            || root == excluded
            || !IsWindowVisible(root).as_bool()
            || !IsWindowEnabled(root).as_bool()
        {
            return None;
        }
        let mut class = [0u16; 256];
        let len = GetClassNameW(root, &mut class);
        let class = String::from_utf16_lossy(&class[..len.max(0) as usize]);
        if [
            "Progman",
            "WorkerW",
            "Shell_TrayWnd",
            "Shell_SecondaryTrayWnd",
            "NotifyIconOverflowWindow",
            "TopLevelWindowForOverflowXamlIsland",
        ]
        .contains(&class.as_str())
        {
            return None;
        }
        if class == "Windows.UI.Core.CoreWindow" {
            let exe = process_path(pid(root)).and_then(|p| {
                Path::new(&p)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_ascii_lowercase())
            });
            if exe.is_none_or(|s| {
                [
                    "searchhost.exe",
                    "searchapp.exe",
                    "startmenuexperiencehost.exe",
                    "shellexperiencehost.exe",
                ]
                .contains(&s.as_str())
            }) {
                return None;
            }
        }
        let thread = GetWindowThreadProcessId(root, None);
        Some(PasteTarget {
            window: root.0 as isize,
            focus: gui_focus(thread)
                .filter(|focus| belongs_to(root, *focus))
                .map(|focus| focus.0 as isize),
            process: pid(root),
            thread,
        })
    }
}
pub fn foreground() -> HWND {
    // SAFETY: no arguments or owned resources.
    unsafe { GetForegroundWindow() }
}

fn terminal_host(class: &str, exe: &str, title: &str) -> bool {
    if [
        "CASCADIA_HOSTING_WINDOW_CLASS",
        "ConsoleWindowClass",
        "mintty",
        "VirtualConsoleClass",
        "org.wezfurlong.wezterm",
        "Alacritty",
    ]
    .contains(&class)
    {
        return true;
    }
    let exe = exe.to_ascii_lowercase();
    if [
        "windowsterminal.exe",
        "wezterm-gui.exe",
        "alacritty.exe",
        "mintty.exe",
        "conemu.exe",
        "conemu64.exe",
    ]
    .contains(&exe.as_str())
    {
        return true;
    }
    // Integrated terminals share their top-level HWND with the editor. Only opt in when
    // its title explicitly names a CLI, rather than treating every editor as a terminal.
    [
        "code.exe",
        "code - insiders.exe",
        "cursor.exe",
        "windsurf.exe",
    ]
    .contains(&exe.as_str())
        && title
            .split(|c: char| !c.is_alphanumeric())
            .any(|s| s.eq_ignore_ascii_case("codex") || s.eq_ignore_ascii_case("claude"))
}

pub fn terminal(window: HWND) -> bool {
    let mut class = [0u16; 256];
    let mut title = [0u16; 1024];
    // SAFETY: both Win32 calls write only into the sized, live UTF-16 buffers.
    let (class_len, title_len) = unsafe {
        (
            GetClassNameW(window, &mut class),
            GetWindowTextW(window, &mut title),
        )
    };
    let exe = process_path(pid(window))
        .and_then(|p| {
            Path::new(&p)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    terminal_host(
        &String::from_utf16_lossy(&class[..class_len.max(0) as usize]),
        &exe,
        &String::from_utf16_lossy(&title[..title_len.max(0) as usize]),
    )
}
pub fn force_foreground(window: HWND) -> bool {
    // SAFETY: validate HWND; attached thread input is detached on every path below.
    unsafe {
        if !IsWindow(Some(window)).as_bool() {
            return false;
        }
        if IsIconic(window).as_bool() {
            let _ = ShowWindow(window, SW_RESTORE);
        }
        let _ = SetForegroundWindow(window);
        if GetForegroundWindow() == window {
            return true;
        }
        let current = GetCurrentThreadId();
        let other = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let mut message = MSG::default();
        let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
        let attached =
            other != 0 && other != current && AttachThreadInput(current, other, true).as_bool();
        let _ = BringWindowToTop(window);
        let _ = SetForegroundWindow(window);
        if attached {
            let _ = AttachThreadInput(current, other, false);
        }
        if GetForegroundWindow() == window {
            return true;
        }
        // A balanced placeholder press releases the foreground lock without modifiers and
        // without any other program mistaking it for a real key.
        let keys = [key(VK_PLACEHOLDER, false), key(VK_PLACEHOLDER, true)];
        let _ = SendInput(&keys, std::mem::size_of::<INPUT>() as i32);
        let _ = SetForegroundWindow(window);
        GetForegroundWindow() == window
    }
}
pub fn key(code: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: code,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                ..Default::default()
            },
        },
    }
}
pub fn paste_to(target: PasteTarget) -> Result<()> {
    let window = hwnd(target.window);
    if !target.activate() {
        return Err(CmdError::new(
            "internal",
            "Paste target did not receive focus",
        ));
    }
    let start = Instant::now();
    loop {
        // SAFETY: virtual key constants are valid; reading key state has no side effects.
        let down = unsafe {
            [
                VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN, VK_LBUTTON, VK_RBUTTON, VK_MBUTTON,
            ]
            .iter()
            .any(|k| GetAsyncKeyState(i32::from(k.0)) < 0)
        };
        if !down {
            break;
        }
        if start.elapsed() >= Duration::from_secs(1) {
            return Err(CmdError::new("internal", "Paste modifiers are still held"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    target.restore_focus();
    let start = Instant::now();
    let mut stable = None;
    while start.elapsed() < Duration::from_millis(500) {
        if !target.valid() || foreground() != window {
            return Err(CmdError::new("internal", "Paste target lost focus"));
        }
        if target.focus_ready() {
            if stable.get_or_insert_with(Instant::now).elapsed() >= Duration::from_millis(80) {
                break;
            }
        } else {
            stable = None;
            target.restore_focus();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !target.focus_ready() || stable.is_none_or(|time| time.elapsed() < Duration::from_millis(80))
    {
        return Err(CmdError::new(
            "internal",
            "Paste input control did not receive stable focus",
        ));
    }
    let keys = [
        key(VK_CONTROL, false),
        key(VK_V, false),
        key(VK_V, true),
        key(VK_CONTROL, true),
    ];
    // SAFETY: INPUT array contains balanced key presses and releases and correct struct size.
    let sent = unsafe { SendInput(&keys, std::mem::size_of::<INPUT>() as i32) };
    if sent != 4 {
        // SAFETY: release any keys potentially left down by a partially accepted batch.
        unsafe {
            let _ = SendInput(
                &[key(VK_V, true), key(VK_CONTROL, true)],
                std::mem::size_of::<INPUT>() as i32,
            );
        }
        return Err(CmdError::new("internal", "Windows rejected paste input"));
    }
    Ok(())
}
pub fn elevated(process_id: u32) -> bool {
    // SAFETY: opened process/token handles are closed exactly once and the output struct is sized.
    unsafe {
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) else {
            return true;
        };
        let mut token = HANDLE::default();
        let opened = OpenProcessToken(process, TOKEN_QUERY, &mut token);
        let _ = CloseHandle(process);
        if opened.is_err() {
            return true;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut needed = 0;
        let result = GetTokenInformation(
            token,
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut needed,
        );
        let _ = CloseHandle(token);
        result.is_err() || elevation.TokenIsElevated != 0
    }
}
pub fn self_elevated() -> bool {
    elevated(std::process::id())
}
pub fn process_path(id: u32) -> Option<String> {
    // SAFETY: process handle is closed; API writes into a correctly sized UTF-16 buffer.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, id).ok()?;
        let mut path = vec![0u16; 32768];
        let mut size = path.len() as u32;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(process);
        result.ok()?;
        Some(String::from_utf16_lossy(&path[..size as usize]))
    }
}
pub fn source(id: u32, dir: &Path) -> Option<SourceApp> {
    use std::{
        collections::HashMap,
        sync::{LazyLock, Mutex},
    };
    static CACHE: LazyLock<Mutex<HashMap<(std::path::PathBuf, String), SourceApp>>> =
        LazyLock::new(Default::default);
    let path = process_path(id)?;
    let cache_key = (dir.to_owned(), path.to_lowercase());
    if let Ok(cache) = CACHE.lock() {
        if let Some(source) = cache.get(&cache_key) {
            return Some(source.clone());
        }
    }
    let exe = Path::new(&path).file_name()?.to_string_lossy().into_owned();
    let name = description(&path).unwrap_or_else(|| {
        Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| exe.clone())
    });
    let digest = Sha256::digest(path.to_lowercase().as_bytes());
    let icon = dir.join("icons").join(format!("{digest:x}.png"));
    if !icon.exists() {
        if let Some(image) = exe_icon(&path) {
            if image.save(&icon).is_err() {
                log::warn!("Source icon cache write failed: io");
            }
        }
    }
    let source = SourceApp {
        name,
        exe,
        icon: icon.exists().then(|| icon.to_string_lossy().into_owned()),
    };
    if let Ok(mut cache) = CACHE.lock() {
        cache.insert(cache_key, source.clone());
    }
    Some(source)
}
fn description(path: &str) -> Option<String> {
    let path = wide(path);
    // SAFETY: version APIs use live buffers; returned pointers point into data until copied.
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(path.as_ptr()), None, size, data.as_mut_ptr().cast()).ok()?;
        let mut ptr = std::ptr::null_mut();
        let mut len = 0;
        let translation = wide("\\VarFileInfo\\Translation");
        let mut pairs = vec![(0x0409u16, 0x04b0u16)];
        if VerQueryValueW(
            data.as_ptr().cast(),
            PCWSTR(translation.as_ptr()),
            &mut ptr,
            &mut len,
        )
        .as_bool()
            && !ptr.is_null()
            && len >= 4
        {
            let bytes = std::slice::from_raw_parts(ptr.cast::<u8>(), len as usize);
            pairs = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| {
                    (
                        u16::from_le_bytes([b[0], b[1]]),
                        u16::from_le_bytes([b[2], b[3]]),
                    )
                })
                .collect();
        }
        for (language, page) in pairs {
            let query = wide(&format!(
                "\\StringFileInfo\\{language:04x}{page:04x}\\FileDescription"
            ));
            if VerQueryValueW(
                data.as_ptr().cast(),
                PCWSTR(query.as_ptr()),
                &mut ptr,
                &mut len,
            )
            .as_bool()
                && !ptr.is_null()
                && len > 1
            {
                let value = String::from_utf16_lossy(std::slice::from_raw_parts(
                    ptr.cast::<u16>(),
                    len as usize,
                ))
                .trim_matches('\0')
                .trim()
                .to_owned();
                if !value.is_empty() {
                    return Some(value);
                }
            }
        }
        None
    }
}
fn exe_icon(path: &str) -> Option<image::RgbaImage> {
    let path = wide(path);
    // SAFETY: GDI resources are released after rendering. The DIB has exactly 32*32*4 bytes.
    unsafe {
        let mut icon = HICON::default();
        if ExtractIconExW(PCWSTR(path.as_ptr()), 0, Some(&mut icon), None, 1) == 0
            || icon.is_invalid()
        {
            return None;
        }
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));
        let _ = ReleaseDC(None, screen);
        if dc.is_invalid() {
            let _ = DestroyIcon(icon);
            return None;
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: 40,
                biWidth: 32,
                biHeight: -32,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(b) => b,
            Err(_) => {
                let _ = DeleteDC(dc);
                let _ = DestroyIcon(icon);
                return None;
            }
        };
        let old = SelectObject(dc, bitmap.into());
        std::ptr::write_bytes(bits, 0, 32 * 32 * 4);
        let drawn = DrawIconEx(dc, 0, 0, icon, 32, 32, 0, None, DI_NORMAL).is_ok();
        let mut pixels = if drawn {
            std::slice::from_raw_parts(bits.cast::<u8>(), 32 * 32 * 4).to_vec()
        } else {
            Vec::new()
        };
        // Modern icons have alpha; for legacy mask icons render onto white too to recover coverage.
        if drawn && pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
            std::ptr::write_bytes(bits, 255, 32 * 32 * 4);
            let _ = DrawIconEx(dc, 0, 0, icon, 32, 32, 0, None, DI_NORMAL);
            let white = std::slice::from_raw_parts(bits.cast::<u8>(), 32 * 32 * 4);
            for (p, w) in pixels
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(white.as_chunks::<4>().0.iter())
            {
                p[3] = 255 - w[0].saturating_sub(p[0]);
            }
        }
        let _ = SelectObject(dc, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        let _ = DestroyIcon(icon);
        if !drawn {
            return None;
        }
        for p in pixels.as_chunks_mut::<4>().0.iter_mut() {
            p.swap(0, 2);
            let alpha = p[3];
            if alpha > 0 && alpha < 255 {
                for c in &mut p[..3] {
                    *c = (u32::from(*c) * 255 / u32::from(alpha)).min(255) as u8;
                }
            }
        }
        image::RgbaImage::from_raw(32, 32, pixels)
    }
}

pub fn chinese_ui() -> bool {
    // SAFETY: reads OS language without accessing memory supplied by the caller.
    unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() & 0x3ff == 0x04 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a Windows desktop; uses only throwaway windows and does not change the clipboard"]
    fn settings_and_owned_dialog_keep_the_original_input_focus() {
        use windows::core::w;
        struct Restore(HWND);
        impl Drop for Restore {
            fn drop(&mut self) {
                force_foreground(self.0);
            }
        }
        struct Owned(HWND);
        impl Drop for Owned {
            fn drop(&mut self) {
                // SAFETY: each fixture HWND is destroyed once on its creating thread.
                unsafe {
                    let _ = DestroyWindow(self.0);
                }
            }
        }
        let _restore = Restore(foreground());
        // SAFETY: built-in window classes and valid live parent handles; every window is
        // owned by an RAII guard and positioned off-screen to avoid touching real apps.
        unsafe {
            let owner = Owned(
                CreateWindowExW(
                    WS_EX_TOOLWINDOW,
                    w!("STATIC"),
                    w!("Copybook focus test owner"),
                    WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                    -10000,
                    -10000,
                    240,
                    160,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap(),
            );
            let dialog = Owned(
                CreateWindowExW(
                    WS_EX_TOOLWINDOW,
                    w!("STATIC"),
                    w!("Copybook focus test dialog"),
                    WS_POPUP | WS_CAPTION | WS_VISIBLE,
                    -10000,
                    -10000,
                    240,
                    160,
                    Some(owner.0),
                    None,
                    None,
                    None,
                )
                .unwrap(),
            );
            let panel = Owned(
                CreateWindowExW(
                    WS_EX_TOOLWINDOW,
                    w!("STATIC"),
                    w!("Copybook focus test panel"),
                    WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                    -10000,
                    -10000,
                    240,
                    160,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap(),
            );
            let edit = Owned(
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("EDIT"),
                    w!("fixture"),
                    WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                    0,
                    0,
                    200,
                    24,
                    Some(dialog.0),
                    None,
                    None,
                    None,
                )
                .unwrap(),
            );
            assert!(force_foreground(dialog.0));
            let _ = SetFocus(Some(edit.0));
            let saved = target(edit.0, panel.0).unwrap();
            assert_eq!(
                saved.window, dialog.0 .0 as isize,
                "Keep the owned dialog rather than its root owner"
            );
            assert_eq!(saved.focus, Some(edit.0 .0 as isize));
            assert!(target(panel.0, panel.0).is_none());
            assert!(force_foreground(panel.0));
            assert!(saved.activate());
            assert!(
                saved.focus_ready(),
                "Restore the original focused child even within this process"
            );
            let mut changed = saved;
            changed.process = changed.process.wrapping_add(1);
            assert!(
                !changed.valid(),
                "Reject a window belonging to a different process"
            );
            println!("same-process input accepted; owned-dialog root retained; panel rejected; original child focus restored; changed identity rejected");
        }
    }

    #[test]
    fn distinguishes_terminal_hosts_from_regular_image_consumers() {
        assert!(terminal_host(
            "CASCADIA_HOSTING_WINDOW_CLASS",
            "WindowsTerminal.exe",
            "PowerShell"
        ));
        assert!(terminal_host(
            "ConsoleWindowClass",
            "conhost.exe",
            "Claude Code"
        ));
        assert!(terminal_host("mintty", "mintty.exe", "bash"));
        assert!(terminal_host(
            "Chrome_WidgetWin_1",
            "Code.exe",
            "Codex — workspace"
        ));
        assert!(terminal_host(
            "Chrome_WidgetWin_1",
            "Code.exe",
            "Claude Code — workspace"
        ));
        assert!(!terminal_host(
            "Chrome_WidgetWin_1",
            "Code.exe",
            "app.ts — workspace"
        ));
        assert!(!terminal_host("Chrome_WidgetWin_1", "chrome.exe", "Claude"));
        assert!(!terminal_host("MSPaintApp", "mspaint.exe", "Untitled"));
    }
}
