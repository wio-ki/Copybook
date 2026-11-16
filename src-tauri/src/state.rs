use crate::{
    clipboard::{Pipeline, Writer},
    hook::Hook,
    model::{CmdError, Result},
    settings::Settings,
    storage::Storage,
    window::PanelState,
};
use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc, Condvar, Mutex, RwLock},
};
pub struct AppState {
    pub dir: PathBuf,
    pub storage: Arc<Mutex<Option<Storage>>>,
    pub settings: Arc<RwLock<Settings>>,
    pub settings_gate: Mutex<()>,
    pub paused: Arc<AtomicBool>,
    pub writer: Arc<Writer>,
    pub panel: Mutex<PanelState>,
    pub hook: Mutex<Option<Hook>>,
    pub pipeline: Mutex<Option<Pipeline>>,
    pub initialization: (Mutex<Option<Result<()>>>, Condvar),
}
impl AppState {
    pub fn initialized(&self, result: Result<()>) {
        if let Ok(mut slot) = self.initialization.0.lock() {
            *slot = Some(result);
            self.initialization.1.notify_all();
        }
    }
    pub fn wait_ready(&self) -> Result<()> {
        let slot = self
            .initialization
            .0
            .lock()
            .map_err(|_| CmdError::internal())?;
        let (slot, _) = self
            .initialization
            .1
            .wait_timeout_while(slot, std::time::Duration::from_secs(30), |s| s.is_none())
            .map_err(|_| CmdError::internal())?;
        slot.clone().unwrap_or_else(|| {
            Err(CmdError::new(
                "internal",
                "Storage initialization timed out",
            ))
        })
    }
    pub fn settings(&self) -> Result<Settings> {
        Ok(self
            .settings
            .read()
            .map_err(|_| CmdError::internal())?
            .clone())
    }
    pub fn db<T>(&self, f: impl FnOnce(&mut Storage) -> Result<T>) -> Result<T> {
        let mut guard = self.storage.lock().map_err(|_| CmdError::internal())?;
        f(guard
            .as_mut()
            .ok_or_else(|| CmdError::new("internal", "Storage is initializing"))?)
    }
}
