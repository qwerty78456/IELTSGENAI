# Listening Exam Generator

One Rust binary (Dioxus 0.7 fullstack) that turns a topic into a complete
draft of a **listening exam part**: script, questions, answer key,
transcript and audio. The exam **format** is data; two ship:

- **IELTS Listening** (4 parts, 40 items, everything played once)
- **HSG Quốc gia – Listening** (4 parts, 35 items, parts 3–4 played twice,
  transcribed from the official 2025–2026 paper)

Generation uses Google Gemini for text and speech. Every generated key is
checked against the script by a validator; problems are shown next to the
draft. The teacher edits; nothing is final until they say so.

Read `docs/architecture.md` first. `docs/domain_model.md` and
`docs/ubiquitous_language.md` define the types and words used everywhere.

## Portable Windows and Linux applications

Version 0.5.0 supports a single Windows x64 EXE and a Linux x86-64 AppImage
(Ubuntu 22.04 baseline). Put the package in a writable folder and run it.
First launch creates .env and voices.json beside the package, then stops
with instructions to set GEMINI_API_KEY. Edit .env and run again; the app
opens your browser at http://127.0.0.1:8080. Keep the console open; Ctrl+C or
closing the console window stops it.
No Rust or Docker installation is needed by users. Generation still needs
internet access and a Gemini API key.

Missing configuration files are recreated; existing invalid files are never
replaced. Syntax errors, invalid settings, inaccessible storage, and occupied
ports fail startup with a console message and nonzero exit status. Windows
double-click errors wait for Enter. Run the AppImage in a terminal for errors;
its desktop entry also requests a terminal.

Options: --no-open, --non-interactive, --config-dir PATH. Relative paths
are resolved against the configuration directory. Environment values override
file settings, but malformed files always fail. Configuration is loaded once;
restart after edits. API-key validity with Google is checked when generating.
Exams built on the Whole exam page are saved on the server and reopen after a
restart, recording included. Their recordings are kept until the exam is deleted;
other recordings expire after AUDIO_RETENTION_HOURS (24 by default).

Build instructions and verification are in [docs/portable.md](docs/portable.md).
Builds go to ignored dist/, with SHA-256 checksums and startup instructions.
Do not distribute your .env or generated data/.

## Run locally

Prerequisites: Rust 1.85+ (edition 2024; developed on 1.92), the Dioxus CLI
0.7.x (`cargo install dioxus-cli --version 0.7.9 --locked`), the
`wasm32-unknown-unknown` target, and on Linux `pkg-config libssl-dev`.

```bash
cp .env.example .env      # set GEMINI_API_KEY
dx serve                  # http://localhost:8080, hot reload
```

Checks that must stay green:

```bash
cargo check                                          # browser side (default feature: web)
cargo check --features server --no-default-features  # server side
cargo test  --features server --no-default-features  # unit tests (domain, export, audio, prompts)
```

## Run in Docker

```bash
cp .env.example .env      # set GEMINI_API_KEY
docker compose up -d --build
```

The container listens on `127.0.0.1:8080`. Put a reverse proxy with TLS and
**authentication** in front (Caddy with `basic_auth`, or Cloudflare Access):
the app rate-limits but has no login, and every request spends API credit.
Data (job database, saved exams, WAVs, logs) lives in the `generator_data`
volume; recordings no saved exam refers to are purged after
`AUDIO_RETENTION_HOURS` (24 by default), saved exams keep theirs.

## Configuration

| Variable | Default | Meaning |
|----------|---------|---------|
| `GEMINI_API_KEY` | — | required |
| `GEMINI_TEXT_MODEL` | `gemini-flash-latest` | scripts, topics, questions; alias hot-swapped by Google to the newest Flash release (`gemini-3.8-flash` at the time of writing) |
| `GEMINI_TTS_MODEL` | `gemini-2.5-pro-preview-tts` | speech; paid tier only, on Google's deprecation list (successor `gemini-3.1-flash-tts-preview`), no shutdown date |
| `DATA_DIR` | `./data` | jobs.db, audio/, logs/ |
| `VOICES_PATH` | `$DATA_DIR/voices.json` | gender + accent → voice name; written with defaults if missing |
| `MUSIC_PATH` | unset | 24 kHz mono 16-bit WAV for the start/end of a full exam recording |
| `AUDIO_RETENTION_HOURS` | `24` | hours an unsaved recording is kept; `0` keeps every recording; recordings of saved exams are never purged |
| `IP`, `PORT` | `0.0.0.0`, `8080` | bind address |

Model facts checked on ai.google.dev, 2026-09-21: the TTS model takes at most
two voices and 8,192 input tokens per request (longer scripts and three-voice
parts are read turn by turn), returns 24 kHz mono 16-bit PCM, and is not on
the free tier. Audio output is billed at 25 tokens per second of audio. The
`generateContent` endpoint the client uses is now labelled "Legacy" by
Google beside the newer `interactions` API; it is still documented and
served.

## Layout

```
src/domain/          pure types and rules (formats, passage, tasks, exam, validation, audio programme)
src/application/     #[server] use cases the UI calls
src/infrastructure/  server only: Gemini client, prompts, TTS, WAV, SQLite jobs and saved exams, config
src/export/          Markdown and DOCX paper / key / transcript
src/ui/              Dioxus components and views
docs/                architecture, domain model, ubiquitous language, scope
```

## Status

The restructure into these layers is complete and all checks pass. Two pages:
the part page generates one part's script, then its questions and recording
side by side, with the transcript downloadable as soon as the script exists
(each piece can still be regenerated alone); the exam page (`/exam`) takes a
topic per part and produces the whole paper, the answer key, every
transcript and one exam recording with announcements, pauses and replays,
streamed from `/audio/{job_id}`. Exams on that page are saved on the server
automatically and can be reopened later, recording included; both pages export
Markdown and Word (DOCX, laid out like the paper with answer boxes). See the
roadmap at the end of `docs/architecture.md` for what comes next: MP3,
authentication.
