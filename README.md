# ClipForge

ClipForge is a Windows-first, local-first game clipping app scaffold inspired by tools like Medal and FTHR. It implements the durable core boundaries for a serious MVP: replay buffer logic, event-based auto clipping, game detection, local library storage, upload adapters, and a Tauri-ready desktop UI.

## Current State

- Rust core compiles and is unit tested.
- Vite TypeScript UI builds.
- Tauri shell files are present under `src-tauri/`.
- Native Windows capture and Media Foundation encoding are represented by interfaces/stubs and are the next major implementation step.

## Commands

Use direct local binaries if the global npm shim is broken:

```powershell
.\node_modules\.bin\tsc.CMD --noEmit
.\node_modules\.bin\vite.CMD build ui
cargo test
cargo run
```

When npm is healthy, the package scripts are:

```powershell
pnpm run dev
pnpm run build
pnpm run tauri dev
```

## Architecture

See `docs/architecture.md` and `docs/product-spec.md`.

## Desktop Installer

Run pnpm run desktop:build to prepare FFmpeg, build the UI, build the Tauri app, and produce MSI/NSIS installers under src-tauri/target/release/bundle/.

