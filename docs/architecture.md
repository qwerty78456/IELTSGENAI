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
                    │  Export (pure)  │  markdown + docx: paper / key / transcript
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
    passage.rs            Passage / Line, parser for "Speaker A: ..." text, script/transcript views, duration estimate
    speech.rs             speech markup: SPEECH_TAGS, EXAM_SPEECH_TAGS, display/speech text, markup problems
    task.rs               Task, Item, Choice, Answer (letters | text | tfng); null lists read as empty
    usage.rs              Usage (tokens + µUSD), UsageStep, ExamUsage (per step, budget)
    exam.rs               Exam aggregate: parts, answer key, completeness
    audio.rs              AudioTrack, AudioProgram (tones, pauses, replays derived from the format)
    validation.rs         invariants -> Vec<ValidationIssue>; grounding of keys in the passage; validate_exam (structural completeness)
    commands.rs           PassageRequest, TaskRequest, AudioRequest, ExamAudioRequest (self-validating)
    error.rs              DomainError (teacher-readable)
  application/            #[server] functions = use cases; DTOs shared with the browser
    topics.rs             suggest_topic                       (every Gemini use case takes `exam: Option<Uuid>`
    passages.rs           generate_passage -> PassageDraft     for the usage ledger; the part page passes None)
    tasks.rs              generate_task    -> TaskDraft { task, issues }
    audio.rs              start_part_audio, start_exam_audio, audio_job_status -> JobView { .., track: AudioTrack }, audio_url
    usage.rs              exam_usage -> ExamUsage, usage_totals -> UsageTotals; record() after every Gemini call
    settings.rs           api_key_status -> KeyStatus, set_api_key (loopback only, never over a working operator key)
    exams.rs              save_exam, list_exams, load_exam, delete_exam; SavedExam (exam + topics + recording job), ExamSummary
  infrastructure/         #[cfg(feature = "server")] only; no #[server] here
    config.rs             StartupOptions (--portable, --config-dir, ...), AppConfig validated once from
                          .env < Windows registry environment < process environment; KeyOrigin
    secrets.rs            the API key requests use: typed in the browser (memory only), else configured;
                          remembers the key Google last rejected (KeyStatus.rejected)
    usage.rs              UsageStore: `usage` ledger table in jobs.db (one row per step run, any outcome)
    exams.rs              ExamStore: `exams` table (SavedExam JSON body + summary columns) in jobs.db; pins recording jobs
    startup.rs            bootstrap -> SQLite -> router + GET /audio/{job_id}; dioxus::serve in debug
                          (hot reload), else an explicit listener, browser opening, graceful Ctrl+C
    llm/gemini.rs         GeminiClient on /v1beta/interactions (store: false): generate_text, generate_json<T>,
                          synthesize(SpeechRequest); one retry policy; usage meter shared by clones
    llm/pricing.rs        price table per model (intro until 2026-12-31, list after), cost_micro_usd
    prompts/              topic_prompt, passage_prompt, task_prompt (+ TaskDraftDto)
    tts/                  voices.json mapping; synthesize_passage (chunks of <= 200 words, <= 2 voices);
                          cache.rs (speech reuse under DATA_DIR/audio/cache); Announcer
    audio/wav.rs          Pcm16: silence, tone, append, WAV encode/decode (no crate)
    audio/program.rs      render_program(AudioProgram, passages, announcer, assets)
    jobs/store.rs         SQLite job table (JobStore); output_path is a file name resolved under DATA_DIR/audio
    jobs/worker.rs        spawn_part_audio, spawn_exam_audio (usage recorded on success and failure),
                          hourly clean-up (AUDIO_RETENTION_HOURS, SPEECH_CACHE_HOURS)
    jobs/serve.rs         serve_audio: plain axum handler streaming a finished WAV (audio/wav, Range)
    rate_limiter.rs       per-minute buckets
  export/markdown.rs      render_part_paper, render_key, render_transcript, render_exam
  export/docx.rs          render_exam_docx, render_part_docx (docx-rs; answer boxes, candidate block, key and transcripts on their own pages)
  ui/                     components (audio player, exam library, issue list, key setup, loading popup, speaker modal), views (home, exam, navbar)
    jobs.rs               wait_for_job: polls audio_job_status with a per-kind cadence and deadline
    clock.rs              local-time formatting (js-sys Date in the browser, UTC fallback on the server)
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
in `prompts/items.rs`, one arm in `export/markdown.rs` and one in
`export/docx.rs` (`answer_layout`, an exhaustive match the compiler checks).

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

* Every Gemini call goes to the **Interactions API**
  (`POST /v1beta/interactions`, GA since June 2026; `generateContent` is
  "Legacy") with `"store": false`: the app keeps no conversation on Google's
  side, and Google would otherwise keep each interaction for 55 days. Output
  is read from the last `model_output` step; `thought` steps are skipped; an
  `incomplete` status (max tokens) or a content-block code (`safety`, ...)
  becomes a readable error, after the response has been metered.
* Models are pinned: `gemini-3.8-flash` (GA) for text and
  `gemini-3.8-flash-tts` (stable) for speech, both configuration. Text
  requests send `thinking_level` (`GEMINI_THINKING_LEVEL`, default `low`;
  3.8 Flash cannot turn thinking off and rejects `minimal`) and
  `max_output_tokens: 8192` as a guard against runaway answers.
* 3.8 TTS reads its input **word for word**. Directions go in each text
  item's `speech_metadata.style`; with two voices each item also names its
  `speech_metadata.speaker`, the passage label without spaces (`SpeakerA`),
  matched to `speech_config {mode: conversational, speakers}`. Nothing but the
  spoken words is ever in the text, apart from what `speech_text` keeps: the
  few speech tags of an expressive script (`<sigh>`, `<cough>`, `<laugh>`,
  `<chuckle>`, measured in `docs/voices.md`) and backchannels in a two-voice
  request.
* At most **two** voices and 8,192 input tokens per request, and a normal
  request stays open about a minute. A passage is therefore cut into chunks
  of consecutive turns, at most 200 words (about 80 s of audio, read in
  about 30 s) and at most two speakers; a long monologue line is split at
  sentence ends. Chunks are joined with 350 ms gaps. The three-voice HSG
  interview is simply more chunks.
* Raw 24 kHz mono 16-bit PCM is requested (`audio/l16`); a WAV reply is
  parsed too. WAV is written without any audio crate. A 30-minute exam WAV
  is about 80 MB; MP3 encoding is a roadmap item.
* A chunk (or announcement) already synthesised for the same model, voices,
  words and style is reused from `DATA_DIR/audio/cache` at no cost
  (`tts/cache.rs`, SHA-256 key, `SPEECH_CACHE_HOURS`, default 72, 0 = off).
  Re-rendering after editing one part pays for that part only.

## Usage and cost

Every billed response is added to the client's usage meter before it is
parsed, so a reply that is cut off or fails to parse still counts. The
server function (or recording job) then writes one row to the `usage` table
of `jobs.db`: step, exam id (none from the part page), model, requests,
reused chunks, input / cached / output / thinking tokens and the price in
µUSD at the rate in force (`llm/pricing.rs`: 3.8 introductory prices until
2026-12-31, list prices after; unknown models are counted as unpriced).
Rows are written whatever the outcome and outlive their exam. The exam page
shows the exam's spend per step beside `EXAM_BUDGET_USD` (default $0.70,
warning only); the saved-exams panel shows the last 24 hours and 30 days.

Measured 2026-09-28 for one full IELTS exam at thinking `low`: $0.308 now,
$0.616 at 2027 list prices (text $0.039, recording $0.269 for 29,728 audio
tokens at 32 tokens per second). At `medium` the same exam cost $0.48. The
recording is almost 90 % of the cost; prompt caching cannot help because no
prompt reaches the 4,096-token minimum.

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
reports progress from 0.1 to 0.8 as parts finish. Concurrency is capped
per process.

A job row stores only the WAV's file name; every reader resolves it under
the current `DATA_DIR/audio` (`JobRecord::output_file`, which also accepts
the absolute paths rows written before 0.6.0 hold), so a portable folder
that moves keeps its recordings. Jobs that no saved exam refers to are
purged hourly from boot once older than `AUDIO_RETENTION_HOURS` (default
24; `0` never purges and the task is not even spawned). A job named by a
saved exam's `recording_job` is skipped by the purge and deleted, with its
WAV, when the exam is deleted and no other exam refers to it.

## Saved exams

The whole-exam page keeps its draft on the server so the teacher can close
the tab and come back. `application/exams.rs` defines `SavedExam` (the
`Exam`, the topic typed for each part, the recording job id and the stale
flag for a script regenerated since the recording; speaker changes are not
flagged but derived from each part's `recorded_for` and each script's
`written_for`) and four server functions: `save_exam`, `list_exams`, `load_exam`,
`delete_exam`. `infrastructure/exams.rs` stores one row per `Exam::id` in
the `exams` table of `jobs.db`: the `SavedExam` as JSON plus the columns the
list needs (title, format key, part counts, `recording_job`, timestamps),
so listing never parses JSON. `load_exam` looks the recording up afresh in
the job table, since a job may finish after the tab closed.

The browser (`ui/views/exam.rs`) saves on its own once a script exists (or
after the first explicit Save): after every finished step, a second after
an edit to the title, theme or a topic, and on the Save button. Saves are
single-flight with one queued follow-up, and the snapshot is taken when the
save is requested, so what is stored is what the teacher saw. Opening a
saved exam replaces the state the way switching formats does (new `run`),
recomputes issues with the pure validators instead of storing them, and
resumes polling a recording job that is still running. Deleting the open
exam starts a fresh one, or the next auto-save would bring the row back.
Two tabs editing the same exam: last write wins.

## Deployment shape

`dx build --release` produces a server binary and a `public/` folder; the
Dockerfile packages both on `debian:bookworm-slim`. Configuration is
environment only (`.env.example`); `AUDIO_RETENTION_HOURS` decides how long
recordings no saved exam refers to are kept. The same server also ships as
a portable Windows EXE and Linux AppImage (`--portable`: `.env`,
`voices.json` and `data/` beside the package, browser opened after startup;
saved exams and their recordings travel with the folder); see
`docs/portable.md`. Put a reverse proxy with TLS and **some
authentication** in front before exposing it: the app has rate limits but
no login, and every request spends Gemini credit. A server bound to anything
but a loopback address never accepts an API key from the browser.

## Roadmap (in order)

1. MP3 output via `ffmpeg`.
2. Authentication (single shared password or Cloudflare Access), then
   per-user rate limits; saved exams are visible to everyone who reaches the
   server until then.
3. Regeneration of a single item with the validator's issues fed back into
   the prompt.
4. Per-part voice editing on the exam page (the part page already has it).
5. Saving on the part page too (its state is not an `Exam`; it would need a
   record of its own).
