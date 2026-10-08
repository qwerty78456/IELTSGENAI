# Saved-exam data safety

Saved exams use optimistic concurrency and session-local drafts. The shared layout owns the save queue, so switching between exams or between One part and Whole exam does not discard pending work. Unsaved drafts are held in memory only. Closing or reloading the tab loses drafts that have not reached the server; the browser's leave-page warning is advisory.

## Save protocol

`SaveRequest` requires a snapshot, `expected_revision` and a UUID `mutation_id`. Revision 0 creates an exam; an existing exam starts at revision 1. Updates compare the expected revision and advance it once. Server timestamps, revisions and recording metadata override client metadata. Clients from an earlier release must reload; writes without concurrency fields are rejected.

The last mutation ID and hash form a per-exam retry receipt. A lost response is retried with exactly the same request before sending newer edits. The same final mutation returns the original revision; changed payloads are rejected. If another mutation or deletion has intervened, the stale retry cannot overwrite it.

A conflict pauses only that exam. The session panel offers the local draft, explicit overwrite, or loading the latest server version. Both replacement actions require confirmation. Overwrite uses the freshly read revision, so a further concurrent edit conflicts again. Network failures require a manual retry and retain the original request and latest draft.

Deletion pauses that exam's queue until an in-flight save has a known outcome. It compares revisions and writes a permanent ID-only tombstone in the same transaction. Edits made after confirming deletion are retained as a draft that can be saved under a new ID. There is no trash, version history, cross-tab draft synchronization or crash recovery.

## Recording lifetime

Saves, deletes and recording cleanup use short `BEGIN IMMEDIATE` transactions. A save pins an existing job or detaches a missing reference and returns a warning while still saving the exam text. Job deletion returns file paths only after commit; failed file removal is logged and never restores deleted database references. A missing WAV does not become a zero-duration playable track.

Only terminal jobs with `finished_at_secs` can expire. Completion, failure and the startup interrupted-job sweep set the terminal timestamp once. Late progress or terminal callbacks cannot restart a finished job or extend its retention. Cleanup atomically removes at most 100 expired, unreferenced jobs per pass using `DELETE ... RETURNING`; active or shared recordings survive. `AUDIO_RETENTION_HOURS=0` continues to disable retention cleanup.

## Upgrade and rollback

1. Stop the application and back up the **entire** data directory before upgrading.
2. Update the server and browser assets together. Reload every open tab before editing.
3. Startup migrations add exam revisions and retry fields, the `deleted_exams` tombstone table, and job finish times and indexes. Each migration is transactional and repeatable. Existing exam JSON is unchanged; metadata is overlaid from database columns when read.
4. Existing terminal jobs receive the migration time as their finish time, avoiding an immediate purge of old recordings. Active jobs retain no finish time until they end or the startup interrupted-job sweep fails them.
5. To roll back, restore both the matching application and the complete pre-upgrade data backup. Do not run an older binary on the upgraded database.

Logs identify conflicts, recognized retries, save/delete failures and cleanup counts without logging exam contents or API keys. Tombstones are retained indefinitely and contain only IDs and deletion times.

## Verification

Rust regression tests cover the pure queue, versioned API use case, concurrent SQLite writers, delete/save and cleanup/pin races, retry receipts, shared recordings, missing WAVs, failed file cleanup and legacy migrations. They use temporary databases and controlled synchronization, without Gemini calls.

Run the normal server tests, both feature checks, a wasm32 check, formatting and the portable smoke test. `packaging/save-safety.cjs` adds browser tests against a built release bundle. It requires an existing Playwright installation and Chromium-compatible browser; no Node dependencies are added to the Rust crate. Example:

```text
node packaging/save-safety.cjs target/dx/vmq_mvp/release/web /path/to/chromium
```

Set `PLAYWRIGHT_MODULE` to an existing Playwright module directory if it is not installed on Node's default module path. The script starts a separate server with a temporary data directory and placeholder key, blocks generation endpoints, uses two browser tabs, controls response ordering, simulates offline and lost acknowledgements, and saves a screenshot under `target/`. Test logs and data remain in the printed temporary directory for diagnosis. It never opens or migrates the user's data directory.

### Local verification — 2026-10-08

- Server-feature test suite: **288 passed, 0 failed, 5 ignored** (17 regression tests added). Paid probes were not run.
- `cargo fmt --check`, default/browser `cargo check`, server `cargo check` and `wasm32-unknown-unknown` `cargo check`: passed, using locked offline dependencies.
- Portable Windows build and `packaging/smoke.py`: passed. The pre-existing no-key startup scenario was skipped because a key is present in the Windows registry environment; the test does not clear that configuration.
- `packaging/save-safety.cjs`: all nine groups passed against the final release bundle in a separate headless Brave/Chromium session. This includes two-tab conflicts, confirmed overwrite against a newer revision, background saves, lost acknowledgements, offline retries, stale opens, cancelled opens, remote deletion/save-as-new, edits during deletion, retained unarmed drafts and the leave-page warning. No browser runtime errors occurred.
- Strict Clippy remains blocked by eight pre-existing warnings: duplicate server cfg, enum variant prefixes, a collapsible validation condition, WAV modulo, the job-row tuple type, Gemini boolean expression, needless TTS `Ok(...?)` and a test-only `vec!`. No new warning was introduced; the old exam-row tuple warning disappeared with this change.
- Verification used temporary data directories. No production database was opened or migrated, and nothing was published.
