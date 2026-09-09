use crate::models::{Clip, ClipSource};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryPaths {
    pub clip_root: PathBuf,
    pub thumbs_root: PathBuf,
    pub sessions_root: PathBuf,
    pub buffer_root: PathBuf,
}

impl LibraryPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            clip_root: root.join("clips"),
            thumbs_root: root.join("thumbs"),
            sessions_root: root.join("sessions"),
            buffer_root: root.join("buffer"),
        }
    }

    pub fn ensure(&self) -> io::Result<()> {
        fs::create_dir_all(&self.clip_root)?;
        fs::create_dir_all(&self.thumbs_root)?;
        fs::create_dir_all(&self.sessions_root)?;
        fs::create_dir_all(&self.buffer_root)?;
        Ok(())
    }
}

pub fn clip_path(
    paths: &LibraryPaths,
    game_id: &str,
    created_at: SystemTime,
    clip_id: &str,
) -> PathBuf {
    let month_bucket = month_bucket(created_at);
    paths
        .clip_root
        .join(sanitize_path_component(game_id))
        .join(month_bucket)
        .join(format!("{}.mp4", sanitize_path_component(clip_id)))
}

pub fn build_placeholder_clip(
    paths: &LibraryPaths,
    clip_id: &str,
    session_id: &str,
    game_id: &str,
    duration: Duration,
) -> Clip {
    let created_at = SystemTime::now();
    Clip {
        id: clip_id.to_string(),
        session_id: session_id.to_string(),
        game_id: game_id.to_string(),
        path: clip_path(paths, game_id, created_at, clip_id),
        thumbnail_path: Some(paths.thumbs_root.join(format!("{clip_id}.jpg"))),
        created_at,
        duration,
        source: ClipSource::ManualHotkey,
        event_type: None,
        tags: Vec::new(),
        upload_url: None,
        upload_provider: None,
    }
}

pub fn sanitize_path_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn month_bucket(created_at: SystemTime) -> String {
    let days = created_at
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86_400;
    let approx_year = 1970 + days / 365;
    let approx_month = ((days % 365) / 31) + 1;
    format!("{approx_year:04}-{approx_month:02}")
}

pub fn is_inside_root(root: &Path, candidate: &Path) -> bool {
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    let Ok(candidate) = candidate.canonicalize() else {
        return false;
    };
    candidate.starts_with(root)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageCleanupReport {
    pub deleted_files: usize,
    pub deleted_bytes: u64,
    pub remaining_bytes: u64,
}

pub fn cleanup_oldest_files_until_under_limit(
    root: &Path,
    max_bytes: u64,
) -> io::Result<StorageCleanupReport> {
    let mut files = collect_files(root)?;
    let mut total_bytes: u64 = files.iter().map(|file| file.bytes).sum();
    let mut deleted_files = 0;
    let mut deleted_bytes = 0;

    files.sort_by_key(|file| file.modified_at);

    for file in files {
        if total_bytes <= max_bytes {
            break;
        }
        fs::remove_file(&file.path)?;
        total_bytes = total_bytes.saturating_sub(file.bytes);
        deleted_files += 1;
        deleted_bytes += file.bytes;
    }

    Ok(StorageCleanupReport {
        deleted_files,
        deleted_bytes,
        remaining_bytes: total_bytes,
    })
}

#[derive(Debug, Clone)]
struct FileFact {
    path: PathBuf,
    bytes: u64,
    modified_at: SystemTime,
}

fn collect_files(root: &Path) -> io::Result<Vec<FileFact>> {
    let mut files = Vec::new();
    if !root.exists() {
        return Ok(files);
    }

    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            files.extend(collect_files(&path)?);
        } else {
            let metadata = entry.metadata()?;
            files.push(FileFact {
                path,
                bytes: metadata.len(),
                modified_at: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            });
        }
    }

    Ok(files)
}

pub fn write_placeholder_mp4(path: &Path, label: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("ClipForge placeholder video: {label}\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;

    #[test]
    fn sanitizes_path_components() {
        assert_eq!(
            sanitize_path_component("Counter Strike 2!"),
            "Counter-Strike-2"
        );
    }

    #[test]
    fn cleanup_removes_old_files_until_under_limit() {
        let root = env::temp_dir().join(format!(
            "clipforge-cleanup-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&root).expect("test root");
        fs::write(root.join("one.tmp"), vec![1_u8; 10]).expect("file one");
        fs::write(root.join("two.tmp"), vec![1_u8; 10]).expect("file two");

        let report = cleanup_oldest_files_until_under_limit(&root, 10).expect("cleanup");

        assert_eq!(report.deleted_files, 1);
        assert_eq!(report.remaining_bytes, 10);
        let _ = fs::remove_dir_all(root);
    }
}
