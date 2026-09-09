# FFmpeg Installer Strategy

ClipForge should stay lightweight while still working for users who do not already have FFmpeg installed.

## Packaging Behavior

- `pnpm run prepare:ffmpeg` prepares `src-tauri/binaries/ffmpeg-x86_64-pc-windows-msvc.exe`.
- If `ffmpeg` is already on PATH, the script copies that binary into the sidecar location.
- If `ffmpeg` is missing, the script downloads the FFmpeg essentials zip and extracts `ffmpeg.exe` into the sidecar location.
- `src-tauri/tauri.conf.json` declares `bundle.externalBin = ["binaries/ffmpeg"]`, so Tauri includes the binary in MSI/NSIS installers.
- Installed users do not need a separate FFmpeg install; ClipForge can prefer system FFmpeg when present and fall back to the bundled sidecar.

## Why This Is Lightweight

Tauri uses the system WebView instead of bundling Chromium. FFmpeg is a single sidecar executable used only for capture/encoding work, not a persistent extra runtime.

## Build Commands

```powershell
pnpm run prepare:ffmpeg
pnpm run desktop:build
```
