# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

One Rust binary (Dioxus 0.7 fullstack, crate `vmq_mvp`) that turns a topic into a draft
**listening exam part**: script, questions, answer key, transcript and audio. Exam formats
are data (`ExamFormat` presets: IELTS Listening, HSG Quốc gia). Text comes from Google Gemini
or Anthropic Claude (`TextProvider`: `TEXT_PROVIDER` is the server's default, and both pages
may choose another for a run); speech, voice samples and Voice Design always come from Gemini.
Everything generated is a draft with validator issues; the teacher decides.

`docs/architecture.md` is the source of truth for layers, module map, pipeline and roadmap.
`docs/domain_model.md` and `docs/ubiquitous_language.md` define the types and words; use
those names (ExamFormat, PartSpec, TaskSpec, TaskKind, Passage, Line, Task, Item, Answer,
Exam, AudioProgram, ValidationIssue, Draft, TextProvider). Retired names never come back:
ListeningSection, Section1–4, GenerationRequest, ListeningScript, GenerationResult.
The CHANGELOG is written in Vietnamese; keep that convention.

## Commands

Prereqs: Rust 1.89+ (edition 2024 let-chains, `File::try_lock`; developed and released on 1.92), `wasm32-unknown-unknown` target,
Dioxus CLI 0.7.x (`cargo install dioxus-cli --version 0.7.9 --locked`), and a Gemini key in
`GEMINI_API_KEY` (environment, or `.env` copied from `.env.example`; without one a release or
portable run from an interactive console asks for it and saves it in `.env`, and otherwise the
app starts and, on a loopback bind, asks in a browser on the server's own computer; on a network
bind (`IP=0.0.0.0`) every request is Remote, so only setting the key and restarting helps;
`dx serve`, `--service` and `--non-interactive` never ask at the console). Claude text also needs
`ANTHROPIC_API_KEY`, found and entered the same way; the console asks for it, after the Gemini
key, only with `TEXT_PROVIDER=anthropic`, and otherwise a page that chooses Claude asks for it in
the browser. Recordings always need the Gemini key.

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
wav, program, gemini, claude, text, synthesize), so they need the `server` feature. Run one test by name:

```bash
cargo test --features server --no-default-features multiple_select_needs_distinct_letters
```

or by module path, e.g. `cargo test --features server --no-default-features domain::validation::`.
The ignored `live_probe` test calls the real API (about $0.01) and prints tokens, latency and
audio tokens per second: `cargo test --features server --no-default-features live_probe -- --ignored --nocapture`.
The ignored `claude_paid_probe` (about $0.03: the free key check and `count_tokens`, then a topic,
the longest IELTS script, its questions and a summary on Claude) and `text_provider_compare_probe`
(about $0.17: one IELTS exam's whole text on Gemini and on Claude, compared) are paid too;
ask before running either. Their names keep them out of the `live_probe` filter:
`cargo test --features server --no-default-features claude_paid_probe -- --ignored --nocapture`.
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
  `prepare()` (validated config loaded once, data dirs, tracing), then on the release path the
  data-folder lock (`DATA_DIR/instance.lock`, held for the whole process) and the binds of the
  main address and, with `PUBLIC_PORT`, the published port (`listeners`), then `finish()`
  (the console key prompts, the key notes, the text provider and notices, `config::initialize`),
  SQLite, `instance.json`, the interrupted-job sweep, and one router: `dioxus::server::router(App)` plus four plain axum
  routes, `GET /audio/{job_id}`, `GET /voice-sample/{voice_id}`, `GET /instance` and
  `POST /instance/stop` (`infrastructure/instance/`, Local requests only). The release path
  serves that router on both listeners (the published one layers `ingress::PublishedListener`
  over it) until Ctrl+C, Ctrl+Break, SIGTERM or `POST /instance/stop`, then drains for at most
  5 s; portable mode opens the browser. `dioxus::serve` in debug (hot reload) serves the main
  address only and ignores `PUBLIC_PORT`. A port or data folder held by another copy of this
  app on a loopback bind is identified through `GET /instance` (`instance::probe`), and its
  process id is acted on only when the system shows that process alone listening on the address
  that answered (`instance::listener`); at an
  interactive console a [y/N] prompt (default N) may stop it (`instance::takeover`: a console
  copy through `POST /instance/stop` with the token from its `instance.json`, a Windows service
  through the service control manager with a hidden watcher that starts it again when this
  copy exits while the user stays signed in; signing out ends the watcher, and the service
  then starts at the next boot) or use it (open the browser to it and exit 0). `--service NAME`
  (for NSSM) never asks and never opens a browser; other unattended starts (scheduled tasks)
  pass `--non-interactive`, and Windows session 0 is detected and never asked. Startup errors are returned, not panicked. The browser build
  uses `dioxus::launch`.
- **`src/ui/` talks to the server only through `crate::application`.** The part view holds a
  single `HomeState` signal; the exam view a single `ExamState` signal provided by the `Navbar`
  layout. A run's text provider is the page state's `text_provider` (`Option<TextProvider>`,
  `None` follows `TEXT_PROVIDER`), chosen in `TextProviderPicker`, bound before each call awaits
  and passed as the last argument of `suggest_topic`, `generate_passage`, `generate_task` and
  `summarize_topics` (`NameRequest.provider`); both pages keep it across a format switch, and the
  exam page saves it (`SavedExam.text_provider`, left out of the body when `None`, so an exam
  without a choice saves exactly as 0.9 did) and calls `note_edit` on a change. `Navbar` also
  provides two more contexts. `KeyStatusCtx` (`ui/components/key_setup.rs`) holds both keys'
  `KeyStatuses` (`None` until the first answer); it has two writers, `KeySetup`'s 5 s
  `api_key_status` poll, which sets it only when it changed, and a `KeyBox` whose key was
  accepted (`set_api_key`'s answer). The pure `key_box_place` puts a provider's `KeyBox` at the
  top (the Gemini key, the default provider's, or any rejected key), under the page's
  `TextProviderPicker` (the other provider's missing key, once the page chose it) or nowhere.
  `VoiceCatalogueCtx` (`ui/components/voices.rs`) is the voice catalogue, loaded once with the
  free `voice_catalogue`. Both views assign voices from
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
- Downloads are named by `export::naming` (pure: test type, five-word summary, browser time
  `DD-MM-YYYY_HH-MM-SS`) through `ui/naming.rs`: each page state carries a `DraftNaming`
  (`AsRef`/`AsMut`), the summary is asked for right after a script is written (`prefetch`) and
  kept with the topics it sums up (derived, never flagged), the first download fixes the stem for
  the draft, and a step that changes a downloadable file calls `new_draft()`. Take a download's
  content synchronously, then `spawn` (part page) or `spawn_forever` (exam page) the naming.
  Never hold a `peek()`/`read()` guard across a call that writes the same signal: bind the value
  first, since an `if let` keeps its scrutinee's guard alive through the block. Automatic
  downloads (`ui/prefs.rs`, `localStorage`, read after mount, off by default) fire only where a
  page sees questions complete or a recording finish, never when opening a saved exam
  (`fetch_exam_audio(.., auto: false)`).
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

- One client per provider, each owning its HTTP path, retry policy, JSON handling and usage
  meter; do not add other HTTP paths. Application code reaches text only through
  `llm::TextClient` (`llm/text.rs`, a plain enum: `from_config(choice)` takes the page's choice,
  else `TEXT_PROVIDER`, through `TextProvider::resolve` and never switches provider), and speech
  and voices only through `GeminiClient`.
- `GeminiClient` (`infrastructure/llm/gemini.rs`) owns the retry policy (429/503/504/timeout,
  exponential backoff, a 429's `retryDelay`), JSON mode and the usage meter for Gemini. Every
  request goes through its `send`: text and speech are
  `POST /v1beta/interactions` with `"store": false` (Google stores interactions for 55 days
  otherwise). The Voices API uses the same path: `GET /v1beta/voices` (`list_voices`, free;
  `type=prompted` lists the key's designed voices), `GET` and `DELETE /v1beta/voices/{id}`
  (`get_voice` with a designed voice's free sample, `delete_voice`), and `POST /v1beta/voices`
  with `"store": true`, `type: prompted` and no `voice.model` (`create_voice`, Voice Design): the
  only stored request, never retried after a timeout, metered at TTS rates as an estimate. Ids go
  through `Voice::check_id` before any path is built. The API key goes in the `x-goog-api-key`
  header, never in the URL. A voice Google does not know (404/403 on `voices/{id}`) is
  `LlmError::UnknownVoice`, never a rejected key; a full project is `LlmError::VoiceLimit`.
  `generate_brief(model, ..)` (the download-name summary, `GEMINI_SUMMARY_MODEL`, default
  `gemini-3.5-flash-lite`) is the one text request that names its own model and sends no
  `thinking_level`, so each model thinks at its default; record its usage with that model. The
  summary model follows the run's provider: `TextClient::summary_model()` is
  `GEMINI_SUMMARY_MODEL` or `ANTHROPIC_SUMMARY_MODEL` (default `claude-haiku-5-5`), and
  `summarize_topics` books the spend under it.
- `ClaudeClient` (`infrastructure/llm/claude.rs`) writes text only: `POST /v1/messages` under the
  hard-coded root `https://api.anthropic.com/v1` (`ANTHROPIC_BASE_URL` and `ANTHROPIC_AUTH_TOKEN`
  are never read; redirects are not followed), with `x-api-key` (marked sensitive, never in the
  URL) and `anthropic-version: 2023-06-01`; the free key check is `GET /v1/models?limit=1`. A
  body is `model`, `max_tokens` (8,192; 1,024 for the brief), one fixed `system` line against
  preambles, the prompt as one user turn and `output_config.effort` (`ANTHROPIC_EFFORT`; `low`
  for the brief), the only thinking control: never `thinking`, sampling parameters, an assistant
  prefill, `cache_control` or `stream`. Text-model requests add `"fallbacks": "default"` with
  `anthropic-beta: server-side-fallback-2026-07-01`; the brief (`ANTHROPIC_SUMMARY_MODEL`, Haiku,
  which has no fallback) sends neither. Usage is priced per `usage.iterations` entry at its own
  model when there are some, else (or for an entry naming none) by the served `model`, then the
  requested one. It retries 429 (never the spend-cap `enforced_spend_limit_reached`), 500, 502,
  503 and 529 at most twice, only when the error came within 10 s, `x-should-retry` is not
  `false` and `retry-after` is at most 20 s (waiting it, else 1 s then 2 s with jitter), and only
  while the next attempt still has 60 s of the 100 s budget (5 s of 20 s for the brief) that
  keeps a request under the tunnel's 125 s. A connection that never opened is retried once;
  nothing is retried after a timeout (Anthropic may still bill it) or a 504 (documented as
  retryable; not retried here because it arrives near the tunnel limit and may be billed). It
  meters a response before checking `stop_reason`, so cut-off and refused replies count. Only a
  401 marks the Anthropic key rejected; 402, a 400 about the credit balance, usage limits or a
  spend limit and the spend-cap 429 are `ClaudeAccountBlocked`; the teacher sees `error.message`
  only from Anthropic's error envelope, never a proxy's page or an HTTP code.
- Designed voices (`tts/designed.rs`) belong to the key's Google project: at most 200, kept a
  year after their last use, unusable with another key (`prepare_speakers` refuses them at job
  start). The project's list is cached 60 s and forgotten after a create or delete. The
  `designed_voices` table of `jobs.db` records the voices this app made, with their exact
  accent (Google keeps only the language tag); only those may be deleted. Designing is allowed
  for Local and Published requests (`voices::design_refusal`, `Bucket::VoiceDesign`), deleting
  for Local requests only (`voices::delete_refusal`, `Bucket::VoiceDelete`); the refusal check
  comes before the rate limit. Voices made elsewhere in the project (the PO's) are listed and
  usable, never deleted.
- Who may change a key or the Google project is decided per request by
  `infrastructure::ingress` (`Origin::{Local, Published, Remote}`): `ingress::current()` at the
  start of a server-fn body (the request context does not follow `tokio::spawn`; with no request
  the answer is Remote), `ingress::of_parts` in plain routes. `classify` is pure and the first match wins: a
  non-loopback bind → Remote; `Sec-Fetch-Site: cross-site` → Remote; the `PUBLIC_PORT` listener
  → Published when its `Host` (port aside) and its `Origin`, if any, name a `PUBLIC_HOST` (the
  tunnel hostname, required with `PUBLIC_PORT`), else Remote; any `FORWARDING_HEADERS` (`cf-connecting-ip`, `x-forwarded-for`, `forwarded`,
  `via`, …) → Remote; a `Host` or `Origin` that does not name this computer (`localhost`,
  `127.x.y.z`, `[::1]`) → Remote; else Local. Application refusals stay pure bool functions
  (`settings::entry_refusal`, `voices::design_refusal`, `voices::delete_refusal`) so they
  compile for wasm. Takeover (`/instance`) therefore works only on a loopback bind, and a reverse
  proxy other than Cloudflare Tunnel must target `PUBLIC_PORT`, never the main port.
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
- Usage: every server function that calls a model (Gemini or Claude) records its client's
  `usage()` in the `usage` table of `jobs.db` right after the call, whatever the outcome
  (`application::usage::record(step, exam, model, &Usage)`), and recording jobs record on
  completion and on failure. The ledger has no provider column: the model tells them apart, and
  the exam page says "AI spend". Prices live in `infrastructure/llm/pricing.rs`: Gemini 3.8
  introductory rates until 2026-12-31, list rates after; Claude has one price (per million
  input / cached / output tokens: `claude-sonnet-5-5` $2 / $0.10 / $10, its fallback
  `claude-sonnet-5` $2 / $0.20 / $10, `claude-haiku-5-5` $0.10 / $0.01 / $0.50 for prompts up
  to 100K tokens, `claude-opus-5-5` $4 / $0.20 / $20). Update that table when Google or
  Anthropic changes prices; a model missing there is counted as unpriced ("+?"). Over-budget
  exams are warned about, never blocked.
  Voice samples and designed voices are booked under `UsageStep::Voices`, download-name
  summaries under `UsageStep::Naming` (`Bucket::Naming`); requests that cost nothing add no row.
- Audio synthesis runs as background jobs (SQLite `jobs.db` + WAV under `DATA_DIR/audio/`);
  the browser polls `audio_job_status` (`ui/jobs.rs`, per-kind cadence and deadline) and streams
  the finished WAV from `/audio/{job_id}`, a plain axum route (`infrastructure/jobs/serve.rs`).
  Keep it a plain route: server functions redirect requests that accept `text/html`, which is
  what a download link sends. Job rows store only the WAV file name; readers resolve it under
  the current `DATA_DIR/audio` (`JobRecord::output_file`), so a moved portable folder keeps its
  recordings. Jobs no saved exam refers to are purged hourly from boot once older than
  `AUDIO_RETENTION_HOURS` (default 24, `0` = never); a job named by a saved exam's
  `recording_job` is never purged and is deleted with the exam. At startup, before serving, a
  server holding the data-folder lock fails every `pending`/`processing` job
  (`JobStore::fail_interrupted`, `jobs::INTERRUPTED_MESSAGE`: no task makes them any more);
  without the lock (another live copy, or a file system that cannot lock) they are left alone,
  and under `dx serve` this happens once, not on every hot-patch. Every answer of `/audio` and
  `/voice-sample`, errors included, carries `Cache-Control: private, no-cache` (no shared cache
  such as Cloudflare keeps a recording; the browser still revalidates with 304).
- Voice samples ("Listen") are made once per voice by `voice_preview` (catalogue ids, or the
  designed voices of the key's project; anything else is refused; `Bucket::VoiceSample`),
  stored as `DATA_DIR/audio/voices/{id}.wav`, streamed from the plain route
  `/voice-sample/{voice_id}` for the same reason as `/audio/{job_id}`, and purged after 30 days
  unused. A stored sample is free, and so is a designed voice's: it is Google's own sample.
- Saved exams live in the same `jobs.db` (`infrastructure/exams.rs`: `exams` table with the
  `SavedExam` JSON body plus summary columns) behind `application/exams.rs`. The exam page
  saves on its own after each finished step once a script exists (`SaveWork` in
  `ui/views/exam.rs`, single-flight with one queued follow-up).
- All configuration is environment only (`infrastructure/config.rs`, see `.env.example`). Each
  API key (`GEMINI_API_KEY`, `ANTHROPIC_API_KEY`, named by `TextProvider::key_env`; one rule for
  both, `config::find_key`) is resolved process environment → Windows registry environment
  (user, then machine) → `.env` → a key typed in the browser (`application/settings.rs`, kept in
  `infrastructure/secrets.rs` with one slot per provider, so a Claude key never replaces or marks
  the Gemini key, nor the reverse), which is accepted only from a Local request, never over a
  working operator key, after a free check with its company (Google, or Anthropic's
  `GET /v1/models?limit=1`, which proves the key, not the account's balance). A missing key does
  not stop startup: first an interactive console (release path; never with `--service`,
  `--non-interactive`, non-console stdio, Windows session 0 or under `dx serve`) asks for the
  Gemini key, then, only with `TEXT_PROVIDER=anthropic`, for the Anthropic key
  (`ask_for_key(provider, ..)` in `infrastructure::finish`), each masked with `*`
  (`console::ask_secret`), checked for free and saved in `.env` under its own name, at most three
  tries; Enter alone leaves it to the browser on a loopback bind, and on a network bind the
  prompt offers to skip and says to set the key's variable and restart. The startup lines follow:
  `Gemini API key (GEMINI_API_KEY): …` (byte-identical; `smoke.py` and `check-clean.sh` read its
  "missing"), `Text provider (TEXT_PROVIDER): …` and `Anthropic API key (ANTHROPIC_API_KEY): …`
  ("not set; needed only when a page chooses Claude…" while Gemini is the default).
  `config::remember_in_dotenv` writes `.env` readable by its owner only (0600; on Windows a
  protected owner/SYSTEM/Administrators DACL on a volume that keeps permissions; FAT/exFAT
  cannot, and the console and log warn). A key Google refuses becomes
  `LlmError::ActiveKeyRejected`, a key Anthropic refuses with a 401
  `LlmError::ClaudeActiveKeyRejected` (each naming its source); it is remembered in `secrets` for
  its provider only, and `KeySetup` polls `api_key_status` and then offers a browser key that
  replaces it in memory until restart. Anthropic account trouble (no balance, a usage or spend
  limit) is `ClaudeAccountBlocked` and leaves the key marked good. `TEXT_PROVIDER` must be
  `gemini` or `anthropic` (blank: `gemini`); a blank Anthropic setting means its default, and a malformed one stops
  startup only with `TEXT_PROVIDER=anthropic`: otherwise it is a startup notice and
  `AppConfig::anthropic_problem`, a run that chooses Claude gets `ClaudeMisconfigured`, and
  `set_api_key` refuses an Anthropic key until the setting is fixed and the app restarts. Keys and
  providers are never retried or tried in turn: there is no automatic fallback, and a failed
  Claude request is never repeated on Gemini, nor the reverse.

## Dioxus 0.7 API constraints (from `.github/agents/dioxus-0-7-rust-ui-expert.agent.md`)

Use only 0.7 APIs (https://dioxuslabs.com/learn/0.7). `cx`, `Scope` and `use_state` are
removed and must not appear. Components are `#[component] fn Name(...) -> Element`; state is
`use_signal` / `use_memo`; props are owned `PartialEq + Clone` values (`ReadOnlySignal<T>` for
reactive props); assets go through `asset!` and stylesheets through `document::Link` /
`document::Stylesheet`; in `rsx!` prefer `for` loops and inline `if` over iterator helpers.
A `select` whose options come from a `for` loop must mark the saved option with `selected:` on
each `option`, not `value:` on the `select`: Dioxus writes the value before the looped options
exist, and the browser then shows the first option.
