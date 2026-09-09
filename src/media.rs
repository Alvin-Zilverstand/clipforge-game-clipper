use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentFile {
    pub path: PathBuf,
    pub modified_at: SystemTime,
}

pub fn collect_segments(buffer_dir: &Path) -> io::Result<Vec<SegmentFile>> {
    let mut segments = Vec::new();
    if !buffer_dir.exists() {
        return Ok(segments);
    }

    for entry in fs::read_dir(buffer_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("mp4") {
            continue;
        }
        let metadata = entry.metadata()?;
        segments.push(SegmentFile {
            path,
            modified_at: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        });
    }

    segments.sort_by(|left, right| left.modified_at.cmp(&right.modified_at));
    Ok(segments)
}

pub fn recent_segments(
    buffer_dir: &Path,
    window: Duration,
    segment_duration: Duration,
    now: SystemTime,
) -> io::Result<Vec<PathBuf>> {
    let cutoff = now
        .checked_sub(window + segment_duration)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    Ok(collect_segments(buffer_dir)?
        .into_iter()
        .filter(|segment| segment.modified_at >= cutoff)
        .map(|segment| segment.path)
        .collect())
}

pub fn prune_old_segments(
    buffer_dir: &Path,
    replay_window: Duration,
    segment_duration: Duration,
) -> io::Result<usize> {
    let mut segments = collect_segments(buffer_dir)?;
    let keep_count =
        (replay_window.as_secs_f64() / segment_duration.as_secs_f64()).ceil() as usize + 4;
    if segments.len() <= keep_count {
        return Ok(0);
    }

    let remove_count = segments.len() - keep_count;
    let mut removed = 0;
    for segment in segments.drain(..remove_count) {
        fs::remove_file(segment.path)?;
        removed += 1;
    }
    Ok(removed)
}

pub fn concat_segments(
    ffmpeg: &Path,
    segments: &[PathBuf],
    output: &Path,
) -> Result<(), MediaError> {
    if segments.is_empty() {
        return Err(MediaError::NoSegments);
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }

    if segments.len() == 1 {
        fs::copy(&segments[0], output)?;
        return Ok(());
    }

    let list_path = output.with_extension("concat.txt");
    let mut list = String::new();
    for segment in segments {
        list.push_str("file '");
        list.push_str(&escape_concat_path(segment));
        list.push_str("'\n");
    }
    fs::write(&list_path, list)?;

    let status = Command::new(ffmpeg)
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
            &list_path.display().to_string(),
            "-c",
            "copy",
            &output.display().to_string(),
        ])
        .status()
        .map_err(|error| MediaError::Ffmpeg(error.to_string()))?;

    let _ = fs::remove_file(list_path);
    if status.success() {
        Ok(())
    } else {
        Err(MediaError::Ffmpeg("FFmpeg concat failed".to_string()))
    }
}

pub fn trim_clip(
    ffmpeg: &Path,
    input: &Path,
    output: &Path,
    start_seconds: f64,
    end_seconds: f64,
) -> Result<(), MediaError> {
    if !(end_seconds > start_seconds && start_seconds >= 0.0) {
        return Err(MediaError::InvalidWindow);
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
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
            &input.display().to_string(),
            "-c",
            "copy",
            &output.display().to_string(),
        ])
        .status()
        .map_err(|error| MediaError::Ffmpeg(error.to_string()))?;

    if status.success() {
        Ok(())
    } else {
        Err(MediaError::Ffmpeg("FFmpeg trim failed".to_string()))
    }
}

pub fn generate_thumbnail(ffmpeg: &Path, input: &Path, output: &Path) -> Result<(), MediaError> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }

    let status = Command::new(ffmpeg)
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            "00:00:01",
            "-i",
            &input.display().to_string(),
            "-frames:v",
            "1",
            "-q:v",
            "3",
            &output.display().to_string(),
        ])
        .status()
        .map_err(|error| MediaError::Ffmpeg(error.to_string()))?;

    if status.success() {
        Ok(())
    } else {
        Err(MediaError::Ffmpeg(
            "FFmpeg thumbnail generation failed".to_string(),
        ))
    }
}

fn escape_concat_path(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "/")
        .replace('\'', "'\\''")
}

#[derive(Debug)]
pub enum MediaError {
    Io(io::Error),
    Ffmpeg(String),
    InvalidWindow,
    NoSegments,
}

impl std::fmt::Display for MediaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Ffmpeg(error) => write!(f, "{error}"),
            Self::InvalidWindow => f.write_str("clip window is invalid"),
            Self::NoSegments => f.write_str("no replay buffer segments are available"),
        }
    }
}

impl From<io::Error> for MediaError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub fn timestamp_millis(time: SystemTime) -> u128 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concat_paths_are_ffmpeg_friendly() {
        let path = Path::new("C:\\clips\\a'b.mp4");
        assert_eq!(escape_concat_path(path), "C:/clips/a'\\''b.mp4");
    }

    #[test]
    fn prune_keeps_replay_window_plus_slack() {
        let root = std::env::temp_dir().join(format!(
            "clipforge-media-{}",
            timestamp_millis(SystemTime::now())
        ));
        fs::create_dir_all(&root).expect("test root");
        for index in 0..8 {
            fs::write(root.join(format!("segment-{index}.mp4")), b"x").expect("segment");
        }

        let removed = prune_old_segments(&root, Duration::from_secs(10), Duration::from_secs(5))
            .expect("prune");

        assert_eq!(removed, 2);
        assert_eq!(collect_segments(&root).expect("segments").len(), 6);
        let _ = fs::remove_dir_all(root);
    }
}
