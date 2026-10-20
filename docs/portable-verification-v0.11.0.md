# Portable 0.11.0 verification — 2026-10-20

Windows x64 and Linux x86-64 releases built from `f60c205` with Rust 1.92.0, Dioxus CLI 0.7.9 and locked dependencies, using `packaging/build-windows.ps1` and `packaging/build-linux.ps1` (Podman connection `ielts-portable-builder`, Ubuntu 22.04 builder).

| Check | Result |
|---|---|
| Formatting, browser/server/wasm32 checks | Passed, 0 warnings |
| Server-feature tests | 367 passed, 0 failed, 7 ignored on Windows; 360 passed, 0 failed, 7 ignored inside the Linux builder (Windows-only tests are not compiled there) |
| Default-feature tests (`cargo test`) | 146 passed, 0 failed, 1 ignored (0.10.0 did not compile them) |
| `cargo clippy --all-targets --features server --no-default-features` | 8 warnings, all present before this release (0.10.0: 9) |
| Release server and browser assets | Built together, version 0.11.0 |
| Access matrix (curl against isolated release servers) | 28 cases as expected: same-origin and `Host: app.localhost` answer; `Sec-Fetch-Site: cross-site` and `same-site`, a rebound `Host`, `Origin: null`, a foreign `Origin` and forwarding headers on the main port are refused with the address to open; the published port answers its `PUBLIC_HOST` only; `/audio` answers 404 before 403 and carries `Cache-Control: private, no-cache`; a network bind answers a rewritten `Host` and refuses other sites |
| Editors in the browser (isolated server) | Text answers typed key by key keep "dynamic retention x", "2,500" and "24/7"; a wrong speaker label names its line; the open script editor survives a visit to the other page; an unchanged Apply sends no `save_exam` |
| Save-safety browser regressions (`packaging/save-safety.cjs`, release bundle, headless Brave through `playwright-core` 1.64.1) | 10/10 groups passed, including the new discard group; 0 runtime errors |
| Windows portable smoke test | Passed, including the text-provider setting, the startup key lines and closing the console window; no-key and no-Anthropic-key startups skipped because Windows has both keys configured |
| Linux AppImage smoke test | Passed on clean Ubuntu 22.04 and 24.04 as user `nobody`, including starting with no key anywhere; native FUSE mounting not tested (no `/dev/fuse` in the container) |
| Paid requests | Gemini $0.0039 (one script and one file name to try the editors) |

Artifacts:

| File | Size | SHA-256 |
|---|---|---|
| `listening-exam-generator-0.11.0-windows-x64.exe` | 10,171,392 bytes | `19b082ce3d02ef816677a275b2e5fe398573ff9c7cfbb6fb1568b6475a433e35` |
| `listening-exam-generator-0.11.0-linux-x86_64.AppImage` | 13,773,304 bytes | `96e2e1e49a933c1f16d106ff3cfd87dc11a7427d7b585d53cac082b4f4404991` |

`packaging/build-linux.ps1` now runs `cargo fetch --locked` before the build and the build itself with `CARGO_NET_OFFLINE=true`. On a slow network `dx build` gave up twice ("cargo metadata took too long"), because its `cargo metadata` needs the crates of every platform and waits only so long for them. For this build the network stayed slow during the fetch as well: the 31 crates still missing were copied from the Windows host's Cargo cache after their SHA-256 matched `Cargo.lock`, `cargo fetch --locked --offline` then passed, and `packaging/build-linux.sh` ran offline in the same container.

Release WASM is still not size-optimized: the `wasm-opt` that dx 0.7.9 downloads (binaryen 127) fails on this app's WASM on Windows (0xc0000409) and on Linux (SIGABRT, "compile unit size was incorrect (this may be an unsupported version of DWARF)"), and dx keeps the unoptimized file.

The smoke suites and the isolated servers used separate temporary data directories and did not open the user's database.
