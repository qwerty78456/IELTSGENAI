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
| **Speaker** (`SpeakerConfig`) | A person heard in the passage: label + gender + accent + role + voice choice. The label is "Speaker A", never a character name. | "voice" (that is the `Voice`) |
| **Voice** (`Voice`) | The TTS voice a speaker is read with, identified by its id. Two speakers of a part never share one. | "speaker", "actor" |
| **Library voice** (`VoiceSource::Library`) | A Google prebuilt voice (Extended Voice Library, id like `en-gb-advisor-1`, or a classic voice). | |
| **Designed voice** (`VoiceSource::Designed`) | A voice made with Voice Design from a description (id `voice_…`); it belongs to the API key's Google project. | "custom voice" |
| **Voice pool** | The catalogue voices of one accent and gender, in preference order; assignment takes from it. | |
| **Voice choice** (`VoiceChoice`) | How a speaker got its voice: Automatic (none yet), assigned by the app, or chosen by the teacher (kept). | |
| **Voice catalogue** (`VoiceCatalogue`) | Every voice this server gives speakers (the pools, in preference order) and the announcer: the built-in pools plus `voices.json` overrides. | "voice list" is fine in UI text |
| **Voice sample** (`VoiceSample`) | A short recording of one voice, made once and kept, behind "Listen". | "preview" |
| **Delivery style** (`SpeakerRole::delivery_style`) | A few words on how a speaker sounds ("polite and helpful"), sent beside the text and never spoken; never age, gender, accent or a name. One per speaker, the same on every turn: there is no per-line style (it moves the voice). | "stage direction" |
| **Speech tag** (`speech::SPEECH_TAGS`, `EXAM_SPEECH_TAGS`) | A sound written inside a line in angle brackets, `<sigh>`, that the speech model performs instead of reading. Scripts ask only for `<sigh>`, `<cough>`, `<laugh>`, `<chuckle>`, mid-sentence; transcripts, questions and grounding never see them. | "[sigh]", "(laughs)", "emotion tag" |
| **Backchannel** | A listener's short reaction inside the other speaker's turn, written `\|mhm\|`. Recognised and kept only in a two-voice request on Flash TTS; never asked for, never printed. | |
| **Expressive script** (`PassageRequest::expressive`) | A script that may carry a few speech tags, one every few turns; its emotion otherwise comes from wording and punctuation. On by default for new work. | "emotional script" |
| **Transcript** (`Passage::transcript_text`) | The words a listener hears, by speaker, without speech tags: what is printed, downloaded and quoted to question prompts. The **script** (`script_text`) is the same with its tags, for the teacher's screen. | |
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
| **Written for** (`Passage::written_for`) | The speakers a script was written for. When a speaker's gender, accent or role changes afterwards, the teacher rewrites the script or keeps it (which makes the current speakers its "written for"). | |
| **Recorded for** (`ExamPart::recorded_for`) | The speakers, voices included, a part was last recorded with. | |
| **Stale** | A script or recording made for other speakers than the current ones. Always worked out by comparing with "written for" / "recorded for", never set as a flag; scripts and recordings from before 0.8 are never stale for this reason. | "dirty", "invalid" |
| **Take** / **New take** (`AudioRequest::fresh`) | One reading of a part's script by the speech model. A new take reads the part again instead of reusing the earlier take for the same words and voices, and pays for it again. | "re-record" is fine in UI text |
| **Saved exam** (`SavedExam`) | An exam kept on the server with the teacher's topics and the recording it refers to, so the teacher can come back to it. Still a draft. | "final", "record" |
| **Draft** | What generation returns: a passage or task plus validator **issues**. Everything is a draft until the teacher accepts it. | "final" |
| **Issue** (`ValidationIssue`) | A validator finding with a severity: Error (unusable key) or Warning (look at it). | "bug" |
| **Grounding** | The rule that a text key or evidence must occur verbatim in the passage. | |
| **Usage** (`Usage`, `ExamUsage`) | What Gemini billed for a step: requests, reused chunks, input / cached / output / thinking tokens, and their USD price at the time. An exam's usage adds up every run, failed and superseded ones included. | "billing" (out of scope), "credits" |
| **Budget** | The amount one exam should stay under (`EXAM_BUDGET_USD`). Passing it warns; it never blocks. | "quota", "limit" |
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
