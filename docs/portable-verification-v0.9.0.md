# Portable v0.9.0 verification — 2026-10-07

Windows EXE and Linux AppImage, both from commit `f1102ef` ("release: 0.9.0").
The tag `v0.9.0` adds only this record and the CHANGELOG figures on top of it.

## What changed

Commit `452ffaa` ("feat: run behind Cloudflare Tunnel and take over a running
copy"): requests are classified per request (`infrastructure::ingress`:
Local, Published, Remote), an optional second listener for Cloudflare Tunnel
(`PUBLIC_PORT`, with the required `PUBLIC_HOST`), a data-folder lock and
`instance.json`, `GET /instance` and `POST /instance/stop`, the interactive
takeover of a running copy or Windows service, `--service NAME`, the masked
console key prompt, an owner-only `.env`, the interrupted-recording sweep and
`Cache-Control: private, no-cache` on recordings and voice samples. New
dependencies: `rpassword`, direct `windows-sys` 0.61 features (service manager,
TCP listener table, process control, security descriptors, console) and `libc`
on Unix. The build scripts did not change; `smoke.py` gained the checks listed
below. The Rust minimum is now 1.89 (`File::try_lock`).

How the work was checked before the build: the design was attacked by four
independent reviewers (security, platform, regressions, architecture); each of
the six implementation units was reviewed and fixed by a second agent; a final
review over six dimensions with three adversarial votes per finding confirmed
11 findings, all fixed in `452ffaa` (among them: the takeover now acts only on
the process that owns the listening socket; `PUBLIC_HOST` stops DNS rebinding
on the published port; the `.env` ACL names the user's SID instead of OWNER
RIGHTS, which an elevated save had turned into Administrators).

## Builds and checks

- Host: Windows 11 IoT Enterprise LTSC 10.0.26100, x64. Rust 1.92.0, Dioxus CLI
  0.7.9, committed lockfiles, `packaging/build-windows.ps1` (`--locked`).
- `cargo fmt --check` for the app and launcher: passed.
- `cargo check` (web), server-only check and wasm32 check: passed, 0 warnings.
- `cargo test --features server --no-default-features`: **271 passed**, 5
  ignored (the paid probes and `dump_fixture`). 0.8.2 had 198.
- Release WASM is still not size-optimized (the `wasm-opt` that dx 0.7.9
  downloads crashes on Windows, 0xc0000409, as since 0.5.0). Size 4.21 MB
  (4,211,914 bytes; 0.8.2: 4,210,183).

## Packaged verification (Windows)

`python packaging/smoke.py dist/listening-exam-generator-0.9.0-windows-x64.exe`
passed on the host. Besides every 0.8.2 check (first-run templates, malformed
configuration, preservation, 0.7 `voices.json`, spaces/Unicode paths, invalid
settings, blocked paths, occupied port, pages and assets, an audio-job server
function, WAV range and download, shutdown, relocation, relative
`--config-dir`, closing a real console window with no payload left), it now
covers:

- `GET /instance` from this computer (JSON, no stop token, `no-store`), and 404
  with a `CF-Connecting-IP` header or a foreign `Host`;
- `PUBLIC_PORT` with `PUBLIC_HOST`: both ports serve, `/instance` is 404 on the
  published port, and `PUBLIC_PORT` without `PUBLIC_HOST` stops startup;
- a second copy on the same port (another folder) and on the same data folder
  (another port), both refused with exit 1 while the first keeps serving;
- `POST /instance/stop`: 403 without the token and with an `Origin` header,
  then a clean stop with the token;
- `Cache-Control: private, no-cache` on `/audio` 206 and 404 answers;
- a `pending` row inserted before start that reads back as failed with the
  interrupted-recording message.

**Not tested:** starting with no key anywhere, as in every 0.7.x and 0.8.x
record: the host's user environment holds `GEMINI_API_KEY` and the smoke test
does not change the registry. The same reason keeps the masked console prompt
from appearing on this host; it was tested on Linux (below).

### Takeover on the release EXE

`verify_takeover.py` (a session script, not in the repo) ran copies of the
release EXE in hidden consoles and typed the answers through the console
input buffer, ports 18591-18592:

| Case | Result |
| --- | --- |
| Port held by another copy (other folder), answer `y` | the old copy logged "Stop requested by another copy of the app", printed "Server stopped cleanly." and its launcher exited 0; the new copy served |
| Another copy, answer Enter (N) | exit 0, the running copy untouched |
| Non-interactive copy on the same port | exit 1, "Cannot listen on 127.0.0.1:18591: Listening Exam Generator 0.9.0 is already running there (PID …)" |
| Same data folder, other port, answer `yes` | the data-folder holder stopped cleanly; the new copy served on the other port |

### Development server

Under `dx serve` (port 8200; 8080 sits in a range Windows had reserved, see
ROADMAP S8) the dev proxy adds no forwarding header: `GET /instance` answered
200 as a Local request with `run.kind = "dev"`, the page rendered with no
console error, and a `POST /instance/stop` sent by the page itself got 403.

## Linux AppImage

- Built with `packaging/build-linux.ps1` in the Ubuntu 22.04 builder image on
  the project's own `ielts-portable-builder` Podman (WSL) machine. Inside the
  container: `cargo fmt --check`, the three `cargo check`s and
  `cargo test --locked --features server --no-default-features` (**264
  passed**, 5 ignored; the Windows-only tests are not compiled there) ran before
  `dx build`. The first attempt of the day stopped at dx's "cargo metadata took
  too long" while the builder's network was slow; the retry passed unchanged.
- `listening-exam-generator-0.9.0-linux-x86_64.AppImage`: 13,412,856 bytes
  (0.8.2: 13,224,440). The server links only `libssl.so.3`, `libcrypto.so.3`,
  `libgcc_s.so.1`, `libm.so.6` and `libc.so.6` (`dist/linux-dependencies.txt`).
- `packaging/test-linux.ps1` passed on clean **Ubuntu 22.04 and 24.04**
  containers as UID 65534: first run before Python is installed, manual
  extraction with AppRun, dependency resolution, licenses and inventory, the
  full smoke suite including the new checks, **starting with no key anywhere**,
  a missing browser helper, and terminal Ctrl+C through the extraction
  supervisor (exit -2 / shell 130, as documented since 0.5.0).
- **Masked console key prompt** (`n1_pty_test.py`, a session script, on Ubuntu
  24.04 as UID 65534, the AppImage on a pseudo-terminal): 11 of 11 passed. The
  prompt appears, echo is off while it asks, each typed character shows as
  `*` and the key itself never appears, a fake key is not accepted and is not
  written to `.env`, Enter alone continues to "No key entered; the browser will
  ask for it." with the startup line still saying "missing", the server then
  stops cleanly, and Ctrl+C at the prompt exits 130 with the terminal's echo
  and signals restored.
- Not tested: native FUSE mounting (no `/dev/fuse` in the containers), a
  graphical desktop browser, and stopping a real Windows service from a manual
  copy (needs an installed service and administrator rights; planned with the
  installer, ROADMAP S6).

## Contents and dependencies

The console-subsystem x64 launcher (9,627,136 bytes; 0.8.2: 9,393,152) embeds
only `server.exe` (18,911,232 bytes; 0.8.2: 18,283,520), `public/` and
`RUST-DEPENDENCIES.txt`. Both import tables contain only Windows system DLLs:

- launcher: `kernel32`, `ntdll`, `bcryptprimitives`, `api-ms-win-core-synch-l1-2-0`
  (unchanged)
- server: `kernel32`, `ntdll`, `advapi32` (registry, service manager, security
  descriptors), `iphlpapi` (**new**: the TCP listener table that ties a running
  copy's PID to its socket), `ws2_32`, `secur32`, `crypt32`, `bcrypt`,
  `bcryptprimitives`, `api-ms-win-core-synch-l1-2-0`. Still no `user32`,
  `shell32` or `gdi32`, so console logoff and shutdown events keep reaching it.

## Paid checks

None. The packaging checks make no Gemini request; the Linux prompt test sent
only a fake key to Google's free model list.

## Artifact SHA-256

| File | SHA-256 |
| --- | --- |
| listening-exam-generator-0.9.0-windows-x64.exe | `17f2976ab032ba8bef5f081e404b665586613ef127fcb7f4bfc8ea97a775a462` |
| listening-exam-generator-0.9.0-linux-x86_64.AppImage | `66ba958d9e7156d840c94711056f31f09f3697da07b636e79e00b1496a1eff63` |

Both packages, `README.txt` and the two SHA256SUMS files are published as the
GitHub release `v0.9.0`. Code signing, ARM64 and auto-updates remain out of
scope.
