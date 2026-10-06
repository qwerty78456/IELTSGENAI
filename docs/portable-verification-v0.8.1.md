# Portable v0.8.1 verification — 2026-10-06

Windows EXE and Linux AppImage, both from commit `fccc44b` ("release: 0.8.1").
The tag `v0.8.1` adds only this record and the CHANGELOG figures on top of it.

## What changed for packaging

No packaging script changed. `packaging/smoke.py` now also removes
`GEMINI_SUMMARY_MODEL` and `GEMINI_THINKING_LEVEL` from the package's
environment. `packaging/PORTABLE-README.txt` (copied to `dist/README.txt`)
describes the new download names and the automatic downloads.

The application adds one server function, `summarize_topics` (a five-word
summary from `gemini-3.5-flash-lite`, `GEMINI_SUMMARY_MODEL`), and four
`web-sys` features for the browser build (`HtmlElement`, `Element`, `Node`,
`Storage`). No dependency was added; no route, table or file layout changed.

## Builds and checks

- Host: Windows 11 IoT Enterprise LTSC 10.0.26100, x64. Rust 1.92.0, Dioxus CLI
  0.7.9, committed lockfiles, `packaging/build-windows.ps1` (`--locked`).
- `cargo fmt --check` for the app and launcher: passed.
- `cargo check` (web), server-only check and wasm32 check: passed, 0 warnings.
- `cargo test --features server --no-default-features`: **194 passed**, 4
  ignored (`live_probe`, `voice_live_probe` and `brief_live_probe` call the
  paid API; `dump_fixture` writes files). 0.8.0 had 181; the 13 new tests
  cover the download names (stamp, accent folding, test type, summary and
  fallback words, Windows-safe stems), the UTC stamp, the Flash-Lite price,
  the summary prompt and reply line, and the request without a thinking
  level.
- `brief_live_probe` against the real API: `gemini-3.5-flash-lite` answered
  in 1.2 s with 120 input and 5 output tokens, no thinking, about $0.00005.
- Release WASM is still not size-optimized: the `wasm-opt` that dx 0.7.9
  downloads crashes on Windows (0xc0000409), as since 0.5.0. Size 4.21 MB
  (4,211,940 bytes; 0.8.0: 4,169,393).

## Packaged verification (Windows)

`python packaging/smoke.py dist/listening-exam-generator-0.8.1-windows-x64.exe`
passed on the host: first-run templates (a version 2 `voices.json`), malformed
configuration despite environment overrides, preservation, a 0.7
`voices.json` at its defaults renamed and replaced, a customised one kept with
the *0.7 format* notice, spaces/Unicode paths, invalid settings, blocked
data/log/database paths, occupied port, `/` and `/exam`, JS/WASM/CSS, an
audio-job server function, WAV range and download, graceful shutdown,
relocation, relative `--config-dir`, and closing a real console window with no
payload left in `%TEMP%`.

**Not tested:** starting with no key anywhere, for the same reason as 0.7.x
and 0.8.0: the host's user environment holds `GEMINI_API_KEY` and the smoke
test does not change the registry.

### Download names on the release EXE (paid, $0.0039)

The release EXE itself, copied to a fresh folder and started with
`--config-dir <scratch>\config --no-open --non-interactive` on port 8097, the
key coming from the environment:

- Both pages showed "Download the DOCX and WAV automatically", unticked (a new
  origin, nothing stored).
- HSG Part 1, *Script only*: one `gemini-3.8-flash` request (886 input, 844
  output tokens, 14.7 s, $0.00383), then at once one `gemini-3.5-flash-lite`
  request (119 input, 6 output tokens, no thinking, 1.0 s, $0.00005).
- *Download script* and *Download transcript (Markdown)* saved
  `HSG-Quoc-gia-Listening-Part1_Hanoi-Weekend-Trip-Student-Discussion_06-10-2026_16-34-16_script.txt`
  (4,330 bytes) and `…_16-34-16_transcript.md` (4,771 bytes): one stem, the
  browser's local time, the topic's Vietnamese words folded to ASCII.

Downloads were captured in the page (the anchor's `download` name and the blob
size) rather than written to disk.

### End-to-end on the development server (paid, about $0.19)

`dx serve` on port 8081 from the same sources, with a download interceptor in
the page:

- Part page, IELTS Part 2, automatic downloads on, one click: the DOCX
  downloaded when the second question block arrived, the 9.4 MB WAV when the
  recording finished, both
  `IELTS-Listening-Part2_Riverside-Community-Garden-Volunteer-Orientation_06-10-2026_16-18-29`;
  script, transcript, paper Markdown, DOCX and WAV buttons reused that stem
  with no new Gemini request.
- Automatic downloads off, new questions: nothing downloaded; the manual DOCX
  took a new time with the same five words.
- Step by step (Script only, Generate questions, Generate audio): DOCX and WAV
  share `…_Museum-Science-Gallery-School-Rules_06-10-2026_16-29-52`.
- Whole exam, HSG, four parts generated part by part: no download while
  Part 4's questions had failed (an unrelated `"stem": null` reply), exactly
  one DOCX (`HSG-Quoc-gia-Listening_Urban-Environment-And-City-Life_06-10-2026_16-25-18.docx`)
  once Part 4's questions were regenerated; the spend line lists "file names".
- Opening that saved exam with automatic downloads on: nothing downloaded;
  the summary was asked for on open, so the next manual DOCX was saved within
  150 ms.
- Changing the stored choice as another tab would: the tick box followed it
  within two seconds.

## Linux AppImage

- Built with `packaging/build-linux.ps1` in the Ubuntu 22.04 builder image on
  the project's own `ielts-portable-builder` Podman (WSL) machine. Inside the
  container: `cargo fmt --check`, the three `cargo check`s and
  `cargo test --locked --features server --no-default-features` (**194
  passed**, 4 ignored) ran before `dx build`.
- `listening-exam-generator-0.8.1-linux-x86_64.AppImage`: 13,224,440 bytes (0.8.0: 13,183,480).
  The server links only `libssl.so.3`, `libcrypto.so.3`, `libgcc_s.so.1`,
  `libm.so.6` and `libc.so.6` (`dist/linux-dependencies.txt`).
- `packaging/test-linux.ps1` passed on clean **Ubuntu 22.04 and 24.04**
  containers as UID 65534: first run before Python is installed, manual
  extraction with AppRun, dependency resolution, licenses and inventory, the
  full smoke suite (including the 0.7 `voices.json` cases), **starting with
  no key anywhere**, a missing browser helper, and terminal Ctrl+C through the
  extraction supervisor (exit -2 / shell 130, as documented since 0.5.0).
- Not tested: native FUSE mounting (no `/dev/fuse` in the containers), a
  graphical desktop browser, and download names on Linux with a real key (the
  naming runs in the browser, the same WASM as on Windows).

## Contents and dependencies

The console-subsystem x64 launcher (9,394,688 bytes; 0.8.0: 9,335,808) embeds
only `server.exe` (18,277,888 bytes; 0.8.0: 18,163,712), `public/` and
`RUST-DEPENDENCIES.txt`. Both import tables contain only Windows system DLLs,
unchanged since 0.7.0:

- launcher: `kernel32`, `ntdll`, `bcryptprimitives`, `api-ms-win-core-synch-l1-2-0`
- server: `kernel32`, `ntdll`, `advapi32` (registry), `ws2_32`, `secur32`,
  `crypt32`, `bcrypt`, `bcryptprimitives`, `api-ms-win-core-synch-l1-2-0`

## Paid checks during development

The packaging checks make no Gemini request. All paid checks ran on
2026-10-06, about **$0.19** in all:

| What | Spend |
| --- | --- |
| `brief_live_probe` | $0.00005 |
| End-to-end on the development server: two IELTS Part 2 drafts with recordings, one script lost to a fixed panic, regenerated questions, an HSG exam of four parts, five summaries | $0.186 |
| Script and summary on the release EXE (this record) | $0.0039 |

Of that, the seven Flash-Lite summaries (probe, development server, release EXE) cost $0.0004.

## Artifact SHA-256

| File | SHA-256 |
| --- | --- |
| listening-exam-generator-0.8.1-windows-x64.exe | `0a23377e681d1acc193e1df7c4154152362477eba080d9101ba4f4eb893390a4` |
| listening-exam-generator-0.8.1-linux-x86_64.AppImage | `8b5e72933fb9f91a897a143b606ea5f41f1d2412255cff5d41dcb2b2594573b5` |

Both packages, `README.txt` and the two SHA256SUMS files are published as the
GitHub release `v0.8.1`. Code signing, ARM64 and auto-updates remain out of
scope.
