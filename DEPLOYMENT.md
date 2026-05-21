# Deployment Guide — VMQ MVP

## Quick Start (Public Internet via Cloudflare)

### Prerequisites
- Static WAN IPv4 address
- Cloudflare account with a domain
- Google Gemini API key
- Windows 10/11

### Step 1: Run the Setup Script

Right-click `setup.bat` and **Run as Administrator**. This opens firewall ports and creates the audio storage directory.

### Step 2: Configure API Key

Create the file `.secrets/api_key.env`:
```
GEMINI_API_KEY=your_google_gemini_api_key_here
```

### Step 3: Router Port Forwarding

In your router admin panel, forward:
- **TCP port 80** → your laptop's LAN IP
- **TCP port 443** → your laptop's LAN IP (optional, for direct HTTPS)

### Step 4: Cloudflare DNS Setup

1. Go to Cloudflare Dashboard → your domain → DNS
2. Add an **A record**: `@` (or a subdomain like `ielts`) → your static WAN IP
3. Enable the **orange proxy cloud** (Cloudflare Proxy)
4. Go to **SSL/TLS** → set mode to **Flexible** (Cloudflare handles HTTPS, connects to your server via HTTP on port 80)

### Step 5: Build and Run

```powershell
# Development (hot-reload)
dx serve --platform web

# Production build
dx build --platform web --release
.\target\release\vmq_mvp.exe
```

The app binds to `0.0.0.0:80` by default. Override with environment variables:
```powershell
$env:IP = "0.0.0.0"
$env:PORT = "8080"
.\target\release\vmq_mvp.exe
```

---

## API Key Security

The API key is loaded from `.secrets/api_key.env` (git-ignored). It is **never sent to the browser** — all API calls happen server-side via `#[server]` functions.

The app accepts both `GEMINI_API_KEY` and `google_api_key` env var names (backward compatibility).

---

## Rate Limits

Built-in rate limiting prevents API abuse:

| Endpoint | Limit |
|---|---|
| Topic Generation | 20 requests/min |
| Script Generation | 15 requests/min |
| Audio Generation | 5 requests/min |

To adjust, edit the values in `src/services/rate_limiter.rs` and rebuild.

---

## Audio Storage

Generated audio files are written to `E:\vmq_data\audio\` as WAV files.
- A background task cleans up files older than **24 hours** automatically
- Maximum **10 concurrent** audio generation jobs
- Audio files are typically 5-30 MB each

To change the storage path, edit `audio_dir()` in `src/services/audio_job_manager.rs`.

---

## Running as a Windows Service (NSSM)

To keep the app running across reboots and auto-restart on crash:

### 1. Download NSSM
Download from https://nssm.cc/download and extract to a folder (e.g., `C:\tools\nssm\`).

### 2. Build a Release Binary
```powershell
dx build --platform web --release
```

### 3. Install as Service
Open **PowerShell as Administrator**:
```powershell
# Install the service
C:\tools\nssm\win64\nssm.exe install "VMQ-MVP" "E:\vmq_mvp 01-01-2026\target\release\vmq_mvp.exe"

# Set the working directory (important for .secrets/ to be found)
C:\tools\nssm\win64\nssm.exe set "VMQ-MVP" AppDirectory "E:\vmq_mvp 01-01-2026"

# Set environment variables
C:\tools\nssm\win64\nssm.exe set "VMQ-MVP" AppEnvironmentExtra "IP=0.0.0.0" "PORT=80"

# Configure auto-restart on failure
C:\tools\nssm\win64\nssm.exe set "VMQ-MVP" AppRestartDelay 5000

# Set up log files
C:\tools\nssm\win64\nssm.exe set "VMQ-MVP" AppStdout "E:\vmq_data\logs\vmq_stdout.log"
C:\tools\nssm\win64\nssm.exe set "VMQ-MVP" AppStderr "E:\vmq_data\logs\vmq_stderr.log"
C:\tools\nssm\win64\nssm.exe set "VMQ-MVP" AppRotateFiles 1
C:\tools\nssm\win64\nssm.exe set "VMQ-MVP" AppRotateBytes 10485760

# Create the logs directory
mkdir "E:\vmq_data\logs" -Force

# Start the service
C:\tools\nssm\win64\nssm.exe start "VMQ-MVP"
```

### 4. Manage the Service
```powershell
# Check status
C:\tools\nssm\win64\nssm.exe status "VMQ-MVP"

# Stop
C:\tools\nssm\win64\nssm.exe stop "VMQ-MVP"

# Restart
C:\tools\nssm\win64\nssm.exe restart "VMQ-MVP"

# Remove the service entirely
C:\tools\nssm\win64\nssm.exe remove "VMQ-MVP" confirm
```

---

## Emergency: API Key Compromised

1. **Revoke the key immediately** in Google Cloud Console
2. Generate a new key
3. Update `.secrets/api_key.env`
4. Restart the app (or NSSM service)
5. Check billing for unexpected charges

---

## Monitoring

- **Logs**: If using NSSM, check `E:\vmq_data\logs\vmq_stdout.log` and `vmq_stderr.log`
- **Audio files**: Check `E:\vmq_data\audio\` for generated files
- **Google Cloud Console**: Monitor API usage and billing
- **Cloudflare Dashboard**: Monitor traffic, threats, and analytics
