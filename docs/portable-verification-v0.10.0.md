# Portable 0.10.0 verification — 2026-10-14

Windows x64 and Linux x86-64 releases built from `45d98d4` with Rust 1.92.0, Dioxus CLI 0.7.9 and locked dependencies, using `packaging/build-windows.ps1` and `packaging/build-linux.ps1` (Podman connection `ielts-portable-builder`, Ubuntu 22.04 builder).

| Check | Result |
|---|---|
| Formatting, browser/server/wasm32 checks | Passed, 0 warnings |
| Server-feature tests | 318 passed, 0 failed, 7 ignored on Windows; 311 passed, 0 failed, 7 ignored inside the Linux builder (Windows-only tests are not compiled there) |
| Release server and browser assets | Built together, version 0.10.0 |
| Windows portable smoke test | Passed, including the text-provider setting and the startup key lines; no-key and no-Anthropic-key startups skipped because Windows has both keys configured |
| Linux AppImage smoke test | Passed on clean Ubuntu 22.04 and 24.04 as user `nobody`, including starting with no key anywhere |
| `dx serve` | Text-provider choice on both pages; Claude saved with an exam and restored on opening it; "AI spend" names both key sources; no console or server errors |
| Save-safety browser regressions | Not run: Playwright is not installed on this machine |
| `claude_paid_probe` | Passed: key check, token counts, topic, IELTS Part 4 script (619 words, 0 issues, 11.3 s), questions (10 items, 0 issues, 7.1 s), Haiku summary (1.1 s); fallback beta header accepted; $0.033 |
| `text_provider_compare_probe` | One IELTS exam's text, four scripts at once: Gemini 3.8 Flash 5 errors, 2 warnings, $0.039; Claude Sonnet 5.5 6 errors, 1 warning, $0.130; no parse failures, no retries |
| Paid requests | Anthropic $0.163, Gemini $0.039 (the two probes above) |

Artifacts:

| File | Size | SHA-256 |
|---|---|---|
| `listening-exam-generator-0.10.0-windows-x64.exe` | 9,821,696 bytes | `39232a73f1b9f5e9907fccb0ad2df48f09d4aa715c54f29ced4e80aac14cb648` |
| `listening-exam-generator-0.10.0-linux-x86_64.AppImage` | 13,625,848 bytes | `ae497b3c5d8226a553135c39de4a01ea55af951a5f73456142eb76d356df12ee` |

`packaging/build-linux.ps1` now extracts the source with `tar --no-same-owner`: the archive made by Git's tar on Windows carries MSYS user ids that the rootless builder cannot assign.

The smoke suites used separate temporary data directories and did not open the user's database. Nothing changes for an existing installation until Claude is chosen: `TEXT_PROVIDER` defaults to `gemini`.
