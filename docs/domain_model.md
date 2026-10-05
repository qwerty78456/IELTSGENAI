# Domain Model

Types of the **Listening Assessment Generation** context, as implemented in
`src/domain/`. Every type is plain data with `serde` derives; rules live in
`validation.rs` and in the `validate()` methods of the commands.

## Format (the blueprint)

### `FormatId`
`IeltsListening` | `HsgNational`. `FormatId::format()` returns the preset.

### `ExamFormat`
- `id`, `name`
- `total_points` (IELTS 40, HSG 5.0), `listening_minutes`, `check_minutes`
- `parts: Vec<PartSpec>`
- `check_consistency()`: numbering contiguous from 1, every part has tasks
  and the right number of default voices.

### `PartSpec`
- `number`, `title`
- `passage: PassageKind` — `Conversation { speakers }`, `Interview { guests }`,
  `Monologue`, `Excerpt`
- `brief` — genre guidance used in prompts
- `playback: PlayCount` — `Once` | `Twice`
- `min_minutes`, `max_minutes`
- `default_speakers: Vec<SpeakerConfig>`
- `tasks: Vec<TaskSpec>`

### `TaskSpec`
`kind: TaskKind`, `first`, `last` (inclusive item numbers).

### `TaskKind`
`TrueFalseNotGiven`, `WhoMentioned { guests }`, `MultipleSelect { choose, options }`,
`MultipleChoice { options }`, `ShortAnswer(WordLimit)`, `SummaryCompletion(WordLimit)`,
`NoteCompletion(WordLimit)`, `SentenceCompletion(WordLimit)`, `Matching { options }`.

### `WordLimit`
`max_words`, `allow_number`; renders as "NO MORE THAN TWO WORDS AND/OR A NUMBER".

## Voices

### `SpeakerConfig`
- `label` — "Speaker A"; the only thing that appears as a turn marker
- `gender: Gender` — Male | Female
- `accent: Accent` — British | American | Australian | Canadian | NewZealand |
  Irish | Scottish | SouthAfrican | Indian (`ALL` in UI order). Saved exams name
  the variants, so one is never removed. `language_code()` ("en-GB"; Scottish
  shares en-GB) and `from_language_code()` (the first match in `ALL`).
- `role: SpeakerRole` — Student, Professor, Clerk, Receptionist, Guide, Host,
  Expert, Guest, Reporter, Narrator, Other(String). `delivery_style()` gives a
  few constant words for the speech style ("polite and helpful"), never age,
  gender, accent or a name.
- `voice: VoiceChoice` — `Auto` (default; speakers saved before 0.8 load so),
  `Assigned(Voice)` (picked by the app, replaceable) or `Chosen(Voice)` (picked
  by the teacher, kept). `with_voice(voice)` chooses a voice and adopts its
  gender and accent.

`describe()` ("Speaker A: Female, British English, Host") feeds prompts and
never names the voice; `describe_with_voice()` adds "; voice Oliver" and is for
transcripts and the answer key only, never the student paper.

### `Voice` (`voice.rs`)
`id` (what the speech request names, compared alone for identity), `name`,
`gender`, `accent`, `source: VoiceSource` (`Library` | `Designed`),
`description`. `Voice::check_id` allows 1–100 ASCII letters, digits, `-`, `_`;
`Voice::is_designed_id` is the one place the Voice Design prefixes
(`voice_`, `voicekey_`) are known.

Pure rules over a catalogue slice, run in the browser and again on the server:
- `assign_voices(speakers, catalogue, elsewhere) -> AssignedVoices { speakers, unvoiced }`:
  keeps `Chosen`; keeps `Assigned` while the catalogue lists it, it fits and no
  one else holds it; gives every other speaker the first fitting free voice in
  catalogue order, preferring ids not in `elsewhere` (the other parts'). Never
  gives two speakers one voice, deterministic, idempotent; `unvoiced` lists the
  labels left on `Auto`.
- `assign_exam_voices(parts, catalogue)` — `assign_voices` part by part, in
  order, each part's `elsewhere` being the other parts' voices; the browser and
  `start_exam_audio` both assign an exam this way.
- `next_voice(speakers, index, catalogue)` — "Another voice": the next fitting
  voice after the current one that no other speaker uses, wrapping.
- `voice_conflicts(speakers)` — a shared voice, or a voice of the other gender.
- `speaker_change(before, after) -> SpeakerChange { script, recording }` — what
  an edit makes out of date (labels, gender, accent, role: both; voice only:
  the recording). Nothing when `before` is empty.

## Content

### `Passage`
`part`, `topic`, `lines: Vec<Line { speaker, text }>`.
`Passage::parse` reads "Speaker A: ..." text; `script_text()` is the canonical
labelled form (question prompts, the teacher's view); `plain_text()` is used for
grounding; `estimated_minutes()` assumes 150 words per minute. Text-to-speech
receives the lines without labels, the speaker travelling beside each line.

### `Task`
`spec`, `instruction`, `shared_options: Vec<Choice>`, `summary: Option<String>`
(paragraph or notes with `(n)______` gaps), `items: Vec<Item>`.

### `Item`
`number`, `stem`, `options: Vec<Choice>` (multiple choice only), `answer: Answer`,
`evidence` (verbatim quote from the passage). A missing or `null` `options`,
`evidence` or `shared_options` reads as empty (`task::null_as_default`).

### `Answer`
Tagged JSON `{ "kind": ..., "value": ... }`:
- `letters` → `["B"]` or `["B", "D"]`
- `text` → accepted spellings, first is canonical
- `tfng` → `"T"`, `"F"`, `"NG"`

## Aggregate

### `Exam`
`id: Uuid`, `format`, `title`, `theme`, `parts: Vec<ExamPart>`.
`answer_key()` flattens every item in number order; `is_complete()` when every
part has a passage and every `TaskSpec` has a `Task`.

### `ExamPart`
`spec`, `speakers`, `passage: Option<Passage>`, `tasks: Vec<Task>`,
`audio: Option<AudioTrack>`.

## Audio

### `AudioTrack`
`container`, `sample_rate`, `duration_ms`, `location` (job id or path). Never bytes.
Built by `audio_job_status` once a job completes; `location` is the job id and
`application::audio::audio_url(location)` is where the browser streams it.
A saved exam (`application::exams::SavedExam`) names the job it refers to;
that job and its WAV are kept until the exam is deleted. `ExamPart.audio` is
reserved for per-part recordings and is not written yet.

### `AudioProgram` / `AudioSegment`
`AudioProgram::for_format(&ExamFormat)` yields the ordered segments:
`Music`, `Tone`, `Silence { ms }`, `Announcement(String)`, `Passage { part }`,
with a replay for `Twice` parts and the checking time at the end.

## Usage

### `Usage`
What Gemini billed for one or more requests: `requests`, `reused` (speech
answered from the cache, free), `input_tokens` (cached included),
`cached_tokens`, `output_tokens` (text or audio), `thinking_tokens`,
`micro_usd` (price when the requests were made, in millionths of a dollar) and
`unpriced` (requests whose model has no known price). `add`, `usd()`,
`cost_text()` ("$0.412", "+?" when something is unpriced).

### `UsageStep`
`Topic` | `Script` | `Questions` | `Recording` | `Voices` (samples and designed
voices), stored by `key()`; `label()` names it in the spend breakdown.

### `ExamUsage`
One `Usage` per step (`voices` defaults to empty for older data) plus
`budget_micro_usd` (0 = none); `step()`, `step_mut()`, `total()`,
`over_budget()`, `budget_text()`. Built by `application::usage::exam_usage`
from the ledger; it includes failed and superseded runs.

## Commands

| Command            | Validates                                              |
|--------------------|--------------------------------------------------------|
| `PassageRequest`   | topic length/content, part exists, speaker count/labels (first Error; warnings never block) |
| `TaskRequest`      | part and task index exist, passage not empty            |
| `AudioRequest`     | passage not empty, every used label configured, every speaker has a voice (none on `Auto`) with a valid id, no shared voice, no voice of the other gender |
| `ExamAudioRequest` | one valid `AudioRequest` per part of the format (errors name the part) |

Voices are assigned before an `AudioRequest` is validated: the browser and the
server both run `assign_voices` first.

## Validation

`validate_speakers`, `validate_passage`, `validate_task` return
`Vec<ValidationIssue { severity, item, message }>`. `Severity::Error` means
the key is unusable; `Warning` means look at it. `first_error` picks the
issue that blocks. `validate_speakers` adds a Warning per `voice_conflicts`
entry: the script does not depend on the voice, the recording does. The
grounding rule: text keys and evidence must occur in the normalised passage
text.

`validate_exam(&Exam)` checks structural completeness before export or the
full recording: every part has a passage, every `TaskSpec` has a task and,
once nothing is missing, item numbers run from 1 to the format's total in
order. Content issues stay with the drafts that produced them.

## Failures

`DomainError::{InvalidRequest, InvalidPassage, InvalidTask}(String)` —
teacher-readable, no HTTP codes, no stack traces. Infrastructure errors
(`LlmError`, `TtsError`, `AudioError`) are converted to messages at the
application boundary.
