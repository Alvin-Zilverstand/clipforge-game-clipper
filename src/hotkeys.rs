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

pub fn default_bindings() -> Vec<HotkeyBinding> {
    vec![
        HotkeyBinding {
            accelerator: "F8".to_string(),
            action: HotkeyAction::SaveReplay {
                length: Duration::from_secs(60),
            },
        },
        HotkeyBinding {
            accelerator: "Shift+F8".to_string(),
            action: HotkeyAction::SaveReplay {
                length: Duration::from_secs(30),
            },
        },
        HotkeyBinding {
            accelerator: "Alt+F7".to_string(),
            action: HotkeyAction::ToggleSessionRecording,
        },
        HotkeyBinding {
            accelerator: "F9".to_string(),
            action: HotkeyAction::Screenshot,
        },
    ]
}

pub fn parse_hotkey_action(accelerator: &str, bindings: &[HotkeyBinding]) -> Option<HotkeyAction> {
    bindings
        .iter()
        .find(|binding| binding.accelerator.eq_ignore_ascii_case(accelerator))
        .map(|binding| binding.action.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_save_replay_hotkey() {
        let action = parse_hotkey_action("f8", &default_bindings());
        assert_eq!(
            action,
            Some(HotkeyAction::SaveReplay {
                length: Duration::from_secs(60)
            })
        );
    }
}
