# Repository Guidelines

## Project Structure & Module Organization

This is one Rust 2024/Dioxus 0.7 crate (`vmq_mvp`). `src/main.rs` wires the app. `src/domain/` holds exam types and validation; `src/application/` exposes server use cases; `src/infrastructure/` contains the Gemini client (Interactions and Voices API) and price table, prompts, the voice catalogue (built-in pools in `tts/default_voices.json`), voice samples and designed voices, TTS chunking and the speech cache, audio, jobs, saved exams, the usage ledger, and configuration. `src/export/` renders Markdown and DOCX, while `src/ui/` contains views and components. CSS and images live in `assets/`; architecture, domain terminology and the voice measurements (`docs/voices.md`) live in `docs/`; `tools/voice_lab.py` probes and auditions voices against the paid API. Tests are inline with the Rust modules they cover, rather than in a separate `tests/` directory.

## Build, Test, and Development Commands

Use Rust 1.88+ (the code uses let-chains; releases are built with 1.92), the `wasm32-unknown-unknown` target, and Dioxus CLI 0.7.x. Generation needs a Gemini key: set `GEMINI_API_KEY` in the environment or in `.env` (copy `.env.example`); without one the app starts and asks for it in the browser. Every generation spends real money, so ask before running `live_probe`, `voice_live_probe`, `tools/voice_lab.py` (only `catalog` and `report` without `--ear` are free) or generating exams.

- `dx serve` starts the hot-reloading app at `http://localhost:8080`.
- `dx build --release` builds the server and browser assets under `target/dx/vmq_mvp/release/web/`.
- `cargo check` checks the default browser feature; `cargo check --features server --no-default-features` checks the server feature.
- `cargo test --features server --no-default-features` runs the full unit-test suite, including server-only modules. The ignored `live_probe` test calls the paid API (`-- --ignored live_probe`).
- `pwsh -NoProfile -File packaging/build-windows.ps1` then `python packaging/smoke.py dist/listening-exam-generator-<version>-windows-x64.exe` builds and tests the portable Windows EXE (see `docs/portable.md`).

## Coding Style & Naming Conventions

Follow rustfmt defaults: four-space indentation, `snake_case` modules/functions/tests, and `PascalCase` types and components. Run `cargo fmt` before submitting; there is no repository-specific rustfmt or Clippy configuration. Keep `domain` and `export` independent of UI and server libraries. Put `#[server]` functions in `application`, server-only integrations in `infrastructure`, and UI calls through `application`. Use the names defined in `docs/domain_model.md` and `docs/ubiquitous_language.md`; consult `docs/architecture.md` for layer boundaries.

## Testing Guidelines

Add focused `#[test]` cases in nearby `#[cfg(test)]` modules; use `#[tokio::test]` where async behavior requires it. Name tests for the behavior being checked, such as `multiple_select_needs_distinct_letters`. Cover new validation rules and task kinds. No numeric coverage threshold is configured. Run both `cargo check` variants and the server-feature test command before a pull request.

## Commit & Pull Request Guidelines

Recent commits use short imperative subjects, sometimes with prefixes such as `chore:` or `refactor:`; no strict commit format is enforced. Keep each commit focused. No pull-request template exists: describe the change, link an issue when relevant, include screenshots for UI changes, and report the check and test results.

## Security & Configuration

Configuration comes from environment variables (on Windows including the user and machine environment in the registry) and `.env`. Keep `.env`, `.secrets/`, and generated `data/` out of commits, and never print or log the API key. Public deployments need TLS and authentication because the app has no built-in login and generation incurs API usage; a server bound to anything but loopback never accepts a key from the browser.
