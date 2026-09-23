Listening Exam Generator 0.5.0

Windows 10/11 x64: run the EXE from a writable folder.
Linux x86-64 (Ubuntu 22.04 or newer baseline): chmod +x the AppImage, then run it.
First launch creates .env and voices.json beside the package. Edit .env and replace
your_api_key_here with your Gemini API key, then restart. Internet and a browser
are required for generation. No Rust, Docker or separate server installation is needed.

The browser opens after initialization. The console prints the address (default
http://127.0.0.1:8080). Keep the console open. Ctrl+C, or closing the
console window, stops the server.
Windows startup errors stay visible when double-clicked; press Enter to close.
Run from a terminal on Linux to read errors.

Options: --no-open (do not open browser), --non-interactive (no error pause),
--config-dir PATH (override configuration directory).
Relative DATA_DIR, VOICES_PATH and MUSIC_PATH values are based on that directory.
Environment variables override file values; malformed files still cause failure.
Edit configuration only while stopped; restart to reload it.

The .env contains your secret key: do not share it. Data and logs are in ./data.
Recordings expire after 24 hours. Exam drafts are not yet saved across browser closure.
Missing configuration files are recreated; invalid files are never overwritten.
The key is checked locally for format only; provider authentication errors appear
when generating. Invalid ports/paths/settings stop startup.

If Linux FUSE is unavailable:
APPIMAGE_EXTRACT_AND_RUN=1 ./listening-exam-generator-0.5.0-linux-x86_64.AppImage
Ctrl+C in this mode may return shell status 130 from the AppImage runtime,
even after "Server stopped cleanly.", and may leave its temporary extraction.
Alternatively, run the AppImage with --appimage-extract once, then run
./squashfs-root/AppRun (configuration defaults to the parent of squashfs-root).

Unsigned packages may trigger OS reputation prompts. Browser/TLS trust comes from
the operating system; the Linux bundle includes a CA fallback for minimal hosts.
SHA256SUMS files verify downloaded/copied artifacts. Dependency notices are embedded.
