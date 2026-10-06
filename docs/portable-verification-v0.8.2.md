# Portable v0.8.2 verification — 2026-10-06

Windows EXE and Linux AppImage, both from commit `8167e35` ("release: 0.8.2").
The tag `v0.8.2` adds only this record and the CHANGELOG figures on top of it.

## What changed

No packaging script changed. The only code change since 0.8.1 is
`7e095c1`: `Item.stem` and `Choice.text` (`src/domain/task.rs`) read a null
or missing value as an empty string, and the question prompt tells the model
that an empty string is `""` and an empty list `[]`, never `null`. No
dependency, route, table or file layout changed.

## Builds and checks

- Host: Windows 11 IoT Enterprise LTSC 10.0.26100, x64. Rust 1.92.0, Dioxus CLI
  0.7.9, committed lockfiles, `packaging/build-windows.ps1` (`--locked`).
- `cargo fmt --check` for the app and launcher: passed.
- `cargo check` (web), server-only check and wasm32 check: passed, 0 warnings.
- `cargo test --features server --no-default-features`: **198 passed**, 4
  ignored (`live_probe`, `voice_live_probe` and `brief_live_probe` call the
  paid API; `dump_fixture` writes files). 0.8.1 had 194; the 4 new tests
  parse the reply that failed (an HSG Part 4 summary completion with
  `"stem": null`), a short answer whose null stem becomes an "Empty question"
  issue, a matching task whose null option text becomes "Option A is empty",
  and missing stems and option texts. With the two serde attributes removed,
  all four fail.
- Release WASM is still not size-optimized (the `wasm-opt` that dx 0.7.9
  downloads crashes on Windows, 0xc0000409, as since 0.5.0). Size 4.21 MB
  (4,210,183 bytes; 0.8.1: 4,211,940).

## Packaged verification (Windows)

`python packaging/smoke.py dist/listening-exam-generator-0.8.2-windows-x64.exe`
passed on the host: first-run templates (a version 2 `voices.json`), malformed
configuration despite environment overrides, preservation, a 0.7
`voices.json` at its defaults renamed and replaced, a customised one kept with
the *0.7 format* notice, spaces/Unicode paths, invalid settings, blocked
data/log/database paths, occupied port, `/` and `/exam`, JS/WASM/CSS, an
audio-job server function, WAV range and download, graceful shutdown,
relocation, relative `--config-dir`, and closing a real console window with no
payload left in `%TEMP%`.

**Not tested:** starting with no key anywhere, for the same reason as 0.7.x
and 0.8.x: the host's user environment holds `GEMINI_API_KEY` and the smoke
test does not change the registry.

### A summary-completion block on the release EXE (paid, $0.0082)

The release EXE itself, copied to a fresh folder and started with
`--config-dir <scratch>\config --no-open --non-interactive` on port 8097:
HSG Part 4 ("how volunteers map urban heat islands with cheap thermometers on
bicycles"), *Script only* (one `gemini-3.8-flash` request, 779 input and 668
output tokens, $0.00309, and one `gemini-3.5-flash-lite` summary, $0.00005),
then *Generate questions*: one request (1,165 input and 1,129 output tokens,
11.8 s, $0.00511). The summary-completion block arrived with all ten gaps
(26)–(35) and no validator issue.

Before release, the same check on the development server with the topic that
failed during the 0.8.1 tests ("measuring street noise with volunteers and
cheap sensors") also parsed, with three word-limit issues from the validator
itself ($0.0088).

## Linux AppImage

- Built with `packaging/build-linux.ps1` in the Ubuntu 22.04 builder image on
  the project's own `ielts-portable-builder` Podman (WSL) machine. Inside the
  container: `cargo fmt --check`, the three `cargo check`s and
  `cargo test --locked --features server --no-default-features` (**198
  passed**, 4 ignored) ran before `dx build`.
- `listening-exam-generator-0.8.2-linux-x86_64.AppImage`: 13,224,440 bytes (0.8.1: 13,224,440).
  The server links only `libssl.so.3`, `libcrypto.so.3`, `libgcc_s.so.1`,
  `libm.so.6` and `libc.so.6` (`dist/linux-dependencies.txt`).
- `packaging/test-linux.ps1` passed on clean **Ubuntu 22.04 and 24.04**
  containers as UID 65534: first run before Python is installed, manual
  extraction with AppRun, dependency resolution, licenses and inventory, the
  full smoke suite (including the 0.7 `voices.json` cases), **starting with
  no key anywhere**, a missing browser helper, and terminal Ctrl+C through the
  extraction supervisor (exit -2 / shell 130, as documented since 0.5.0).
- Not tested: native FUSE mounting (no `/dev/fuse` in the containers), a
  graphical desktop browser, and question generation on Linux with a real key
  (the parsing is the same server code as on Windows).

## Contents and dependencies

The console-subsystem x64 launcher (9,393,152 bytes; 0.8.1: 9,394,688) embeds
only `server.exe` (18,283,520 bytes; 0.8.1: 18,277,888), `public/` and
`RUST-DEPENDENCIES.txt`. Both import tables contain only Windows system DLLs,
unchanged since 0.7.0:

- launcher: `kernel32`, `ntdll`, `bcryptprimitives`, `api-ms-win-core-synch-l1-2-0`
- server: `kernel32`, `ntdll`, `advapi32` (registry), `ws2_32`, `secur32`,
  `crypt32`, `bcrypt`, `bcryptprimitives`, `api-ms-win-core-synch-l1-2-0`

## Paid checks

The packaging checks make no Gemini request. The two question checks above
cost **$0.017** in all.

## Artifact SHA-256

| File | SHA-256 |
| --- | --- |
| listening-exam-generator-0.8.2-windows-x64.exe | `a2fc4a51d55a3aabb68a56eafe3a5898e1f4d168f6df70380b4a4b4d03dfdf68` |
| listening-exam-generator-0.8.2-linux-x86_64.AppImage | `0e8fd95fa0da59079ae258330cb28519eacb9723e48c3573fef95bdfb7d89a01` |

Both packages, `README.txt` and the two SHA256SUMS files are published as the
GitHub release `v0.8.2`. Code signing, ARM64 and auto-updates remain out of
scope.
