use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeAudioSource {
    SystemLoopback,
    Microphone,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeAudioDevice {
    pub name: String,
    pub source: NativeAudioSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativePcmFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub block_align: u16,
}

impl NativePcmFormat {
    pub fn stereo_i16_48khz() -> Self {
        Self {
            sample_rate: 48_000,
            channels: 2,
            bits_per_sample: 16,
            block_align: 4,
        }
    }
}

pub struct NativeAudioCapture {
    stop: Arc<AtomicBool>,
    rx: Receiver<Vec<u8>>,
    worker: Option<JoinHandle<()>>,
    format: NativePcmFormat,
}

impl NativeAudioCapture {
    #[cfg(target_os = "windows")]
    pub fn start_default_system_loopback() -> Result<Self, String> {
        Self::start_system_loopback(None)
    }

    #[cfg(not(target_os = "windows"))]
    pub fn start_default_system_loopback() -> Result<Self, String> {
        Err("native WASAPI capture is only available on Windows".to_string())
    }

    #[cfg(target_os = "windows")]
    pub fn start_default_microphone() -> Result<Self, String> {
        Self::start_microphone(None)
    }

    #[cfg(not(target_os = "windows"))]
    pub fn start_default_microphone() -> Result<Self, String> {
        Err("native WASAPI capture is only available on Windows".to_string())
    }

    #[cfg(target_os = "windows")]
    pub fn start_system_loopback(device_name: Option<&str>) -> Result<Self, String> {
        Self::start(
            NativeAudioSource::SystemLoopback,
            device_name.map(str::to_string),
        )
    }

    #[cfg(not(target_os = "windows"))]
    pub fn start_system_loopback(_device_name: Option<&str>) -> Result<Self, String> {
        Err("native WASAPI capture is only available on Windows".to_string())
    }

    #[cfg(target_os = "windows")]
    pub fn start_microphone(device_name: Option<&str>) -> Result<Self, String> {
        Self::start(
            NativeAudioSource::Microphone,
            device_name.map(str::to_string),
        )
    }

    #[cfg(not(target_os = "windows"))]
    pub fn start_microphone(_device_name: Option<&str>) -> Result<Self, String> {
        Err("native WASAPI capture is only available on Windows".to_string())
    }

    pub fn format(&self) -> NativePcmFormat {
        self.format
    }

    pub fn drain_chunks(&self) -> Vec<Vec<u8>> {
        let mut chunks = Vec::new();
        while let Ok(chunk) = self.rx.try_recv() {
            chunks.push(chunk);
        }
        chunks
    }

    #[cfg(target_os = "windows")]
    fn start(source: NativeAudioSource, device_name: Option<String>) -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let worker_stop = stop.clone();
        let format = NativePcmFormat::stereo_i16_48khz();
        let worker = thread::Builder::new()
            .name(match source {
                NativeAudioSource::SystemLoopback => "clipforge-wasapi-loopback".to_string(),
                NativeAudioSource::Microphone => "clipforge-wasapi-mic".to_string(),
            })
            .spawn(move || {
                if let Err(error) = windows_impl::capture_loop(source, device_name, worker_stop, tx)
                {
                    eprintln!("ClipForge native audio capture stopped: {error}");
                }
            })
            .map_err(|error| error.to_string())?;

        Ok(Self {
            stop,
            rx,
            worker: Some(worker),
            format,
        })
    }
}

impl Drop for NativeAudioCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(target_os = "windows")]
pub fn list_native_audio_devices() -> Vec<NativeAudioDevice> {
    windows_impl::list_native_audio_devices().unwrap_or_default()
}

#[cfg(not(target_os = "windows"))]
pub fn list_native_audio_devices() -> Vec<NativeAudioDevice> {
    Vec::new()
}

pub fn native_system_loopback_available() -> bool {
    list_native_audio_devices()
        .iter()
        .any(|device| device.source == NativeAudioSource::SystemLoopback)
}

pub fn native_microphone_available() -> bool {
    list_native_audio_devices()
        .iter()
        .any(|device| device.source == NativeAudioSource::Microphone)
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::{NativeAudioDevice, NativeAudioSource, NativePcmFormat};
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::Sender;
    use std::sync::Arc;
    use wasapi::{initialize_mta, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

    type WasapiResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

    pub fn list_native_audio_devices() -> WasapiResult<Vec<NativeAudioDevice>> {
        initialize_mta().ok()?;
        let enumerator = DeviceEnumerator::new()?;
        let mut devices = Vec::new();

        if let Ok(collection) = enumerator.get_device_collection(&Direction::Render) {
            for device in &collection {
                let device = device?;
                devices.push(NativeAudioDevice {
                    name: device.get_friendlyname()?,
                    source: NativeAudioSource::SystemLoopback,
                });
            }
        }

        if let Ok(collection) = enumerator.get_device_collection(&Direction::Capture) {
            for device in &collection {
                let device = device?;
                devices.push(NativeAudioDevice {
                    name: device.get_friendlyname()?,
                    source: NativeAudioSource::Microphone,
                });
            }
        }

        Ok(devices)
    }

    pub fn capture_loop(
        source: NativeAudioSource,
        device_name: Option<String>,
        stop: Arc<AtomicBool>,
        tx: Sender<Vec<u8>>,
    ) -> WasapiResult<()> {
        initialize_mta().ok()?;
        let format = NativePcmFormat::stereo_i16_48khz();
        let desired_format = WaveFormat::new(
            format.bits_per_sample as usize,
            format.bits_per_sample as usize,
            &SampleType::Int,
            format.sample_rate as usize,
            format.channels as usize,
            None,
        );
        let block_align = desired_format.get_blockalign() as usize;
        let chunksize_frames = 2048usize;
        let enumerator = DeviceEnumerator::new()?;
        let direction = match source {
            NativeAudioSource::SystemLoopback => Direction::Render,
            NativeAudioSource::Microphone => Direction::Capture,
        };
        let device = if let Some(name) = device_name {
            enumerator
                .get_device_collection(&direction)?
                .get_device_with_name(&name)?
        } else {
            enumerator.get_default_device(&direction)?
        };
        let mut audio_client = device.get_iaudioclient()?;
        let mode = StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: 0,
        };
        audio_client.initialize_client(&desired_format, &Direction::Capture, &mode)?;
        let event = audio_client.set_get_eventhandle()?;
        let capture_client = audio_client.get_audiocaptureclient()?;
        let mut sample_queue = VecDeque::with_capacity(block_align * chunksize_frames * 4);

        audio_client.start_stream()?;
        while !stop.load(Ordering::SeqCst) {
            capture_client.read_from_device_to_deque(&mut sample_queue)?;
            while sample_queue.len() >= block_align * chunksize_frames {
                let mut chunk = Vec::with_capacity(block_align * chunksize_frames);
                for _ in 0..(block_align * chunksize_frames) {
                    if let Some(byte) = sample_queue.pop_front() {
                        chunk.push(byte);
                    }
                }
                if tx.send(chunk).is_err() {
                    stop.store(true, Ordering::SeqCst);
                    break;
                }
            }
            let _ = event.wait_for_event(250);
        }
        audio_client.stop_stream()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_pcm_format_matches_encoder_audio_settings() {
        let format = NativePcmFormat::stereo_i16_48khz();

        assert_eq!(format.sample_rate, 48_000);
        assert_eq!(format.channels, 2);
        assert_eq!(format.bits_per_sample, 16);
        assert_eq!(format.block_align, 4);
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "opens the default WASAPI system loopback client briefly"]
    fn native_system_loopback_smoke_starts() {
        let capture = NativeAudioCapture::start_default_system_loopback()
            .expect("start default WASAPI loopback");
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert_eq!(capture.format(), NativePcmFormat::stereo_i16_48khz());
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "opens the default WASAPI microphone capture client briefly"]
    fn native_microphone_smoke_starts() {
        let capture =
            NativeAudioCapture::start_default_microphone().expect("start default WASAPI mic");
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert_eq!(capture.format(), NativePcmFormat::stereo_i16_48khz());
    }
}
