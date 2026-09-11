use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameEventType {
    Kill,
    Death,
    Assist,
    RoundWin,
    MatchWin,
    Objective,
    MultiKill,
    Bookmark,
    Unknown(String),
}

impl fmt::Display for GameEventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Kill => "kill",
            Self::Death => "death",
            Self::Assist => "assist",
            Self::RoundWin => "round_win",
            Self::MatchWin => "match_win",
            Self::Objective => "objective",
            Self::MultiKill => "multi_kill",
            Self::Bookmark => "bookmark",
            Self::Unknown(value) => value,
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GameEvent {
    pub id: String,
    pub game_id: String,
    pub session_id: String,
    pub event_type: GameEventType,
    pub occurred_at: SystemTime,
    pub confidence: f32,
    pub player: Option<String>,
    pub metadata: BTreeMap<String, String>,
}

impl GameEvent {
    pub fn new(
        id: impl Into<String>,
        game_id: impl Into<String>,
        session_id: impl Into<String>,
        event_type: GameEventType,
        occurred_at: SystemTime,
    ) -> Self {
        Self {
            id: id.into(),
            game_id: game_id.into(),
            session_id: session_id.into(),
            event_type,
            occurred_at,
            confidence: 1.0,
            player: None,
            metadata: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipSource {
    ManualHotkey,
    AutoEvent,
    FullSessionBookmark,
    Imported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadProvider {
    Catbox,
    Litterbox,
    Lustful,
    CustomHttp,
}

impl fmt::Display for UploadProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Catbox => "catbox",
            Self::Litterbox => "litterbox",
            Self::Lustful => "lustful",
            Self::CustomHttp => "custom_http",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clip {
    pub id: String,
    pub title: Option<String>,
    pub session_id: String,
    pub game_id: String,
    pub path: PathBuf,
    pub thumbnail_path: Option<PathBuf>,
    pub created_at: SystemTime,
    pub duration: Duration,
    pub source: ClipSource,
    pub event_type: Option<GameEventType>,
    pub tags: Vec<String>,
    pub upload_url: Option<String>,
    pub upload_provider: Option<UploadProvider>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordingStatus {
    WaitingForGame,
    Buffering,
    RecordingSession,
    Clipping,
    Processing,
    StorageLow,
    Error(String),
}
