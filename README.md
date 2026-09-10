# ClipForge

ClipForge is a Windows-first, local-first game clipping app inspired by tools like Medal and FTHR. It runs as a lightweight Tauri desktop app using the system WebView2, with Rust handling capture, replay clips, local storage, game events, hotkeys, and uploads.

## Current State

- Rolling replay capture writes short MP4 segments and manual clips extract from those segments.
- Full-session recording concatenates the session segments into a saved MP4.
- Capture prefers native Windows Graphics Capture with H.264 for detected game windows or the desktop, then falls back to FFmpeg Desktop Duplication (`ddagrab`) or `gdigrab`.
- FFmpeg Media Foundation H.264 is used when available.
- FFmpeg is bundled as a Tauri sidecar for installer builds, so users do not need a separate FFmpeg install.
- SQLite stores the clip library; `settings.json` stores versioned local settings.
- Global hotkeys are registered while the app runs: `F8`, `Shift+F8`, and `Alt+F7`.
- League Live Client events and Valve GSI-style events feed the auto-clip pipeline.
- Catbox, Litterbox, and custom multipart HTTP upload adapters are implemented; the UI can choose providers.
- Native WASAPI system-audio and mic capture are wired into the native Windows Graphics Capture path; FFmpeg remains the fallback capture path.

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

The NSIS setup EXE installs for the current Windows user by default and creates the normal Windows uninstaller entry/file during installation. The MSI can also be removed through Windows Apps/Programs and Features.

## GitHub Releases

GitHub Actions builds Windows installer artifacts on pushes to `master`/`main`, pull requests, and manual runs. Any pushed file change, including newly added files, triggers a build. Pushing a tag like `v0.1.0` also publishes the MSI and setup EXE to a GitHub Release.

