use crate::capture::{CaptureBackend, CaptureConfig, CaptureError};
use std::path::PathBuf;

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::*;
    use std::error::Error;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
    use windows_capture::encoder::{
        AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder,
        VideoSettingsSubType,
    };
    use windows_capture::frame::Frame;
    use windows_capture::graphics_capture_api::InternalCaptureControl;
    use windows_capture::monitor::Monitor;
    use windows_capture::settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
        MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    };

    type HandlerError = Box<dyn Error + Send + Sync>;

    struct WgcFlags {
        output_path: String,
        stop: Arc<AtomicBool>,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
    }

    struct WgcRecorderHandler {
        encoder: Option<VideoEncoder>,
        stop: Arc<AtomicBool>,
        started_at: Instant,
    }

    struct WgcSegmentFlags {
        segment_pattern: String,
        stop: Arc<AtomicBool>,
        segment_duration: Duration,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
    }

    struct WgcSegmentHandler {
        encoder: Option<VideoEncoder>,
        stop: Arc<AtomicBool>,
        segment_pattern: String,
        segment_duration: Duration,
        segment_started_at: Instant,
        segment_index: u32,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
    }

    impl GraphicsCaptureApiHandler for WgcRecorderHandler {
        type Flags = WgcFlags;
        type Error = HandlerError;

        fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
            let video = VideoSettingsBuilder::new(ctx.flags.width, ctx.flags.height)
                .sub_type(VideoSettingsSubType::H264)
                .frame_rate(ctx.flags.fps)
                .bitrate(ctx.flags.bitrate);
            let encoder = VideoEncoder::new(
                video,
                AudioSettingsBuilder::default().disabled(true),
                ContainerSettingsBuilder::default(),
                &ctx.flags.output_path,
            )?;

            Ok(Self {
                encoder: Some(encoder),
                stop: ctx.flags.stop,
                started_at: Instant::now(),
            })
        }

        fn on_frame_arrived(
            &mut self,
            frame: &mut Frame,
            capture_control: InternalCaptureControl,
        ) -> Result<(), Self::Error> {
            if let Some(encoder) = self.encoder.as_mut() {
                encoder.send_frame(frame)?;
            }

            if self.stop.load(Ordering::SeqCst) {
                if let Some(encoder) = self.encoder.take() {
                    encoder.finish()?;
                }
                capture_control.stop();
            }

            let _ = self.started_at.elapsed();
            Ok(())
        }

        fn on_closed(&mut self) -> Result<(), Self::Error> {
            self.stop.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    impl GraphicsCaptureApiHandler for WgcSegmentHandler {
        type Flags = WgcSegmentFlags;
        type Error = HandlerError;

        fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
            let first_segment = segment_path(&ctx.flags.segment_pattern, 0);
            let encoder = create_encoder(
                &first_segment,
                ctx.flags.width,
                ctx.flags.height,
                ctx.flags.fps,
                ctx.flags.bitrate,
            )?;

            Ok(Self {
                encoder: Some(encoder),
                stop: ctx.flags.stop,
                segment_pattern: ctx.flags.segment_pattern,
                segment_duration: ctx.flags.segment_duration,
                segment_started_at: Instant::now(),
                segment_index: 0,
                width: ctx.flags.width,
                height: ctx.flags.height,
                fps: ctx.flags.fps,
                bitrate: ctx.flags.bitrate,
            })
        }

        fn on_frame_arrived(
            &mut self,
            frame: &mut Frame,
            capture_control: InternalCaptureControl,
        ) -> Result<(), Self::Error> {
            if self.segment_started_at.elapsed() >= self.segment_duration {
                if let Some(encoder) = self.encoder.take() {
                    encoder.finish()?;
                }
                self.segment_index = self.segment_index.saturating_add(1);
                self.segment_started_at = Instant::now();
                let next_segment = segment_path(&self.segment_pattern, self.segment_index);
                self.encoder = Some(create_encoder(
                    &next_segment,
                    self.width,
                    self.height,
                    self.fps,
                    self.bitrate,
                )?);
            }

            if let Some(encoder) = self.encoder.as_mut() {
                encoder.send_frame(frame)?;
            }

            if self.stop.load(Ordering::SeqCst) {
                if let Some(encoder) = self.encoder.take() {
                    encoder.finish()?;
                }
                capture_control.stop();
            }

            Ok(())
        }

        fn on_closed(&mut self) -> Result<(), Self::Error> {
            self.stop.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    fn create_encoder(
        output_path: &str,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
    ) -> Result<VideoEncoder, HandlerError> {
        let video = VideoSettingsBuilder::new(width, height)
            .sub_type(VideoSettingsSubType::H264)
            .frame_rate(fps)
            .bitrate(bitrate);
        Ok(VideoEncoder::new(
            video,
            AudioSettingsBuilder::default().disabled(true),
            ContainerSettingsBuilder::default(),
            output_path,
        )?)
    }

    fn segment_path(pattern: &str, index: u32) -> String {
        if pattern.contains("%05d") {
            pattern.replace("%05d", &format!("{index:05}"))
        } else if pattern.contains("%d") {
            pattern.replace("%d", &index.to_string())
        } else {
            format!("{pattern}.{index:05}.mp4")
        }
    }

    pub struct NativeWgcCaptureBackend {
        output_path: PathBuf,
        stop: Option<Arc<AtomicBool>>,
        control: Option<CaptureControl<WgcRecorderHandler, HandlerError>>,
    }

    impl NativeWgcCaptureBackend {
        pub fn new(output_path: impl Into<PathBuf>) -> Self {
            Self {
                output_path: output_path.into(),
                stop: None,
                control: None,
            }
        }
    }

    impl CaptureBackend for NativeWgcCaptureBackend {
        fn name(&self) -> &'static str {
            "windows-graphics-capture"
        }

        fn start(&mut self, config: CaptureConfig) -> Result<(), CaptureError> {
            if self.control.is_some() {
                return Ok(());
            }
            let Some(parent) = self.output_path.parent() else {
                return Err(CaptureError::DeviceUnavailable(
                    "native WGC output path has no parent directory".to_string(),
                ));
            };
            std::fs::create_dir_all(parent)
                .map_err(|error| CaptureError::DeviceUnavailable(error.to_string()))?;

            let monitor = Monitor::primary()
                .map_err(|error| CaptureError::DeviceUnavailable(error.to_string()))?;
            let width = even_dimension(monitor.width().unwrap_or(config.width).min(config.width));
            let height =
                even_dimension(monitor.height().unwrap_or(config.height).min(config.height));
            let stop = Arc::new(AtomicBool::new(false));
            let settings = Settings::new(
                monitor,
                CursorCaptureSettings::Default,
                DrawBorderSettings::WithoutBorder,
                SecondaryWindowSettings::Default,
                MinimumUpdateIntervalSettings::Default,
                DirtyRegionSettings::Default,
                ColorFormat::Bgra8,
                WgcFlags {
                    output_path: self.output_path.display().to_string(),
                    stop: stop.clone(),
                    width,
                    height,
                    fps: config.fps,
                    bitrate: config.bitrate_kbps.saturating_mul(1_000),
                },
            );
            let control = WgcRecorderHandler::start_free_threaded(settings)
                .map_err(|error| CaptureError::DeviceUnavailable(error.to_string()))?;
            self.stop = Some(stop);
            self.control = Some(control);
            Ok(())
        }

        fn stop(&mut self) -> Result<(), CaptureError> {
            if let Some(stop) = &self.stop {
                stop.store(true, Ordering::SeqCst);
            }
            if let Some(control) = self.control.take() {
                control
                    .stop()
                    .map_err(|error| CaptureError::DeviceUnavailable(format!("{error:?}")))?;
            }
            self.stop = None;
            Ok(())
        }
    }

    impl Drop for NativeWgcCaptureBackend {
        fn drop(&mut self) {
            let _ = self.stop();
        }
    }

    pub struct NativeWgcReplayCaptureBackend {
        segment_pattern: PathBuf,
        segment_duration: Duration,
        stop: Option<Arc<AtomicBool>>,
        control: Option<CaptureControl<WgcSegmentHandler, HandlerError>>,
    }

    impl NativeWgcReplayCaptureBackend {
        pub fn new(segment_pattern: impl Into<PathBuf>, segment_duration: Duration) -> Self {
            Self {
                segment_pattern: segment_pattern.into(),
                segment_duration,
                stop: None,
                control: None,
            }
        }
    }

    impl CaptureBackend for NativeWgcReplayCaptureBackend {
        fn name(&self) -> &'static str {
            "windows-graphics-capture-segments"
        }

        fn start(&mut self, config: CaptureConfig) -> Result<(), CaptureError> {
            if self.control.is_some() {
                return Ok(());
            }
            let Some(parent) = self.segment_pattern.parent() else {
                return Err(CaptureError::DeviceUnavailable(
                    "native WGC segment pattern has no parent directory".to_string(),
                ));
            };
            std::fs::create_dir_all(parent)
                .map_err(|error| CaptureError::DeviceUnavailable(error.to_string()))?;

            let monitor = Monitor::primary()
                .map_err(|error| CaptureError::DeviceUnavailable(error.to_string()))?;
            let width = even_dimension(monitor.width().unwrap_or(config.width));
            let height = even_dimension(monitor.height().unwrap_or(config.height));
            let stop = Arc::new(AtomicBool::new(false));
            let settings = Settings::new(
                monitor,
                CursorCaptureSettings::Default,
                DrawBorderSettings::WithoutBorder,
                SecondaryWindowSettings::Default,
                MinimumUpdateIntervalSettings::Default,
                DirtyRegionSettings::Default,
                ColorFormat::Bgra8,
                WgcSegmentFlags {
                    segment_pattern: self.segment_pattern.display().to_string(),
                    stop: stop.clone(),
                    segment_duration: self.segment_duration,
                    width,
                    height,
                    fps: config.fps,
                    bitrate: config.bitrate_kbps.saturating_mul(1_000),
                },
            );
            let control = WgcSegmentHandler::start_free_threaded(settings)
                .map_err(|error| CaptureError::DeviceUnavailable(error.to_string()))?;
            self.stop = Some(stop);
            self.control = Some(control);
            Ok(())
        }

        fn stop(&mut self) -> Result<(), CaptureError> {
            if let Some(stop) = &self.stop {
                stop.store(true, Ordering::SeqCst);
            }
            if let Some(control) = self.control.take() {
                control
                    .stop()
                    .map_err(|error| CaptureError::DeviceUnavailable(format!("{error:?}")))?;
            }
            self.stop = None;
            Ok(())
        }
    }

    impl Drop for NativeWgcReplayCaptureBackend {
        fn drop(&mut self) {
            let _ = self.stop();
        }
    }

    fn even_dimension(value: u32) -> u32 {
        value.saturating_sub(value % 2).max(2)
    }
}

#[cfg(target_os = "windows")]
pub use windows_impl::NativeWgcCaptureBackend;
#[cfg(target_os = "windows")]
pub use windows_impl::NativeWgcReplayCaptureBackend;

#[cfg(not(target_os = "windows"))]
pub struct NativeWgcCaptureBackend;
#[cfg(not(target_os = "windows"))]
pub struct NativeWgcReplayCaptureBackend;

#[cfg(not(target_os = "windows"))]
impl NativeWgcCaptureBackend {
    pub fn new(_output_path: impl Into<PathBuf>) -> Self {
        Self
    }
}

#[cfg(not(target_os = "windows"))]
impl NativeWgcReplayCaptureBackend {
    pub fn new(
        _segment_pattern: impl Into<PathBuf>,
        _segment_duration: std::time::Duration,
    ) -> Self {
        Self
    }
}

#[cfg(not(target_os = "windows"))]
impl CaptureBackend for NativeWgcCaptureBackend {
    fn name(&self) -> &'static str {
        "windows-graphics-capture"
    }

    fn start(&mut self, _config: CaptureConfig) -> Result<(), CaptureError> {
        Err(CaptureError::UnsupportedPlatform)
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
impl CaptureBackend for NativeWgcReplayCaptureBackend {
    fn name(&self) -> &'static str {
        "windows-graphics-capture-segments"
    }

    fn start(&mut self, _config: CaptureConfig) -> Result<(), CaptureError> {
        Err(CaptureError::UnsupportedPlatform)
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        Ok(())
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use crate::capture::{CaptureMethod, CaptureSource, EncoderPreference};
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[test]
    #[ignore = "captures the desktop for a short local smoke test"]
    fn native_wgc_replay_smoke_writes_segment() {
        let root = std::env::temp_dir().join(format!(
            "clipforge-wgc-smoke-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));
        std::fs::create_dir_all(&root).expect("smoke root");
        let pattern = root.join("segment-%05d.mp4");
        let config = CaptureConfig {
            source: CaptureSource::Desktop,
            method: CaptureMethod::DesktopDuplication,
            width: 1280,
            height: 720,
            fps: 10,
            bitrate_kbps: 1_000,
            encoder: EncoderPreference::HardwareH264,
            system_audio_enabled: false,
            mic_enabled: false,
            system_audio_device: None,
            mic_device: None,
        };
        let mut backend = NativeWgcReplayCaptureBackend::new(&pattern, Duration::from_secs(1));

        backend.start(config).expect("start native WGC");
        thread::sleep(Duration::from_secs(3));
        backend.stop().expect("stop native WGC");

        let segment_count = std::fs::read_dir(&root)
            .expect("read smoke root")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.path().extension().and_then(|value| value.to_str()) == Some("mp4")
            })
            .count();
        assert!(
            segment_count > 0,
            "native WGC should write at least one segment"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
