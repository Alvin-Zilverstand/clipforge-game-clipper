# Security Policy

## Supported Versions

We release security updates for the following versions:

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

Instead, please report them via email to: **alvin@alvnx.xyz**

You should receive a response within 48 hours. If for some reason you do not, please follow up via email to ensure we received your original message.

Please include the following information:
- Type of issue (e.g., buffer overflow, injection, information disclosure)
- Full paths of source file(s) related to the vulnerability
- Steps to reproduce the issue
- Proof-of-concept or exploit code (if possible)
- Impact of the vulnerability

## Disclosure Policy

- We will acknowledge receipt of your vulnerability report within 48 hours
- We will provide a timeline for a fix within 5 business days
- We will keep you informed of progress
- We will credit you in the release notes (unless you prefer anonymity)
- We will not take legal action against you for good-faith security research

## Security Considerations

ClipForge is a local-first application with the following security characteristics:

### Data Handling
- All clips and metadata stored locally (SQLite + filesystem)
- No telemetry or analytics
- No account system or cloud sync
- Upload is explicitly user-initiated (or via opt-in auto-upload)

### Network
- League Live Client Data: HTTPS to localhost (127.0.0.1:2999) with cert verification disabled (Riot's self-signed cert)
- Valve GSI: HTTP receiver on 127.0.0.1:49321 (local only)
- Upload providers: HTTPS to Catbox/Litterbox/custom endpoints
- No outbound connections except explicit uploads

### Permissions
- Requires Windows screen capture permission (Windows Graphics Capture)
- Requires microphone permission (if enabled)
- Runs as standard user (not elevated)

### Known Attack Surface
1. **GSI HTTP Receiver** - Local-only, but could be accessed by other local processes
2. **FFmpeg subprocess** - User-controlled arguments sanitized, but FFmpeg itself is a large attack surface
3. **SQLite database** - Local file, no network exposure
4. **Tauri IPC** - Commands validated server-side

## Security Best Practices for Users

- Keep ClipForge updated
- Only enable auto-upload to trusted providers
- Review custom upload endpoints before use
- Run on supported Windows versions with latest patches
