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
        let event_type = if lower.contains("kill") && !lower.contains("multi") && !lower.contains("mvp") {
            GameEventType::Kill
        } else if lower.contains("death") {
            GameEventType::Death
        } else if lower.contains("assist") {
            GameEventType::Assist
        } else if lower.contains("multi_kill") || lower.contains("double_kill") || lower.contains("triple_kill")
            || lower.contains("quad_kill") || lower.contains("penta_kill")
        {
            GameEventType::MultiKill
        } else if lower.contains("round_win") || lower.contains("round_start") {
            GameEventType::RoundWin
        } else if lower.contains("match_end") {
            GameEventType::MatchWin
        } else if lower.contains("match_start") {
            GameEventType::Bookmark
        } else if lower.contains("bomb") {
            GameEventType::Objective
        } else if lower.contains("mvp") {
            GameEventType::MultiKill
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

    // Round events
    let round_number = value
        .pointer("/map/round")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    if let Some(round_phase) = value
        .pointer("/round/phase")
        .and_then(Value::as_str)
    {
        match round_phase {
            "over" => {
                if let Some(win_team) = value.pointer("/round/win_team").and_then(Value::as_str) {
                    events.push(RawGameEvent {
                        event_id: format!("round_win:{win_team}:{}", millis(now)),
                        name: "round_win".to_string(),
                        timestamp: now,
                        player: None,
                        metadata: [
                            ("win_team".to_string(), win_team.to_string()),
                            ("round".to_string(), round_number.to_string()),
                            (
                                "ct_score".to_string(),
                                map_stat_string(&value, "/map/team_ct/score"),
                            ),
                            (
                                "t_score".to_string(),
                                map_stat_string(&value, "/map/team_t/score"),
                            ),
                        ]
                        .into(),
                    });
                }
            }
            "freezeover" => {
                events.push(RawGameEvent {
                    event_id: format!("round_start:{}", millis(now)),
                    name: "round_start".to_string(),
                    timestamp: now,
                    player: None,
                    metadata: [("round".to_string(), round_number.to_string())].into(),
                });
            }
            "live" => {
                // Round is live, could track bomb events. A fresh match is live on round 1.
                if round_number == 1 {
                    events.push(RawGameEvent {
                        event_id: format!("match_start:{}", millis(now)),
                        name: "match_start".to_string(),
                        timestamp: now,
                        player: value
                            .pointer("/player/name")
                            .and_then(Value::as_str)
                            .map(ToString::to_string),
                        metadata: [
                            (
                                "map".to_string(),
                                map_stat_string(&value, "/map/name"),
                            ),
                            (
                                "mode".to_string(),
                                map_stat_string(&value, "/map/mode"),
                            ),
                        ]
                        .into(),
                    });
                }
            }
            _ => {}
        }
    }

    // Bomb events (CS2)
    if let Some(bomb_state) = value.pointer("/round/bomb").and_then(Value::as_str) {
        let bomb_region = map_stat_string(&value, "/map/region");
        let bomb_metadata = |region: String| {
            let mut metadata = BTreeMap::new();
            metadata.insert("round".to_string(), round_number.to_string());
            if !region.is_empty() {
                metadata.insert("region".to_string(), region);
            }
            metadata
        };
        match bomb_state {
            "planted" => {
                events.push(RawGameEvent {
                    event_id: format!("bomb_planted:{}", millis(now)),
                    name: "bomb_planted".to_string(),
                    timestamp: now,
                    player: value
                        .pointer("/player/name")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    metadata: bomb_metadata(bomb_region),
                });
            }
            "defused" => {
                events.push(RawGameEvent {
                    event_id: format!("bomb_defused:{}", millis(now)),
                    name: "bomb_defused".to_string(),
                    timestamp: now,
                    player: value
                        .pointer("/player/name")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    metadata: bomb_metadata(bomb_region),
                });
            }
            "exploded" => {
                events.push(RawGameEvent {
                    event_id: format!("bomb_exploded:{}", millis(now)),
                    name: "bomb_exploded".to_string(),
                    timestamp: now,
                    player: None,
                    metadata: bomb_metadata(bomb_region),
                });
            }
            _ => {}
        }
    }

    // Player state changes for kills/deaths/assists
    if let Some(player_state) = value.pointer("/player/state").and_then(Value::as_object) {
        let player_name = value
            .pointer("/player/name")
            .and_then(Value::as_str)
            .map(ToString::to_string);

        // Multi-kill detection (2+ kills in round)
        if let Some(round_kills) = player_state.get("round_kills").and_then(Value::as_u64) {
            if round_kills >= 2 {
                let kill_count = round_kills;
                events.push(RawGameEvent {
                    event_id: format!("multi_kill:{kill_count}:{}", millis(now)),
                    name: match kill_count {
                        2 => "double_kill".to_string(),
                        3 => "triple_kill".to_string(),
                        4 => "quad_kill".to_string(),
                        5 => "penta_kill".to_string(),
                        _ => "multi_kill".to_string(),
                    },
                    timestamp: now,
                    player: player_name.clone(),
                    metadata: [("kill_count".to_string(), kill_count.to_string())].into(),
                });
            }
        }

        // Track total kills/deaths/assists for the match
        if let Some(kills) = player_state.get("kills").and_then(Value::as_u64) {
            if kills > 0 {
                events.push(RawGameEvent {
                    event_id: format!("kill:{kills}:{}", millis(now)),
                    name: "kill".to_string(),
                    timestamp: now,
                    player: player_name.clone(),
                    metadata: [("total_kills".to_string(), kills.to_string())].into(),
                });
            }
        }

        if let Some(deaths) = player_state.get("deaths").and_then(Value::as_u64) {
            if deaths > 0 {
                events.push(RawGameEvent {
                    event_id: format!("death:{deaths}:{}", millis(now)),
                    name: "death".to_string(),
                    timestamp: now,
                    player: player_name.clone(),
                    metadata: [("total_deaths".to_string(), deaths.to_string())].into(),
                });
            }
        }

        if let Some(assists) = player_state.get("assists").and_then(Value::as_u64) {
            if assists > 0 {
                events.push(RawGameEvent {
                    event_id: format!("assist:{assists}:{}", millis(now)),
                    name: "assist".to_string(),
                    timestamp: now,
                    player: player_name.clone(),
                    metadata: [("total_assists".to_string(), assists.to_string())].into(),
                });
            }
        }

        // MVP/Clutch detection
        if let Some(mvps) = player_state.get("mvps").and_then(Value::as_u64) {
            if mvps > 0 {
                events.push(RawGameEvent {
                    event_id: format!("mvp:{mvps}:{}", millis(now)),
                    name: "mvp".to_string(),
                    timestamp: now,
                    player: player_name.clone(),
                    metadata: [("mvp_count".to_string(), mvps.to_string())].into(),
                });
            }
        }
    }

    // Dota 2 game-state transitions (GSI posts a periodic snapshot; the auto-clip merge
    // window collapses repeated posts into a single event)
    if let Some(game_state) = value.pointer("/map/game_state").and_then(Value::as_str) {
        let game_time = value
            .pointer("/map/game_time")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        match game_state {
            "DOTA_GAMERULES_STATE_GAME_IN_PROGRESS" if game_time < 10 => {
                events.push(RawGameEvent {
                    event_id: format!("dota_match_start:{}", millis(now)),
                    name: "match_start".to_string(),
                    timestamp: now,
                    player: None,
                    metadata: [
                        ("map".to_string(), map_stat_string(&value, "/map/name")),
                        ("game_time".to_string(), game_time.to_string()),
                    ]
                    .into(),
                });
            }
            "DOTA_GAMERULES_STATE_POST_GAME" => {
                events.push(RawGameEvent {
                    event_id: format!("dota_match_end:{}", millis(now)),
                    name: "match_end".to_string(),
                    timestamp: now,
                    player: None,
                    metadata: [
                        (
                            "radiant_score".to_string(),
                            map_stat_string(&value, "/map/radiant_score"),
                        ),
                        (
                            "dire_score".to_string(),
                            map_stat_string(&value, "/map/dire_score"),
                        ),
                        (
                            "win_team".to_string(),
                            map_stat_string(&value, "/map/win_team"),
                        ),
                    ]
                    .into(),
                });
            }
            _ => {}
        }
    }

    // Match end events
    if let Some(phase) = value.pointer("/map/phase").and_then(Value::as_str) {
        if phase == "gameover" {
            events.push(RawGameEvent {
                event_id: format!("match_end:{}", millis(now)),
                name: "match_end".to_string(),
                timestamp: now,
                player: None,
                metadata: BTreeMap::new(),
            });
        }
    }

    // Player activity (connect/disconnect)
    if let Some(steamid) = value.pointer("/player/steamid").and_then(Value::as_str) {
        if let Some(activity) = value.pointer("/player/activity").and_then(Value::as_str) {
            match activity {
                "playing" => {
                    events.push(RawGameEvent {
                        event_id: format!("player_join:{steamid}:{}", millis(now)),
                        name: "player_join".to_string(),
                        timestamp: now,
                        player: value
                            .pointer("/player/name")
                            .and_then(Value::as_str)
                            .map(ToString::to_string),
                        metadata: [("steamid".to_string(), steamid.to_string())].into(),
                    });
                }
                "menu" => {
                    events.push(RawGameEvent {
                        event_id: format!("player_leave:{steamid}:{}", millis(now)),
                        name: "player_leave".to_string(),
                        timestamp: now,
                        player: value
                            .pointer("/player/name")
                            .and_then(Value::as_str)
                            .map(ToString::to_string),
                        metadata: [("steamid".to_string(), steamid.to_string())].into(),
                    });
                }
                _ => {}
            }
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

fn map_stat_string(value: &Value, pointer: &str) -> String {
    match value.pointer(pointer) {
        Some(inner) => inner
            .as_str()
            .map(ToString::to_string)
            .or_else(|| inner.as_u64().map(|n| n.to_string()))
            .or_else(|| inner.as_i64().map(|n| n.to_string()))
            .unwrap_or_default(),
        None => String::new(),
    }
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
        assert_eq!(events[0].name, "double_kill");
        assert_eq!(events[0].player.as_deref(), Some("Alvin"));
    }

    #[test]
    fn parses_valve_gsi_triple_kill() {
        let body = r#"{"player":{"name":"Alvin","state":{"round_kills":3}}}"#;

        let events = valve_gsi_raw_events(body, SystemTime::UNIX_EPOCH).expect("events");

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name, "triple_kill");
        assert_eq!(events[0].player.as_deref(), Some("Alvin"));
    }

    #[test]
    fn parses_valve_gsi_round_metadata_and_match_start() {
        let body = r#"{
            "round": {"phase": "over", "win_team": "CT"},
            "map": {
                "round": 5,
                "phase": "live",
                "name": "de_mirage",
                "mode": "competitive",
                "team_ct": {"score": 3},
                "team_t": {"score": 2}
            }
        }"#;

        let events = valve_gsi_raw_events(body, SystemTime::UNIX_EPOCH).expect("events");

        let round_win = events
            .iter()
            .find(|event| event.name == "round_win")
            .expect("round win event");
        assert_eq!(round_win.metadata.get("win_team").map(String::as_str), Some("CT"));
        assert_eq!(round_win.metadata.get("round").map(String::as_str), Some("5"));
        assert_eq!(
            round_win.metadata.get("ct_score").map(String::as_str),
            Some("3")
        );
        assert_eq!(
            round_win.metadata.get("t_score").map(String::as_str),
            Some("2")
        );
        assert!(!events.iter().any(|event| event.name == "match_start"));
    }

    #[test]
    fn parses_valve_gsi_match_start_on_fresh_live_round() {
        let body = r#"{
            "round": {"phase": "live"},
            "map": {"round": 1, "phase": "live", "name": "de_inferno", "mode": "casual"},
            "player": {"name": "Alvin"}
        }"#;

        let events = valve_gsi_raw_events(body, SystemTime::UNIX_EPOCH).expect("events");

        let match_start = events
            .iter()
            .find(|event| event.name == "match_start")
            .expect("match start event");
        assert_eq!(match_start.player.as_deref(), Some("Alvin"));
        assert_eq!(
            match_start.metadata.get("map").map(String::as_str),
            Some("de_inferno")
        );
        assert_eq!(
            match_start.metadata.get("mode").map(String::as_str),
            Some("casual")
        );
    }

    #[test]
    fn parses_valve_gsi_bomb_plant_region() {
        let body = r#"{
            "round": {"phase": "live", "bomb": "planted"},
            "map": {"round": 2, "region": "b"},
            "player": {"name": "Alvin"}
        }"#;

        let events = valve_gsi_raw_events(body, SystemTime::UNIX_EPOCH).expect("events");

        let bomb = events
            .iter()
            .find(|event| event.name == "bomb_planted")
            .expect("bomb planted event");
        assert_eq!(bomb.player.as_deref(), Some("Alvin"));
        assert_eq!(
            bomb.metadata.get("region").map(String::as_str),
            Some("b")
        );
        assert_eq!(bomb.metadata.get("round").map(String::as_str), Some("2"));
    }

    #[test]
    fn parses_dota2_match_end_with_score_metadata() {
        let body = r#"{
            "map": {
                "game_state": "DOTA_GAMERULES_STATE_POST_GAME",
                "game_time": 2543,
                "name": "dota",
                "radiant_score": 28,
                "dire_score": 14,
                "win_team": "radiant"
            }
        }"#;

        let events = valve_gsi_raw_events(body, SystemTime::UNIX_EPOCH).expect("events");

        let match_end = events
            .iter()
            .find(|event| event.name == "match_end")
            .expect("match_end event");
        assert_eq!(
            match_end.metadata.get("radiant_score").map(String::as_str),
            Some("28")
        );
        assert_eq!(
            match_end.metadata.get("dire_score").map(String::as_str),
            Some("14")
        );
        assert_eq!(
            match_end.metadata.get("win_team").map(String::as_str),
            Some("radiant")
        );
    }

    #[test]
    fn parses_dota2_match_start_on_game_in_progress() {
        let body = r#"{
            "map": {
                "game_state": "DOTA_GAMERULES_STATE_GAME_IN_PROGRESS",
                "game_time": 4,
                "name": "dota"
            }
        }"#;

        let events = valve_gsi_raw_events(body, SystemTime::UNIX_EPOCH).expect("events");

        let start = events
            .iter()
            .find(|event| event.name == "match_start")
            .expect("match_start event");
        assert_eq!(
            start.metadata.get("map").map(String::as_str),
            Some("dota")
        );
        assert_eq!(
            start.metadata.get("game_time").map(String::as_str),
            Some("4")
        );
    }

    #[test]
    fn dota2_ignores_match_start_after_initial_snapshot() {
        let body = r#"{
            "map": {
                "game_state": "DOTA_GAMERULES_STATE_GAME_IN_PROGRESS",
                "game_time": 30
            }
        }"#;

        let events = valve_gsi_raw_events(body, SystemTime::UNIX_EPOCH).expect("events");

        assert!(!events.iter().any(|event| event.name == "match_start"));
    }
}
