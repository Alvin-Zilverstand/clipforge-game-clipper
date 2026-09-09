use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualityPreset {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeySettings {
    pub clip_last_60s: String,
    pub clip_last_30s: String,
    pub toggle_session_recording: String,
    pub screenshot: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivacySettings {
    pub mic_enabled: bool,
    pub desktop_capture_requires_confirmation: bool,
    pub excluded_window_titles: Vec<String>,
    pub upload_requires_confirmation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoClipSettings {
    pub enabled: bool,
    pub pre_roll: Duration,
    pub post_roll: Duration,
    pub merge_window: Duration,
    pub enabled_events_by_game: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSettings {
    pub schema_version: u32,
    pub clip_root: PathBuf,
    pub buffer_root: PathBuf,
    pub storage_limit_gb: u64,
    pub replay_buffer: Duration,
    pub quality: QualityPreset,
    pub hotkeys: HotkeySettings,
    pub privacy: PrivacySettings,
    pub auto_clip: AutoClipSettings,
}

impl AppSettings {
    pub fn default_for_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let clip_root = root.join("clips");
        let buffer_root = root.join("buffer");

        Self {
            schema_version: 1,
            clip_root,
            buffer_root,
            storage_limit_gb: 50,
            replay_buffer: Duration::from_secs(60),
            quality: QualityPreset {
                name: "Performance 720p30".to_string(),
                width: 1280,
                height: 720,
                fps: 30,
                bitrate_kbps: 6_000,
            },
            hotkeys: HotkeySettings {
                clip_last_60s: "F8".to_string(),
                clip_last_30s: "Shift+F8".to_string(),
                toggle_session_recording: "Alt+F7".to_string(),
                screenshot: "F9".to_string(),
            },
            privacy: PrivacySettings {
                mic_enabled: false,
                desktop_capture_requires_confirmation: true,
                excluded_window_titles: Vec::new(),
                upload_requires_confirmation: true,
            },
            auto_clip: AutoClipSettings {
                enabled: true,
                pre_roll: Duration::from_secs(10),
                post_roll: Duration::from_secs(8),
                merge_window: Duration::from_secs(12),
                enabled_events_by_game: BTreeMap::new(),
            },
        }
    }

    pub fn clamp_replay_buffer(&mut self) {
        self.replay_buffer = self
            .replay_buffer
            .clamp(Duration::from_secs(15), Duration::from_secs(600));
    }
}
