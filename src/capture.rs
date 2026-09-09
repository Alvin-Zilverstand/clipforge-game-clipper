use std::fmt;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureSource {
    GameWindow {
        process_name: String,
        window_title: String,
    },
    Monitor {
        display_id: String,
    },
    Desktop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncoderPreference {
    HardwareH264,
    SoftwareH264,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureConfig {
    pub source: CaptureSource,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
    pub encoder: EncoderPreference,
    pub mic_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureError {
    UnsupportedPlatform,
    DeviceUnavailable(String),
    PermissionDenied(String),
    EncoderUnavailable(String),
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => {
                f.write_str("capture is currently implemented for Windows only")
            }
            Self::DeviceUnavailable(message) => write!(f, "capture device unavailable: {message}"),
            Self::PermissionDenied(message) => write!(f, "capture permission denied: {message}"),
            Self::EncoderUnavailable(message) => write!(f, "encoder unavailable: {message}"),
        }
    }
}

pub trait CaptureBackend {
    fn name(&self) -> &'static str;
    fn start(&mut self, config: CaptureConfig) -> Result<(), CaptureError>;
    fn stop(&mut self) -> Result<(), CaptureError>;
}

#[derive(Debug, Default)]
pub struct WindowsCaptureBackend {
    active: bool,
}

impl CaptureBackend for WindowsCaptureBackend {
    fn name(&self) -> &'static str {
        "windows-graphics-capture"
    }

    fn start(&mut self, _config: CaptureConfig) -> Result<(), CaptureError> {
        #[cfg(target_os = "windows")]
        {
            self.active = true;
            Ok(())
        }

        #[cfg(not(target_os = "windows"))]
        {
            Err(CaptureError::UnsupportedPlatform)
        }
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        self.active = false;
        Ok(())
    }
}

pub fn find_ffmpeg_executable() -> Option<PathBuf> {
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
        .map(|parent| parent.join("ffmpeg-x86_64-pc-windows-msvc.exe"));
    if let Some(path) = bundled {
        if path.exists() {
            return Some(path);
        }
    }

    let candidates = ["ffmpeg.exe", "ffmpeg"];
    for candidate in candidates {
        if Command::new(candidate).arg("-version").output().is_ok() {
            return Some(PathBuf::from(candidate));
        }
    }

    None
}

pub fn ffmpeg_is_available() -> bool {
    find_ffmpeg_executable().is_some()
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegRecordingPlan {
    pub executable: PathBuf,
    pub args: Vec<String>,
}

impl FfmpegRecordingPlan {
    pub fn for_windows_desktop(
        executable: impl Into<PathBuf>,
        output: impl Into<PathBuf>,
        config: &CaptureConfig,
    ) -> Self {
        let preset = match config.encoder {
            EncoderPreference::HardwareH264 => "h264_mf",
            EncoderPreference::SoftwareH264 => "libx264",
        };
        let output = output.into();
        let size = format!("{}x{}", config.width, config.height);
        let fps = config.fps.to_string();
        let bitrate = format!("{}k", config.bitrate_kbps);

        Self {
            executable: executable.into(),
            args: vec![
                "-y".to_string(),
                "-hide_banner".to_string(),
                "-loglevel".to_string(),
                "error".to_string(),
                "-f".to_string(),
                "gdigrab".to_string(),
                "-framerate".to_string(),
                fps,
                "-video_size".to_string(),
                size,
                "-i".to_string(),
                "desktop".to_string(),
                "-c:v".to_string(),
                preset.to_string(),
                "-b:v".to_string(),
                bitrate,
                "-pix_fmt".to_string(),
                "yuv420p".to_string(),
                "-movflags".to_string(),
                "+faststart".to_string(),
                output.display().to_string(),
            ],
        }
    }
}

#[derive(Debug)]
pub struct FfmpegCaptureBackend {
    executable: PathBuf,
    output: PathBuf,
    child: Option<Child>,
}

impl FfmpegCaptureBackend {
    pub fn new(executable: impl Into<PathBuf>, output: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            output: output.into(),
            child: None,
        }
    }

    pub fn plan(&self, config: &CaptureConfig) -> FfmpegRecordingPlan {
        FfmpegRecordingPlan::for_windows_desktop(&self.executable, &self.output, config)
    }
}

impl CaptureBackend for FfmpegCaptureBackend {
    fn name(&self) -> &'static str {
        "ffmpeg-gdigrab"
    }

    fn start(&mut self, config: CaptureConfig) -> Result<(), CaptureError> {
        if self.child.is_some() {
            return Ok(());
        }

        let plan = self.plan(&config);
        let child = Command::new(&plan.executable)
            .args(&plan.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| CaptureError::DeviceUnavailable(error.to_string()))?;
        self.child = Some(child);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        if let Some(mut child) = self.child.take() {
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(b"q\n");
            }
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if child
                    .try_wait()
                    .map_err(|error| CaptureError::DeviceUnavailable(error.to_string()))?
                    .is_some()
                {
                    break;
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
        Ok(())
    }
}

impl Drop for FfmpegCaptureBackend {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffmpeg_plan_prefers_media_foundation_h264() {
        let config = CaptureConfig {
            source: CaptureSource::Desktop,
            width: 1280,
            height: 720,
            fps: 30,
            bitrate_kbps: 6000,
            encoder: EncoderPreference::HardwareH264,
            mic_enabled: false,
        };

        let plan = FfmpegRecordingPlan::for_windows_desktop("ffmpeg", "out.mp4", &config);

        assert!(plan.args.contains(&"h264_mf".to_string()));
        assert!(plan.args.contains(&"1280x720".to_string()));
        assert!(plan.args.contains(&"6000k".to_string()));
    }
}
