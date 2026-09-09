use clipforge::capture::{
    ffmpeg_is_available, find_ffmpeg_executable, CaptureBackend, CaptureConfig, CaptureSource,
    EncoderPreference, FfmpegCaptureBackend,
};
use clipforge::game_detection::{default_profiles, detect_game, RunningProcess};
use clipforge::models::{Clip, ClipSource, RecordingStatus, UploadProvider};
use clipforge::recorder::{BufferSegment, RecorderAction, RecorderService};
use clipforge::settings::AppSettings;
use clipforge::storage::{clip_path, is_inside_root, write_placeholder_mp4, LibraryPaths};
use serde::Serialize;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::State;

struct AppRuntime {
    recorder: Mutex<RecorderService>,
    capture: Mutex<Option<ActiveCapture>>,
}

#[derive(Debug)]
struct ActiveCapture {
    output_path: PathBuf,
    started_at: SystemTime,
    backend: FfmpegCaptureBackend,
}

#[derive(Debug, Serialize)]
struct DesktopStatus {
    recording_state: String,
    detected_game: Option<String>,
    replay_buffer_seconds: u64,
    mic_enabled: bool,
    upload_enabled: bool,
    session_recording: bool,
    capture_active: bool,
    capture_path: Option<String>,
    clip_count: usize,
    library_root: String,
    ffmpeg_available: bool,
    ffmpeg_path: Option<String>,
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
    tags: Vec<String>,
    color_class: String,
}

#[tauri::command]
fn get_status(runtime: State<'_, AppRuntime>) -> Result<DesktopStatus, String> {
    let recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let capture = runtime.capture.lock().map_err(|error| error.to_string())?;
    Ok(status_from_recorder(&recorder, capture.as_ref()))
}

#[tauri::command]
fn list_clips(runtime: State<'_, AppRuntime>) -> Result<Vec<ClipDto>, String> {
    let recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    Ok(recorder.library.all().iter().map(clip_to_dto).collect())
}

#[tauri::command]
fn save_manual_clip(seconds: u64, runtime: State<'_, AppRuntime>) -> Result<ClipDto, String> {
    let mut recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let now = SystemTime::now();
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

    write_placeholder_mp4(&clip.path, &clip.id).map_err(|error| error.to_string())?;
    save_manifest(&recorder)?;
    Ok(clip_to_dto(&clip))
}

#[tauri::command]
fn start_capture(runtime: State<'_, AppRuntime>) -> Result<DesktopStatus, String> {
    let mut recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let mut capture = runtime.capture.lock().map_err(|error| error.to_string())?;

    if capture.is_some() {
        return Ok(status_from_recorder(&recorder, capture.as_ref()));
    }

    let ffmpeg = find_ffmpeg_executable()
        .ok_or_else(|| "FFmpeg was not found on PATH or in the bundled sidecar.".to_string())?;
    let now = SystemTime::now();
    if recorder.state.active_session_id.is_none() {
        recorder.start_for_game("desktop", now);
    }
    recorder.toggle_session_recording();

    let session_id = recorder
        .state
        .active_session_id
        .clone()
        .ok_or_else(|| "Recorder session did not start.".to_string())?;
    let output_dir = recorder.paths.sessions_root.join(&session_id);
    fs::create_dir_all(&output_dir).map_err(|error| error.to_string())?;
    let output_path = output_dir.join(format!("session-{}.mp4", unix_millis(now)));

    let config = CaptureConfig {
        source: CaptureSource::Desktop,
        width: recorder.settings.quality.width,
        height: recorder.settings.quality.height,
        fps: recorder.settings.quality.fps,
        bitrate_kbps: recorder.settings.quality.bitrate_kbps,
        encoder: EncoderPreference::HardwareH264,
        mic_enabled: recorder.settings.privacy.mic_enabled,
    };
    let mut backend = FfmpegCaptureBackend::new(ffmpeg, &output_path);
    backend.start(config).map_err(|error| error.to_string())?;
    *capture = Some(ActiveCapture {
        output_path,
        started_at: now,
        backend,
    });

    Ok(status_from_recorder(&recorder, capture.as_ref()))
}

#[tauri::command]
fn stop_capture(runtime: State<'_, AppRuntime>) -> Result<DesktopStatus, String> {
    let mut recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let mut capture = runtime.capture.lock().map_err(|error| error.to_string())?;

    if let Some(mut active) = capture.take() {
        active.backend.stop().map_err(|error| error.to_string())?;
        add_session_clip(&mut recorder, active.output_path, active.started_at, SystemTime::now())?;
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

    let status = Command::new(ffmpeg)
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            &format!("{start_seconds:.3}"),
            "-to",
            &format!("{end_seconds:.3}"),
            "-i",
            &source_clip.path.display().to_string(),
            "-c",
            "copy",
            &output_path.display().to_string(),
        ])
        .status()
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err("FFmpeg could not trim this clip.".to_string());
    }

    let clip = Clip {
        id: trimmed_id,
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
    recorder.library.add_clip(clip.clone());
    save_manifest(&recorder)?;
    Ok(clip_to_dto(&clip))
}

#[tauri::command]
fn upload_clip(clip_id: String, runtime: State<'_, AppRuntime>) -> Result<ClipDto, String> {
    let mut recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let queued_url = format!("clipforge://upload-queued/{clip_id}");
    if !recorder
        .library
        .update_upload(&clip_id, UploadProvider::CustomHttp, queued_url)
    {
        return Err(format!("Clip {clip_id} was not found."));
    }
    save_manifest(&recorder)?;
    recorder
        .library
        .all()
        .iter()
        .find(|clip| clip.id == clip_id)
        .map(clip_to_dto)
        .ok_or_else(|| format!("Clip {clip_id} was not found."))
}

fn main() {
    let recorder = create_boot_recorder();

    tauri::Builder::default()
        .manage(AppRuntime {
            recorder: Mutex::new(recorder),
            capture: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            list_clips,
            save_manual_clip,
            start_capture,
            stop_capture,
            delete_clip,
            reveal_clip,
            trim_clip,
            upload_clip
        ])
        .run(tauri::generate_context!())
        .expect("error while running ClipForge desktop shell");
}

fn create_boot_recorder() -> RecorderService {
    let root = env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("clipforge-library");
    let settings = AppSettings::default_for_root(&root);
    let paths = LibraryPaths::new(&root);
    let _ = paths.ensure();
    let mut recorder = RecorderService::new(settings, paths);

    let sample_processes = vec![RunningProcess {
        pid: 730,
        process_name: "cs2.exe".to_string(),
        executable_path: "C:/Steam/cs2.exe".to_string(),
        window_title: Some("Counter-Strike 2".to_string()),
    }];

    if let Some(game) = detect_game(&sample_processes, &default_profiles()) {
        let now = SystemTime::now();
        recorder.start_for_game(game.game_id, now);
        for index in 0..12 {
            recorder.add_segment(BufferSegment {
                id: format!("boot-segment-{index}"),
                started_at: now + Duration::from_secs(index * 5),
                duration: Duration::from_secs(5),
                path_hint: format!("boot_segment_{index}.mp4"),
            });
        }
    }

    recorder
}

fn add_session_clip(
    recorder: &mut RecorderService,
    output_path: PathBuf,
    started_at: SystemTime,
    stopped_at: SystemTime,
) -> Result<(), String> {
    if !output_path.exists() {
        return Ok(());
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
    recorder.library.add_clip(clip);
    save_manifest(recorder)
}

fn status_from_recorder(recorder: &RecorderService, capture: Option<&ActiveCapture>) -> DesktopStatus {
    DesktopStatus {
        recording_state: recording_status_name(&recorder.state.status).to_string(),
        detected_game: recorder.state.detected_game_id.clone(),
        replay_buffer_seconds: recorder.settings.replay_buffer.as_secs(),
        mic_enabled: recorder.settings.privacy.mic_enabled,
        upload_enabled: false,
        session_recording: matches!(recorder.state.status, RecordingStatus::RecordingSession),
        capture_active: capture.is_some(),
        capture_path: capture.map(|active| active.output_path.display().to_string()),
        clip_count: recorder.library.all().len(),
        library_root: recorder.paths.clip_root.display().to_string(),
        ffmpeg_available: ffmpeg_is_available(),
        ffmpeg_path: find_ffmpeg_executable().map(|path| path.display().to_string()),
    }
}

fn clip_to_dto(clip: &Clip) -> ClipDto {
    ClipDto {
        id: clip.id.clone(),
        title: match clip.source {
            ClipSource::ManualHotkey => "Manual clip".to_string(),
            ClipSource::AutoEvent => "Auto clip".to_string(),
            ClipSource::FullSessionBookmark => "Session bookmark".to_string(),
            ClipSource::Imported => "Imported clip".to_string(),
        },
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
        tags: clip.tags.clone(),
        color_class: match clip.source {
            ClipSource::ManualHotkey => "manual".to_string(),
            ClipSource::AutoEvent => "kill".to_string(),
            ClipSource::FullSessionBookmark => "objective".to_string(),
            ClipSource::Imported => "manual".to_string(),
        },
    }
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
