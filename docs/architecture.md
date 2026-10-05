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
                                    │ #[server] calls (HTTP, serde); GET /audio/{job_id} and /voice-sample/{voice_id} stream WAVs
                    ┌───────────────▼───────────────────────────┐
                    │  Application (use cases)                  │
                    │  application/{topics,passages,tasks,audio}│
                    │  + voices, exams, usage, settings         │
                    └──────┬─────────────────────────┬──────────┘
                           │ validates with          │ adapts through
                    ┌──────▼──────────┐       ┌──────▼──────────────────────┐
                    │  Domain (pure)  │       │  Infrastructure (server)    │
                    │  format, exam,  │       │  llm/gemini  prompts        │
                    │  passage, task, │       │  tts         audio (wav,    │
                    │  speaker, voice,│       │  (voices)    program)       │
                    │  speech, usage, │       │  jobs        exams, usage   │
                    │  validation,    │       │  config      secrets        │
                    │  audio program  │       │  startup     rate_limiter   │
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
    speaker.rs            SpeakerConfig (label, gender, accent, role, voice: VoiceChoice), Accent (nine, language_code),
                          SpeakerRole::delivery_style; describe() never names the voice, describe_with_voice() for keys
    voice.rs              Voice, VoiceSource (Library | Designed), VoiceChoice; assign_voices, assign_exam_voices,
                          next_voice, voice_conflicts, speaker_change; VoiceDesignRequest (+ validate)
    passage.rs            Passage / Line, parser for "Speaker A: ..." text, script/transcript views, duration estimate;
                          written_for + speakers_changed (stale scripts)
    speech.rs             speech markup: SPEECH_TAGS, EXAM_SPEECH_TAGS, display/speech text, markup problems
    task.rs               Task, Item, Choice, Answer (letters | text | tfng); null lists read as empty
    usage.rs              Usage (tokens + µUSD), UsageStep (+ Voices), ExamUsage (per step, budget)
    exam.rs               Exam aggregate: parts, answer key, completeness; ExamPart.recorded_for (stale recordings)
    audio.rs              AudioTrack, AudioProgram (tones, pauses, replays derived from the format)
    validation.rs         invariants -> Vec<ValidationIssue>; grounding of keys in the passage; validate_exam (structural completeness)
    commands.rs           PassageRequest (+ expressive), TaskRequest, AudioRequest (+ fresh), ExamAudioRequest
                          (self-validating; recordings refuse Auto, shared or wrong-gender voices)
    error.rs              DomainError (teacher-readable)
  application/            #[server] functions = use cases; DTOs shared with the browser
    topics.rs             suggest_topic                       (every Gemini use case takes `exam: Option<Uuid>`
    passages.rs           generate_passage -> PassageDraft     for the usage ledger; the part page passes None)
    tasks.rs              generate_task    -> TaskDraft { task, issues }
    audio.rs              start_part_audio, start_exam_audio, audio_job_status -> JobView { .., track: AudioTrack }, audio_url
    usage.rs              exam_usage -> ExamUsage, usage_totals -> UsageTotals; record() after every Gemini call
    settings.rs           api_key_status -> KeyStatus, set_api_key (loopback only, never over a working operator key);
                          local_server(), the loopback rule voices reuse
    voices.rs             voice_catalogue, voice_preview (catalogue or this key's designed voices) -> VoiceSample;
                          design_voice -> DesignedVoice, designed_voices -> DesignedVoices, delete_voice
                          (design/delete: loopback only, app-made voices only); prepare_speakers (server, async)
    exams.rs              save_exam, list_exams, load_exam, delete_exam; SavedExam (exam + topics + recording job), ExamSummary
  infrastructure/         #[cfg(feature = "server")] only; no #[server] here
    config.rs             StartupOptions (--portable, --config-dir, ...), AppConfig validated once from
                          .env < Windows registry environment < process environment; KeyOrigin
    secrets.rs            the API key requests use: typed in the browser (memory only), else configured;
                          remembers the key Google last rejected (KeyStatus.rejected)
    usage.rs              UsageStore: `usage` ledger table in jobs.db (one row per step run, any outcome)
    exams.rs              ExamStore: `exams` table (SavedExam JSON body + summary columns) in jobs.db; pins recording jobs
    startup.rs            bootstrap (prints config notices) -> SQLite -> router + GET /audio/{job_id} and
                          GET /voice-sample/{voice_id}; dioxus::serve in debug
                          (hot reload), else an explicit listener, browser opening, graceful Ctrl+C
    llm/gemini.rs         GeminiClient on /v1beta/interactions (store: false): generate_text, generate_json<T>,
                          synthesize(SpeechRequest); Voices API: list_voices, get_voice, delete_voice,
                          create_voice (POST /v1beta/voices, store: true, never retried after a timeout);
                          one retry policy; usage meter shared by clones
    llm/pricing.rs        price table per model (intro until 2026-12-31, list after), cost_micro_usd
    prompts/              topic_prompt, passage_prompt (names and wording that fit each speaker; the speech-tag
                          DELIVERY block when expressive), task_prompt over the transcript (+ TaskDraftDto)
    tts/                  voices.rs (VoiceCatalog: default_voices.json pools, compiled in, + voices.json v2 overrides;
                          a 0.7 file renamed or ignored); synthesize.rs (plan_passage: one voice table, chunks of
                          <= 200 words and <= 2 voices, designed voices alone; speaker_style, synthesize_passage,
                          synthesize_announcement);
                          cache.rs (speech reuse under DATA_DIR/audio/cache); samples.rs (voice samples under
                          DATA_DIR/audio/voices); designed.rs (design_voice, designed_voices cached 60 s,
                          find_voice, delete_designed_voice; `designed_voices` table of the voices made here); Announcer
    audio/wav.rs          Pcm16: silence, tone, append, WAV encode/decode (no crate)
    audio/program.rs      render_program(AudioProgram, passages, announcer, assets)
    jobs/store.rs         SQLite job table (JobStore); output_path is a file name resolved under DATA_DIR/audio
    jobs/worker.rs        spawn_part_audio, spawn_exam_audio (usage recorded on success and failure),
                          hourly clean-up (AUDIO_RETENTION_HOURS, SPEECH_CACHE_HOURS, voice samples after 30 days)
    jobs/serve.rs         serve_audio, serve_voice_sample: plain axum handlers streaming a WAV (audio/wav, Range)
    rate_limiter.rs       per-minute buckets (Bucket::ALL; VoiceSample 30, VoiceDesign 5)
  export/markdown.rs      render_part_paper, render_key, render_transcript, render_exam
  export/docx.rs          render_exam_docx, render_part_docx (docx-rs; answer boxes, candidate block, key and transcripts on their own pages)
  ui/                     components (audio player, exam library, issue list, key setup, loading popup, speaker modal:
                          SpeakerCards + SpeakerEditModal, voices: VoiceCatalogueCtx, VoicePicker and the dialog's
                          DesignedVoicesPanel), views (home, exam, navbar: loads the voice catalogue once)
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
  what TTS receives and what grounding checks run against. A line may carry
  speech tags (`<sigh>`), which only TTS and the teacher's screen see. The
  passage remembers the speakers it was `written_for`.
* **Task** is one question block: rubric, optional shared options, optional
  summary/notes text with `(n)______` gaps, and **Item**s each carrying an
  **Answer** and a verbatim **evidence** quote.
* **Speaker** (`SpeakerConfig`) is a label, gender, accent and role plus a
  **Voice** of its own (`VoiceChoice`); two speakers of a part never share
  one.
* **Exam** aggregates the format with one **ExamPart** per part (speakers,
  passage, tasks, audio track, the speakers it was `recorded_for`) and
  derives the **answer key**. A script or recording made for other speakers
  than the current ones is stale; that is worked out, never stored.
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
* **The accent belongs to the voice.** Google's 30 classic voices (`Zephyr`,
  `Puck`, `despina`, ...) are all General American; up to 0.7 gender and
  accent picked one of them, so the accent never reached the model and every
  "British" speaker sounded American (`docs/voices.md`). Speakers are read by
  regional voices of Google's Extended Voice Library (`en-gb-advisor-1`,
  `en-au-tutor-3`, `en-ie-concierge-5`, `en-in-tutor-2`, ...); a classic voice
  may only sit in the American pool. Accent, gender, age or a name never go in
  a style.
* **Voice pools.** `tts/default_voices.json` (compiled in, chosen with
  `tools/voice_lab.py audition`) lists, for each of the nine accents and
  each gender, several library voices in preference order (the most
  different ones first) plus the announcer (`en-gb-tutor-9`). Minimums keep a
  part's speakers apart: 4 British voices per gender (HSG Part 1 has three
  women), 3 for American, Australian, Canadian and New Zealand English, 2 for
  Irish, Scottish (`en-GB` voices from Glasgow), South African and Indian
  English. Some voices are kept only on the catalogue's word because the AI
  ear cannot tell those accents apart (all Canadian and South African, some
  Australian, New Zealand and Irish ones); they stay until the PO has
  listened. `voices.json` version 2 (`VOICES_PATH`) holds overrides only,
  `{version: 2, announcer?, pools: {accent: {female, male: [{id, name,
  description}]}}}`: a non-empty list replaces one pool and everything else
  follows the release. A pool below its minimum is a startup notice. A 0.7
  file (no `version`) is parsed strictly, so a broken one still stops
  startup, and then ignored: left at the 0.7 defaults it is renamed
  `voices.0.7.json` and the version 2 template is written; customised, it is
  kept and a notice says it is in the "0.7 format".
* **A voice of its own for every speaker.** `SpeakerConfig.voice` is a
  `VoiceChoice`: `Auto`, `Assigned` by the app or `Chosen` by the teacher.
  `domain::assign_voices` keeps chosen voices, keeps assigned ones that still
  fit, and gives every other speaker the first free voice of its accent and
  gender, preferring voices no other part uses (`assign_exam_voices`, parts in
  order); it is deterministic and never gives two speakers of a part one
  voice. The browser assigns as soon as the catalogue arrives, the server
  assigns again at job start (`prepare_speakers`), and `AudioRequest::validate`
  refuses `Auto`, shared or wrong-gender voices. The reason is measured: with
  two speakers on one voice Gemini improvised a man's voice for the second
  woman (3 of 5 takes; 0 of 5 with two distinct voices, gate G2).
* 3.8 TTS reads its input **word for word**. Directions go in each text
  item's `speech_metadata.style`; with two voices each item also names its
  `speech_metadata.speaker`, the passage label without spaces (`SpeakerA`),
  matched to `speech_config {mode: conversational, speakers}` with the voices
  listed in label order.
* **One style per speaker, none per line.** Every turn of a speaker carries
  the same short style, its role's `delivery_style()` ("polite and helpful")
  plus `EXAM_PACE` ("clear, at a steady exam pace"). A style that changes from
  line to line also moves the voice (F0 -18 % on a "calm" line, gate G5), so
  per-line emotion comes from wording, punctuation and speech tags only.
* **Speech tags.** Nothing but the spoken words is in the text, apart from
  what `domain::speech::speech_text` keeps: in an expressive script
  (`PassageRequest.expressive`, on by default) the `EXAM_SPEECH_TAGS`
  `<sigh>`, `<cough>`, `<laugh>` and `<chuckle>`, placed mid-sentence
  (performed and never read aloud in the probes, gate G3; `<long pause>` and
  `<whispers>` were read aloud), and `|backchannels|` in a two-voice request
  on Flash TTS (not lite). Unknown tags, `[notes]` and stray pipes are
  dropped because the model would read them. Tags are atomic and never count
  as words; transcripts, question prompts, Markdown, DOCX and grounding never
  see them (`display_text`, `transcript_text`).
* At most **two** voices and 8,192 input tokens per request, and a normal
  request stays open about a minute. `plan_passage` makes one voice table for
  the whole passage and cuts it into chunks of consecutive turns, at most 200
  words (about 80 s of audio, read in about 30 s) and at most two speakers; a
  long monologue line is split at sentence ends, and only turns of the same
  speaker and style are merged. Chunks are joined with 250 ms of silence when
  another speaker starts and 350 ms when one speaker goes on. The three-voice
  HSG interview is simply more chunks. Every voice in a request also bills
  its reference audio as input (library voices 740-1,970 tokens).
* **Designed voices read alone.** A voice made with Voice Design (`voice_...`)
  reads each of its turns in a request of its own (`reads_alone`), as Google
  documents two-voice requests for prebuilt voices only; the probe found them
  accepted anyway (gate G4), and one constant changes that. A designed voice
  belongs to the Google project of the API key (at most 200, kept a year after
  its last use), so `prepare_speakers` refuses one the project no longer has
  with a readable reason.
* At most 3 speech requests run at once in the whole process (a semaphore in
  `GeminiClient`), each passage keeps 2 chunks in flight, and a 429's
  `retryDelay` (at most 60 s) replaces the exponential backoff.
* Raw 24 kHz mono 16-bit PCM is requested (`audio/l16`); a WAV reply is
  parsed too. WAV is written without any audio crate. A 30-minute exam WAV
  is about 80 MB; MP3 encoding is a roadmap item.
* **Speech cache v2.** A chunk (or announcement) already synthesised for the
  same model, voices (label and id) and turns (speaker, style, text) is
  reused from `DATA_DIR/audio/cache` at no cost (`tts/cache.rs`, SHA-256 key
  starting with `speech-cache-v2`, so no 0.7 entry matches; `SPEECH_CACHE_HOURS`,
  default 72, 0 = off). Re-rendering after editing one part pays for that part
  only. Gemini ignores `generation_config.seed` (gate G7), so a "New take"
  (`AudioRequest::fresh`) reads that part with `Reuse::Refresh` and
  overwrites its entries; announcements always reuse.

### Voices API, samples and designed voices

* `GeminiClient` sends the Voices API through the same `send` path, retry
  policy and key header as everything else. `GET /v1beta/voices` lists the
  catalogue (free; `voice_live_probe` checks that every pooled voice still
  exists with its gender and language) and, with `type=prompted`, the key's
  designed voices; `GET` and `DELETE /v1beta/voices/{id}` read (with Google's
  free sample) and delete one; `POST /v1beta/voices` with `"store": true`,
  `type: prompted` and no `voice.model` creates one. That is the only stored
  request the app makes, it is never retried after a timeout (one click makes
  at most one voice), and it is metered at TTS rates as an estimate because
  Google lists no price. Ids pass `Voice::check_id` before any path is built;
  a voice Google does not know is `LlmError::UnknownVoice`, never a rejected
  key, and a full project is `LlmError::VoiceLimit`.
* **Listen** (`voice_preview`) accepts only catalogue ids and the designed
  voices of the key's project. The first listen of a library voice
  synthesises one fixed sentence (about $0.005, booked under
  `UsageStep::Voices`, `Bucket::VoiceSample` 30 a minute); the WAV is kept as
  `DATA_DIR/audio/voices/{id}.wav`, streamed from the plain route
  `/voice-sample/{voice_id}` and purged after 30 days unused, so every later
  listen is free. A designed voice's sample is Google's own and free.
* **Voice Design** (`design_voice`: name, a 20-500 character description,
  gender, accent) takes about 20 s and about $0.01. The `designed_voices` table
  of `jobs.db` records the voices this app made, with their exact accent
  (Google keeps only a language tag); only those can be deleted. Creating and
  deleting work only when the server listens on a loopback address (the rule
  of the browser key, `settings::local_server`) and are limited by
  `Bucket::VoiceDesign` (5 a minute). Voices made elsewhere in the project are
  listed and usable, never deleted. The project's list is cached 60 s.

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

Measured 2026-10-05 with 0.8.0 (regional voices, expressive scripts, one
part on Irish and Scottish voices): $0.364 now, **$0.728 at 2027 list
prices, over the $0.70 bound** (text $0.042, recording $0.322 for 19
requests and a 30:18 recording, one of them retried). Part of the rise is
the reference audio every library voice bills as input. Voice samples and
designed voices are booked under their own step, `voices`, and show in the
exam's spend line.

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
4. Saving on the part page too (its state is not an `Exam`; it would need a
   record of its own).
5. The PO's blind listening review of the voice pools kept on the
   catalogue's word (all Canadian and South African voices, one Australian
   male, two New Zealand female, one Irish voice per gender; the audition
   sheet is under `TESTING_DUMP/voice-lab/`, not in git), then
   `default_voices.json` updated from it (`docs/voices.md`).
6. Voice Replication (a voice made from a recording of a real person): left
   out of 0.8 by decision; Voice Design covers voices described in words.
