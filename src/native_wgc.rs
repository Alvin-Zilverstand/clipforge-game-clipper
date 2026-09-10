use crate::capture::{CaptureBackend, CaptureConfig, CaptureError};
use std::path::PathBuf;

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::*;
    use crate::native_audio::NativeAudioCapture;
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
        GraphicsCaptureItemType, MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    };
    use windows_capture::window::Window;

    type HandlerError = Box<dyn Error + Send + Sync>;

    struct WgcFlags {
        output_path: String,
        stop: Arc<AtomicBool>,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
        system_audio_enabled: bool,
        mic_enabled: bool,
        system_audio_device: Option<String>,
        mic_device: Option<String>,
    }

    struct WgcRecorderHandler {
        encoder: Option<VideoEncoder>,
        audio: Vec<NativeAudioCapture>,
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
        system_audio_enabled: bool,
        mic_enabled: bool,
        system_audio_device: Option<String>,
        mic_device: Option<String>,
    }

    struct WgcSegmentHandler {
        encoder: Option<VideoEncoder>,
        audio: Vec<NativeAudioCapture>,
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
                AudioSettingsBuilder::default()
                    .disabled(!(ctx.flags.system_audio_enabled || ctx.flags.mic_enabled)),
                ContainerSettingsBuilder::default(),
                &ctx.flags.output_path,
            )?;
            let audio = start_audio_captures(
                ctx.flags.system_audio_enabled,
                ctx.flags.mic_enabled,
                ctx.flags.system_audio_device.as_deref(),
                ctx.flags.mic_device.as_deref(),
            )?;

            Ok(Self {
                encoder: Some(encoder),
                audio,
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
                for chunk in mixed_audio_chunks(&self.audio) {
                    encoder.send_audio_buffer(&chunk, 0)?;
                }
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
                ctx.flags.system_audio_enabled || ctx.flags.mic_enabled,
            )?;
            let audio = start_audio_captures(
                ctx.flags.system_audio_enabled,
                ctx.flags.mic_enabled,
                ctx.flags.system_audio_device.as_deref(),
                ctx.flags.mic_device.as_deref(),
            )?;

            Ok(Self {
                encoder: Some(encoder),
                audio,
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
                    !self.audio.is_empty(),
                )?);
            }

            if let Some(encoder) = self.encoder.as_mut() {
                for chunk in mixed_audio_chunks(&self.audio) {
                    encoder.send_audio_buffer(&chunk, 0)?;
                }
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
        include_audio: bool,
    ) -> Result<VideoEncoder, HandlerError> {
        let video = VideoSettingsBuilder::new(width, height)
            .sub_type(VideoSettingsSubType::H264)
            .frame_rate(fps)
            .bitrate(bitrate);
        Ok(VideoEncoder::new(
            video,
            AudioSettingsBuilder::default().disabled(!include_audio),
            ContainerSettingsBuilder::default(),
            output_path,
        )?)
    }

    fn start_audio_captures(
        system_audio_enabled: bool,
        mic_enabled: bool,
        system_audio_device: Option<&str>,
        mic_device: Option<&str>,
    ) -> Result<Vec<NativeAudioCapture>, HandlerError> {
        let mut captures = Vec::new();
        if system_audio_enabled {
            captures.push(NativeAudioCapture::start_system_loopback(
                system_audio_device,
            )?);
        }
        if mic_enabled {
            captures.push(NativeAudioCapture::start_microphone(mic_device)?);
        }
        Ok(captures)
    }

    fn mixed_audio_chunks(audio: &[NativeAudioCapture]) -> Vec<Vec<u8>> {
        match audio {
            [] => Vec::new(),
            [single] => single.drain_chunks(),
            captures => {
                let drained = captures
                    .iter()
                    .map(NativeAudioCapture::drain_chunks)
                    .collect::<Vec<_>>();
                let chunk_count = drained.iter().map(Vec::len).max().unwrap_or(0);
                let mut mixed = Vec::new();
                for chunk_index in 0..chunk_count {
                    let target_len = drained
                        .iter()
                        .filter_map(|chunks| chunks.get(chunk_index))
                        .map(Vec::len)
                        .max()
                        .unwrap_or(0);
                    if target_len == 0 {
                        continue;
                    }
                    let mut output = vec![0u8; target_len - (target_len % 2)];
                    for sample_offset in (0..output.len()).step_by(2) {
                        let mut sample = 0i32;
                        for chunks in &drained {
                            if let Some(chunk) = chunks.get(chunk_index) {
                                if sample_offset + 1 < chunk.len() {
                                    let value = i16::from_le_bytes([
                                        chunk[sample_offset],
                                        chunk[sample_offset + 1],
                                    ]);
                                    sample += value as i32;
                                }
                            }
                        }
                        let sample = sample.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                        output[sample_offset..sample_offset + 2]
                            .copy_from_slice(&sample.to_le_bytes());
                    }
                    mixed.push(output);
                }
                mixed
            }
        }
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

            let stop = Arc::new(AtomicBool::new(false));
            let control = if let Some(window) = find_window_for_source(&config.source)
                .map_err(|error| CaptureError::DeviceUnavailable(error.to_string()))?
            {
                let (width, height) = window_size(window, config.width, config.height);
                let settings = recording_settings(
                    window,
                    &self.output_path,
                    &config,
                    stop.clone(),
                    width,
                    height,
                );
                WgcRecorderHandler::start_free_threaded(settings)
                    .map_err(|error| CaptureError::DeviceUnavailable(error.to_string()))?
            } else {
                let monitor = Monitor::primary()
                    .map_err(|error| CaptureError::DeviceUnavailable(error.to_string()))?;
                let (width, height) = monitor_size(monitor, config.width, config.height);
                let settings = recording_settings(
                    monitor,
                    &self.output_path,
                    &config,
                    stop.clone(),
                    width,
                    height,
                );
                WgcRecorderHandler::start_free_threaded(settings)
                    .map_err(|error| CaptureError::DeviceUnavailable(error.to_string()))?
            };
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

            let stop = Arc::new(AtomicBool::new(false));
            let control = if let Some(window) = find_window_for_source(&config.source)
                .map_err(|error| CaptureError::DeviceUnavailable(error.to_string()))?
            {
                let (width, height) = window_size(window, config.width, config.height);
                let settings = segment_settings(
                    window,
                    &self.segment_pattern,
                    self.segment_duration,
                    &config,
                    stop.clone(),
                    width,
                    height,
                );
                WgcSegmentHandler::start_free_threaded(settings)
                    .map_err(|error| CaptureError::DeviceUnavailable(error.to_string()))?
            } else {
                let monitor = Monitor::primary()
                    .map_err(|error| CaptureError::DeviceUnavailable(error.to_string()))?;
                let (width, height) = monitor_size(monitor, config.width, config.height);
                let settings = segment_settings(
                    monitor,
                    &self.segment_pattern,
                    self.segment_duration,
                    &config,
                    stop.clone(),
                    width,
                    height,
                );
                WgcSegmentHandler::start_free_threaded(settings)
                    .map_err(|error| CaptureError::DeviceUnavailable(error.to_string()))?
            };
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

    fn find_window_for_source(
        source: &crate::capture::CaptureSource,
    ) -> Result<Option<Window>, HandlerError> {
        let crate::capture::CaptureSource::GameWindow {
            process_name,
            window_title,
        } = source
        else {
            return Ok(None);
        };

        let lowered_title = window_title.to_lowercase();
        for window in Window::enumerate()? {
            let process_matches = window
                .process_name()
                .map(|name| name.eq_ignore_ascii_case(process_name))
                .unwrap_or(false);
            let title_matches = if lowered_title.is_empty() {
                false
            } else {
                window
                    .title()
                    .map(|title| title.to_lowercase().contains(&lowered_title))
                    .unwrap_or(false)
            };

            if process_matches || title_matches {
                return Ok(Some(window));
            }
        }

        Ok(None)
    }

    fn window_size(window: Window, fallback_width: u32, fallback_height: u32) -> (u32, u32) {
        let width = window
            .width()
            .ok()
            .and_then(|value| u32::try_from(value.max(0)).ok())
            .unwrap_or(fallback_width);
        let height = window
            .height()
            .ok()
            .and_then(|value| u32::try_from(value.max(0)).ok())
            .unwrap_or(fallback_height);
        (even_dimension(width), even_dimension(height))
    }

    fn monitor_size(monitor: Monitor, fallback_width: u32, fallback_height: u32) -> (u32, u32) {
        (
            even_dimension(monitor.width().unwrap_or(fallback_width)),
            even_dimension(monitor.height().unwrap_or(fallback_height)),
        )
    }

    fn recording_settings<T: TryInto<GraphicsCaptureItemType> + Send + 'static>(
        item: T,
        output_path: &std::path::Path,
        config: &CaptureConfig,
        stop: Arc<AtomicBool>,
        width: u32,
        height: u32,
    ) -> Settings<WgcFlags, T> {
        Settings::new(
            item,
            CursorCaptureSettings::Default,
            DrawBorderSettings::WithoutBorder,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            WgcFlags {
                output_path: output_path.display().to_string(),
                stop,
                width,
                height,
                fps: config.fps,
                bitrate: config.bitrate_kbps.saturating_mul(1_000),
                system_audio_enabled: config.system_audio_enabled,
                mic_enabled: config.mic_enabled,
                system_audio_device: config.system_audio_device.clone(),
                mic_device: config.mic_device.clone(),
            },
        )
    }

    fn segment_settings<T: TryInto<GraphicsCaptureItemType> + Send + 'static>(
        item: T,
        segment_pattern: &std::path::Path,
        segment_duration: Duration,
        config: &CaptureConfig,
        stop: Arc<AtomicBool>,
        width: u32,
        height: u32,
    ) -> Settings<WgcSegmentFlags, T> {
        Settings::new(
            item,
            CursorCaptureSettings::Default,
            DrawBorderSettings::WithoutBorder,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            WgcSegmentFlags {
                segment_pattern: segment_pattern.display().to_string(),
                stop,
                segment_duration,
                width,
                height,
                fps: config.fps,
                bitrate: config.bitrate_kbps.saturating_mul(1_000),
                system_audio_enabled: config.system_audio_enabled,
                mic_enabled: config.mic_enabled,
                system_audio_device: config.system_audio_device.clone(),
                mic_device: config.mic_device.clone(),
            },
        )
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
    use std::sync::Mutex;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    static WGC_SMOKE_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    #[ignore = "captures the desktop for a short local smoke test"]
    fn native_wgc_replay_smoke_writes_segment() {
        let _guard = WGC_SMOKE_LOCK.lock().expect("lock WGC smoke");
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

    #[test]
    #[ignore = "captures the desktop with native WASAPI system loopback briefly"]
    fn native_wgc_replay_smoke_writes_segment_with_system_audio() {
        let _guard = WGC_SMOKE_LOCK.lock().expect("lock WGC smoke");
        let root = std::env::temp_dir().join(format!(
            "clipforge-wgc-audio-smoke-{}",
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
            system_audio_enabled: true,
            mic_enabled: false,
            system_audio_device: None,
            mic_device: None,
        };
        let mut backend = NativeWgcReplayCaptureBackend::new(&pattern, Duration::from_secs(1));

        backend.start(config).expect("start native WGC with audio");
        thread::sleep(Duration::from_secs(3));
        backend.stop().expect("stop native WGC with audio");

        let segment_count = std::fs::read_dir(&root)
            .expect("read smoke root")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.path().extension().and_then(|value| value.to_str()) == Some("mp4")
            })
            .count();
        assert!(
            segment_count > 0,
            "native WGC with audio should write at least one segment"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    #[ignore = "captures the desktop with native WASAPI microphone briefly"]
    fn native_wgc_replay_smoke_writes_segment_with_mic() {
        let _guard = WGC_SMOKE_LOCK.lock().expect("lock WGC smoke");
        let root = std::env::temp_dir().join(format!(
            "clipforge-wgc-mic-smoke-{}",
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
            mic_enabled: true,
            system_audio_device: None,
            mic_device: None,
        };
        let mut backend = NativeWgcReplayCaptureBackend::new(&pattern, Duration::from_secs(1));

        backend.start(config).expect("start native WGC with mic");
        thread::sleep(Duration::from_secs(3));
        backend.stop().expect("stop native WGC with mic");

        let segment_count = std::fs::read_dir(&root)
            .expect("read smoke root")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.path().extension().and_then(|value| value.to_str()) == Some("mp4")
            })
            .count();
        assert!(
            segment_count > 0,
            "native WGC with mic should write at least one segment"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
