# ClipForge Product Spec

## V1 Experience

ClipForge opens directly into the clipping app. The user sees recording state, detected game, library, and selected clip details without a marketing screen.

Default posture:

- Local-first.
- Accountless.
- Mic off.
- Upload off.
- Manual hotkey available for unsupported games.
- Event auto-clipping enabled only for integrations with validated local event sources.

## Recording States

- `WaitingForGame`: no matching profile is active.
- `Buffering`: a supported game is active and replay segments are rotating.
- `RecordingSession`: full-session recording is active.
- `Clipping`: an event or hotkey requested clip extraction.
- `Processing`: clip is being finalized, thumbnailed, trimmed, or uploaded.
- `StorageLow`: recording stops or refuses to start until space is freed.

## Auto Clip Policy

- Default pre-roll: 10 seconds.
- Default post-roll: 8 seconds.
- Merge events within 12 seconds for the same game/session.
- Duplicate event IDs are ignored.
- Full-session recording stores bookmarks instead of immediately rendering separate clips.

## Upload Policy

Uploading is optional and user initiated unless the user creates an explicit auto-upload rule.

Provider order for V1:

1. Catbox and Litterbox.
2. Custom HTTP uploader.
3. Lustful after its API details and limits are confirmed.

Failures should never delete the local clip. Upload history stores provider, status, link, attempt count, and last error.

## Non-Goals For This Scaffold

- Production Windows Graphics Capture implementation.
- Production Media Foundation encoder pipeline.
- SQLite migrations.
- Account system or social feed.
- AI visual highlight detection or voice command clipping.
