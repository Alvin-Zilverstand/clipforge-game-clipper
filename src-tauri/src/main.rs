use clipforge::capture::{
    ffmpeg_is_available, find_ffmpeg_executable, CaptureBackend, CaptureConfig, CaptureSource,
    EncoderPreference, FfmpegReplayCaptureBackend,
};
use clipforge::database::ClipDatabase;
use clipforge::game_detection::{default_profiles, detect_game};
use clipforge::game_watcher::running_processes;
use clipforge::media::{
    collect_segments, concat_segments, generate_thumbnail, prune_old_segments, recent_segments,
    trim_clip as ffmpeg_trim_clip,
};
use clipforge::models::{Clip, ClipSource, RecordingStatus, UploadProvider};
use clipforge::recorder::{RecorderAction, RecorderService};
use clipforge::settings::AppSettings;
use clipforge::storage::{clip_path, is_inside_root, LibraryPaths};
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
    database: Mutex<ClipDatabase>,
    capture: Mutex<Option<ActiveCapture>>,
}

#[derive(Debug)]
struct ActiveCapture {
    buffer_dir: PathBuf,
    session_output_path: PathBuf,
    segment_duration: Duration,
    started_at: SystemTime,
    backend: FfmpegReplayCaptureBackend,
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
    thumbnail_path: Option<String>,
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
fn refresh_detected_game(runtime: State<'_, AppRuntime>) -> Result<DesktopStatus, String> {
    let mut recorder = runtime.recorder.lock().map_err(|error| error.to_string())?;
    let capture = runtime.capture.lock().map_err(|error| error.to_string())?;
    let detected = detect_game(&running_processes(), &default_profiles());

    match detected {
        Some(game) => {
            if recorder.state.detected_game_id.as_deref() != Some(&game.game_id) {
                recorder.start_for_game(game.game_id, SystemTime::now());
            }
        }
        None if capture.is_none() => {
            let _ = recorder.stop();
        }
        None => {}
    }

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
    persist_clip(&runtime, &clip)?;
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
    let session_dir = recorder.paths.sessions_root.join(&session_id);
    let buffer_dir = recorder.paths.buffer_root.join(&session_id);
    fs::create_dir_all(&session_dir).map_err(|error| error.to_string())?;
    fs::create_dir_all(&buffer_dir).map_err(|error| error.to_string())?;
    let session_output_path = session_dir.join(format!("session-{}.mp4", unix_millis(now)));
    let segment_duration = Duration::from_secs(5);
    let segment_pattern = buffer_dir.join("segment-%05d.mp4");

    let config = CaptureConfig {
        source: CaptureSource::Desktop,
        width: recorder.settings.quality.width,
        height: recorder.settings.quality.height,
        fps: recorder.settings.quality.fps,
        bitrate_kbps: recorder.settings.quality.bitrate_kbps,
        encoder: EncoderPreference::HardwareH264,
        system_audio_enabled: true,
        mic_enabled: recorder.settings.privacy.mic_enabled,
        system_audio_device: None,
        mic_device: None,
    };
    let mut backend = FfmpegReplayCaptureBackend::new(ffmpeg, &segment_pattern, segment_duration);
    backend.start(config).map_err(|error| error.to_string())?;
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
            persist_clip(&runtime, &clip)?;
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

    delete_clip_record(&runtime, &clip_id)?;
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
    persist_clip(&runtime, &clip)?;
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
    let clip = recorder
        .library
        .all()
        .iter()
        .find(|clip| clip.id == clip_id)
        .cloned()
        .ok_or_else(|| format!("Clip {clip_id} was not found."))?;
    persist_clip(&runtime, &clip)?;
    save_manifest(&recorder)?;
    Ok(clip_to_dto(&clip))
}

fn main() {
    let (recorder, database) = create_runtime().expect("could not initialize ClipForge runtime");

    tauri::Builder::default()
        .manage(AppRuntime {
            recorder: Mutex::new(recorder),
            database: Mutex::new(database),
            capture: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            refresh_detected_game,
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

fn create_runtime() -> Result<(RecorderService, ClipDatabase), String> {
    let root = env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("clipforge-library");
    let settings = AppSettings::default_for_root(&root);
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
    Ok((recorder, database))
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

fn status_from_recorder(recorder: &RecorderService, capture: Option<&ActiveCapture>) -> DesktopStatus {
    DesktopStatus {
        recording_state: recording_status_name(&recorder.state.status).to_string(),
        detected_game: recorder.state.detected_game_id.clone(),
        replay_buffer_seconds: recorder.settings.replay_buffer.as_secs(),
        mic_enabled: recorder.settings.privacy.mic_enabled,
        upload_enabled: false,
        session_recording: matches!(recorder.state.status, RecordingStatus::RecordingSession),
        capture_active: capture.is_some(),
        capture_path: capture.map(|active| active.session_output_path.display().to_string()),
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

fn persist_clip(runtime: &State<'_, AppRuntime>, clip: &Clip) -> Result<(), String> {
    let database = runtime.database.lock().map_err(|error| error.to_string())?;
    database
        .upsert_clip(clip)
        .map_err(|error| format!("Could not save clip database record: {error}"))
}

fn delete_clip_record(runtime: &State<'_, AppRuntime>, clip_id: &str) -> Result<(), String> {
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
