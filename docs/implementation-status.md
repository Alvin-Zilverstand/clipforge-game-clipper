# ClipForge Implementation Status

## Working Now

- Rust core builds and passes tests.
- Tauri desktop shell checks and builds as a lightweight Windows WebView2 app, not a browser-hosted web app.
- Windows MSI and NSIS installers build successfully.
- FFmpeg installer support is configured: `scripts/prepare-ffmpeg-sidecar.ps1` copies system FFmpeg if present or downloads an essentials build, and Tauri bundles it as `externalBin`.
- The installer bundles `ffmpeg-x86_64-pc-windows-msvc.exe`, so users do not need to install FFmpeg separately.
- Runtime FFmpeg detection checks the bundled sidecar first and then system PATH.
- Tauri commands exist for status, live game detection, clip listing, manual clip creation, auto-clip polling, Start Capture, Stop Capture, clip delete, reveal in Explorer, trim, upload, and settings updates.
- Start Capture launches a rolling segmented replay recorder with a low-spec default profile: 720p30 at 6 Mbps using H.264 when available.
- Recording now prefers native Windows Graphics Capture with Windows Media Foundation H.264; it targets detected game windows when possible and falls back to the primary monitor.
- Native Windows Graphics Capture can now embed native WASAPI system loopback audio and default mic capture; ignored local smoke tests cover standalone loopback, standalone mic, WGC plus loopback, and WGC plus mic.
- Audio-enabled capture falls back to FFmpeg's Windows Desktop Duplication `ddagrab` filter when available and then `gdigrab`; a local smoke test produced a real MP4 segment with `ddagrab`.
- Stop Capture asks FFmpeg to finalize gracefully, falls back after a short timeout, concatenates recorded segments, and adds the completed full-session MP4 to the local library.
- Manual clips now extract from the rolling segment buffer instead of writing placeholder MP4 files.
- Auto clips now use the same segment extraction/thumbnail pipeline as manual clips.
- Clip detail actions are wired to the backend for Trim, Reveal, Upload, Delete, metadata updates, and export copy.
- Clip records now support editable titles and tags, and the SQLite schema migrates older libraries to include clip titles.
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
- A backend worker loop now refreshes game detection and drains auto-clip integrations while the desktop app is running, independent of the currently selected UI view.
- Local clip library can add, filter, remove, update upload state, write a tab-separated manifest, and persist clip metadata in SQLite.
- Versioned local settings are saved to `settings.json`; replay buffer length, audio toggles, mic device, auto-record, upload settings, and auto-clip event toggles are wired from the UI to Rust.
- Storage layer creates the planned folders, sanitizes clip paths, provides test clip builders for unit coverage, and cleans oldest temporary files under a size limit.
- Upload adapters perform live HTTP uploads for Catbox, Litterbox, and custom multipart endpoints; Lustful remains blocked behind unknown API details.
- The UI Upload button uses the selected upload provider and saved provider configuration, including Catbox userhash, Litterbox expiry, custom endpoint, response path, and custom headers.
- Auto-upload is persisted and, when enabled, attempts upload after manual and event clips while preserving local clips on failure.
- Upload attempts are recorded in SQLite upload history.
- Vite TypeScript UI builds and has interactive Library, Recording, Auto Clip, Uploads, and Settings views.
- Library search, event/upload filters, and grid/list switching are functional in the UI.
- User-visible notices are shown for major actions and failures instead of relying only on console output.

## Not Fully Working Yet

- Native Windows Graphics Capture is wired for rolling replay capture with native audio; resolution downscaling still needs more work.
- Native WASAPI audio is mixed into a single AAC track for MVP clips; separate audio tracks and per-process audio capture are still future work.
- Capture has not yet been manually QA-tested on real NVIDIA/AMD/Intel gaming systems.
- Upload retry controls and a visible upload-history screen are not complete yet.
- Valve GSI receiver is a lightweight MVP receiver; config templates can be generated from the Auto Clip screen, but richer event mapping per game is still needed.
- Auto start/stop is conservative but live: when auto-record is enabled, supported detected games can start recording and stop when the game disappears.
- Trim still uses timestamp text inputs instead of draggable timeline handles.
- Hotkeys are still hardcoded; editable hotkey registration is future work.
- Screenshot capture is still not implemented.
- Capture source, quality, storage cap, excluded windows, and per-game override UI are still incomplete.
- Storage critical-low-disk guardrails are not fully wired into recording start/stop behavior.
- Recorder service still runs inside the Tauri process instead of a separate background service process.
- Installer onboarding, auto-update, and crash logging are still not implemented.

## Next Engineering Steps

1. Extend native Windows Graphics Capture with resolution downscaling and richer capture-source selection.
2. Add separate audio tracks and optional per-process loopback capture.
3. Expand CS2/Dota GSI event mapping beyond the current MVP event parser.
4. Add installer/onboarding screens for privacy, capture source, storage, and hotkeys.
5. Run manual QA on Windows gaming hardware and tune CPU/GPU overhead.
