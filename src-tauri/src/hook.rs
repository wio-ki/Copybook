use crate::{
    model::{CmdError, Result},
    platform,
};
use std::{
    cell::RefCell,
    sync::{mpsc, Arc},
    thread::{self, JoinHandle},
};
use windows::Win32::{
    Foundation::*,
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};
type Toggle = Arc<dyn Fn() + Send + Sync>;
thread_local! {
    static TOGGLE:RefCell<Option<Toggle>>=const{RefCell::new(None)};
    static HELD:RefCell<bool>=const{RefCell::new(false)};
}
unsafe extern "system" fn keyboard(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: WH_KEYBOARD_LL supplies a live KBDLLHOOKSTRUCT for HC_ACTION.
        let event = unsafe { &*(l.0 as *const KBDLLHOOKSTRUCT) };
        if event.flags.contains(LLKHF_INJECTED) {
            // SAFETY: forward the unmodified callback arguments; null hook is permitted.
            return unsafe { CallNextHookEx(None, code, w, l) };
        }
        if event.vkCode == u32::from(VK_V.0) {
            let up = event.flags.contains(LLKHF_UP);
            // SAFETY: valid key constants, read-only keyboard state query.
            let win = unsafe {
                GetAsyncKeyState(i32::from(VK_LWIN.0)) < 0
                    || GetAsyncKeyState(i32::from(VK_RWIN.0)) < 0
            };
            let was = HELD.with(|held| *held.borrow());
            if win || was {
                if !up && !was {
                    HELD.with(|held| *held.borrow_mut() = true);
                    // Balanced unused key prevents the bare Win key from opening Start on release.
                    // SAFETY: correctly sized INPUTs contain a balanced F24 pair, no modifiers.
                    unsafe {
                        let _ = SendInput(
                            &[platform::key(VK_F24, false), platform::key(VK_F24, true)],
                            std::mem::size_of::<INPUT>() as i32,
                        );
                    }
                    TOGGLE.with(|toggle| {
                        if let Some(toggle) = toggle.borrow().as_ref() {
                            toggle();
                        }
                    });
                }
                if up {
                    HELD.with(|held| *held.borrow_mut() = false);
                }
                return LRESULT(1);
            }
        }
    }
    // SAFETY: forward callback arguments when the event was not consumed.
    unsafe { CallNextHookEx(None, code, w, l) }
}
pub struct Hook {
    id: u32,
    thread: Option<JoinHandle<()>>,
}
impl Hook {
    pub fn start(toggle: Toggle) -> Result<Self> {
        let (tx, rx) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("win-v-hook".into())
            .spawn(move || {
                TOGGLE.with(|t| *t.borrow_mut() = Some(toggle));
                // SAFETY: hook procedure is static and the hook is uninstalled on its owning thread.
                unsafe {
                    let mut message = MSG::default();
                    let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
                    let result = GetModuleHandleW(None).and_then(|module| {
                        SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard), Some(module.into()), 0)
                    });
                    match result {
                        Ok(hook) => {
                            let _ = tx.send(Ok(GetCurrentThreadId()));
                            while GetMessageW(&mut message, None, 0, 0).0 > 0 {
                                let _ = TranslateMessage(&message);
                                DispatchMessageW(&message);
                            }
                            let _ = UnhookWindowsHookEx(hook);
                        }
                        Err(_) => {
                            let _ = tx.send(Err(CmdError::new(
                                "internal",
                                "Could not install Win+V hook",
                            )));
                        }
                    }
                }
                TOGGLE.with(|t| *t.borrow_mut() = None);
            })?;
        let id = rx.recv().map_err(|_| CmdError::internal())??;
        Ok(Self {
            id,
            thread: Some(thread),
        })
    }
}
impl Drop for Hook {
    fn drop(&mut self) {
        // SAFETY: thread id belongs to the hook's message loop; WM_QUIT requests orderly cleanup.
        unsafe {
            let _ = PostThreadMessageW(self.id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
