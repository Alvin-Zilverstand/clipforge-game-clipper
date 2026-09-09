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
    pub system_audio_enabled: bool,
    pub mic_enabled: bool,
    pub system_audio_device: Option<String>,
    pub mic_device: Option<String>,
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

pub fn ffmpeg_supports_input_device(executable: &std::path::Path, device: &str) -> bool {
    let Ok(output) = Command::new(executable)
        .args(["-hide_banner", "-devices"])
        .output()
    else {
        return false;
    };
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    combined
        .lines()
        .any(|line| line.contains('D') && line.split_whitespace().any(|part| part == device))
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
        let output = output.into();

        let mut args = base_desktop_input_args(config);
        args.extend([
            "-movflags".to_string(),
            "+faststart".to_string(),
            output.display().to_string(),
        ]);

        Self {
            executable: executable.into(),
            args,
        }
    }

    pub fn for_windows_desktop_segments(
        executable: impl Into<PathBuf>,
        output_pattern: impl Into<PathBuf>,
        config: &CaptureConfig,
        segment_duration: Duration,
    ) -> Self {
        let output_pattern = output_pattern.into();
        let mut args = base_desktop_input_args(config);
        args.extend([
            "-f".to_string(),
            "segment".to_string(),
            "-segment_time".to_string(),
            segment_duration.as_secs().max(1).to_string(),
            "-reset_timestamps".to_string(),
            "1".to_string(),
            "-segment_format".to_string(),
            "mp4".to_string(),
            output_pattern.display().to_string(),
        ]);

        Self {
            executable: executable.into(),
            args,
        }
    }
}

fn base_desktop_input_args(config: &CaptureConfig) -> Vec<String> {
    let preset = match config.encoder {
        EncoderPreference::HardwareH264 => "h264_mf",
        EncoderPreference::SoftwareH264 => "libx264",
    };
    let size = format!("{}x{}", config.width, config.height);
    let fps = config.fps.to_string();
    let bitrate = format!("{}k", config.bitrate_kbps);
    let mut args = vec![
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
    ];
    let mut next_input_index = 1;
    let mut audio_maps = Vec::new();

    if config.system_audio_enabled {
        args.extend([
            "-f".to_string(),
            "wasapi".to_string(),
            "-i".to_string(),
            config
                .system_audio_device
                .clone()
                .unwrap_or_else(|| "default".to_string()),
        ]);
        audio_maps.push(format!("{next_input_index}:a?"));
        next_input_index += 1;
    }

    if config.mic_enabled {
        args.extend([
            "-f".to_string(),
            "dshow".to_string(),
            "-i".to_string(),
            format!(
                "audio={}",
                config
                    .mic_device
                    .clone()
                    .unwrap_or_else(|| "Microphone".to_string())
            ),
        ]);
        audio_maps.push(format!("{next_input_index}:a?"));
    }

    args.extend([
        "-map".to_string(),
        "0:v:0".to_string(),
        "-c:v".to_string(),
        preset.to_string(),
        "-b:v".to_string(),
        bitrate,
        "-pix_fmt".to_string(),
        "yuv420p".to_string(),
    ]);
    for audio_map in audio_maps {
        args.extend(["-map".to_string(), audio_map]);
    }
    if config.system_audio_enabled || config.mic_enabled {
        args.extend([
            "-c:a".to_string(),
            "aac".to_string(),
            "-b:a".to_string(),
            "160k".to_string(),
        ]);
    }
    args
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

#[derive(Debug)]
pub struct FfmpegReplayCaptureBackend {
    executable: PathBuf,
    output_pattern: PathBuf,
    segment_duration: Duration,
    child: Option<Child>,
}

impl FfmpegReplayCaptureBackend {
    pub fn new(
        executable: impl Into<PathBuf>,
        output_pattern: impl Into<PathBuf>,
        segment_duration: Duration,
    ) -> Self {
        Self {
            executable: executable.into(),
            output_pattern: output_pattern.into(),
            segment_duration,
            child: None,
        }
    }

    pub fn plan(&self, config: &CaptureConfig) -> FfmpegRecordingPlan {
        FfmpegRecordingPlan::for_windows_desktop_segments(
            &self.executable,
            &self.output_pattern,
            config,
            self.segment_duration,
        )
    }
}

impl CaptureBackend for FfmpegReplayCaptureBackend {
    fn name(&self) -> &'static str {
        "ffmpeg-gdigrab-segments"
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
        stop_ffmpeg_child(&mut self.child)
    }
}

impl Drop for FfmpegReplayCaptureBackend {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn stop_ffmpeg_child(child: &mut Option<Child>) -> Result<(), CaptureError> {
    if let Some(mut child) = child.take() {
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
            system_audio_enabled: false,
            mic_enabled: false,
            system_audio_device: None,
            mic_device: None,
        };

        let plan = FfmpegRecordingPlan::for_windows_desktop("ffmpeg", "out.mp4", &config);

        assert!(plan.args.contains(&"h264_mf".to_string()));
        assert!(plan.args.contains(&"1280x720".to_string()));
        assert!(plan.args.contains(&"6000k".to_string()));
    }

    #[test]
    fn ffmpeg_segment_plan_uses_segment_muxer() {
        let config = CaptureConfig {
            source: CaptureSource::Desktop,
            width: 1280,
            height: 720,
            fps: 30,
            bitrate_kbps: 6000,
            encoder: EncoderPreference::HardwareH264,
            system_audio_enabled: false,
            mic_enabled: false,
            system_audio_device: None,
            mic_device: None,
        };

        let plan = FfmpegRecordingPlan::for_windows_desktop_segments(
            "ffmpeg",
            "segment-%05d.mp4",
            &config,
            Duration::from_secs(5),
        );

        assert!(plan.args.contains(&"segment".to_string()));
        assert!(plan.args.contains(&"segment-%05d.mp4".to_string()));
    }

    #[test]
    fn device_parser_ignores_unknown_executable() {
        assert!(!ffmpeg_supports_input_device(
            PathBuf::from("definitely-missing-ffmpeg.exe").as_path(),
            "wasapi"
        ));
    }
}
