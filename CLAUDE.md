# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

One Rust binary (Dioxus 0.7 fullstack, crate `vmq_mvp`) that turns a topic into a draft
**listening exam part**: script, questions, answer key, transcript and audio. Exam formats
are data (`ExamFormat` presets: IELTS Listening, HSG Quốc gia). Text and speech come from
Google Gemini. Everything generated is a draft with validator issues; the teacher decides.

`docs/architecture.md` is the source of truth for layers, module map, pipeline and roadmap.
`docs/domain_model.md` and `docs/ubiquitous_language.md` define the types and words; use
those names (ExamFormat, PartSpec, TaskSpec, TaskKind, Passage, Line, Task, Item, Answer,
Exam, AudioProgram, ValidationIssue, Draft). Retired names never come back:
ListeningSection, Section1–4, GenerationRequest, ListeningScript, GenerationResult.
The CHANGELOG is written in Vietnamese; keep that convention.

## Commands

Prereqs: Rust 1.88+ (edition 2024 let-chains; developed and released on 1.92), `wasm32-unknown-unknown` target,
Dioxus CLI 0.7.x (`cargo install dioxus-cli --version 0.7.9 --locked`), and a Gemini key in
`GEMINI_API_KEY` (environment, or `.env` copied from `.env.example`; without one the app starts
and asks in the browser).

```bash
dx serve                                              # dev server, http://localhost:8080, hot reload
dx build --release                                    # server binary + public/ under target/dx/vmq_mvp/release/web/
docker compose up -d --build                          # containerised run on 127.0.0.1:8080
pwsh -NoProfile -File packaging/build-windows.ps1      # portable EXE in dist/ (needs Rust 1.92.0, dx 0.7.9)
python packaging/smoke.py dist/listening-exam-generator-<version>-windows-x64.exe   # test the real EXE
```

Releases also run `cargo fmt --check` and record results in `docs/portable-verification-v<version>.md`
and the CHANGELOG's "Con số biết nói" table.

Two compile targets share the crate. Both checks must stay green after any change:

```bash
cargo check                                           # browser side (default feature = web)
cargo check --features server --no-default-features   # server side
cargo test  --features server --no-default-features   # all unit tests
```

Tests are inline `#[cfg(test)]` modules (domain, export, and server-only infrastructure:
wav, program, gemini, synthesize), so they need the `server` feature. Run one test by name:

```bash
cargo test --features server --no-default-features multiple_select_needs_distinct_letters
```

or by module path, e.g. `cargo test --features server --no-default-features domain::validation::`.
The ignored `live_probe` test calls the real API (about $0.01) and prints tokens, latency and
audio tokens per second: `cargo test --features server --no-default-features live_probe -- --ignored --nocapture`.
There is no rustfmt/clippy config; defaults apply.

## Architecture rules (compiler-enforced, keep them that way)

Dependency direction: `ui → application → {domain, infrastructure}`, `infrastructure → domain`,
`export → domain`.

- **`src/domain/` and `src/export/` are pure.** No Dioxus, reqwest, sqlx, tokio. They compile
  identically for wasm and server, and the browser uses them to validate and render the same
  objects the server produces (`export/docx.rs` uses `docx-rs` without its `image` feature:
  pure Rust, builds for wasm, so DOCX downloads are made in the browser too).
- **`#[server]` functions live only in `src/application/`.** Dioxus 0.7 strips server-fn bodies
  from the wasm bundle, so bodies may use `crate::infrastructure` freely, but signatures and
  DTOs must be plain serialisable types. Do not reintroduce `#[cfg(feature = "server")]`
  blocks inside server-fn bodies.
- **`src/infrastructure/` is `#![cfg(feature = "server")]`** and must not define `#[server]`
  functions. `main.rs` gates the module and calls `infrastructure::startup::run(App)`:
  `bootstrap()` (validated config loaded once, data dirs, tracing), SQLite, then
  `dioxus::server::router(App)` plus two plain axum routes, `GET /audio/{job_id}` and
  `GET /voice-sample/{voice_id}`, served by `dioxus::serve` in debug (hot reload) or an explicit
  listener otherwise (portable mode opens the browser); startup errors are returned, not
  panicked. The browser build uses `dioxus::launch`.
- **`src/ui/` talks to the server only through `crate::application`.** The part view holds a
  single `HomeState` signal; the exam view a single `ExamState` signal provided by the `Navbar`
  layout. `Navbar` also provides the second context, `VoiceCatalogueCtx` (`ui/components/voices.rs`):
  the voice catalogue, loaded once with the free `voice_catalogue`. Both views assign voices from
  it in the browser with the domain's `assign_voices` / `assign_exam_voices`, writing the state
  only when a voice changed; "Another voice" is `domain::next_voice`. Components keep only
  form-local signals (the speaker dialog's fields, a voice picker's sample, the dialog's
  `DesignedVoicesPanel` list and form, loaded when opened; on both pages). On the exam page
  every speaker edit goes through `set_part_speakers` (reassign, revalidate, `note_edit`).
  Out-of-date scripts and recordings are derived (`Passage::written_for`,
  `ExamPart::recorded_for`, `HomeState.recorded_for`), never flagged on edit, so opening an
  old exam or assigning voices marks nothing. Both views only display
  issues; rule checks belong to the domain. Browser-side orchestration
  (script first, then questions and recording side by side, `futures_util::future::join`) lives
  in the views and chains the application's server functions; a `run` counter drops late results
  of a cancelled or superseded run.
- Platform-specific deps are split in `Cargo.toml` by `target_arch = "wasm32"`; use
  `#[cfg(target_arch = "wasm32")]` in UI code for gloo/web-sys, not feature flags.

### Domain conventions

- Formats are data. A new exam format is a new `ExamFormat` preset in `domain/format.rs`,
  never a `match` on a format id in business logic.
- Adding a `TaskKind`: one enum variant, then one arm each in `domain/validation.rs`,
  `infrastructure/prompts/items.rs`, `export/markdown.rs` and `export/docx.rs` (its
  `answer_layout` match is exhaustive, so the compiler flags it), plus a preset that uses it
  and a unit test for the validator arm.
- Passage speaker labels are always "Speaker A/B/C"; names live inside the lines. TTS and
  grounding checks depend on this.
- Speech markup has one grammar, `domain/speech.rs`. `Line.text` keeps it and only the
  teacher's on-screen script (`script_text()`) shows it. Transcripts never carry markup: question
  prompts, "Download script", Markdown and DOCX use `transcript_text()` / `display_text()`,
  grounding (`normalize`) and `word_count()` strip it, and TTS gets `speech_text()` (documented
  tags kept, anything that would be read aloud dropped). `describe_with_voice()` is for
  transcripts and keys only, never the student paper.
- Adding a speech tag: measure it first with `tools/voice_lab.py probe --only E3` (put it in the
  E3 sentences, mid-sentence, several takes; read the ear's `spoken_markup` and listen). Add it
  to `EXAM_SPEECH_TAGS` only if it is performed and never read aloud; a tag read aloud even once
  goes in `READ_ALOUD_TAGS`. Record the result under G3 in `docs/voices.md`. `SPEECH_TAGS`
  follows Google's documentation only.
- Adding an accent: one `Accent` variant appended to the enum and `Accent::ALL` (never remove or
  rename a variant: saved exams name them), its `label`, `key` and `language_code`, then the
  compiler's exhaustive matches (`minimum_pool` in `tts/voices.rs`, `spelling` in
  `prompts/passage.rs`), a `CELLS` entry in `tools/voice_lab.py`, a pool in
  `tts/default_voices.json` chosen with `voice_lab.py audition` that meets `minimum_pool`
  (`builtin_pools_meet_minimums` checks it) and a row in `docs/voices.md`.
- Every speaker of a part has a voice of its own (`SpeakerConfig.voice: VoiceChoice`: `Auto`,
  `Assigned` by the app, `Chosen` by the teacher and never replaced). The browser assigns as soon
  as the catalogue is there; `start_part_audio` / `start_exam_audio` assign again on the server
  (`application::voices::prepare_speakers`, same rule, parts in order) before `validate()`, which
  refuses `Auto` and shared or wrong-gender voices, and return the speakers they recorded with
  for the views to keep. `describe()` never names the voice (a name would turn up in the script).
- Errors are teacher-readable (`DomainError`, `ValidationIssue`). Infrastructure errors are
  converted at the application boundary via `application::user_error`; never surface HTTP
  codes or stack traces. Validation failures are returned as issues beside the draft, never
  swallowed.
- Disallowed by design: CQRS, event sourcing, domain events, repositories without a
  persistence need, trait hierarchies for their own sake.

### Infrastructure facts that shape code

- One `GeminiClient` (`infrastructure/llm/gemini.rs`) owns the retry policy (429/503/504/timeout,
  exponential backoff, a 429's `retryDelay`), JSON mode and the usage meter; do not add parallel
  HTTP paths. Every request goes through its `send`: text and speech are
  `POST /v1beta/interactions` with `"store": false` (Google stores interactions for 55 days
  otherwise). The Voices API uses the same path: `GET /v1beta/voices` (`list_voices`, free;
  `type=prompted` lists the key's designed voices), `GET` and `DELETE /v1beta/voices/{id}`
  (`get_voice` with a designed voice's free sample, `delete_voice`), and `POST /v1beta/voices`
  with `"store": true`, `type: prompted` and no `voice.model` (`create_voice`, Voice Design): the
  only stored request, never retried after a timeout, metered at TTS rates as an estimate. Ids go
  through `Voice::check_id` before any path is built. The API key goes in the `x-goog-api-key`
  header, never in the URL. A voice Google does not know (404/403 on `voices/{id}`) is
  `LlmError::UnknownVoice`, never a rejected key; a full project is `LlmError::VoiceLimit`.
- Designed voices (`tts/designed.rs`) belong to the key's Google project: at most 200, kept a
  year after their last use, unusable with another key (`prepare_speakers` refuses them at job
  start). The project's list is cached 60 s and forgotten after a create or delete. The
  `designed_voices` table of `jobs.db` records the voices this app made, with their exact
  accent (Google keeps only the language tag); only those may be deleted, and designing or
  deleting is allowed only on a loopback bind (`settings::local_server`, the browser-key rule)
  and limited by `Bucket::VoiceDesign`. Voices made elsewhere in the project (the PO's) are
  listed and usable, never deleted.
- The accent belongs to the voice: pools hold regional library voices (`en-gb-…`, `en-au-…`,
  `en-in-…`); the 30 classic voices (`despina`, `Puck`, …) are all General American and may only
  appear in the American pool. The built-in pools (`tts/default_voices.json`, compiled in,
  chosen with `tools/voice_lab.py audition`, see `docs/voices.md`) list several voices per
  accent and gender plus the announcer; `voices.json` version 2 (`VOICES_PATH`) holds overrides only (a non-empty list
  replaces one pool). A 0.7 file is parsed strictly, then ignored: left at the 0.7 defaults it
  is renamed `voices.0.7.json` and the v2 template written; a customised one is kept and a
  startup notice says "0.7 format".
- Gemini 3.8 TTS reads its input **verbatim**: delivery directions go in each item's
  `speech_metadata.style`, speakers in `speech_metadata.speaker` (`"SpeakerA"`, the label
  without spaces), never in the text. The style is per turn, short and the same for every turn
  of a speaker: its role's `delivery_style()` plus `EXAM_PACE` (long or changing styles make
  voices drift, gate G5: there is no per-line style). Never put accent, gender, age or a name in a
  style. The only markup in the text is what `speech_text` keeps: the few `EXAM_SPEECH_TAGS` an
  expressive script (`PassageRequest.expressive`) asks for, and `|backchannels|` only in a
  two-voice request on a model that is not lite; tags are atomic and never count as words. At most two voices and 8,192
  input tokens per request, so `tts/synthesize.rs` (`plan_passage`) cuts a passage into chunks of
  at most 200 words and two speakers, listing the voices in label order, and joins them; a
  designed voice (`voice_…`) reads alone, one turn per request. At most 3 speech requests run at
  once in the whole process. `tts/cache.rs` (`speech-cache-v2`) reuses a chunk synthesised
  before for the same model, voices, words and per-turn styles (`DATA_DIR/audio/cache`,
  `SPEECH_CACHE_HOURS`); a part requested `fresh` ("New take") is read with `Reuse::Refresh`
  and overwrites those entries, while announcements always reuse. Raw 24 kHz mono 16-bit PCM is requested
  (`audio/l16`, WAV accepted too); WAV is encoded/decoded by hand in
  `infrastructure/audio/wav.rs` (no audio crate).
- Usage: every server function that calls Gemini records its client's `usage()` in the `usage`
  table of `jobs.db` right after the call, whatever the outcome (`application::usage::record`),
  and recording jobs record on completion and on failure. Prices live in
  `infrastructure/llm/pricing.rs` (3.8 introductory rates until 2026-12-31, list rates after);
  update that table when Google changes prices. Over-budget exams are warned about, never blocked.
  Voice samples and designed voices are booked under `UsageStep::Voices`; requests that cost
  nothing add no row.
- Audio synthesis runs as background jobs (SQLite `jobs.db` + WAV under `DATA_DIR/audio/`);
  the browser polls `audio_job_status` (`ui/jobs.rs`, per-kind cadence and deadline) and streams
  the finished WAV from `/audio/{job_id}`, a plain axum route (`infrastructure/jobs/serve.rs`).
  Keep it a plain route: server functions redirect requests that accept `text/html`, which is
  what a download link sends. Job rows store only the WAV file name; readers resolve it under
  the current `DATA_DIR/audio` (`JobRecord::output_file`), so a moved portable folder keeps its
  recordings. Jobs no saved exam refers to are purged hourly from boot once older than
  `AUDIO_RETENTION_HOURS` (default 24, `0` = never); a job named by a saved exam's
  `recording_job` is never purged and is deleted with the exam.
- Voice samples ("Listen") are made once per voice by `voice_preview` (catalogue ids, or the
  designed voices of the key's project; anything else is refused; `Bucket::VoiceSample`),
  stored as `DATA_DIR/audio/voices/{id}.wav`, streamed from the plain route
  `/voice-sample/{voice_id}` for the same reason as `/audio/{job_id}`, and purged after 30 days
  unused. A stored sample is free, and so is a designed voice's: it is Google's own sample.
- Saved exams live in the same `jobs.db` (`infrastructure/exams.rs`: `exams` table with the
  `SavedExam` JSON body plus summary columns) behind `application/exams.rs`. The exam page
  saves on its own after each finished step once a script exists (`SaveWork` in
  `ui/views/exam.rs`, single-flight with one queued follow-up).
- All configuration is environment only (`infrastructure/config.rs`, see `.env.example`). The
  API key is resolved process environment → Windows registry environment (user, then machine)
  → `.env` → a key typed in the browser (`application/settings.rs`, kept in
  `infrastructure/secrets.rs`), which is accepted only on a loopback bind, never over a working
  operator key, after a free check with Google. A missing key does not stop startup. A key Google
  refuses becomes `LlmError::ActiveKeyRejected` (naming its source) and is remembered in
  `secrets`; `KeySetup` polls `api_key_status` and then offers a browser key that replaces it in
  memory until restart. Keys are never retried or tried in turn: there is no automatic fallback.

## Dioxus 0.7 API constraints (from `.github/agents/dioxus-0-7-rust-ui-expert.agent.md`)

Use only 0.7 APIs (https://dioxuslabs.com/learn/0.7). `cx`, `Scope` and `use_state` are
removed and must not appear. Components are `#[component] fn Name(...) -> Element`; state is
`use_signal` / `use_memo`; props are owned `PartialEq + Clone` values (`ReadOnlySignal<T>` for
reactive props); assets go through `asset!` and stylesheets through `document::Link` /
`document::Stylesheet`; in `rsx!` prefer `for` loops and inline `if` over iterator helpers.
A `select` whose options come from a `for` loop must mark the saved option with `selected:` on
each `option`, not `value:` on the `select`: Dioxus writes the value before the looped options
exist, and the browser then shows the first option.
