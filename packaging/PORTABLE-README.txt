Listening Exam Generator 0.8.0

Windows 10/11 x64: run the EXE from a writable folder.
Linux x86-64 (Ubuntu 22.04 or newer baseline): chmod +x the AppImage, then run it.
First launch creates .env and voices.json beside the package and opens the browser.
Internet and a browser are required for generation. No Rust, Docker or separate
server installation is needed.

Gemini API key, first match wins:
  1. the GEMINI_API_KEY environment variable (on Windows this includes a user or
     machine variable set after the console was opened);
  2. GEMINI_API_KEY in .env (replace your_api_key_here);
  3. otherwise every page shows a form to paste the key.
The console says where the key came from, never the key. Read the warning above
the form: the key travels over plain HTTP, the app has no login, and "Remember"
writes it unencrypted to .env. The form works only when the app listens on
127.0.0.1 and checks the key with Google (free) before keeping it. It never
replaces a working key from the environment or .env. If Google rejects that key,
every page says where the key came from and offers the form; a key pasted there
is used until the app restarts. "Remember" is offered only when .env would win
after a restart, i.e. not when the bad key is an environment variable.

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

Cost: text uses gemini-3.8-flash, speech gemini-3.8-flash-tts (paid tier). A full
IELTS exam measured about 0.36 USD (0.73 USD at the prices Google announced from
2027-01-01, above the default budget). The Whole exam page shows each exam's
spend against EXAM_BUDGET_USD (0.70 by default; it only warns).
GEMINI_THINKING_LEVEL (low by default) and SPEECH_CACHE_HOURS (72: speech already
made for the same words and voices is reused for free) are in .env too. Speech
made by 0.7 is not reused: rendering an old exam again pays once.

Voices: every speaker gets a regional voice of its own (British, American,
Australian, Canadian, New Zealand, Irish, Scottish, South African or Indian
English). Listen plays a short sample: the first listen of a voice costs about
0.005 USD, later ones are free. Another voice and Automatic change it, on both
pages. Voices designed from a description belong to the Google project of the
API key (about 0.01 USD each): keys of other projects cannot use them, and they
can be created or deleted only while the app listens on 127.0.0.1 (the default).
voices.json (version 2 since 0.8.0) only overrides the built-in voices. A 0.7
voices.json that still held the 0.7 defaults is renamed voices.0.7.json and a
new template is written. A customised 0.7 file is kept but ignored, and the
console says so: rewrite it in the version 2 format or delete it.

The .env may contain your secret key: do not share it. Data and logs are in ./data.
Exams made on the Whole exam page are saved beside the package and reopen after a
restart, recording included; their recordings stay until the exam is deleted. Other
recordings expire after AUDIO_RETENTION_HOURS (24 by default; 0 keeps them all).
Both pages export Word (DOCX) and Markdown.
Missing configuration files are recreated; invalid files are never overwritten.
Invalid ports/paths/settings stop startup; a missing key does not.

If Linux FUSE is unavailable:
APPIMAGE_EXTRACT_AND_RUN=1 ./listening-exam-generator-0.8.0-linux-x86_64.AppImage
Ctrl+C in this mode may return shell status 130 from the AppImage runtime,
even after "Server stopped cleanly.", and may leave its temporary extraction.
Alternatively, run the AppImage with --appimage-extract once, then run
./squashfs-root/AppRun (configuration defaults to the parent of squashfs-root).

Unsigned packages may trigger OS reputation prompts. Browser/TLS trust comes from
the operating system; the Linux bundle includes a CA fallback for minimal hosts.
SHA256SUMS files verify downloaded/copied artifacts. Dependency notices are embedded.
