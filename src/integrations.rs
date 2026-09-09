use crate::models::{GameEvent, GameEventType};
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

pub trait GameIntegration {
    fn game_id(&self) -> &'static str;
    fn normalize_event(&self, raw: &RawGameEvent, session_id: &str) -> Option<GameEvent>;
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
}
