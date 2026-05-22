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
Docker is the easiest way to run this app.

1. **Clone the repository**:
   ```bash
   git clone <https://github.com/qwerty78456/IELTSGENAI>
   cd vmq_mvp
   ```

2. **Set up your environment variables**:
   - **Ubuntu/Debian:**
     ```bash
     cp .env.example .env
     ```
   - **Windows (PowerShell):**
     ```powershell
     copy .env.example .env
     ```
   Open `.env` in a text editor and set your `GEMINI_API_KEY`.

3. **Start the application**:
   ```bash
   docker compose up -d
   ```
   The app will run at `http://localhost:8080`.

## 🛠️ Local Development

### 1. Install Prerequisites

**Ubuntu/Debian:**
```bash
sudo apt update
sudo apt install pkg-config libssl-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo install dioxus-cli --version 0.6.1
```

**Windows:**
1. Install [Rust](https://rustup.rs/) (this will prompt you to install Visual Studio C++ Build Tools).
2. Install `dioxus-cli` via PowerShell:
   ```powershell
   cargo install dioxus-cli --version 0.6.1
   ```
*(Note: Native Windows compilation might require OpenSSL depending on features. Using WSL2 on Windows is highly recommended for a smoother experience.)*

### 2. Configure Environment
Create a `.env` file in the project root:
```env
GEMINI_API_KEY=your_gemini_api_key_here
PORT=8080
DATA_DIR=./data
```

### 3. Run the App
```bash
dx serve
```
This runs both frontend and backend concurrently with hot-reloading.
## 📁 Project Structure
- `src/views/` - Frontend components, pages, and UI layouts.
- `src/components/` - Reusable UI components.
- `src/services/` - Backend logic, including API integrations, database management (`sqlx`), and job queuing.
- `src/domain/` - Shared domain logic and types (models) used by both frontend and backend.
- `data/` - (Auto-generated) Local storage for the SQLite database, generated WAV files, and logs.
- `voices.json` - Configuration file for mapping specific accents and genders to Gemini TTS voices.
