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

Prereqs: Rust 1.85+ (edition 2024, developed on 1.92), `wasm32-unknown-unknown` target,
Dioxus CLI 0.7.x (`cargo install dioxus-cli --version 0.7.9 --locked`), `.env` with
`GEMINI_API_KEY` (copy from `.env.example`).

```bash
dx serve                                              # dev server, http://localhost:8080, hot reload
dx build --release                                    # server binary + public/ under target/dx/vmq_mvp/release/web/
docker compose up -d --build                          # containerised run on 127.0.0.1:8080
```

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
There is no rustfmt/clippy config; defaults apply.

## Architecture rules (compiler-enforced, keep them that way)

Dependency direction: `ui → application → {domain, infrastructure}`, `infrastructure → domain`,
`export → domain`.

- **`src/domain/` and `src/export/` are pure.** No Dioxus, reqwest, sqlx, tokio. They compile
  identically for wasm and server, and the browser uses them to validate and render the same
  objects the server produces.
- **`#[server]` functions live only in `src/application/`.** Dioxus 0.7 strips server-fn bodies
  from the wasm bundle, so bodies may use `crate::infrastructure` freely, but signatures and
  DTOs must be plain serialisable types. Do not reintroduce `#[cfg(feature = "server")]`
  blocks inside server-fn bodies.
- **`src/infrastructure/` is `#![cfg(feature = "server")]`** and must not define `#[server]`
  functions. `main.rs` gates the module, calls `infrastructure::bootstrap()` (config, data
  dirs, tracing) and then `dioxus::serve` with `dioxus::server::router(App)` plus the one plain
  axum route `GET /audio/{job_id}`; the browser build uses `dioxus::launch`.
- **`src/ui/` talks to the server only through `crate::application`.** The part view holds a
  single `HomeState` signal; the exam view a single `ExamState` signal provided by the `Navbar`
  layout. Both only display issues; rule checks belong to the domain. Browser-side orchestration
  (script first, then questions and recording side by side, `futures_util::future::join`) lives
  in the views and chains the application's server functions; a `run` counter drops late results
  of a cancelled or superseded run.
- Platform-specific deps are split in `Cargo.toml` by `target_arch = "wasm32"`; use
  `#[cfg(target_arch = "wasm32")]` in UI code for gloo/web-sys, not feature flags.

### Domain conventions

- Formats are data. A new exam format is a new `ExamFormat` preset in `domain/format.rs`,
  never a `match` on a format id in business logic.
- Adding a `TaskKind`: one enum variant, then one arm each in `domain/validation.rs`,
  `infrastructure/prompts/items.rs` and `export/markdown.rs`, plus a preset that uses it and a
  unit test for the validator arm. The compiler lists every place.
- Passage speaker labels are always "Speaker A/B/C"; names live inside the lines. TTS and
  grounding checks depend on this.
- Errors are teacher-readable (`DomainError`, `ValidationIssue`). Infrastructure errors are
  converted at the application boundary via `application::user_error`; never surface HTTP
  codes or stack traces. Validation failures are returned as issues beside the draft, never
  swallowed.
- Disallowed by design: CQRS, event sourcing, domain events, repositories without a
  persistence need, trait hierarchies for their own sake.

### Infrastructure facts that shape code

- One `GeminiClient` (`infrastructure/llm/gemini.rs`) owns the retry policy (429/503/timeout,
  exponential backoff) and JSON mode; do not add parallel HTTP paths. The API key goes in the
  `x-goog-api-key` header, never in the URL.
- Gemini multi-speaker TTS takes at most two voices and 8,192 input tokens per request;
  three-voice or long passages are synthesised turn by turn and joined. Output is 24 kHz mono
  16-bit PCM; WAV is encoded/decoded by hand in `infrastructure/audio/wav.rs` (no audio crate).
- Audio synthesis runs as background jobs (SQLite `jobs.db` + WAV under `DATA_DIR/audio/`);
  the browser polls `audio_job_status` (`ui/jobs.rs`, per-kind cadence and deadline) and streams
  the finished WAV from `/audio/{job_id}`, a plain axum route (`infrastructure/jobs/serve.rs`).
  Keep it a plain route: server functions redirect requests that accept `text/html`, which is
  what a download link sends. Jobs older than 24 h are purged hourly, starting at boot.
- All configuration is environment only (`infrastructure/config.rs`, see `.env.example`).

## Dioxus 0.7 API constraints (from `.github/agents/dioxus-0-7-rust-ui-expert.agent.md`)

Use only 0.7 APIs (https://dioxuslabs.com/learn/0.7). `cx`, `Scope` and `use_state` are
removed and must not appear. Components are `#[component] fn Name(...) -> Element`; state is
`use_signal` / `use_memo`; props are owned `PartialEq + Clone` values (`ReadOnlySignal<T>` for
reactive props); assets go through `asset!` and stylesheets through `document::Link` /
`document::Stylesheet`; in `rsx!` prefer `for` loops and inline `if` over iterator helpers.
