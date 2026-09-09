use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const GSI_ENDPOINT: &str = "http://127.0.0.1:49321";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenGsiConfig {
    pub game_id: String,
    pub path: PathBuf,
}

pub fn write_gsi_config_templates(root: &Path) -> io::Result<Vec<WrittenGsiConfig>> {
    let config_root = root.join("integrations").join("valve-gsi");
    fs::create_dir_all(&config_root)?;
    let cs2_path = config_root.join("gamestate_integration_clipforge_cs2.cfg");
    let dota_path = config_root.join("gamestate_integration_clipforge_dota2.cfg");
    fs::write(&cs2_path, cs2_config())?;
    fs::write(&dota_path, dota2_config())?;
    Ok(vec![
        WrittenGsiConfig {
            game_id: "counter-strike-2".to_string(),
            path: cs2_path,
        },
        WrittenGsiConfig {
            game_id: "dota-2".to_string(),
            path: dota_path,
        },
    ])
}

pub fn cs2_config() -> String {
    format!(
        r#""ClipForge"
{{
  "uri" "{GSI_ENDPOINT}"
  "timeout" "5.0"
  "buffer"  "0.1"
  "throttle" "0.1"
  "heartbeat" "30.0"
  "data"
  {{
    "provider" "1"
    "map" "1"
    "round" "1"
    "player_id" "1"
    "player_state" "1"
    "player_match_stats" "1"
  }}
}}
"#
    )
}

pub fn dota2_config() -> String {
    format!(
        r#""ClipForge"
{{
  "uri" "{GSI_ENDPOINT}"
  "timeout" "5.0"
  "buffer"  "0.1"
  "throttle" "0.1"
  "heartbeat" "30.0"
  "data"
  {{
    "provider" "1"
    "map" "1"
    "player" "1"
    "hero" "1"
    "abilities" "1"
  }}
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_reference_local_receiver() {
        assert!(cs2_config().contains(GSI_ENDPOINT));
        assert!(dota2_config().contains(GSI_ENDPOINT));
    }

    #[test]
    fn writes_templates() {
        let root = std::env::temp_dir().join(format!(
            "clipforge-gsi-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));

        let written = write_gsi_config_templates(&root).expect("write");

        assert_eq!(written.len(), 2);
        assert!(written.iter().all(|config| config.path.exists()));
        let _ = fs::remove_dir_all(root);
    }
}
