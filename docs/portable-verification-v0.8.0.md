# Portable v0.8.0 verification — 2026-10-05

Windows EXE and Linux AppImage, both from commit `79aeb9c` ("release: 0.8.0").
The tag `v0.8.0` adds only this record and the CHANGELOG figures on top of it.

## What changed for packaging

No packaging script changed. `packaging/smoke.py` gained three checks for the
0.8 voice catalogue: the first launch writes a version 2 `voices.json`; a 0.7
file left at its shipped defaults is renamed to `voices.0.7.json` and replaced
by the version 2 template; a customised 0.7 file is kept byte for byte and the
console reports *0.7 format*. `packaging/PORTABLE-README.txt` (copied to
`dist/README.txt`) describes the regional voices, Listen, designed voices
(created and deleted only on 127.0.0.1 or ::1) and how a 0.7 `voices.json` is
handled.

The application adds one plain route, `GET /voice-sample/{voice_id}`, voice
samples under `DATA_DIR/audio/voices` and the `designed_voices` table in
`jobs.db`. No dependency was added.

## Builds and checks

- Host: Windows 11 IoT Enterprise LTSC 10.0.26100, x64. Rust 1.92.0, Dioxus CLI
  0.7.9, committed lockfiles, `packaging/build-windows.ps1` (`--locked`).
- `cargo fmt --check` for the app and launcher: passed.
- `cargo check` (web), server-only check and wasm32 check: passed, 0 warnings.
- `cargo test --features server --no-default-features`: **181 passed**, 3
  ignored (`live_probe` and `voice_live_probe` call the paid API;
  `dump_fixture` writes files). 0.7.1 had 86; the 95 new tests cover voice
  assignment, the catalogue and `voices.json` version 2, chunking by voice,
  the v2 speech cache, speech tags, tag-free transcripts, the Voices API and
  Voice Design, opening a real 0.7.1 exam, and stale script/recording
  detection.
- Release WASM is still not size-optimized: the `wasm-opt` that dx 0.7.9
  downloads crashes on Windows (0xc0000409), as since 0.5.0. Size 4.17 MB
  (4,169,393 bytes; 0.7.1: 3,877,810).

## Packaged verification (Windows)

`python packaging/smoke.py dist/listening-exam-generator-0.8.0-windows-x64.exe`
passed on the host: first-run templates (a version 2 `voices.json`), malformed
configuration despite environment overrides, preservation, a 0.7
`voices.json` at its defaults renamed and replaced, a customised one kept with
the *0.7 format* notice, spaces/Unicode paths, invalid settings, blocked
data/log/database paths, occupied port, `/` and `/exam`, JS/WASM/CSS, an
audio-job server function, WAV range and download, graceful shutdown,
relocation, relative `--config-dir`, and closing a real console window with no
payload left in `%TEMP%`.

**Not tested:** starting with no key anywhere, for the same reason as 0.7.x:
the host's user environment holds `GEMINI_API_KEY` and the smoke test does not
change the registry.

### Voices on the release EXE (paid, $0.0054)

The release EXE itself, copied to a fresh folder and started with
`--config-dir <scratch>\config --no-open --non-interactive` on port 8097, the
key coming from the environment:

- The part page (HSG Part 1) showed three speaker cards with automatic voices:
  A *Female, British English, Host*: **Digital Assistant 1**
  (`en-gb-assistant-1`); B *Female, American English, Guest*: **Sola**
  (`en-us-sola`); C *Male, British English, Expert*: **Authoritative Advisor
  8** (`en-gb-advisor-8`). Each card had Listen and Another voice.
- **Listen** on Speaker A made one request to `gemini-3.8-flash-tts` (804
  input and 553 output tokens, 7.3 s, $0.005379), wrote
  `data/audio/voices/en-gb-assistant-1.wav` (17.28 s) and played it in the
  page from `/voice-sample/en-gb-assistant-1`. The `usage` table holds one
  row with step `voices`.
- A second Listen replayed the same sample with no Gemini request.
- Ctrl+Break on the console stopped it: *Server stopped cleanly.* and the
  payload directory was removed.

Not tested on the release EXE: generating a script or a recording, Another
voice, the exam page's voice editing and Voice Design. They were covered on a
scratch server built from the same sources in the end-to-end check below.

## Linux AppImage

- Built with `packaging/build-linux.ps1` in the Ubuntu 22.04 builder image on
  the project's own `ielts-portable-builder` Podman (WSL) machine. Inside the
  container: `cargo fmt --check`, the three `cargo check`s and
  `cargo test --locked --features server --no-default-features` (**181
  passed**, 3 ignored) ran before `dx build`.
- `listening-exam-generator-0.8.0-linux-x86_64.AppImage`: 13,183,480 bytes (0.7.1: 12,634,616).
  The server links only `libssl.so.3`, `libcrypto.so.3`, `libgcc_s.so.1`,
  `libm.so.6` and `libc.so.6` (`dist/linux-dependencies.txt`).
- `packaging/test-linux.ps1` passed on clean **Ubuntu 22.04 and 24.04**
  containers as UID 65534: first run before Python is installed, manual
  extraction with AppRun, dependency resolution, licenses and inventory, the
  full smoke suite (including the 0.7 `voices.json` cases), **starting with
  no key anywhere**, a missing browser helper, and terminal Ctrl+C through the
  extraction supervisor (exit -2 / shell 130, as documented since 0.5.0).
- Not tested: native FUSE mounting (no `/dev/fuse` in the containers), a
  graphical desktop browser, and voices on Linux with a real key (the code
  path is the same as on Windows).

## Contents and dependencies

The console-subsystem x64 launcher (9,335,808 bytes; 0.7.1: 8,733,696) embeds
only `server.exe` (18,163,712 bytes; 0.7.1: 16,636,928), `public/` and
`RUST-DEPENDENCIES.txt`. Both import tables contain only Windows system DLLs,
unchanged from 0.7.0:

- launcher: `kernel32`, `ntdll`, `bcryptprimitives`, `api-ms-win-core-synch-l1-2-0`
- server: `kernel32`, `ntdll`, `advapi32` (registry), `ws2_32`, `secur32`,
  `crypt32`, `bcrypt`, `bcryptprimitives`, `api-ms-win-core-synch-l1-2-0`

## Paid checks during development

The packaging checks above make no Gemini request; the one Listen on the
release EXE cost $0.0054. The voice work itself was measured against the real
API, all on 2026-10-05, about **$1.9** in all:

| What | Spend | Record |
| --- | --- | --- |
| Probes and voice auditions: the 0.7.1 cause, gates G1–G9, pool selection, two full re-reads of the HSG passage | about $1.3 | `docs/voices.md` |
| Checks in the app while building milestones M1–M4 | $0.025 | scratch `jobs.db` files |
| End-to-end check: HSG Part 1, a full IELTS exam with Irish and Scottish voices, re-render, New take, Listen | $0.530 | `docs/voices.md`, CHANGELOG |
| AI-ear checks of the end-to-end recordings | $0.016 | `TESTING_DUMP/voice-lab/e2e/report/` |
| Listen on the release EXE (this record) | $0.005 | above |

The end-to-end IELTS exam cost $0.364, $0.728 at the list prices Google
charges from 2027, which is over the $0.70 bar in `docs/project_scope.md`; the
CHANGELOG lists it under known issues.

## Artifact SHA-256

| File | SHA-256 |
| --- | --- |
| listening-exam-generator-0.8.0-windows-x64.exe | `e4d95fb97b34166af2e192357f6932f37a22171370f920c800d8d691446d1be5` |
| listening-exam-generator-0.8.0-linux-x86_64.AppImage | `a3e73fd9c82af947eef3177b797148a4f71bf1fff5682d09259f46258aaa413f` |

Both packages, `README.txt` and the two SHA256SUMS files are published as the
GitHub release `v0.8.0`. Code signing, ARM64 and auto-updates remain out of
scope.
