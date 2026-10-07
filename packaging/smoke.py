"""Exercise the actual portable artifact without Gemini calls or real credentials."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import signal
import socket
import sqlite3
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import wave

parser = argparse.ArgumentParser()
parser.add_argument("artifact", type=Path)
args = parser.parse_args()
artifact = args.artifact.resolve()
env = {k: v for k, v in os.environ.items() if k not in {
    "GEMINI_API_KEY", "IP", "PORT", "PUBLIC_PORT", "PUBLIC_HOST", "DATA_DIR", "VOICES_PATH", "MUSIC_PATH",
    "GEMINI_TEXT_MODEL", "GEMINI_TTS_MODEL", "GEMINI_SUMMARY_MODEL", "GEMINI_THINKING_LEVEL",
    "EXAM_BUDGET_USD", "SPEECH_CACHE_HOURS", "AUDIO_RETENTION_HOURS", "RUST_LOG",
    "DIOXUS_PUBLIC_PATH", "APPIMAGE",
}}
env["APPIMAGE_EXTRACT_AND_RUN"] = "1"
env["NO_COLOR"] = "1"
# The Windows registry environment cannot be scrubbed and overrides .env; a
# blank process value overrides both and means no published port (and no
# tunnel hostname).
env["PUBLIC_PORT"] = ""
env["PUBLIC_HOST"] = ""
# The tunnel hostname of the published-port run below.
PUBLIC_HOST = "exams.example.org"
# What the server answers and expects (src/infrastructure/instance, jobs).
APP_ID = "listening-exam-generator"
STOP_HEADER = "x-listening-exam-generator-stop"
INTERRUPTED = "The server stopped before this recording was finished. Make the recording again."
def free_port(*taken):
    """A loopback port nothing listens on, other than `taken`."""
    while True:
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        if port not in taken:
            return port
def fetch(request):
    """Status, headers and body of a request, error statuses included."""
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            return response.status, response.headers, response.read()
    except urllib.error.HTTPError as error:
        with error:
            return error.code, error.headers, error.read()
def windows_key_persisted():
    """Whether the Windows user or machine environment holds GEMINI_API_KEY (value not read out)."""
    if os.name != "nt":
        return False
    import winreg
    scopes = [(winreg.HKEY_CURRENT_USER, "Environment"),
              (winreg.HKEY_LOCAL_MACHINE, r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment")]
    for root, path in scopes:
        try:
            with winreg.OpenKey(root, path) as key:
                if str(winreg.QueryValueEx(key, "GEMINI_API_KEY")[0]).strip():
                    return True
        except OSError:
            pass
    return False
def payloads():
    """Windows launcher extraction directories; every run must remove its own."""
    return set(Path(tempfile.gettempdir()).glob("listening-generator-*")) if os.name == "nt" else set()
existing_payloads = payloads()

with tempfile.TemporaryDirectory(prefix="IELTS portable résumé ") as temporary:
    base = Path(temporary)
    appdir = base / "app"
    appdir.mkdir()
    executable = appdir / artifact.name
    shutil.copy2(artifact, executable)
    executable.chmod(0o755)
    command = [str(executable), "--no-open", "--non-interactive"]
    def failure(expected, extra=None, arguments=()):
        result = subprocess.run(command + list(arguments), cwd=base, env=env | (extra or {}), capture_output=True, timeout=60)
        output = (result.stdout + result.stderr).decode("utf-8", errors="replace")
        assert result.returncode == 1, (result.returncode, output)
        assert expected in output, output
        assert "never-print-this-secret" not in output, output
        return output
    logpath = base / "server.log"
    def start(extra=None, open_browser=False):
        output = logpath.open("wb")
        options = {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP} if os.name == "nt" else {"start_new_session": True}
        launch = [arg for arg in command if not (open_browser and arg == "--no-open")]
        process = subprocess.Popen(launch, cwd=base, env=env | (extra or {}), stdout=output, stderr=subprocess.STDOUT, **options)
        output.close()
        url = f"http://127.0.0.1:{port}"
        for _ in range(300):
            if process.poll() is not None:
                raise AssertionError(logpath.read_text(errors="replace"))
            try:
                with urllib.request.urlopen(url, timeout=1) as response:
                    return process, url, response.read().decode()
            except (OSError, TimeoutError):
                time.sleep(0.1)
        process.kill()
        raise AssertionError("Server did not become ready")
    def stop_request(headers):
        return urllib.request.Request(f"http://127.0.0.1:{port}/instance/stop", data=b"", headers=headers, method="POST")
    def stop(process, terminal_interrupt=False, token=None):
        if token is not None:
            # How another copy of the app stops this one, with the token from data/instance.json.
            status, _, body = fetch(stop_request({STOP_HEADER: token}))
            assert status == 202, (status, body)
        elif os.name == "nt":
            process.send_signal(signal.CTRL_BREAK_EVENT)
        else:
            if terminal_interrupt:
                os.killpg(process.pid, signal.SIGINT)
            else:
                # In extraction mode the upstream runtime supervises the server.
                # Signalling its child lets it relay exit 0 and clean the payload.
                children = Path(f"/proc/{process.pid}/task/{process.pid}/children").read_text().split()
                assert len(children) == 1, children
                os.kill(int(children[0]), signal.SIGINT)
        process.wait(timeout=20)
        expected_codes = (0, -signal.SIGINT, 128 + signal.SIGINT) if terminal_interrupt and os.name != "nt" else (0,)
        assert process.returncode in expected_codes, (process.returncode, logpath.read_text(errors="replace"))
        for _ in range(100):
            if "Server stopped cleanly." in logpath.read_text(errors="replace"):
                with socket.socket() as probe:
                    if probe.connect_ex(("127.0.0.1", port)) != 0:
                        break
            time.sleep(0.1)
        else:
            raise AssertionError("Server did not finish graceful shutdown")
        if terminal_interrupt and os.name != "nt":
            print(f"PASS: Ctrl+C stops the server; upstream extraction supervisor exit: {process.returncode} (shell 130 for SIGINT).")

    # The first run writes both templates before validating anything. Without a
    # key it would start and ask in the browser, so an invalid port stops it here.
    failure("PORT", {"PORT": "0"})
    config = appdir / ".env"
    voices = appdir / "voices.json"
    assert config.is_file() and voices.is_file()
    assert json.loads(voices.read_bytes())["version"] == 2
    original_voices = voices.read_bytes()
    config.write_bytes(b"GEMINI_API_KEY=never-print-this-secret\xff")
    failure("UTF-8")
    config.write_text('GEMINI_API_KEY="never-print-this-secret', encoding="utf-8")
    failure("dotenv", {"GEMINI_API_KEY": "offline-test-override"})
    port = free_port()
    public_port = free_port(port)
    valid = f"GEMINI_API_KEY=offline-test-key\nIP=127.0.0.1\nPORT={port}\n"
    config.write_text(valid, encoding="utf-8")
    for setting, value in [("PORT", "70000"), ("IP", "invalid"), ("RUST_LOG", "bad["), ("AUDIO_RETENTION_HOURS", "abc")]:
        failure(setting, {setting: value})
    # A published port needs the tunnel hostname it serves.
    failure("PUBLIC_HOST must name the Cloudflare Tunnel hostname", {"PUBLIC_PORT": str(public_port)})
    voices.write_text("{broken", encoding="utf-8")
    failure("voices.json")
    assert voices.read_text() == "{broken"
    # A 0.7 voices.json left at its defaults is renamed and replaced by the
    # version 2 template; a customised one is kept as it is (checked below).
    shipped_0_7 = (b'{"male": {"british": "Puck", "american": "Orus", "australian": "Fenrir", "canadian": "Puck", '
                   b'"newzealand": "Fenrir", "default": "Puck"}, "female": {"british": "Zephyr", "american": "Leda", '
                   b'"australian": "Aoede", "canadian": "Zephyr", "newzealand": "Aoede", "default": "Zephyr"}, '
                   b'"announcer": "Charon"}')
    customised_0_7 = shipped_0_7.replace(b'"Leda"', b'"Kore"')
    retired = appdir / "voices.0.7.json"
    voices.write_bytes(shipped_0_7)
    failure("PORT", {"PORT": "0"})
    assert retired.read_bytes() == shipped_0_7
    assert json.loads(voices.read_bytes())["version"] == 2
    retired.unlink()
    voices.write_bytes(original_voices)
    voices.unlink()
    failure("PORT", {"PORT": "0"})
    assert config.read_text() == valid and voices.is_file()
    original_voices = voices.read_bytes()
    config.unlink()
    failure("PORT", {"PORT": "0"})
    assert config.is_file() and voices.read_bytes() == original_voices
    config.write_text(valid, encoding="utf-8")
    if os.name != "nt" and os.geteuid() != 0:
        config.chmod(0)
        try:
            failure("Cannot read")
        finally:
            config.chmod(0o600)
        assert config.read_text() == valid
    blocker = appdir / "blocked"
    blocker.write_text("do not overwrite")
    failure("Cannot create", {"DATA_DIR": "./blocked"})
    data = appdir / "data"
    data.mkdir(exist_ok=True)
    (data / "logs").write_text("blocked log directory")
    failure("Cannot create")
    (data / "logs").unlink()
    (data / "jobs.db").mkdir()
    failure("job database")
    (data / "jobs.db").rmdir()
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", port))
        sock.listen()
        failure("Cannot listen")
    # Only a copy that owns the data folder and its port opens the job
    # database, so none of the failures above created it: one clean run does,
    # for the read-only check and the fixtures below.
    process, _, _ = start()
    stop(process)
    assert (data / "jobs.db").is_file()

    if os.name != "nt" and os.geteuid() != 0:
        for path, expected in [(data / "jobs.db", "job database"), (data / "logs", "Cannot write")]:
            original_mode = path.stat().st_mode
            path.chmod(0o444 if path.is_file() else 0o555)
            try:
                failure(expected)
            finally:
                path.chmod(original_mode)

    # Install fixtures while stopped so test setup cannot race pooled SQLite reads.
    wavpath = data / "audio" / "fixture.wav"
    with wave.open(str(wavpath), "wb") as recording:
        recording.setnchannels(1)
        recording.setsampwidth(2)
        recording.setframerate(24000)
        recording.writeframes(b"\0\0" * 24000)
    with sqlite3.connect(data / "jobs.db") as db:
        db.execute("INSERT INTO jobs (id,kind,state,progress,output_path,error,created_at_secs) VALUES (?,?,?,?,?,?,?)",
                   ("fixture", "part_audio", "completed", 1, str(wavpath), None, int(time.time())))
        # Left unfinished by a server that stopped: the next start fails it.
        db.execute("INSERT INTO jobs (id,kind,state,progress,output_path,error,created_at_secs) VALUES (?,?,?,?,?,?,?)",
                   ("interrupted", "part_audio", "pending", 0, None, None, int(time.time())))
    db.close()
    # The working directory must not contribute configuration.
    (base / ".env").write_text("deliberately malformed dotenv !", encoding="utf-8")

    voices.write_bytes(customised_0_7)

    # The healthy run also serves the published port (Cloudflare Tunnel).
    process, url, html = start({"PUBLIC_PORT": str(public_port), "PUBLIC_HOST": PUBLIC_HOST})
    try:
        assert urllib.request.urlopen(url + "/exam").status == 200
        assets = {p.replace("/./", "/") for p in re.findall(r'["\'](/(?:\./)?assets/[^"\'<>\s]+)', html)}
        assert assets, html[:1000]
        for asset in list(assets):
            with urllib.request.urlopen(url + asset) as response:
                content = response.read()
                if asset.endswith(".js"):
                    # dx passes a hashed module path; the un-hashed default is unused.
                    for name in re.findall(rb'["\']([^"\']+-dxh[^"\']+\.wasm)["\']', content):
                        name = name.decode().replace("/./", "/")
                        assets.add(name if name.startswith("/") else asset.rsplit("/", 1)[0] + "/" + name.removeprefix("./"))
        assert any(asset.endswith(".wasm") for asset in assets), assets
        assert any(asset.endswith(".css") for asset in assets), assets
        for asset in assets:
            with urllib.request.urlopen(url + asset) as response:
                assert response.status == 200
                content = response.read()
                assert content
                if asset.endswith(".wasm"):
                    assert content.startswith(b"\0asm")
        request = urllib.request.Request(url + "/audio/fixture", headers={"Range": "bytes=0-43"})
        with urllib.request.urlopen(request) as response:
            assert response.status == 206
            assert response.headers.get_content_type() in ("audio/wav", "audio/x-wav")
            assert response.headers["Content-Range"] == f"bytes 0-43/{wavpath.stat().st_size}"
            assert response.read() == wavpath.read_bytes()[:44]
            # Recordings stay out of shared caches (Cloudflare), errors included.
            assert response.headers["Cache-Control"] == "private, no-cache", response.headers
        with urllib.request.urlopen(url + "/audio/fixture") as response:
            assert response.read() == wavpath.read_bytes()
        status, headers, _ = fetch(url + "/audio/no-such-recording")
        assert status == 404 and headers["Cache-Control"] == "private, no-cache", (status, headers)
        logs = logpath.read_text(errors="replace")
        assert "0.7 format" in logs, logs[-5000:]
        assert voices.read_bytes() == customised_0_7 and not retired.exists()
        route = re.search(r"POST (/\S*audio_job_status\S*)", logs)
        assert route, logs[-5000:]
        request = urllib.request.Request(url + route[1], data=json.dumps({"job_id": "fixture"}).encode(),
                                         headers={"Content-Type": "application/json"}, method="POST")
        with urllib.request.urlopen(request) as response:
            assert b"fixture" in response.read()
        # A recording left pending by a stopped server reads as failed.
        request = urllib.request.Request(url + route[1], data=json.dumps({"job_id": "interrupted"}).encode(),
                                         headers={"Content-Type": "application/json"}, method="POST")
        with urllib.request.urlopen(request) as response:
            job = response.read().decode()
        assert '"Failed"' in job and INTERRUPTED in job, job
        assert "1 recording(s) interrupted by the last stop marked as failed" in logs, logs[-5000:]

        # Who runs here: answered to this computer only, never with the stop token.
        status, headers, body = fetch(url + "/instance")
        assert status == 200 and headers["Cache-Control"] == "no-store", (status, body)
        info = json.loads(body)
        stop_token = json.loads((data / "instance.json").read_text(encoding="utf-8"))["stop_token"]
        assert "stop_token" not in info and stop_token.encode() not in body, info
        assert info["app"] == APP_ID and info["run"]["kind"] == "console", info
        assert info["address"] == f"127.0.0.1:{port}" and info["public_port"] == public_port, info
        # The server's own process, which another copy may stop: not the launcher
        # (Windows) or the AppImage runtime (Linux) smoke started.
        assert isinstance(info["pid"], int) and info["pid"] != process.pid, (info, process.pid)
        assert os.path.samefile(info["data_dir"], data), info
        for headers in [{"CF-Connecting-IP": "203.0.113.7"}, {"Host": "evil.example"}]:
            status, _, body = fetch(urllib.request.Request(url + "/instance", headers=headers))
            assert status == 404 and b"pid" not in body, (headers, status, body)

        # The published port serves the app, but never as this computer.
        published = f"http://127.0.0.1:{public_port}"
        assert f"Published port (Cloudflare Tunnel): {published}" in logs, logs[-5000:]
        assert fetch(published)[0] == 200
        assert fetch(published + "/instance")[0] == 404
        through_tunnel = urllib.request.Request(published + "/instance", headers={"Host": PUBLIC_HOST})
        assert fetch(through_tunnel)[0] == 404
        assert fetch(urllib.request.Request(published, headers={"Host": PUBLIC_HOST}))[0] == 200

        # A second copy is refused, naming the first, which keeps serving.
        # Linux: the AppImage runtime extracts every run of one image to the
        # same directory under TMPDIR and deletes it when that run ends, which
        # would pull the first copy's files away and fail its exit; a copy
        # running beside it gets a directory of its own and keeps it.
        beside = {}
        if os.name != "nt":
            beside = {"TMPDIR": str(base / "second copy extraction"), "NO_CLEANUP": "1"}
            Path(beside["TMPDIR"]).mkdir()
        # Another configuration folder on the same port...
        second = base / "second copy"
        second.mkdir()
        (second / ".env").write_text(valid, encoding="utf-8")
        output = failure("Cannot listen", beside, arguments=["--config-dir", str(second)])
        assert "already running" in output and f"PID {info['pid']}" in output, output
        # ...and the same data folder on another port.
        output = failure("data folder", beside | {"PORT": str(free_port(port, public_port))})
        assert f"(PID {info['pid']}) is already using the data folder" in output, output
        wasm = next(asset for asset in assets if asset.endswith(".wasm"))
        assert fetch(url)[0] == 200 and fetch(url + wasm)[0] == 200

        # Only another copy with the token stops it: not a web page (Origin), not without the token.
        assert fetch(stop_request({}))[0] == 403
        assert fetch(stop_request({STOP_HEADER: stop_token, "Origin": url}))[0] == 403
        assert fetch(url)[0] == 200
        stop(process, token=stop_token)
        assert "Stop requested by another copy of the app" in logpath.read_text(errors="replace")
        with socket.socket() as probe:
            assert probe.connect_ex(("127.0.0.1", public_port)) != 0, "Published port still open"
    finally:
        if process.poll() is None:
            stop(process)
    voices.write_bytes(original_voices)

    # Without a key anywhere the server still starts and asks for one in the
    # browser. On Windows the key may also come from the registry, which this
    # test must not change.
    if windows_key_persisted():
        print("NOT TESTED: starting without a key; GEMINI_API_KEY is set in the Windows environment.")
    else:
        config.write_text(f"IP=127.0.0.1\nPORT={port}\n", encoding="utf-8")
        process, url, _ = start({"GEMINI_API_KEY": ""})
        try:
            logs = logpath.read_text(errors="replace")
            assert "GEMINI_API_KEY): missing" in logs, logs
            assert urllib.request.urlopen(url + "/exam").status == 200
        finally:
            stop(process)
        config.write_text(valid, encoding="utf-8")
        print("PASS: without a key the server starts and asks for one in the browser.")

    # Linux: make desktop helpers unavailable without changing OS associations.
    # Windows' system command resolver cannot be safely disabled for this test.
    if os.name != "nt":
        helpers = base / "helpers"
        helpers.mkdir()
        for helper in ("dirname", "printenv"):
            (helpers / helper).symlink_to(shutil.which(helper))
        process, url, _ = start({"PATH": str(helpers)}, open_browser=True)
        try:
            for _ in range(100):
                if "Could not open the browser" in logpath.read_text(errors="replace"):
                    break
                time.sleep(0.1)
            logs = logpath.read_text(errors="replace")
            assert "Could not open the browser" in logs and url in logs, logs
            assert urllib.request.urlopen(url + "/exam").status == 200
        finally:
            stop(process)
        print("PASS: browser-opening failure leaves the server running (Linux).")

    # Windows: closing the console window is how most people stop a double-clicked
    # app. The server must stop and the launcher must still remove its payload.
    if os.name == "nt":
        import ctypes
        from ctypes import wintypes
        user32 = ctypes.WinDLL("user32")
        before = payloads()
        process = subprocess.Popen(command, cwd=base, env=env, creationflags=subprocess.CREATE_NEW_CONSOLE)
        try:
            for _ in range(300):
                assert process.poll() is None, process.returncode
                try:
                    urllib.request.urlopen(f"http://127.0.0.1:{port}", timeout=1).close()
                    break
                except OSError:
                    time.sleep(0.1)
            windows = []
            @ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
            def visit(hwnd, _):
                owner = wintypes.DWORD()
                user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
                name = ctypes.create_unicode_buffer(64)
                user32.GetClassNameW(hwnd, name, 64)
                if owner.value == process.pid and name.value == "ConsoleWindowClass":
                    windows.append(hwnd)
                return True
            user32.EnumWindows(visit, 0)
            if windows:
                user32.PostMessageW(windows[0], 0x0010, 0, 0)  # WM_CLOSE, like the X button
                process.wait(timeout=20)
                for _ in range(50):
                    with socket.socket() as probe:
                        if probe.connect_ex(("127.0.0.1", port)) != 0:
                            break
                    time.sleep(0.1)
                else:
                    raise AssertionError("Server still listening after the console window closed")
                assert not (payloads() - before), payloads() - before
                print("PASS: closing the console window stops the server and removes the payload (Windows).")
            else:
                print("NOT TESTED: console window close; the new console is not a classic conhost window.")
        finally:
            if process.poll() is None:
                subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"], capture_output=True)
                process.wait()
    moved = base / "moved app"
    appdir.rename(moved)
    command[0] = str(moved / artifact.name)
    process, _, _ = start()
    stop(process, terminal_interrupt=True)
    # A relative --config-dir is relative to the launch directory.
    command += ["--config-dir", "custom configuration"]
    failure("PORT", {"PORT": "0"})
    assert (base / "custom configuration" / ".env").is_file()
    assert (base / "custom configuration" / "voices.json").is_file()
    assert not (payloads() - existing_payloads), payloads() - existing_payloads
    print("PASS: first run, malformed config, preservation, 0.7 voices.json (renamed or kept), paths,")
    print("      logs/database failures, port conflict,")
    print("      pages/assets/WASM/CSS, server function, audio range/download and caching,")
    print("      interrupted recordings, /instance (local only), published port, second copies refused,")
    print("      stop by another copy, successful exit codes, shutdown, relocation and explicit configuration directory.")
