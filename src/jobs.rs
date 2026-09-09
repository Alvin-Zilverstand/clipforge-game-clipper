use crate::models::UploadProvider;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrimJob {
    pub id: String,
    pub clip_id: String,
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub start: Duration,
    pub end: Duration,
    pub status: JobStatus,
}

impl TrimJob {
    pub fn new(
        id: impl Into<String>,
        clip_id: impl Into<String>,
        input_path: impl Into<PathBuf>,
        output_path: impl Into<PathBuf>,
        start: Duration,
        end: Duration,
    ) -> Result<Self, String> {
        if end <= start {
            return Err("trim end must be after trim start".to_string());
        }

        Ok(Self {
            id: id.into(),
            clip_id: clip_id.into(),
            input_path: input_path.into(),
            output_path: output_path.into(),
            start,
            end,
            status: JobStatus::Queued,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadJob {
    pub id: String,
    pub clip_id: String,
    pub provider: UploadProvider,
    pub queued_at: SystemTime,
    pub attempts: u8,
    pub status: JobStatus,
}

impl UploadJob {
    pub fn new(
        id: impl Into<String>,
        clip_id: impl Into<String>,
        provider: UploadProvider,
    ) -> Self {
        Self {
            id: id.into(),
            clip_id: clip_id.into(),
            provider,
            queued_at: SystemTime::now(),
            attempts: 0,
            status: JobStatus::Queued,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_job_rejects_empty_window() {
        let result = TrimJob::new(
            "job",
            "clip",
            "in.mp4",
            "out.mp4",
            Duration::from_secs(10),
            Duration::from_secs(10),
        );

        assert!(result.is_err());
    }
}
