# Portable releases

The application remains a Dioxus 0.7 fullstack browser/server app. Packaging
adds a small distribution launcher, not another application backend.

## Configuration and lifecycle

Portable mode creates .env, voices.json, and data/ beside the distributed
EXE/AppImage. The Windows launcher supplies its original directory explicitly,
and AppRun derives it from APPIMAGE, not the read-only mount or current directory.
An explicit --config-dir PATH overrides that location.

Missing templates are created exclusively, with mode 0600 on Unix. Existing
files are preserved. Permission failures, malformed dotenv/JSON, empty voice
defaults, invalid IP/port/model/log settings, invalid optional music, and
unwritable logs/database cause startup failure. Error messages omit dotenv
contents and API-key values. Environment variables override file values;
syntax errors are still rejected. A missing/placeholder key exits after creating
the templates. Startup makes no paid or credential-validation API requests.

Development and Docker retain their current-working-directory configuration
and data conventions; no parent-directory dotenv search is performed. Docker
may supply configuration entirely through environment variables. They do not
open browsers. Debug launches retain Dioxus hot reload. All modes now require
valid startup configuration.

Portable production startup initializes logging, SQLite, and a listener before
opening the browser. Missing browser launchers leave the URL visible and the
server running. An occupied port fails instead of selecting another port.
Ctrl+C shuts down the server. Windows additionally handles Ctrl+Break.

## Rebuilding

Use Rust 1.92.0, wasm32-unknown-unknown, Dioxus CLI 0.7.9, and Python 3.

Windows (MSVC build tools installed):

    pwsh -NoProfile -File packaging/build-windows.ps1
    python packaging/smoke.py dist/listening-exam-generator-0.5.0-windows-x64.exe

The Windows server and packaging launcher statically link the C runtime.
The launcher embeds only the server, public assets, and dependency notices,
extracts into a private temporary directory, passes console I/O through,
waits for the child, and cleans up. Errors pause only when the launcher owns
the console, stdin is interactive, and --non-interactive was not supplied.
Forced process termination or power loss can leave a temporary directory.

Linux from Windows with an existing running Podman machine:

    pwsh -NoProfile -File packaging/build-linux.ps1 -Connection ielts-portable-builder

The build image uses Ubuntu 22.04 and compiles Dioxus CLI from source because
its published Linux binary requires newer glibc. Rust and CLI versions are pinned;
appimagetool 1.9.1 and the Type 2 runtime are checked against recorded SHA-256s.
If the moving upstream runtime download changes, checksum verification stops;
review and update the pinned hash explicitly.

The script transfers an allowlist of source files, excluding .env, data,
Git metadata, and secrets. The AppImage includes non-system shared libraries,
license notices, and a CA-bundle fallback. glibc remains a host dependency
(2.35 minimum). The user's browser is not bundled.

Without FUSE, run with APPIMAGE_EXTRACT_AND_RUN=1. Extracted AppDir execution
uses its parent as the default config directory; --config-dir makes it explicit.
In automatic extraction mode, terminal Ctrl+C also interrupts the upstream
AppImage supervisor (shell exit 130). The server still shuts down gracefully;
look for "Server stopped cleanly.". That supervisor can leave its temporary
extraction directory after an interrupt. For predictable exit 0 and no temporary
extraction cleanup, use --appimage-extract once and run squashfs-root/AppRun.

## Release verification

Run cargo fmt/check, both cargo check feature combinations, the wasm check,
and all server-feature unit tests. The smoke script tests the actual package
in a temporary path with spaces/Unicode and no real API key. It checks missing
and malformed config, environment overrides, preservation, storage errors,
occupied ports, both pages, JS/WASM/CSS, an audio-job server function, WAV ranges
and downloads, shutdown, and relocation.

Audit EXE imports and Linux ldd output. Verify package inventories contain only
runtime files and notices. Test AppImages on Ubuntu 22.04 and a newer distribution.
Record FUSE/graphical-browser tests separately from headless container tests.
Code signing, ARM64, updates, and paid live generation are outside this release.

Run `pwsh -NoProfile -File packaging/test-linux.ps1` to verify the built AppImage
on clean Ubuntu 22.04 and 24.04 containers. Each first tests startup before
installing Python, then runs the full smoke suite as an unprivileged user,
including permission errors and missing browser helpers. Windows smoke tests
do not alter system browser associations to force a browser-launch failure.

See [the v0.5.0 verification record](portable-verification-v0.5.0.md) for measured
results and platform limitations. Build scripts pin tools and lockfiles, but
do not promise byte-for-byte identical binaries (upstream OS packages, build
timestamps and paths can vary).
