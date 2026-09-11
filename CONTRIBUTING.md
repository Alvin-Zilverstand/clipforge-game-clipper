# Contributing to ClipForge

Thank you for your interest in contributing to ClipForge! This document provides guidelines for contributing to the project.

## Getting Started

### Prerequisites

- **Rust** (stable toolchain) - install via [rustup](https://rustup.rs/)
- **Node.js** (v22+) and **pnpm** (v11+) - install via [corepack](https://github.com/nodejs/corepack)
- **Windows** (Windows 10/11) - ClipForge is Windows-first
- **Visual Studio 2022** with C++ workload (for MSVC toolchain)

### Building from Source

```bash
# Clone the repository
git clone https://github.com/Alvin-Zilverstand/clipforge-game-clipper.git
cd clipforge-game-clipper

# Install frontend dependencies
pnpm install

# Build the frontend
pnpm run build

# Build the Tauri app (debug)
pnpm run tauri dev

# Build the Tauri app (release installers)
pnpm run desktop:build
```

Installers will be created in `src-tauri/target/release/bundle/`.

### Running Tests

```bash
# Run all Rust tests
cargo test

# Run only library tests
cargo test --lib

# Run specific test
cargo test test_name
```

## Development Workflow

### Branching Strategy

- `master` - Protected branch, only updated via PR merges
- Feature branches: `feat/description` or `feature/description`
- Bug fix branches: `fix/description` or `bugfix/description`
- Documentation: `docs/description`

### Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
type(scope): short description

Longer description if needed.

Fixes #123
```

Types:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style (formatting, etc.)
- `refactor`: Code refactoring
- `test`: Adding/updating tests
- `chore`: Maintenance tasks

### Pull Request Process

1. Fork the repository
2. Create a feature branch from `master`
3. Make your changes with tests
4. Ensure all tests pass (`cargo test`)
5. Ensure code is formatted (`cargo fmt`)
6. Ensure clippy is clean (`cargo clippy`)
7. Open a PR against `master`
8. Wait for review and CI checks

### Code Style

- Rust: `cargo fmt` (standard rustfmt)
- TypeScript: `pnpm run lint` (ESLint + Prettier)
- Run `cargo clippy` before committing

## Architecture Overview

```
clipforge-game-clipper/
├── src/                    # Rust library (core logic)
│   ├── capture.rs          # FFmpeg capture backends
│   ├── native_wgc.rs       # Windows Graphics Capture
│   ├── native_audio.rs     # WASAPI audio capture
│   ├── recorder.rs         # Replay buffer, auto-clipping
│   ├── integrations.rs     # League/Valve GSI events
│   ├── library.rs          # Clip library (SQLite)
│   ├── upload.rs           # Upload providers
│   ├── settings.rs         # Versioned settings
│   └── ...
├── src-tauri/              # Tauri desktop shell
│   └── src/main.rs         # Tauri commands, app setup
├── ui/                     # TypeScript/Vite frontend
│   └── src/main.ts         # Single-page UI
└── scripts/                # Build scripts
```

## Adding Features

### New Capture Backend

1. Implement `CaptureBackend` trait in `src/capture.rs`
2. Add to `start_best_capture_backend` in `src-tauri/src/main.rs`
3. Add tests

### New Game Integration

1. Add game profile in `src/game_detection.rs`
2. Implement `GameIntegration` trait in `src/integrations.rs`
3. Add event normalization
4. Update `default_auto_clip_events_for_game` in `src-tauri/src/main.rs`
5. Add UI entries in `ui/src/main.ts`

### New Upload Provider

1. Implement `Uploader` trait in `src/upload.rs`
2. Add to `upload_clip_with_settings` in `src-tauri/src/main.rs`
3. Add UI in `ui/src/main.ts` (Uploads view)

## Reporting Issues

Use the GitHub issue templates:
- [Bug Report](.github/ISSUE_TEMPLATE/bug_report.md)
- [Feature Request](.github/ISSUE_TEMPLATE/feature_request.md)

## Security

See [SECURITY.md](SECURITY.md) for reporting security vulnerabilities.

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).