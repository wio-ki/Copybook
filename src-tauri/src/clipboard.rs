use crate::{
    detect, formats,
    model::*,
    platform,
    settings::Settings,
    storage::{Storage, StoredContent},
};
use std::{
    cell::RefCell,
    io::Cursor,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc::{self, SyncSender},
        Arc, Mutex, RwLock,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::*,
        System::{DataExchange::*, LibraryLoader::GetModuleHandleW, Memory::*},
        UI::{
            Shell::{DragQueryFileW, HDROP as DropHandle},
            WindowsAndMessaging::*,
        },
    },
};

pub const UNICODE: u32 = 13;
pub const DIB: u32 = 8;
pub const DIBV5: u32 = 17;
pub const HDROP: u32 = 15;
pub fn format(name: &str) -> u32 {
    let name = platform::wide(name);
    // SAFETY: valid NUL-terminated name, copied by Windows.
    unsafe { RegisterClipboardFormatW(PCWSTR(name.as_ptr())) }
}
pub struct ClipboardGuard;
impl ClipboardGuard {
    pub fn open(owner: Option<HWND>) -> Result<Self> {
        for _ in 0..15 {
            // SAFETY: Windows validates optional HWND and serializes clipboard access.
            if unsafe { OpenClipboard(owner) }.is_ok() {
                return Ok(Self);
            }
            thread::sleep(Duration::from_millis(10));
        }
        Err(CmdError::new("io", "Clipboard is busy"))
    }
}
impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        // SAFETY: guard exists only following a successful OpenClipboard on this thread.
        unsafe {
            let _ = CloseClipboard();
        }
    }
}
pub fn available(format: u32) -> bool {
    // SAFETY: querying a format does not access memory supplied by caller.
    unsafe { IsClipboardFormatAvailable(format).is_ok() }
}
pub fn bytes(format: u32, max: usize) -> Option<Vec<u8>> {
    if !available(format) {
        return None;
    }
    // SAFETY: clipboard is open at call sites; lock size comes from GlobalSize and data is copied before unlock.
    unsafe {
        for attempt in 0..3 {
            if let Ok(handle) = GetClipboardData(format) {
                let global = HGLOBAL(handle.0);
                let size = GlobalSize(global);
                if size > max {
                    return None;
                }
                if size > 0 {
                    let ptr = GlobalLock(global);
                    if !ptr.is_null() {
                        let mut bytes = Vec::new();
                        if bytes.try_reserve_exact(size).is_err() {
                            let _ = GlobalUnlock(global);
                            return None;
                        }
                        bytes.extend_from_slice(std::slice::from_raw_parts(ptr.cast::<u8>(), size));
                        let _ = GlobalUnlock(global);
                        return Some(bytes);
                    }
                }
            }
            if attempt < 2 {
                thread::sleep(Duration::from_millis(5));
            }
        }
        None
    }
}
pub fn text() -> Option<String> {
    let bytes = bytes(UNICODE, usize::MAX)?;
    let units: Vec<_> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .take_while(|u| *u != 0)
        .collect();
    Some(String::from_utf16_lossy(&units))
}
pub fn put(format: u32, bytes: &[u8]) -> Result<()> {
    // SAFETY: allocation is sized, locked, filled, and transferred to Windows only on successful SetClipboardData.
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1))?;
        let ptr = GlobalLock(memory);
        if ptr.is_null() {
            let _ = GlobalFree(Some(memory));
            return Err(CmdError::new("io", "Clipboard allocation failed"));
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast::<u8>(), bytes.len());
        let _ = GlobalUnlock(memory);
        if SetClipboardData(format, Some(HANDLE(memory.0))).is_err() {
            let _ = GlobalFree(Some(memory));
            return Err(CmdError::new("io", "Clipboard write failed"));
        }
        Ok(())
    }
}
pub fn text_bytes(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}
fn files() -> Option<Vec<String>> {
    // SAFETY: clipboard remains open; HDROP is owned by the system, each query uses a sized buffer.
    unsafe {
        let handle = GetClipboardData(HDROP).ok()?;
        let drop = DropHandle(handle.0);
        let count = DragQueryFileW(drop, u32::MAX, None);
        let mut paths = Vec::new();
        for i in 0..count {
            let len = DragQueryFileW(drop, i, None);
            if len > 32767 {
                return None;
            }
            let mut path = vec![0u16; len as usize + 1];
            DragQueryFileW(drop, i, Some(&mut path));
            paths.push(String::from_utf16_lossy(&path[..len as usize]));
        }
        (!paths.is_empty()).then_some(paths)
    }
}
pub fn hdrop_bytes(paths: &[String]) -> Vec<u8> {
    let mut bytes = vec![0u8; 20];
    bytes[..4].copy_from_slice(&20u32.to_le_bytes());
    bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
    for path in paths {
        bytes.extend(text_bytes(path));
    }
    bytes.extend([0, 0]);
    bytes
}
fn privacy() -> bool {
    if available(format("ExcludeClipboardContentFromMonitorProcessing"))
        || available(format("Clipboard Viewer Ignore"))
    {
        return true;
    }
    ["CanIncludeInClipboardHistory", "CanUploadToCloudClipboard"]
        .iter()
        .any(|n| bytes(format(n), 16).is_some_and(|b| b.get(..4) == Some(&[0, 0, 0, 0])))
}
fn read(settings: &Settings) -> Option<Capture> {
    if settings.respect_privacy_hints && privacy() {
        return None;
    }
    if settings.capture_files {
        if let Some(paths) = files() {
            let mut c = Capture::text("");
            c.kind = ClipKind::Files;
            c.paths = paths;
            return Some(c);
        }
    }
    let html = bytes(format("HTML Format"), usize::MAX).and_then(|b| formats::parse_html(&b));
    if settings.capture_text {
        if let Some(text) = text().filter(|s| !s.trim().is_empty()) {
            if settings.skip_secrets && detect::secret(&text) {
                return None;
            }
            let mut c = Capture::text(text);
            if settings.capture_rich {
                c.html = html.clone();
                c.rtf = bytes(format("Rich Text Format"), usize::MAX);
            }
            return Some(c);
        }
    }
    if settings.capture_images {
        let max = settings.max_image_mb as usize * 1024 * 1024;
        let png = bytes(format("PNG"), max);
        let pixels = png
            .and_then(|b| {
                let mut reader =
                    image::ImageReader::with_format(Cursor::new(b), image::ImageFormat::Png);
                let mut limits = image::Limits::default();
                // maxImageMB bounds the input format; separately bound decompression so
                // compressed 4K screenshots can be captured without admitting image bombs.
                limits.max_alloc = Some(512 * 1024 * 1024);
                limits.max_image_width = Some(32768);
                limits.max_image_height = Some(32768);
                reader.limits(limits);
                reader.decode().ok().map(|i| i.to_rgba8())
            })
            .or_else(|| bytes(DIBV5, max).and_then(|b| formats::decode_dib(&b).ok()))
            .or_else(|| bytes(DIB, max).and_then(|b| formats::decode_dib(&b).ok()));
        if let Some(pixels) =
            pixels.filter(|p| u64::from(p.width()) * u64::from(p.height()) <= 100_000_000)
        {
            let mut c = Capture::text("");
            c.kind = ClipKind::Image;
            c.pixels = Some(pixels);
            return Some(c);
        }
    }
    if settings.capture_text && !available(format("PNG")) && !available(DIBV5) && !available(DIB) {
        if let Some(html) = html {
            let text = formats::html_text(&html);
            if text.trim().is_empty() || (settings.skip_secrets && detect::secret(&text)) {
                return None;
            }
            let mut c = Capture::text(text);
            if settings.capture_rich {
                c.html = Some(html);
            }
            return Some(c);
        }
    }
    None
}

#[derive(Default)]
pub struct Writer {
    pub sequence: AtomicU32,
    gate: Mutex<()>,
}
impl Writer {
    pub fn write(&self, content: &StoredContent, plain: bool, owner: HWND) -> Result<()> {
        self.write_for_paste(content, plain, owner, None)
    }

    pub fn write_for_paste(
        &self,
        content: &StoredContent,
        plain: bool,
        owner: HWND,
        image_path: Option<&str>,
    ) -> Result<()> {
        // Encode before opening clipboard, so readers are blocked for as little time as possible.
        let mut formats = Vec::new();
        match content.item.kind {
            ClipKind::Text => {
                formats.push((UNICODE, text_bytes(&content.text)));
                if !plain {
                    if let Some(html) = &content.html {
                        formats.push((format("HTML Format"), formats::build_html(html)));
                    }
                    if let Some(rtf) = &content.rtf {
                        formats.push((format("Rich Text Format"), rtf.clone()));
                    }
                }
            }
            ClipKind::Files if plain => formats.push((UNICODE, text_bytes(&content.text))),
            ClipKind::Files => {
                let paths: Vec<_> = content
                    .item
                    .files
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .map(|f| f.path.clone())
                    .collect();
                formats.push((HDROP, hdrop_bytes(&paths)));
                formats.push((format("Preferred DropEffect"), 1u32.to_le_bytes().to_vec()));
            }
            ClipKind::Image => {
                let info = content.item.image.as_ref().ok_or_else(CmdError::internal)?;
                let data = std::fs::read(&info.path)?;
                let pixels = image::load_from_memory_with_format(&data, image::ImageFormat::Png)
                    .map_err(|_| CmdError::new("io", "Stored image is invalid"))?
                    .to_rgba8();
                formats.push((DIB, formats::encode_dib(&pixels)));
                formats.push((DIBV5, formats::encode_dibv5(&pixels)));
                formats.push((format("PNG"), data));
                if let Some(path) = image_path {
                    formats.push((UNICODE, text_bytes(path)));
                }
            }
        }
        let _gate = self.gate.lock().map_err(|_| CmdError::internal())?;
        let _clipboard = ClipboardGuard::open(Some(owner))?;
        // SAFETY: clipboard is open and this process supplies a live owner HWND.
        unsafe {
            EmptyClipboard()?;
        }
        let result = formats
            .into_iter()
            .try_for_each(|(format, data)| put(format, &data));
        // Closing can synthesize formats and advance the sequence. Keep ingestion gated until
        // the final number is recorded, including when only some formats were written.
        drop(_clipboard);
        self.sequence.store(sequence(), Ordering::SeqCst);
        result
    }
}
pub fn sequence() -> u32 {
    // SAFETY: reads global clipboard counter, no pointers involved.
    unsafe { GetClipboardSequenceNumber() }
}

thread_local! {static NOTIFY:RefCell<Option<SyncSender<()>>>=const{RefCell::new(None)};}
unsafe extern "system" fn wnd_proc(window: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match message {
        WM_CLIPBOARDUPDATE => {
            NOTIFY.with(|tx| {
                if let Some(tx) = tx.borrow().as_ref() {
                    let _ = tx.try_send(());
                }
            });
            LRESULT(0)
        }
        WM_CLOSE => {
            // SAFETY: this callback runs on the owning thread of window.
            unsafe {
                let _ = DestroyWindow(window);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            // SAFETY: quits only this listener's message loop.
            unsafe {
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => {
            // SAFETY: forward unchanged Win32 callback arguments.
            unsafe { DefWindowProcW(window, message, w, l) }
        }
    }
}
pub struct Listener {
    window: isize,
    thread: Option<JoinHandle<()>>,
}
impl Listener {
    pub fn start(tx: SyncSender<()>) -> Result<Self> {
        let (ready, rx) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("clipboard-listener".into())
            .spawn(move || {
                NOTIFY.with(|slot| *slot.borrow_mut() = Some(tx));
                // SAFETY: class procedure is static, window and message loop live on this thread.
                unsafe {
                    let result = (|| -> Result<HWND> {
                        let instance = GetModuleHandleW(None)?;
                        let class = w!("CopybookClipboardListener");
                        let _ = RegisterClassW(&WNDCLASSW {
                            lpfnWndProc: Some(wnd_proc),
                            hInstance: instance.into(),
                            lpszClassName: class,
                            ..Default::default()
                        });
                        let window = CreateWindowExW(
                            WINDOW_EX_STYLE(0),
                            class,
                            w!(""),
                            WINDOW_STYLE(0),
                            0,
                            0,
                            0,
                            0,
                            Some(HWND_MESSAGE),
                            None,
                            Some(instance.into()),
                            None,
                        )?;
                        if let Err(e) = AddClipboardFormatListener(window) {
                            let _ = DestroyWindow(window);
                            return Err(e.into());
                        }
                        Ok(window)
                    })();
                    match result {
                        Ok(window) => {
                            let _ = ready.send(Ok(window.0 as isize));
                            let mut msg = MSG::default();
                            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                                let _ = TranslateMessage(&msg);
                                DispatchMessageW(&msg);
                            }
                            let _ = RemoveClipboardFormatListener(window);
                        }
                        Err(e) => {
                            let _ = ready.send(Err(e));
                        }
                    }
                }
                NOTIFY.with(|slot| *slot.borrow_mut() = None);
            })?;
        let window = rx.recv().map_err(|_| CmdError::internal())??;
        Ok(Self {
            window,
            thread: Some(thread),
        })
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        // SAFETY: a stale HWND fails harmlessly; thread owns the live window.
        unsafe {
            let _ = PostMessageW(
                Some(platform::hwnd(self.window)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            );
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
pub type ChangeCallback = Arc<dyn Fn(&'static str, Option<i64>) + Send + Sync>;
pub struct Pipeline {
    listener: Option<Listener>,
    thread: Option<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    wake: SyncSender<()>,
}
impl Pipeline {
    pub fn start(
        dir: PathBuf,
        storage: Arc<Mutex<Option<Storage>>>,
        settings: Arc<RwLock<Settings>>,
        paused: Arc<AtomicBool>,
        writer: Arc<Writer>,
        changed: ChangeCallback,
    ) -> Result<Self> {
        let (tx, rx) = mpsc::sync_channel(1);
        let listener = Listener::start(tx.clone())?;
        let clipboard_window = listener.window;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = thread::Builder::new()
            .name("clipboard-ingest".into())
            .spawn(move || {
                let mut cleanup = Instant::now();
                let mut last = 0;
                while !stopping.load(Ordering::Relaxed) {
                    match rx.recv_timeout(Duration::from_secs(60)) {
                        Ok(()) => {
                            while rx.recv_timeout(Duration::from_millis(100)).is_ok() {
                                if stopping.load(Ordering::Relaxed) {
                                    return;
                                }
                            }
                            let result = (|| -> Result<()> {
                                if stopping.load(Ordering::Relaxed)
                                    || paused.load(Ordering::Relaxed)
                                {
                                    return Ok(());
                                }
                                let settings =
                                    settings.read().map_err(|_| CmdError::internal())?.clone();
                                let gate = writer.gate.lock().map_err(|_| CmdError::internal())?;
                                // A real HWND also excludes other same-process clipboard
                                // users (for example WebView2); NULL permits overlapping opens.
                                let guard =
                                    ClipboardGuard::open(Some(platform::hwnd(clipboard_window)))?;
                                let seq = sequence();
                                if seq == last || seq == writer.sequence.load(Ordering::SeqCst) {
                                    return Ok(());
                                }
                                last = seq;
                                // SAFETY: clipboard is open; owner/foreground may be null, which pid handles.
                                let owner = unsafe {
                                    GetClipboardOwner().unwrap_or_else(|_| platform::foreground())
                                };
                                let pid = platform::pid(owner);
                                if pid == std::process::id() {
                                    return Ok(());
                                }
                                let capture = read(&settings);
                                drop(guard);
                                drop(gate);
                                let Some(mut capture) = capture else {
                                    return Ok(());
                                };
                                capture.source = platform::source(pid, &dir);
                                // A screenshot taken while the panel is up only ever captures the
                                // panel itself, so it is never worth keeping. Such tools often
                                // write the image from a helper process that leaves no owner, so
                                // the rule only fires for an unknown source at full-screen size.
                                if capture.kind == ClipKind::Image
                                    && capture.source.is_none()
                                    && crate::window::panel_visible()
                                    && capture.pixels.as_ref().is_some_and(platform::full_screen)
                                {
                                    return Ok(());
                                }
                                if capture.source.as_ref().is_some_and(|s| {
                                    settings
                                        .ignored_apps
                                        .iter()
                                        .any(|e| e.eq_ignore_ascii_case(&s.exe))
                                }) {
                                    return Ok(());
                                }
                                let (id, added) = storage
                                    .lock()
                                    .map_err(|_| CmdError::internal())?
                                    .as_mut()
                                    .ok_or_else(CmdError::internal)?
                                    .ingest(capture, &settings)?;
                                changed(if added { "added" } else { "bumped" }, Some(id));
                                Ok(())
                            })();
                            if let Err(e) = result {
                                log::warn!("Clipboard ingest failed: {}", e.code);
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    if cleanup.elapsed() >= Duration::from_secs(3600) {
                        if let (Ok(settings), Ok(mut storage)) = (settings.read(), storage.lock()) {
                            if let Some(storage) = storage.as_mut() {
                                match storage.cleanup(&settings) {
                                    Ok(n) if n > 0 => changed("cleanup", None),
                                    Err(e) => log::warn!("Cleanup failed: {}", e.code),
                                    _ => {}
                                }
                            }
                        }
                        cleanup = Instant::now();
                    }
                }
            })?;
        Ok(Self {
            listener: Some(listener),
            thread: Some(thread),
            stop,
            wake: tx,
        })
    }
}
impl Drop for Pipeline {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.wake.try_send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.listener.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    fn owner() -> HWND {
        // SAFETY: standard STATIC class has no application callbacks, owned by this test thread.
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("Copybook clipboard test"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
            .unwrap()
        }
    }
    fn replace(owner: HWND, values: Vec<(u32, Vec<u8>)>) {
        let _guard = ClipboardGuard::open(Some(owner)).unwrap();
        // SAFETY: clipboard is open with a real owner, owned data is transferred by put.
        unsafe {
            EmptyClipboard().unwrap();
        }
        for (format, bytes) in values {
            put(format, &bytes).unwrap();
        }
    }
    struct Restore {
        text: Option<String>,
        window: HWND,
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            if let Ok(_guard) = ClipboardGuard::open(Some(self.window)) {
                // SAFETY: clipboard is open on this thread; restores saved text even during test unwinding.
                unsafe {
                    let _ = EmptyClipboard();
                }
                if let Some(text) = &self.text {
                    let _ = put(UNICODE, &text_bytes(text));
                }
            }
            // SAFETY: test owns the standard-class window on this thread.
            unsafe {
                let _ = DestroyWindow(self.window);
            }
        }
    }
    fn external(kind: &str, dir: &std::path::Path) {
        use std::os::windows::process::CommandExt;
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "clipboard::tests::fixture_writer",
                "--nocapture",
            ])
            .env("COPYBOOK_TEST_FIXTURE", kind)
            .env("COPYBOOK_TEST_DIR", dir)
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "fixture writer failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    #[test]
    #[ignore = "helper for real clipboard pipeline; only writes when explicitly given fixture env"]
    fn fixture_writer() {
        let Ok(kind) = std::env::var("COPYBOOK_TEST_FIXTURE") else {
            return;
        };
        let window = owner();
        let values = match kind.as_str() {
            "text" => vec![(
                UNICODE,
                text_bytes("https://example.org/copybook-integration"),
            )],
            "html" => vec![
                (UNICODE, text_bytes("copybook rich 中文")),
                (
                    format("HTML Format"),
                    formats::build_html("<b>copybook rich 中文</b>"),
                ),
                (
                    format("Rich Text Format"),
                    b"{\\rtf1 copybook rich}\0".to_vec(),
                ),
            ],
            "html-only" => vec![(
                format("HTML Format"),
                formats::build_html("<b>copybook html only</b>"),
            )],
            "image" => {
                let pixels = image::RgbaImage::from_pixel(3, 2, image::Rgba([100, 20, 40, 255]));
                let mut png = Cursor::new(Vec::new());
                pixels.write_to(&mut png, image::ImageFormat::Png).unwrap();
                vec![
                    (format("PNG"), png.into_inner()),
                    (DIB, formats::encode_dib(&pixels)),
                ]
            }
            "files" => {
                let dir = PathBuf::from(std::env::var_os("COPYBOOK_TEST_DIR").unwrap());
                vec![
                    (
                        HDROP,
                        hdrop_bytes(&[dir.join("fixture.txt").to_string_lossy().into_owned()]),
                    ),
                    (format("Preferred DropEffect"), 1u32.to_le_bytes().to_vec()),
                ]
            }
            hint => {
                let name = match hint {
                    "exclude" => "ExcludeClipboardContentFromMonitorProcessing",
                    "ignore" => "Clipboard Viewer Ignore",
                    "history" => "CanIncludeInClipboardHistory",
                    _ => "CanUploadToCloudClipboard",
                };
                vec![
                    (UNICODE, text_bytes(&format!("copybook privacy {hint}"))),
                    (format(name), 0u32.to_le_bytes().to_vec()),
                ]
            }
        };
        replace(window, values);
        thread::sleep(Duration::from_millis(700));
        // SAFETY: the fixture owns this message-only window.
        unsafe {
            DestroyWindow(window).unwrap();
        }
    }
    #[test]
    #[ignore = "requires a Windows desktop clipboard; saves and restores clipboard text"]
    fn real_listener_ingest_pipeline() {
        let window = owner();
        let original = {
            let _guard = ClipboardGuard::open(None).unwrap();
            text()
        };
        let _restore = Restore {
            text: original,
            window,
        };
        let dir = tempfile::tempdir_in(crate::test_dir()).unwrap();
        std::fs::write(dir.path().join("fixture.txt"), b"fixture").unwrap();
        let db = Arc::new(Mutex::new(Some(Storage::open(dir.path()).unwrap())));
        let settings = Arc::new(RwLock::new(Settings::default()));
        let paused = Arc::new(AtomicBool::new(false));
        let writer = Arc::new(Writer::default());
        let (tx, events) = mpsc::channel();
        let changed = Arc::new(move |reason, id| {
            let _ = tx.send((reason, id));
        });
        let pipeline = Pipeline::start(
            dir.path().to_owned(),
            db.clone(),
            settings,
            paused.clone(),
            writer.clone(),
            changed,
        )
        .unwrap();
        let list = || {
            db.lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .list(ListQuery {
                    query: String::new(),
                    filter: ListFilter::All,
                    offset: 0,
                    limit: 50,
                    ..ListQuery::default()
                })
                .unwrap()
        };
        for (kind, expected, sub) in [
            ("text", ClipKind::Text, Some("url")),
            ("html", ClipKind::Text, None),
            ("image", ClipKind::Image, None),
            ("files", ClipKind::Files, None),
            ("html-only", ClipKind::Text, None),
        ] {
            external(kind, dir.path());
            events.recv_timeout(Duration::from_secs(5)).unwrap();
            let item = list().items[0].clone();
            assert_eq!(item.kind, expected);
            assert_eq!(item.sub_kind.as_deref(), sub);
            assert!(item
                .source
                .as_ref()
                .is_some_and(|s| !s.exe.is_empty() && !s.name.is_empty()));
            if kind == "html" {
                assert!(item.has_html && item.has_rtf);
                let content = db
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .content(item.id)
                    .unwrap();
                assert_eq!(content.html.as_deref(), Some("<b>copybook rich 中文</b>"));
            }
            if kind == "image" {
                let info = item.image.unwrap();
                assert_eq!((info.width, info.height), (3, 2));
                assert!(std::path::Path::new(&info.path).exists());
            }
            if kind == "files" {
                assert!(item.files.unwrap()[0].exists);
            }
        }
        assert_eq!(list().total, 5);
        for item in list().items {
            let content = db
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .content(item.id)
                .unwrap();
            for plain in [false, true] {
                writer.write(&content, plain, window).unwrap();
                assert_eq!(writer.sequence.load(Ordering::SeqCst), sequence());
                // The production reader and writer share this gate. Test readback must
                // participate too: OpenClipboard(NULL) can succeed concurrently within
                // one process, then one guard closes the other thread's clipboard access.
                let _gate = writer.gate.lock().unwrap();
                let guard = ClipboardGuard::open(Some(window)).unwrap();
                let captured = read(&Settings::default()).unwrap_or_else(|| {
                    let unicode=bytes(UNICODE,usize::MAX);
                    let private=privacy();
                    // SAFETY: clipboard metadata only, while the test guard is open.
                    let status=unsafe {GetClipboardData(UNICODE).map(|h|{let size=GlobalSize(HGLOBAL(h.0));(size,GetLastError().0)}).map_err(|e|e.code().0)};
                    panic!("writer readback missing: kind={:?}, plain={plain}, seq={}/{}, privacy={private}, unicode_bytes={:?}, unicode_status={status:?}, png_available={}, dib_available={}, png_bytes={:?}, dib_bytes={:?}",item.kind,writer.sequence.load(Ordering::SeqCst),sequence(),unicode.as_ref().map(Vec::len),available(format("PNG")),available(DIB),bytes(format("PNG"),20*1024*1024).as_ref().map(Vec::len),bytes(DIB,20*1024*1024).as_ref().map(Vec::len));
                });
                if item.kind == ClipKind::Files && plain {
                    assert_eq!(captured.kind, ClipKind::Text);
                    assert_eq!(captured.text, content.text);
                } else {
                    assert_eq!(captured.kind, item.kind);
                }
                if item.kind == ClipKind::Text {
                    assert_eq!(captured.text, content.text);
                    assert_eq!(
                        captured.html,
                        if plain { None } else { content.html.clone() }
                    );
                }
                if item.kind == ClipKind::Image {
                    assert!(available(DIB) && available(DIBV5) && available(format("PNG")));
                    assert!(
                        text().is_none(),
                        "non-terminal image copies must not turn into paths"
                    );
                }
                if item.kind == ClipKind::Files && !plain {
                    assert_eq!(
                        bytes(format("Preferred DropEffect"), 16).unwrap()[..4],
                        [1, 0, 0, 0]
                    );
                }
                drop(guard);
            }
            if item.kind == ClipKind::Image {
                let path = format!("\"{}\"", item.image.as_ref().unwrap().path);
                writer
                    .write_for_paste(&content, false, window, Some(&path))
                    .unwrap();
                let _guard = ClipboardGuard::open(Some(window)).unwrap();
                assert_eq!(text().as_deref(), Some(path.as_str()));
                assert!(available(DIB) && available(DIBV5) && available(format("PNG")));
                let png = bytes(format("PNG"), 20 * 1024 * 1024).unwrap();
                let pixels = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
                    .unwrap()
                    .to_rgba8();
                assert_eq!(
                    formats::decode_dib(&bytes(DIBV5, 20 * 1024 * 1024).unwrap()).unwrap(),
                    pixels
                );
                assert_eq!(writer.sequence.load(Ordering::SeqCst), sequence());
            }
        }
        thread::sleep(Duration::from_millis(350));
        assert!(events.try_recv().is_err());
        assert_eq!(list().total, 5);
        println!("writer round trips: rich/plain text, PNG+CF_DIB+CF_DIBV5, quoted terminal PNG path, CF_HDROP+copy effect, plain file paths");
        println!(
            "captured text/url, HTML+RTF, PNG+DIB image, files, HTML-only; all sources populated"
        );
        let id = list()
            .items
            .iter()
            .find(|i| i.sub_kind.as_deref() == Some("url"))
            .unwrap()
            .id;
        let content = db.lock().unwrap().as_ref().unwrap().content(id).unwrap();
        writer.write(&content, false, window).unwrap();
        assert_eq!(writer.sequence.load(Ordering::SeqCst), sequence());
        thread::sleep(Duration::from_millis(350));
        assert!(events.try_recv().is_err());
        assert_eq!(list().total, 5);
        external("text", dir.path());
        events.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            db.lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .get(id)
                .unwrap()
                .use_count,
            2
        );
        for hint in ["exclude", "ignore", "history", "cloud"] {
            external(hint, dir.path());
            assert!(events.try_recv().is_err());
            assert_eq!(list().total, 5);
        }
        paused.store(true, Ordering::Relaxed);
        external("text", dir.path());
        assert!(events.try_recv().is_err());
        assert_eq!(
            db.lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .get(id)
                .unwrap()
                .use_count,
            2
        );
        println!("own writes, four privacy hints, and paused capture skipped; clipboard text restored on scope exit");
        drop(pipeline);
    }
}
