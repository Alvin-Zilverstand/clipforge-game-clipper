use clipforge::audio::{list_ffmpeg_dshow_audio_inputs, AudioDeviceKind};
use clipforge::capture::{
    ffmpeg_is_available, ffmpeg_supports_filter, ffmpeg_supports_input_device,
    find_ffmpeg_executable, CaptureBackend, CaptureConfig, CaptureMethod, CaptureSource,
    EncoderPreference, FfmpegReplayCaptureBackend,
};
use clipforge::database::ClipDatabase;
use clipforge::game_detection::{default_profiles, detect_game};
use clipforge::game_watcher::running_processes;
use clipforge::gsi_config::write_gsi_config_templates;
use clipforge::integrations::{
    valve_gsi_raw_events, GameIntegration, IntegrationError, LeagueIntegration,
    LeagueLiveClientPoller, RawGameEvent, ValveGsiIntegration,
};
use clipforge::media::{
    collect_segments, concat_segments, generate_thumbnail, prune_old_segments, recent_segments,
    trim_clip as ffmpeg_trim_clip,
};
use clipforge::models::{Clip, ClipSource, GameEvent, RecordingStatus};
use clipforge::native_audio::{
    list_native_audio_devices, native_microphone_available, native_system_loopback_available,
    NativeAudioSource,
};
use clipforge::native_wgc::NativeWgcReplayCaptureBackend;
use clipforge::recorder::{RecorderAction, RecorderService};
use clipforge::settings::{load_or_create_settings, save_settings, AppSettings};
use clipforge::storage::{clip_path, is_inside_root, LibraryPaths};
use clipforge::upload::{
    CatboxUploader, CustomHttpUploader, LitterboxUploader, UploadMetadata, UploadResult, Uploader,
};
use serde::Serialize;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{Manager, State};

struct AppRuntime {
    library_root: PathBuf,
    recorder: Mutex<RecorderService>,
    database: Mutex<ClipDatabase>,
    capture: Mutex<Option<ActiveCapture>>,
    league_poller: Mutex<LeagueLiveClientPoller>,
    valve_events: Mutex<Vec<RawGameEvent>>,
    hotkey_shortcuts: Mutex<Option<HotkeyShortcuts>>,
    app_handle: Mutex<Option<tauri::AppHandle>>,
}

struct HotkeyShortcuts {
    clip_60: tauri_plugin_global_shortcut::Shortcut,
    clip_30: tauri_plugin_global_shortcut::Shortcut,
    toggle_recording: tauri_plugin_global_shortcut::Shortcut,
}

struct ActiveCapture {
    buffer_dir: PathBuf,
    session_output_path: PathBuf,
    segment_duration: Duration,
    started_at: SystemTime,
    backend: ActiveCaptureBackend,
}

enum ActiveCaptureBackend {
    Ffmpeg(FfmpegReplayCaptureBackend),
    NativeWgc(NativeWgcReplayCaptureBackend),
}

impl ActiveCaptureBackend {
    fn name(&self) -> &'static str {
        match self {
            Self::Ffmpeg(backend) => backend.name(),
            Self::NativeWgc(backend) => backend.name(),
        }
    }

    fn stop(&mut self) -> Result<(), clipforge::capture::CaptureError> {
        match self {
            Self::Ffmpeg(backend) => backend.stop(),
            Self::NativeWgc(backend) => backend.stop(),
        }
    }
}

#[derive(Debug, Serialize)]
struct DesktopStatus {
    recording_state: String,
    detected_game: Option<String>,
    replay_buffer_seconds: u64,
    system_audio_enabled: bool,
    mic_enabled: bool,
    mic_device: Option<String>,
    auto_record_enabled: bool,
    upload_enabled: bool,
    upload_provider: String,
    catbox_userhash: Option<String>,
    litterbox_expiry_hours: u8,
    custom_upload_endpoint: Option<String>,
    custom_upload_response_url_path: String,
    custom_upload_headers: Vec<String>,
    session_recording: bool,
    capture_active: bool,
    capture_backend: Option<String>,
    capture_path: Option<String>,
    clip_count: usize,
    library_root: String,
    ffmpeg_available: bool,
    ffmpeg_path: Option<String>,
    system_audio_available: bool,
    desktop_duplication_available: bool,
    auto_clip_enabled_events: Vec<AutoClipEventDto>,
    hotkey_clip_last_60s: String,
    hotkey_clip_last_30s: String,
    hotkey_toggle_session_recording: String,
    hotkey_screenshot: String,
}

#[derive(Debug, Serialize)]
struct ClipDto {
    id: String,
    title: String,
    game: String,
    event: String,
    source: String,
    duration: String,
    created_at: String,
    upload_state: String,
    path: String,
    thumbnail_path: Option<String>,
    tags: Vec<String>,
    color_class: String,
}

#[derive(Debug, Serialize)]
struct GsiConfigDto {
    game_id: String,
    path: String,
}

#[derive(Debug, Serialize)]
struct AudioDeviceDto {
    name: String,
    kind: String,
}

#[derive(Debug, Serialize)]
struct AutoClipEventDto {
    game_id: String,
    event_type: String,
    enabled: bool,
}

#[tauri::command]
fn get_status(runtime: State<'_, AppRuntime>) -> Result<DesktopStatus, String> {
    let recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let capture = runtime.capture.lock().map_err(|error| error.to_string())?;
    Ok(status_from_recorder(&recorder, capture.as_ref()))
}

#[tauri::command]
fn refresh_detected_game(runtime: State<'_, AppRuntime>) -> Result<DesktopStatus, String> {
    refresh_detected_game_inner(runtime.inner())
}

fn refresh_detected_game_inner(runtime: &AppRuntime) -> Result<DesktopStatus, String> {
    let detected = detect_game(&running_processes(), &default_profiles());
    let mut should_auto_start = false;
    let mut should_auto_stop = false;

    {
        let mut recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
        let capture = runtime.capture.lock().map_err(|error| error.to_string())?;

        match detected {
            Some(game) => {
                should_auto_start = game.process.process_name.len() > 0
                    && default_profiles()
                        .iter()
                        .any(|profile| profile.game_id == game.game_id && profile.auto_record)
                    && capture.is_none()
                    && !recorder.settings.privacy.desktop_capture_requires_confirmation;
                if recorder.state.detected_game_id.as_deref() != Some(&game.game_id) {
                    recorder.start_for_game(game.game_id, SystemTime::now());
                }
            }
            None if capture.is_none() => {
                let _ = recorder.stop();
            }
            None => {
                should_auto_stop = recorder.state.detected_game_id.as_deref() != Some("desktop");
            }
        }

        if !should_auto_start {
            if !should_auto_stop {
                return Ok(status_from_recorder(&recorder, capture.as_ref()));
            }
        }
    }

    if should_auto_stop {
        stop_capture_inner(runtime)
    } else {
        start_capture_inner(runtime)
    }
}

#[tauri::command]
fn list_clips(runtime: State<'_, AppRuntime>) -> Result<Vec<ClipDto>, String> {
    let recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    Ok(recorder.library.all().iter().map(clip_to_dto).collect())
}

#[tauri::command]
fn set_replay_buffer(seconds: u64, runtime: State<'_, AppRuntime>) -> Result<DesktopStatus, String> {
    let mut recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let capture = runtime.capture.lock().map_err(|error| error.to_string())?;
    recorder.settings.replay_buffer = Duration::from_secs(seconds);
    recorder.settings.clamp_replay_buffer();
    save_settings(&runtime.library_root, &recorder.settings)
        .map_err(|error| format!("Could not save settings: {error}"))?;
    Ok(status_from_recorder(&recorder, capture.as_ref()))
}

#[tauri::command]
fn set_mic_enabled(enabled: bool, runtime: State<'_, AppRuntime>) -> Result<DesktopStatus, String> {
    let mut recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let capture = runtime.capture.lock().map_err(|error| error.to_string())?;
    recorder.settings.privacy.mic_enabled = enabled;
    save_settings(&runtime.library_root, &recorder.settings)
        .map_err(|error| format!("Could not save settings: {error}"))?;
    Ok(status_from_recorder(&recorder, capture.as_ref()))
}

#[tauri::command]
fn set_system_audio_enabled(
    enabled: bool,
    runtime: State<'_, AppRuntime>,
) -> Result<DesktopStatus, String> {
    let mut recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let capture = runtime.capture.lock().map_err(|error| error.to_string())?;
    recorder.settings.privacy.system_audio_enabled = enabled;
    save_settings(&runtime.library_root, &recorder.settings)
        .map_err(|error| format!("Could not save settings: {error}"))?;
    Ok(status_from_recorder(&recorder, capture.as_ref()))
}

#[tauri::command]
fn list_audio_devices() -> Result<Vec<AudioDeviceDto>, String> {
    let mut devices = list_native_audio_devices()
        .into_iter()
        .map(|device| AudioDeviceDto {
            name: device.name,
            kind: match device.source {
                NativeAudioSource::Microphone => "input".to_string(),
                NativeAudioSource::SystemLoopback => "system_loopback".to_string(),
            },
        })
        .collect::<Vec<_>>();

    if let Some(ffmpeg) = find_ffmpeg_executable() {
        for device in list_ffmpeg_dshow_audio_inputs(&ffmpeg) {
            if !devices
                .iter()
                .any(|existing| existing.name == device.name && existing.kind == "input")
            {
                devices.push(AudioDeviceDto {
                    name: device.name,
                    kind: match device.kind {
                        AudioDeviceKind::Input => "input".to_string(),
                        AudioDeviceKind::SystemLoopback => "system_loopback".to_string(),
                    },
                });
            }
        }
    }

    Ok(devices)
}

#[tauri::command]
fn set_mic_device(
    device: Option<String>,
    runtime: State<'_, AppRuntime>,
) -> Result<DesktopStatus, String> {
    let mut recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let capture = runtime.capture.lock().map_err(|error| error.to_string())?;
    recorder.settings.privacy.mic_device = device.filter(|value| !value.trim().is_empty());
    save_settings(&runtime.library_root, &recorder.settings)
        .map_err(|error| format!("Could not save settings: {error}"))?;
    Ok(status_from_recorder(&recorder, capture.as_ref()))
}

#[tauri::command]
fn set_auto_record_enabled(
    enabled: bool,
    runtime: State<'_, AppRuntime>,
) -> Result<DesktopStatus, String> {
    let mut recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let capture = runtime.capture.lock().map_err(|error| error.to_string())?;
    recorder.settings.privacy.desktop_capture_requires_confirmation = !enabled;
    save_settings(&runtime.library_root, &recorder.settings)
        .map_err(|error| format!("Could not save settings: {error}"))?;
    Ok(status_from_recorder(&recorder, capture.as_ref()))
}

#[tauri::command]
fn set_upload_settings(
    auto_upload_enabled: bool,
    provider: String,
    catbox_userhash: Option<String>,
    litterbox_expiry_hours: u8,
    custom_endpoint: Option<String>,
    custom_response_url_path: String,
    custom_headers: Vec<String>,
    runtime: State<'_, AppRuntime>,
) -> Result<DesktopStatus, String> {
    let mut recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let capture = runtime.capture.lock().map_err(|error| error.to_string())?;
    recorder.settings.upload.auto_upload_enabled = auto_upload_enabled;
    recorder.settings.upload.provider = clean_provider(&provider);
    recorder.settings.upload.catbox_userhash = clean_optional(catbox_userhash);
    recorder.settings.upload.litterbox_expiry_hours = match litterbox_expiry_hours {
        1 | 12 | 24 | 72 => litterbox_expiry_hours,
        _ => 24,
    };
    recorder.settings.upload.custom_endpoint = clean_optional(custom_endpoint);
    recorder.settings.upload.custom_response_url_path = if custom_response_url_path.trim().is_empty() {
        "url".to_string()
    } else {
        custom_response_url_path.trim().to_string()
    };
    recorder.settings.upload.custom_headers = parse_header_lines(&custom_headers.join("\n"));
    save_settings(&runtime.library_root, &recorder.settings)
        .map_err(|error| format!("Could not save upload settings: {error}"))?;
    Ok(status_from_recorder(&recorder, capture.as_ref()))
}

#[tauri::command]
fn set_auto_clip_event_enabled(
    game_id: String,
    event_type: String,
    enabled: bool,
    runtime: State<'_, AppRuntime>,
) -> Result<DesktopStatus, String> {
    let mut recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let capture = runtime.capture.lock().map_err(|error| error.to_string())?;
    let defaults = default_auto_clip_events_for_game(&game_id);
    let events = recorder
        .settings
        .auto_clip
        .enabled_events_by_game
        .entry(game_id)
        .or_insert_with(|| defaults.iter().map(|event| (*event).to_string()).collect());
    if enabled {
        events.insert(event_type);
    } else {
        events.remove(&event_type);
    }
    save_settings(&runtime.library_root, &recorder.settings)
        .map_err(|error| format!("Could not save auto-clip setting: {error}"))?;
    Ok(status_from_recorder(&recorder, capture.as_ref()))
}

#[tauri::command]
fn write_gsi_configs(runtime: State<'_, AppRuntime>) -> Result<Vec<GsiConfigDto>, String> {
    write_gsi_config_templates(&runtime.library_root)
        .map_err(|error| format!("Could not write GSI configs: {error}"))
        .map(|configs| {
            configs
                .into_iter()
                .map(|config| GsiConfigDto {
                    game_id: config.game_id,
                    path: config.path.display().to_string(),
                })
                .collect()
        })
}

#[tauri::command]
fn poll_auto_clip_events(runtime: State<'_, AppRuntime>) -> Result<Vec<ClipDto>, String> {
    poll_auto_clip_events_inner(runtime.inner())
}

fn poll_auto_clip_events_inner(runtime: &AppRuntime) -> Result<Vec<ClipDto>, String> {
    let (session_id, game_id, session_started_at) = {
        let recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
        let capture = runtime.capture.lock().map_err(|error| error.to_string())?;
        let Some(active) = capture.as_ref() else {
            return Ok(Vec::new());
        };
        let Some(session_id) = recorder.state.active_session_id.clone() else {
            return Ok(Vec::new());
        };
        let Some(game_id) = recorder.state.detected_game_id.clone() else {
            return Ok(Vec::new());
        };
        (session_id, game_id, active.started_at)
    };

    let mut created = Vec::new();
    if game_id == "league-of-legends" {
        let raw_events = {
            let mut poller = runtime
                .league_poller
                .lock()
                .map_err(|error| error.to_string())?;
            match poller.poll_new_events(session_started_at) {
                Ok(events) => events,
                Err(IntegrationError::Http(_)) => Vec::new(),
                Err(error) => return Err(error.to_string()),
            }
        };

        let integration = LeagueIntegration;
        for raw_event in raw_events {
                if let Some(event) = integration.normalize_event(&raw_event, &session_id) {
                if let Some(clip) = handle_auto_event_inner(event, runtime)? {
                    created.push(clip);
                }
            }
        }
    }

    if game_id == "counter-strike-2" || game_id == "dota-2" {
        let raw_events = {
            let mut queue = runtime
                .valve_events
                .lock()
                .map_err(|error| error.to_string())?;
            std::mem::take(&mut *queue)
        };
        let integration = if game_id == "counter-strike-2" {
            ValveGsiIntegration::counter_strike_2()
        } else {
            ValveGsiIntegration::dota_2()
        };
        for raw_event in raw_events {
            if let Some(event) = integration.normalize_event(&raw_event, &session_id) {
                if let Some(clip) = handle_auto_event_inner(event, runtime)? {
                    created.push(clip);
                }
            }
        }
    }
    Ok(created)
}

#[tauri::command]
fn save_manual_clip(seconds: u64, runtime: State<'_, AppRuntime>) -> Result<ClipDto, String> {
    save_manual_clip_inner(seconds, runtime.inner())
}

fn save_manual_clip_inner(seconds: u64, runtime: &AppRuntime) -> Result<ClipDto, String> {
    let mut recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let capture = runtime.capture.lock().map_err(|error| error.to_string())?;
    let Some(active) = capture.as_ref() else {
        return Err("Start recording before saving a replay clip.".to_string());
    };
    let ffmpeg = find_ffmpeg_executable()
        .ok_or_else(|| "FFmpeg was not found on PATH or in the bundled sidecar.".to_string())?;
    let now = SystemTime::now();
    let segment_paths = recent_segments(
        &active.buffer_dir,
        Duration::from_secs(seconds),
        active.segment_duration,
        now,
    )
    .map_err(|error| error.to_string())?;
    if segment_paths.is_empty() {
        return Err("The replay buffer has not produced any segments yet.".to_string());
    }

    let action = recorder
        .save_manual_clip(Duration::from_secs(seconds), now)
        .ok_or_else(|| "No active recording session is available.".to_string())?;

    let RecorderAction::CreatedClip { clip_id, .. } = action else {
        return Err("Recorder did not create a clip.".to_string());
    };

    let clip = recorder
        .library
        .all()
        .iter()
        .find(|clip| clip.id == clip_id)
        .cloned()
        .ok_or_else(|| "Created clip was not found in the library.".to_string())?;

    if let Err(error) = concat_segments(&ffmpeg, &segment_paths, &clip.path) {
        recorder.library.remove_clip(&clip.id);
        if let Ok(database) = runtime.database.lock() {
            let _ = database.delete_clip(&clip.id);
        }
        return Err(error.to_string());
    }
    if let Some(thumbnail_path) = &clip.thumbnail_path {
        let _ = generate_thumbnail(&ffmpeg, &clip.path, thumbnail_path);
    }
    let _ = prune_old_segments(
        &active.buffer_dir,
        recorder.settings.replay_buffer,
        active.segment_duration,
    );
    let mut clip = clip;
    maybe_auto_upload_clip(runtime, &mut recorder, &mut clip);
    recorder.library.add_clip(clip.clone());
    persist_clip(runtime, &clip)?;
    save_manifest(&recorder)?;
    Ok(clip_to_dto(&clip))
}

fn handle_auto_event_inner(
    event: GameEvent,
    runtime: &AppRuntime,
) -> Result<Option<ClipDto>, String> {
    let mut recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let capture = runtime.capture.lock().map_err(|error| error.to_string())?;
    let Some(active) = capture.as_ref() else {
        return Ok(None);
    };
    let ffmpeg = find_ffmpeg_executable()
        .ok_or_else(|| "FFmpeg was not found on PATH or in the bundled sidecar.".to_string())?;
    let clip_window = recorder.settings.auto_clip.pre_roll + recorder.settings.auto_clip.post_roll;
    let segment_paths = recent_segments(
        &active.buffer_dir,
        clip_window,
        active.segment_duration,
        SystemTime::now(),
    )
    .map_err(|error| error.to_string())?;
    if segment_paths.is_empty() {
        return Ok(None);
    }

    if !auto_clip_event_enabled(&recorder.settings, &event.game_id, &event.event_type.to_string()) {
        return Ok(None);
    }

    let action = recorder.handle_game_event(event);
    let RecorderAction::CreatedClip { clip_id, .. } = action else {
        return Ok(None);
    };
    let clip = recorder
        .library
        .all()
        .iter()
        .find(|clip| clip.id == clip_id)
        .cloned()
        .ok_or_else(|| "Created auto clip was not found in the library.".to_string())?;

    if let Err(error) = concat_segments(&ffmpeg, &segment_paths, &clip.path) {
        recorder.library.remove_clip(&clip.id);
        let _ = runtime
            .database
            .lock()
            .map(|database| database.delete_clip(&clip.id));
        return Err(error.to_string());
    }
    if let Some(thumbnail_path) = &clip.thumbnail_path {
        let _ = generate_thumbnail(&ffmpeg, &clip.path, thumbnail_path);
    }
    let mut clip = clip;
    maybe_auto_upload_clip(runtime, &mut recorder, &mut clip);
    recorder.library.add_clip(clip.clone());
    persist_clip(runtime, &clip)?;
    save_manifest(&recorder)?;
    Ok(Some(clip_to_dto(&clip)))
}

#[tauri::command]
fn start_capture(runtime: State<'_, AppRuntime>) -> Result<DesktopStatus, String> {
    start_capture_inner(runtime.inner())
}

fn start_capture_inner(runtime: &AppRuntime) -> Result<DesktopStatus, String> {
    let mut recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let mut capture = runtime.capture.lock().map_err(|error| error.to_string())?;

    if capture.is_some() {
        return Ok(status_from_recorder(&recorder, capture.as_ref()));
    }

    let ffmpeg = find_ffmpeg_executable()
        .ok_or_else(|| "FFmpeg was not found on PATH or in the bundled sidecar.".to_string())?;
    let now = SystemTime::now();
    if recorder.state.active_session_id.is_none() {
        let detected = detect_game(&running_processes(), &default_profiles());
        recorder.start_for_game(
            detected
                .as_ref()
                .map(|game| game.game_id.as_str())
                .unwrap_or("desktop"),
            now,
        );
    }
    recorder.toggle_session_recording();

    let session_id = recorder
        .state
        .active_session_id
        .clone()
        .ok_or_else(|| "Recorder session did not start.".to_string())?;
    let session_dir = recorder.paths.sessions_root.join(&session_id);
    let buffer_dir = recorder.paths.buffer_root.join(&session_id);
    fs::create_dir_all(&session_dir).map_err(|error| error.to_string())?;
    fs::create_dir_all(&buffer_dir).map_err(|error| error.to_string())?;
    let session_output_path = session_dir.join(format!("session-{}.mp4", unix_millis(now)));
    let segment_duration = Duration::from_secs(5);
    let segment_pattern = buffer_dir.join("segment-%05d.mp4");

    let native_system_audio_available = native_system_loopback_available();
    let native_mic_available = native_microphone_available();
    let ffmpeg_system_audio_available = ffmpeg_supports_input_device(&ffmpeg, "wasapi");
    let system_audio_enabled = recorder.settings.privacy.system_audio_enabled
        && (native_system_audio_available || ffmpeg_system_audio_available);
    let method = if ffmpeg_supports_filter(&ffmpeg, "ddagrab") {
        CaptureMethod::DesktopDuplication
    } else {
        CaptureMethod::GdiGrab
    };
    let source = detect_game(&running_processes(), &default_profiles())
        .map(|game| CaptureSource::GameWindow {
            process_name: game.process.process_name,
            window_title: game
                .process
                .window_title
                .unwrap_or(game.display_name),
        })
        .unwrap_or(CaptureSource::Desktop);
    let config = CaptureConfig {
        source,
        method,
        width: recorder.settings.quality.width,
        height: recorder.settings.quality.height,
        fps: recorder.settings.quality.fps,
        bitrate_kbps: recorder.settings.quality.bitrate_kbps,
        encoder: EncoderPreference::HardwareH264,
        system_audio_enabled,
        mic_enabled: recorder.settings.privacy.mic_enabled,
        system_audio_device: recorder.settings.privacy.system_audio_device.clone(),
        mic_device: recorder.settings.privacy.mic_device.clone(),
        separate_audio_tracks: recorder.settings.privacy.separate_audio_tracks,
    };
    let backend = start_best_capture_backend(
        &ffmpeg,
        &segment_pattern,
        segment_duration,
        config,
        native_system_audio_available,
        native_mic_available,
        ffmpeg_system_audio_available,
    )?;
    *capture = Some(ActiveCapture {
        buffer_dir,
        session_output_path,
        segment_duration,
        started_at: now,
        backend,
    });

    Ok(status_from_recorder(&recorder, capture.as_ref()))
}

#[tauri::command]
fn stop_capture(runtime: State<'_, AppRuntime>) -> Result<DesktopStatus, String> {
    stop_capture_inner(runtime.inner())
}

fn stop_capture_inner(runtime: &AppRuntime) -> Result<DesktopStatus, String> {
    let mut recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let mut capture = runtime.capture.lock().map_err(|error| error.to_string())?;

    if let Some(mut active) = capture.take() {
        active.backend.stop().map_err(|error| error.to_string())?;
        let ffmpeg = find_ffmpeg_executable()
            .ok_or_else(|| "FFmpeg was not found on PATH or in the bundled sidecar.".to_string())?;
        let segments = collect_segments(&active.buffer_dir)
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|segment| segment.path)
            .collect::<Vec<_>>();
        if !segments.is_empty() {
            concat_segments(&ffmpeg, &segments, &active.session_output_path)
                .map_err(|error| error.to_string())?;
            let clip = add_session_clip(
                &mut recorder,
                active.session_output_path,
                active.started_at,
                SystemTime::now(),
            )?;
            if let Some(thumbnail_path) = &clip.thumbnail_path {
                let _ = generate_thumbnail(&ffmpeg, &clip.path, thumbnail_path);
            }
            persist_clip(runtime, &clip)?;
        }
        if is_inside_root(&recorder.paths.buffer_root, &active.buffer_dir) {
            let _ = fs::remove_dir_all(&active.buffer_dir);
        }
    }

    if matches!(recorder.state.status, RecordingStatus::RecordingSession) {
        recorder.toggle_session_recording();
    }
    Ok(status_from_recorder(&recorder, capture.as_ref()))
}

#[tauri::command]
fn delete_clip(clip_id: String, runtime: State<'_, AppRuntime>) -> Result<Vec<ClipDto>, String> {
    let mut recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let clip = recorder
        .library
        .remove_clip(&clip_id)
        .ok_or_else(|| format!("Clip {clip_id} was not found."))?;

    let can_delete_file = is_inside_root(&recorder.paths.clip_root, &clip.path)
        || is_inside_root(&recorder.paths.sessions_root, &clip.path);
    if can_delete_file && clip.path.exists() {
        fs::remove_file(&clip.path).map_err(|error| error.to_string())?;
    }
    if let Some(thumbnail_path) = &clip.thumbnail_path {
        if is_inside_root(&recorder.paths.thumbs_root, thumbnail_path) && thumbnail_path.exists() {
            fs::remove_file(thumbnail_path).map_err(|error| error.to_string())?;
        }
    }

    delete_clip_record(runtime.inner(), &clip_id)?;
    save_manifest(&recorder)?;
    Ok(recorder.library.all().iter().map(clip_to_dto).collect())
}

#[tauri::command]
fn reveal_clip(clip_id: String, runtime: State<'_, AppRuntime>) -> Result<(), String> {
    let recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let clip = recorder
        .library
        .all()
        .iter()
        .find(|clip| clip.id == clip_id)
        .ok_or_else(|| format!("Clip {clip_id} was not found."))?;

    if !clip.path.exists() {
        return Err(format!("Clip file does not exist: {}", clip.path.display()));
    }

    Command::new("explorer")
        .arg(format!("/select,{}", clip.path.display()))
        .spawn()
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
fn trim_clip(
    clip_id: String,
    start_seconds: f64,
    end_seconds: f64,
    runtime: State<'_, AppRuntime>,
) -> Result<ClipDto, String> {
    if !(end_seconds > start_seconds && start_seconds >= 0.0) {
        return Err("Trim end must be after trim start.".to_string());
    }

    let mut recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let source_clip = recorder
        .library
        .all()
        .iter()
        .find(|clip| clip.id == clip_id)
        .cloned()
        .ok_or_else(|| format!("Clip {clip_id} was not found."))?;

    if !source_clip.path.exists() {
        return Err(format!(
            "Clip file does not exist: {}",
            source_clip.path.display()
        ));
    }

    let ffmpeg = find_ffmpeg_executable()
        .ok_or_else(|| "FFmpeg was not found on PATH or in the bundled sidecar.".to_string())?;
    let now = SystemTime::now();
    let trimmed_id = format!("trim-{}", unix_millis(now));
    let output_path = clip_path(&recorder.paths, &source_clip.game_id, now, &trimmed_id);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    ffmpeg_trim_clip(
        &ffmpeg,
        &source_clip.path,
        &output_path,
        start_seconds,
        end_seconds,
    )
    .map_err(|error| error.to_string())?;

    let clip = Clip {
        id: trimmed_id,
        title: source_clip
            .title
            .as_ref()
            .map(|title| format!("{title} (trimmed)")),
        session_id: source_clip.session_id.clone(),
        game_id: source_clip.game_id.clone(),
        path: output_path,
        thumbnail_path: Some(recorder.paths.thumbs_root.join(format!(
            "trim-{}.jpg",
            unix_millis(now)
        ))),
        created_at: now,
        duration: Duration::from_secs_f64(end_seconds - start_seconds),
        source: ClipSource::Imported,
        event_type: source_clip.event_type.clone(),
        tags: {
            let mut tags = source_clip.tags.clone();
            if !tags.iter().any(|tag| tag == "trimmed") {
                tags.push("trimmed".to_string());
            }
            tags
        },
        upload_url: None,
        upload_provider: None,
    };
    if let Some(thumbnail_path) = &clip.thumbnail_path {
        let _ = generate_thumbnail(&ffmpeg, &clip.path, thumbnail_path);
    }
    recorder.library.add_clip(clip.clone());
    persist_clip(runtime.inner(), &clip)?;
    save_manifest(&recorder)?;
    Ok(clip_to_dto(&clip))
}

#[tauri::command]
fn upload_clip(
    clip_id: String,
    provider: Option<String>,
    custom_endpoint: Option<String>,
    custom_response_url_path: Option<String>,
    runtime: State<'_, AppRuntime>,
) -> Result<ClipDto, String> {
    let mut recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let clip = recorder
        .library
        .all()
        .iter()
        .find(|clip| clip.id == clip_id)
        .cloned()
        .ok_or_else(|| format!("Clip {clip_id} was not found."))?;
    if !clip.path.exists() {
        return Err(format!("Clip file does not exist: {}", clip.path.display()));
    }
    let provider_name = provider.unwrap_or_else(|| recorder.settings.upload.provider.clone());
    let history_provider = clean_provider(&provider_name);
    let result = upload_clip_with_settings(
        &clip,
        &recorder.settings,
        Some(provider_name),
        custom_endpoint,
        custom_response_url_path,
    );
    let result = match result {
        Ok(result) => {
            record_upload_history(runtime.inner(), &clip.id, &result.provider.to_string(), "uploaded", Some(&result.url), None);
            result
        }
        Err(error) => {
            record_upload_history(
                runtime.inner(),
                &clip.id,
                &history_provider,
                "failed",
                None,
                Some(&error),
            );
            return Err(error);
        }
    };
    if !recorder
        .library
        .update_upload(&clip_id, result.provider, result.url)
    {
        return Err(format!("Clip {clip_id} was not found."));
    }
    let clip = recorder
        .library
        .all()
        .iter()
        .find(|clip| clip.id == clip_id)
        .cloned()
        .ok_or_else(|| format!("Clip {clip_id} was not found."))?;
    persist_clip(runtime.inner(), &clip)?;
    save_manifest(&recorder)?;
    Ok(clip_to_dto(&clip))
}

#[tauri::command]
fn update_clip_metadata(
    clip_id: String,
    title: Option<String>,
    tags: Vec<String>,
    runtime: State<'_, AppRuntime>,
) -> Result<ClipDto, String> {
    let mut recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let clip = recorder
        .library
        .all_mut()
        .iter_mut()
        .find(|clip| clip.id == clip_id)
        .ok_or_else(|| format!("Clip {clip_id} was not found."))?;
    clip.title = clean_optional(title);
    clip.tags = tags
        .into_iter()
        .map(|tag| tag.trim().to_string())
        .filter(|tag| !tag.is_empty())
        .collect();
    let clip = clip.clone();
    persist_clip(runtime.inner(), &clip)?;
    save_manifest(&recorder)?;
    Ok(clip_to_dto(&clip))
}

#[tauri::command]
fn export_clip_copy(clip_id: String, runtime: State<'_, AppRuntime>) -> Result<String, String> {
    let recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let clip = recorder
        .library
        .all()
        .iter()
        .find(|clip| clip.id == clip_id)
        .ok_or_else(|| format!("Clip {clip_id} was not found."))?;
    if !clip.path.exists() {
        return Err(format!("Clip file does not exist: {}", clip.path.display()));
    }
    let export_dir = runtime.library_root.join("exports");
    fs::create_dir_all(&export_dir).map_err(|error| error.to_string())?;
    let title = clip
        .title
        .as_deref()
        .unwrap_or(&clip.id)
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' { ch } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let file_name = if title.is_empty() {
        format!("{}.mp4", clip.id)
    } else {
        format!("{title}.mp4")
    };
    let output = export_dir.join(file_name);
    fs::copy(&clip.path, &output).map_err(|error| error.to_string())?;
    Ok(output.display().to_string())
}

fn upload_clip_with_settings(
    clip: &Clip,
    settings: &AppSettings,
    provider_override: Option<String>,
    custom_endpoint_override: Option<String>,
    custom_response_path_override: Option<String>,
) -> Result<UploadResult, String> {
    let provider = clean_provider(
        provider_override
            .as_deref()
            .unwrap_or(&settings.upload.provider),
    );
    let metadata = UploadMetadata {
        clip_id: clip.id.clone(),
        game_id: clip.game_id.clone(),
        title: clip.title.clone().unwrap_or_else(|| clip.id.clone()),
    };

    match provider.as_str() {
        "catbox" => CatboxUploader {
            userhash: settings.upload.catbox_userhash.clone(),
        }
        .upload(&clip.path, &metadata),
        "litterbox" => LitterboxUploader {
            expiry_hours: settings.upload.litterbox_expiry_hours,
        }
        .upload(&clip.path, &metadata),
        "custom_http" => CustomHttpUploader {
            endpoint: custom_endpoint_override
                .and_then(|value| clean_optional(Some(value)))
                .or_else(|| settings.upload.custom_endpoint.clone())
                .unwrap_or_default(),
            method: "POST".to_string(),
            multipart_field: "file".to_string(),
            response_url_path: custom_response_path_override
                .and_then(|value| clean_optional(Some(value)))
                .unwrap_or_else(|| settings.upload.custom_response_url_path.clone()),
            headers: settings.upload.custom_headers.clone(),
        }
        .upload(&clip.path, &metadata),
        "lustful" => clipforge::upload::LustfulUploader::default().upload(&clip.path, &metadata),
        unknown => {
            return Err(format!("Unsupported upload provider: {unknown}"));
        }
    }
    .map_err(|error| error.to_string())
}

fn maybe_auto_upload_clip(runtime: &AppRuntime, recorder: &mut RecorderService, clip: &mut Clip) {
    if !recorder.settings.upload.auto_upload_enabled {
        return;
    }
    let provider = recorder.settings.upload.provider.clone();
    match upload_clip_with_settings(clip, &recorder.settings, None, None, None) {
        Ok(result) => {
            record_upload_history(
                runtime,
                &clip.id,
                &result.provider.to_string(),
                "uploaded",
                Some(&result.url),
                None,
            );
            clip.upload_provider = Some(result.provider);
            clip.upload_url = Some(result.url);
        }
        Err(error) => {
            record_upload_history(runtime, &clip.id, &provider, "failed", None, Some(&error));
            clip.upload_url = Some(format!("clipforge://upload-failed/{provider}"));
        }
    }
}

fn record_upload_history(
    runtime: &AppRuntime,
    clip_id: &str,
    provider: &str,
    status: &str,
    url: Option<&str>,
    error: Option<&str>,
) {
    if let Ok(database) = runtime.database.lock() {
        let _ = database.insert_upload_history(clip_id, provider, status, url, error, SystemTime::now());
    }
}

fn clean_provider(provider: &str) -> String {
    match provider.trim() {
        "catbox" | "litterbox" | "custom_http" | "lustful" => provider.trim().to_string(),
        _ => "catbox".to_string(),
    }
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_header_lines(value: &str) -> Vec<(String, String)> {
    value
        .lines()
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            let name = name.trim();
            let value = value.trim();
            if name.is_empty() || value.is_empty() {
                None
            } else {
                Some((name.to_string(), value.to_string()))
            }
        })
        .collect()
}

fn default_auto_clip_events_for_game(game_id: &str) -> Vec<&'static str> {
    match game_id {
        "league-of-legends" => vec!["kill", "death", "assist", "objective", "match_win"],
        "counter-strike-2" => vec!["kill", "death", "assist", "round_win", "match_win", "multi_kill"],
        "dota-2" => vec!["kill", "death", "assist", "objective", "match_win", "multi_kill"],
        _ => Vec::new(),
    }
}

fn auto_clip_event_enabled(settings: &AppSettings, game_id: &str, event_type: &str) -> bool {
    if !settings.auto_clip.enabled {
        return false;
    }
    settings
        .auto_clip
        .enabled_events_by_game
        .get(game_id)
        .map(|events| events.contains(event_type))
        .unwrap_or_else(|| {
            default_auto_clip_events_for_game(game_id)
                .iter()
                .any(|event| event == &event_type)
        })
}

fn auto_clip_event_dtos(settings: &AppSettings) -> Vec<AutoClipEventDto> {
    let games = [
        ("league-of-legends", default_auto_clip_events_for_game("league-of-legends")),
        ("counter-strike-2", default_auto_clip_events_for_game("counter-strike-2")),
        ("dota-2", default_auto_clip_events_for_game("dota-2")),
    ];
    games
        .into_iter()
        .flat_map(|(game_id, events)| {
            events.into_iter().map(move |event_type| AutoClipEventDto {
                game_id: game_id.to_string(),
                event_type: event_type.to_string(),
                enabled: auto_clip_event_enabled(settings, game_id, event_type),
            })
        })
        .collect()
}

#[cfg(desktop)]
fn parse_accelerator(accel: &str) -> Result<(Option<tauri_plugin_global_shortcut::Modifiers>, tauri_plugin_global_shortcut::Code), String> {
    use tauri_plugin_global_shortcut::{Code, Modifiers};
    
    let parts: Vec<&str> = accel.split('+').collect();
    let key_str = parts.last().ok_or("Empty accelerator")?.trim();
    
    let code = match key_str.to_uppercase().as_str() {
        "F1" => Code::F1, "F2" => Code::F2, "F3" => Code::F3, "F4" => Code::F4,
        "F5" => Code::F5, "F6" => Code::F6, "F7" => Code::F7, "F8" => Code::F8,
        "F9" => Code::F9, "F10" => Code::F10, "F11" => Code::F11, "F12" => Code::F12,
        "A" => Code::KeyA, "B" => Code::KeyB, "C" => Code::KeyC, "D" => Code::KeyD,
        "E" => Code::KeyE, "F" => Code::KeyF, "G" => Code::KeyG, "H" => Code::KeyH,
        "I" => Code::KeyI, "J" => Code::KeyJ, "K" => Code::KeyK, "L" => Code::KeyL,
        "M" => Code::KeyM, "N" => Code::KeyN, "O" => Code::KeyO, "P" => Code::KeyP,
        "Q" => Code::KeyQ, "R" => Code::KeyR, "S" => Code::KeyS, "T" => Code::KeyT,
        "U" => Code::KeyU, "V" => Code::KeyV, "W" => Code::KeyW, "X" => Code::KeyX,
        "Y" => Code::KeyY, "Z" => Code::KeyZ,
        "0" => Code::Digit0, "1" => Code::Digit1, "2" => Code::Digit2, "3" => Code::Digit3,
        "4" => Code::Digit4, "5" => Code::Digit5, "6" => Code::Digit6, "7" => Code::Digit7,
        "8" => Code::Digit8, "9" => Code::Digit9,
        "SPACE" => Code::Space, "ENTER" => Code::Enter, "ESCAPE" => Code::Escape,
        "TAB" => Code::Tab, "BACKSPACE" => Code::Backspace, "DELETE" => Code::Delete,
        "UP" => Code::ArrowUp, "DOWN" => Code::ArrowDown, "LEFT" => Code::ArrowLeft, "RIGHT" => Code::ArrowRight,
        "HOME" => Code::Home, "END" => Code::End, "PAGEUP" => Code::PageUp, "PAGEDOWN" => Code::PageDown,
        "INSERT" => Code::Insert, "NUMLOCK" => Code::NumLock, "SCROLLLOCK" => Code::ScrollLock, "PAUSE" => Code::Pause,
        _ => return Err(format!("Unsupported key: {}", key_str)),
    };
    
    let mut modifiers = Modifiers::empty();
    for modifier in &parts[..parts.len().saturating_sub(1)] {
        match modifier.trim().to_uppercase().as_str() {
            "CTRL" | "CONTROL" => modifiers |= Modifiers::CONTROL,
            "SHIFT" => modifiers |= Modifiers::SHIFT,
            "ALT" => modifiers |= Modifiers::ALT,
            "META" | "SUPER" | "COMMAND" => modifiers |= Modifiers::SUPER,
            _ => return Err(format!("Unsupported modifier: {}", modifier)),
        }
    }
    
    Ok((if modifiers.is_empty() { None } else { Some(modifiers) }, code))
}

#[cfg(desktop)]
fn register_hotkeys(
    app: &tauri::AppHandle,
    runtime: &AppRuntime,
) -> Result<(), String> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
    
    let settings = {
        let recorder = runtime.recorder.lock().map_err(|e| e.to_string())?;
        recorder.settings.hotkeys.clone()
    };
    
    // Unregister existing shortcuts
    if let Ok(mut shortcuts_guard) = runtime.hotkey_shortcuts.lock() {
        if let Some(existing) = shortcuts_guard.take() {
            let _ = app.global_shortcut().unregister(existing.clip_60);
            let _ = app.global_shortcut().unregister(existing.clip_30);
            let _ = app.global_shortcut().unregister(existing.toggle_recording);
        }
    }
    
    // Parse new shortcuts
    let (clip_60_mods, clip_60_code) = parse_accelerator(&settings.clip_last_60s)?;
    let (clip_30_mods, clip_30_code) = parse_accelerator(&settings.clip_last_30s)?;
    let (toggle_mods, toggle_code) = parse_accelerator(&settings.toggle_session_recording)?;
    
    let clip_60 = Shortcut::new(clip_60_mods, clip_60_code);
    let clip_30 = Shortcut::new(clip_30_mods, clip_30_code);
    let toggle_recording = Shortcut::new(toggle_mods, toggle_code);
    
    let handler_clip_60 = clip_60.clone();
    let handler_clip_30 = clip_30.clone();
    let handler_toggle_recording = toggle_recording.clone();
    
    app.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(move |app, shortcut, event| {
                if event.state() != ShortcutState::Pressed {
                    return;
                }
                let runtime = app.state::<AppRuntime>();
                if shortcut == &handler_clip_60 {
                    let _ = save_manual_clip_inner(60, runtime.inner());
                } else if shortcut == &handler_clip_30 {
                    let _ = save_manual_clip_inner(30, runtime.inner());
                } else if shortcut == &handler_toggle_recording {
                    let capture_active = runtime
                        .capture
                        .lock()
                        .map(|capture| capture.is_some())
                        .unwrap_or(false);
                    if capture_active {
                        let _ = stop_capture_inner(runtime.inner());
                    } else {
                        let _ = start_capture_inner(runtime.inner());
                    }
                }
            })
            .build(),
    ).map_err(|e| e.to_string())?;
    
    app.global_shortcut().register(clip_60.clone()).map_err(|e| e.to_string())?;
    app.global_shortcut().register(clip_30.clone()).map_err(|e| e.to_string())?;
    app.global_shortcut().register(toggle_recording.clone()).map_err(|e| e.to_string())?;
    
    if let Ok(mut shortcuts_guard) = runtime.hotkey_shortcuts.lock() {
        *shortcuts_guard = Some(HotkeyShortcuts {
            clip_60,
            clip_30,
            toggle_recording,
        });
    }
    
    Ok(())
}

#[tauri::command]
fn set_hotkeys(
    clip_last_60s: String,
    clip_last_30s: String,
    toggle_session_recording: String,
    screenshot: String,
    runtime: State<'_, AppRuntime>,
) -> Result<DesktopStatus, String> {
    let mut recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let capture = runtime.capture.lock().map_err(|error| error.to_string())?;
    
    // Validate hotkeys
    #[cfg(desktop)]
    {
        parse_accelerator(&clip_last_60s).map_err(|e| format!("Invalid clip_last_60s: {}", e))?;
        parse_accelerator(&clip_last_30s).map_err(|e| format!("Invalid clip_last_30s: {}", e))?;
        parse_accelerator(&toggle_session_recording).map_err(|e| format!("Invalid toggle_session_recording: {}", e))?;
        parse_accelerator(&screenshot).map_err(|e| format!("Invalid screenshot: {}", e))?;
    }
    
    recorder.settings.hotkeys.clip_last_60s = clip_last_60s;
    recorder.settings.hotkeys.clip_last_30s = clip_last_30s;
    recorder.settings.hotkeys.toggle_session_recording = toggle_session_recording;
    recorder.settings.hotkeys.screenshot = screenshot;
    
    save_settings(&runtime.library_root, &recorder.settings)
        .map_err(|error| format!("Could not save settings: {error}"))?;
    
    // Re-register hotkeys
    #[cfg(desktop)]
    {
        let app_handle = {
            let runtime_guard = runtime.app_handle.lock().map_err(|e| e.to_string())?;
            runtime_guard.clone().ok_or("App handle not available")?
        };
        register_hotkeys(&app_handle, runtime.inner())?;
    }
    
    Ok(status_from_recorder(&recorder, capture.as_ref()))
}

fn main() {
    let (library_root, recorder, database) =
        create_runtime().expect("could not initialize ClipForge runtime");

    tauri::Builder::default()
        .manage(AppRuntime {
            library_root,
            recorder: Mutex::new(recorder),
            database: Mutex::new(database),
            capture: Mutex::new(None),
            league_poller: Mutex::new(LeagueLiveClientPoller::default()),
            valve_events: Mutex::new(Vec::new()),
            hotkey_shortcuts: Mutex::new(None),
            app_handle: Mutex::new(None),
        })
.setup(|app| {
            let handle = app.handle().clone();
            
            // Store app handle for hotkey re-registration
            {
                let runtime = app.state::<AppRuntime>();
                let _ = runtime.app_handle.lock().map(|mut guard| {
                    *guard = Some(handle.clone());
                });
            }
            
            start_valve_gsi_receiver(handle.clone());
            start_backend_workers(handle.clone());
            
            // Register hotkeys after storing the handle
            #[cfg(desktop)]
            {
                let runtime = app.state::<AppRuntime>();
                register_hotkeys(&handle, runtime.inner())?;
            }
            
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            refresh_detected_game,
            list_clips,
            set_replay_buffer,
            set_mic_enabled,
            set_system_audio_enabled,
            list_audio_devices,
        set_mic_device,
        set_auto_record_enabled,
        set_upload_settings,
        set_auto_clip_event_enabled,
        write_gsi_configs,
        poll_auto_clip_events,
        save_manual_clip,
        start_capture,
        stop_capture,
        delete_clip,
        reveal_clip,
        trim_clip,
        upload_clip,
        update_clip_metadata,
        export_clip_copy,
        set_hotkeys
        ])
        .run(tauri::generate_context!())
        .expect("error while running ClipForge desktop shell");
}

fn create_runtime() -> Result<(PathBuf, RecorderService, ClipDatabase), String> {
    let root = env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("clipforge-library");
    let settings = load_or_create_settings(&root)
        .map_err(|error| format!("Could not load settings: {error}"))?;
    let paths = LibraryPaths::new(&root);
    paths.ensure().map_err(|error| error.to_string())?;
    let database = ClipDatabase::open(root.join("library.sqlite"))
        .map_err(|error| format!("Could not open clip database: {error}"))?;
    let mut recorder = RecorderService::new(settings, paths);
    for clip in database
        .load_clips()
        .map_err(|error| format!("Could not load clip database: {error}"))?
    {
        recorder.library.add_clip(clip);
    }

    let _ = detect_game(&[], &default_profiles());
    Ok((root, recorder, database))
}

fn add_session_clip(
    recorder: &mut RecorderService,
    output_path: PathBuf,
    started_at: SystemTime,
    stopped_at: SystemTime,
) -> Result<Clip, String> {
    if !output_path.exists() {
        return Err(format!(
            "Session recording was not written: {}",
            output_path.display()
        ));
    }

    let duration = stopped_at
        .duration_since(started_at)
        .unwrap_or_else(|_| Duration::from_secs(0));
    let session_id = recorder
        .state
        .active_session_id
        .clone()
        .unwrap_or_else(|| format!("session-{}", unix_millis(started_at)));
    let game_id = recorder
        .state
        .detected_game_id
        .clone()
        .unwrap_or_else(|| "desktop".to_string());
    let clip_id = format!("session-{}", unix_millis(stopped_at));

    let clip = Clip {
        id: clip_id.clone(),
        title: Some("Full session".to_string()),
        session_id,
        game_id,
        path: output_path,
        thumbnail_path: Some(recorder.paths.thumbs_root.join(format!("{clip_id}.jpg"))),
        created_at: stopped_at,
        duration,
        source: ClipSource::Imported,
        event_type: None,
        tags: vec!["session".to_string()],
        upload_url: None,
        upload_provider: None,
    };
    recorder.library.add_clip(clip.clone());
    save_manifest(recorder)?;
    Ok(clip)
}

fn start_best_capture_backend(
    ffmpeg: &std::path::Path,
    segment_pattern: &std::path::Path,
    segment_duration: Duration,
    config: CaptureConfig,
    native_system_audio_available: bool,
    native_mic_available: bool,
    ffmpeg_system_audio_available: bool,
) -> Result<ActiveCaptureBackend, String> {
    if (!config.system_audio_enabled || native_system_audio_available)
        && (!config.mic_enabled || native_mic_available)
    {
        let mut native = NativeWgcReplayCaptureBackend::new(segment_pattern, segment_duration);
        if native.start(config.clone()).is_ok() {
            return Ok(ActiveCaptureBackend::NativeWgc(native));
        }
    }

    let mut ffmpeg_backend = FfmpegReplayCaptureBackend::new(ffmpeg, segment_pattern, segment_duration);
    let mut ffmpeg_config = config;
    if ffmpeg_config.system_audio_enabled && !ffmpeg_system_audio_available {
        ffmpeg_config.system_audio_enabled = false;
    }
    ffmpeg_backend
        .start(ffmpeg_config)
        .map_err(|error| error.to_string())?;
    Ok(ActiveCaptureBackend::Ffmpeg(ffmpeg_backend))
}

fn start_valve_gsi_receiver(app: tauri::AppHandle) {
    thread::spawn(move || {
        let Ok(listener) = TcpListener::bind("127.0.0.1:49321") else {
            return;
        };
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                continue;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
            let mut request = String::new();
            if stream.read_to_string(&mut request).is_ok() {
                if let Some(body) = request.split("\r\n\r\n").nth(1) {
                    if let Ok(events) = valve_gsi_raw_events(body, SystemTime::now()) {
                        if !events.is_empty() {
                            let runtime = app.state::<AppRuntime>();
                            if let Ok(mut queue) = runtime.valve_events.lock() {
                                queue.extend(events);
                            };
                        }
                    }
                }
            }
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nOK",
            );
        }
    });
}

fn start_backend_workers(app: tauri::AppHandle) {
    thread::Builder::new()
        .name("clipforge-backend-workers".to_string())
        .spawn(move || loop {
            let runtime = app.state::<AppRuntime>();
            let _ = refresh_detected_game_inner(runtime.inner());
            let _ = poll_auto_clip_events_inner(runtime.inner());
            thread::sleep(Duration::from_secs(2));
        })
        .expect("could not start ClipForge backend workers");
}

fn status_from_recorder(recorder: &RecorderService, capture: Option<&ActiveCapture>) -> DesktopStatus {
    DesktopStatus {
        recording_state: recording_status_name(&recorder.state.status).to_string(),
        detected_game: recorder.state.detected_game_id.clone(),
        replay_buffer_seconds: recorder.settings.replay_buffer.as_secs(),
        system_audio_enabled: recorder.settings.privacy.system_audio_enabled,
        mic_enabled: recorder.settings.privacy.mic_enabled,
        mic_device: recorder.settings.privacy.mic_device.clone(),
        auto_record_enabled: !recorder.settings.privacy.desktop_capture_requires_confirmation,
        upload_enabled: recorder.settings.upload.auto_upload_enabled,
        upload_provider: recorder.settings.upload.provider.clone(),
        catbox_userhash: recorder.settings.upload.catbox_userhash.clone(),
        litterbox_expiry_hours: recorder.settings.upload.litterbox_expiry_hours,
        custom_upload_endpoint: recorder.settings.upload.custom_endpoint.clone(),
        custom_upload_response_url_path: recorder.settings.upload.custom_response_url_path.clone(),
        custom_upload_headers: recorder
            .settings
            .upload
            .custom_headers
            .iter()
            .map(|(name, value)| format!("{name}: {value}"))
            .collect(),
        session_recording: matches!(recorder.state.status, RecordingStatus::RecordingSession),
        capture_active: capture.is_some(),
        capture_backend: capture.map(|active| active.backend.name().to_string()),
        capture_path: capture.map(|active| active.session_output_path.display().to_string()),
        clip_count: recorder.library.all().len(),
        library_root: recorder.paths.clip_root.display().to_string(),
        ffmpeg_available: ffmpeg_is_available(),
        ffmpeg_path: find_ffmpeg_executable().map(|path| path.display().to_string()),
        system_audio_available: native_system_loopback_available()
            || find_ffmpeg_executable()
            .map(|path| ffmpeg_supports_input_device(&path, "wasapi"))
            .unwrap_or(false),
        desktop_duplication_available: find_ffmpeg_executable()
            .map(|path| ffmpeg_supports_filter(&path, "ddagrab"))
            .unwrap_or(false),
        auto_clip_enabled_events: auto_clip_event_dtos(&recorder.settings),
        hotkey_clip_last_60s: recorder.settings.hotkeys.clip_last_60s.clone(),
        hotkey_clip_last_30s: recorder.settings.hotkeys.clip_last_30s.clone(),
        hotkey_toggle_session_recording: recorder.settings.hotkeys.toggle_session_recording.clone(),
        hotkey_screenshot: recorder.settings.hotkeys.screenshot.clone(),
    }
}

fn clip_to_dto(clip: &Clip) -> ClipDto {
    ClipDto {
        id: clip.id.clone(),
        title: clip.title.clone().unwrap_or_else(|| match clip.source {
            ClipSource::ManualHotkey => "Manual clip".to_string(),
            ClipSource::AutoEvent => "Auto clip".to_string(),
            ClipSource::FullSessionBookmark => "Session bookmark".to_string(),
            ClipSource::Imported => "Imported clip".to_string(),
        }),
        game: clip.game_id.clone(),
        event: clip
            .event_type
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "Manual".to_string()),
        source: match clip.source {
            ClipSource::ManualHotkey => "Manual Hotkey".to_string(),
            ClipSource::AutoEvent => "Auto Event".to_string(),
            ClipSource::FullSessionBookmark => "Bookmark".to_string(),
            ClipSource::Imported => "Imported".to_string(),
        },
        duration: format_duration(clip.duration),
        created_at: format_system_time(clip.created_at),
        upload_state: if clip
            .upload_url
            .as_deref()
            .map(|url| url.starts_with("clipforge://upload-failed/"))
            .unwrap_or(false)
        {
            "Failed".to_string()
        } else if clip
            .upload_url
            .as_deref()
            .map(|url| url.starts_with("clipforge://upload-queued/"))
            .unwrap_or(false)
        {
            "Queued".to_string()
        } else if clip.upload_url.is_some() {
            "Uploaded".to_string()
        } else {
            "Local only".to_string()
        },
        path: clip.path.display().to_string(),
        thumbnail_path: clip
            .thumbnail_path
            .as_ref()
            .map(|path| path.display().to_string()),
        tags: clip.tags.clone(),
        color_class: match clip.source {
            ClipSource::ManualHotkey => "manual".to_string(),
            ClipSource::AutoEvent => "kill".to_string(),
            ClipSource::FullSessionBookmark => "objective".to_string(),
            ClipSource::Imported => "manual".to_string(),
        },
    }
}

fn persist_clip(runtime: &AppRuntime, clip: &Clip) -> Result<(), String> {
    let database = runtime.database.lock().map_err(|error| error.to_string())?;
    database
        .upsert_clip(clip)
        .map_err(|error| format!("Could not save clip database record: {error}"))
}

fn delete_clip_record(runtime: &AppRuntime, clip_id: &str) -> Result<(), String> {
    let database = runtime.database.lock().map_err(|error| error.to_string())?;
    database
        .delete_clip(clip_id)
        .map_err(|error| format!("Could not delete clip database record: {error}"))
}

fn save_manifest(recorder: &RecorderService) -> Result<(), String> {
    let manifest = recorder
        .paths
        .sessions_root
        .parent()
        .unwrap_or(&recorder.paths.sessions_root)
        .join("library.tsv");
    recorder
        .library
        .save_manifest(&manifest)
        .map_err(|error| error.to_string())
}

fn recording_status_name(status: &RecordingStatus) -> &'static str {
    match status {
        RecordingStatus::WaitingForGame => "WaitingForGame",
        RecordingStatus::Buffering => "Buffering",
        RecordingStatus::RecordingSession => "RecordingSession",
        RecordingStatus::Clipping => "Clipping",
        RecordingStatus::Processing => "Processing",
        RecordingStatus::StorageLow => "StorageLow",
        RecordingStatus::Error(_) => "Processing",
    }
}

fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn format_system_time(time: SystemTime) -> String {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{seconds}")
}

fn unix_millis(time: SystemTime) -> u128 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
