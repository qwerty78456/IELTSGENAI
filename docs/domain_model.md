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
  the recording). Nothing when `before` is empty. A voice counts only when
  both sides have one: the app assigning a voice to a speaker on `Auto` (the
  catalogue loading, or after "Automatic") is no change.

### Designed voices (`VoiceDesignRequest`, `voice.rs`)
A teacher designs a voice from a description: `VoiceDesignRequest { name,
description, gender, accent }`. `cleaned()` collapses whitespace;
`validate()` wants a name of 1–60 characters and a description of 20–500
characters with at least 10 letters (age, timbre, accent, pace, in English).
Google gets `accent.language_code()`; the app remembers the accent itself,
since en-GB is both British and Scottish English. A designed voice is a
`Voice` with `source: Designed`: it belongs to the API key's Google project,
is only ever `Chosen` (never assigned from the catalogue), brings its gender
and accent with it (`with_voice`), and reads each of its turns in a request of
its own. Designed voices made outside the app get the first accent of their
language tag.

### Out of date is derived, never flagged
A script and a recording each remember the line-up they were made for:
`Passage.written_for` and `ExamPart.recorded_for` (the part page keeps its
recording's line-up beside the track). The current speakers are compared with
them whenever the page renders:
- the script no longer fits when `passage.speakers_changed(&speakers).script`:
  `validate_passage` warns and the page offers **Rewrite** or **Keep this
  script** (`for_speakers(current)`);
- the recording is stale when it exists and
  `speaker_change(&recorded_for, &speakers).recording` (`ExamPart::recording_stale`);
  the exam recording is stale when any part's is.

Empty line-ups (scripts and recordings from before 0.8) compare as unchanged,
so opening an old exam, or assigning voices once the catalogue is loaded,
marks nothing. A regenerated script still flags the exam recording
(`SavedExam.recording_stale`), as before.

## Content

### `Passage`
`part`, `topic`, `lines: Vec<Line { speaker, text }>`, `written_for:
Vec<SpeakerConfig>` (the line-up the script was written for; `#[serde(default)]`,
empty before 0.8). `generate_passage` returns `parse(..).for_speakers(&request.speakers)`;
`speakers_changed(&current) -> SpeakerChange` compares with it (all false when
empty). `Passage::parse` reads "Speaker A: ..." text. `Line.text` keeps any
speech markup (below); `Line::display_text()` and `Line::speech_text(backchannels)`
are its two readings. The text views:

| View | Markup | Used by |
|------|--------|---------|
| `script_text()` | kept | the teacher's on-screen script |
| `transcript_text()` | none | question prompts, "Download script", transcripts |
| `plain_text()` | none | grounding (`normalize` strips markup too) |
| `word_count()` / `estimated_minutes()` | not counted | length checks (150 words per minute) |

Text-to-speech receives each line's `speech_text` without labels, the speaker
travelling beside each line.

### Speech markup (`speech.rs`)
Gemini 3.8 TTS reads its text aloud except inline tags in angle brackets
(`<sigh>`), which it performs, and `|backchannels|` in a two-voice request. The
one grammar for that markup:
- `SPEECH_TAGS` — every tag Google documents for 3.8, variants included; used to
  recognise markup. `speech_tag(name)` gives the documented form (lower case,
  "laughs" read as "laugh").
- `EXAM_SPEECH_TAGS` — `sigh`, `cough`, `laugh`, `chuckle`: the only tags an
  expressive script asks for (measured, `docs/voices.md` gate G3).
  `READ_ALOUD_TAGS` — `long pause`, `whispers`, `whispering`: heard read aloud,
  never sent.
- The scanner: `<x>` is a tag when x is 1-32 letters, spaces, hyphens or
  apostrophes starting and ending with a letter (so `5 < 6` stays text); `[x]`
  on one line is a note; `|x|` is a backchannel; any other `|` is a stray pipe.
- `display_text(text)` — the words a listener hears: all markup removed, the
  seams tidied (one space, none before punctuation, no comma left over).
- `speech_text(text, backchannels)` — what TTS gets: documented tags kept in
  lower case (except `READ_ALOUD_TAGS`), unknown tags, notes and stray pipes
  dropped, backchannels kept only when asked for. Text without markup is
  returned unchanged, so earlier takes stay cached.
- `tokens(text)` / `spoken_words(text)` — whitespace-separated pieces with
  markup kept whole; a tag never counts as a word.
- `markup_problems(text)` — teacher-readable problems with one turn: an unknown
  tag, a tag read aloud, a tag not tested for exams, a `[note]` (except
  `[FILL ...]`, a gap), a stray pipe, `(laughs)` or `*sighs*` written as a stage
  direction, a tag opening the turn, more than one tag in a turn under 40 words.

There is no per-line delivery style: a style that changes between lines moves
the voice (gate G5), so every turn of a speaker keeps its role's
`delivery_style()` and emotion comes from wording, punctuation and the allowed
tags.

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
`audio: Option<AudioTrack>`, `recorded_for: Vec<SpeakerConfig>` (the line-up,
voices included, the part was last recorded with: what `start_exam_audio`
sent back; `#[serde(default)]`, empty before 0.8). `recording_stale()` is
`speaker_change(&recorded_for, &speakers).recording`; whether a recording
exists is the caller's to check.

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
voices) | `Naming` (the five-word summaries in download names), stored by
`key()`; `label()` names it in the spend breakdown, where `always_listed()`
steps show even at $0 and `Voices` and `Naming` only once they cost something.

### `ExamUsage`
One `Usage` per step (`voices` and `naming` default to empty for older data) plus
`budget_micro_usd` (0 = none); `step()`, `step_mut()`, `total()`,
`over_budget()`, `budget_text()`. Built by `application::usage::exam_usage`
from the ledger; it includes failed and superseded runs.

## Commands

| Command            | Validates                                              |
|--------------------|--------------------------------------------------------|
| `PassageRequest`   | topic length/content, part exists, speaker count/labels (first Error; warnings never block). `expressive` (`#[serde(default)]`, false) lets the script carry a few `EXAM_SPEECH_TAGS` |
| `TaskRequest`      | part and task index exist, passage not empty            |
| `AudioRequest`     | passage not empty, every used label configured, every speaker has a voice (none on `Auto`) with a valid id, no shared voice, no voice of the other gender |
| `ExamAudioRequest` | one valid `AudioRequest` per part of the format (errors name the part) |

Both pages start new work expressive; a saved exam keeps the choice
(`SavedExam.expressive`, false for exams saved before 0.8).

Voices are assigned before an `AudioRequest` is validated: the browser and the
server both run `assign_voices` first. The server also refuses, before the job
starts, a chosen designed voice that is not in the key's Google project
("Speaker B's designed voice "X" is not in the Google project of this API key;
choose another voice."). `AudioRequest.fresh` (`#[serde(default)]`)
asks for a **new take**: the job reads that passage again instead of reusing
speech made before for the same words and voices (`Reuse::Refresh`) and keeps
the new take in their place. In an `ExamAudioRequest` it is per part; the
other parts and the announcements reuse what they can.

## Validation

`validate_speakers`, `validate_passage`, `validate_task` return
`Vec<ValidationIssue { severity, item, message }>`. `Severity::Error` means
the key is unusable; `Warning` means look at it. `first_error` picks the
issue that blocks. `validate_speakers` adds a Warning per `voice_conflicts`
entry: the script does not depend on the voice, the recording does.
`validate_passage` adds a Warning per speaker whose gender, accent or role
differs from `written_for` ("Speaker B was … when the script was written and
is now …; rewrite the script or keep it"); a new voice alone gives none. It
also adds a Warning per `markup_problems` entry, prefixed with the turn
("Speaker B (turn 4): <smirk> is not a speech tag; it is left out of the
recording"); a gap (`___`, `[FILL ...]`) stays an Error. The grounding rule:
text keys and evidence must occur in the normalised passage text, where
`normalize` drops speech markup first, so a quote matches with or without a
`<sigh>` in it.

A saved exam (`application::exams::check`) must have each part's speaker count
and distinct labels and valid voice ids, but two speakers sharing a voice are
saved as they are: the recording refuses them, an autosave never loses work.

`validate_exam(&Exam)` checks structural completeness before export or the
full recording: every part has a passage, every `TaskSpec` has a task and,
once nothing is missing, item numbers run from 1 to the format's total in
order. Content issues stay with the drafts that produced them.

## Failures

`DomainError::{InvalidRequest, InvalidPassage, InvalidTask}(String)` —
teacher-readable, no HTTP codes, no stack traces. Infrastructure errors
(`LlmError`, `TtsError`, `AudioError`) are converted to messages at the
application boundary.
