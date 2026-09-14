use crate::capture::{CaptureConfig, CaptureError, CaptureMethod, CaptureSource, EncoderPreference};
use crate::proc::hidden_command;
use dirs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

impl Default for ScreenshotCapture {
    fn default() -> Self {
        let output_dir = default_screenshot_dir();
        Self {
            executable: default_ffmpeg_executable(),
            output_dir,
        }
    }
}

fn default_ffmpeg_executable() -> PathBuf {
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
        .map(|parent| parent.join("ffmpeg-x86_64-pc-windows-msvc.exe"));
    if let Some(path) = bundled {
        if path.exists() {
            return path;
        }
    }
    PathBuf::from("ffmpeg")
}

fn default_screenshot_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join("Pictures").join("ClipForge")
}

pub fn screenshot_dir() -> PathBuf {
    default_screenshot_dir()
}

pub fn list_screenshots() -> Vec<PathBuf> {
    let dir = default_screenshot_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut screenshots = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .map(|ext| {
                        ext.eq_ignore_ascii_case("png")
                            || ext.eq_ignore_ascii_case("jpg")
                            || ext.eq_ignore_ascii_case("jpeg")
                    })
                    .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    screenshots.sort_by_key(|path| {
        std::fs::metadata(path)
            .and_then(|meta| meta.created())
            .ok()
    });
    screenshots
}

pub fn take_screenshot() -> Result<PathBuf, CaptureError> {
    let capture = ScreenshotCapture::default();
    let config = CaptureConfig {
        source: CaptureSource::Desktop,
        method: CaptureMethod::DesktopDuplication,
        width: 1920,
        height: 1080,
        fps: 30,
        bitrate_kbps: 6000,
        encoder: EncoderPreference::HardwareH264,
        system_audio_enabled: false,
        mic_enabled: false,
        system_audio_device: None,
        mic_device: None,
        separate_audio_tracks: false,
    };
    capture.capture(&config)
}

#[derive(Debug, Clone)]
pub struct ScreenshotCapture {
    executable: PathBuf,
    output_dir: PathBuf,
}

impl ScreenshotCapture {
    pub fn new(executable: impl Into<PathBuf>, output_dir: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            output_dir: output_dir.into(),
        }
    }

    pub fn capture(&self, config: &CaptureConfig) -> Result<PathBuf, CaptureError> {
        let now = SystemTime::now();
        let timestamp = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
        let output_path = self.output_dir.join(format!("screenshot-{}.png", timestamp));

        std::fs::create_dir_all(&self.output_dir).map_err(|e| CaptureError::DeviceUnavailable(e.to_string()))?;

        let size = format!("{}x{}", config.width, config.height);
        let fps = config.fps.to_string();

        let mut args = vec![
            "-y".to_string(),
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "error".to_string(),
        ];

        match config.method {
            CaptureMethod::DesktopDuplication => {
                args.extend([
                    "-f".to_string(),
                    "lavfi".to_string(),
                    "-i".to_string(),
                    format!("ddagrab=framerate={fps}:video_size={size},hwdownload,format=bgra"),
                ]);
            }
            CaptureMethod::GdiGrab => {
                args.extend([
                    "-f".to_string(),
                    "gdigrab".to_string(),
                    "-framerate".to_string(),
                    fps,
                    "-video_size".to_string(),
                    size,
                    "-i".to_string(),
                    "desktop".to_string(),
                ]);
            }
        }

        args.extend([
            "-vframes".to_string(),
            "1".to_string(),
            "-update".to_string(),
            "1".to_string(),
            output_path.display().to_string(),
        ]);

        let status = hidden_command(&self.executable)
            .args(&args)
            .status()
            .map_err(|e| CaptureError::DeviceUnavailable(e.to_string()))?;

        if !status.success() {
            return Err(CaptureError::DeviceUnavailable("FFmpeg screenshot failed".to_string()));
        }

        Ok(output_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{CaptureMethod, CaptureSource, EncoderPreference};

    #[test]
    fn test_screenshot_args() {
        let _config = CaptureConfig {
            source: CaptureSource::Desktop,
            method: CaptureMethod::GdiGrab,
            width: 1920,
            height: 1080,
            fps: 30,
            bitrate_kbps: 6000,
            encoder: EncoderPreference::HardwareH264,
            system_audio_enabled: false,
            mic_enabled: false,
            system_audio_device: None,
            mic_device: None,
            separate_audio_tracks: false,
        };

        let capture = ScreenshotCapture::new("ffmpeg", "screenshots");
        assert_eq!(capture.output_dir, PathBuf::from("screenshots"));
        assert_eq!(capture.executable, PathBuf::from("ffmpeg"));
    }
}