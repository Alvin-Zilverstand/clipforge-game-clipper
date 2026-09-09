# ClipForge Architecture

ClipForge is a Windows-first, local-first game clipping app scaffold. The code in this repository implements the testable core and app shell boundaries for a Medal/FTHR-style product without using game memory injection or anti-cheat-sensitive hooks.

## Runtime Shape

- Tauri shell hosts the frontend UI and calls Rust commands.
- Rust recorder service owns capture lifecycle, replay buffer, clip rendering, game integrations, upload adapters, and local storage.
- Windows capture should be implemented behind `CaptureBackend` using Windows Graphics Capture first, then DXGI/Desktop Duplication fallback.
- Encoding should prefer hardware H.264 through Media Foundation, with software H.264 as fallback.
- Audio should use WASAPI loopback for system/game audio and WASAPI input for mic, with mic disabled by default.

## Implemented Core

- Replay buffer window selection and pruning.
- Auto-clip event duplicate detection and merge-window policy.
- Game profile detection against process/window facts.
- Normalized event model for League Live Client Data and Valve GSI-style feeds.
- Upload adapter contracts for Catbox, Litterbox, Lustful, and custom HTTP.
- Local library path layout for clips, thumbnails, sessions, and buffer files.

## Next Native Work

1. Wire Tauri commands to the Rust library.
2. Replace capture stubs with Windows Graphics Capture and Media Foundation implementations.
3. Add SQLite persistence.
4. Add an HTTP client dependency for live upload adapters.
5. Package with installer and auto-update strategy.
