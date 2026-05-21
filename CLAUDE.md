# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

VMQ MVP is a fullstack Rust web application built with **Dioxus 0.7** that generates IELTS listening practice materials using the **Google Gemini API**. It enables teachers to create multi-speaker listening exercises with synthesized audio.

## Commands

```bash
# Local development
dx serve --platform web        # Serve web app with hot reload
dx serve --platform desktop    # Run as native desktop app

# Production build
dx build --platform desktop --release
dx build --platform web --release

# Check compilation without running
cargo check
cargo clippy
```

The `dx` binary is the Dioxus CLI tool. Install it with `cargo install dioxus-cli` if missing.

## API Key Setup

The app requires a Google Gemini API key at `.secrets/api_key.env` (git-ignored):

```
GEMINI_API_KEY=your_key_here
```

Without this file the server functions will fail at runtime. See `DEPLOYMENT.md` for Cloudflare Tunnel deployment options.

## Architecture

The app uses Dioxus **fullstack** mode: Rust code compiles to WASM for the browser and to native code for the server. Server functions (`#[server]` macros in services) execute server-side only and are called from the client as async RPC.

```
src/
├── main.rs                 # App root, router, rate limiter initialization
├── domain/                 # Core types (no I/O, no async)
│   ├── types.rs            # ListeningSection, SpeakerConfig, AudioTrack, etc.
│   ├── commands.rs         # GenerationRequest (input command)
│   └── results.rs          # GenerationResult / GenerationFailure
├── services/               # API integrations and business logic
│   ├── api_config.rs       # Loads GEMINI_API_KEY (server-only)
│   ├── rate_limiter.rs     # Global rate limiters (20 topic/min, 15 script/min, 5 audio/min)
│   ├── topic_generator.rs  # Gemini API → topic suggestions (server fn)
│   ├── script_generator.rs # Gemini API → IELTS dialogue script (server fn)
│   ├── audio_generator.rs  # Gemini TTS → MP3 audio (server fn)
│   └── audio_job_manager.rs# Background job queue to avoid Cloudflare timeout
├── views/
│   └── home.rs             # Main page: topic input, section config, generation UI
└── components/             # Reusable UI components
```

## Domain Model

Key types in `src/domain/types.rs`:

- **`ListeningSection`** — Section1 (2 speakers), Section2 (1), Section3 (up to 4), Section4 (1)
- **`SpeakerConfig`** — name, `Gender`, `Accent`, `SpeakerRole`
- **`ListeningScript`** — ordered dialogue lines; has a `validate()` method checking speaker counts per section
- **`GenerationRequest`** — topic string (10–500 chars) + section
- **`AudioTrack`** — final MP3 bytes output

Domain types are pure Rust with no I/O. Validation lives on the types themselves. See `docs/domain_model.md` and `docs/ubiquitous_language.md` for full invariants.

## Cargo Features

`Cargo.toml` defines platform features: `web`, `desktop`, `mobile`, `server`. Default is `web`. WASM-only dependencies (gloo-*, web-sys, wasm-bindgen) are gated behind `cfg(target_arch = "wasm32")`; server-only code is gated behind `cfg(feature = "server")`.

## Background Job Pattern

Audio generation can exceed Cloudflare's 100s timeout. `audio_job_manager.rs` implements an in-memory job queue (`Arc<Mutex<HashMap<JobId, JobStatus>>>`). The client polls job status rather than awaiting a single long request.

## GitHub Agent Definitions

`.github/agents/` contains custom agent prompts:
- `dioxus-0-7-rust-ui-expert.agent.md` — consult for Dioxus 0.7 UI patterns
- `ielts-listening-domain-architect.agent.md` — consult for domain model decisions
