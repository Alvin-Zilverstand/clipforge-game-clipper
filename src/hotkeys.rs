use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyAction {
    SaveReplay { length: Duration },
    ToggleSessionRecording,
    Screenshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyBinding {
    pub accelerator: String,
    pub action: HotkeyAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyConfig {
    pub clip_last_60s: String,
    pub clip_last_30s: String,
    pub toggle_session_recording: String,
    pub screenshot: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            clip_last_60s: "F8".to_string(),
            clip_last_30s: "Shift+F8".to_string(),
            toggle_session_recording: "Alt+F7".to_string(),
            screenshot: "F9".to_string(),
        }
    }
}

impl HotkeyConfig {
    pub fn into_bindings(self) -> Vec<HotkeyBinding> {
        vec![
            HotkeyBinding {
                accelerator: self.clip_last_60s,
                action: HotkeyAction::SaveReplay {
                    length: Duration::from_secs(60),
                },
            },
            HotkeyBinding {
                accelerator: self.clip_last_30s,
                action: HotkeyAction::SaveReplay {
                    length: Duration::from_secs(30),
                },
            },
            HotkeyBinding {
                accelerator: self.toggle_session_recording,
                action: HotkeyAction::ToggleSessionRecording,
            },
            HotkeyBinding {
                accelerator: self.screenshot,
                action: HotkeyAction::Screenshot,
            },
        ]
    }
}

pub fn parse_hotkey_action(accelerator: &str, bindings: &[HotkeyBinding]) -> Option<HotkeyAction> {
    bindings
        .iter()
        .find(|binding| binding.accelerator.eq_ignore_ascii_case(accelerator))
        .map(|binding| binding.action.clone())
}

pub fn validate_hotkey(accelerator: &str) -> Result<(), String> {
    let parts: Vec<&str> = accelerator.split('+').collect();
    let key = parts.last().ok_or("Empty hotkey")?;
    if key.is_empty() {
        return Err("Empty hotkey".to_string());
    }
    let valid_keys = [
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M",
        "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
        "0", "1", "2", "3", "4", "5", "6", "7", "8", "9",
        "Space", "Enter", "Escape", "Tab", "Backspace", "Delete",
        "Up", "Down", "Left", "Right",
        "Home", "End", "PageUp", "PageDown",
        "Insert", "NumLock", "ScrollLock", "Pause",
    ];
    if !valid_keys.iter().any(|k| k.eq_ignore_ascii_case(key)) {
        return Err(format!("Unsupported key: {key}"));
    }
    for modifier in &parts[..parts.len().saturating_sub(1)] {
        if !["Ctrl", "Shift", "Alt", "Meta", "Super", "Command"].iter().any(|m| m.eq_ignore_ascii_case(modifier)) {
            return Err(format!("Unsupported modifier: {modifier}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_save_replay_hotkey() {
        let config = HotkeyConfig::default();
        let bindings = config.into_bindings();
        let action = parse_hotkey_action("f8", &bindings);
        assert_eq!(
            action,
            Some(HotkeyAction::SaveReplay {
                length: Duration::from_secs(60)
            })
        );
    }

    #[test]
    fn validates_supported_hotkeys() {
        assert!(validate_hotkey("F8").is_ok());
        assert!(validate_hotkey("Shift+F8").is_ok());
        assert!(validate_hotkey("Alt+F7").is_ok());
        assert!(validate_hotkey("Ctrl+Shift+A").is_ok());
        assert!(validate_hotkey("Meta+Space").is_ok());
    }

    #[test]
    fn rejects_invalid_hotkeys() {
        assert!(validate_hotkey("").is_err());
        assert!(validate_hotkey("InvalidKey").is_err());
        assert!(validate_hotkey("Ctrl+Invalid").is_err());
        assert!(validate_hotkey("+").is_err());
    }
}
