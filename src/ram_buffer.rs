use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// One finished replay segment held entirely in memory. Encoders write to a
/// file path, so a background worker moves each completed segment from disk
/// into RAM (and removes the on-disk copy) right after it is finalized.
#[derive(Debug, Clone)]
pub struct RamSegment {
    pub file_name: String,
    pub modified_at: SystemTime,
    pub data: Vec<u8>,
}

#[derive(Default)]
pub struct RamReplayBuffer {
    segments: Vec<RamSegment>,
}

/// Hard ceiling so the in-memory replay buffer can never exhaust the
/// machine's RAM, even if the replay window grows well beyond the default.
pub const DEFAULT_RAM_CAP_BYTES: usize = 512 * 1024 * 1024;

impl RamReplayBuffer {
    pub fn insert(&mut self, file_name: String, modified_at: SystemTime, data: Vec<u8>) {
        self.segments.retain(|segment| segment.file_name != file_name);
        self.segments.push(RamSegment {
            file_name,
            modified_at,
            data,
        });
        self.segments
            .sort_by(|left, right| left.modified_at.cmp(&right.modified_at));
    }

    pub fn contains(&self, file_name: &str) -> bool {
        self.segments
            .iter()
            .any(|segment| segment.file_name == file_name)
    }

    /// Keeps only segments younger than the replay window plus one segment of
    /// slack, mirroring the disk-side policy in `prune_old_segments`.
    pub fn prune_to_window(
        &mut self,
        window: Duration,
        segment_duration: Duration,
        now: SystemTime,
    ) {
        let cutoff = now
            .checked_sub(window + segment_duration)
            .unwrap_or(SystemTime::UNIX_EPOCH);
        self.segments.retain(|segment| segment.modified_at >= cutoff);
    }

    /// Safety net: drops the oldest segments until the retained bytes fit
    /// under `cap`. Independent of the configured replay window.
    pub fn cap_bytes(&mut self, cap: usize) {
        let mut total: usize = self.segments.iter().map(|segment| segment.data.len()).sum();
        while total > cap {
            let Some(oldest) = self.segments.first() else {
                break;
            };
            total = total.saturating_sub(oldest.data.len());
            self.segments.remove(0);
        }
    }

    pub fn clear(&mut self) {
        self.segments.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn buffered_bytes(&self) -> usize {
        self.segments.iter().map(|segment| segment.data.len()).sum()
    }
}

/// A save candidate that may live either on disk (not yet ingested into RAM)
/// or strictly in RAM.
#[derive(Debug)]
pub struct MergedSegment {
    pub modified_at: SystemTime,
    pub file_name: String,
    pub on_disk: Option<PathBuf>,
    pub in_ram: Option<RamSegment>,
}

/// Merges the rolling segments for a save operation: everything still on disk
/// plus everything held in RAM, deduplicated by file name (a segment is either
/// on disk or already ingested), and sorted oldest-first. When `window` is
/// provided only segments inside the window are kept; `None` selects the whole
/// buffer (used when finishing a session recording).
pub fn merged_segments(
    ram: &RamReplayBuffer,
    buffer_dir: &Path,
    window: Option<Duration>,
    segment_duration: Duration,
    now: SystemTime,
) -> io::Result<Vec<MergedSegment>> {
    let cutoff = match window {
        Some(window) => now
            .checked_sub(window + segment_duration)
            .unwrap_or(SystemTime::UNIX_EPOCH),
        None => SystemTime::UNIX_EPOCH,
    };

    let mut merged: Vec<MergedSegment> = collect_disk_segments(buffer_dir)?
        .into_iter()
        .filter(|segment| segment.modified_at >= cutoff)
        .map(|segment| MergedSegment {
            modified_at: segment.modified_at,
            file_name: segment.file_name,
            on_disk: Some(segment.path),
            in_ram: None,
        })
        .collect();

    for ram_segment in ram
        .segments
        .iter()
        .filter(|segment| segment.modified_at >= cutoff)
    {
        if let Some(disk) = merged
            .iter_mut()
            .find(|candidate| candidate.file_name == ram_segment.file_name)
        {
            disk.in_ram = Some(ram_segment.clone());
        } else {
            merged.push(MergedSegment {
                modified_at: ram_segment.modified_at,
                file_name: ram_segment.file_name.clone(),
                on_disk: None,
                in_ram: Some(ram_segment.clone()),
            });
        }
    }

    merged.sort_by(|left, right| left.modified_at.cmp(&right.modified_at));
    Ok(merged)
}

/// Materializes RAM-held segments into real files inside `staging_dir` so that
/// FFmpeg can concatenate them, and returns the ordered list of paths covering
/// the whole merge (disk files are used in place, never copied).
pub fn spool_merged(
    staging_dir: &Path,
    prefix: &str,
    merged: &[MergedSegment],
) -> io::Result<Vec<PathBuf>> {
    if merged.is_empty() {
        return Ok(Vec::new());
    }
    fs::create_dir_all(staging_dir)?;
    let mut paths = Vec::with_capacity(merged.len());
    for (index, segment) in merged.iter().enumerate() {
        if let Some(path) = &segment.on_disk {
            paths.push(path.clone());
        } else if let Some(segment) = &segment.in_ram {
            let staged = staging_dir.join(format!(
                "{prefix}-{index:04}-{}",
                sanitize_file_name(&segment.file_name)
            ));
            fs::write(&staged, &segment.data)?;
            paths.push(staged);
        }
    }
    Ok(paths)
}

fn sanitize_file_name(name: &str) -> String {
    name.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

/// Lists `.mp4` files directly inside a buffer directory (current session only).
pub fn collect_disk_segments(buffer_dir: &Path) -> io::Result<Vec<DiskSegmentFile>> {
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
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let modified_at = entry.metadata()?.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        segments.push(DiskSegmentFile {
            file_name,
            path,
            modified_at,
        });
    }
    segments.sort_by(|left, right| left.modified_at.cmp(&right.modified_at));
    Ok(segments)
}

#[derive(Debug, Clone)]
pub struct DiskSegmentFile {
    pub file_name: String,
    pub path: PathBuf,
    pub modified_at: SystemTime,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(file_name: &str, age: u64, now: SystemTime) -> RamSegment {
        RamSegment {
            file_name: file_name.to_string(),
            modified_at: now - Duration::from_secs(age),
            data: file_name.as_bytes().to_vec(),
        }
    }

    #[test]
    fn window_prune_drops_only_stale_segments() {
        let now = SystemTime::now();
        let mut ram = RamReplayBuffer::default();
        ram.insert("a.mp4".to_string(), now - Duration::from_secs(60), vec![1]);
        ram.insert("b.mp4".to_string(), now - Duration::from_secs(20), vec![2]);
        ram.insert("c.mp4".to_string(), now - Duration::from_secs(2), vec![3]);

        ram.prune_to_window(Duration::from_secs(30), Duration::from_secs(5), now);

        let remaining: Vec<&str> = ram
            .segments
            .iter()
            .map(|segment| segment.file_name.as_str())
            .collect();
        assert_eq!(remaining, vec!["b.mp4", "c.mp4"]);
    }

    #[test]
    fn byte_cap_frees_oldest_first() {
        let now = SystemTime::now();
        let mut ram = RamReplayBuffer::default();
        ram.insert("a.mp4".to_string(), now - Duration::from_secs(30), vec![0; 100]);
        ram.insert("b.mp4".to_string(), now - Duration::from_secs(20), vec![0; 100]);
        ram.insert("c.mp4".to_string(), now - Duration::from_secs(10), vec![0; 100]);

        ram.cap_bytes(250);

        let first = ram.segments.first().map(|segment| segment.file_name.as_str());
        assert_eq!(first, Some("b.mp4"));
        assert_eq!(ram.buffered_bytes(), 200);
    }

    #[test]
    fn merge_prefers_disk_copy_and_is_sorted() {
        let root = std::env::temp_dir().join(format!(
            "clipforge-ram-{}",
            crate::media::timestamp_millis(SystemTime::now())
        ));
        fs::create_dir_all(&root).expect("test root");
        let now = SystemTime::now();
        fs::write(root.join("disk.mp4"), b"disk").expect("disk file");
        // The disk file's real mtime is essentially "now", so it sorts after
        // the RAM-held segment that claims to be 6 seconds old.
        let disk_modified_at = fs::metadata(root.join("disk.mp4"))
            .expect("disk metadata")
            .modified()
            .expect("disk modified");

        let mut ram = RamReplayBuffer::default();
        ram.insert(
            "disk.mp4".to_string(),
            now - Duration::from_secs(10),
            b"ram-copy".to_vec(),
        );
        ram.insert(
            "ram-only.mp4".to_string(),
            now - Duration::from_secs(6),
            b"ram-only".to_vec(),
        );
        ram.insert(
            "old.mp4".to_string(),
            now - Duration::from_secs(120),
            b"old".to_vec(),
        );

        let merged = merged_segments(
            &ram,
            &root,
            Some(Duration::from_secs(60)),
            Duration::from_secs(5),
            now,
        )
        .expect("merge");

        let file_names: Vec<String> = merged
            .iter()
            .map(|segment| segment.file_name.clone())
            .collect();
        // Sorted oldest-first: ram-only (6s old) before the just-written disk.mp4.
        assert_eq!(file_names, vec!["ram-only.mp4", "disk.mp4"]);
        // The disk file wins over the identical RAM copy.
        let disk_merge = &merged[1];
        assert!(disk_merge.on_disk.is_some());
        assert_eq!(
            disk_merge.on_disk.as_ref().unwrap().file_name().unwrap().to_str(),
            Some("disk.mp4")
        );
        assert!(disk_merge.modified_at >= disk_modified_at);

        let spool = root.join("staging");
        let paths = spool_merged(&spool, "clip", &merged).expect("spool");
        assert_eq!(paths.len(), 2);
        let _ = fs::remove_dir_all(root);
    }
}