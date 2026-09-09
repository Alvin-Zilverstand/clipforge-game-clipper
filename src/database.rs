use crate::models::{Clip, ClipSource, GameEventType, UploadProvider};
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub struct ClipDatabase {
    connection: Connection,
}

impl ClipDatabase {
    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        }
        let connection = Connection::open(path)?;
        let database = Self { connection };
        database.init()?;
        Ok(database)
    }

    pub fn init(&self) -> rusqlite::Result<()> {
        self.connection.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS clips (
                id TEXT PRIMARY KEY NOT NULL,
                session_id TEXT NOT NULL,
                game_id TEXT NOT NULL,
                path TEXT NOT NULL,
                thumbnail_path TEXT,
                created_at_ms INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                source TEXT NOT NULL,
                event_type TEXT,
                tags_json TEXT NOT NULL,
                upload_url TEXT,
                upload_provider TEXT
            );
            CREATE INDEX IF NOT EXISTS clips_created_at_idx ON clips(created_at_ms DESC);
            CREATE INDEX IF NOT EXISTS clips_game_id_idx ON clips(game_id);
            ",
        )
    }

    pub fn load_clips(&self) -> rusqlite::Result<Vec<Clip>> {
        let mut statement = self.connection.prepare(
            "
            SELECT id, session_id, game_id, path, thumbnail_path, created_at_ms, duration_ms,
                   source, event_type, tags_json, upload_url, upload_provider
            FROM clips
            ORDER BY created_at_ms DESC
            ",
        )?;

        let rows = statement.query_map([], |row| {
            let tags_json: String = row.get(9)?;
            let tags = serde_json::from_str::<Vec<String>>(&tags_json).unwrap_or_default();
            let event_type = row
                .get::<_, Option<String>>(8)?
                .map(|value| parse_event_type(&value));
            let upload_provider = row
                .get::<_, Option<String>>(11)?
                .and_then(|value| parse_upload_provider(&value));

            Ok(Clip {
                id: row.get(0)?,
                session_id: row.get(1)?,
                game_id: row.get(2)?,
                path: PathBuf::from(row.get::<_, String>(3)?),
                thumbnail_path: row.get::<_, Option<String>>(4)?.map(PathBuf::from),
                created_at: millis_to_system_time(row.get(5)?),
                duration: Duration::from_millis(row.get::<_, i64>(6)?.max(0) as u64),
                source: parse_clip_source(&row.get::<_, String>(7)?),
                event_type,
                tags,
                upload_url: row.get(10)?,
                upload_provider,
            })
        })?;

        rows.collect()
    }

    pub fn upsert_clip(&self, clip: &Clip) -> rusqlite::Result<()> {
        let tags_json = serde_json::to_string(&clip.tags).unwrap_or_else(|_| "[]".to_string());
        self.connection.execute(
            "
            INSERT INTO clips (
                id, session_id, game_id, path, thumbnail_path, created_at_ms, duration_ms,
                source, event_type, tags_json, upload_url, upload_provider
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            ON CONFLICT(id) DO UPDATE SET
                session_id = excluded.session_id,
                game_id = excluded.game_id,
                path = excluded.path,
                thumbnail_path = excluded.thumbnail_path,
                created_at_ms = excluded.created_at_ms,
                duration_ms = excluded.duration_ms,
                source = excluded.source,
                event_type = excluded.event_type,
                tags_json = excluded.tags_json,
                upload_url = excluded.upload_url,
                upload_provider = excluded.upload_provider
            ",
            params![
                clip.id,
                clip.session_id,
                clip.game_id,
                clip.path.display().to_string(),
                clip.thumbnail_path
                    .as_ref()
                    .map(|path| path.display().to_string()),
                system_time_to_millis(clip.created_at),
                clip.duration.as_millis().min(i64::MAX as u128) as i64,
                clip_source_name(&clip.source),
                clip.event_type.as_ref().map(ToString::to_string),
                tags_json,
                clip.upload_url,
                clip.upload_provider.as_ref().map(upload_provider_name),
            ],
        )?;
        Ok(())
    }

    pub fn delete_clip(&self, clip_id: &str) -> rusqlite::Result<()> {
        self.connection
            .execute("DELETE FROM clips WHERE id = ?1", params![clip_id])?;
        Ok(())
    }
}

fn parse_clip_source(value: &str) -> ClipSource {
    match value {
        "manual_hotkey" => ClipSource::ManualHotkey,
        "auto_event" => ClipSource::AutoEvent,
        "full_session_bookmark" => ClipSource::FullSessionBookmark,
        _ => ClipSource::Imported,
    }
}

fn clip_source_name(source: &ClipSource) -> &'static str {
    match source {
        ClipSource::ManualHotkey => "manual_hotkey",
        ClipSource::AutoEvent => "auto_event",
        ClipSource::FullSessionBookmark => "full_session_bookmark",
        ClipSource::Imported => "imported",
    }
}

fn parse_event_type(value: &str) -> GameEventType {
    match value {
        "kill" => GameEventType::Kill,
        "death" => GameEventType::Death,
        "assist" => GameEventType::Assist,
        "round_win" => GameEventType::RoundWin,
        "match_win" => GameEventType::MatchWin,
        "objective" => GameEventType::Objective,
        "multi_kill" => GameEventType::MultiKill,
        "bookmark" => GameEventType::Bookmark,
        value => GameEventType::Unknown(value.to_string()),
    }
}

fn parse_upload_provider(value: &str) -> Option<UploadProvider> {
    match value {
        "catbox" => Some(UploadProvider::Catbox),
        "litterbox" => Some(UploadProvider::Litterbox),
        "lustful" => Some(UploadProvider::Lustful),
        "custom_http" => Some(UploadProvider::CustomHttp),
        _ => None,
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

fn system_time_to_millis(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn millis_to_system_time(millis: i64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(millis.max(0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_clip_records() {
        let db_path = std::env::temp_dir().join(format!(
            "clipforge-db-{}.sqlite",
            system_time_to_millis(SystemTime::now())
        ));
        let database = ClipDatabase::open(&db_path).expect("database");
        let clip = Clip {
            id: "clip-1".to_string(),
            session_id: "session-1".to_string(),
            game_id: "counter-strike-2".to_string(),
            path: PathBuf::from("C:/clips/clip-1.mp4"),
            thumbnail_path: Some(PathBuf::from("C:/clips/clip-1.jpg")),
            created_at: UNIX_EPOCH + Duration::from_secs(10),
            duration: Duration::from_secs(60),
            source: ClipSource::ManualHotkey,
            event_type: Some(GameEventType::Kill),
            tags: vec!["clutch".to_string()],
            upload_url: Some("https://example.test/clip.mp4".to_string()),
            upload_provider: Some(UploadProvider::CustomHttp),
        };

        database.upsert_clip(&clip).expect("upsert");
        let loaded = database.load_clips().expect("load");

        assert_eq!(loaded, vec![clip]);
        database.delete_clip("clip-1").expect("delete");
        assert!(database.load_clips().expect("load empty").is_empty());
        let _ = std::fs::remove_file(db_path);
    }
}
