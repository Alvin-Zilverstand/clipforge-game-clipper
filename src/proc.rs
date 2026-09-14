use std::path::Path;
use std::process::Command;

/// Builds a `Command` that never shows a console window on Windows. FFmpeg and other
/// console subprocesses otherwise flash a terminal window each time they start from a
/// GUI (Tauri) process, which looks like the app constantly opening and closing.
pub fn hidden_command(program: impl AsRef<Path>) -> Command {
    let mut command = Command::new(program.as_ref());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}