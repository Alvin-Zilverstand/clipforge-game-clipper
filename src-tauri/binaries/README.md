# Tauri Sidecar Binaries

This folder is intentionally kept out of Git except for this note and `.gitkeep`.

`scripts/prepare-ffmpeg-sidecar.ps1` populates the required FFmpeg sidecar as:

`ffmpeg-x86_64-pc-windows-msvc.exe`

The binary is bundled into installer builds by Tauri, but it should not be committed to the repository.
