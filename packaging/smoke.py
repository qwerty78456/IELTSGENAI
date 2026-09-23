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
import urllib.request
import wave

parser = argparse.ArgumentParser()
parser.add_argument("artifact", type=Path)
args = parser.parse_args()
artifact = args.artifact.resolve()
env = {k: v for k, v in os.environ.items() if k not in {
    "GEMINI_API_KEY", "IP", "PORT", "DATA_DIR", "VOICES_PATH", "MUSIC_PATH",
    "GEMINI_TEXT_MODEL", "GEMINI_TTS_MODEL", "RUST_LOG", "DIOXUS_PUBLIC_PATH", "APPIMAGE",
}}
env["APPIMAGE_EXTRACT_AND_RUN"] = "1"
env["NO_COLOR"] = "1"

with tempfile.TemporaryDirectory(prefix="IELTS portable résumé ") as temporary:
    base = Path(temporary)
    appdir = base / "app"
    appdir.mkdir()
    executable = appdir / artifact.name
    shutil.copy2(artifact, executable)
    executable.chmod(0o755)
    command = [str(executable), "--no-open", "--non-interactive"]
    def failure(expected, extra=None):
        result = subprocess.run(command, cwd=base, env=env | (extra or {}), capture_output=True, timeout=60)
        output = (result.stdout + result.stderr).decode("utf-8", errors="replace")
        assert result.returncode == 1, (result.returncode, output)
        assert expected in output, output
        assert "never-print-this-secret" not in output, output
        return output

    failure("GEMINI_API_KEY")
    config = appdir / ".env"
    voices = appdir / "voices.json"
    assert config.is_file() and voices.is_file()
    original_voices = voices.read_bytes()
    config.write_bytes(b"GEMINI_API_KEY=never-print-this-secret\xff")
    failure("UTF-8")
    config.write_text('GEMINI_API_KEY="never-print-this-secret', encoding="utf-8")
    failure("dotenv", {"GEMINI_API_KEY": "offline-test-override"})
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    valid = f"GEMINI_API_KEY=offline-test-key\nIP=127.0.0.1\nPORT={port}\n"
    config.write_text(valid, encoding="utf-8")
    for setting, value in [("PORT", "70000"), ("IP", "invalid"), ("RUST_LOG", "bad[")]:
        failure(setting, {setting: value})
    voices.write_text("{broken", encoding="utf-8")
    failure("voices.json")
    assert voices.read_text() == "{broken"
    voices.write_bytes(original_voices)
    voices.unlink()
    failure("PORT", {"PORT": "0"})
    assert config.read_text() == valid and voices.is_file()
    original_voices = voices.read_bytes()
    config.unlink()
    failure("GEMINI_API_KEY")
    assert voices.read_bytes() == original_voices
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
    db.close()
    # The working directory must not contribute configuration.
    (base / ".env").write_text("deliberately malformed dotenv !", encoding="utf-8")

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
    def stop(process, terminal_interrupt=False):
        if os.name == "nt":
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

    process, url, html = start()
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
        with urllib.request.urlopen(url + "/audio/fixture") as response:
            assert response.read() == wavpath.read_bytes()
        logs = logpath.read_text(errors="replace")
        route = re.search(r"POST (/\S*audio_job_status\S*)", logs)
        assert route, logs[-5000:]
        request = urllib.request.Request(url + route[1], data=json.dumps({"job_id": "fixture"}).encode(),
                                         headers={"Content-Type": "application/json"}, method="POST")
        with urllib.request.urlopen(request) as response:
            assert b"fixture" in response.read()
    finally:
        stop(process)

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
    moved = base / "moved app"
    appdir.rename(moved)
    command[0] = str(moved / artifact.name)
    process, _, _ = start()
    stop(process, terminal_interrupt=True)
    # A relative --config-dir is relative to the launch directory.
    command += ["--config-dir", "custom configuration"]
    failure("GEMINI_API_KEY")
    assert (base / "custom configuration" / ".env").is_file()
    assert (base / "custom configuration" / "voices.json").is_file()
    print("PASS: first run, malformed config, preservation, paths, logs/database failures, port conflict,")
    print("      pages/assets/WASM/CSS, server function, audio range/download,")
    print("      successful exit codes, shutdown, relocation and explicit configuration directory.")
