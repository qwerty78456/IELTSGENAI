# Deployment Guide - Cloudflare Tunnel Setup

## ⚠️ CRITICAL: API Key Security

Your Google Gemini API key is now loaded from environment variables instead of being hardcoded. **DO NOT push the old commits with hardcoded keys to GitHub.**

---

## Setup for Local Development (Desktop Build)

### 1. Ensure your `.secrets/api_key.env` file exists:

```bash
# File: .secrets/api_key.env
google_api_key="YOUR_GOOGLE_GEMINI_API_KEY_HERE"
```

### 2. Build and run locally:

```bash
# Desktop build (reads from .env file)
dx serve --platform desktop
```

---

## Setup for Cloudflare Tunnel (Public Access)

### Option A: Desktop Build (Recommended - Key stays on your machine)

**How it works:** Run the desktop app locally, tunnel only exposes the HTTP server.

```bash
# 1. Build desktop version
dx build --platform desktop --release

# 2. Run the desktop app
./target/release/vmq_mvp

# 3. In another terminal, create Cloudflare Tunnel
cloudflared tunnel --url http://localhost:8080
```

**Security:** ✅ API key stays in your `.env` file on your machine
**Cost Protection:** ✅ Rate limited (5 audio/min, 15 scripts/min, 20 topics/min)

---

### Option B: Web Build (Key embedded in WASM - Less secure)

⚠️ **WARNING:** The API key will be embedded in the WASM binary and can be extracted.

```bash
# 1. Set API key as environment variable
export GOOGLE_API_KEY="YOUR_API_KEY_HERE"

# 2. Build WASM with embedded key
dx build --platform web --release

# 3. Serve the built files
cd dist
python -m http.server 8080

# 4. In another terminal, create Cloudflare Tunnel
cloudflared tunnel --url http://localhost:8080
```

**Security:** ⚠️ API key is in the WASM file (extractable)
**Cost Protection:** ✅ Rate limited

---

## Rate Limits (Prevents API Abuse)

The app now has built-in rate limiting:

- **Topic Generation:** 20 requests/minute
- **Script Generation:** 15 requests/minute
- **Audio Generation:** 5 requests/minute (most expensive)

Users will see "Rate limit exceeded" messages if they spam requests.

---

## Input Validation

Topics now require:
- Minimum 10 characters
- Maximum 500 characters
- Must contain actual text (not just numbers/symbols)

This prevents API abuse and prompt injection attacks.

---

## Monitoring Costs

1. Check your Google Cloud Console regularly
2. Set up billing alerts in GCP
3. Monitor the logs for suspicious activity:
   - Rapid-fire requests (should be blocked by rate limiter)
   - Unusual topics (potential abuse)

---

## Emergency: API Key Compromised

If your API key gets exposed:

1. **Revoke the key immediately** in Google Cloud Console
2. Generate a new key
3. Update `.secrets/api_key.env` with new key
4. Restart your application
5. Check billing for unexpected charges

---

## Cloudflare Tunnel Setup (First Time)

```bash
# Install cloudflared
# Windows: https://github.com/cloudflare/cloudflared/releases
# Mac: brew install cloudflare/cloudflare/cloudflared
# Linux: Check Cloudflare docs

# Login to Cloudflare
cloudflared tunnel login

# Create a tunnel
cloudflared tunnel create vmq-mvp

# Run the tunnel (temporary)
cloudflared tunnel --url http://localhost:8080

# OR create a config for permanent tunnel
cloudflared tunnel route dns vmq-mvp vmq.yourdomain.com
cloudflared tunnel run vmq-mvp
```

---

## Recommended Setup

For maximum security:

1. Use **Desktop build** with local `.env` file
2. Run desktop app: `./target/release/vmq_mvp`
3. Tunnel to localhost: `cloudflared tunnel --url http://localhost:8080`
4. Monitor costs daily for first week
5. Adjust rate limits if needed (in `src/services/rate_limiter.rs`)

---

## If Costs Get High

Adjust rate limits lower:

```rust
// In src/services/rate_limiter.rs
let _ = TOPIC_LIMITER.set(RateLimiter::new(10));   // Was 20
let _ = SCRIPT_LIMITER.set(RateLimiter::new(5));    // Was 15
let _ = AUDIO_LIMITER.set(RateLimiter::new(2));     // Was 5
```

Then rebuild and restart.

---

## What Got Fixed

✅ Removed hardcoded API keys from source code
✅ API key now loaded from environment variables
✅ Added rate limiting (5-20 requests/min per endpoint)
✅ Added input validation (10-500 characters, must contain text)
✅ Improved error messages
✅ Code compiles without errors

Still vulnerable (if using web build):
⚠️ API key embedded in WASM binary (use desktop build to avoid this)

Not fixed (would require backend):
❌ Request timeouts
❌ Cancel button actually aborting requests
❌ True API key security (need backend proxy)
