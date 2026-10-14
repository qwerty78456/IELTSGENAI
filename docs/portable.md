# Portable releases

The application remains a Dioxus 0.7 fullstack browser/server app. Packaging
adds a small distribution launcher, not another application backend.

## Configuration and lifecycle

Portable mode creates .env, voices.json, and data/ beside the distributed
EXE/AppImage. The Windows launcher supplies its original directory explicitly,
and AppRun derives it from APPIMAGE, not the read-only mount or current directory.
An explicit --config-dir PATH overrides that location.

Missing templates are created exclusively, with mode 0600 on Unix. Existing
files are preserved. Permission failures, malformed dotenv/JSON, a voices.json
with another version, an unknown accent or an invalid voice id, invalid
IP/port/model/log settings, a TEXT_PROVIDER other than gemini or anthropic,
invalid optional music, and unwritable logs/database cause startup failure.
The Anthropic settings (ANTHROPIC_API_KEY, ANTHROPIC_TEXT_MODEL,
ANTHROPIC_SUMMARY_MODEL, ANTHROPIC_EFFORT) stop startup only with
TEXT_PROVIDER=anthropic; with gemini a malformed one is a "Note:" line and
Claude cannot write text until it is fixed and the app restarts. A blank value
means the default. Error messages omit dotenv contents and API-key values.
Environment variables override file values (on Windows the user and machine
environment is read from the registry too, so a variable set after the console
opened counts); syntax errors are still rejected. A missing or placeholder
Gemini key does not stop startup. A run with a
real console on stdin and stdout, without --non-interactive or --service and
outside Windows session 0, first asks for it once the port and data folder are
its own: "Paste your Gemini API key (shown as *), or press Enter to enter it in
the browser instead". On a network bind (IP=0.0.0.0 or a LAN address) no
browser may enter a key, so the prompt ends "or press Enter to skip" and the
fallback lines say to set GEMINI_API_KEY and restart. Each character shows as
*, the key is checked with one free Google request and written to .env
(readable by its owner only: 0600 on Unix, and on Windows a protected
owner/SYSTEM/Administrators DACL on an NTFS drive; a FAT or exFAT drive, such as
most USB sticks, keeps no permissions, so the file stays readable by every
account and the console and log say so), at most three tries.
A key Google cannot check (busy, offline) is saved only after a "[y/N]". Ctrl+C
at the prompt ends the program with the console restored, so the launcher's
"Press Enter to close." still works. Enter alone, a non-interactive run, or
three unusable keys leave the server to start, log "GEMINI_API_KEY): missing"
and let the page ask for a key, which it accepts only from a browser on the
server's own computer (a Local request: loopback bind, main port, no proxy).

The Anthropic key (ANTHROPIC_API_KEY) is found the same way and needed only
for text written by Claude. With TEXT_PROVIDER=anthropic and no Anthropic key,
the same console asks for it too, after any Gemini prompt: "Paste your
Anthropic API key (shown as *), or press Enter to enter it in the browser
instead" (on a network bind "or press Enter to skip", and the fallback lines
say to set ANTHROPIC_API_KEY and restart, or set TEXT_PROVIDER=gemini). The
key is checked with one free Anthropic request (GET /v1/models?limit=1) and
written to .env as ANTHROPIC_API_KEY under the same rules. With the default
TEXT_PROVIDER=gemini the console never asks for it. After the Gemini key line,
startup prints "Text provider (TEXT_PROVIDER): gemini" (or anthropic) and
"Anthropic API key (ANTHROPIC_API_KEY): ...": where the key came from, "missing;
..." with TEXT_PROVIDER=anthropic, or "not set; needed only when a page
chooses Claude to write the text". Startup makes no paid API requests; the
console key checks are the only requests (to Google, and to Anthropic only
with TEXT_PROVIDER=anthropic and no Anthropic key), and they are free.

Since 0.8.0 voices.json is version 2 and holds overrides only: the voice pools
per accent and gender (British, American, Australian, Canadian, New Zealand,
Irish, Scottish, South African, Indian English) and the announcer are compiled
in, a non-empty list in the file replaces one pool, and the first launch writes
an empty template. A 0.7 file (no "version") is still parsed strictly, so a
broken one stops startup, and is then ignored: left at the 0.7 defaults it is
renamed voices.0.7.json and the version 2 template is written; customised, it
is kept byte for byte and the console prints a "0.7 format" notice. A pool
below its minimum (4 British voices per gender, 3 for the other core accents,
2 for the accents added in 0.8) is a notice, not a failure. Voice samples
("Listen") are kept under data/audio/voices and purged after 30 days unused.
Designed voices belong to the Google project of the API key. Creating one is
allowed in a browser on the server's own computer (127.0.0.1 or ::1, which
portable mode uses by default) and through PUBLIC_PORT (Cloudflare Tunnel,
for requests whose Host names a PUBLIC_HOST; PUBLIC_HOST is required with
PUBLIC_PORT); deleting one only in a browser on the server's own computer.

Development and Docker retain their current-working-directory configuration
and data conventions; no parent-directory dotenv search is performed. Docker
may supply configuration entirely through environment variables. They do not
open browsers. Debug launches retain Dioxus hot reload. All modes now require
valid startup configuration.

Portable production startup initializes logging, takes the data-folder lock
(data/instance.lock), binds the listeners (PORT, and PUBLIC_PORT when set),
asks for missing keys, opens SQLite, marks recordings an earlier stop
interrupted as failed, and writes data/instance.json before opening the
browser. Missing browser launchers leave the URL visible and the server
running. An occupied port or data folder never selects another one. When
another copy of this app holds it on a loopback address, the error names that
copy (version, process id, address); at an interactive console a "[y/N]"
prompt (Enter = no) offers to stop it and start this copy instead. No opens
the browser at the running copy and exits 0; yes stops a console copy (a
clean stop request, then termination after 10 s) or, on Windows, a service
copy, which a hidden helper starts again once this copy exits while the user
stays signed in; signing out ends the helper too, and the service (Automatic
start) then runs again only from the next boot. Nothing is stopped or ended
unless the system shows the process that answered listening on that address.
Anything else on the port, any other IP, or PUBLIC_PORT being taken fails as
before ("Cannot listen on ..."). Copies of 0.8.2 and earlier take no
data-folder lock: stop such a copy before starting a newer one on the same
data folder, or a recording it is still making is shown as failed. Ctrl+C shuts down the server. Windows additionally
handles Ctrl+Break, Unix SIGTERM; open connections get 5 seconds.

--service NAME is for a service manager (NSSM): no console questions, no
browser, no "Press Ctrl+C to stop." line, and GET /instance reports the
service name so a manual copy can offer to stop that service. Scheduled tasks
and other unattended starts should pass --non-interactive (and --no-open): a
hidden console would otherwise wait for an answer no one can give. A run in
Windows session 0 (services, a task set to run whether the user is signed in
or not, OpenSSH sessions) is detected and never asks.

## Rebuilding

Use Rust 1.92.0, wasm32-unknown-unknown, Dioxus CLI 0.7.9, and Python 3.

Windows (MSVC build tools installed):

    pwsh -NoProfile -File packaging/build-windows.ps1
    python packaging/smoke.py dist/listening-exam-generator-0.9.1-windows-x64.exe

The Windows server and packaging launcher statically link the C runtime.
The launcher embeds only the server, public assets, and dependency notices,
extracts into a private temporary directory, passes console I/O through,
waits for the child, and cleans up. Closing the console window (and logoff or
shutdown) stops the server too; the launcher's console handler holds Windows'
close deadline until the payload is removed. Errors pause only when the
launcher owns the console, stdin is interactive, and --non-interactive was not
supplied. Forced termination (Task Manager) or power loss can still leave a
listening-generator-* directory under %TEMP%.

Linux from Windows with an existing running Podman machine:

    pwsh -NoProfile -File packaging/build-linux.ps1 -Connection ielts-portable-builder

The build image uses Ubuntu 22.04 and compiles Dioxus CLI from source because
its published Linux binary requires newer glibc. Rust and CLI versions are pinned;
appimagetool 1.9.1 and the Type 2 runtime are checked against recorded SHA-256s.
If the moving upstream runtime download changes, checksum verification stops;
review and update the pinned hash explicitly.

The script transfers an allowlist of source files, excluding .env, data,
Git metadata, and secrets. The AppImage includes non-system shared libraries,
license notices, and a CA-bundle fallback. glibc remains a host dependency
(2.35 minimum). The user's browser is not bundled.

Without FUSE, run with APPIMAGE_EXTRACT_AND_RUN=1. Extracted AppDir execution
uses its parent as the default config directory; --config-dir makes it explicit.
In automatic extraction mode, terminal Ctrl+C also interrupts the upstream
AppImage supervisor (shell exit 130). The server still shuts down gracefully;
look for "Server stopped cleanly.". That supervisor can leave its temporary
extraction directory after an interrupt. For predictable exit 0 and no temporary
extraction cleanup, use --appimage-extract once and run squashfs-root/AppRun.

## Release verification

Run cargo fmt/check, both cargo check feature combinations, the wasm check,
and all server-feature unit tests. The smoke script tests the actual package
in a temporary path with spaces/Unicode and makes no Gemini or Anthropic
request. It removes `GEMINI_API_KEY`, `TEXT_PROVIDER` and the Anthropic
settings (`ANTHROPIC_BASE_URL` and `ANTHROPIC_AUTH_TOKEN`, which the app never
reads, included) from the package's environment and sets `TEXT_PROVIDER` blank,
so Gemini writes the text whatever the registry says. On Windows the server
also reads the registry: on a machine whose user or machine environment holds a
key, the "no key anywhere" case for that key prints NOT TESTED instead of
changing the registry (the Gemini key's "missing" line, or the Anthropic key's
"not set; needed only when a page chooses Claude to write the text"). It checks
missing and malformed config (`TEXT_PROVIDER=openai` among it), environment
overrides, preservation, storage errors, occupied ports, the startup key lines
(every start prints "Anthropic API key (ANTHROPIC_API_KEY): ", and the default
start "Text provider (TEXT_PROVIDER): gemini"), both pages, JS/WASM/CSS, an
audio-job server function, WAV ranges and downloads, shutdown, and relocation.
On Windows it also closes a real console window (WM_CLOSE, like the X button)
and asserts that no launcher payload directory remains after any run.

Audit EXE imports and Linux ldd output. Verify package inventories contain only
runtime files and notices. Test AppImages on Ubuntu 22.04 and a newer distribution.
Record FUSE/graphical-browser tests separately from headless container tests.
Code signing, ARM64 and updates are outside this release. Paid live checks are
separate from packaging: `cargo test --features server --no-default-features
live_probe -- --ignored --nocapture` (about $0.01) confirms the Gemini request
shapes and measures audio tokens per second; `voice_live_probe` (same flags)
checks for free that every pooled voice still exists with its gender and
language, then spends about $0.003. `claude_paid_probe` (same flags, about
$0.03) checks the Anthropic key for free and counts the prompts' tokens, then
writes a topic, the longest IELTS script, its questions and a download-name
summary with Claude; `text_provider_compare_probe` (about $0.17) writes
one IELTS exam's text with Gemini and with Claude and compares them. Their
names keep them out of the `live_probe` filter; ask before running either. The
smoke script also checks that the first launch writes a version 2 voices.json,
that a 0.7 file at its defaults is renamed and that a customised one is kept
with the "0.7 format" notice.

Run `pwsh -NoProfile -File packaging/test-linux.ps1` to verify the built AppImage
on clean Ubuntu 22.04 and 24.04 containers. Each first tests startup before
installing Python, then runs the full smoke suite as an unprivileged user,
including permission errors and missing browser helpers. Windows smoke tests
do not alter system browser associations to force a browser-launch failure.

See [the v0.9.1 Windows verification record](portable-verification-v0.9.1.md),
[the v0.9.0 verification record](portable-verification-v0.9.0.md), [the
v0.8.2 record](portable-verification-v0.8.2.md), [the
v0.8.1 record](portable-verification-v0.8.1.md), [the
v0.8.0 record](portable-verification-v0.8.0.md), [the
v0.7.1 record](portable-verification-v0.7.1.md), [the
v0.7.0 record](portable-verification-v0.7.0.md) and
[the v0.5.0 record](portable-verification-v0.5.0.md) (with the full platform
limitations) for measured results. Build scripts pin tools and lockfiles, but
do not promise byte-for-byte identical binaries (upstream OS packages, build
timestamps and paths can vary).
