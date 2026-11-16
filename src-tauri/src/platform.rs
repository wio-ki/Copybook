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
pub fn target(window: HWND) -> Option<isize> {
    // SAFETY: all queried handles are checked with IsWindow; APIs accept null/stale handles.
    unsafe {
        if !IsWindow(Some(window)).as_bool() {
            return None;
        }
        let root = GetAncestor(window, GA_ROOTOWNER);
        if root.0.is_null() || pid(root) == GetCurrentProcessId() {
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
        Some(root.0 as isize)
    }
}
pub fn foreground() -> HWND {
    // SAFETY: no arguments or owned resources.
    unsafe { GetForegroundWindow() }
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
pub fn paste_to(window: HWND) -> Result<()> {
    force_foreground(window);
    let start = Instant::now();
    while foreground() != window && start.elapsed() < Duration::from_millis(300) {
        std::thread::sleep(Duration::from_millis(10));
    }
    if foreground() != window {
        return Err(CmdError::new(
            "internal",
            "Paste target did not receive focus",
        ));
    }
    let start = Instant::now();
    loop {
        // SAFETY: virtual key constants are valid; reading key state has no side effects.
        let down = unsafe {
            [VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN]
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
    // Let the target restore the control that had focus before the panel (the paste recipient)
    // before injecting; a few runs otherwise sent Ctrl+V during that restore and lost it.
    std::thread::sleep(Duration::from_millis(80));
    if foreground() != window {
        return Err(CmdError::new("internal", "Paste target lost focus"));
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
