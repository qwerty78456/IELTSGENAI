# Ubiquitous Language

Terms of the **Listening Assessment Generation** context. Use them in code,
prompts, docs and conversation. Type names in `src/domain/` match them.

| Term | Meaning | Not |
|------|---------|-----|
| **Format** (`ExamFormat`) | The blueprint of an exam: parts, playback rules, task plan, timing. IELTS Listening and HSG Quốc gia are formats. | "template", "mode" |
| **Part** (`PartSpec`) | One recording and its tasks. Numbered 1–4 in both shipped formats. | "section" (IELTS legacy wording; do not use in code) |
| **Passage** | The script of one part: ordered lines attributed to speaker labels. | "script" is acceptable in the UI; the type is `Passage` |
| **Passage kind** (`PassageKind`) | Conversation, Interview (host + guests), Monologue, Excerpt ("part of a talk"). | |
| **Playback** (`PlayCount`) | Once or Twice. Decides replays in the audio programme. | |
| **Speaker** (`SpeakerConfig`) | A voice: label + gender + accent + role. The label is "Speaker A", never a character name. | "voice" alone (that is the TTS voice name) |
| **Task** | One question block under one rubric: a task kind over a contiguous item range. | "exercise", "section" |
| **Task kind** (`TaskKind`) | T/F/NG, who-mentioned, multiple selection, multiple choice, short answer, summary / note / sentence completion, matching. | |
| **Item** | One numbered question with its key and evidence. | "question" is fine in UI text; the type is `Item` |
| **Key** (`Answer`) | The accepted answer(s) of an item. | "solution" |
| **Evidence** | Verbatim words from the passage that justify the key. | |
| **Word limit** (`WordLimit`) | "NO MORE THAN n WORDS AND/OR A NUMBER". | |
| **Exam** | The aggregate: a format filled in with parts, passages, tasks and audio. | "test" in code (fine in prose) |
| **Answer key** | Every item's key in paper order, derived from the exam. | |
| **Audio programme** (`AudioProgram`) | The ordered plan of the full recording: music, tone, announcement, pause, passage, replay, checking time. | |
| **Recording** (`AudioTrack`) | Where a rendered WAV lives and how long it is. Never the bytes. | |
| **Draft** | What generation returns: a passage or task plus validator **issues**. Everything is a draft until the teacher accepts it. | "final" |
| **Issue** (`ValidationIssue`) | A validator finding with a severity: Error (unusable key) or Warning (look at it). | "bug" |
| **Grounding** | The rule that a text key or evidence must occur verbatim in the passage. | |
| **Teacher** | The person we serve. | "user" |

## Format-specific vocabulary

**HSG Quốc gia (Listening, 5.0 points, 35 items, 30 minutes)** — Part 1
interview played once (T/F/NG 1–5, who-mentioned 6–10 with letters S/A/B),
Part 2 talk played once (choose 2 of 5, choose 3 of 7, multiple choice
16–20), Part 3 excerpt played twice (short answer ≤ 2 words, 21–25), Part 4
excerpt played twice (summary completion ≤ 1 word and/or a number, 26–35),
two minutes to check.

**IELTS Listening (40 items, 30 minutes + transfer)** — four parts played
once: everyday conversation, everyday monologue, academic conversation,
academic monologue. Task kinds vary per paper; the preset ships a typical plan.

## Retired terms

`ListeningSection`, `Section1..4`, `GenerationRequest`, `ListeningScript`,
`GenerationResult` (replaced by `PartSpec`, `PassageRequest`, `Passage`,
`PassageDraft` / `TaskDraft`). The rule "this product does not generate
questions" is withdrawn: generating questions with a key is the point.
