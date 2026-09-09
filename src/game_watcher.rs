use crate::game_detection::RunningProcess;
use sysinfo::System;

pub fn running_processes() -> Vec<RunningProcess> {
    let mut system = System::new_all();
    system.refresh_all();
    system
        .processes()
        .iter()
        .map(|(pid, process)| RunningProcess {
            pid: pid.as_u32(),
            process_name: process.name().to_string_lossy().to_string(),
            executable_path: process
                .exe()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            window_title: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_snapshot_is_available() {
        let processes = running_processes();
        assert!(!processes.is_empty());
    }
}
