# VMQ MVP - IELTS Listening Audio Generator
VMQ MVP is a full-stack web application built with **Rust** and **Dioxus**. It leverages the **Google Gemini TTS API** to generate high-quality, multi-speaker IELTS listening audio scripts. 
The application is designed to be highly concurrent, managing long-running audio generation tasks in the background using `sqlx` and `tokio`, ensuring a smooth user experience without request timeouts.
## ✨ Features
- **Multi-Speaker TTS**: Supports generating audio with multiple distinct voices, accents (British, American, Australian, etc.), and roles (Student, Professor, Guide, etc.) mimicking real IELTS listening tests.
- **Asynchronous Background Processing**: Uses an Async SQLite-backed job queue to process heavy TTS requests in the background, preventing timeouts and UI blocking.
- **Dioxus Fullstack**: A unified codebase for both the frontend (WebAssembly/HTML) and backend server logic in pure Rust.
- **Docker Ready**: Fully containerized with a multi-stage Dockerfile for easy deployment to any Linux server (e.g., Ubuntu).
- **Automated Cleanup**: Built-in background tasks automatically clean up temporary audio files and database records older than 24 hours.
## 🚀 Quick Start (Docker - Recommended)
The easiest way to run the application is using Docker and Docker Compose.
1. **Clone the repository**:
   ```bash
   git clone <your-repo-url>
   cd vmq_mvp
   ```
2. **Configure Environment Variables**:
   Copy the example environment file and add your Gemini API key:
   ```bash
   cp .env.example .env
   # Edit .env and set your GEMINI_API_KEY
   ```
3. **Start the application**:
   ```bash
   docker compose up -d
   ```
   The application will be available at `http://localhost:8080` (or whichever port you specified).
## 🛠️ Local Development
### Prerequisites
- **Rust**: Ensure you have the latest stable Rust toolchain installed.
- **Dioxus CLI**: Install the Dioxus CLI tool:
  ```bash
  cargo install dioxus-cli --version 0.6.1
  ```
- **System Dependencies**: You may need `pkg-config` and OpenSSL headers installed on your system (e.g., `libssl-dev` on Ubuntu).
### Running Locally
1. Create a `.env` file in the root directory:
   ```env
   GEMINI_API_KEY=your_gemini_api_key_here
   PORT=8080
   DATA_DIR=./data
   ```
2. Run the application using the Dioxus CLI:
   ```bash
   dx serve
   ```
   *Note: Using `dx serve` will run both the frontend and backend server concurrently with hot-reloading enabled.*
## 📁 Project Structure
- `src/views/` - Frontend components, pages, and UI layouts.
- `src/components/` - Reusable UI components.
- `src/services/` - Backend logic, including API integrations, database management (`sqlx`), and job queuing.
- `src/domain/` - Shared domain logic and types (models) used by both frontend and backend.
- `data/` - (Auto-generated) Local storage for the SQLite database, generated WAV files, and logs.
- `voices.json` - Configuration file for mapping specific accents and genders to Gemini TTS voices.