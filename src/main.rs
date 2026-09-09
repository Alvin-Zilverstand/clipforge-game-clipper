use clipforge::game_detection::{default_profiles, detect_game, RunningProcess};
use clipforge::recorder::{BufferSegment, RecorderAction, RecorderService};
use clipforge::settings::AppSettings;
use clipforge::storage::{write_placeholder_mp4, LibraryPaths};
use std::env;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

fn main() {
    let root = env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("work")
        .join("clipforge-library");
    let settings = AppSettings::default_for_root(&root);
    let paths = LibraryPaths::new(&root);

    if let Err(error) = paths.ensure() {
        eprintln!("Could not prepare local clip library: {error}");
        std::process::exit(1);
    }

    let sample_processes = vec![RunningProcess {
        pid: 730,
        process_name: "cs2.exe".to_string(),
        executable_path: "C:/Program Files (x86)/Steam/steamapps/common/Counter-Strike Global Offensive/game/bin/win64/cs2.exe".to_string(),
        window_title: Some("Counter-Strike 2".to_string()),
    }];
    let detected = detect_game(&sample_processes, &default_profiles());
    let mut service = RecorderService::new(settings, paths.clone());
    let now = SystemTime::now();

    if let Some(game) = detected {
        println!(
            "Sample game detection: {} ({})",
            game.display_name, game.game_id
        );
        println!("{:?}", service.start_for_game(game.game_id, now));

        for index in 0..12 {
            service.add_segment(BufferSegment {
                id: format!("segment-{index}"),
                started_at: now + Duration::from_secs(index * 5),
                duration: Duration::from_secs(5),
                path_hint: format!("segment_{index}.mp4"),
            });
        }

        let clip_action =
            service.save_manual_clip(Duration::from_secs(60), now + Duration::from_secs(60));
        if let Some(RecorderAction::CreatedClip { clip_id, .. }) = clip_action {
            if let Some(clip) = service.library.all().iter().find(|clip| clip.id == clip_id) {
                if let Err(error) = write_placeholder_mp4(&clip.path, &clip.id) {
                    eprintln!("Could not write placeholder clip: {error}");
                }
            }
            println!("Created manual clip: {clip_id}");
        }
    } else {
        println!("Sample game detection: waiting for game");
    }

    let manifest_path = root.join("library.tsv");
    if let Err(error) = service.library.save_manifest(&manifest_path) {
        eprintln!("Could not save clip manifest: {error}");
    }

    println!("ClipForge recorder scaffold");
    println!("Library root: {}", root.display());
    println!("Manifest: {}", manifest_path.display());
    println!("Replay segments retained: {}", service.replay_buffer.len());
}
