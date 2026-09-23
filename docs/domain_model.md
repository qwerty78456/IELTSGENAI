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
- `accent: Accent` — British | American | Australian | Canadian | NewZealand
- `role: SpeakerRole` — Student, Professor, Clerk, Receptionist, Guide, Host,
  Expert, Guest, Reporter, Narrator, Other(String)

## Content

### `Passage`
`part`, `topic`, `lines: Vec<Line { speaker, text }>`.
`Passage::parse` reads "Speaker A: ..." text; `script_text()` is the canonical
form sent to TTS; `plain_text()` is used for grounding; `estimated_minutes()`
assumes 150 words per minute.

### `Task`
`spec`, `instruction`, `shared_options: Vec<Choice>`, `summary: Option<String>`
(paragraph or notes with `(n)______` gaps), `items: Vec<Item>`.

### `Item`
`number`, `stem`, `options: Vec<Choice>` (multiple choice only), `answer: Answer`,
`evidence` (verbatim quote from the passage).

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

### `AudioProgram` / `AudioSegment`
`AudioProgram::for_format(&ExamFormat)` yields the ordered segments:
`Music`, `Tone`, `Silence { ms }`, `Announcement(String)`, `Passage { part }`,
with a replay for `Twice` parts and the checking time at the end.

## Commands

| Command            | Validates                                              |
|--------------------|--------------------------------------------------------|
| `PassageRequest`   | topic length/content, part exists, speaker count/labels |
| `TaskRequest`      | part and task index exist, passage not empty            |
| `AudioRequest`     | passage not empty, every used label has a voice         |
| `ExamAudioRequest` | one valid `AudioRequest` per part of the format         |

## Validation

`validate_speakers`, `validate_passage`, `validate_task` return
`Vec<ValidationIssue { severity, item, message }>`. `Severity::Error` means
the key is unusable; `Warning` means look at it. The grounding rule: text
keys and evidence must occur in the normalised passage text.

`validate_exam(&Exam)` checks structural completeness before export or the
full recording: every part has a passage, every `TaskSpec` has a task and,
once nothing is missing, item numbers run from 1 to the format's total in
order. Content issues stay with the drafts that produced them.

## Failures

`DomainError::{InvalidRequest, InvalidPassage, InvalidTask}(String)` —
teacher-readable, no HTTP codes, no stack traces. Infrastructure errors
(`LlmError`, `TtsError`, `AudioError`) are converted to messages at the
application boundary.
