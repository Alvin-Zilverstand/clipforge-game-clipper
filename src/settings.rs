use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub const CURRENT_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualityPreset {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotkeySettings {
    pub clip_last_60s: String,
    pub clip_last_30s: String,
    pub toggle_session_recording: String,
    pub screenshot: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivacySettings {
    #[serde(default = "default_true")]
    pub system_audio_enabled: bool,
    pub mic_enabled: bool,
    #[serde(default)]
    pub mic_device: Option<String>,
    #[serde(default)]
    pub system_audio_device: Option<String>,
    pub desktop_capture_requires_confirmation: bool,
    pub excluded_window_titles: Vec<String>,
    pub upload_requires_confirmation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoClipSettings {
    pub enabled: bool,
    pub pre_roll: Duration,
    pub post_roll: Duration,
    pub merge_window: Duration,
    pub enabled_events_by_game: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadSettings {
    pub auto_upload_enabled: bool,
    pub provider: String,
    pub catbox_userhash: Option<String>,
    pub litterbox_expiry_hours: u8,
    pub custom_endpoint: Option<String>,
    pub custom_response_url_path: String,
    pub custom_headers: Vec<(String, String)>,
}

impl Default for UploadSettings {
    fn default() -> Self {
        Self {
            auto_upload_enabled: false,
            provider: "catbox".to_string(),
            catbox_userhash: None,
            litterbox_expiry_hours: 24,
            custom_endpoint: None,
            custom_response_url_path: "url".to_string(),
            custom_headers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub clip_root: PathBuf,
    pub buffer_root: PathBuf,
    pub storage_limit_gb: u64,
    pub replay_buffer: Duration,
    pub quality: QualityPreset,
    pub hotkeys: HotkeySettings,
    pub privacy: PrivacySettings,
    pub auto_clip: AutoClipSettings,
    #[serde(default)]
    pub upload: UploadSettings,
}

impl AppSettings {
    pub fn default_for_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let clip_root = root.join("clips");
        let buffer_root = root.join("buffer");

        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
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
                system_audio_enabled: true,
                mic_enabled: false,
                mic_device: None,
                system_audio_device: None,
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
            upload: UploadSettings::default(),
        }
    }

    pub fn clamp_replay_buffer(&mut self) {
        self.replay_buffer = self
            .replay_buffer
            .clamp(Duration::from_secs(15), Duration::from_secs(600));
    }

    pub fn migrate(&mut self) {
        self.clamp_replay_buffer();
        self.upload.litterbox_expiry_hours = match self.upload.litterbox_expiry_hours {
            1 | 12 | 24 | 72 => self.upload.litterbox_expiry_hours,
            _ => 24,
        };
        if self.upload.provider.trim().is_empty() {
            self.upload.provider = "catbox".to_string();
        }
        if self.upload.custom_response_url_path.trim().is_empty() {
            self.upload.custom_response_url_path = "url".to_string();
        }
        self.schema_version = CURRENT_SCHEMA_VERSION;
    }
}

pub fn load_or_create_settings(root: impl Into<PathBuf>) -> io::Result<AppSettings> {
    let root = root.into();
    let path = settings_path(&root);
    if path.exists() {
        let mut settings: AppSettings = serde_json::from_str(&fs::read_to_string(&path)?)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let previous_version = settings.schema_version;
        settings.migrate();
        if previous_version != settings.schema_version {
            save_settings(&root, &settings)?;
        }
        Ok(settings)
    } else {
        let settings = AppSettings::default_for_root(&root);
        save_settings(&root, &settings)?;
        Ok(settings)
    }
}

pub fn save_settings(root: impl Into<PathBuf>, settings: &AppSettings) -> io::Result<()> {
    let root = root.into();
    fs::create_dir_all(&root)?;
    let contents = serde_json::to_string_pretty(settings)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    fs::write(settings_path(&root), contents)
}

pub fn settings_path(root: impl Into<PathBuf>) -> PathBuf {
    root.into().join("settings.json")
}

fn default_true() -> bool {
    true
}

fn default_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_as_versioned_json() {
        let root =
            std::env::temp_dir().join(format!("clipforge-settings-{}", SystemTimeCompat::millis()));
        let mut settings = AppSettings::default_for_root(&root);
        settings.replay_buffer = Duration::from_secs(120);

        save_settings(&root, &settings).expect("save");
        let loaded = load_or_create_settings(&root).expect("load");

        assert_eq!(loaded.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(loaded.replay_buffer, Duration::from_secs(120));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_settings_migrate_upload_defaults() {
        let root = std::env::temp_dir().join(format!(
            "clipforge-settings-migrate-{}",
            SystemTimeCompat::millis()
        ));
        fs::create_dir_all(&root).expect("settings root");
        let mut value =
            serde_json::to_value(AppSettings::default_for_root(&root)).expect("settings json");
        value["schema_version"] = serde_json::json!(1);
        value
            .as_object_mut()
            .expect("settings object")
            .remove("upload");
        fs::write(
            settings_path(&root),
            serde_json::to_string_pretty(&value).expect("legacy settings"),
        )
        .expect("write legacy settings");

        let loaded = load_or_create_settings(&root).expect("load migrated settings");

        assert_eq!(loaded.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(loaded.upload.provider, "catbox");
        assert!(!loaded.upload.auto_upload_enabled);
        let _ = fs::remove_dir_all(root);
    }

    struct SystemTimeCompat;

    impl SystemTimeCompat {
        fn millis() -> u128 {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        }
    }
}
