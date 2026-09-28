# Project Scope

## Purpose

Give a teacher who prepares students for listening exams a tool that
produces, from a topic, a complete draft of one exam part in a chosen
format: script, questions, key, transcript and audio. The teacher reviews
and edits; the tool never claims to produce an official exam.

## In scope

- Formats as data: IELTS Listening and HSG Quốc gia (Listening) ship;
  teacher-defined formats are the same type.
- Script generation shaped by the part (conversation, interview, monologue,
  excerpt) and by the tasks that will follow.
- Question generation per task kind with a validator that grounds every key
  in the script and enforces word limits, option letters and numbering.
- Paper, key and transcript export as Markdown and Word (DOCX).
- Saved exams on the server, reopened with their recording.
- Audio: per-part recordings and the full exam recording with announcements,
  tones, pauses and replays, as background jobs.
- Knowing and bounding the cost: every Gemini request is metered and priced,
  an exam's spend is shown against a budget (warning only), and synthesised
  speech is reused instead of paid for twice.
- Deployment as one container behind a reverse proxy, or as a portable
  Windows EXE / Linux AppImage on the teacher's own computer.

## Out of scope

- Grading, scoring, candidate accounts, analytics.
- Reading, writing, speaking.
- Any claim of equivalence with an official paper.
- Real-time collaboration.

## Non-goals for the next milestone

- Multi-tenant SaaS features (billing customers, organisations). Tracking
  what Gemini costs us is in scope; charging anyone for it is not.
- Fine-tuned models; prompts plus validation are the quality lever.

## Quality bar

- Every generated key passes the validator or is flagged with an issue the
  teacher can see.
- The domain layer is unit-tested and compiles for wasm and the server.
- No secret in URLs or logs; the Gemini key travels in a header and is never
  sent back to the browser.
- A full IELTS exam costs at most $0.70 of Gemini usage at the prices Google
  announced for 2027 (measured $0.616 on 2026-09-28).
