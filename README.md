# ClipForge

ClipForge is a Windows-first, local-first game clipping app inspired by tools like Medal and FTHR. It runs as a lightweight Tauri desktop app using the system WebView2, with Rust handling capture, replay clips, local storage, game events, hotkeys, and uploads.

## Current State

- Rolling replay capture writes short MP4 segments and manual clips extract from those segments.
- Full-session recording concatenates the session segments into a saved MP4.
- Capture prefers FFmpeg Desktop Duplication (`ddagrab`) when available and falls back to `gdigrab`.
- FFmpeg Media Foundation H.264 is used when available.
- FFmpeg is bundled as a Tauri sidecar for installer builds, so users do not need a separate FFmpeg install.
- SQLite stores the clip library; `settings.json` stores versioned local settings.
- Global hotkeys are registered while the app runs: `F8`, `Shift+F8`, and `Alt+F7`.
- League Live Client events and Valve GSI-style events feed the auto-clip pipeline.
- Catbox, Litterbox, and custom multipart HTTP upload adapters are implemented; the UI can choose providers.
- Native Windows Graphics Capture and native Rust WASAPI mixing are still future work.

## Commands

```powershell
pnpm install
pnpm run build
cargo test
cargo check --manifest-path src-tauri\Cargo.toml
```

For the desktop app:

```powershell
pnpm run tauri dev
pnpm run desktop:build
```

`pnpm run desktop:build` prepares the FFmpeg sidecar, builds the frontend, builds the Tauri app, and produces MSI/NSIS installers under `src-tauri/target/release/bundle/`.

## Architecture

See `docs/architecture.md` and `docs/product-spec.md`.

## Desktop Installer

The verified build outputs are:

- `src-tauri/target/release/bundle/msi/ClipForge_0.1.0_x64_en-US.msi`
- `src-tauri/target/release/bundle/nsis/ClipForge_0.1.0_x64-setup.exe`

