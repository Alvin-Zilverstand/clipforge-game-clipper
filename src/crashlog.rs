use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_CRASH_FILES: u64 = 5;

pub fn install_panic_hook(log_dir: impl Into<PathBuf>) {
    let log_dir = log_dir.into();
    let _ = fs::create_dir_all(&log_dir);
    let guard_path = log_dir.join("latest-crash.txt");

    std::panic::set_hook(Box::new(move |panic_info| {
        let now = SystemTime::now();
        let timestamp = now
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let payload = panic_info
            .payload()
            .downcast_ref::<&str>()
            .map(|value| value.to_string())
            .or_else(|| {
                panic_info
                    .payload()
                    .downcast_ref::<String>()
                    .map(Clone::clone)
            })
            .unwrap_or_else(|| "unknown panic payload".to_string());
        let location = panic_info
            .location()
            .map(|location| format!("{}:{}", location.file(), location.line()))
            .unwrap_or_else(|| "unknown location".to_string());

        let mut body = String::new();
        body.push_str("ClipForge crash report\n");
        body.push_str(&format!("time: {timestamp}\n"));
        body.push_str(&format!("thread: {}\n", thread_name()));
        body.push_str(&format!("location: {location}\n"));
        body.push_str(&format!("message: {payload}\n"));
        body.push_str("\nBacktrace:\n");
        body.push_str(&backtrace());

        let _ = fs::write(&guard_path, &body);
        let file = log_dir.join(format!("crash-{timestamp}.log"));
        let _ = fs::write(&file, &body);
        prune_crash_files(&log_dir);

        eprint!("{body}");
    }));
}

fn thread_name() -> String {
    std::thread::current()
        .name()
        .map(ToString::to_string)
        .unwrap_or_else(|| "<unnamed>".to_string())
}

fn backtrace() -> String {
    let backtrace = std::backtrace::Backtrace::force_capture();
    format!("{backtrace}")
}

fn prune_crash_files(log_dir: &Path) {
    let Ok(entries) = fs::read_dir(log_dir) else {
        return;
    };
    let mut files = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with("crash-") && name.ends_with(".log"))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    files.sort();
    while files.len() as u64 > MAX_CRASH_FILES {
        let oldest = files.remove(0);
        let _ = fs::remove_file(oldest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panic_hook_writes_crash_file() {
        let log_dir = std::env::temp_dir().join(format!(
            "clipforge-crashlog-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&log_dir).expect("log dir");
        install_panic_hook(&log_dir);

        let result = std::panic::catch_unwind(|| {
            let before = fs::read_dir(&log_dir).expect("read").count();
            assert_eq!(before, 0);
            panic!("intentional-test-panic");
        });
        assert!(result.is_err());
        drop(std::panic::take_hook());

        let crash_files = fs::read_dir(&log_dir)
            .expect("read")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.starts_with("crash-"))
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        assert!(!crash_files.is_empty(), "expected a crash log file");
        let body = fs::read_to_string(&crash_files[0]).expect("crash body");
        assert!(body.contains("intentional-test-panic"));

        let _ = fs::remove_dir_all(log_dir);
    }
}