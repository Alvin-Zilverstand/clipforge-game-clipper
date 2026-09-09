#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningProcess {
    pub pid: u32,
    pub process_name: String,
    pub executable_path: String,
    pub window_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameProfile {
    pub game_id: String,
    pub display_name: String,
    pub process_names: Vec<String>,
    pub window_title_contains: Vec<String>,
    pub auto_record: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedGame {
    pub game_id: String,
    pub display_name: String,
    pub process: RunningProcess,
}

pub fn detect_game(processes: &[RunningProcess], profiles: &[GameProfile]) -> Option<DetectedGame> {
    profiles.iter().find_map(|profile| {
        processes.iter().find_map(|process| {
            let process_matches = profile
                .process_names
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&process.process_name));
            let title_matches = process
                .window_title
                .as_deref()
                .map(|title| {
                    profile
                        .window_title_contains
                        .iter()
                        .any(|needle| title.to_lowercase().contains(&needle.to_lowercase()))
                })
                .unwrap_or(false);

            if process_matches || title_matches {
                Some(DetectedGame {
                    game_id: profile.game_id.clone(),
                    display_name: profile.display_name.clone(),
                    process: process.clone(),
                })
            } else {
                None
            }
        })
    })
}

pub fn default_profiles() -> Vec<GameProfile> {
    vec![
        GameProfile {
            game_id: "league-of-legends".to_string(),
            display_name: "League of Legends".to_string(),
            process_names: vec!["League of Legends.exe".to_string()],
            window_title_contains: vec!["League of Legends".to_string()],
            auto_record: true,
        },
        GameProfile {
            game_id: "counter-strike-2".to_string(),
            display_name: "Counter-Strike 2".to_string(),
            process_names: vec!["cs2.exe".to_string()],
            window_title_contains: vec!["Counter-Strike 2".to_string()],
            auto_record: true,
        },
        GameProfile {
            game_id: "dota-2".to_string(),
            display_name: "Dota 2".to_string(),
            process_names: vec!["dota2.exe".to_string()],
            window_title_contains: vec!["Dota 2".to_string()],
            auto_record: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_by_process_name() {
        let process = RunningProcess {
            pid: 42,
            process_name: "cs2.exe".to_string(),
            executable_path: "C:/Steam/cs2.exe".to_string(),
            window_title: None,
        };

        let detected =
            detect_game(&[process], &default_profiles()).expect("game should be detected");
        assert_eq!(detected.game_id, "counter-strike-2");
    }
}
