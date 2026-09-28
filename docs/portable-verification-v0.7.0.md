# Portable v0.7.0 verification — 2026-09-28

Windows only. The Linux AppImage was not rebuilt for 0.7.0; the newest Linux
build in `dist/` is 0.6.0.

## What changed for packaging

- A missing or placeholder `GEMINI_API_KEY` no longer stops startup. The
  server starts, prints `Gemini API key (GEMINI_API_KEY): missing; ...`, and
  the page offers a key form (loopback binds only).
- On Windows the key and every other setting are also read from the user and
  machine environment in the registry (`winreg`), between `.env` and the
  process environment.
- `packaging/smoke.py`: the first-run and `--config-dir` cases now force a
  failure with `PORT=0` after the templates are written (a first run without a
  key would otherwise start and wait). A new case starts with no key anywhere
  and expects the "missing" line and a working `/exam`; it is skipped with
  `NOT TESTED` when the Windows environment already holds a key, because the
  test must not change the registry. `packaging/linux/check-clean.sh` expects a
  running server (exit 124 from `timeout`) instead of exit 1 on first run.
- `packaging/PORTABLE-README.txt` (copied to `dist/README.txt`) describes the
  key order, the browser form and its risks, and the cost settings.

## Builds and checks

- Host: Windows 11 IoT Enterprise LTSC 10.0.26100, x64. Rust 1.92.0, Dioxus CLI
  0.7.9, committed lockfiles, `packaging/build-windows.ps1` (`--locked`).
- `cargo fmt --check` for the app and launcher: passed (after formatting the
  0.7.0 code; the EXE was rebuilt from the formatted source).
- `cargo check`, server-only check and wasm32 check: passed, 0 warnings.
- `cargo test --features server --no-default-features`: **84 passed**, 2
  ignored (`live_probe` calls the paid API; `dump_fixture` writes files).
- Release WASM is still not size-optimized: the `wasm-opt` that dx 0.7.9
  downloads crashes on Windows (0xc0000409), as since 0.5.0. Size 3.88 MB.

## Packaged verification

`python packaging/smoke.py dist/listening-exam-generator-0.7.0-windows-x64.exe`
passed on the host: first-run templates, malformed configuration despite
environment overrides, preservation, spaces/Unicode paths, invalid settings,
blocked data/log/database paths, occupied port, `/` and `/exam`, JS/WASM/CSS,
an audio-job server function, WAV range and download, graceful shutdown,
relocation, relative `--config-dir`, and closing a real console window with no
payload left in `%TEMP%`.

**Not tested:** starting with no key anywhere. The host's user environment
holds `GEMINI_API_KEY`, and the smoke test deliberately does not change it.
The refusal rules of the browser form are unit-tested
(`application::settings`), and the missing-key startup is unit-tested in
`config`.

Manually, the release EXE started from a fresh folder with `GEMINI_API_KEY`
removed from its process: it printed `from the Windows environment`, served
`/exam` with 200, and `api_key_status` returned
`{"source":"WindowsEnvironment","can_enter":false}` (form hidden). A forced
stop of the process tree left no payload directory.

## Paid checks made during development (not part of the smoke test)

With the developer's key, on the debug build of the same source:

- `live_probe`: every Interactions request shape accepted (text, JSON, one
  voice, two voices named `SpeakerA` / `SpeakerB` with `mode: conversational`);
  audio billed at 32.0–32.1 tokens per second; a Gemini transcription of the
  probe audio matched the script word for word, with no style text or speaker
  label spoken.
- One full IELTS exam at thinking `medium`: $0.480 ($0.961 at 2027 prices).
  One at `low`: **$0.308 ($0.616)**, 34 requests, a 29:10 recording. Rendering
  that recording again unchanged: 0 requests, 20 of 20 chunks reused, $0.

About $0.80 was spent in total.

## Contents and dependencies

The console-subsystem x64 launcher (8,732,672 bytes) embeds only `server.exe`,
`public/` and `RUST-DEPENDENCIES.txt` (10 entries); no `.env`, voice map,
database, recording, log or source file. Both import tables contain only
Windows system DLLs:

- launcher: `kernel32`, `ntdll`, `bcryptprimitives`, `api-ms-win-core-synch-l1-2-0`
- server: `kernel32`, `ntdll`, `advapi32` (registry), `ws2_32`, `secur32`,
  `crypt32`, `bcrypt`, `bcryptprimitives`, `api-ms-win-core-synch-l1-2-0`

## Artifact SHA-256

| File | SHA-256 |
| --- | --- |
| listening-exam-generator-0.7.0-windows-x64.exe | `c1f62c8fbcef6b75d1c5b2fe9d5a61d8b94b8e32123786e1af46695a08387575` |

Binaries stay in ignored `dist/`; no GitHub Release is published. Code
signing, ARM64 and auto-updates remain out of scope.
