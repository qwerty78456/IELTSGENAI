# Repository Guidelines

## Project Structure & Module Organization

This is one Rust 2024/Dioxus 0.7 crate (`vmq_mvp`). `src/main.rs` wires the app. `src/domain/` holds exam types and validation; `src/application/` exposes server use cases; `src/infrastructure/` contains Gemini, prompts, TTS, audio, jobs, and configuration. `src/export/` renders Markdown, while `src/ui/` contains views and components. CSS and images live in `assets/`; architecture and domain terminology live in `docs/`. Tests are inline with the Rust modules they cover, rather than in a separate `tests/` directory.

## Build, Test, and Development Commands

Use Rust 1.85+, the `wasm32-unknown-unknown` target, and Dioxus CLI 0.7.x. Copy `.env.example` to `.env` and set `GEMINI_API_KEY` before generating content.

- `dx serve` starts the hot-reloading app at `http://localhost:8080`.
- `dx build --release` builds the server and browser assets under `target/dx/vmq_mvp/release/web/`.
- `cargo check` checks the default browser feature; `cargo check --features server --no-default-features` checks the server feature.
- `cargo test --features server --no-default-features` runs the full unit-test suite, including server-only modules.

## Coding Style & Naming Conventions

Follow rustfmt defaults: four-space indentation, `snake_case` modules/functions/tests, and `PascalCase` types and components. Run `cargo fmt` before submitting; there is no repository-specific rustfmt or Clippy configuration. Keep `domain` and `export` independent of UI and server libraries. Put `#[server]` functions in `application`, server-only integrations in `infrastructure`, and UI calls through `application`. Use the names defined in `docs/domain_model.md` and `docs/ubiquitous_language.md`; consult `docs/architecture.md` for layer boundaries.

## Testing Guidelines

Add focused `#[test]` cases in nearby `#[cfg(test)]` modules; use `#[tokio::test]` where async behavior requires it. Name tests for the behavior being checked, such as `multiple_select_needs_distinct_letters`. Cover new validation rules and task kinds. No numeric coverage threshold is configured. Run both `cargo check` variants and the server-feature test command before a pull request.

## Commit & Pull Request Guidelines

Recent commits use short imperative subjects, sometimes with prefixes such as `chore:` or `refactor:`; no strict commit format is enforced. Keep each commit focused. No pull-request template exists: describe the change, link an issue when relevant, include screenshots for UI changes, and report the check and test results.

## Security & Configuration

Configuration comes from environment variables. Keep `.env`, `.secrets/`, and generated `data/` out of commits. Public deployments need TLS and authentication because the app has no built-in login and generation incurs API usage.
