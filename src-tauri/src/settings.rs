use crate::model::{CmdError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fs, path::Path};
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub language: String,
    pub theme: String,
    pub density: String,
    pub autostart: bool,
    pub run_as_admin: bool,
    pub hotkey: String,
    pub win_v: bool,
    pub panel_position: String,
    pub hide_on_blur: bool,
    pub auto_paste: bool,
    pub paste_plain: bool,
    pub move_to_top: bool,
    pub capture_text: bool,
    pub capture_rich: bool,
    pub capture_images: bool,
    pub capture_files: bool,
    pub history_limit: u32,
    pub retention_days: u32,
    #[serde(rename = "maxImageMB")]
    pub max_image_mb: u32,
    pub ignored_apps: Vec<String>,
    pub respect_privacy_hints: bool,
    pub skip_secrets: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "system".into(),
            theme: "system".into(),
            density: "comfortable".into(),
            autostart: false,
            run_as_admin: false,
            hotkey: "Alt+KeyV".into(),
            win_v: false,
            panel_position: "cursor".into(),
            hide_on_blur: true,
            auto_paste: true,
            paste_plain: false,
            move_to_top: true,
            capture_text: true,
            capture_rich: true,
            capture_images: true,
            capture_files: true,
            history_limit: 1000,
            retention_days: 0,
            max_image_mb: 20,
            ignored_apps: [
                "KeePass.exe",
                "KeePassXC.exe",
                "1Password.exe",
                "Bitwarden.exe",
            ]
            .map(String::from)
            .to_vec(),
            respect_privacy_hints: true,
            skip_secrets: true,
        }
    }
}

// Accept modifier order permutations, then persist the canonical Ctrl,Alt,Shift,Super order.
pub fn hotkey(value: &str) -> Result<(String, Option<Shortcut>)> {
    if value.is_empty() {
        return Ok((String::new(), None));
    }
    let parts: Vec<_> = value.split('+').collect();
    let key = parts.last().copied().unwrap_or("");
    let code: Code = key
        .parse()
        .map_err(|_| CmdError::new("hotkey_invalid", "Unknown keyboard code"))?;
    // Code's parser also supports aliases; the contract accepts KeyboardEvent.code only.
    if code.to_string() != key {
        return Err(CmdError::new(
            "hotkey_invalid",
            "Use a KeyboardEvent.code key",
        ));
    }
    let mut mods = Modifiers::empty();
    for part in &parts[..parts.len() - 1] {
        let flag = match *part {
            "Ctrl" => Modifiers::CONTROL,
            "Alt" => Modifiers::ALT,
            "Shift" => Modifiers::SHIFT,
            "Super" => Modifiers::SUPER,
            _ => return Err(CmdError::new("hotkey_invalid", "Unknown modifier")),
        };
        if mods.contains(flag) {
            return Err(CmdError::new("hotkey_invalid", "Duplicate modifier"));
        }
        mods |= flag;
    }
    let function_key = key
        .strip_prefix('F')
        .and_then(|s| s.parse::<u8>().ok())
        .is_some_and(|n| (1..=24).contains(&n));
    if mods.is_empty() && !function_key {
        return Err(CmdError::new("hotkey_invalid", "A modifier is required"));
    }
    if key.starts_with("Control")
        || key.starts_with("Shift")
        || key.starts_with("Alt")
        || key.starts_with("Meta")
        || key == "Unidentified"
    {
        return Err(CmdError::new(
            "hotkey_invalid",
            "A non-modifier key is required",
        ));
    }
    let mut canonical = String::new();
    for (flag, name) in [
        (Modifiers::CONTROL, "Ctrl"),
        (Modifiers::ALT, "Alt"),
        (Modifiers::SHIFT, "Shift"),
        (Modifiers::SUPER, "Super"),
    ] {
        if mods.contains(flag) {
            canonical.push_str(name);
            canonical.push('+');
        }
    }
    canonical.push_str(key);
    Ok((canonical, Some(Shortcut::new(Some(mods), code))))
}

fn valid_key(key: &str, value: &Value) -> bool {
    match key {
        "language" => value
            .as_str()
            .is_some_and(|s| ["system", "zh-CN", "en-US"].contains(&s)),
        "theme" => value
            .as_str()
            .is_some_and(|s| ["system", "light", "dark"].contains(&s)),
        "density" => value
            .as_str()
            .is_some_and(|s| ["comfortable", "compact"].contains(&s)),
        "panelPosition" => value
            .as_str()
            .is_some_and(|s| ["cursor", "center", "remember"].contains(&s)),
        "hotkey" => value.as_str().is_some_and(|s| hotkey(s).is_ok()),
        "historyLimit" => value
            .as_u64()
            .is_some_and(|n| n == 0 || (50..=100000).contains(&n)),
        "retentionDays" => value.as_u64().is_some_and(|n| n <= 3650),
        "maxImageMB" => value.as_u64().is_some_and(|n| (1..=200).contains(&n)),
        "ignoredApps" => value.as_array().is_some_and(|a| {
            a.iter().all(|v| {
                v.as_str().is_some_and(|s| {
                    !s.is_empty()
                        && !s.contains(['/', '\\', '\0'])
                        && s.to_ascii_lowercase().ends_with(".exe")
                })
            })
        }),
        "autostart"
        | "runAsAdmin"
        | "winV"
        | "hideOnBlur"
        | "autoPaste"
        | "pastePlain"
        | "moveToTop"
        | "captureText"
        | "captureRich"
        | "captureImages"
        | "captureFiles"
        | "respectPrivacyHints"
        | "skipSecrets" => value.is_boolean(),
        _ => false,
    }
}
impl Settings {
    pub fn patch(&self, patch: Value) -> Result<Self> {
        let patch = patch
            .as_object()
            .ok_or_else(|| CmdError::invalid("Expected settings object"))?;
        let mut value = serde_json::to_value(self).map_err(|_| CmdError::internal())?;
        for (key, v) in patch {
            if !valid_key(key, v) {
                return Err(CmdError::new(
                    if key == "hotkey" {
                        "hotkey_invalid"
                    } else {
                        "invalid"
                    },
                    "Invalid settings value",
                ));
            }
            value[key] = if key == "hotkey" {
                Value::String(hotkey(v.as_str().unwrap_or_default())?.0)
            } else {
                v.clone()
            };
        }
        serde_json::from_value(value).map_err(|_| CmdError::internal())
    }
    pub fn tolerant(value: Value) -> Self {
        let mut settings = Self::default();
        if let Some(map) = value.as_object() {
            for (key, value) in map {
                if let Ok(next) = settings.patch(serde_json::json!({key: value})) {
                    settings = next;
                }
            }
        }
        settings
    }
    pub fn load(dir: &Path) -> Result<(Self, bool)> {
        fs::create_dir_all(dir)?;
        let path = dir.join("settings.json");
        let first = !path.exists();
        let settings = match fs::read(&path) {
            Ok(bytes) => Self::tolerant(serde_json::from_slice(&bytes).unwrap_or(Value::Null)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(e.into()),
        };
        settings.save(dir)?;
        Ok((settings, first))
    }
    pub fn save(&self, dir: &Path) -> Result<()> {
        atomic_json(&dir.join("settings.json"), self)
    }
}

pub fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    use std::io::Write;
    use windows::{
        core::PCWSTR,
        Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        },
    };
    let temp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(value).map_err(|_| CmdError::internal())?;
    let mut file = fs::File::create(&temp)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    let from: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: both paths are NUL-terminated UTF-16 buffers alive through the call.
    unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )?;
    }
    Ok(())
}
use std::os::windows::ffi::OsStrExt;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tolerant_defaults_and_atomic_write() {
        let dir = tempfile::tempdir_in(crate::test_dir()).unwrap();
        let (s, first) = Settings::load(dir.path()).unwrap();
        assert!(first);
        assert_eq!(s, Settings::default());
        let s = Settings::tolerant(
            serde_json::json!({"unknown":42,"maxImageMB":0,"autoPaste":"no","theme":"dark"}),
        );
        assert_eq!(s.theme, "dark");
        assert_eq!(s.max_image_mb, 20);
        assert!(s.auto_paste);
        s.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path()).unwrap(), (s, false));
    }
    #[test]
    fn ranges_and_hotkeys() {
        let s = Settings::default();
        for key in ["historyLimit", "retentionDays", "maxImageMB"] {
            let (good, bad) = match key {
                "historyLimit" => (vec![0, 50, 100000], vec![-1, 1, 49, 100001]),
                "retentionDays" => (vec![0, 1, 3650], vec![-1, 3651]),
                _ => (vec![1, 200], vec![-1, 0, 201]),
            };
            for n in good {
                assert!(s.patch(serde_json::json!({key:n})).is_ok());
            }
            for n in bad {
                assert!(s.patch(serde_json::json!({key:n})).is_err());
            }
            assert!(s.patch(serde_json::json!({key:1.5})).is_err());
        }
        for key in [
            "Alt+KeyV",
            "Ctrl+Shift+Digit1",
            "F9",
            "Super+Alt+Backquote",
            "",
        ] {
            assert!(hotkey(key).is_ok(), "{key}");
        }
        for key in ["KeyV", "Alt+", "Alt+Foo", "Alt+Alt+KeyV", "Alt+ShiftLeft"] {
            assert!(hotkey(key).is_err(), "{key}");
        }
        assert_eq!(
            hotkey("Super+Alt+Backquote").unwrap().0,
            "Alt+Super+Backquote"
        );
    }

    #[test]
    fn administrator_preference_survives_restart_and_old_settings() {
        let dir = tempfile::tempdir_in(crate::test_dir()).unwrap();
        assert!(!Settings::tolerant(serde_json::json!({"autostart": true})).run_as_admin);
        let enabled = Settings::default()
            .patch(serde_json::json!({"runAsAdmin": true}))
            .unwrap();
        enabled.save(dir.path()).unwrap();
        assert!(Settings::load(dir.path()).unwrap().0.run_as_admin);
        let disabled = enabled
            .patch(serde_json::json!({"runAsAdmin": false}))
            .unwrap();
        disabled.save(dir.path()).unwrap();
        assert!(!Settings::load(dir.path()).unwrap().0.run_as_admin);
        assert!(disabled
            .patch(serde_json::json!({"runAsAdmin": "true"}))
            .is_err());
    }
}
