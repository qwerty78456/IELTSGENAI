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

The app ships as a single Windows x64 EXE (0.9.0) and a Linux x86-64
AppImage (0.9.0, Ubuntu 22.04 baseline). Put the package in a writable folder and run it.
First launch creates .env and voices.json beside the package and opens your
browser at http://127.0.0.1:8080. The Gemini API key comes from the
GEMINI_API_KEY environment variable (on Windows also one set after the console
opened), then from .env; with neither, the console asks for it (shown as `*`,
checked with Google for free, saved in .env), and if you press Enter there the
page asks instead (see "API key" below). Keep the console open; Ctrl+C or
closing the console window stops it.
No Rust or Docker installation is needed by users. Generation still needs
internet access and a Gemini API key.

Missing configuration files are recreated; existing invalid files are never
replaced. Syntax errors, invalid settings, inaccessible storage, and occupied
ports fail startup with a console message and nonzero exit status (except when
you choose to use a copy of the app that is already running, below). Windows
double-click errors wait for Enter. Run the AppImage in a terminal for errors;
its desktop entry also requests a terminal.

Only one copy of the app may run per port and per data folder. When the port or
the data folder is held by another copy of this app on a loopback address
(`IP=127.0.0.1`, the portable default), the console names it (version, process
id, address) instead of a bare "address in use". At an interactive console it
asks "[y/N]" (Enter means no): yes stops that copy and starts this one in its
place; no opens the browser at the copy that is already running and exits. A
copy that runs as a Windows service can be stopped the same way, by an account
allowed to stop that service; the service starts again when this copy stops
(window closed, Ctrl+C, a crash) while you stay signed in to Windows. Signing
out ends the helper that would start it, so the service (Automatic start) then
runs again only from the next boot. Before stopping or ending anything, the app
checks that the copy which answered is the process the system shows listening
on that address. With any other `IP`, or anything else on the port, startup
fails as before. Copies of 0.8.2 and earlier take no data-folder lock: stop
such a copy before starting a newer one on the same data folder, or a
recording it is still making is shown as failed.

Options: --no-open, --non-interactive, --config-dir PATH, --service NAME.
`--service NAME` is for a service manager (see "Run as a Windows service"): it
never asks at the console and never opens a browser. Scheduled tasks and other
unattended starts should pass `--non-interactive` (and `--no-open`), so a
console no one watches never waits for an answer; a run in Windows session 0
(services, a task set to run whether the user is signed in or not, OpenSSH
sessions) is detected and never asks. Relative paths
are resolved against the configuration directory. Environment values override
file settings, but malformed files always fail. Configuration is loaded once;
restart after edits. A key typed at the console or in the browser is checked
with Google before it is kept; other keys are checked when generating. If
Google rejects the configured key, every page says where that key came from
and, in a browser on the server's own computer, lets you paste a working one
that replaces it until the next restart.
Exams built on the Whole exam page are saved on the server and reopen after a
restart, recording included. Their recordings are kept until the exam is deleted;
other recordings expire after AUDIO_RETENTION_HOURS (24 by default). A
recording still being made when the app stops is shown as failed after the next
start ("The server stopped before this recording was finished. Make the
recording again.").
Since 0.8.0 voices.json holds version 2 overrides: a 0.7 file left at its
defaults is renamed voices.0.7.json and a new template is written; a customised
0.7 file is kept but ignored, and the console says so. Designing a voice works
in a browser on the server's own computer and through the published port
(Cloudflare Tunnel, `PUBLIC_PORT`); deleting one only on the server's own
computer.

Build instructions and verification are in [docs/portable.md](docs/portable.md).
Builds go to ignored dist/, with SHA-256 checksums and startup instructions.
Do not distribute your .env or generated data/.

## Run locally

Prerequisites: Rust 1.89+ (edition 2024 let-chains, `File::try_lock`; developed and released on 1.92), the Dioxus CLI
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
per second. `voice_live_probe` (same flags) checks for free that every pooled
voice still exists with its gender and language, then spends about $0.003 on
a three-speaker passage. Nothing else in the test suite spends money;
`tools/voice_lab.py` (see `docs/voices.md`) runs the voice probes and auditions
with a spending cap.

## Run in Docker

```bash
cp .env.example .env      # set GEMINI_API_KEY (a container never accepts a key from the browser)
docker compose up -d --build
```

The container listens on `127.0.0.1:8080`. Put a reverse proxy with TLS and
**authentication** in front (Caddy with `basic_auth`, or Cloudflare Access):
the app rate-limits but has no login, and every request spends API credit.
Inside the container the app binds `0.0.0.0`, so every request is a Remote
request: no key from the browser, no designing or deleting voices (see
"Who may do what" below).
Data (job database, saved exams, WAVs, logs) lives in the `generator_data`
volume; recordings no saved exam refers to are purged after
`AUDIO_RETENTION_HOURS` (24 by default), saved exams keep theirs.

## Run as a Windows service

Run the release binary under a service manager (NSSM) with
`--service NAME`, `NAME` being the service's own name (letters, digits, `.`,
`_`, `-`). The flag never asks at the console (no key prompt, no "[y/N]") and
never opens a browser, and `GET /instance` reports the copy as that service. Put
the key in `GEMINI_API_KEY` (environment or the `.env` of the configuration
directory): without one the startup line says "missing; set GEMINI_API_KEY in
… and restart the service". The app stops cleanly on Ctrl+C (what NSSM sends
first), Ctrl+Break or, on Unix, SIGTERM; open downloads get 5 seconds. A
recording interrupted by the stop is shown as failed after the next start.

## Publish with Cloudflare Tunnel

Keep `IP=127.0.0.1` and set `PUBLIC_PORT` together with `PUBLIC_HOST`, the
tunnel's public hostname (startup fails when `PUBLIC_PORT` is set without it):

```
IP=127.0.0.1
PORT=8080
PUBLIC_PORT=8081
PUBLIC_HOST=app.example.com
```

The app then listens on two ports of the same address: `PORT` for browsers on
the server's own computer, `PUBLIC_PORT` for `cloudflared`, which should point
at `http://127.0.0.1:8081` with Cloudflare Access in front. A request on
`PUBLIC_PORT` counts as an internet user (a Published request) only when its
`Host` (and `Origin`, if sent) is one of the `PUBLIC_HOST` names; anything else
on that port is Remote. This keeps a web page whose name was rebound to
127.0.0.1 (DNS rebinding) from using the published port without passing
Cloudflare Access. Leave the `Host` header as the browser sent it: a tunnel
ingress rule that rewrites it (`httpHostHeader`) makes every tunnel request
Remote. Several hostnames are separated by commas, without a port or scheme.
Any reverse
proxy other than the tunnel must also target `PUBLIC_PORT`, never `PORT`: a
proxy on the main port is recognised only by the forwarding headers it adds.
With a non-loopback `IP` the published port is reachable from the network
without Access, so the app treats it like any other network address and says so
at startup. `PUBLIC_PORT` is ignored under `dx serve`.

### Who may do what

Each request is classified as Local, Published or Remote:

| | Local | Published | Remote |
|---|---|---|---|
| comes from | a browser on the server's own computer, on `PORT` | `PUBLIC_PORT` (the tunnel), for a `PUBLIC_HOST` name | anything else |
| enter an API key in the browser | yes | no | no |
| design a voice | yes | yes | no |
| delete a designed voice (made by this app) | yes | no | no |
| `GET /instance`, `POST /instance/stop` | yes | no (404) | no (404) |

A request is Local only when the server listens on a loopback address, the
request arrived on `PORT`, it carries no forwarding header (`CF-Connecting-IP`,
`X-Forwarded-For`, `Forwarded`, `Via` and the like), its `Host` (and `Origin`,
if sent) names this computer (`localhost`, `127.x.x.x`, `[::1]`), and the
browser does not mark it as made by another site (`Sec-Fetch-Site:
cross-site`). A request is Published only when the server listens on a
loopback address, it arrived on `PUBLIC_PORT`, its `Host` (and `Origin`, if
sent) is a `PUBLIC_HOST` name, and it is not cross-site.

## Configuration

| Variable | Default | Meaning |
|----------|---------|---------|
| `GEMINI_API_KEY` | — | process environment, then the Windows user/machine environment, then `.env`; without any, an interactive console asks and saves it in `.env`, else the page asks (in a browser on the server's own computer, loopback bind only) |
| `GEMINI_TEXT_MODEL` | `gemini-3.8-flash` | scripts, topics, questions; pinned GA model (prices are known for it, not for aliases) |
| `GEMINI_TTS_MODEL` | `gemini-3.8-flash-tts` | speech; any 3.8-generation TTS model, e.g. `gemini-3.8-flash-lite-tts` (a third cheaper) |
| `GEMINI_SUMMARY_MODEL` | `gemini-3.5-flash-lite` | the five-word summary in download names; sent without a thinking level, so the model thinks at its default (minimal on Flash-Lite) |
| `GEMINI_THINKING_LEVEL` | `low` | `low`, `medium` or `high` for text requests; thinking tokens are billed as output |
| `EXAM_BUDGET_USD` | `0.70` | the exam page warns once an exam's Gemini spend passes it; `0` = no budget; nothing is blocked |
| `SPEECH_CACHE_HOURS` | `72` | synthesised speech is reused for identical words, styles, voices and model this long after its last use; `0` = off |
| `DATA_DIR` | `./data` | jobs.db, audio/, logs/ |
| `VOICES_PATH` | `$DATA_DIR/voices.json` | version 2 voice overrides: a list replaces the built-in voices of one accent and gender, `announcer` the announcer; written as an empty template if missing; a 0.7 file is renamed (defaults) or ignored (customised) |
| `MUSIC_PATH` | unset | 24 kHz mono 16-bit WAV for the start/end of a full exam recording |
| `AUDIO_RETENTION_HOURS` | `24` | hours an unsaved recording is kept; `0` keeps every recording; recordings of saved exams are never purged |
| `IP`, `PORT` | `127.0.0.1`, `8080` | bind address (Docker sets `IP=0.0.0.0` itself) |
| `PUBLIC_PORT` | unset | a second port on the same `IP` for Cloudflare Tunnel; requests on it for a `PUBLIC_HOST` name are Published (internet users); must differ from `PORT`; ignored under `dx serve` |
| `PUBLIC_HOST` | unset | required with `PUBLIC_PORT`: the tunnel's hostname, or several separated by commas (no port or scheme, e.g. `app.example.com`); a request on `PUBLIC_PORT` whose `Host` or `Origin` names anything else is Remote; ignored, with a notice, without `PUBLIC_PORT` |

Model facts checked on ai.google.dev and measured, 2026-09-28: every request
goes to the Interactions API (`POST /v1beta/interactions`) with
`"store": false`, so Google keeps no copy (stored interactions are otherwise
kept 55 days). 3.8 Flash TTS reads its input word for word, takes at most two
voices and 8,192 input tokens per request, and bills **32 audio tokens per
second** (measured; the pricing page says 25). The app reads a script in
chunks of at most 200 words and two voices, and asks for raw 24 kHz mono
16-bit PCM. Google's 30 classic voices are all General American, so speakers
are read by regional voices of the Extended Voice Library instead; each voice
in a request also bills its reference audio (740-1,970 input tokens).
Measurements and decisions are in `docs/voices.md`.

### Voices

Every speaker of a part gets a voice of its own that fits its gender and
accent: British, American, Australian, Canadian, New Zealand, Irish,
Scottish, South African or Indian English, from pools built into the app
(`src/infrastructure/tts/default_voices.json`). Parts of one exam prefer
different voices. The speaker cards on both pages show each voice with
**Listen** (the first listen of a voice records a short sample for about
$0.005; later listens are free), **Another voice** and **Automatic**. In the
speaker dialog, **Designed voices** lists the voices of the API key's Google
project and creates one from a description with Gemini Voice Design (about
20 s and $0.01; at most 200 per project, kept a year after last use; in a
browser on the server's own computer or through `PUBLIC_PORT`). Only voices
this app made can be deleted, and only in a browser on the server's own
computer. A designed voice reads each of its turns
in a request of its own. With **Expressive delivery** (on by default) a script
may carry a few `<sigh>`, `<cough>`, `<laugh>` or `<chuckle>` tags that the
voice performs; they never appear in transcripts, the paper or the DOCX. When
a speaker's gender, accent or role changes after the script was written, the
page offers to rewrite the script or keep it; a changed voice marks the
recording to render again, and **New take** reads a part again without the
speech cache.

### Downloads

Every DOCX, WAV and Markdown download is named after the test, five words
about the draft and the browser's time, day first:
`IELTS-Listening-Part1_Booking-A-Hotel-Room-Online_06-10-2026_14-32-05.docx`
(the exam page leaves the part out). The five words come from
`gemini-3.5-flash-lite` right after a script is written (about $0.0001 per
draft, booked as "file names"); without them the files are named after the
topics. The DOCX and the WAV of one draft share a name; new questions or a
new take get a new time. **Download the DOCX and WAV automatically** (on
both pages, remembered in this browser, off by default) saves the DOCX as
soon as the questions are written and the WAV as soon as the recording is
ready; opening a saved exam never downloads anything. Chrome and Edge ask
once whether the site may download several files: allow it, or later
downloads are blocked silently.

### API key

The key is looked up in the process environment, then (Windows) in the user
and machine environment stored in the registry, so a variable set after the
terminal or IDE opened still counts, then in `.env`. The console says where it
came from, never the key. With none, a run started from an interactive console
asks for it first: the key is shown as `*`, checked with a free Google request
and saved in `.env`; Enter alone skips to the browser, and a key Google cannot
check (busy, offline) is saved only if you say so. On a network bind (`IP`
other than a loopback address) no browser may enter a key, so the prompt offers
to skip instead and says to set `GEMINI_API_KEY` and restart. A run with
`--service` or `--non-interactive`, without a console, in Windows session 0 or
under `dx serve` never asks. The
server then starts anyway and every page shows a form to paste one, under a
security warning: the key travels over plain HTTP and the app has no login. The
form works only in a browser on the server's own computer (a Local request:
loopback bind, `PORT`, not through a proxy or the tunnel) and never replaces a
key from the environment or `.env`. The key is checked with a free Google
request and kept in memory; tick "Remember" to write it (unencrypted) to
`.env`. When the app saves a key in `.env` (console or "Remember"), only the
file's owner may read it afterwards: mode 0600 on Unix, and on Windows, on an
NTFS drive, access for the owner, SYSTEM and Administrators only. A FAT or
exFAT drive (a typical USB stick) keeps no permissions, so there the file is
readable by every account on the computer; the console and the log say so.

### What an exam costs

Every Gemini request is metered from its `usage` and priced at the rate in
force (3.8 introductory prices until 2026-12-31, list prices after). The exam
page shows the exam's spend per step against `EXAM_BUDGET_USD`; the saved
exams panel shows the server's last 24 hours and 30 days; each request is also
logged. Measured on 2026-10-05 with 0.8.0, one full IELTS exam (4 topics,
4 expressive scripts, 6 question blocks, one 30-minute recording with regional
voices, Part 3 on Irish and Scottish voices) at `GEMINI_THINKING_LEVEL=low`:

| | now | from 2027-01-01 |
|---|---|---|
| topics, scripts, questions (gemini-3.8-flash) | $0.042 | $0.084 |
| recording (gemini-3.8-flash-tts, 19 requests, 30:18) | $0.322 | $0.644 |
| **whole exam** | **$0.364** | **$0.728** |

From 2027 that is over the default $0.70 budget (0.7.0 measured $0.308 and
$0.616; part of the rise is the reference audio each regional voice bills on
every request, part a longer recording). Rendering the recording again
without changes cost $0: every chunk was reused. A new take of one part cost
$0.052; the first listen of a voice about $0.005, later listens nothing. The
0.7.0 exam at `medium` cost $0.48 ($0.96 from 2027).

## Layout

```
src/domain/          pure types and rules (formats, passage, tasks, exam, validation, audio programme)
src/application/     #[server] use cases the UI calls
src/infrastructure/  server only: Gemini client (Interactions and Voices API) and prices, prompts, voice
                     catalogue, samples and designed voices, TTS chunks and speech cache, WAV, SQLite
                     jobs, saved exams and usage ledger, config and key
src/export/          Markdown and DOCX paper / key / transcript
src/ui/              Dioxus components and views
docs/                architecture, domain model, ubiquitous language, scope, voices
tools/voice_lab.py   voice catalogue, probes, auditions and F0 / AI-ear reports (not packaged)
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
speaker is read by a regional voice of its own that the teacher can listen to,
change or design on either page. Every Gemini request is metered: the exam
page shows what the exam has cost against its budget. See the roadmap at the
end of `docs/architecture.md` for what comes next: MP3, authentication.
