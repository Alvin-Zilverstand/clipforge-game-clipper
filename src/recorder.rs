use crate::library::{deterministic_clip_id, ClipLibrary};
use crate::models::{Clip, ClipSource, GameEvent, RecordingStatus};
use crate::settings::AppSettings;
use crate::storage::{clip_path, LibraryPaths};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferSegment {
    pub id: String,
    pub started_at: SystemTime,
    pub duration: Duration,
    pub path_hint: String,
}

impl BufferSegment {
    fn ends_at(&self) -> SystemTime {
        self.started_at + self.duration
    }
}

#[derive(Debug, Clone)]
pub struct ReplayBuffer {
    capacity: Duration,
    segments: VecDeque<BufferSegment>,
}

impl ReplayBuffer {
    pub fn new(capacity: Duration) -> Self {
        Self {
            capacity,
            segments: VecDeque::new(),
        }
    }

    pub fn push(&mut self, segment: BufferSegment) {
        self.segments.push_back(segment);
        self.prune();
    }

    pub fn segments_for_window(&self, end: SystemTime, length: Duration) -> Vec<BufferSegment> {
        let start = end.checked_sub(length).unwrap_or(SystemTime::UNIX_EPOCH);
        self.segments
            .iter()
            .filter(|segment| segment.ends_at() >= start && segment.started_at <= end)
            .cloned()
            .collect()
    }

    pub fn len(&self) -> usize {
        self.segments.len()
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    fn prune(&mut self) {
        let Some(newest) = self.segments.back().map(BufferSegment::ends_at) else {
            return;
        };
        let cutoff = newest
            .checked_sub(self.capacity)
            .unwrap_or(SystemTime::UNIX_EPOCH);
        while self
            .segments
            .front()
            .map(|segment| segment.ends_at() < cutoff)
            .unwrap_or(false)
        {
            self.segments.pop_front();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoClipPolicy {
    pub pre_roll: Duration,
    pub post_roll: Duration,
    pub merge_window: Duration,
}

impl Default for AutoClipPolicy {
    fn default() -> Self {
        Self {
            pre_roll: Duration::from_secs(10),
            post_roll: Duration::from_secs(8),
            merge_window: Duration::from_secs(12),
        }
    }
}

impl From<&AppSettings> for AutoClipPolicy {
    fn from(settings: &AppSettings) -> Self {
        Self {
            pre_roll: settings.auto_clip.pre_roll,
            post_roll: settings.auto_clip.post_roll,
            merge_window: settings.auto_clip.merge_window,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AutoClipDecision {
    CreateClip {
        event: GameEvent,
        start: SystemTime,
        end: SystemTime,
    },
    MergeIntoPrevious {
        event: GameEvent,
        previous_event_id: String,
    },
    Duplicate,
}

#[derive(Debug, Clone)]
pub struct EventDebouncer {
    policy: AutoClipPolicy,
    seen_ids: HashMap<String, SystemTime>,
    last_created_event: Option<GameEvent>,
}

impl EventDebouncer {
    pub fn new(policy: AutoClipPolicy) -> Self {
        Self {
            policy,
            seen_ids: HashMap::new(),
            last_created_event: None,
        }
    }

    pub fn decide(&mut self, event: GameEvent) -> AutoClipDecision {
        if self.seen_ids.contains_key(&event.id) {
            return AutoClipDecision::Duplicate;
        }

        self.seen_ids.insert(event.id.clone(), event.occurred_at);

        if let Some(previous) = &self.last_created_event {
            if same_session(previous, &event)
                && within_merge_window(
                    previous.occurred_at,
                    event.occurred_at,
                    self.policy.merge_window,
                )
            {
                return AutoClipDecision::MergeIntoPrevious {
                    event,
                    previous_event_id: previous.id.clone(),
                };
            }
        }

        let start = event
            .occurred_at
            .checked_sub(self.policy.pre_roll)
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let end = event.occurred_at + self.policy.post_roll;
        self.last_created_event = Some(event.clone());

        AutoClipDecision::CreateClip { event, start, end }
    }
}

fn same_session(left: &GameEvent, right: &GameEvent) -> bool {
    left.session_id == right.session_id && left.game_id == right.game_id
}

fn within_merge_window(left: SystemTime, right: SystemTime, window: Duration) -> bool {
    left.duration_since(right)
        .or_else(|_| right.duration_since(left))
        .map(|delta| delta <= window)
        .unwrap_or(false)
}

#[derive(Debug, Clone)]
pub struct RecorderState {
    pub status: RecordingStatus,
    pub detected_game_id: Option<String>,
    pub active_session_id: Option<String>,
}

impl Default for RecorderState {
    fn default() -> Self {
        Self {
            status: RecordingStatus::WaitingForGame,
            detected_game_id: None,
            active_session_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RecorderAction {
    StartedSession {
        session_id: String,
        game_id: String,
    },
    StoppedSession {
        session_id: String,
    },
    CreatedClip {
        clip_id: String,
        segment_count: usize,
    },
    CreatedBookmark {
        event_id: String,
    },
    MergedEvent {
        event_id: String,
        into_event_id: String,
    },
    IgnoredDuplicate {
        event_id: String,
    },
}

#[derive(Debug, Clone)]
pub struct RecorderService {
    pub settings: AppSettings,
    pub paths: LibraryPaths,
    pub state: RecorderState,
    pub replay_buffer: ReplayBuffer,
    pub library: ClipLibrary,
    debouncer: EventDebouncer,
    session_recording: bool,
}

impl RecorderService {
    pub fn new(settings: AppSettings, paths: LibraryPaths) -> Self {
        let policy = AutoClipPolicy::from(&settings);
        Self {
            replay_buffer: ReplayBuffer::new(settings.replay_buffer),
            debouncer: EventDebouncer::new(policy),
            settings,
            paths,
            state: RecorderState::default(),
            library: ClipLibrary::new(),
            session_recording: false,
        }
    }

    pub fn start_for_game(
        &mut self,
        game_id: impl Into<String>,
        now: SystemTime,
    ) -> RecorderAction {
        let game_id = game_id.into();
        let session_id = deterministic_clip_id("session", now);
        self.state.status = RecordingStatus::Buffering;
        self.state.detected_game_id = Some(game_id.clone());
        self.state.active_session_id = Some(session_id.clone());
        RecorderAction::StartedSession {
            session_id,
            game_id,
        }
    }

    pub fn stop(&mut self) -> Option<RecorderAction> {
        let session_id = self.state.active_session_id.take()?;
        self.state.status = RecordingStatus::WaitingForGame;
        self.state.detected_game_id = None;
        self.session_recording = false;
        Some(RecorderAction::StoppedSession { session_id })
    }

    pub fn toggle_session_recording(&mut self) {
        self.session_recording = !self.session_recording;
        self.state.status = if self.session_recording {
            RecordingStatus::RecordingSession
        } else {
            RecordingStatus::Buffering
        };
    }

    pub fn add_segment(&mut self, segment: BufferSegment) {
        self.replay_buffer.push(segment);
    }

    pub fn save_manual_clip(
        &mut self,
        length: Duration,
        now: SystemTime,
    ) -> Option<RecorderAction> {
        let game_id = self.state.detected_game_id.clone()?;
        let session_id = self.state.active_session_id.clone()?;
        let segments = self.replay_buffer.segments_for_window(now, length);
        let clip_id = deterministic_clip_id("manual", now);
        let clip = self.build_clip(
            &clip_id,
            &session_id,
            &game_id,
            length,
            ClipSource::ManualHotkey,
            None,
            now,
        );
        self.library.add_clip(clip);
        self.state.status = RecordingStatus::Processing;
        Some(RecorderAction::CreatedClip {
            clip_id,
            segment_count: segments.len(),
        })
    }

    pub fn handle_game_event(&mut self, event: GameEvent) -> RecorderAction {
        if self.session_recording {
            return RecorderAction::CreatedBookmark { event_id: event.id };
        }

        match self.debouncer.decide(event) {
            AutoClipDecision::CreateClip { event, start, end } => {
                let duration = end.duration_since(start).unwrap_or_default();
                let segments = self.replay_buffer.segments_for_window(end, duration);
                let clip_id = deterministic_clip_id("auto", event.occurred_at);
                let clip = self.build_clip(
                    &clip_id,
                    &event.session_id,
                    &event.game_id,
                    duration,
                    ClipSource::AutoEvent,
                    Some(event.event_type),
                    event.occurred_at,
                );
                self.library.add_clip(clip);
                RecorderAction::CreatedClip {
                    clip_id,
                    segment_count: segments.len(),
                }
            }
            AutoClipDecision::MergeIntoPrevious {
                event,
                previous_event_id,
            } => RecorderAction::MergedEvent {
                event_id: event.id,
                into_event_id: previous_event_id,
            },
            AutoClipDecision::Duplicate => RecorderAction::IgnoredDuplicate {
                event_id: "duplicate".to_string(),
            },
        }
    }

    fn build_clip(
        &self,
        clip_id: &str,
        session_id: &str,
        game_id: &str,
        duration: Duration,
        source: ClipSource,
        event_type: Option<crate::models::GameEventType>,
        created_at: SystemTime,
    ) -> Clip {
        Clip {
            id: clip_id.to_string(),
            session_id: session_id.to_string(),
            game_id: game_id.to_string(),
            path: clip_path(&self.paths, game_id, created_at, clip_id),
            thumbnail_path: Some(self.paths.thumbs_root.join(format!("{clip_id}.jpg"))),
            created_at,
            duration,
            source,
            event_type,
            tags: Vec::new(),
            upload_url: None,
            upload_provider: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{GameEvent, GameEventType};

    #[test]
    fn replay_buffer_prunes_old_segments() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let mut buffer = ReplayBuffer::new(Duration::from_secs(10));

        for index in 0..5 {
            buffer.push(BufferSegment {
                id: format!("seg-{index}"),
                started_at: base + Duration::from_secs(index * 5),
                duration: Duration::from_secs(5),
                path_hint: format!("segment_{index}.mp4"),
            });
        }

        assert_eq!(buffer.len(), 3);
        let ids: Vec<_> = buffer
            .segments_for_window(base + Duration::from_secs(24), Duration::from_secs(10))
            .into_iter()
            .map(|segment| segment.id)
            .collect();
        assert_eq!(ids, vec!["seg-2", "seg-3", "seg-4"]);
    }

    #[test]
    fn debouncer_creates_merges_and_rejects_duplicates() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(2_000);
        let policy = AutoClipPolicy::default();
        let mut debouncer = EventDebouncer::new(policy);
        let first = GameEvent::new("1", "league", "session", GameEventType::Kill, base);
        let second = GameEvent::new(
            "2",
            "league",
            "session",
            GameEventType::Assist,
            base + Duration::from_secs(5),
        );

        assert!(matches!(
            debouncer.decide(first.clone()),
            AutoClipDecision::CreateClip { .. }
        ));
        assert!(matches!(
            debouncer.decide(second),
            AutoClipDecision::MergeIntoPrevious { previous_event_id, .. } if previous_event_id == "1"
        ));
        assert_eq!(debouncer.decide(first), AutoClipDecision::Duplicate);
    }

    #[test]
    fn recorder_service_creates_manual_clip() {
        let root = LibraryPaths::new("root");
        let settings = AppSettings::default_for_root("root");
        let mut service = RecorderService::new(settings, root);
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        service.start_for_game("counter-strike-2", base);
        service.add_segment(BufferSegment {
            id: "segment".to_string(),
            started_at: base,
            duration: Duration::from_secs(5),
            path_hint: "segment.mp4".to_string(),
        });

        let action = service
            .save_manual_clip(Duration::from_secs(60), base + Duration::from_secs(5))
            .expect("manual clip should be created");

        assert!(matches!(
            action,
            RecorderAction::CreatedClip {
                segment_count: 1,
                ..
            }
        ));
        assert_eq!(service.library.all().len(), 1);
    }

    #[test]
    fn recorder_service_creates_bookmark_during_full_session() {
        let root = LibraryPaths::new("root");
        let settings = AppSettings::default_for_root("root");
        let mut service = RecorderService::new(settings, root);
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        service.start_for_game("league-of-legends", base);
        service.toggle_session_recording();
        let event = GameEvent::new(
            "event",
            "league-of-legends",
            "session",
            GameEventType::Kill,
            base,
        );

        let action = service.handle_game_event(event);

        assert_eq!(
            action,
            RecorderAction::CreatedBookmark {
                event_id: "event".to_string()
            }
        );
    }
}
