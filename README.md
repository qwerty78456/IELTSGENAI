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

The app ships as a single Windows x64 EXE (0.7.0) and a Linux x86-64
AppImage (Ubuntu 22.04 baseline; the newest Linux build is 0.6.0). Put the package in a writable folder and run it.
First launch creates .env and voices.json beside the package and opens your
browser at http://127.0.0.1:8080. The Gemini API key comes from the
GEMINI_API_KEY environment variable (on Windows also one set after the console
opened), then from .env; with neither, the page asks for it (see "API key"
below). Keep the console open; Ctrl+C or closing the console window stops it.
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
restart after edits. A key typed in the browser is checked with Google before
it is kept; other keys are checked when generating.
Exams built on the Whole exam page are saved on the server and reopen after a
restart, recording included. Their recordings are kept until the exam is deleted;
other recordings expire after AUDIO_RETENTION_HOURS (24 by default).

Build instructions and verification are in [docs/portable.md](docs/portable.md).
Builds go to ignored dist/, with SHA-256 checksums and startup instructions.
Do not distribute your .env or generated data/.

## Run locally

Prerequisites: Rust 1.88+ (edition 2024 let-chains; developed and released on 1.92), the Dioxus CLI
0.7.x (`cargo install dioxus-cli --version 0.7.9 --locked`), the
`wasm32-unknown-unknown` target, and on Linux `pkg-config libssl-dev`.

```bash
cp .env.example .env      # optional: GEMINI_API_KEY here, or in the environment
dx serve                  # http://localhost:8080, hot reload
```

Checks that must stay green:

```bash
cargo check                                          # browser side (default feature: web)
cargo check --features server --no-default-features  # server side
cargo test  --features server --no-default-features  # unit tests (domain, export, audio, prompts, usage)
```

`cargo test --features server --no-default-features live_probe -- --ignored --nocapture`
calls the real API (about $0.01) and prints tokens, latency and audio tokens
per second; nothing else in the test suite spends money.

## Run in Docker

```bash
cp .env.example .env      # set GEMINI_API_KEY (a container never accepts a key from the browser)
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
| `GEMINI_API_KEY` | — | process environment, then the Windows user/machine environment, then `.env`; without any, the page asks (loopback servers only) |
| `GEMINI_TEXT_MODEL` | `gemini-3.8-flash` | scripts, topics, questions; pinned GA model (prices are known for it, not for aliases) |
| `GEMINI_TTS_MODEL` | `gemini-3.8-flash-tts` | speech; any 3.8-generation TTS model, e.g. `gemini-3.8-flash-lite-tts` (a third cheaper) |
| `GEMINI_THINKING_LEVEL` | `low` | `low`, `medium` or `high` for text requests; thinking tokens are billed as output |
| `EXAM_BUDGET_USD` | `0.70` | the exam page warns once an exam's Gemini spend passes it; `0` = no budget; nothing is blocked |
| `SPEECH_CACHE_HOURS` | `72` | synthesised speech is reused for identical words, voices and model this long after its last use; `0` = off |
| `DATA_DIR` | `./data` | jobs.db, audio/, logs/ |
| `VOICES_PATH` | `$DATA_DIR/voices.json` | gender + accent → voice name; written with defaults if missing |
| `MUSIC_PATH` | unset | 24 kHz mono 16-bit WAV for the start/end of a full exam recording |
| `AUDIO_RETENTION_HOURS` | `24` | hours an unsaved recording is kept; `0` keeps every recording; recordings of saved exams are never purged |
| `IP`, `PORT` | `0.0.0.0`, `8080` | bind address |

Model facts checked on ai.google.dev and measured, 2026-09-28: every request
goes to the Interactions API (`POST /v1beta/interactions`) with
`"store": false`, so Google keeps no copy (stored interactions are otherwise
kept 55 days). 3.8 Flash TTS reads its input word for word, takes at most two
voices and 8,192 input tokens per request, and bills **32 audio tokens per
second** (measured; the pricing page says 25). The app reads a script in
chunks of at most 200 words and two voices, and asks for raw 24 kHz mono
16-bit PCM.

### API key

The key is looked up in the process environment, then (Windows) in the user
and machine environment stored in the registry, so a variable set after the
terminal or IDE opened still counts, then in `.env`. The console says where it
came from, never the key. With none, the server starts anyway and every page
shows a form to paste one, under a security warning: the key travels over
plain HTTP and the app has no login. The form works only when the server is
bound to 127.0.0.1 / ::1 and never replaces a key from the environment or
`.env`. The key is checked with a free Google request and kept in memory; tick
"Remember" to write it (unencrypted) to `.env`.

### What an exam costs

Every Gemini request is metered from its `usage` and priced at the rate in
force (3.8 introductory prices until 2026-12-31, list prices after). The exam
page shows the exam's spend per step against `EXAM_BUDGET_USD`; the saved
exams panel shows the server's last 24 hours and 30 days; each request is also
logged. Measured on 2026-09-28, one full IELTS exam (4 topics, 4 scripts,
6 question blocks, one 29-minute recording) at `GEMINI_THINKING_LEVEL=low`:

| | now | from 2027-01-01 |
|---|---|---|
| topics, scripts, questions (gemini-3.8-flash) | $0.039 | $0.078 |
| recording (gemini-3.8-flash-tts, 29,728 audio tokens) | $0.269 | $0.538 |
| **whole exam** | **$0.308** | **$0.616** |

The same exam at `medium` cost $0.48 ($0.96 from 2027). Rendering the
recording again without changes cost $0: every chunk was reused.

## Layout

```
src/domain/          pure types and rules (formats, passage, tasks, exam, validation, audio programme)
src/application/     #[server] use cases the UI calls
src/infrastructure/  server only: Gemini client (Interactions API) and prices, prompts, TTS chunks and
                     speech cache, WAV, SQLite jobs, saved exams and usage ledger, config and key
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
Markdown and Word (DOCX, laid out like the paper with answer boxes). Every
Gemini request is metered: the exam page shows what the exam has cost against
its budget. See the roadmap at the end of `docs/architecture.md` for what comes
next: MP3, authentication.
