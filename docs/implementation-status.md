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
- The Uploads screen shows recent upload history with retry buttons for failed uploads, refreshed from the backend on load and after uploads.
- Screenshot capture is implemented: F9 takes a PNG screenshot via bundled FFmpeg into `~/Pictures/ClipForge`, the recording bar has a Screenshot button, and the Library shows recent screenshots with an Open-in-Explorer action.
- Storage guardrails are wired: recording refuses to start below 2 GB free and a backend worker stops an active recording and flags `StorageLow` when available disk drops to critical; the recording bar shows free disk space and a low-disk warning.
- Panic reporting writes crash reports with stack traces to the logs folder (`library/logs`), prunes old entries, and the Settings screen can open the logs folder.
- Native Windows Graphics Capture now downscales the captured window/monitor to the configured resolution cap (e.g. 720p) with even dimensions while preserving aspect ratio; covered by unit tests.
- Capture quality and storage settings are exposed: the Settings screen can change resolution, FPS, bitrate, buffer storage cap, excluded window titles, and the separate-audio-tracks toggle via the `set_capture_settings` command, and current values are shown in DesktopStatus.
- Per-game capture quality overrides can be set from the Recording screen per detected game title, saved in settings, and applied automatically by the recorder when that game is active.
- The configured storage cap is enforced by a backend worker that prunes the oldest rolling buffer segments once the buffer exceeds `storage_limit_gb`.
- SQLite schema uses versioned migrations tracked via `PRAGMA user_version`; legacy databases (missing tables or the `title` column) are repaired and upgraded in place, with migration tests.
- Valve GSI event mapping enriches CS2 round, bomb, and match-start events with round number, map scores, and plant-region metadata; Dota 2 game-state transitions map to match-start bookmarks and match-end events carrying final radiant/dire scores. These events are unit tested against realistic JSON payloads.
- A GitHub Actions release workflow (`.github/workflows/release.yml`) builds MSI and NSIS installers on tag push or manual dispatch and publishes them to a GitHub Release, ready for the auto-updater to pick up.
- Auto-update checks the GitHub repo releases (`Alvin-Zilverstand/clipforge-game-clipper`) for the latest MSI/NSIS installer, compares it against the running version, downloads it to temp, launches the installer, and exits the app to finish; version comparison, asset selection, and parsing are unit tested plus one ignored live test against the GitHub API.
- A GitHub Actions release workflow publishes MSI/NSIS assets on tag push; auto-update will pick up newer releases automatically once the repo ships a tag newer than the running app version.
- The Settings screen shows the current version, checks for updates, and offers a Download & install button when a newer release exists.
- First-run onboarding wizard walks new installs through privacy/audio, capture-source, storage, and hotkey setup before showing the main app; it uses the defaults already saved in `settings.json` so existing users are unaffected.
- The default clip library root is `~/Documents/ClipForge` (rather than the process working directory), and a first-run migration moves any legacy `clipforge-library` folder found in the CWD into the new location while rewriting stored clip paths in the SQLite database.
- Capture capabilities (bundled FFmpeg discovery, WASAPI/ddagrab support) are detected once at startup and cached, so background workers (`refresh_detected_game` every 2s, UI status poll every 3s) never spawn FFmpeg; all FFmpeg/console subprocesses are launched with a hidden console window on Windows, preventing the app from freezing and flashing a terminal repeatedly.
- Vite TypeScript UI builds and has interactive Library, Recording, Auto Clip, Uploads, and Settings views.
- Tauri's asset protocol is enabled and scoped to `$DOCUMENT/ClipForge/**` and `$PICTURE/ClipForge/**`, so `convertFileSrc` URLs load for clip thumbnails, video previews, and screenshot images (previously returned blocked asset responses).
- Screenshot thumbnails use `object-fit: contain` so the full screenshot is shown without cropping.
- An opt-in "Minimize to tray" setting keeps ClipForge running in the system tray when the main window is closed; closing the window hides it, and the tray menu offers "Open ClipForge" to restore it or "Quit ClipForge" to exit fully. The toggle is under Settings > Tray and defaults to off.
- Library search, event/upload filters, and grid/list switching are functional in the UI.
- User-visible notices are shown for major actions and failures instead of relying only on console output.
- Versions are bumped to `0.1.1` across root `Cargo.toml`, `src-tauri/Cargo.toml`, `tauri.conf.json`, `package.json`, and the frontend `appVersion` constant.
- The updater now requires both a newer release and a published installer asset before offering a download; the UI always shows "Check for updates" and only shows a Download button when an installer is available.
- The clip folder can be changed at runtime via `set_clip_directory`; the library root and asset-protocol scope are rebuilt without restarting the app.
- An optional auto-prune toggle deletes the oldest non-manual clips once the saved library exceeds the configured storage limit.
- Free disk space and storage-limit status are cached for 10 seconds, reducing repeated FS calls from the background and UI polls.
- A RAM-backed clip buffer (512 MB cap) keeps recent segments in memory and merges them with disk segments when manual, auto, or hotkey clips are saved; this lowers disk I/O and lets clips span RAM-only material.
- Clip-related commands (`save_manual_clip`, `start_capture`, `stop_capture`, `trim_clip`, `upload_clip`, `take_screenshot`) now run as async Tauri commands so they execute off the main thread.
- The recording toggle flips optimistically in the UI and rolls back on error.
- The backend emits `clip-saved`, `clip-deleted`, `screenshots-changed`, and `storage-cleanup` events; the UI listens for them and refreshes clips and screenshots on demand.
- A modal viewer opens for clips and screenshots; when a clip's MP4 cannot load, the modal shows the file path and a Reveal-in-Explorer button instead of a broken `<video>` element.
- Bitrate settings now offer a preset dropdown (Low / Medium / Stream-ready / High / Ultra) plus a custom numeric input.
- Auto Clip rules are displayed per game with an Enable / Disable toggle per game, plus Enable all / Disable all / Restore defaults buttons.
- The app icon source is now a checked-in SVG (`src-tauri/icons/icon.svg`); all platform raster icons and `icon.ico` are regenerated from a 1024×1024 PNG rendered from that SVG.

## Not Fully Working Yet

- Native Windows Graphics Capture and FFmpeg capture finalization have not yet been manually QA-tested on real NVIDIA/AMD/Intel gaming systems.
- Native WASAPI audio is mixed into a single AAC track for MVP clips; separate audio tracks are now honored by the FFmpeg recording backend (routing around the single-track native WGC encoder), while per-process audio capture remains future work.
- Valve GSI receiver is a lightweight MVP receiver; config templates can be generated from the Auto Clip screen. CS2 round/bomb/match-start and Dota 2 match-start/match-end transitions are mapped, but per-player kill/death/objective event detection for Dota 2 and other Source titles is still needed.
- Auto start/stop is conservative but live: when auto-record is enabled, supported detected games can start recording and stop when the game disappears.
- Recorder service still runs inside the Tauri process instead of a separate background service process.
- Auto-update requires a GitHub release whose tag is newer than the running app version and that ships MSI/NSIS assets (the existing `v0.0.1` release carries 0.1.0 assets, so it correctly reports up to date).
- RAM-backed clipping reduces disk churn but cannot eliminate it entirely; FFmpeg still writes final clips to disk, and very large segment histories may briefly exceed the RAM cap before pruning.
- SVG icon rasterization depends on the sharp-based tooling used during development; the generated `icon.ico` and PNGs are checked in so normal builds do not require it.

## Next Engineering Steps

1. Add per-process loopback capture so individual application audio can be captured independently of the system mix.
2. Add per-player kill/death/objective event detection for Dota 2 and other Source titles beyond the current match-level transitions.
3. Split the recorder service into a separate background process so capture survives app restarts.
4. Run manual QA on Windows gaming hardware and tune CPU/GPU overhead.
5. Exercise the GitHub Actions release workflow with the `v0.1.1` tag push and verify the published assets appear in a GitHub Release.
