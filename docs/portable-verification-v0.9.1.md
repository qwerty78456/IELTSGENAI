# Portable 0.9.1 verification — 2026-10-08

Windows x64 release built with Rust 1.92.0, Dioxus CLI 0.7.9 and locked dependencies using `packaging/build-windows.ps1`.

| Check | Result |
|---|---|
| Formatting, browser/server/wasm32 checks | Passed |
| Server-feature tests | 288 passed, 0 failed, 5 ignored |
| Release server and browser assets | Built together, version 0.9.1 |
| Windows portable launcher | Built with static CRT |
| Portable smoke test | Passed; no-key startup skipped because Windows has a configured key |
| Save-safety browser regressions | All nine groups passed in headless Brave/Chromium |
| Paid Gemini requests | None |
| Strict Clippy | Eight pre-existing warnings; no new warnings |
| Linux 0.9.1 | Not built in this verification; Linux 0.9.0 remains the previous verified artifact |

Artifact: `dist/listening-exam-generator-0.9.1-windows-x64.exe`  
Size: **9,780,736 bytes**  
SHA-256: `171e7e21480d4b39fde043d43781815a4c7608213ce603ea5a515f30d994c041`

The smoke and browser suites used separate temporary data directories. They did not open or migrate the user's database. Browser checks cover two-tab conflicts, confirmed overwrite against newer revisions, background saves during navigation, lost acknowledgements, offline retries, stale/cancelled opens, remote deletion and edits during deletion, and session-only draft recovery and leave-page warnings.

Before upgrading, stop the app and back up the entire data directory. Update server and browser assets together and reload old tabs. Rollback must restore the matching application and the complete pre-upgrade data backup. See [saved-exam data safety](save-safety.md) for protocol and migration details.
