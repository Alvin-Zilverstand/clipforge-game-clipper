use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioDeviceKind {
    Input,
    SystemLoopback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDevice {
    pub name: String,
    pub kind: AudioDeviceKind,
}

pub fn list_ffmpeg_dshow_audio_inputs(ffmpeg: &Path) -> Vec<AudioDevice> {
    let Ok(output) = Command::new(ffmpeg)
        .args([
            "-hide_banner",
            "-list_devices",
            "true",
            "-f",
            "dshow",
            "-i",
            "dummy",
        ])
        .output()
    else {
        return Vec::new();
    };
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    parse_dshow_audio_devices(&combined)
}

pub fn parse_dshow_audio_devices(output: &str) -> Vec<AudioDevice> {
    output
        .lines()
        .filter_map(|line| {
            if !line.contains("\" (audio)") {
                return None;
            }
            let start = line.find('"')?;
            let rest = &line[start + 1..];
            let end = rest.find('"')?;
            Some(AudioDevice {
                name: rest[..end].to_string(),
                kind: AudioDeviceKind::Input,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dshow_audio_devices() {
        let output = r#"
[in#0 @ 000001] "HP 5MP Camera" (video)
[in#0 @ 000001] "Microfoon (Realtek(R) Audio)" (audio)
[in#0 @ 000001]   Alternative name "@device_cm_{...}"
[in#0 @ 000001] "Headset (Crusher ANC 2)" (audio)
"#;

        let devices = parse_dshow_audio_devices(output);

        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].name, "Microfoon (Realtek(R) Audio)");
        assert_eq!(devices[1].name, "Headset (Crusher ANC 2)");
    }
}
