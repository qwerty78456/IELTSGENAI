# Architecture

One Rust binary (Dioxus 0.7 fullstack) that turns a **topic** into a
complete **listening exam part**: script, questions, key, transcript and
audio, for any exam **format** the domain describes. Two formats ship:
IELTS Listening and the Vietnamese national gifted-student exam (HSG Quốc
gia, Listening section).

## Bounded context

**Listening Assessment Generation.** Everything the teacher needs to run a
listening test, generated as drafts the teacher reviews. Out of scope:
grading, candidate management, official-exam claims, any other skill.

```
                    ┌───────────────────────────────────────────┐
   teacher ───────► │  UI (Dioxus, wasm)                        │
                    │  ui/views/{home,exam}.rs                  │
                    └───────────────┬───────────────────────────┘
                                    │ #[server] calls (HTTP, serde); GET /audio/{job_id} streams the WAV
                    ┌───────────────▼───────────────────────────┐
                    │  Application (use cases)                  │
                    │  application/{topics,passages,tasks,audio}│
                    └──────┬─────────────────────────┬──────────┘
                           │ validates with          │ adapts through
                    ┌──────▼──────────┐       ┌──────▼──────────────────────┐
                    │  Domain (pure)  │       │  Infrastructure (server)    │
                    │  format, exam,  │       │  llm/gemini  prompts        │
                    │  passage, task, │       │  tts         audio (wav,    │
                    │  validation,    │       │  jobs/store  program)       │
                    │  audio program  │       │  config      rate_limiter   │
                    └─────────────────┘       └─────────────────────────────┘
                           ▲
                    ┌──────┴──────────┐
                    │  Export (pure)  │  markdown paper / key / transcript
                    └─────────────────┘
```

Dependency rule: `ui → application → {domain, infrastructure}`,
`infrastructure → domain`, `export → domain`. The domain and export layers
never import Dioxus, reqwest, sqlx or tokio, so they compile for wasm and
are unit-tested with plain `cargo test`.

## Module map

```
src/
  main.rs                 wiring only: routes; server build calls infrastructure::startup::run(App)
  domain/                 pure; compiles on wasm and server
    format.rs             ExamFormat, PartSpec, TaskSpec, TaskKind, WordLimit, PlayCount, PassageKind
                          + presets ExamFormat::ielts_listening(), ::hsg_national()
    speaker.rs            SpeakerConfig (label, gender, accent, role)
    passage.rs            Passage / Line, parser for "Speaker A: ..." text, duration estimate
    task.rs               Task, Item, Choice, Answer (letters | text | tfng)
    exam.rs               Exam aggregate: parts, answer key, completeness
    audio.rs              AudioTrack, AudioProgram (tones, pauses, replays derived from the format)
    validation.rs         invariants -> Vec<ValidationIssue>; grounding of keys in the passage; validate_exam (structural completeness)
    commands.rs           PassageRequest, TaskRequest, AudioRequest, ExamAudioRequest (self-validating)
    error.rs              DomainError (teacher-readable)
  application/            #[server] functions = use cases; DTOs shared with the browser
    topics.rs             suggest_topic
    passages.rs           generate_passage -> PassageDraft { passage, issues }
    tasks.rs              generate_task    -> TaskDraft { task, issues }
    audio.rs              start_part_audio, start_exam_audio, audio_job_status -> JobView { .., track: AudioTrack }, audio_url
  infrastructure/         #[cfg(feature = "server")] only; no #[server] here
    config.rs             StartupOptions (--portable, --config-dir, ...), AppConfig validated once from .env + env
    startup.rs            bootstrap -> SQLite -> router + GET /audio/{job_id}; dioxus::serve in debug
                          (hot reload), else an explicit listener, browser opening, graceful Ctrl+C
    llm/gemini.rs         GeminiClient: generate_text, generate_json<T>, synthesize; one retry policy
    prompts/              topic_prompt, passage_prompt, task_prompt (+ TaskDraftDto)
    tts/                  voices.json mapping; synthesize_passage (2-voice or turn-by-turn); Announcer
    audio/wav.rs          Pcm16: silence, tone, append, WAV encode/decode (no crate)
    audio/program.rs      render_program(AudioProgram, passages, announcer, assets)
    jobs/store.rs         SQLite job table (JobStore)
    jobs/worker.rs        spawn_part_audio, spawn_exam_audio, hourly clean-up
    jobs/serve.rs         serve_audio: plain axum handler streaming a finished WAV (audio/wav, Range)
    rate_limiter.rs       per-minute buckets
  export/markdown.rs      render_part_paper, render_key, render_transcript, render_exam
  ui/                     components (audio player, issue list, loading popup, speaker modal), views (home, exam, navbar)
    jobs.rs               wait_for_job: polls audio_job_status with a per-kind cadence and deadline
```

## Core model

* **ExamFormat** is data: an ordered list of **PartSpec**. A part says what
  kind of recording it is (`PassageKind`: conversation, interview, monologue,
  excerpt), how many times it is played (`PlayCount`), its length window,
  its default voices and its ordered **TaskSpec**s (`TaskKind` + item range).
  Item numbering is checked to be contiguous across the exam.
* **Passage** is the script of one part: `Line { speaker label, text }`.
  Labels are always "Speaker A/B/C"; names live inside the lines. This is
  what TTS receives and what grounding checks run against.
* **Task** is one question block: rubric, optional shared options, optional
  summary/notes text with `(n)______` gaps, and **Item**s each carrying an
  **Answer** and a verbatim **evidence** quote.
* **Exam** aggregates the format with one **ExamPart** per part (speakers,
  passage, tasks, audio track) and derives the **answer key**.
* **AudioProgram** is the plan of the full recording (music, tone,
  announcement, reading pause, passage, replay for `Twice`, checking time),
  derived from the format. Infrastructure renders it.

### Task kinds

| `TaskKind`                | Answer      | Shared options | Used by                     |
|---------------------------|-------------|----------------|-----------------------------|
| TrueFalseNotGiven         | tfng        | no             | HSG part 1                  |
| WhoMentioned { guests }   | one letter  | yes (S/A/B)    | HSG part 1                  |
| MultipleSelect { n of m } | one letter per item, distinct across the task | yes | HSG part 2 |
| MultipleChoice { options }| one letter  | per item       | HSG part 2, IELTS 2–3       |
| ShortAnswer(limit)        | text        | no             | HSG part 3                  |
| SummaryCompletion(limit)  | text, gaps in `summary` | no | HSG part 4                  |
| NoteCompletion(limit)     | text, gaps in `summary` | no | IELTS 1, 4                  |
| SentenceCompletion(limit) | text        | no             | IELTS                       |
| Matching { options }      | one letter  | yes            | IELTS 2–3                   |

Adding a task kind = one enum variant, one arm in `validation.rs`, one arm
in `prompts/items.rs`, one arm in `export/markdown.rs`. The compiler lists
every place.

## Generation pipeline

```
topic ──► passage_prompt ──► Gemini text ──► Passage::parse ──► validate_passage ──► PassageDraft
                                                                       │
          for each TaskSpec of the part:                               ▼
          task_prompt(passage) ──► Gemini JSON ──► TaskDraftDto ──► Task ──► validate_task(passage) ──► TaskDraft
                                                                       │
          AudioRequest ──► job ──► synthesize_passage ──► WAV on disk ◄─┘
          ExamAudioRequest ──► job ──► every part ──► render_program ──► one WAV
```

The browser orchestrates. For one part, `ui/views/home.rs` chains these use
cases: `generate_passage`, then, unless the script has Error-severity
issues, `start_part_audio` and the `generate_task` loop side by side
(`futures_util::future::join`), with `ui/jobs.rs` polling the job. Questions
and synthesis both read the same immutable `Passage`, so nothing on the
server is shared between them; the recording is the long pole and the
questions finish while it renders.

For the whole exam, `ui/views/exam.rs` runs every part's `generate_passage`
at once (`join_all`), then every part's `generate_task` loop at once beside
one `start_exam_audio` job. A part whose script failed or has Error-severity
issues keeps its own status and can be regenerated alone, and
`validate_exam` names what is still missing before `render_exam` is
downloaded. The `ExamState` lives in the `Navbar` layout's context and its
pipelines run on the root scope, so switching pages does not drop them.

Validation is the product's quality gate. Text keys must occur verbatim in
the passage (after normalisation), respect the word limit and the
number rule; letter keys must exist among the options; multiple selection
must have exactly the required distinct letters; numbering must match the
spec. Failures are returned as **issues** beside the draft, never hidden.
The teacher edits; nothing is "final" until they say so.

## Text-to-speech constraints

* Gemini multi-speaker synthesis accepts **two** voices per request. A
  two-voice passage is sent whole. Three voices (HSG part 1: host + two
  guests) are synthesised **turn by turn** and joined with short gaps.
* Output is 24 kHz mono 16-bit PCM; WAV is written without any audio
  crate. A 30-minute exam WAV is about 86 MB; MP3 encoding is a roadmap
  item (shell out to `ffmpeg` in the container).
* The TTS model name is configuration (`GEMINI_TTS_MODEL`) because the
  default, `gemini-2.5-pro-preview-tts`, is a preview model already on
  Google's deprecation list (successor `gemini-3.1-flash-tts-preview`, no
  shutdown date as of 2026-09-21). It accepts 8,192 input tokens per
  request; longer scripts are read turn by turn automatically.
* The text model is the `gemini-flash-latest` alias, which Google hot-swaps
  to the newest Flash release (`gemini-3.8-flash` at the time of writing,
  two weeks' notice for breaking changes). Left unpinned on purpose;
  `GEMINI_TEXT_MODEL` pins a versioned id when needed.
* Both calls go through `models/<id>:generateContent`, which Google now
  labels "Legacy" next to the newer `interactions` endpoint. It is still
  documented and served; moving to `interactions` is a change confined to
  `infrastructure/llm/gemini.rs`.

## Jobs

Synthesis outlives an HTTP request, so it runs as a job: a row in SQLite
(`DATA_DIR/jobs.db`) plus a WAV under `DATA_DIR/audio/`. The browser polls
`audio_job_status` (`ui/jobs.rs`: every 2 s for a part, 5 s for an exam,
with a deadline per kind that never cancels the job) and, once the job is
complete, receives an `AudioTrack` whose `location` is the job id. The WAV
itself is streamed by a plain axum route, `GET /audio/{job_id}`
(`infrastructure/jobs/serve.rs`, mounted beside the Dioxus router in
`infrastructure/startup.rs`), as `audio/wav` with `Range` support, so the player can
seek and the download link needs no blob. It is deliberately not a server
function: those redirect requests that accept `text/html`, which is exactly
what a download link sends. An exam job reads two parts at a time and
reports progress from 0.1 to 0.8 as parts finish. Jobs and files older than
24 h are purged hourly; the clean-up starts at boot. Concurrency is capped
per process.

## Deployment shape

`dx build --release` produces a server binary and a `public/` folder; the
Dockerfile packages both on `debian:bookworm-slim`. Configuration is
environment only (`.env.example`). The same server also ships as a portable
Windows EXE and Linux AppImage (`--portable`: `.env`, `voices.json` and
`data/` beside the package, browser opened after startup); see
`docs/portable.md`. Put a reverse proxy with TLS and **some
authentication** in front before exposing it: the app has rate limits but
no login, and every request spends Gemini credit.

## Roadmap (in order)

1. Persist exams (SQLite table keyed by `Exam::id`) so a teacher can return
   to a draft, and to a recording, after closing the tab.
2. DOCX export beside the Markdown one (`docx-rs`), matching the reference
   paper layout (answer boxes, số phách block).
3. MP3 output via `ffmpeg`.
4. Authentication (single shared password or Cloudflare Access), then
   per-user rate limits.
5. Regeneration of a single item with the validator's issues fed back into
   the prompt.
6. Per-part voice editing on the exam page (the part page already has it).
