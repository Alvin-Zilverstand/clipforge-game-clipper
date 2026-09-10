use crate::models::{Clip, ClipSource, GameEventType, UploadProvider};
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Default)]
pub struct ClipFilter {
    pub game_id: Option<String>,
    pub event_type: Option<GameEventType>,
    pub upload_provider: Option<UploadProvider>,
    pub tag: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ClipLibrary {
    clips: Vec<Clip>,
}

impl ClipLibrary {
    pub fn new() -> Self {
        Self { clips: Vec::new() }
    }

    pub fn add_clip(&mut self, clip: Clip) {
        if let Some(existing) = self
            .clips
            .iter_mut()
            .find(|existing| existing.id == clip.id)
        {
            *existing = clip;
        } else {
            self.clips.push(clip);
            self.clips
                .sort_by(|left, right| right.created_at.cmp(&left.created_at));
        }
    }

    pub fn remove_clip(&mut self, clip_id: &str) -> Option<Clip> {
        let index = self.clips.iter().position(|clip| clip.id == clip_id)?;
        Some(self.clips.remove(index))
    }

    pub fn update_upload(
        &mut self,
        clip_id: &str,
        provider: UploadProvider,
        url: impl Into<String>,
    ) -> bool {
        let Some(clip) = self.clips.iter_mut().find(|clip| clip.id == clip_id) else {
            return false;
        };
        clip.upload_provider = Some(provider);
        clip.upload_url = Some(url.into());
        true
    }

    pub fn all(&self) -> &[Clip] {
        &self.clips
    }

    pub fn filter(&self, filter: &ClipFilter) -> Vec<&Clip> {
        self.clips
            .iter()
            .filter(|clip| {
                filter
                    .game_id
                    .as_ref()
                    .map(|game_id| &clip.game_id == game_id)
                    .unwrap_or(true)
                    && filter
                        .event_type
                        .as_ref()
                        .map(|event_type| clip.event_type.as_ref() == Some(event_type))
                        .unwrap_or(true)
                    && filter
                        .upload_provider
                        .as_ref()
                        .map(|provider| clip.upload_provider.as_ref() == Some(provider))
                        .unwrap_or(true)
                    && filter
                        .tag
                        .as_ref()
                        .map(|tag| clip.tags.iter().any(|clip_tag| clip_tag == tag))
                        .unwrap_or(true)
            })
            .collect()
    }

    pub fn save_manifest(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, self.to_manifest())
    }

    pub fn to_manifest(&self) -> String {
        let mut manifest = String::from("id\tgame_id\tsession_id\tpath\tduration_ms\tsource\tevent_type\ttags\tupload_provider\tupload_url\n");
        for clip in &self.clips {
            let _ = writeln!(
                manifest,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                escape(&clip.id),
                escape(&clip.game_id),
                escape(&clip.session_id),
                escape(&clip.path.to_string_lossy()),
                clip.duration.as_millis(),
                clip_source_name(&clip.source),
                clip.event_type
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                escape(&clip.tags.join(",")),
                clip.upload_provider
                    .as_ref()
                    .map(upload_provider_name)
                    .unwrap_or_default(),
                escape(clip.upload_url.as_deref().unwrap_or_default())
            );
        }
        manifest
    }
}

pub fn deterministic_clip_id(prefix: &str, at: SystemTime) -> String {
    let millis = at
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{prefix}-{millis}")
}

fn escape(value: &str) -> String {
    value.replace('\t', " ").replace('\n', " ")
}

fn clip_source_name(source: &ClipSource) -> &'static str {
    match source {
        ClipSource::ManualHotkey => "manual_hotkey",
        ClipSource::AutoEvent => "auto_event",
        ClipSource::FullSessionBookmark => "full_session_bookmark",
        ClipSource::Imported => "imported",
    }
}

fn upload_provider_name(provider: &UploadProvider) -> &'static str {
    match provider {
        UploadProvider::Catbox => "catbox",
        UploadProvider::Litterbox => "litterbox",
        UploadProvider::Lustful => "lustful",
        UploadProvider::CustomHttp => "custom_http",
    }
}

pub fn total_clip_bytes(clips: &[Clip]) -> u64 {
    clips
        .iter()
        .filter_map(|clip| clip.path.metadata().ok())
        .map(|metadata| metadata.len())
        .sum()
}

pub fn trim_duration(start: Duration, end: Duration) -> Option<Duration> {
    end.checked_sub(start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{build_test_clip, LibraryPaths};

    #[test]
    fn filters_by_game_and_tag() {
        let paths = LibraryPaths::new("root");
        let mut library = ClipLibrary::new();
        let mut clip = build_test_clip(
            &paths,
            "clip",
            "session",
            "counter-strike-2",
            Duration::from_secs(18),
        );
        clip.tags.push("clutch".to_string());
        library.add_clip(clip);

        let filter = ClipFilter {
            game_id: Some("counter-strike-2".to_string()),
            tag: Some("clutch".to_string()),
            ..ClipFilter::default()
        };

        assert_eq!(library.filter(&filter).len(), 1);
    }

    #[test]
    fn manifest_contains_clip_rows() {
        let paths = LibraryPaths::new("root");
        let mut library = ClipLibrary::new();
        library.add_clip(build_test_clip(
            &paths,
            "clip",
            "session",
            "dota-2",
            Duration::from_secs(60),
        ));

        assert!(library.to_manifest().contains("dota-2"));
    }
}
