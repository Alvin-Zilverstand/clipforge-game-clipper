# ClipForge Implementation Status

## Working Now

- Rust core builds and passes tests.
- Tauri desktop shell checks, runs, and builds as a lightweight Windows WebView app.
- Windows MSI and NSIS installers build successfully.
- FFmpeg installer support is configured: `scripts/prepare-ffmpeg-sidecar.ps1` copies system FFmpeg if present or downloads an essentials build, and Tauri bundles it as `externalBin`.
- The installer bundles `ffmpeg-x86_64-pc-windows-msvc.exe`, so users do not need to install FFmpeg separately.
- Runtime FFmpeg detection checks the bundled sidecar first and then system PATH.
- Tauri commands exist for status, clip listing, manual clip creation, Start Capture, Stop Capture, clip delete, reveal in Explorer, trim, and upload queue marking.
- Start Capture launches FFmpeg `gdigrab` recording with a low-spec default profile: 720p30 at 6 Mbps using Media Foundation H.264 when available.
- Stop Capture asks FFmpeg to finalize gracefully, falls back after a short timeout, and adds the completed full-session MP4 to the local library.
- Clip detail actions are wired to the backend for Trim, Reveal, Upload, and Delete.
- Trim uses FFmpeg copy trimming to create a new local library clip when the source file exists.
- Replay buffer segment pruning and clip-window selection are implemented.
- Manual hotkey mapping is modeled and tested.
- Recorder service can start/stop sessions, retain buffer segments, create manual clip records, and create full-session bookmarks.
- Desktop UI calls Rust commands and displays FFmpeg/capture status.
- Event debouncing supports duplicate detection and merge-window behavior.
- League Live Client Data and Valve GSI-style events normalize into shared `GameEvent` records.
- Local clip library can add, filter, remove, update upload state, and write a tab-separated manifest.
- Storage layer creates the planned folders, sanitizes clip paths, writes placeholder clip files, and cleans oldest temporary files under a size limit.
- Upload adapter boundaries exist for Catbox, Litterbox, Lustful, and custom HTTP.
- Vite TypeScript UI builds and has interactive Library, Recording, Auto Clip, Uploads, and Settings views.

## Not Fully Working Yet

- Record button starts FFmpeg full-session desktop capture and saves the completed MP4 to the library, but instant replay clipping still writes placeholder clip files instead of extracting from the rolling video buffer while recording.
- Native Windows Graphics Capture is still a trait/stub boundary; FFmpeg `gdigrab` is the current MVP bridge.
- WASAPI audio capture is not implemented in native Rust yet; audio capture should be added either through FFmpeg device args or a Rust WASAPI layer.
- Global OS hotkey registration is modeled but not registered with Windows yet.
- SQLite persistence is still represented by a manifest-backed library.
- Live HTTP uploads are adapter stubs; the UI can mark an upload as queued, but real Catbox/Litterbox/custom HTTP transfer still needs an HTTP client implementation.

## Next Engineering Steps

1. Replace placeholder clip writer with FFmpeg segment extraction/concat from the recording file or rolling buffer segments.
2. Add optional FFmpeg audio device selection for system audio and mic.
3. Add SQLite migrations for clips, sessions, settings, and upload jobs.
4. Add global hotkey registration through a Tauri plugin or Windows API wrapper.
5. Add live Catbox/Litterbox/custom HTTP upload implementations.
6. Replace FFmpeg gdigrab with native Windows Graphics Capture for lower overhead after the MVP path is working.
