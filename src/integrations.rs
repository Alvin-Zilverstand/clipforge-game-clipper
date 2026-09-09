use crate::models::{GameEvent, GameEventType};
use serde_json::Value;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt;
use std::time::{Duration, SystemTime};

pub trait GameIntegration {
    fn game_id(&self) -> &'static str;
    fn normalize_event(&self, raw: &RawGameEvent, session_id: &str) -> Option<GameEvent>;
}

#[derive(Debug)]
pub enum IntegrationError {
    Http(String),
    Parse(String),
}

impl fmt::Display for IntegrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(message) => write!(f, "integration HTTP error: {message}"),
            Self::Parse(message) => write!(f, "integration parse error: {message}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawGameEvent {
    pub event_id: String,
    pub name: String,
    pub timestamp: SystemTime,
    pub player: Option<String>,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Default)]
pub struct LeagueIntegration;

#[derive(Debug, Default)]
pub struct LeagueLiveClientPoller {
    seen_event_ids: BTreeSet<u64>,
}

impl LeagueLiveClientPoller {
    pub fn poll_new_events(
        &mut self,
        session_started_at: SystemTime,
    ) -> Result<Vec<RawGameEvent>, IntegrationError> {
        let client = reqwest::blocking::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(Duration::from_millis(900))
            .build()
            .map_err(|error| IntegrationError::Http(error.to_string()))?;
        let body = client
            .get("https://127.0.0.1:2999/liveclientdata/eventdata")
            .send()
            .map_err(|error| IntegrationError::Http(error.to_string()))?
            .error_for_status()
            .map_err(|error| IntegrationError::Http(error.to_string()))?
            .text()
            .map_err(|error| IntegrationError::Http(error.to_string()))?;
        let events = parse_league_live_events(&body, session_started_at)?;
        Ok(events
            .into_iter()
            .filter(|event| {
                event
                    .event_id
                    .parse::<u64>()
                    .map(|id| self.seen_event_ids.insert(id))
                    .unwrap_or(false)
            })
            .collect())
    }
}

pub fn parse_league_live_events(
    body: &str,
    session_started_at: SystemTime,
) -> Result<Vec<RawGameEvent>, IntegrationError> {
    let value: Value =
        serde_json::from_str(body).map_err(|error| IntegrationError::Parse(error.to_string()))?;
    let Some(events) = value.get("Events").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    Ok(events
        .iter()
        .filter_map(|event| {
            let event_id = event.get("EventID")?.as_u64()?;
            let event_name = event.get("EventName")?.as_str()?;
            let event_time = event
                .get("EventTime")
                .and_then(Value::as_f64)
                .unwrap_or_default();
            let mut raw =
                league_event_from_live_client(event_id, event_name, event_time, session_started_at);
            if let Some(killer) = event.get("KillerName").and_then(Value::as_str) {
                raw.player = Some(killer.to_string());
            }
            Some(raw)
        })
        .collect())
}

impl GameIntegration for LeagueIntegration {
    fn game_id(&self) -> &'static str {
        "league-of-legends"
    }

    fn normalize_event(&self, raw: &RawGameEvent, session_id: &str) -> Option<GameEvent> {
        let event_type = match raw.name.as_str() {
            "ChampionKill" => GameEventType::Kill,
            "Multikill" => GameEventType::MultiKill,
            "FirstBrick" | "TurretKilled" | "DragonKill" | "HeraldKill" | "BaronKill" => {
                GameEventType::Objective
            }
            "GameEnd" => GameEventType::MatchWin,
            _ => return None,
        };
        Some(to_game_event(self.game_id(), raw, session_id, event_type))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValveGame {
    CounterStrike2,
    Dota2,
}

#[derive(Debug)]
pub struct ValveGsiIntegration {
    game: ValveGame,
}

impl ValveGsiIntegration {
    pub fn counter_strike_2() -> Self {
        Self {
            game: ValveGame::CounterStrike2,
        }
    }

    pub fn dota_2() -> Self {
        Self {
            game: ValveGame::Dota2,
        }
    }
}

impl GameIntegration for ValveGsiIntegration {
    fn game_id(&self) -> &'static str {
        match self.game {
            ValveGame::CounterStrike2 => "counter-strike-2",
            ValveGame::Dota2 => "dota-2",
        }
    }

    fn normalize_event(&self, raw: &RawGameEvent, session_id: &str) -> Option<GameEvent> {
        let lower = raw.name.to_lowercase();
        let event_type = if lower.contains("kill") {
            GameEventType::Kill
        } else if lower.contains("death") {
            GameEventType::Death
        } else if lower.contains("assist") {
            GameEventType::Assist
        } else if lower.contains("round") && lower.contains("win") {
            GameEventType::RoundWin
        } else if lower.contains("match") && lower.contains("win") {
            GameEventType::MatchWin
        } else if lower.contains("objective") || lower.contains("aegis") || lower.contains("roshan")
        {
            GameEventType::Objective
        } else {
            return None;
        };

        Some(to_game_event(self.game_id(), raw, session_id, event_type))
    }
}

pub fn valve_gsi_raw_events(
    body: &str,
    now: SystemTime,
) -> Result<Vec<RawGameEvent>, IntegrationError> {
    let value: Value =
        serde_json::from_str(body).map_err(|error| IntegrationError::Parse(error.to_string()))?;
    let mut events = Vec::new();

    if let Some(round_phase) = value
        .pointer("/round/phase")
        .and_then(Value::as_str)
        .filter(|phase| *phase == "over")
    {
        events.push(RawGameEvent {
            event_id: format!("round:{round_phase}:{}", millis(now)),
            name: "round_win".to_string(),
            timestamp: now,
            player: None,
            metadata: BTreeMap::new(),
        });
    }

    if let Some(player_state) = value.pointer("/player/state").and_then(Value::as_object) {
        if player_state
            .get("round_kills")
            .and_then(Value::as_u64)
            .unwrap_or_default()
            >= 2
        {
            events.push(RawGameEvent {
                event_id: format!("multi-kill:{}", millis(now)),
                name: "multi_kill".to_string(),
                timestamp: now,
                player: value
                    .pointer("/player/name")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                metadata: BTreeMap::new(),
            });
        }
    }

    Ok(events)
}

fn to_game_event(
    game_id: &str,
    raw: &RawGameEvent,
    session_id: &str,
    event_type: GameEventType,
) -> GameEvent {
    let mut event = GameEvent::new(
        format!("{game_id}:{}", raw.event_id),
        game_id,
        session_id,
        event_type,
        raw.timestamp,
    );
    event.player = raw.player.clone();
    event.metadata = raw.metadata.clone();
    event
}

pub fn league_event_from_live_client(
    event_id: u64,
    event_name: &str,
    game_time_seconds: f64,
    session_started_at: SystemTime,
) -> RawGameEvent {
    RawGameEvent {
        event_id: event_id.to_string(),
        name: event_name.to_string(),
        timestamp: session_started_at + Duration::from_millis((game_time_seconds * 1000.0) as u64),
        player: None,
        metadata: BTreeMap::new(),
    }
}

fn millis(time: SystemTime) -> u128 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_league_kill() {
        let integration = LeagueIntegration;
        let raw = league_event_from_live_client(8, "ChampionKill", 15.0, SystemTime::UNIX_EPOCH);

        let event = integration
            .normalize_event(&raw, "session")
            .expect("event should normalize");

        assert_eq!(event.game_id, "league-of-legends");
        assert_eq!(event.event_type, GameEventType::Kill);
    }

    #[test]
    fn parses_live_client_events() {
        let body = r#"{
            "Events": [
                {"EventID": 1, "EventName": "GameStart", "EventTime": 0},
                {"EventID": 2, "EventName": "ChampionKill", "EventTime": 42.5, "KillerName": "Player"}
            ]
        }"#;

        let events = parse_league_live_events(body, SystemTime::UNIX_EPOCH).expect("events");

        assert_eq!(events.len(), 2);
        assert_eq!(events[1].event_id, "2");
        assert_eq!(events[1].player.as_deref(), Some("Player"));
    }

    #[test]
    fn parses_valve_gsi_multi_kill() {
        let body = r#"{"player":{"name":"Alvin","state":{"round_kills":2}}}"#;

        let events = valve_gsi_raw_events(body, SystemTime::UNIX_EPOCH).expect("events");

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name, "multi_kill");
        assert_eq!(events[0].player.as_deref(), Some("Alvin"));
    }
}
