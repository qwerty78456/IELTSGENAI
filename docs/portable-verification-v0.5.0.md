# Portable v0.5.0 verification — 2026-09-23

## Status at handoff

The interrupted task had implemented startup/configuration and both packaging
pipelines, built a Windows EXE and an AppImage still inside its builder, and
passed 34 unit tests. Linux package verification, final artifact audits and
Git delivery had not been completed. The work remained uncommitted.

Completion added explicit encoding diagnostics, a transactional SQLite write
probe, truncated-WAV validation without panics, voice-map validation tests,
launcher/runtime dependency notices, and broader repeatable package tests.
That run committed and pushed `1f2ff20`, then was interrupted before reporting.

A second continuation re-verified everything independently: the pushed commit,
artifact checksums, build inputs against the commit (identical apart from CRLF
checkout), all checks and tests, and both packages. It found and fixed one
defect: closing the Windows console window (the usual way to stop a
double-clicked app) killed the launcher before cleanup, leaving a 19.8 MB
`listening-generator-*` directory in `%TEMP%` on every run. The launcher now
holds Windows' close/logoff/shutdown deadline until the payload is removed,
and does not start the server if the console closed during extraction.
`smoke.py` closes a real console window and asserts no payload remains
(it fails on the old EXE and passes on the new one). Only the Windows EXE was
rebuilt; its `server.exe` is byte-identical to the one first verified, and
no AppImage input changed, so the AppImage was re-tested but not rebuilt.
The CHANGELOG entry was rewritten in Vietnamese (project convention), and the
architecture notes, CLAUDE.md and READMEs now describe the startup module.

## Builds and checks

- Windows host: Windows 11 IoT Enterprise LTSC, 10.0.26100, x64.
- Linux builder: Ubuntu 22.04 on the `ielts-portable-builder` Podman (WSL)
  machine, which the first interrupted run created for this project. The
  machine that existed before (`epg-builder`, another project's default
  connection) was not used or modified. A clean Containerfile build and
  full PowerShell build pipeline succeeded; the final incremental rebuild
  incorporated the last shutdown diagnostic and library notices.
- Rust 1.92.0, Dioxus CLI 0.7.9, committed application and launcher lockfiles.
- `cargo fmt --check` for the app and launcher: passed.
- `cargo check --locked`, server-only check, and wasm32 check: passed on
  Windows and the Ubuntu builder.
- `cargo test --locked --features server --no-default-features`: **42 passed**
  on both platforms. Includes configuration creation/preservation, malformed
  dotenv/JSON/encoding, relative paths, secrets, music and SQLite startup checks.
- `dx serve --web --open false`: HTTP 200 and CSS hot reload observed. The
  temporary CSS edit was restored byte-for-byte. Dioxus CLI prints a version
  mismatch diagnostic for the existing locked Dioxus 0.7.2 libraries, but
  production builds and the development/hot-reload test succeeded.
- Release WASM is not size-optimized on either platform (about 2.7 MB). On
  Windows the binaryen `wasm-opt` that dx 0.7.9 downloads crashes on this
  module (exit 0xc0000409, reproducible when run directly); dx logs an ERROR
  and ships the unoptimized WASM. It is served from localhost and every page,
  WASM and UI check passes; this predates the portable work.

## Packaged verification

`packaging/smoke.py` passed on the Windows host and, as UID 65534, in clean
Ubuntu **22.04 and 24.04** containers. `packaging/test-linux.ps1` reproduces
the Linux checks. Each Linux image successfully initialized the application
before Python or any test-runner dependencies were installed.

Verified:

- First-run `.env` and `voices.json`, independently missing files, preservation,
  malformed configuration despite environment overrides, and exit 1 on errors.
- Spaces/Unicode in paths, a different working directory containing invalid
  dotenv, relocation, and explicit relative `--config-dir`.
- Invalid settings, occupied port, blocked data/log/database paths; Linux also
  checks actual permission-denied configuration/log paths and a read-only DB.
- `/`, `/exam`, nonempty JS/CSS and valid WASM bytes; an audio-job server
  function, full WAV download, exact byte-range content and Content-Range.
- Graceful server shutdown, released listening port, normal exit 0 and restart.
- Windows: closing the console window (WM_CLOSE, as the X button) stops the
  server, frees the port and removes the extracted payload (ten runs);
  no payload directory remains after any smoke-test run.
- Linux missing-browser-helper failure: URL remains visible, server stays usable.
- Automatic AppImage extraction and manual `--appimage-extract` plus AppRun;
  configuration remains outside the extracted application.
- Windows rendered UI and WASM interaction in the Codex browser: both pages,
  navigation, styled layout and expanding speaker customization.

All requests used dummy credentials and local audio/SQLite fixtures. No Gemini
generation or paid API request was made.

## Contents and dependencies

The Windows console-subsystem x64 launcher contains only `server.exe`, matching
`public/` assets and Rust/launcher dependency notices. Both PE import tables
contain only Windows system DLLs; the C runtime is statically linked.

The Type 2 AppImage contains its desktop entry/icon, server/public assets,
libssl.so.3, libcrypto.so.3, libgcc_s.so.1, a fallback CA bundle, Rust dependency
notices, Ubuntu library copyright notices and AppImage/runtime library licenses.
Its server's highest GLIBC symbol requirement is 2.34; the supported/tested
distribution baseline remains Ubuntu 22.04 (glibc 2.35).

Inventories exclude real `.env`, voice overrides, credentials, recordings,
SQLite databases, logs and development source/data. Binaries and audit files
are local under ignored `dist/`; no GitHub Release is published.

## Platform limitations

- Native FUSE mounting and a graphical Linux desktop/default browser were not
  tested: the clean containers have no `/dev/fuse` or desktop. Extraction tests
  do not establish native-mount coverage.
- Windows 10 was not tested (host is Windows 11). Explorer itself was not
  used; a double-click was simulated by giving the EXE its own new console.
  With no key it showed the `.env` path, the fix and "Press Enter to close.",
  stayed open, and exited 1 after an Enter keystroke written to the console;
  launched through an existing `cmd` console it exited 1 immediately. Without
  --no-open it opened the default browser (Firefox), which connected to the
  server; Ctrl+Break then stopped it with exit 0 and no leftover payload.
  Browser-opening failure was forced only on Linux, without changing Windows
  associations.
- Upstream AppImage runtime 75849dc does not intercept SIGINT in its extraction
  supervisor. Terminal Ctrl+C can return shell **130** and leave the temporary
  extraction even though the server confirms graceful shutdown. Tests verify
  both the application's exit 0 (signal the supervised server) and terminal
  Ctrl+C (signal its process group and wait for the server and port to stop).
  Manual extraction with AppRun avoids this supervisor behavior.
- Code signing, ARM64, auto-updates and live paid generation were outside scope.

References: [AppImage environment variables](https://docs.appimage.org/packaging-guide/environment-variables.html),
[pinned runtime source](https://github.com/AppImage/type2-runtime/blob/75849dc/src/runtime/runtime.c),
[Rust C-runtime linkage](https://doc.rust-lang.org/reference/linkage.html#static-and-dynamic-c-runtimes).

## Artifact SHA-256

| File | SHA-256 |
| --- | --- |
| listening-exam-generator-0.5.0-windows-x64.exe | `4ee7852381d2d0a6281589882013ed4e000d9e0095b85211012e949511252e2f` |
| listening-exam-generator-0.5.0-linux-x86_64.AppImage | `438158a62733fa77417025e17ad1c196ebc3d3c27fdc5f55ac9a05b647717fb0` |

`dist/README.txt`, checksum files, dependency audits and a copy of this report
accompany the two local executables. The final source commit is recorded in
`dist/BUILD-INFO.txt` after committing and verifying the remote.
