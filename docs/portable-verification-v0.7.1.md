# Portable v0.7.1 verification — 2026-10-05

Windows EXE and Linux AppImage, both from commit `714e1e6` ("release: 0.7.1").
The tag `v0.7.1` adds only this record and the CHANGELOG figures on top of it.

## What changed for packaging

Nothing in the packaging scripts or the smoke test. The application change is
the rejected-key flow: a key Google refuses is reported with its source
(`LlmError::ActiveKeyRejected`), and on a loopback bind the page offers a
browser key that replaces it until restart. `packaging/PORTABLE-README.txt`
(copied to `dist/README.txt`) describes it.

## Builds and checks

- Host: Windows 11 IoT Enterprise LTSC 10.0.26100, x64. Rust 1.92.0, Dioxus CLI
  0.7.9, committed lockfiles, `packaging/build-windows.ps1` (`--locked`).
- `cargo fmt --check` for the app and launcher: passed.
- `cargo check` (web), server-only check and wasm32 check, all `--locked`:
  passed, 0 warnings.
- `cargo test --features server --no-default-features`: **86 passed**, 2
  ignored (`live_probe` calls the paid API; `dump_fixture` writes files). The
  two new tests cover key-refusal bodies (recorded from free calls with a
  made-up key and with no key) and when "Remember" can outlast a restart.
- Release WASM is still not size-optimized: the `wasm-opt` that dx 0.7.9
  downloads crashes on Windows (0xc0000409), as since 0.5.0. Size 3.88 MB
  (3,877,810 bytes).

## Packaged verification (Windows)

`python packaging/smoke.py dist/listening-exam-generator-0.7.1-windows-x64.exe`
passed on the host: first-run templates, malformed configuration despite
environment overrides, preservation, spaces/Unicode paths, invalid settings,
blocked data/log/database paths, occupied port, `/` and `/exam`, JS/WASM/CSS,
an audio-job server function, WAV range and download, graceful shutdown,
relocation, relative `--config-dir`, and closing a real console window with no
payload left in `%TEMP%`.

**Not tested:** starting with no key anywhere, for the same reason as 0.7.0:
the host's user environment holds `GEMINI_API_KEY` and the smoke test does not
change the registry.

The new flow, on the release EXE itself, started from a fresh folder with a
made-up key in its process environment: the console printed
`from the server's environment`; no key box before any request; "Suggest a
topic" failed with *Google rejected the Gemini API key from the server's
environment…* (Google charges nothing for a refused key); the box *Google
rejected the Gemini API key* appeared with a password field and without
"Remember" (an environment key would shadow `.env`). Replacing the key with a
real one was not tried. Stopping that run with `taskkill /F` left its payload
directory in `%TEMP%`, the documented limitation of forced termination; it was
removed by hand.

## Linux AppImage

- Built with `packaging/build-linux.ps1` in the Ubuntu 22.04 builder image on
  the project's own `ielts-portable-builder` Podman (WSL) machine. Inside the
  container: `cargo fmt --check`, the three `cargo check`s and
  `cargo test --locked --features server --no-default-features` (**86 passed**,
  2 ignored) ran before `dx build`.
- `listening-exam-generator-0.7.1-linux-x86_64.AppImage`: 12,634,616 bytes.
  The server links only `libssl.so.3`, `libcrypto.so.3`, `libgcc_s.so.1`,
  `libm.so.6` and `libc.so.6` (`dist/linux-dependencies.txt`).
- `packaging/test-linux.ps1` passed on clean **Ubuntu 22.04 and 24.04**
  containers as UID 65534: first run before Python is installed, manual
  extraction with AppRun, dependency resolution, licenses and inventory, the
  full smoke suite, **starting with no key anywhere**, a missing browser
  helper, and terminal Ctrl+C through the extraction supervisor (exit -2 /
  shell 130, as documented since 0.5.0).
- Not tested: native FUSE mounting (no `/dev/fuse` in the containers), a
  graphical desktop browser, and the rejected-key flow on Linux (the code path
  is the same as on Windows).

## Contents and dependencies

The console-subsystem x64 launcher (8,733,696 bytes) embeds only `server.exe`
(16,636,928 bytes), `public/` and `RUST-DEPENDENCIES.txt`. Both import tables
contain only Windows system DLLs, unchanged from 0.7.0:

- launcher: `kernel32`, `ntdll`, `bcryptprimitives`, `api-ms-win-core-synch-l1-2-0`
- server: `kernel32`, `ntdll`, `advapi32` (registry), `ws2_32`, `secur32`,
  `crypt32`, `bcrypt`, `bcryptprimitives`, `api-ms-win-core-synch-l1-2-0`

## Paid checks

None. Every Gemini request made for this release used a made-up key or no
key and was refused free of charge.

## Artifact SHA-256

| File | SHA-256 |
| --- | --- |
| listening-exam-generator-0.7.1-windows-x64.exe | `e9f30bc8a5344e803a422bdfa5c2346824b19b00bec045eb80b256811227e2ea` |
| listening-exam-generator-0.7.1-linux-x86_64.AppImage | `5e09237a2134dd43709616c8b91c3fe7714fed1c3360d28cef74febf5918b2a6` |

Both packages, `README.txt` and the two SHA256SUMS files are published as the
GitHub release `v0.7.1`. Code signing, ARM64 and auto-updates remain out of
scope.
