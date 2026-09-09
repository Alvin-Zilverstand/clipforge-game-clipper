# ClipForge Implementation Status

## Working Now

- Rust core builds and passes tests.
- Tauri desktop shell checks and builds as a lightweight Windows WebView2 app, not a browser-hosted web app.
- Windows MSI and NSIS installers build successfully.
- FFmpeg installer support is configured: `scripts/prepare-ffmpeg-sidecar.ps1` copies system FFmpeg if present or downloads an essentials build, and Tauri bundles it as `externalBin`.
- The installer bundles `ffmpeg-x86_64-pc-windows-msvc.exe`, so users do not need to install FFmpeg separately.
- Runtime FFmpeg detection checks the bundled sidecar first and then system PATH.
- Tauri commands exist for status, live game detection, clip listing, manual clip creation, auto-clip polling, Start Capture, Stop Capture, clip delete, reveal in Explorer, trim, upload, and settings updates.
- Start Capture launches a rolling FFmpeg segmented replay recorder with a low-spec default profile: 720p30 at 6 Mbps using Media Foundation H.264 when available.
- Capture prefers FFmpeg's Windows Desktop Duplication `ddagrab` filter when available and falls back to `gdigrab`; a local smoke test produced a real MP4 segment with `ddagrab`.
- Stop Capture asks FFmpeg to finalize gracefully, falls back after a short timeout, concatenates recorded segments, and adds the completed full-session MP4 to the local library.
- Manual clips now extract from the rolling segment buffer instead of writing placeholder MP4 files.
- Auto clips now use the same segment extraction/thumbnail pipeline as manual clips.
- Clip detail actions are wired to the backend for Trim, Reveal, Upload, and Delete.
- Trim uses FFmpeg copy trimming to create a new local library clip when the source file exists.
- Clip thumbnails are generated with FFmpeg and rendered in the library when available.
- Clip detail preview renders the local MP4 through Tauri's asset URL conversion when available.
- Replay buffer segment pruning and clip-window selection are implemented.
- Global desktop hotkeys are registered with `tauri-plugin-global-shortcut`: F8 saves 60 seconds, Shift+F8 saves 30 seconds, and Alt+F7 toggles recording while the app is running.
- Recorder service can start/stop sessions, retain buffer segments, create manual clip records, and create full-session bookmarks.
- Desktop UI calls Rust commands and displays FFmpeg/capture status.
- Event debouncing supports duplicate detection and merge-window behavior.
- League Live Client Data events are polled from the local Riot endpoint while recording and normalize into shared `GameEvent` records.
- A localhost Valve GSI receiver on `127.0.0.1:49321` accepts CS2/Dota-style event posts and queues normalized event clips.
- Local clip library can add, filter, remove, update upload state, write a tab-separated manifest, and persist clip metadata in SQLite.
- Versioned local settings are saved to `settings.json`; replay buffer length and mic toggle are wired from the UI to Rust.
- Storage layer creates the planned folders, sanitizes clip paths, keeps placeholder helpers for tests/demo paths, and cleans oldest temporary files under a size limit.
- Upload adapters perform live HTTP uploads for Catbox, Litterbox, and custom multipart endpoints; Lustful remains blocked behind unknown API details.
- The UI Upload button currently performs an explicit Catbox upload for the selected clip and saves the returned URL.
- Vite TypeScript UI builds and has interactive Library, Recording, Auto Clip, Uploads, and Settings views.

## Not Fully Working Yet

- Native Windows Graphics Capture is still a trait/stub boundary; FFmpeg `gdigrab` is the current MVP bridge.
- WASAPI/system-audio capture is enabled only when the selected FFmpeg build exposes a WASAPI input device; the current local FFmpeg build does not, so video capture remains enabled while system-audio capture is marked unavailable.
- Capture has not yet been manually QA-tested on real NVIDIA/AMD/Intel gaming systems.
- Upload provider configuration UI is not complete; Catbox is live from the default Upload action, but Litterbox/custom HTTP need UI selection and configuration.
- Valve GSI receiver is a lightweight MVP receiver; config templates can be generated from the Auto Clip screen, but richer event mapping per game is still needed.
- Auto start/stop is conservative: live process detection updates the current game/session state, but capture still starts from explicit user action/hotkey.

## Next Engineering Steps

1. Replace the FFmpeg Desktop Duplication/GDI bridge with native Windows Graphics Capture for lower overhead and better game/window targeting.
2. Add native Rust WASAPI capture/mixing and audio device selection.
3. Add upload provider configuration UI for Litterbox/custom HTTP.
4. Generate CS2/Dota GSI config files and expand event mapping.
5. Add installer/onboarding screens for privacy, capture source, storage, and hotkeys.
6. Run manual QA on Windows gaming hardware and tune CPU/GPU overhead.
