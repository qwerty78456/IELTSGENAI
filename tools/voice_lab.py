#!/usr/bin/env python3
"""Voice lab: a developer tool for Gemini 3.8 TTS voices.

Catalog dump, live probe (experiments E0-E10), pool audition, scoring of the
product owner's blind answers and F0 reports. It is not part of the app:
tools/ is outside the packaging allowlist, its spend is NOT in the app's usage
ledger (copy it into the verification record by hand), and every paid
subcommand refuses to run without --max-usd.

    python tools/voice_lab.py catalog                                   # free
    python tools/voice_lab.py probe    --max-usd 0.30 [--only E1,E2] [--design]
    python tools/voice_lab.py audition --max-usd 0.45 [--accents british,...] [--per-cell 6] [--dry-run | --offline]
    python tools/voice_lab.py score    answers.json                     # free: PO sheet-2 answers -> pools
    python tools/voice_lab.py report   (--data-dir DIR [--log FILE] | --wav F.wav --expect female) [--ear --max-usd 0.05]

Output goes to TESTING_DUMP/voice-lab/<YYYY-MM-DD>/ (gitignored) unless --out
is given. The API key is looked up like the app does it: process
GEMINI_API_KEY, then the Windows user and machine environment (registry), then
./.env. It travels only in the x-goog-api-key header and is never printed or
written. Speech bodies are built exactly like speech_body in
src/infrastructure/llm/gemini.rs and serialised like serde_json (sorted keys,
compact), so what the probe proves is what the app sends.

The --max-usd cap of probe and audition is cumulative per output folder: spend
already recorded in that folder's manifest counts against it, so re-running
a subset can never take the folder's total past the cap.
"""
from __future__ import annotations

import argparse
import base64
import datetime as dt
import html
import json
import math
import os
import random
import re
import shutil
import socket
import struct
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parent.parent
API_ROOT = "https://generativelanguage.googleapis.com/v1beta"
TTS_MODEL = "gemini-3.8-flash-tts"
LITE_MODEL = "gemini-3.8-flash-lite-tts"
EAR_MODEL = "gemini-3.8-flash"
SAMPLE_RATE = 24_000
AUDIO_TOKENS_PER_SECOND = 32
# synthesize.rs:27-30
CHUNK_GAP_MS = 350
PASSAGE_STYLE = "natural, clear pronunciation at a steady exam pace"
ANNOUNCEMENT_STYLE = "slow and clear, like an exam announcer"
# gemini.rs: MAX_RETRIES 3, backoff 1 s doubling, retry on 429/503/504/timeout.
MAX_RETRIES = 3
FIRST_BACKOFF_S = 1.0
TTS_TIMEOUT_S = 300
TEXT_TIMEOUT_S = 120
# USD per million tokens (input, cached input, output) - mirrors llm/pricing.rs.
INTRO_ENDS = 1_798_761_600  # 2027-01-01T00:00:00Z
PRICES = {
    "gemini-3.8-flash": ((0.75, 0.075, 3.75), (1.50, 0.15, 7.50)),
    "gemini-3.8-flash-tts": ((0.50, 0.125, 9.00), (1.00, 0.25, 18.00)),
    "gemini-3.8-flash-lite-tts": ((0.50, 0.125, 6.00), (1.00, 0.25, 12.00)),
}
# Audio input to the AI ear: Google lists one input price for 3.8 Flash; until
# a bill says otherwise the tool assumes a higher $1/M for audio tokens.
AUDIO_INPUT_RATE = (1.00, 2.00)
# The product owner's designed voices: read-only. Never modified or deleted.
PROTECTED_VOICES = {
    "voice_60zf03beui2x", "voice_iheskl3lyu7z", "voice_et3hwo42hxxs", "voice_kpd3e297369r",
    "voice_3wcq2e9jzz1b", "voice_3vp96n1h9k8s", "voice_t4ha07idgf29", "voice_nva3gh8uk4cg",
}
CLASSICS = {"Zephyr", "Puck", "Charon", "Kore", "Fenrir", "Leda", "Orus", "Aoede"}

# ---------------------------------------------------------------- fixed texts

D_MIXED = [  # A female receptionist, B male caller (IELTS Part 1 style), ~66 words
    ("Speaker A", "Good morning, Riverside Leisure Centre, this is Emma speaking. How can I help you today?"),
    ("Speaker B", "Hello, my name's David Clarke. I'd like to book a tennis court for Saturday morning, please."),
    ("Speaker A", "Certainly, Mr Clarke. We have a court free at ten o'clock. It's twelve pounds for an hour."),
    ("Speaker B", "That's perfect. Can I pay by card when I arrive, or do you need the money now?"),
]
D_FEMALE = [  # two women, HSG-interview host and guest, ~83 words
    ("Speaker A", "Good evening and welcome to Science Today. I'm Sarah Jones, and tonight I'm here with "
                  "Doctor Helen Ward, a marine biologist who has studied coral reefs for ten years."),
    ("Speaker B", "Thank you, Sarah, it's lovely to be here. I've loved the sea since I was a little girl."),
    ("Speaker A", "Helen, let's start with the basics. Why are coral reefs so important to people living on the coast?"),
    ("Speaker B", "Well, they protect the shoreline from storms, and they feed the fish that many local families depend on."),
]
ACCENT_TEXT = ("Right, I'd better check the car park first. Last year we paid for parking on Tuesday, "
               "but the new schedule says the castle tour starts after lunch, so we can't be late. "
               "Water and a map are included.")
ANNOUNCE_TEXT = "Part one. You will hear a conversation between a receptionist and a caller."
SHORT_PAIR = [
    ("Speaker A", "Good morning. How can I help you?"),
    ("Speaker B", "Hello. I'd like to book a room for Friday, please."),
]
E3A_TEXT = ("Oh, that's brilliant <laugh> I honestly didn't expect that. <short pause> Right, so the deadline "
            "is Friday the twelfth. <sigh> I suppose we'll manage. <long pause> <breath> Anyway, let's move on.")
E3B_TEXT = ("<chuckle> Well, <throat-clearing> as I was saying, the museum <cough> opens at nine. <gasp> Oh no, "
            "I've left my ticket at home! <whispers> Don't tell anyone.")
E3C_TEXT = "[laughs] That's funny. <medium pause> The meeting is on Tuesday. <laughter> Really?"
E3D_TURNS = [
    ("Speaker A", "So I went to the library first |mhm| and then I took the bus to the station."),
    ("Speaker B", "<laugh> That sounds like a long day."),
]
E5_ITEMS = [
    ("I've just heard that the new library will open next month.", "cheerful and warm"),
    ("I'm not sure the bus will get us there before the doors close.", "worried, slightly hesitant"),
    ("The tickets cost eight pounds each, and the show starts at seven.", "calm and matter-of-fact"),
]
E5C_STYLES = ["cheerful and warm", "worried, slightly hesitant", "calm and matter-of-fact", "relieved and grateful"]
E7_TEXT = "The library opens at nine o'clock from Monday to Friday."
E8_TEXTS = [
    "Welcome to the museum. Please leave large bags in the lockers by the entrance.",
    "The cafe on the second floor serves hot drinks and sandwiches until four o'clock.",
    "If you have any questions, ask one of the guides wearing a green jacket.",
]
E9_TEXT = "Good morning, everyone. Today we're going to practise listening for numbers and dates."
E9_DESIGN = {
    "store": True,
    "voice": {
        "type": "prompted",
        "display_name": "probe 2026-10 British teacher",
        "gender": "female",
        "language_code": "en-GB",
        "prompted": {"input": "A woman in her forties with a warm, clear Southern British accent, "
                              "an experienced teacher speaking at a steady pace."},
    },
}

# ---------------------------------------------------------------- small utils


def say(msg: str) -> None:
    """Console output is cp1252 on this machine: print ASCII only."""
    print(msg.encode("ascii", "replace").decode("ascii"), flush=True)


def write_json(path: Path, data) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(json.dumps(data, indent=2, ensure_ascii=False).encode("utf-8"))


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(text.encode("utf-8"))


def default_out() -> Path:
    return REPO / "TESTING_DUMP" / "voice-lab" / dt.date.today().isoformat()


def wire_json(body) -> bytes:
    """serde_json without preserve_order: keys sorted, compact, UTF-8."""
    return json.dumps(body, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def usd(micro: float) -> str:
    return f"${micro / 1e6:.4f}"


# ---------------------------------------------------------------- the API key

def find_key() -> tuple[str, str]:
    value = os.environ.get("GEMINI_API_KEY", "").strip()
    if value:
        return value, "the process environment"
    if os.name == "nt":
        import winreg
        scopes = [
            (winreg.HKEY_CURRENT_USER, "Environment", "the Windows user environment"),
            (winreg.HKEY_LOCAL_MACHINE, r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
             "the Windows machine environment"),
        ]
        for root, path, label in scopes:
            try:
                with winreg.OpenKey(root, path) as handle:
                    value = str(winreg.QueryValueEx(handle, "GEMINI_API_KEY")[0]).strip()
                    if value:
                        return value, label
            except OSError:
                pass
    env_file = Path.cwd() / ".env"
    if env_file.is_file():
        for line in env_file.read_text(encoding="utf-8", errors="replace").splitlines():
            line = line.strip()
            if line.startswith("GEMINI_API_KEY="):
                value = line.split("=", 1)[1].strip().strip('"').strip("'")
                if value:
                    return value, "./.env"
    raise SystemExit("No GEMINI_API_KEY found (process, Windows registry, ./.env).")


# ---------------------------------------------------------------- scrubbing

_SCRUB = [
    (re.compile(r"AIza[0-9A-Za-z_\-]{20,}"), "<key>"),
    (re.compile(r"projects/[^/\s\"'\]]+"), "projects/<project>"),
    (re.compile(r"(?i)(project[ _-]?(?:number|id)?[\"']?\s*[:=]?\s*[\"']?)(\d{6,})"), r"\1<project>"),
    (re.compile(r"\b\d{9,}\b"), "<number>"),
]


def scrub(text: str, key: str | None = None) -> str:
    if key:
        text = text.replace(key, "<key>")
    for pattern, repl in _SCRUB:
        text = pattern.sub(repl, text)
    return text


def truncate_audio(value, keep: int = 24):
    """A deep copy of a JSON value with every base64 `data` field shortened."""
    if isinstance(value, dict):
        out = {}
        for k, v in value.items():
            if k == "data" and isinstance(v, str) and len(v) > 64:
                out[k] = f"{v[:keep]}...<{len(v) * 3 // 4} bytes>"
            else:
                out[k] = truncate_audio(v, keep)
        return out
    if isinstance(value, list):
        return [truncate_audio(v, keep) for v in value]
    return value


def body_for_manifest(body):
    """Audio data replaced by "<n bytes>"."""
    if isinstance(body, dict):
        out = {}
        for k, v in body.items():
            if k == "data" and isinstance(v, str) and len(v) > 64:
                out[k] = f"<{len(v) * 3 // 4} bytes>"
            else:
                out[k] = body_for_manifest(v)
        return out
    if isinstance(body, list):
        return [body_for_manifest(v) for v in body]
    return body


# ---------------------------------------------------------------- pricing / budget

def rates_for(model: str, at: float | None = None):
    price = PRICES.get(model)
    if not price:
        return None
    return price[0] if (at or time.time()) < INTRO_ENDS else price[1]


def modality_tokens(usage: dict, field: str, modality: str) -> int:
    total = 0
    for row in usage.get(field) or []:
        if str(row.get("modality", "")).lower() == modality:
            total += int(row.get("tokens") or 0)
    return total


def usage_micro_usd(model: str, usage: dict | None) -> tuple[int, bool]:
    """(micro_usd, priced). Same arithmetic as tokens_of/cost_micro_usd, plus
    the assumed audio-input rate for the ear."""
    usage = usage or {}
    rates = rates_for(model)
    if rates is None:
        rates = rates_for(TTS_MODEL)  # Voice design and unknown models: priced like TTS, flagged
        priced = False
    else:
        priced = True
    inp = int(usage.get("total_input_tokens") or 0)
    cached = min(int(usage.get("total_cached_tokens") or 0), inp)
    out = int(usage.get("total_output_tokens") or 0)
    thought = int(usage.get("total_thought_tokens") or 0)
    audio_in = modality_tokens(usage, "input_tokens_by_modality", "audio") if model == EAR_MODEL else 0
    audio_in = min(audio_in, inp - cached)
    audio_rate = AUDIO_INPUT_RATE[0] if time.time() < INTRO_ENDS else AUDIO_INPUT_RATE[1]
    micro = ((inp - cached - audio_in) * rates[0] + audio_in * audio_rate + cached * rates[1]
             + (out + thought) * rates[2])
    return int(round(micro)), priced


def estimate_speech(model: str, texts: list[str], voices: int = 1) -> int:
    """Conservative: 2.4 words/s plus 1.5 s per item, 25 % margin, and about
    2,500 input tokens per voice (each voice's audio reference is billed as
    input: 700-2,400 tokens measured on 2026-10-05)."""
    words = sum(len(t.split()) for t in texts)
    seconds = words / 2.4 + 1.5 * len(texts)
    out_tokens = seconds * AUDIO_TOKENS_PER_SECOND * 1.25
    in_tokens = sum(len(t) for t in texts) / 3 + 60 * len(texts) + 80 + 2500 * voices
    rates = rates_for(model) or rates_for(TTS_MODEL)
    return int(in_tokens * rates[0] + out_tokens * rates[2])


def estimate_ear(seconds: float, mode: str) -> int:
    rates = rates_for(EAR_MODEL)
    audio_rate = AUDIO_INPUT_RATE[0]
    out_tokens = {"lean": 700, "full": 1100, "tone": 1300, "audition": 450}.get(mode, 1100)
    return int((seconds * AUDIO_TOKENS_PER_SECOND + 300) * audio_rate + 700 * rates[0] + out_tokens * rates[2])


class BudgetStop(Exception):
    pass


class Ledger:
    def __init__(self, cap_usd: float, prior_micro: int = 0):
        self.cap = int(round(cap_usd * 1e6))
        self.prior = prior_micro
        self.spent = prior_micro

    def allow(self, estimate_micro: int, what: str) -> None:
        if self.spent + estimate_micro > self.cap:
            raise BudgetStop(f"{what}: estimated {usd(estimate_micro)} would take spend "
                             f"{usd(self.spent)} past the cap {usd(self.cap)}")

    def add(self, micro: int) -> None:
        self.spent += micro


# ---------------------------------------------------------------- HTTP

class Reply:
    def __init__(self, status, raw: bytes, error, latency_ms: int, wall_ms: int, retries: list):
        self.status = status
        self.raw = raw
        self.error = error
        self.latency_ms = latency_ms
        self.wall_ms = wall_ms
        self.retries = retries

    @property
    def ok(self) -> bool:
        return self.status is not None and 200 <= self.status < 300

    def json(self):
        try:
            return json.loads(self.raw.decode("utf-8"))
        except (ValueError, UnicodeDecodeError):
            return None

    def count_429(self) -> int:
        return sum(1 for r in self.retries if r.get("status") == 429) + (1 if self.status == 429 else 0)


class Api:
    def __init__(self, key: str):
        self._key = key

    def send(self, method: str, path: str, query=None, body=None, timeout: float = TEXT_TIMEOUT_S) -> Reply:
        url = API_ROOT + path
        if query:
            url += "?" + urllib.parse.urlencode(query, doseq=True)
        data = None if body is None else wire_json(body)
        headers = {"x-goog-api-key": self._key}
        if data is not None:
            headers["Content-Type"] = "application/json"
        started = time.monotonic()
        retries = []
        backoff = FIRST_BACKOFF_S
        status, raw, error, attempt_started = None, b"", None, started
        for attempt in range(MAX_RETRIES + 1):
            attempt_started = time.monotonic()
            status, raw, error = None, b"", None
            try:
                request = urllib.request.Request(url, data=data, headers=headers, method=method)
                with urllib.request.urlopen(request, timeout=timeout) as response:
                    status, raw = response.status, response.read()
            except urllib.error.HTTPError as e:
                status, raw = e.code, e.read()
            except (TimeoutError, socket.timeout):
                error = "timeout"
            except urllib.error.URLError as e:
                error = "timeout" if isinstance(e.reason, (TimeoutError, socket.timeout)) else \
                    f"network error ({type(e.reason).__name__})"
            retryable = status in (429, 503, 504) or error == "timeout"
            if retryable and attempt < MAX_RETRIES:
                retries.append({"attempt": attempt + 1, "status": status or error, "backoff_s": backoff})
                say(f"    retry {attempt + 1}/{MAX_RETRIES} after {status or error}, waiting {backoff:.0f} s")
                time.sleep(backoff)
                backoff *= 2
                continue
            break
        now = time.monotonic()
        return Reply(status, raw, error, int((now - attempt_started) * 1000), int((now - started) * 1000), retries)

    def scrub(self, text: str) -> str:
        return scrub(text, self._key)


# ---------------------------------------------------------------- speech bodies

def wire_speaker(label: str) -> str:
    return "".join(c for c in label if not c.isspace())


def speech_body(model: str, turns, voices, style: str = PASSAGE_STYLE, styles=None,
                voice_extra: dict | None = None, gen_extra: dict | None = None) -> dict:
    """speech_body of gemini.rs. `turns` are (label|None, text); `voices` are
    (label, voice). `styles` (one per turn), `voice_extra` (merged into the
    single-voice entry, e.g. language) and `gen_extra` (e.g. seed) exist only
    for experiments that say so."""
    if not turns:
        raise ValueError("nothing to read aloud")
    if len(voices) == 1:
        entry = {"voice": voices[0][1]}
        if voice_extra:
            entry.update(voice_extra)
        speech_config = [entry]
    elif len(voices) == 2:
        speech_config = {"mode": "conversational",
                         "speakers": [{"speaker": wire_speaker(label), "voice": voice} for label, voice in voices]}
    else:
        raise ValueError("one or two voices per speech request")
    labels = {label for label, _ in voices}
    content = []
    for index, (speaker, text) in enumerate(turns):
        metadata = {"type": "speech_metadata", "style": styles[index] if styles else style}
        if len(voices) == 2:
            if speaker not in labels:
                raise ValueError(f"turn by {speaker!r} has no voice in this request")
            metadata["speaker"] = wire_speaker(speaker)
        content.append({"type": "text", "text": text, "annotations": [metadata]})
    generation_config = {"speech_config": speech_config}
    if gen_extra:
        generation_config.update(gen_extra)
    return {
        "model": model,
        "input": [{"type": "user_input", "content": content}],
        "response_format": {"type": "audio", "mime_type": "audio/l16", "sample_rate": SAMPLE_RATE},
        "generation_config": generation_config,
        "store": False,
    }


def check_speech_body() -> None:
    """The JSON asserted by two_voice_speech_sends_one_annotated_item_per_turn."""
    body = speech_body("gemini-3.8-flash-tts",
                       [("Speaker A", "Good morning."), ("Speaker B", "Hello.")],
                       [("Speaker A", "Kore"), ("Speaker B", "Puck")], style="calm")
    assert body["generation_config"]["speech_config"] == {
        "mode": "conversational",
        "speakers": [{"speaker": "SpeakerA", "voice": "Kore"}, {"speaker": "SpeakerB", "voice": "Puck"}]}
    assert body["input"][0]["content"][1]["annotations"][0] == {
        "type": "speech_metadata", "style": "calm", "speaker": "SpeakerB"}
    single = speech_body("m", [(None, "Part one.")], [("Announcer", "Charon")], style="slow")
    assert single["generation_config"]["speech_config"] == [{"voice": "Charon"}]
    assert single["input"][0]["content"][0]["annotations"][0] == {"type": "speech_metadata", "style": "slow"}
    assert wire_json(single).startswith(b'{"generation_config":{"speech_config":[{"voice":"Charon"}]},"input":')


# ---------------------------------------------------------------- responses

def model_outputs(response: dict):
    for step in response.get("steps") or []:
        if step.get("type") == "model_output":
            yield step


def output_text(response: dict) -> str | None:
    steps = list(model_outputs(response))
    if not steps:
        return None
    text = "".join(item.get("text", "") for item in steps[-1].get("content") or [] if item.get("type") == "text")
    text = text.strip()
    return text or None


def output_audio(response: dict) -> np.ndarray:
    parts = []
    for step in model_outputs(response):
        for item in step.get("content") or []:
            if item.get("type") != "audio":
                continue
            raw = base64.b64decode(item["data"])
            if raw[:4] == b"RIFF":
                samples, rate = parse_wav(raw)
            else:
                rate = int(item.get("sample_rate") or SAMPLE_RATE)
                samples = np.frombuffer(raw[: len(raw) // 2 * 2], dtype="<i2")
            if rate != SAMPLE_RATE:
                raise ValueError(f"audio came back at {rate} Hz")
            parts.append(samples)
    if not parts:
        raise ValueError("no audio in the response")
    return np.concatenate(parts).astype(np.int16)


def response_shape(response: dict) -> dict:
    """The response without its audio: for the manifest and fixtures."""
    return truncate_audio(response)


def strip_fences(text: str) -> str:
    text = text.strip()
    if text.startswith("```"):
        text = text.split("\n", 1)[1] if "\n" in text else ""
        text = text.rstrip()
        if text.endswith("```"):
            text = text[:-3]
    return text.strip()


# ---------------------------------------------------------------- WAV

def wav_bytes(samples: np.ndarray, rate: int = SAMPLE_RATE) -> bytes:
    data = samples.astype("<i2").tobytes()
    header = b"RIFF" + struct.pack("<I", 36 + len(data)) + b"WAVE"
    header += b"fmt " + struct.pack("<IHHIIHH", 16, 1, 1, rate, rate * 2, 2, 16)
    header += b"data" + struct.pack("<I", len(data))
    return header + data


def parse_wav(raw: bytes) -> tuple[np.ndarray, int]:
    if raw[:4] != b"RIFF" or raw[8:12] != b"WAVE":
        raise ValueError("not a RIFF/WAVE file")
    pos, fmt, data = 12, None, None
    while pos + 8 <= len(raw):
        cid, size = raw[pos:pos + 4], struct.unpack("<I", raw[pos + 4:pos + 8])[0]
        chunk = raw[pos + 8:pos + 8 + size]
        if cid == b"fmt ":
            fmt = struct.unpack("<HHIIHH", chunk[:16])
        elif cid == b"data":
            data = chunk
        pos += 8 + size + (size & 1)
    if fmt is None or data is None:
        raise ValueError("WAV without fmt or data")
    _, channels, rate, _, _, bits = fmt
    if bits != 16:
        raise ValueError(f"{bits}-bit WAV is not supported")
    samples = np.frombuffer(data[: len(data) // 2 * 2], dtype="<i2")
    if channels > 1:
        samples = samples[: len(samples) // channels * channels].reshape(-1, channels).mean(axis=1).astype(np.int16)
    return samples, rate


def read_wav(path: Path) -> tuple[np.ndarray, int]:
    return parse_wav(Path(path).read_bytes())


def silence(ms: int, rate: int = SAMPLE_RATE) -> np.ndarray:
    return np.zeros(int(rate * ms / 1000), dtype=np.int16)


# ---------------------------------------------------------------- F0 (YIN)

F0_FRAME_S, F0_HOP_S, F0_THRESHOLD = 0.040, 0.010, 0.15
F0_MIN_HZ, F0_MAX_HZ = 65.0, 400.0
VOICED_APERIODICITY, SILENCE_DB = 0.2, -40.0
ISLAND_GAP_S, ISLAND_MIN_S, ISLAND_MIN_VOICED = 0.200, 1.0, 30
FEMALE_LOW_HZ, MALE_HIGH_HZ, CLUSTER_GAP_HZ = 150.0, 190.0, 40.0
DEEP_HZ = 120.0  # below this a "female" island sounds like the 0.7.1 male flips (84-109 Hz)


def f0_track(samples: np.ndarray, rate: int):
    """YIN: 40 ms frames, 10 ms hop, threshold 0.15, 65-400 Hz. Returns frame
    times (s), f0 (Hz), aperiodicity and RMS in dB relative to the loudest frame."""
    x = samples.astype(np.float64) / 32768.0
    width, hop = int(F0_FRAME_S * rate), int(F0_HOP_S * rate)
    tau_min, tau_max = int(rate / F0_MAX_HZ), int(math.ceil(rate / F0_MIN_HZ))
    span = width + tau_max
    n_frames = max(0, (len(x) - width) // hop + 1)
    if n_frames == 0:
        return np.zeros(0), np.zeros(0), np.zeros(0), np.zeros(0)
    x = np.concatenate([x, np.zeros(span)])
    nfft = 1 << (span - 1).bit_length()
    f0 = np.zeros(n_frames)
    aper = np.ones(n_frames)
    rms = np.zeros(n_frames)
    lags = np.arange(span)
    taus = np.arange(tau_max + 1)
    for start in range(0, n_frames, 2048):
        idx = np.arange(start, min(n_frames, start + 2048)) * hop
        frames = x[idx[:, None] + lags]
        head = frames[:, :width]
        corr = np.fft.irfft(np.conj(np.fft.rfft(head, nfft)) * np.fft.rfft(frames, nfft), nfft)[:, : tau_max + 1]
        cs = np.concatenate([np.zeros((len(idx), 1)), np.cumsum(frames ** 2, axis=1)], axis=1)
        r0 = cs[:, width]
        rtau = cs[:, width + taus] - cs[:, taus]
        diff = np.maximum(r0[:, None] + rtau - 2 * corr, 0.0)
        diff[:, 0] = 0.0
        cum = np.cumsum(diff[:, 1:], axis=1)
        cmnd = np.ones_like(diff)
        with np.errstate(divide="ignore", invalid="ignore"):
            cmnd[:, 1:] = np.where(cum > 0, diff[:, 1:] * taus[1:] / cum, 1.0)
        search = cmnd[:, tau_min:]
        below = search < F0_THRESHOLD
        has = below.any(axis=1)
        tau = np.where(has, np.argmax(below, axis=1), np.argmin(search, axis=1)) + tau_min
        rows = np.arange(len(idx))
        for _ in range(tau_max):
            nxt = np.minimum(tau + 1, tau_max)
            move = has & (tau < tau_max) & (cmnd[rows, nxt] < cmnd[rows, tau])
            if not move.any():
                break
            tau = tau + move
        a = cmnd[rows, np.maximum(tau - 1, 0)]
        b = cmnd[rows, tau]
        c = cmnd[rows, np.minimum(tau + 1, tau_max)]
        denom = a - 2 * b + c
        with np.errstate(divide="ignore", invalid="ignore"):
            shift = np.where(np.abs(denom) > 1e-12, 0.5 * (a - c) / denom, 0.0)
        shift = np.clip(shift, -1, 1)
        f0[start:start + len(idx)] = rate / (tau + shift)
        aper[start:start + len(idx)] = b
        rms[start:start + len(idx)] = np.sqrt(r0 / width)
    peak = rms.max() if rms.size and rms.max() > 0 else 1.0
    rms_db = 20 * np.log10(np.maximum(rms, 1e-12) / peak)
    times = (np.arange(n_frames) * hop + width / 2) / rate
    return times, f0, aper, rms_db


def islands_of(times, f0, aper, rms_db) -> list[dict]:
    loud = rms_db > SILENCE_DB
    voiced = loud & (aper < VOICED_APERIODICITY)
    gap_frames = int(round(ISLAND_GAP_S / F0_HOP_S))
    out, i, n = [], 0, len(times)
    while i < n:
        if not loud[i]:
            i += 1
            continue
        start, j, quiet = i, i, 0
        last_loud = i
        while j < n:
            if loud[j]:
                last_loud, quiet = j, 0
            else:
                quiet += 1
                if quiet >= gap_frames:
                    break
            j += 1
        end = last_loud
        span = slice(start, end + 1)
        v = voiced[span]
        duration = times[end] - times[start] + F0_HOP_S
        if duration >= ISLAND_MIN_S and int(v.sum()) >= ISLAND_MIN_VOICED:
            values = f0[span][v]
            out.append({"start": round(float(times[start] - F0_FRAME_S / 2), 2),
                        "end": round(float(times[end] + F0_FRAME_S / 2), 2),
                        "median_hz": round(float(np.median(values)), 1),
                        "voiced": int(v.sum()),
                        "below_150": round(float((values < FEMALE_LOW_HZ).mean()), 2)})
        i = j + 1
    return out


def two_means(values: list[float]):
    """Optimal 1-D 2-means by trying every split of the sorted values."""
    if len(values) < 2:
        return None
    order = sorted(values)
    best = None
    for k in range(1, len(order)):
        low, high = order[:k], order[k:]
        cost = sum((v - np.mean(low)) ** 2 for v in low) + sum((v - np.mean(high)) ** 2 for v in high)
        if best is None or cost < best[0]:
            best = (cost, float(np.mean(low)), float(np.mean(high)), order[k - 1], order[k])
    _, low_mean, high_mean, low_max, high_min = best
    return {"low_hz": round(low_mean, 1), "high_hz": round(high_mean, 1),
            "split_hz": round((low_max + high_min) / 2, 1), "separation_hz": round(high_mean - low_mean, 1)}


def f0_report(samples: np.ndarray, rate: int, expect: str | None) -> dict:
    """Islands with median F0 and gender flags. `expect`: female, male,
    female-pair / male-pair (same-gender requests: attribution certain), mixed
    (2-means on island medians; clusters must be > 40 Hz apart) or None."""
    times, f0, aper, rms_db = f0_track(samples, rate)
    islands = islands_of(times, f0, aper, rms_db)
    voiced = (rms_db > SILENCE_DB) & (aper < VOICED_APERIODICITY)
    report = {"duration_s": round(len(samples) / rate, 2), "expect": expect, "islands": islands,
              "median_hz": round(float(np.median(f0[voiced])), 1) if voiced.any() else None,
              "flags": []}
    gender = (expect or "").split("-")[0]
    for island in islands:
        island["severity"] = None
    if gender == "female":
        report["flags"] = [i for i in islands if i["median_hz"] < FEMALE_LOW_HZ]
    elif gender == "male":
        report["flags"] = [i for i in islands if i["median_hz"] > MALE_HIGH_HZ]
    elif gender == "mixed":
        clusters = two_means([i["median_hz"] for i in islands])
        report["clusters"] = clusters
        if clusters:
            report["separated"] = clusters["separation_hz"] > CLUSTER_GAP_HZ
            for island in islands:
                island["cluster"] = "low" if island["median_hz"] <= clusters["split_hz"] else "high"
            report["flags"] = [i for i in islands
                               if (i["cluster"] == "low" and i["median_hz"] > MALE_HIGH_HZ)
                               or (i["cluster"] == "high" and i["median_hz"] < FEMALE_LOW_HZ)]
            if not report["separated"]:
                report["note"] = "island medians do not form two clusters > 40 Hz apart"
    for flag in report["flags"]:
        # The 0.7.1 flips sat at 84-109 Hz; 120-150 Hz is more often a low female voice.
        if flag["median_hz"] < FEMALE_LOW_HZ:
            flag["severity"] = "deep" if flag["median_hz"] < DEEP_HZ else "borderline"
        else:
            flag["severity"] = "high" if flag["median_hz"] > MALE_HIGH_HZ + 30 else "borderline"
    return report


def f0_line(report: dict) -> str:
    isl = ", ".join(f"{i['start']:.1f}-{i['end']:.1f}s {i['median_hz']:.0f}Hz" for i in report["islands"])
    deep = sum(1 for f in report["flags"] if f.get("severity") in ("deep", "high"))
    flags = f"{len(report['flags'])} ({deep} deep/high)" if report["flags"] else "0"
    extra = ""
    if report.get("clusters"):
        c = report["clusters"]
        extra = f"; clusters {c['low_hz']:.0f}/{c['high_hz']:.0f} Hz (sep {c['separation_hz']:.0f})"
    return f"median {report['median_hz']} Hz, {len(report['islands'])} islands, flags {flags}{extra} [{isl}]"


# ---------------------------------------------------------------- the AI ear

EAR_PROMPT = """You are an expert phonetician. Listen carefully to the attached recording of English speech and judge only what you hear.

Return one JSON object with exactly these keys:
- "distinct_speakers": how many different voices you hear (integer).
- "speakers": one entry per distinct voice, in order of first appearance: {"order": 1, "gender": "female" | "male" | "unclear", "age": "20s" | "30s" | "40s" | "50s+", "accent": "England" | "Scotland" | "Ireland" | "USA" | "Canada" | "Australia" | "New Zealand" | "South Africa" | "India" | "other", "confidence": your confidence in that accent from 0.0 to 1.0, "cues": up to three short phonetic cues for the accent}.
- "gender_change_within_a_speaker": true if any voice changes its apparent gender or turns into a clearly different voice partway through (for example a woman's voice that becomes a deep man's voice), otherwise false.
- "gender_change_at_s": the time in seconds where that happens, or null.
- "transcript": TRANSCRIPT_RULE
- "spoken_markup": every stage-direction or markup word that is pronounced as a word instead of being performed, for example "laugh", "laughs", "laughter", "sigh", "short pause", "long pause", "medium pause", "breath", "chuckle", "cough", "gasp", "whispers", "throat clearing"; [] if none.
- "non_speech": every non-speech vocal event or clearly deliberate pause you hear: {"kind": "laugh" | "sigh" | "breath" | "pause" | "cough" | "gasp" | "whisper" | "other", "at_s": seconds from the start}; [] if none.
- "backchannels": short listener reactions such as "mhm" or "uh-huh" heard while or between someone else's sentences: {"text": "...", "by_speaker": order, "at_s": seconds}; [] if none.EXTRA_KEYS
"""
EAR_TRANSCRIPT_FULL = ("the words you hear, verbatim, including any markup words that were read aloud; "
                       "do not write non-speech sounds into it.")
EAR_TRANSCRIPT_LEAN = '"" (leave it empty for this recording).'
EAR_TONE = ('\n- "tone": for each sentence in order, {"at_s": start seconds, "emotion": two to four words '
            'describing the emotion or delivery you hear}.')


def ear_prompt(mode: str) -> str:
    prompt = EAR_PROMPT.replace("TRANSCRIPT_RULE", EAR_TRANSCRIPT_LEAN if mode == "lean" else EAR_TRANSCRIPT_FULL)
    return prompt.replace("EXTRA_KEYS", EAR_TONE if mode == "tone" else "")


def ear_body(wav: bytes, mode: str) -> dict:
    return {
        "model": EAR_MODEL,
        "input": [{"type": "user_input", "content": [
            {"type": "audio", "data": base64.b64encode(wav).decode("ascii"), "mime_type": "audio/wav"},
            {"type": "text", "text": ear_prompt(mode)}]}],
        "generation_config": {"thinking_level": "low", "max_output_tokens": 2048},
        "response_format": {"type": "text", "mime_type": "application/json"},
        "store": False,
    }


def ear_summary(parsed: dict | None) -> str:
    if not parsed:
        return "no answer"
    speakers = parsed.get("speakers") or []
    who = "; ".join(f"{s.get('gender')}/{s.get('accent')}({s.get('confidence')})" for s in speakers)
    bits = [f"{parsed.get('distinct_speakers')} speaker(s): {who}"]
    if parsed.get("gender_change_within_a_speaker"):
        bits.append(f"GENDER CHANGE at {parsed.get('gender_change_at_s')}s")
    if parsed.get("spoken_markup"):
        bits.append(f"spoken markup {parsed['spoken_markup']}")
    if parsed.get("non_speech"):
        bits.append("non-speech " + ",".join(f"{n.get('kind')}@{n.get('at_s')}" for n in parsed["non_speech"]))
    if parsed.get("backchannels"):
        bits.append("backchannels " + ",".join(f"{b.get('text')}by{b.get('by_speaker')}@{b.get('at_s')}"
                                               for b in parsed["backchannels"]))
    return " | ".join(bits)


# ---------------------------------------------------------------- run folder / manifest

class Run:
    """One output folder: manifest (all requests, cumulative spend), WAVs."""

    def __init__(self, folder: Path, api: Api | None, cap_usd: float | None, name: str):
        self.folder = folder
        self.api = api
        self.name = name
        self.manifest_path = folder / "manifest.json"
        folder.mkdir(parents=True, exist_ok=True)
        if self.manifest_path.is_file():
            self.manifest = json.loads(self.manifest_path.read_text(encoding="utf-8"))
        else:
            self.manifest = {"tool": "tools/voice_lab.py", "subcommand": name, "runs": [], "requests": []}
        prior = sum(int(r.get("micro_usd") or 0) for r in self.manifest["requests"])
        self.ledger = Ledger(cap_usd, prior) if cap_usd is not None else None
        self.manifest["runs"].append({"started": dt.datetime.now().isoformat(timespec="seconds"),
                                      "cap_usd": cap_usd, "prior_spend_usd": prior / 1e6})
        self.save()

    def save(self) -> None:
        self.manifest["spent_usd"] = round(sum(int(r.get("micro_usd") or 0) for r in self.manifest["requests"]) / 1e6, 6)
        write_json(self.manifest_path, self.manifest)

    def entry(self, rid: str) -> dict | None:
        for r in reversed(self.manifest["requests"]):
            if r["id"] == rid:
                return r
        return None

    def record(self, entry: dict) -> dict:
        self.manifest["requests"].append(entry)
        self.save()
        return entry

    def call(self, rid: str, purpose: str, model: str | None, method: str, path: str, body=None,
             query=None, estimate: int = 0, timeout: float = TEXT_TIMEOUT_S, paid: bool = True):
        """Sends one request after the budget check; records it; returns (entry, json)."""
        if paid and self.ledger is not None:
            self.ledger.allow(estimate, rid)
        reply = self.api.send(method, path, query=query, body=body, timeout=timeout)
        data = reply.json()
        usage = data.get("usage") if isinstance(data, dict) else None
        micro, priced = (0, True)
        if reply.ok and usage:
            micro, priced = usage_micro_usd(model or "", usage)
        entry = {
            "id": rid, "purpose": purpose, "model": model, "method": method, "path": path,
            "query": query, "body": body_for_manifest(body) if body is not None else None,
            "status": reply.status if reply.status is not None else reply.error,
            "error_body": None if reply.ok else self.api.scrub(reply.raw.decode("utf-8", "replace"))[:4000],
            "latency_ms": reply.latency_ms, "wall_ms": reply.wall_ms, "retries": reply.retries,
            "http_429": reply.count_429(), "usage": usage, "micro_usd": micro,
            "estimate_micro_usd": estimate, "priced": priced,
            "served_by": data.get("model") if isinstance(data, dict) else None,
            "interaction_status": data.get("status") if isinstance(data, dict) else None,
            "at": dt.datetime.now().isoformat(timespec="seconds"),
        }
        if self.ledger is not None:
            self.ledger.add(micro)
        self.record(entry)
        spent = f" spent {usd(self.ledger.spent)}" if self.ledger else ""
        say(f"  {rid}: {entry['status']} in {reply.wall_ms} ms, {usd(micro)}{spent}")
        return entry, data


# ---------------------------------------------------------------- catalog

CELLS = {
    # accent key: (language codes, accent substrings (lowercase, any); empty = any accent)
    "british": (["en-GB"], ["winchester"]),
    "american": (["en-US"], []),
    "australian": (["en-AU"], ["sydney"]),
    "canadian": (["en-CA"], ["toronto", "vancouver"]),
    "newzealand": (["en-NZ"], ["auckland"]),
    "irish": (["en-IE"], ["dublin"]),
    "scottish": (["en-GB"], ["glasgow"]),
    "southafrican": (["en-ZA"], ["cape town"]),
    "indian": (["en-IN"], []),
}
CORE_ACCENTS = ["british", "american", "australian", "canadian", "newzealand"]
NEW_ACCENTS = ["irish", "scottish", "southafrican", "indian"]
EXAM_PERSONAS = ["teacher", "advisor", "assistant", "narrator", "presenter", "announcer", "instructor",
                 "lecturer", "professor", "host", "guide", "educator", "news"]
FACTS_2026_10_05 = {
    "prebuilt": 2089, "en-GB Winchester English": 49, "en-AU Sydney": 44, "en-NZ Auckland": 27,
    "en-CA Toronto+Vancouver": 80, "en-IE": 80, "en-ZA": 80, "en-IN": 120, "en-US General American": 30,
}


def in_cell(voice: dict, cell: str) -> bool:
    languages, accents = CELLS[cell]
    if voice.get("language_code") not in languages:
        return False
    accent = str(voice.get("accent") or "").lower()
    return not accents or any(a in accent for a in accents)


def gender_of(voice: dict) -> str:
    return str(voice.get("gender") or "").lower()


def persona_rank(voice: dict) -> int:
    text = " ".join(str(voice.get(k) or "") for k in ("id", "display_name", "persona", "context")).lower()
    for rank, word in enumerate(EXAM_PERSONAS):
        if word in text:
            return rank
    return len(EXAM_PERSONAS)


def candidates(voices: list[dict], cell: str, gender: str, limit: int) -> list[dict]:
    """Exam personas first, pitch spread low/medium/high, ties by id."""
    pool = [v for v in voices if in_cell(v, cell) and gender_of(v) == gender and v.get("type") == "prebuilt"]
    tiers = [pool]
    if cell == "american":
        # General American (the 30 classics) first, then the other en-US regions.
        general = [v for v in pool if "general american" in str(v.get("accent") or "").lower()]
        tiers = [general, [v for v in pool if v not in general]]
    picked: list[dict] = []
    for tier in tiers:
        tier = sorted(tier, key=lambda v: (persona_rank(v), str(v.get("id"))))
        by_pitch: dict[str, list[dict]] = {}
        for v in tier:
            by_pitch.setdefault(str(v.get("pitch") or "unknown").lower(), []).append(v)
        order = [p for p in ("low", "medium", "high") if p in by_pitch] + \
                [p for p in by_pitch if p not in ("low", "medium", "high")]
        while len(picked) < limit and any(by_pitch[p] for p in order):
            for p in order:
                if by_pitch[p] and len(picked) < limit:
                    picked.append(by_pitch[p].pop(0))
    return picked


def cmd_catalog(args) -> None:
    key, source = find_key()
    say(f"key from {source}")
    api = Api(key)
    folder = Path(args.out) / "catalog"
    raw_dir = folder / "raw"
    run = Run(folder, api, None, "catalog")
    voices: dict[str, dict] = {}
    order: list[str] = []
    page, token = 0, None
    sample_in_list = 0
    while True:
        page += 1
        query = {"page_size": 1000}
        if token:
            query["page_token"] = token
        entry, data = run.call(f"E0.list.p{page}", "list every voice", None, "GET", "/voices", query=query, paid=False)
        if not entry["status"] == 200 or not isinstance(data, dict):
            break
        write_json(raw_dir / f"voices-page-{page}.json", truncate_audio(data))
        for v in data.get("voices") or []:
            if v.get("sample_audio") or (v.get("prompted") or {}).get("sample_audio"):
                sample_in_list += 1
            vid = v.get("id") or v.get("name")
            if vid not in voices:
                order.append(vid)
            voices[vid] = truncate_audio(v)
        token = data.get("next_page_token")
        if not token:
            break
    entry, prompted = run.call("E0.list.prompted", "designed voices of this key's project", None, "GET", "/voices",
                               query={"type": "prompted", "page_size": 1000}, paid=False)
    if isinstance(prompted, dict):
        write_json(raw_dir / "voices-prompted.json", truncate_audio(prompted))
    get_library, _ = run.call("E0.get.library", "GetVoice on a library voice", None, "GET",
                              "/voices/en-gb-advisor-1", paid=False)
    entry, designed = run.call("E0.get.designed", "GetVoice on one designed voice (read only)", None, "GET",
                               "/voices/voice_kpd3e297369r", paid=False)
    designed_sample = None
    if isinstance(designed, dict) and entry["status"] == 200:
        write_json(raw_dir / "voice_kpd3e297369r.json", truncate_audio(designed))
        sample = designed.get("sample_audio") or (designed.get("prompted") or {}).get("sample_audio")
        if sample and sample.get("data"):
            raw = base64.b64decode(sample["data"])
            designed_sample = {"mime_type": sample.get("mime_type"), "bytes": len(raw)}
            (folder / "samples").mkdir(parents=True, exist_ok=True)
            (folder / "samples" / "voice_kpd3e297369r.wav").write_bytes(raw)
    all_voices = [voices[v] for v in order]
    write_json(folder / "catalog.json", {"fetched_at": dt.datetime.now().isoformat(timespec="seconds"),
                                         "pages": page, "voices": all_voices})
    summary = catalog_summary(all_voices, prompted if isinstance(prompted, dict) else {},
                              get_library, designed if isinstance(designed, dict) else None,
                              designed_sample, sample_in_list)
    write_text(folder / "catalog-summary.md", summary)
    say(f"{len(all_voices)} voices in {page} page(s); summary at {folder / 'catalog-summary.md'}")


def counts(voices, *fields) -> dict:
    out: dict[str, int] = {}
    for v in voices:
        k = " | ".join(str(v.get(f)) for f in fields)
        out[k] = out.get(k, 0) + 1
    return dict(sorted(out.items(), key=lambda kv: (-kv[1], kv[0])))


def catalog_summary(voices, prompted, get_library, designed, designed_sample, sample_in_list) -> str:
    lines = [f"# Voice catalog, {dt.date.today().isoformat()}", ""]
    types = counts(voices, "type")
    lines += ["## Counts per type", ""] + [f"- {k}: {n}" for k, n in types.items()] + [""]
    fields = sorted({k for v in voices for k in v.keys()})
    lines += ["## Fields seen on voice objects", "", ", ".join(f"`{f}`" for f in fields), ""]
    english = [v for v in voices if str(v.get("language_code", "")).startswith("en")]
    lines += ["## English voices per language_code / accent / gender (exact strings)", "",
              "| language_code | accent | gender | n |", "|---|---|---|---|"]
    for k, n in counts(english, "language_code", "accent", "gender").items():
        lc, ac, g = k.split(" | ")
        lines.append(f"| {lc} | {ac} | {g} | {n} |")
    lines += ["", "## All languages: voices per language_code", ""]
    lines += [", ".join(f"{k}: {n}" for k, n in counts(voices, "language_code").items()), ""]
    lines += ["## Pitch / persona / context values (English)", ""]
    for f in ("pitch", "persona", "context"):
        c = counts(english, f)
        lines.append(f"- {f} ({len(c)} values): " + ", ".join(f"{k} ({n})" for k, n in list(c.items())[:40]))
    lines += ["", "## Diff against the facts of 2026-10-05", "", "| fact | then | now |", "|---|---|---|"]
    prebuilt = [v for v in voices if v.get("type") == "prebuilt"]
    now = {
        "prebuilt": len(prebuilt),
        "en-GB Winchester English": sum(1 for v in prebuilt if v.get("language_code") == "en-GB" and "winchester" in str(v.get("accent", "")).lower()),
        "en-AU Sydney": sum(1 for v in prebuilt if in_cell(v, "australian")),
        "en-NZ Auckland": sum(1 for v in prebuilt if in_cell(v, "newzealand")),
        "en-CA Toronto+Vancouver": sum(1 for v in prebuilt if in_cell(v, "canadian")),
        "en-IE": sum(1 for v in prebuilt if v.get("language_code") == "en-IE"),
        "en-ZA": sum(1 for v in prebuilt if v.get("language_code") == "en-ZA"),
        "en-IN": sum(1 for v in prebuilt if v.get("language_code") == "en-IN"),
        "en-US General American": sum(1 for v in prebuilt if v.get("language_code") == "en-US" and "general american" in str(v.get("accent", "")).lower()),
    }
    for k, then in FACTS_2026_10_05.items():
        lines.append(f"| {k} | {then} | {now[k]}{'' if now[k] == then else ' (changed)'} |")
    lines += ["", "## Designed (prompted) voices of this key's project", ""]
    plist = prompted.get("voices") or []
    lines.append(f"{len(plist)} listed with type=prompted.")
    lines += ["", "| id | display_name | gender | language_code | accent | protected |", "|---|---|---|---|---|---|"]
    for v in plist:
        lines.append(f"| {v.get('id')} | {v.get('display_name')} | {v.get('gender')} | {v.get('language_code')} | "
                     f"{v.get('accent')} | {'yes' if v.get('id') in PROTECTED_VOICES else 'no'} |")
    missing = PROTECTED_VOICES - {v.get("id") for v in plist}
    lines.append("")
    lines.append(f"The 8 known designed voices are {'all listed' if not missing else 'NOT all listed; missing ' + ', '.join(sorted(missing))}.")
    lines += ["", "## sample_audio", "",
              f"- in ListVoices responses: {sample_in_list} voice(s) carried sample_audio",
              f"- GetVoice on a library voice (`/voices/en-gb-advisor-1`): status {get_library['status']}"
              + (f", body `{(get_library['error_body'] or '')[:300]}`" if get_library.get("error_body") else ""),
              f"- GetVoice on a designed voice (`voice_kpd3e297369r`): "
              + (f"sample_audio {designed_sample}" if designed_sample else "no sample_audio")
              + (f"; fields {sorted(designed.keys())}" if designed else ""), ""]
    lines += ["## Candidates per accent x gender (exam personas first, pitch spread, ties by id)", ""]
    for cell in CORE_ACCENTS + NEW_ACCENTS:
        for gender in ("female", "male"):
            total = sum(1 for v in prebuilt if in_cell(v, cell) and gender_of(v) == gender)
            picks = candidates(voices, cell, gender, 8)
            lines.append(f"- **{cell} {gender}** ({total} in cell): " + ", ".join(
                f"`{v.get('id')}` ({v.get('pitch')}, {v.get('persona')}, {v.get('accent')})" for v in picks))
    lines += ["", "## Example voice objects", ""]
    for vid in ("en-gb-advisor-1", "zephyr"):
        v = next((x for x in voices if str(x.get("id")).lower() == vid), None)
        if v:
            lines += ["```json", json.dumps(v, indent=2, ensure_ascii=False), "```", ""]
    return "\n".join(lines) + "\n"


def load_catalog(out: Path) -> list[dict]:
    path = out / "catalog" / "catalog.json"
    if not path.is_file():
        raise SystemExit(f"{path} is missing: run `voice_lab.py catalog --out {out}` first (free).")
    return json.loads(path.read_text(encoding="utf-8"))["voices"]


def load_prompted(out: Path) -> list[dict]:
    path = out / "catalog" / "raw" / "voices-prompted.json"
    if not path.is_file():
        return []
    return json.loads(path.read_text(encoding="utf-8")).get("voices") or []


# ---------------------------------------------------------------- probe

class Req:
    def __init__(self, rid, exp, purpose, turns, voices, model=TTS_MODEL, style=PASSAGE_STYLE, styles=None,
                 voice_extra=None, gen_extra=None, expect=None, ear=None, expect_error=False, sheet=None,
                 ear_if_flagged=False):
        self.rid, self.exp, self.purpose = rid, exp, purpose
        self.turns, self.voices, self.model = turns, voices, model
        self.style, self.styles = style, styles
        self.voice_extra, self.gen_extra = voice_extra, gen_extra
        self.expect, self.ear, self.expect_error, self.sheet = expect, ear, expect_error, sheet
        # True: the ear runs only when F0 flags this clip (keeps repeated takes cheap).
        self.ear_if_flagged = ear_if_flagged

    def body(self) -> dict:
        return speech_body(self.model, self.turns, self.voices, self.style, self.styles,
                           self.voice_extra, self.gen_extra)


def pair(a: str, b: str):
    return [("Speaker A", a), ("Speaker B", b)]


def experiments(catalog: list[dict], prompted: list[dict]) -> dict[str, list[Req]]:
    by_id = {v.get("id"): v for v in catalog}
    winchester_f = [v["id"] for v in candidates(catalog, "british", "female", 20) if v["id"] != "en-gb-advisor-1"]
    second_f = winchester_f[0] if winchester_f else None
    designed_f = sorted(v["id"] for v in prompted if v.get("id") in PROTECTED_VOICES and gender_of(v) == "female")
    designed_m = sorted(v["id"] for v in prompted if v.get("id") in PROTECTED_VOICES and gender_of(v) == "male")
    designed_gender = {v.get("id"): gender_of(v) or None for v in prompted}
    single = lambda text: [(None, text)]
    exps: dict[str, list[Req]] = {}
    exps["E1"] = [
        Req("E1a", "E1a", "library pair F+M in speakers[]", D_MIXED, pair("en-gb-advisor-1", "en-gb-assistant-2"),
            expect="mixed", ear="lean", sheet="pair"),
        Req("E1b", "E1b", f"same-cell library pair F+F (en-gb-advisor-1 + {second_f})", D_FEMALE,
            pair("en-gb-advisor-1", second_f), expect="female-pair", ear="lean", sheet="flip"),
        Req("E1c", "E1c", "library + classic (Orus)", D_MIXED, pair("en-gb-advisor-1", "Orus"),
            expect="mixed", ear="lean", sheet="pair"),
        Req("E1d", "E1d", "cross-region library pair", D_MIXED, pair("en-au-advisor-5", "en-gb-assistant-2"),
            expect="mixed", ear="lean", sheet="pair"),
    ]
    e2 = []
    for take in (1, 2, 3):
        e2.append(Req(f"E2a.t{take}", "E2a", "Zephyr+Zephyr (0.7.1 HSG case)", D_FEMALE, pair("Zephyr", "Zephyr"),
                      expect="female-pair", ear="lean", sheet="flip", ear_if_flagged=take > 1))
    for take in (1, 2, 3):
        e2.append(Req(f"E2b.t{take}", "E2b", "Zephyr+Leda", D_FEMALE, pair("Zephyr", "Leda"),
                      expect="female-pair", ear="lean", sheet="flip", ear_if_flagged=take > 1))
    for take in (2, 3):  # take 1 of E2c is E1b (the identical request)
        e2.append(Req(f"E2c.t{take}", "E2c", "E1b pair again", D_FEMALE, pair("en-gb-advisor-1", second_f),
                      expect="female-pair", ear="lean", sheet="flip", ear_if_flagged=True))
    for take in (1, 2, 3):
        e2.append(Req(f"E2d.t{take}", "E2d", "Zephyr+Orus (does Orus drift high?)", D_MIXED, pair("Zephyr", "Orus"),
                      expect="mixed", ear="lean", sheet="pair", ear_if_flagged=take > 1))
    e2.append(Req("E2d.ctl", "E2d", "Orus alone (control for E2d)", single(ACCENT_TEXT), [("Speaker", "Orus")],
                  expect="male"))
    exps["E2"] = e2
    gb = [("Speaker", "en-gb-advisor-1")]
    exps["E3"] = [
        *[Req(f"E3a.t{t}", "E3a", "tags performed, not read", single(E3A_TEXT), gb, expect="female", ear="full",
              sheet="tags") for t in (1, 2)],
        *[Req(f"E3b.t{t}", "E3b", "more candidate tags", single(E3B_TEXT), gb, expect="female", ear="full",
              sheet="tags") for t in (1, 2)],
        *[Req(f"E3c.t{t}", "E3c", "negative controls [laughs], <medium pause>, <laughter>", single(E3C_TEXT), gb,
              expect="female", ear="full", sheet="tags") for t in (1, 2)],
        *[Req(f"E3d.t{t}", "E3d", "tag and |mhm| backchannel in conversational mode", E3D_TURNS,
              pair("en-gb-advisor-1", "en-gb-assistant-2"), expect="mixed", ear="full", sheet="tags") for t in (1, 2)],
        Req("E3e", "E3e", "E3d on lite", E3D_TURNS, pair("en-gb-advisor-1", "en-gb-assistant-2"), model=LITE_MODEL,
            expect="mixed", ear="full", sheet="tags"),
    ]
    e4_pair = [("Speaker A", "voice_kpd3e297369r"), ("Speaker B", "voice_3wcq2e9jzz1b")]
    exps["E4"] = [
        # Expected 4xx; if accepted after all, the audio is checked like any mixed pair.
        Req("E4a", "E4a", "designed voices in speakers[] (expect 4xx)", SHORT_PAIR, e4_pair, expect_error=True,
            expect="mixed", ear="lean", sheet="pair"),
        Req("E4b", "E4b", "designed + library in speakers[] (expect 4xx)", SHORT_PAIR,
            [("Speaker A", "voice_kpd3e297369r"), ("Speaker B", "en-gb-advisor-1")], expect_error=True,
            expect="mixed", ear="lean", sheet="pair"),
        Req("E4c", "E4c", "designed voice, single voice", single(D_MIXED[1][1]), [("Speaker", "voice_kpd3e297369r")],
            expect=designed_gender.get("voice_kpd3e297369r"), ear="lean"),
        Req("E4e", "E4e", "unknown designed id (expect 4xx)", single("Good morning."),
            [("Speaker", "voice_doesnotexist0000")], expect_error=True),
    ]
    # D_MIXED per turn: prefer the PO's "IELTS Part 1" pair (Jackie F, Danny M), else any F and M.
    a_voice = "voice_3vp96n1h9k8s" if "voice_3vp96n1h9k8s" in designed_f else (designed_f or ["voice_3wcq2e9jzz1b"])[0]
    b_voice = "voice_nva3gh8uk4cg" if "voice_nva3gh8uk4cg" in designed_m else (designed_m or ["voice_kpd3e297369r"])[0]
    for index, (label, text) in enumerate(D_MIXED, start=1):
        voice = a_voice if label == "Speaker A" else b_voice
        exps["E4"].append(Req(f"E4d.{index}", "E4d", f"per-turn dialogue, turn {index} ({label})", single(text),
                              [("Speaker", voice)], expect=designed_gender.get(voice)))
    # Extension (added 2026-10-05 after E4a/E4b were accepted): the same two designed
    # voices as E4d, but in one conversational request, for a like-for-like comparison.
    exps["E4"].append(Req("E4f", "E4f", "designed F+M pair in speakers[] on D_MIXED (E4d voices)", D_MIXED,
                          pair(a_voice, b_voice), expect="mixed", ear="lean", sheet="pair"))
    exps["E5"] = [
        Req("E5a", "E5a", "per-item style, one library voice", [(None, t) for t, _ in E5_ITEMS], gb,
            styles=[s for _, s in E5_ITEMS], expect="female", ear="tone", sheet="emotion"),
        Req("E5b", "E5b", "same items, constant PASSAGE_STYLE (control)", [(None, t) for t, _ in E5_ITEMS], gb,
            expect="female", ear="tone", sheet="emotion"),
        Req("E5c", "E5c", "E1a pair with a style per turn", D_MIXED, pair("en-gb-advisor-1", "en-gb-assistant-2"),
            styles=E5C_STYLES, expect="mixed", ear="tone", sheet="emotion"),
    ]
    exps["E6"] = [
        Req("E6a", "E6a", "Zephyr with language en-GB", single(ACCENT_TEXT), [("Speaker", "Zephyr")],
            voice_extra={"language": "en-GB"}, expect="female", ear="lean", sheet="accent_ab"),
        Req("E6b", "E6b", "Zephyr without language (control)", single(ACCENT_TEXT), [("Speaker", "Zephyr")],
            expect="female", ear="lean", sheet="accent_ab"),
    ]
    exps["E7"] = [Req(f"E7.s{seed}.{n}", "E7", f"seed {seed}", single(E7_TEXT), gb, gen_extra={"seed": seed},
                      expect="female") for n, seed in ((1, 42), (2, 42), (3, 43))]
    exps["E8"] = [Req(f"E8.{n}", "E8", f"drift across requests, text {n}", single(t), gb, expect="female")
                  for n, t in enumerate(E8_TEXTS, start=1)]
    exps["E10"] = [Req("E10", "E10", "E1a on lite", D_MIXED, pair("en-gb-advisor-1", "en-gb-assistant-2"),
                       model=LITE_MODEL, expect="mixed", ear="lean", sheet="pair")]
    return exps


def run_speech(run: Run, req: Req, wav_dir: Path, use_ear: bool) -> dict:
    body = req.body()
    estimate = estimate_speech(req.model, [t for _, t in req.turns], len(req.voices))
    say(f"{req.rid}: {req.purpose}")
    entry, data = run.call(req.rid, req.purpose, req.model, "POST", "/interactions", body=body,
                           estimate=estimate, timeout=TTS_TIMEOUT_S)
    entry.update({"exp": req.exp, "voices": [v for _, v in req.voices], "expect": req.expect, "sheet": req.sheet})
    if entry["status"] == 200 and isinstance(data, dict):
        try:
            samples = output_audio(data)
        except ValueError as e:
            entry["audio_error"] = str(e)
            run.save()
            return entry
        path = wav_dir / f"{req.rid}.wav"
        path.write_bytes(wav_bytes(samples))
        entry["wav"] = str(path.relative_to(run.folder))
        entry["duration_s"] = round(len(samples) / SAMPLE_RATE, 2)
        entry["sha256_pcm"] = __import__("hashlib").sha256(samples.tobytes()).hexdigest()
        entry["response_shape"] = {k: v for k, v in response_shape(data).items() if k not in ("steps",)}
        entry["audio_items"] = [{k: v for k, v in item.items() if k != "data"} for step in model_outputs(data)
                                for item in step.get("content") or [] if item.get("type") == "audio"]
        if entry["usage"]:
            tokens = int(entry["usage"].get("total_output_tokens") or 0)
            entry["audio_tokens_per_s"] = round(tokens / max(entry["duration_s"], 0.01), 1)
        if req.expect:
            entry["f0"] = f0_report(samples, SAMPLE_RATE, req.expect)
            say(f"    F0: {f0_line(entry['f0'])}")
        run.save()
        flagged = bool(entry.get("f0", {}).get("flags"))
        if req.ear and use_ear and (not req.ear_if_flagged or flagged):
            run_ear(run, f"{req.rid}.ear", path, req.ear, entry)
    else:
        if entry.get("error_body"):
            say(f"    error body: {entry['error_body'][:300]}")
    return entry


def run_ear(run: Run, rid: str, wav_path: Path, mode: str, attach_to: dict | None = None) -> dict | None:
    wav = wav_path.read_bytes()
    seconds = (len(wav) - 44) / (2 * SAMPLE_RATE)
    body = ear_body(wav, mode)
    entry, data = run.call(rid, f"AI ear ({mode}) on {wav_path.name}", EAR_MODEL, "POST", "/interactions",
                           body=body, estimate=estimate_ear(seconds, mode), timeout=TEXT_TIMEOUT_S)
    parsed = None
    if entry["status"] == 200 and isinstance(data, dict):
        text = output_text(data) or ""
        try:
            parsed = json.loads(strip_fences(text))
        except ValueError:
            entry["ear_raw"] = text[:3000]
    entry["ear"] = parsed
    if attach_to is not None:
        attach_to["ear"] = parsed
        attach_to["ear_request"] = rid
    run.save()
    say(f"    ear: {ear_summary(parsed)}")
    return parsed


def join_wavs(paths: list[Path], gap_ms: int) -> np.ndarray:
    parts = []
    for i, p in enumerate(paths):
        if i:
            parts.append(silence(gap_ms))
        parts.append(read_wav(p)[0])
    return np.concatenate(parts)


def cmd_probe(args) -> None:
    if args.max_usd is None:
        raise SystemExit("probe makes paid requests: pass --max-usd (e.g. --max-usd 0.30).")
    check_speech_body()
    out = Path(args.out)
    catalog = load_catalog(out)
    prompted = load_prompted(out)
    key, source = find_key()
    say(f"key from {source}")
    api = Api(key)
    folder = out / "probe"
    wav_dir = folder / "wav"
    wav_dir.mkdir(parents=True, exist_ok=True)
    run = Run(folder, api, args.max_usd, "probe")
    exps = experiments(catalog, prompted)
    wanted = [w.strip().upper() for w in (args.only.split(",") if args.only else [])]
    groups = ["E4", "E1", "E2", "E3", "E9", "E10", "E5", "E7", "E8", "E6"]
    if args.only:
        groups = [g for g in groups if g in wanted or any(w.startswith(g) and w[len(g):len(g) + 1].isalpha() for w in wanted)]
    estimate = sum(estimate_speech(r.model, [t for _, t in r.turns], len(r.voices)) for g in groups if g in exps
                   for r in exps[g] if _selected(r, wanted))
    ear_estimate = sum(estimate_ear(sum(len(t.split()) for _, t in r.turns) / 2.4, r.ear) for g in groups if g in exps
                       for r in exps[g] if r.ear and _selected(r, wanted)) if not args.no_ear else 0
    if not args.reanalyse:
        say(f"estimate: speech {usd(estimate)}, ear {usd(ear_estimate)}; folder spend so far {usd(run.ledger.prior)}; "
            f"cap {usd(run.ledger.cap)}")
    done: list[str] = []
    try:
        for group in groups:
            if group == "E9":
                if args.design and (not wanted or "E9" in wanted):
                    probe_design(run, wav_dir, not args.no_ear)
                    done.append("E9")
                continue
            for req in exps.get(group, []):
                if not _selected(req, wanted):
                    continue
                if args.reanalyse:
                    reanalyse(run, req, not args.no_ear)
                    continue
                if args.skip_done and run.entry(req.rid) and run.entry(req.rid).get("status") in (200, 400, 403, 404):
                    continue
                run_speech(run, req, wav_dir, not args.no_ear)
            if args.reanalyse:
                if group == "E8":
                    post_e8(run, wav_dir, False)
                done.append(group)
                continue
            if group == "E4":
                post_e4d(run, wav_dir, not args.no_ear)
            if group == "E8":
                post_e8(run, wav_dir, not args.no_ear)
            if group == "E7":
                post_e7(run)
            done.append(group)
    except BudgetStop as stop:
        say(f"STOPPED: {stop}")
        run.manifest["runs"][-1]["stopped"] = str(stop)
        run.manifest["runs"][-1]["unfinished"] = [g for g in groups if g not in done]
        run.save()
    run.manifest["runs"][-1]["finished"] = dt.datetime.now().isoformat(timespec="seconds")
    run.save()
    write_probe_report(run)
    write_probe_sheet(run)
    say(f"spent in folder: {usd(run.ledger.spent)} (cap {usd(run.ledger.cap)})")


def reanalyse(run: Run, req: Req, use_ear: bool) -> None:
    """F0 again (current thresholds) and the ear if it has not answered yet; no new synthesis."""
    entry = run.entry(req.rid)
    if not entry or not entry.get("wav"):
        return
    samples, rate = read_wav(run.folder / entry["wav"])
    entry["expect"], entry["sheet"] = req.expect, req.sheet
    if req.expect:
        entry["f0"] = f0_report(samples, rate, req.expect)
        say(f"{req.rid} F0: {f0_line(entry['f0'])}")
    run.save()
    if use_ear and req.ear and not entry.get("ear"):
        run_ear(run, f"{req.rid}.ear", run.folder / entry["wav"], req.ear, entry)


def _selected(req: Req, wanted: list[str]) -> bool:
    if not wanted:
        return True
    rid = req.rid.upper()
    exp = req.exp.upper()
    for w in wanted:
        if w == rid or w == exp or (rid.startswith(w) and not rid[len(w):len(w) + 1].isdigit()):
            return True
    return False


def post_e4d(run: Run, wav_dir: Path, use_ear: bool) -> None:
    parts = [run.entry(f"E4d.{i}") for i in range(1, 5)]
    if not all(p and p.get("wav") for p in parts):
        return
    previous = run.entry("E4d.joined")
    if previous and previous.get("ear") and previous.get("parts_at") == [p.get("at") for p in parts]:
        return
    joined = join_wavs([run.folder / p["wav"] for p in parts], CHUNK_GAP_MS)
    path = wav_dir / "E4d.joined.wav"
    path.write_bytes(wav_bytes(joined))
    entry = {"id": "E4d.joined", "exp": "E4d", "purpose": "the 4 per-turn requests joined with 350 ms",
             "derived": True, "wav": str(path.relative_to(run.folder)),
             "duration_s": round(len(joined) / SAMPLE_RATE, 2), "micro_usd": 0, "sheet": "joins",
             "requests": 4, "wall_ms_total": sum(p.get("wall_ms") or 0 for p in parts),
             "latency_ms": [p.get("latency_ms") for p in parts], "http_429": sum(p.get("http_429") or 0 for p in parts),
             "voices": sorted({v for p in parts for v in p.get("voices") or []}),
             "parts_at": [p.get("at") for p in parts]}
    entry["f0"] = f0_report(joined, SAMPLE_RATE, "mixed")
    run.record(entry)
    if use_ear:
        run_ear(run, "E4d.joined.ear", path, "lean", entry)


E8_SUBSTITUTE = ["E8.1", "E7.s43.3", "E5b"]


def post_e8(run: Run, wav_dir: Path, use_ear: bool) -> None:
    parts = [run.entry(f"E8.{i}") for i in range(1, 4)]
    if not all(p and p.get("wav") for p in parts):
        post_e8_substitute(run, wav_dir)
        return
    previous = run.entry("E8.joined")
    if previous and previous.get("ear") and previous.get("parts_at") == [p.get("at") for p in parts]:
        return
    joined = join_wavs([run.folder / p["wav"] for p in parts], 1000)
    path = wav_dir / "E8.joined.wav"
    path.write_bytes(wav_bytes(joined))
    medians = [p["f0"]["median_hz"] for p in parts if p.get("f0")]
    spread = (max(medians) - min(medians)) / min(medians) if medians else None
    entry = {"id": "E8.joined", "exp": "E8", "purpose": "3 single-voice requests joined with 1 s gaps",
             "derived": True, "wav": str(path.relative_to(run.folder)), "micro_usd": 0, "sheet": "same",
             "duration_s": round(len(joined) / SAMPLE_RATE, 2), "f0_medians": medians,
             "f0_spread": round(spread, 3) if spread is not None else None, "voices": ["en-gb-advisor-1"],
             "parts_at": [p.get("at") for p in parts]}
    run.record(entry)
    if use_ear:
        run_ear(run, "E8.joined.ear", path, "lean", entry)


def post_e8_substitute(run: Run, wav_dir: Path) -> None:
    """E8 unfinished (budget): the same question asked of separate single-voice
    en-gb-advisor-1 requests that already exist, all with PASSAGE_STYLE and no
    tags. Free: no synthesis, no ear."""
    parts = [run.entry(rid) for rid in E8_SUBSTITUTE]
    if not all(p and p.get("wav") for p in parts):
        return
    joined = join_wavs([run.folder / p["wav"] for p in parts], 1000)
    path = wav_dir / "E8.substitute.wav"
    path.write_bytes(wav_bytes(joined))
    singles = [r for r in run.manifest["requests"] if r.get("voices") == ["en-gb-advisor-1"] and r.get("f0")
               and r.get("model") == TTS_MODEL and r["id"].split(".")[0] in ("E5b", "E7", "E8")]
    medians = {r["id"]: r["f0"]["median_hz"] for r in singles}
    values = list(medians.values())
    spread = (max(values) - min(values)) / min(values) if values else None
    run.record({"id": "E8.substitute", "exp": "E8", "derived": True, "micro_usd": 0, "sheet": "same",
                "purpose": "E8 unfinished: " + " + ".join(E8_SUBSTITUTE) + " joined with 1 s gaps (free substitute)",
                "wav": str(path.relative_to(run.folder)), "duration_s": round(len(joined) / SAMPLE_RATE, 2),
                "voices": ["en-gb-advisor-1"], "f0_medians": medians,
                "f0_spread": round(spread, 3) if spread is not None else None})
    say(f"  E8 substitute: medians {medians}, spread {spread:.1%}")


def post_e7(run: Run) -> None:
    takes = [run.entry(f"E7.s42.1"), run.entry("E7.s42.2"), run.entry("E7.s43.3")]
    if not all(t and t.get("wav") for t in takes):
        result = {"accepted": [t.get("status") if t else None for t in takes]}
    else:
        pcm = [read_wav(run.folder / t["wav"])[0].astype(np.float64) for t in takes]

        def corr(a, b):
            n = min(len(a), len(b))
            if n == 0:
                return None
            return round(float(np.corrcoef(a[:n], b[:n])[0, 1]), 4)
        result = {"accepted": [t.get("status") for t in takes],
                  "identical_42_42": takes[0]["sha256_pcm"] == takes[1]["sha256_pcm"],
                  "identical_42_43": takes[0]["sha256_pcm"] == takes[2]["sha256_pcm"],
                  "lengths": [len(p) for p in pcm],
                  "corr_42_42": corr(pcm[0], pcm[1]), "corr_42_43": corr(pcm[0], pcm[2])}
    run.record({"id": "E7.compare", "exp": "E7", "purpose": "seed comparison", "derived": True, "micro_usd": 0,
                "result": result})
    say(f"  E7: {result}")


def probe_design(run: Run, wav_dir: Path, use_ear: bool) -> None:
    say("E9: Voice Design create (store:true; the voice is kept)")
    existing = run.entry("E9.create")
    voice_id = None
    if existing and existing.get("voice_id"):
        voice_id = existing["voice_id"]
        say(f"  E9 already created {voice_id}; not creating another")
    else:
        entry, data = run.call("E9.create", "POST /v1beta/voices (prompted, store:true)", "voice-design", "POST",
                               "/voices", body=E9_DESIGN, estimate=15_000, timeout=TTS_TIMEOUT_S)
        entry["exp"] = "E9"
        if entry["status"] == 200 and isinstance(data, dict):
            voice = data.get("voice") if isinstance(data.get("voice"), dict) and "id" not in data else data
            voice_id = voice.get("id")
            entry["voice_id"] = voice_id
            entry["response_shape"] = truncate_audio(data)
            usage = voice.get("usage") or data.get("usage")
            if usage and not entry.get("usage"):  # run.call already priced a top-level usage
                entry["usage"] = usage
                micro, priced = usage_micro_usd("voice-design", usage)
                entry["micro_usd"], entry["priced"] = micro, priced
                run.ledger.add(micro)
            sample = voice.get("sample_audio") or (voice.get("prompted") or {}).get("sample_audio")
            if sample and sample.get("data"):
                raw = base64.b64decode(sample["data"])
                path = wav_dir / "E9.sample.wav"
                path.write_bytes(raw)
                entry["sample_audio"] = {"mime_type": sample.get("mime_type"), "bytes": len(raw),
                                         "wav": str(path.relative_to(run.folder))}
                try:
                    samples, rate = parse_wav(raw)
                    entry["sample_audio"].update({"rate": rate, "duration_s": round(len(samples) / rate, 2)})
                    if rate == SAMPLE_RATE:
                        entry["sample_audio"]["f0"] = f0_report(samples, rate, "female")
                except ValueError as e:
                    entry["sample_audio"]["parse_error"] = str(e)
            run.save()
            say(f"  created {voice_id}; usage {usage}")
    if not voice_id:
        return
    if voice_id in PROTECTED_VOICES:
        raise SystemExit("refusing: E9 returned a protected voice id")
    req = Req("E9.turn", "E9", f"one turn with the designed voice {voice_id}", [(None, E9_TEXT)],
              [("Speaker", voice_id)], expect="female", ear="lean", sheet="designed")
    if not (run.entry("E9.turn") and run.entry("E9.turn").get("wav")):
        run_speech(run, req, wav_dir, use_ear)


# ---------------------------------------------------------------- probe report

def _fmt_usage(u) -> str:
    if not u:
        return "-"
    return f"in {u.get('total_input_tokens', 0)}/out {u.get('total_output_tokens', 0)}/think {u.get('total_thought_tokens', 0)}"


def write_probe_report(run: Run) -> None:
    reqs = run.manifest["requests"]
    latest: dict[str, dict] = {}
    for r in reqs:
        latest[r["id"]] = r
    gates = run.folder / "gates.md"
    unfinished = [r.get("unfinished") for r in run.manifest["runs"] if r.get("unfinished")]
    lines = [f"# Voice probe report ({dt.date.today().isoformat()})", ""]
    if unfinished:
        lines += [f"**Stopped at the cap** in a run; unfinished then: {unfinished[-1]}.", ""]
    if gates.is_file():
        lines += [gates.read_text(encoding="utf-8").strip(), "", "---", ""]
    lines += [
             f"Tool: `tools/voice_lab.py probe`. Folder spend: {usd(sum(int(r.get('micro_usd') or 0) for r in reqs))} "
             f"over {sum(1 for r in reqs if not r.get('derived'))} requests "
             f"(speech {usd(sum(int(r.get('micro_usd') or 0) for r in reqs if r.get('model') in (TTS_MODEL, LITE_MODEL)))}, "
             f"ear {usd(sum(int(r.get('micro_usd') or 0) for r in reqs if r.get('model') == EAR_MODEL))}, "
             f"design {usd(sum(int(r.get('micro_usd') or 0) for r in reqs if r.get('model') == 'voice-design'))}).",
             "", "Rates: intro 3.8 rates (TTS $0.50/M in, $9/M out; lite $6/M out; 3.8 Flash $0.75/M in, $3.75/M out; "
             "ear audio input assumed $1/M). F0: YIN 40 ms / 10 ms, threshold 0.15, 65-400 Hz; islands split by "
             ">= 200 ms silence, >= 1 s and >= 30 voiced frames; flags: female < 150 Hz, male > 190 Hz, "
             "mixed pairs by 2-means (> 40 Hz apart).", "",
             "## Requests", "",
             "| id | status | model | voices | s | latency ms | usage | cost | F0 | ear |", "|---|---|---|---|---|---|---|---|---|---|"]
    for rid, r in latest.items():
        if (r.get("model") == EAR_MODEL and rid.endswith(".ear")) or r.get("superseded"):
            continue
        f0 = r.get("f0")
        f0s = "-" if not f0 else f"{f0['median_hz']} Hz, {len(f0['islands'])} isl, {len(f0['flags'])} flag"
        if f0 and f0.get("clusters"):
            f0s += f", sep {f0['clusters']['separation_hz']}"
        lines.append(f"| {rid} | {r.get('status', '-')} | {r.get('model') or '-'} | {', '.join(r.get('voices') or [])} | "
                     f"{r.get('duration_s', '-')} | {r.get('latency_ms', '-')} | {_fmt_usage(r.get('usage'))} | "
                     f"{usd(int(r.get('micro_usd') or 0))} | {f0s} | {html.escape(ear_summary(r.get('ear')))[:160] if r.get('ear') else '-'} |")
    lines += ["", "## Errors (scrubbed bodies)", ""]
    for rid, r in latest.items():
        if r.get("error_body"):
            lines += [f"### {rid} -> {r['status']}", "", "```json", r["error_body"], "```", ""]
    lines += ["## F0 islands", ""]
    for rid, r in latest.items():
        if r.get("f0"):
            lines.append(f"- **{rid}** ({r.get('expect')}): {f0_line(r['f0'])}")
            if r["f0"]["flags"]:
                lines.append("  - FLAGGED: " + ", ".join(f"{i['start']}-{i['end']} s {i['median_hz']} Hz ({i.get('severity')})"
                                                      for i in r['f0']['flags']))
    lines += ["", "## AI ear answers", ""]
    for rid, r in latest.items():
        if r.get("ear"):
            lines.append(f"- **{rid}**: {ear_summary(r['ear'])}")
            if r["ear"].get("transcript"):
                lines.append(f"  - transcript: {r['ear']['transcript']}")
            if r["ear"].get("tone"):
                lines.append(f"  - tone: {r['ear']['tone']}")
    for rid in ("E7.compare", "E8.joined", "E8.substitute", "E4d.joined", "E9.create"):
        r = latest.get(rid)
        if r:
            show = {k: v for k, v in r.items() if k not in ("body", "f0")}
            lines += ["", f"## {rid}", "", "```json", json.dumps(show, indent=2, ensure_ascii=False)[:6000], "```"]
    write_text(run.folder / "report.md", "\n".join(lines) + "\n")


# ---------------------------------------------------------------- listening sheets

SHEET_QUESTIONS = {
    "pair": [
        ("speakers", "number", "Bạn nghe thấy mấy người nói?"),
        ("g1", "choice:Nữ|Nam|Không rõ", "Người nói thứ nhất: giới tính?"),
        ("a1", "accent", "Người nói thứ nhất: giọng vùng nào?"),
        ("g2", "choice:Nữ|Nam|Không rõ", "Người nói thứ hai: giới tính?"),
        ("a2", "accent", "Người nói thứ hai: giọng vùng nào?"),
        ("change", "choice:Có|Không", "Có ai đổi giọng hay đổi giới tính giữa chừng không?"),
    ],
    "flip": [
        ("male", "choice:Có|Không", "Có người nói nào nghe như giọng nam không?"),
        ("when", "text", "Ở giây thứ mấy? (vd 0:12)"),
        ("speakers", "number", "Bạn nghe thấy mấy người nói?"),
    ],
    "tags": [
        ("spoken", "choice:Có|Không", "Có nghe thấy chữ như 'laugh', 'sigh', 'pause', 'mhm' bị đọc thành lời không?"),
        ("which", "text", "Nếu có: chữ nào, ở giây thứ mấy?"),
        ("natural", "scale", "Tiếng cười, thở dài có tự nhiên không? (1 = rất giả, 5 = rất tự nhiên)"),
    ],
    "joins": [
        ("join_x", "scale", "Mẫu X: chỗ nối giữa các lượt có tự nhiên không? (1–5)"),
        ("join_y", "scale", "Mẫu Y: chỗ nối giữa các lượt có tự nhiên không? (1–5)"),
        ("usable", "choice:X|Y|Cả hai|Không bản nào", "Bản nào dùng được cho học sinh?"),
    ],
    "emotion": [
        ("emotions", "textarea", "Mỗi câu nghe cảm xúc gì? (mỗi câu một dòng)"),
        ("over", "choice:Có|Không", "Có quá lố cho đề thi không?"),
    ],
    "accent_ab": [
        ("british", "choice:X|Y|Như nhau", "Mẫu nào giọng Anh (England) hơn?"),
    ],
    "same": [
        ("same", "choice:Có|Không|Không chắc", "Ba đoạn này có phải cùng một người nói không?"),
    ],
    "designed": [
        ("teacher", "scale", "Giọng này nghe như một cô giáo người Anh (England) không? (1–5)"),
        ("gender", "choice:Nữ|Nam|Không rõ", "Giới tính nghe được?"),
    ],
}
ACCENT_CHOICES = ["Anh (England)", "Scotland", "Ireland", "Mỹ", "Canada", "Úc", "New Zealand", "Nam Phi", "Ấn Độ",
                  "Khác / không rõ"]


def write_probe_sheet(run: Run, seed: int | None = None) -> None:
    latest: dict[str, dict] = {}
    for r in run.manifest["requests"]:
        latest[r["id"]] = r
    seed = seed if seed is not None else int(run.manifest.get("sheet_seed") or random.SystemRandom().randrange(1, 10**6))
    run.manifest["sheet_seed"] = seed
    rng = random.Random(seed)
    sheet_dir = run.folder / "sheet1"
    if sheet_dir.exists():
        shutil.rmtree(sheet_dir)
    sheet_dir.mkdir(parents=True)
    singles = [r for r in latest.values() if r.get("wav") and not r.get("superseded")
               and r.get("sheet") in ("pair", "flip", "tags", "emotion", "same", "designed")]
    groups = []
    for r in singles:
        groups.append({"kind": r["sheet"], "sources": [r]})
    # Paired comparisons (X/Y in random order).
    for other in ("E1a", "E4f"):  # E4f: the same designed voices in one conversational request
        if latest.get("E4d.joined", {}).get("wav") and latest.get(other, {}).get("wav"):
            groups.append({"kind": "joins", "sources": [latest["E4d.joined"], latest[other]]})
    if latest.get("E6a", {}).get("wav") and latest.get("E6b", {}).get("wav"):
        groups.append({"kind": "accent_ab", "sources": [latest["E6a"], latest["E6b"]]})
    if latest.get("E9.create", {}).get("sample_audio", {}).get("wav"):
        e9 = latest["E9.create"]
        groups.append({"kind": "designed", "sources": [{"id": "E9.sample", "wav": e9["sample_audio"]["wav"],
                                                         "voices": [e9.get("voice_id")]}]})
    rng.shuffle(groups)
    tokens = list(range(1, 1 + sum(len(g["sources"]) for g in groups)))
    rng.shuffle(tokens)
    items, key = [], {"seed": seed, "sheet": "probe", "items": []}
    for n, group in enumerate(groups, start=1):
        sources = list(group["sources"])
        if len(sources) == 2:
            rng.shuffle(sources)
        files = []
        for src in sources:
            name = f"s{tokens.pop():02d}.wav"
            shutil.copyfile(run.folder / src["wav"], sheet_dir / name)
            files.append(name)
        qid = f"q{n:02d}"
        items.append({"id": qid, "kind": group["kind"], "files": files})
        key["items"].append({"id": qid, "kind": group["kind"],
                             "files": {f: {"request": s["id"], "voices": s.get("voices")} for f, s in zip(files, sources)}})
    write_json(run.folder / "key.json", key)
    run.save()
    intro = ("Phiếu nghe 1 — thăm dò giọng đọc. Mỗi mục có một hoặc hai mẫu âm thanh tiếng Anh (tên file là mã ngẫu nhiên, "
             "nên bạn không biết mẫu nào dùng giọng nào). Hãy nghe bằng tai nghe, trả lời các câu hỏi, rồi bấm "
             "“Tải kết quả (JSON)” và gửi file đó lại. Câu trả lời được lưu tự động trong trình duyệt này, nên bạn có thể "
             "dừng và quay lại sau. Khoảng 15 phút.")
    write_text(sheet_dir / "sheet.html", sheet_html("Phiếu nghe 1 — thăm dò", intro, items, f"voice-lab-sheet1-{seed}",
                                                    seed, "probe"))
    say(f"listening sheet: {sheet_dir / 'sheet.html'} ({len(items)} items; key in {run.folder / 'key.json'})")


def sheet_html(title: str, intro: str, items: list[dict], storage_key: str, seed: int, sheet: str,
               questions: dict | None = None, sections: dict | None = None) -> str:
    """Items may carry a `label` (shown after "Muc n") and a `section` (a heading from
    `sections` is shown where the section changes)."""
    data = {"items": items, "questions": questions or SHEET_QUESTIONS, "accents": ACCENT_CHOICES,
            "storageKey": storage_key, "seed": seed, "sheet": sheet, "sections": sections or {}}
    payload = json.dumps(data, ensure_ascii=False).replace("</", "<\\/")
    return SHEET_TEMPLATE.replace("__TITLE__", html.escape(title)).replace("__INTRO__", html.escape(intro)) \
        .replace("__DATA__", payload)


SHEET_TEMPLATE = """<!doctype html>
<html lang="vi">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>__TITLE__</title>
<style>
:root { --bg:#f7f7f5; --card:#ffffff; --ink:#1d1d1b; --muted:#5f5f5a; --line:#dcdcd6; --accent:#1f5fae; --ok:#2f7d32; }
@media (prefers-color-scheme: dark) { :root { --bg:#161615; --card:#21211f; --ink:#ececea; --muted:#a6a6a0; --line:#3a3a37; --accent:#7fb0ff; --ok:#7cc47f; } }
* { box-sizing: border-box; }
body { margin:0; background:var(--bg); color:var(--ink); font:16px/1.5 system-ui, -apple-system, "Segoe UI", Roboto, sans-serif; }
main { max-width: 760px; margin: 0 auto; padding: 24px 16px 96px; }
h1 { font-size: 1.4rem; margin: 0 0 8px; }
p.intro { color: var(--muted); margin: 0 0 20px; }
h2.part { font-size: 1.2rem; margin: 32px 0 6px; padding-top: 12px; border-top: 2px solid var(--line); }
section.item { background: var(--card); border: 1px solid var(--line); border-radius: 10px; padding: 16px; margin: 0 0 16px; }
section.item h2 { font-size: 1.05rem; margin: 0 0 10px; display:flex; justify-content:space-between; gap:8px; }
section.item h2 .done { color: var(--ok); font-weight: 600; font-size: .9rem; }
.clip { display:flex; align-items:center; gap:10px; margin: 6px 0; flex-wrap: wrap; }
.clip b { min-width: 1.5em; }
audio { width: 100%; max-width: 520px; }
.q { margin: 12px 0 0; }
.q label.t { display:block; font-weight: 600; margin-bottom: 4px; }
.opts { display:flex; flex-wrap: wrap; gap: 6px 14px; }
.opts label { display:inline-flex; align-items:center; gap:4px; cursor:pointer; }
input[type=text], input[type=number], textarea, select { font: inherit; color: var(--ink); background: var(--bg); border:1px solid var(--line); border-radius: 6px; padding: 6px 8px; width: 100%; max-width: 520px; }
input[type=number] { max-width: 100px; }
textarea { min-height: 70px; }
.bar { position: fixed; left:0; right:0; bottom:0; background: var(--card); border-top:1px solid var(--line); padding: 10px 16px; display:flex; gap:12px; align-items:center; justify-content:center; flex-wrap: wrap; }
button { font: inherit; padding: 8px 14px; border-radius: 8px; border: 1px solid var(--accent); background: var(--accent); color: #fff; cursor: pointer; }
button.ghost { background: transparent; color: var(--accent); }
.progress { color: var(--muted); }
</style>
</head>
<body>
<main>
<h1>__TITLE__</h1>
<p class="intro">__INTRO__</p>
<div id="items"></div>
</main>
<div class="bar"><span class="progress" id="progress"></span><button id="export">Tải kết quả (JSON)</button><button class="ghost" id="reset">Xoá câu trả lời</button></div>
<script>
const DATA = __DATA__;
let answers = {};
try { answers = JSON.parse(localStorage.getItem(DATA.storageKey) || "{}"); } catch (e) { answers = {}; }
function save() { try { localStorage.setItem(DATA.storageKey, JSON.stringify(answers)); } catch (e) {} progress(); }
function progress() {
  let done = 0;
  for (const item of DATA.items) {
    const a = answers[item.id] || {};
    const qs = DATA.questions[item.kind] || [];
    const complete = qs.length && qs.every(([k, t]) => t === "text" || t === "textarea" || (a[k] !== undefined && a[k] !== ""));
    const mark = document.getElementById("done-" + item.id);
    if (mark) mark.textContent = complete ? "đã trả lời" : "";
    if (complete) done++;
  }
  document.getElementById("progress").textContent = done + " / " + DATA.items.length + " mục";
}
function el(tag, attrs, ...kids) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) { if (k === "text") e.textContent = v; else e.setAttribute(k, v); }
  for (const k of kids) if (k) e.appendChild(k);
  return e;
}
function question(item, [key, type, label]) {
  const a = answers[item.id] = answers[item.id] || {};
  const box = el("div", {class: "q"});
  box.appendChild(el("label", {class: "t", text: label}));
  const set = (v) => { a[key] = v; save(); };
  const name = item.id + "-" + key;
  if (type.startsWith("choice:") || type === "scale") {
    const opts = type === "scale" ? ["1", "2", "3", "4", "5"] : type.slice(7).split("|");
    const row = el("div", {class: "opts"});
    for (const o of opts) {
      const input = el("input", {type: "radio", name, value: o});
      if (a[key] === o) input.checked = true;
      input.addEventListener("change", () => set(o));
      row.appendChild(el("label", {}, input, document.createTextNode(o)));
    }
    box.appendChild(row);
  } else if (type === "accent") {
    const sel = el("select", {});
    sel.appendChild(el("option", {value: "", text: "— chọn —"}));
    for (const o of DATA.accents) { const op = el("option", {value: o, text: o}); if (a[key] === o) op.selected = true; sel.appendChild(op); }
    sel.addEventListener("change", () => set(sel.value));
    box.appendChild(sel);
  } else if (type === "textarea") {
    const t = el("textarea", {}); t.value = a[key] || ""; t.addEventListener("input", () => set(t.value)); box.appendChild(t);
  } else {
    const t = el("input", {type: type === "number" ? "number" : "text"});
    if (type === "number") { t.min = "1"; t.max = "6"; }
    t.value = a[key] || ""; t.addEventListener("input", () => set(t.value)); box.appendChild(t);
  }
  return box;
}
const root = document.getElementById("items");
let lastSection = null;
DATA.items.forEach((item, n) => {
  if (item.section && item.section !== lastSection && DATA.sections && DATA.sections[item.section]) {
    lastSection = item.section;
    const part = DATA.sections[item.section];
    root.appendChild(el("h2", {class: "part", text: part.title}));
    if (part.intro) root.appendChild(el("p", {class: "intro", text: part.intro}));
  }
  const sec = el("section", {class: "item"});
  const h = el("h2", {}, el("span", {text: "Mục " + (n + 1) + (item.label ? " — " + item.label : "")}), el("span", {class: "done", id: "done-" + item.id}));
  sec.appendChild(h);
  const labels = item.files.length === 2 ? ["X", "Y"] : [""];
  item.files.forEach((f, i) => {
    const clip = el("div", {class: "clip"});
    if (labels[i]) clip.appendChild(el("b", {text: labels[i]}));
    clip.appendChild(el("audio", {controls: "", preload: "none", src: f}));
    sec.appendChild(clip);
  });
  for (const q of (DATA.questions[item.kind] || [])) sec.appendChild(question(item, q));
  sec.appendChild(question(item, ["comment", "textarea", "Ghi chú (không bắt buộc)"]));
  root.appendChild(sec);
});
progress();
document.getElementById("export").addEventListener("click", () => {
  const out = {sheet: DATA.sheet, seed: DATA.seed, exported_at: new Date().toISOString(), answers};
  const blob = new Blob([JSON.stringify(out, null, 2)], {type: "application/json"});
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = "answers-" + DATA.sheet + "-" + DATA.seed + ".json";
  document.body.appendChild(a); a.click(); a.remove();
});
document.getElementById("reset").addEventListener("click", () => {
  if (confirm("Xoá mọi câu trả lời trên phiếu này?")) { answers = {}; save(); location.reload(); }
});
</script>
</body>
</html>
"""


# ---------------------------------------------------------------- audition

# The audition reads ACCENT_TEXT once per candidate in a single-voice request with a
# short neutral style; announcer candidates read ANNOUNCE_TEXT with ANNOUNCEMENT_STYLE.
AUDITION_STYLE = "clear, at a steady exam pace"
AUDITION_MAX_PER_CELL = 6
# Smallest pool per accent (voices.rs minimum_pool): British 4 (HSG has three British
# speakers of one gender, plus one for "Doi giong khac"), core 3, accents new in 0.8 2.
POOL_MINIMUM = {"british": 4, "american": 3, "australian": 3, "canadian": 3, "newzealand": 3,
                "irish": 2, "scottish": 2, "southafrican": 2, "indian": 2}
EAR_ACCENT = {"british": "England", "american": "USA", "australian": "Australia", "canadian": "Canada",
              "newzealand": "New Zealand", "irish": "Ireland", "scottish": "Scotland",
              "southafrican": "South Africa", "indian": "India", "announcer": "England"}
EAR_ACCENT_CHOICES = ["England", "Scotland", "Ireland", "USA", "Canada", "Australia", "New Zealand",
                      "South Africa", "India"]
ACCENT_NAMES = {"british": "British", "american": "American", "australian": "Australian", "canadian": "Canadian",
                "newzealand": "New Zealand", "irish": "Irish", "scottish": "Scottish",
                "southafrican": "South African", "indian": "Indian"}
# Voices the probe used most (E1a, E3d, E5c, E10): always auditioned, for comparison.
AUDITION_PINNED = {("british", "female"): ["en-gb-advisor-1"], ("british", "male"): ["en-gb-assistant-2"]}
# G1 (E1c): a classic + library pair was accepted in one conversational request, so the
# American cell may hold classics. One per gender is auditioned; it doubles as the
# AI ear's calibration control (the catalog calls every classic General American).
AMERICAN_CLASSICS = 1
ANNOUNCER_CANDIDATES = 3
ANNOUNCER_CONTROL = "charon"  # the 0.7 announcer (General American): a blind control, never proposed
CONFIDENCE_RANK = {"low": 0, "medium": 1, "high": 2}

# Exam fit from catalog metadata: persona role, context, the description's tone and
# setting ("Currently on the phone" ...), age and, for en-US, region.
_POOR_ROLES = ("influencer", "fashion consultant", "cooking show host", "trivia host", "reality tv", "comedian",
               "dating coach", "fitness trainer", "nutritionist", "driving directions", "advertising")
_EDU_ROLES = ("teacher", "tutor", "professor", "librarian", "mentor", "researcher", "instructional video host")
_TALK_ROLES = ("concierge", "tour guide", "executive assistant", "customer service", "customer support",
               "social worker", "doctor", "lawyer", "financial advisor", "podcast", "radio host", "journalist",
               "parent", "sibling", "friend", "aunt/uncle", "event planner", "sales associate", "architect",
               "tech support", "counselor", "therapist", "philosopher", "narrator", "grandparent", "news",
               "companion", "advisor")
_TONES = {"natural and clear": 2.0, "clear, medium-pitch, and friendly": 2.0, "objective, direct, and helpful": 1.5,
          "professional yet approachable": 1.0, "warm and engaging": 1.0, "highly approachable": 1.0,
          "crisp and eager": 1.0, "engaging and empathic": 1.0, "reflective, calm, and reassuring": 1.0,
          "relaxed": 0.5, "cool, confident, and collaborative": 0.5, "peer-to-peer and curious": 0.5,
          "textured, resonant, and soothing": 0.5, "resonant and witty": 0.5, "light, airy, and precise": 0.5,
          "conversational, slightly dry humor, and warm": 0.5, "bright, breezy, and youthful": 0.5,
          "enthusiastic, engaging, and fun": 0.0, "deep, velvety, and unhurried": 0.0,
          "laid-back, encouraging, and chill": 0.0, "comforting someone who is stressed": -0.5,
          "intimate talkshow": -1.0, "fast-paced, articulate, energetic": -1.5, "energetic, colorful, and fast": -1.5}
_SETTINGS = {"on the phone": -2.0, "at a meditation retreat": -1.0, "in an oscar winning film": -0.5,
             "in an indie film": -0.5, "in a bookstore": -0.5, "sitting next to you on the bus": -0.5,
             "sitting next to you on an airplane": -0.5, "sitting on couch": -0.25,
             "brainstorming in a meeting": -0.25, "at an appointment": -0.25}
_US_REGIONS = {"general american": 1.5, "midwest": 1.5, "west coast": 1.5, "northwest": 1.5, "east coast": 0.5,
               "inland southern": -1.0, "gulf coast": -1.0}
_NARRATOR_WORDS = ("narrat", "announc", "presenter", "news", "radio host", "instructional video host")


def voice_traits(v: dict) -> dict:
    desc = str(v.get("description") or "")
    age = re.search(r"(\d+)-year-old", desc)
    setting = re.search(r"Currently ([^.]*)\.", desc)
    tone = re.search(r"(?:Voice|Tone) is ([^.]*)", desc)
    if tone:
        tone_text = tone.group(1).strip().lower()
    else:  # classics: "Smooth, mellow, and polished voice with a medium pitch. ..."
        m = re.match(r"\s*([^.]*?) voice with", desc)
        tone_text = m.group(1).strip().lower() if m else ""
    return {"age": int(age.group(1)) if age else None,
            "setting": setting.group(1).strip().lower() if setting else "",
            "tone": tone_text}


def exam_fit(v: dict, cell: str) -> float:
    t = voice_traits(v)
    persona = str(v.get("persona") or "").lower()
    if any(w in persona for w in _POOR_ROLES):
        role = 0.0
    elif any(w in persona for w in _EDU_ROLES):
        role = 3.0
    elif any(w in persona for w in _TALK_ROLES):
        role = 2.0
    else:
        role = 1.0
    context = str(v.get("context") or "").lower()
    ctx = 2.0 if "edu" in context else 1.0 if ("enterprise" in context or "content" in context) else 0.0
    tone = _TONES.get(t["tone"], 0.0)
    setting = _SETTINGS.get(t["setting"], 0.0) if t["setting"] else 0.0
    age = t["age"]
    age_fit = 0.5 if age is None else 1.0 if 25 <= age <= 60 else 0.0 if 22 <= age <= 65 else -1.5
    region = _US_REGIONS.get(str(v.get("accent") or "").lower(), 0.0) if cell == "american" else 0.0
    return round(role + ctx + tone + setting + age_fit + region, 2)


def announcer_fit(v: dict) -> float:
    age = voice_traits(v)["age"]
    return exam_fit(v, "british") + (1.0 if age is not None and 35 <= age <= 60 else 0.0)


def spread_by_pitch(voices: list[dict], cell: str, count: int) -> list[dict]:
    """Best exam fit first, round-robin over catalog pitch (low/medium/high) among the
    voices within 2.5 points of the best, then the weaker ones; ties by id."""
    if count <= 0:
        return []
    ranked = sorted(voices, key=lambda v: (-exam_fit(v, cell), str(v.get("id"))))
    if not ranked:
        return []
    best = exam_fit(ranked[0], cell)
    good = [v for v in ranked if exam_fit(v, cell) >= best - 2.5]
    weaker = [v for v in ranked if exam_fit(v, cell) < best - 2.5]
    groups: dict[str, list[dict]] = {}
    for v in good:
        groups.setdefault(str(v.get("pitch") or "unknown").lower(), []).append(v)
    order = sorted(groups, key=lambda p: (-exam_fit(groups[p][0], cell), p))
    out: list[dict] = []
    while len(out) < count and any(groups[p] for p in order):
        for p in order:
            if groups[p] and len(out) < count:
                out.append(groups[p].pop(0))
    return out + weaker[: count - len(out)]


def audition_candidates(catalog: list[dict], cell: str, gender: str, limit: int, exclude=()) -> list[dict]:
    pool = [v for v in catalog if in_cell(v, cell) and gender_of(v) == gender and v.get("type") == "prebuilt"
            and v.get("id") not in exclude]
    by_id = {v["id"]: v for v in pool}
    pinned = [by_id[i] for i in AUDITION_PINNED.get((cell, gender), []) if i in by_id]
    classics: list[dict] = []
    if cell == "american":
        general = [v for v in pool if "general american" in str(v.get("accent") or "").lower()]
        classics = sorted(general, key=lambda v: (-exam_fit(v, cell), v["id"]))[:AMERICAN_CLASSICS]
        pool = [v for v in pool if v not in general]
    rest = spread_by_pitch([v for v in pool if v not in pinned], cell, limit - len(pinned) - len(classics))
    picked = pinned + rest
    for i, c in enumerate(classics):  # second place: inside round 1 as the calibration control
        picked.insert(min(1 + i, len(picked)), c)
    return picked[:limit]


def announcer_candidates(catalog: list[dict], count: int) -> list[dict]:
    pool = [v for v in catalog if in_cell(v, "british") and gender_of(v) == "male" and v.get("type") == "prebuilt"
            and any(w in " ".join(str(v.get(k) or "") for k in ("id", "persona", "context")).lower()
                    for w in _NARRATOR_WORDS)]
    pool.sort(key=lambda v: (-announcer_fit(v), str(v.get("id"))))
    return pool[:count]


def audition_plan(catalog: list[dict], accents: list[str], per_cell: int) -> list[dict]:
    """Cells in run order (British, announcer, other core accents, new accents). Each
    holds its ranked candidates; round 1 synthesises the first `need` of them."""
    by_id = {str(v.get("id")).lower(): v for v in catalog}
    announcers = announcer_candidates(catalog, ANNOUNCER_CANDIDATES) if "british" in accents else []
    taken = {v["id"] for v in announcers}  # the announcer never shares a voice with a speaker pool
    cells = []
    for cell in accents:
        for gender in ("female", "male"):
            exclude = taken if (cell, gender) == ("british", "male") else ()
            picks = audition_candidates(catalog, cell, gender, per_cell, exclude)
            cells.append({"cell": cell, "gender": gender, "need": min(POOL_MINIMUM[cell], len(picks)),
                          "candidates": [{"voice": v["id"], "text": ACCENT_TEXT, "style": AUDITION_STYLE,
                                          "fit": exam_fit(v, cell), "pitch": v.get("pitch"),
                                          "accent": v.get("accent"), "persona": v.get("persona")} for v in picks]})
        if cell == "british" and announcers:
            control = by_id.get(ANNOUNCER_CONTROL)
            entries = [{"voice": v["id"], "text": ANNOUNCE_TEXT, "style": ANNOUNCEMENT_STYLE,
                        "fit": announcer_fit(v), "pitch": v.get("pitch"), "accent": v.get("accent"),
                        "persona": v.get("persona")} for v in announcers]
            if control:
                entries.append({"voice": control["id"], "text": ANNOUNCE_TEXT, "style": ANNOUNCEMENT_STYLE,
                                "fit": None, "pitch": control.get("pitch"), "accent": control.get("accent"),
                                "persona": control.get("persona"), "control": "classic-announcer"})
            cells.append({"cell": "announcer", "gender": "male", "need": len(entries), "candidates": entries})
    return cells


def sample_id(cell: str, gender: str, voice: str, take: int = 1) -> str:
    return f"A.{cell}.{gender}.{voice}" + (f".take{take}" if take > 1 else "")


# The AI ear for auditions: one clip, one reader, the 9 accents only, JSON out.
AUDITION_EAR_PROMPT = """You are an expert phonetician and dialect coach. The attached recording is one short clip of English speech. The reader was asked to read exactly this text:
"__TEXT__"
The text was written in British English for every reader, whatever their accent, so ignore vocabulary and spelling and judge the accent only from pronunciation: __CUES__ and intonation.

Return one JSON object with exactly these keys:
- "speakers": how many different voices you hear (integer).
- "gender": "female" | "male" | "unclear".
- "gender_confidence": "low" | "medium" | "high".
- "age": "child" | "teen" | "20s" | "30s" | "40s" | "50s" | "60s+".
- "adult": true if the speaker sounds 18 or older, otherwise false.
- "accent": the closest of exactly these nine: "England", "Scotland", "Ireland", "USA", "Canada", "Australia", "New Zealand", "South Africa", "India".
- "accent_confidence": "low" | "medium" | "high".
- "runner_up": the second closest of the same nine.
- "cues": up to three short phonetic cues behind your accent judgment.
- "clarity": 1 to 5, how easily an intermediate (B1) learner of English would catch every word (5 = every word crisp and easy).
- "naturalness": 1 to 5 (5 = sounds like a real person reading aloud; lower for robotic or sing-song prosody, odd rhythm or glitches).
- "pace": "slow" | "normal" | "fast".
- "misread": words skipped, added or changed compared with the text; [] if none.
- "artefacts": short descriptions of any audio problems (clicks, distortion, noise, muffled or telephone-like sound, breathiness or whispering, unnatural pauses, music, a voice that changes partway through); [] if none.__EXTRA__
"""
AUDITION_EAR_CUES = ('the vowels in "last", "after", "can\'t", "castle", "late", "paid" and "parking", whether "r" is '
                     'pronounced after vowels ("car park", "year", "water", "starts"), how "t" sounds between vowels '
                     '("better", "water"), "schedule", "Tuesday"')
ANNOUNCE_EAR_CUES = ('the vowels in "part", "conversation", "receptionist" and "caller", whether "r" is pronounced '
                     'after vowels ("part", "caller", "hear"), how "t" sounds')
AUDITION_EAR_ANNOUNCER = ('\n- "announcer_fit": 1 to 5, how well this voice and delivery suit reading the instructions '
                          'of a formal English listening exam (calm, clear, neutral).')


def audition_ear_prompt(text: str, announcer: bool) -> str:
    return (AUDITION_EAR_PROMPT.replace("__TEXT__", text)
            .replace("__CUES__", ANNOUNCE_EAR_CUES if announcer else AUDITION_EAR_CUES)
            .replace("__EXTRA__", AUDITION_EAR_ANNOUNCER if announcer else ""))


def audition_ear_body(wav: bytes, text: str, announcer: bool) -> dict:
    return {
        "model": EAR_MODEL,
        "input": [{"type": "user_input", "content": [
            {"type": "audio", "data": base64.b64encode(wav).decode("ascii"), "mime_type": "audio/wav"},
            {"type": "text", "text": audition_ear_prompt(text, announcer)}]}],
        "generation_config": {"thinking_level": "low", "max_output_tokens": 2048},
        "response_format": {"type": "text", "mime_type": "application/json"},
        "store": False,
    }


def run_audition_ear(run: Run, rid: str, wav_path: Path, text: str, announcer: bool, attach_to: dict) -> dict | None:
    wav = wav_path.read_bytes()
    seconds = (len(wav) - 44) / (2 * SAMPLE_RATE)
    entry, data = run.call(rid, f"AI ear (audition) on {wav_path.name}", EAR_MODEL, "POST", "/interactions",
                           body=audition_ear_body(wav, text, announcer), estimate=estimate_ear(seconds, "audition"),
                           timeout=TEXT_TIMEOUT_S)
    parsed = None
    if entry["status"] == 200 and isinstance(data, dict):
        raw = output_text(data) or ""
        try:
            parsed = json.loads(strip_fences(raw))
            if isinstance(parsed, list) and parsed and isinstance(parsed[0], dict):
                parsed = parsed[0]
            if not isinstance(parsed, dict):
                raise ValueError("not an object")
        except ValueError:
            parsed = None
            entry["ear_raw"] = raw[:3000]
    entry["ear"] = parsed
    attach_to["ear"] = parsed
    attach_to["ear_request"] = rid
    run.save()
    say(f"    ear: {audition_ear_line(parsed)}")
    return parsed


def audition_ear_line(ear: dict | None) -> str:
    if not ear:
        return "no answer"
    bits = [f"{ear.get('gender')}({ear.get('gender_confidence')})", f"{ear.get('accent')}({ear.get('accent_confidence')})"
            f"/2nd {ear.get('runner_up')}", f"age {ear.get('age')}", f"clarity {ear.get('clarity')}",
            f"natural {ear.get('naturalness')}", f"pace {ear.get('pace')}"]
    if ear.get("announcer_fit") is not None:
        bits.append(f"announcer {ear.get('announcer_fit')}")
    if _num(ear.get("speakers")) and _num(ear.get("speakers")) > 1:
        bits.append(f"{ear.get('speakers')} voices")
    if ear.get("misread"):
        bits.append(f"misread {ear.get('misread')}")
    if ear.get("artefacts"):
        bits.append(f"artefacts {ear.get('artefacts')}")
    return ", ".join(bits)


def _num(value) -> float | None:
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def spectrum_check(samples: np.ndarray, rate: int) -> dict:
    """Share of energy above 4 kHz and the 99 % energy bandwidth: a telephone-band
    voice ("Currently on the phone") has almost nothing above 3.4 kHz."""
    n = 4096
    frames = len(samples) // n
    if frames < 2:
        return {}
    x = samples[: frames * n].astype(np.float64).reshape(frames, n) * np.hanning(n)
    power = (np.abs(np.fft.rfft(x, axis=1)) ** 2).mean(axis=0)
    freqs = np.fft.rfftfreq(n, 1 / rate)
    band = freqs >= 80
    total = float(power[band].sum()) or 1.0
    cumulative = np.cumsum(power[band]) / total
    return {"hf_share_4k": round(float(power[freqs >= 4000].sum()) / total, 4),
            "bandwidth_99_hz": int(freqs[band][min(int(np.searchsorted(cumulative, 0.99)), int(band.sum()) - 1)])}


def judge_sample(entry: dict, cell: str, gender: str) -> tuple[bool, list[str]]:
    """The audition rule: the AI ear hears the cell's accent with confidence >= medium,
    the catalog gender, clarity >= 4, an adult and one voice."""
    ear = entry.get("ear")
    if not entry.get("wav"):
        return False, [f"no audio ({entry.get('status')})"]
    if not ear:
        return False, ["no AI-ear answer"]
    reasons = []
    want = EAR_ACCENT[cell]
    heard = str(ear.get("accent") or "")
    confidence = str(ear.get("accent_confidence") or "").lower()
    if heard.lower() != want.lower():
        reasons.append(f"accent heard {heard or '?'} ({confidence or '?'})")
    elif CONFIDENCE_RANK.get(confidence, -1) < 1:
        reasons.append(f"{want} only with {confidence or 'no'} confidence")
    heard_gender = str(ear.get("gender") or "").lower()
    if heard_gender != gender:
        reasons.append(f"gender heard {heard_gender or '?'}")
    clarity = _num(ear.get("clarity"))
    if clarity is None or clarity < 4:
        reasons.append(f"clarity {ear.get('clarity')}")
    if ear.get("adult") is False:
        reasons.append("not heard as an adult")
    speakers = _num(ear.get("speakers"))
    if speakers and speakers > 1:
        reasons.append(f"{int(speakers)} voices heard in a single-voice clip")
    return not reasons, reasons


def gender_contradicted(entry: dict, gender: str) -> bool:
    """Design pre-screen: the AI ear hears the other gender with high confidence."""
    ear = entry.get("ear") or {}
    heard = str(ear.get("gender") or "").lower()
    return heard in ("female", "male") and heard != gender and str(ear.get("gender_confidence")).lower() == "high"


def _rank_key(entry: dict, fit) -> tuple:
    ear = entry.get("ear") or {}
    return (-(_num(ear.get("naturalness")) or 0), -(_num(ear.get("clarity")) or 0),
            -CONFIDENCE_RANK.get(str(ear.get("accent_confidence") or "").lower(), -1), -(fit or 0), entry["voice"])


def vary_pitch(ranked: list[dict]) -> list[dict]:
    """Keep the ranking, but let the first three differ in pitch when a voice of
    another catalog pitch (or, when every voice shares one, an F0 at least 12 Hz from
    the ones already chosen) is at most one naturalness point behind."""
    out, rest = [], list(ranked)
    while rest and len(out) < 3:
        best_nat = _num((rest[0].get("ear") or {}).get("naturalness")) or 0
        close = [e for e in rest if (_num((e.get("ear") or {}).get("naturalness")) or 0) >= best_nat - 1]
        used = {e.get("pitch") for e in out}
        pick = next((e for e in close if e.get("pitch") not in used), None)
        if pick is None and out:
            pick = next((e for e in close if e.get("f0_hz") and all(
                u.get("f0_hz") and abs(e["f0_hz"] - u["f0_hz"]) >= 12 for u in out)), None)
        pick = pick or rest[0]
        out.append(pick)
        rest.remove(pick)
    return out + rest


def accent_region(accent: str | None) -> str:
    a = str(accent or "").strip()
    if not a or a.lower() == "indian english":
        return ""
    if a.lower() == "auckland new zealand english":
        return "Auckland"
    return a[: -len(" English")] if a.endswith(" English") else a


def pool_entry(voice: dict, cell: str, gender: str) -> dict:
    t = voice_traits(voice)
    parts = [gender.capitalize()]
    region = accent_region(voice.get("accent"))
    name = ACCENT_NAMES.get(cell, "British")
    parts.append(f"{name} ({region})" if region else name)
    if t["age"]:
        parts.append(f"{t['age'] // 10 * 10}s")
    if t["tone"]:  # the pitch is said once, from the catalog's pitch field
        parts.append(t["tone"].replace("medium-pitch, ", "").replace(", and ", " and "))
    if voice.get("pitch"):
        parts.append(f"{voice['pitch']} pitch")
    return {"id": voice["id"], "name": voice.get("display_name") or voice["id"], "description": ", ".join(parts)}


def audition_rows(run: Run, plan: list[dict]) -> dict[tuple, list[dict]]:
    """Per (cell, gender): one row per synthesised candidate (take 1), in plan order."""
    rows: dict[tuple, list[dict]] = {}
    for cell in plan:
        out = []
        for c in cell["candidates"]:
            entry = run.entry(sample_id(cell["cell"], cell["gender"], c["voice"]))
            if not entry:
                continue
            ok, reasons = judge_sample(entry, cell["cell"], cell["gender"])
            row = {"voice": c["voice"], "entry": entry, "ear": entry.get("ear"), "ok": ok, "reasons": reasons,
                   "fit": c.get("fit"), "pitch": c.get("pitch"), "control": c.get("control"),
                   "f0_hz": (entry.get("f0") or {}).get("median_hz"), "spectrum": entry.get("spectrum")}
            if cell["cell"] == "announcer":
                check = run.entry(announcer_accent_rid(c["voice"]))
                if check and check.get("ear"):
                    a_ok, a_reasons = judge_sample(check, "british", "male")
                    row["accent_check"] = {"ok": a_ok, "reasons": a_reasons, "ear": check["ear"],
                                           "f0_hz": (check.get("f0") or {}).get("median_hz")}
                    if not a_ok:
                        row["ok"] = False
                        row["reasons"] = reasons + [f"ACCENT_TEXT check: {r}" for r in a_reasons]
            out.append(row)
        rows[(cell["cell"], cell["gender"])] = out
    return rows


def announcer_accent_rid(voice: str) -> str:
    return sample_id("announcer", "male", voice) + ".accent"


def announcer_ranking(rows: list[dict]) -> list[dict]:
    """Announcer candidates that pass on ANNOUNCE_TEXT (controls excluded), best first."""
    return sorted((r for r in rows if r["ok"] and not r.get("control")),
                  key=lambda r: (-(_num((r["ear"] or {}).get("announcer_fit")) or 0),) + _rank_key(r, r["fit"]))


def run_announcer_accent(run: Run, wav_dir: Path, voice: str, use_ear: bool) -> dict:
    """The announcement is too short for accent: on 2026-10-05 the ear called the
    General American control Charon "England (high)" on it. An announcer candidate
    therefore also reads ACCENT_TEXT, judged like a British male sample."""
    rid = announcer_accent_rid(voice)
    entry = run.entry(rid)
    if not (entry and entry.get("wav")):
        req = Req(rid, "A.announcer", "announcer candidate reads ACCENT_TEXT (accent check)", [(None, ACCENT_TEXT)],
                  [("Speaker", voice)], style=AUDITION_STYLE, expect="male", ear=None)
        entry = run_speech(run, req, wav_dir, False)
        entry.update({"cell": "announcer", "gender": "male", "voice": voice, "take": 1, "text": "ACCENT_TEXT",
                      "style": AUDITION_STYLE, "accent_check": True})
        if entry.get("wav"):
            samples, rate = read_wav(run.folder / entry["wav"])
            entry["spectrum"] = spectrum_check(samples, rate)
        run.save()
    if entry.get("wav") and use_ear and not entry.get("ear"):
        run_audition_ear(run, f"{rid}.ear", run.folder / entry["wav"], ACCENT_TEXT, False, entry)
        ok, reasons = judge_sample(entry, "british", "male")
        say(f"    -> accent check {'passed' if ok else 'failed: ' + '; '.join(reasons)}")
    return entry


def approved_count(run: Run, cell: dict) -> int:
    return sum(1 for c in cell["candidates"] if not c.get("control")
               and (e := run.entry(sample_id(cell["cell"], cell["gender"], c["voice"])))
               and judge_sample(e, cell["cell"], cell["gender"])[0])


def propose(run: Run, catalog: list[dict], plan: list[dict], out: Path) -> dict:
    by_id = {v.get("id"): v for v in catalog}
    rows = audition_rows(run, plan)
    pools: dict[str, dict[str, list]] = {}
    summary: dict[tuple, dict] = {}
    unconfirmed, dropped, short = [], [], []
    for cell in plan:
        key = (cell["cell"], cell["gender"])
        if cell["cell"] == "announcer":
            continue
        cell_rows = [r for r in rows.get(key, []) if not r.get("control")]
        approved = vary_pitch(sorted((r for r in cell_rows if r["ok"]), key=lambda r: _rank_key(r, r["fit"])))
        chosen = list(approved)
        need = POOL_MINIMUM[cell["cell"]]
        fills = []
        if len(chosen) < need and cell["cell"] in CORE_ACCENTS:
            # Core accents cannot be dropped: fill up to the minimum with voices that failed
            # only on the accent judgment, the ear's runner-up matching first.
            want = EAR_ACCENT[cell["cell"]]
            near = [r for r in cell_rows if not r["ok"] and r["ear"] and all(
                x.startswith("accent heard") or " only with " in x for x in r["reasons"])]
            near.sort(key=lambda r: (str((r["ear"] or {}).get("runner_up") or "").lower() != want.lower(),)
                      + _rank_key(r, r["fit"]))
            fills = near[: need - len(chosen)]
            chosen += fills
            unconfirmed += [f"{cell['cell']} {cell['gender']}: {r['voice']} ({'; '.join(r['reasons'])})" for r in fills]
        if len(chosen) < need:
            short.append(f"{cell['cell']} {cell['gender']}: {len(chosen)} of {need}")
        if chosen:
            pools.setdefault(cell["cell"], {})[cell["gender"]] = [
                pool_entry(by_id[r["voice"]], cell["cell"], cell["gender"]) for r in chosen if r["voice"] in by_id]
        summary[key] = {"approved": [r["voice"] for r in approved], "unconfirmed": [r["voice"] for r in fills],
                        "rejected": [(r["voice"], r["reasons"]) for r in cell_rows if not r["ok"]],
                        "tried": len(cell_rows), "need": need}
    # An accent new in 0.8 with no voice for a gender is not offered at all.
    for accent in NEW_ACCENTS:
        if accent in pools and not (pools[accent].get("female") and pools[accent].get("male")):
            dropped.append(accent)
            del pools[accent]
        elif accent not in pools and any(c["cell"] == accent for c in plan):
            dropped.append(accent)
    # Announcer: British male narrators judged like a British male, ranked by announcer fit;
    # the accent counts only from the ACCENT_TEXT check (ANNOUNCE_TEXT is too short).
    ann_rows = rows.get(("announcer", "male"), [])
    ann_ranked = announcer_ranking(ann_rows)
    ann_ok = [r for r in ann_ranked if (r.get("accent_check") or {}).get("ok")]
    announcer_unverified = not ann_ok and bool(ann_ranked)
    if announcer_unverified:
        ann_ok = ann_ranked
    announcer = pool_entry(by_id[ann_ok[0]["voice"]], "british", "male") if ann_ok else None
    summary[("announcer", "male")] = {"approved": [r["voice"] for r in ann_ok], "unconfirmed": [],
                                     "rejected": [(r["voice"], r["reasons"] + (["blind control"] if r.get("control") else []))
                                                  for r in ann_rows if not r["ok"] or r.get("control")],
                                     "tried": len(ann_rows), "need": 1}
    calibration = []
    for r in [r for rs in rows.values() for r in rs]:
        v = by_id.get(r["voice"]) or {}
        if "general american" in str(v.get("accent") or "").lower() and r["ear"]:
            calibration.append(f"{r['voice']} on {r['entry'].get('text') or 'ACCENT_TEXT'} heard "
                               f"{r['ear'].get('accent')} ({r['ear'].get('accent_confidence')})")
    spent = run.manifest.get("spent_usd", 0)
    comment = (f"PROPOSED by tools/voice_lab.py audition on {dt.date.today().isoformat()} from the AI ear "
               f"({EAR_MODEL}) on one ACCENT_TEXT sample per voice (style \"{AUDITION_STYLE}\"); not yet confirmed "
               f"by the PO (listening sheet 2). Rule: the ear hears the pool's accent with confidence >= medium, the "
               f"catalog gender, clarity >= 4, an adult, one voice; ordered by naturalness then clarity, the first "
               f"three spread in pitch. Audition spend ${spent:.4f}.")
    if unconfirmed:
        comment += " Unconfirmed fills (accent not confirmed by the ear, core accent kept at its minimum): " + \
                   "; ".join(unconfirmed) + "."
    noted = [s for s in short if s.split()[0] in pools]
    if noted:
        comment += (" Offered with a warning, below the minimum (two speakers of that accent and gender in one part "
                    "cannot both be voiced): " + "; ".join(noted) + ".")
    if dropped:
        comment += " Dropped (no approved voice for a gender): " + ", ".join(dropped) + "."
    if calibration:
        comment += " Ear calibration (catalog General American): " + "; ".join(calibration) + "."
    if not announcer:
        comment += " No announcer candidate passed; keep the current announcer until the PO picks one."
    elif announcer_unverified:
        comment += " The announcer's accent is not verified on ACCENT_TEXT (the ear cannot judge accent on the short " \
                   "announcement)."
    else:
        comment += " Announcer: read ANNOUNCE_TEXT, accent checked on ACCENT_TEXT."
    order = [a for a in CORE_ACCENTS + NEW_ACCENTS if a in pools]
    proposal = {"version": 2, "comment": comment}
    if announcer:
        proposal["announcer"] = announcer
    proposal["pools"] = {a: {g: pools[a][g] for g in ("male", "female") if g in pools[a]} for a in order}
    write_json(out / "default_voices.proposed.json", proposal)
    write_text(run.folder / "proposal.md", proposal_markdown(rows, summary, proposal, calibration, by_id, run))
    return {"file": proposal, "summary": summary, "rows": rows, "calibration": calibration,
            "unconfirmed": unconfirmed, "dropped": dropped, "short": short}


def proposal_markdown(rows, summary, proposal, calibration, by_id, run: Run) -> str:
    reqs = run.manifest["requests"]
    speech = sum(int(r.get("micro_usd") or 0) for r in reqs if r.get("model") in (TTS_MODEL, LITE_MODEL))
    ear = sum(int(r.get("micro_usd") or 0) for r in reqs if r.get("model") == EAR_MODEL)
    lines = [f"# Audition proposal ({dt.date.today().isoformat()})", "",
             f"Spend in this folder: {usd(speech + ear)} (speech {usd(speech)} over "
             f"{sum(1 for r in reqs if r.get('model') == TTS_MODEL)} requests, AI ear {usd(ear)} over "
             f"{sum(1 for r in reqs if r.get('model') == EAR_MODEL)}).", "",
             f"Text: ACCENT_TEXT with style \"{AUDITION_STYLE}\" (announcer: ANNOUNCE_TEXT with \"{ANNOUNCEMENT_STYLE}\"). "
             "Rule: ear accent = cell with confidence >= medium, ear gender = catalog gender, clarity >= 4, adult, "
             "one voice. F0 is the YIN median of voiced frames; hf = share of energy above 4 kHz, bw99 = the frequency below which 99 % of the energy lies (under about 3.4 kHz can sound muffled or telephone-like).", "",
             "## AI-ear calibration (catalog General American voices)", ""]
    lines += [f"- {c}" for c in calibration] or ["- none auditioned"]
    lines += ["", "## Per cell", ""]
    for (cell, gender), rs in rows.items():
        s = summary.get((cell, gender), {})
        lines += [f"### {cell} {gender}: {len(s.get('approved', []))} approved of {s.get('tried', 0)} "
                  f"(minimum {s.get('need', '-')})", "",
                  "| voice | catalog | fit | s | F0 Hz | hf / bw99 Hz | ear gender | ear accent (conf) / 2nd | age | clarity | natural | other | verdict |",
                  "|---|---|---|---|---|---|---|---|---|---|---|---|---|"]
        for r in rs:
            v = by_id.get(r["voice"]) or {}
            ear = r["ear"] or {}
            t = voice_traits(v)
            catalog_cell = f"{v.get('pitch')}, {t['age'] or '-'}, {accent_region(v.get('accent')) or v.get('language_code')}"
            other = []
            if ear.get("announcer_fit") is not None:
                other.append(f"announcer {ear['announcer_fit']}")
            if r.get("accent_check"):
                ac = r["accent_check"]["ear"] or {}
                other.append(f"ACCENT_TEXT check: {ac.get('gender')} {ac.get('accent')} ({ac.get('accent_confidence')}), "
                             f"clarity {ac.get('clarity')}, natural {ac.get('naturalness')}, F0 {r['accent_check']['f0_hz']}")
            elif cell == "announcer" and not r.get("control"):
                other.append("accent not checked on ACCENT_TEXT")
            if ear.get("misread"):
                other.append(f"misread {ear['misread']}")
            if ear.get("artefacts"):
                other.append(f"artefacts {ear['artefacts']}")
            if ear.get("pace") and ear.get("pace") != "normal":
                other.append(f"pace {ear['pace']}")
            verdict = "control" if r.get("control") else ("approved" if r["ok"] else
                      ("unconfirmed fill: " if r["voice"] in s.get("unconfirmed", []) else "") + "; ".join(r["reasons"]))
            spec = r.get("spectrum") or {}
            lines.append(f"| {r['voice']} | {catalog_cell} | {r['fit']} | {r['entry'].get('duration_s', '-')} | "
                         f"{r['f0_hz']} | {spec.get('hf_share_4k', '-')} / {spec.get('bandwidth_99_hz', '-')} | {ear.get('gender')} ({ear.get('gender_confidence')}) | "
                         f"{ear.get('accent')} ({ear.get('accent_confidence')}) / {ear.get('runner_up')} | {ear.get('age')} | "
                         f"{ear.get('clarity')} | {ear.get('naturalness')} | {html.escape('; '.join(str(o) for o in other))[:200]} | "
                         f"{verdict} |")
        lines.append("")
    lines += ["## Proposed pools file", "", "```json", json.dumps(proposal, indent=2, ensure_ascii=False), "```", ""]
    return "\n".join(lines)


def run_candidate(run: Run, wav_dir: Path, cell: dict, cand: dict, use_ear: bool, take: int = 1) -> dict | None:
    rid = sample_id(cell["cell"], cell["gender"], cand["voice"], take)
    entry = run.entry(rid)
    if not (entry and entry.get("wav")):
        req = Req(rid, f"A.{cell['cell']}", f"audition {cell['cell']} {cell['gender']}"
                  + (f" take {take}" if take > 1 else ""), [(None, cand["text"])], [("Speaker", cand["voice"])],
                  style=cand["style"], expect=cell["gender"], ear=None)
        entry = run_speech(run, req, wav_dir, False)
        entry.update({"cell": cell["cell"], "gender": cell["gender"], "voice": cand["voice"], "take": take,
                      "control": cand.get("control"), "text": "ANNOUNCE_TEXT" if cand["text"] == ANNOUNCE_TEXT
                      else "ACCENT_TEXT", "style": cand["style"]})
        if entry.get("wav"):
            samples, rate = read_wav(run.folder / entry["wav"])
            entry["spectrum"] = spectrum_check(samples, rate)
        run.save()
    if take == 1 and entry.get("wav") and use_ear and not entry.get("ear"):
        run_audition_ear(run, f"{rid}.ear", run.folder / entry["wav"], cand["text"], cell["cell"] == "announcer", entry)
        ok, reasons = judge_sample(entry, cell["cell"], cell["gender"])
        say(f"    -> {'APPROVED' if ok else 'not approved: ' + '; '.join(reasons)}")
        if gender_contradicted(entry, cell["gender"]):
            entry["prescreen"] = "dropped: the AI ear hears the other gender (high confidence)"
        run.save()
    return entry


def cmd_audition(args) -> None:
    out = Path(args.out)
    catalog = load_catalog(out)
    accents = [a.strip() for a in args.accents.split(",")] if args.accents else CORE_ACCENTS + NEW_ACCENTS
    unknown = [a for a in accents if a not in CELLS]
    if unknown:
        raise SystemExit(f"unknown accent(s) {unknown}; known: {', '.join(CELLS)}")
    accents = [a for a in CORE_ACCENTS + NEW_ACCENTS if a in accents]  # core first, new second
    per_cell = max(1, min(args.per_cell, AUDITION_MAX_PER_CELL))
    plan = audition_plan(catalog, accents, per_cell)
    per_sample = estimate_speech(TTS_MODEL, [ACCENT_TEXT]) + estimate_ear(15, "audition")
    first = sum(c["need"] for c in plan)
    total = sum(len(c["candidates"]) for c in plan)
    say(f"{len(plan)} cells; round 1 {first} samples, at most {total}; conservative estimate {usd(first * per_sample)} "
        f"/ {usd(total * per_sample)} (measured on 2026-10-05: about $0.006 per sample with the ear)")
    for c in plan:
        say(f"  {c['cell']:<12} {c['gender']:<6} need {c['need']}: " + ", ".join(
            f"{'*' if i < c['need'] else ''}{x['voice']}({x.get('pitch')},{x.get('fit')})" for i, x in enumerate(c["candidates"])))
    if args.dry_run:
        return
    folder = out / "audition"
    wav_dir = folder / "wav"
    if args.offline:
        run = Run(folder, None, None, "audition")
    else:
        if args.max_usd is None:
            raise SystemExit("audition makes paid requests: pass --max-usd (e.g. --max-usd 0.40), or --offline.")
        key, source = find_key()
        say(f"key from {source}")
        wav_dir.mkdir(parents=True, exist_ok=True)
        run = Run(folder, Api(key), args.max_usd, "audition")
        say(f"folder spend so far {usd(run.ledger.prior)}, cap {usd(run.ledger.cap)}")
        use_ear = not args.no_ear
        try:
            # Round 1: the minimum per cell (announcer: every candidate), core accents first.
            for cell in plan:
                for cand in cell["candidates"][: cell["need"]]:
                    run_candidate(run, wav_dir, cell, cand, use_ear)
            # Later passes: one more candidate per cell while it lacks a spare voice
            # (minimum + 1); cells whose misses are all one confused accent go last.
            while True:
                progressed = False
                pending = []
                for cell in plan:
                    if cell["cell"] == "announcer":
                        continue
                    entries = [run.entry(sample_id(cell["cell"], cell["gender"], c["voice"])) for c in cell["candidates"]]
                    done = [e for e in entries if e]
                    rest = [c for c, e in zip(cell["candidates"], entries) if not e]
                    if not rest or approved_count(run, cell) >= POOL_MINIMUM[cell["cell"]] + 1:
                        continue
                    heard = {str((e.get("ear") or {}).get("accent")) for e in done}
                    have = approved_count(run, cell)
                    confused = have == 0 and len(done) >= 3 and len(heard) == 1
                    # Cells below their minimum first, then cells that only lack the spare.
                    pending.append(((confused, have >= POOL_MINIMUM[cell["cell"]], len(pending)), cell, rest[0]))
                pending.sort(key=lambda p: p[0])
                for _, cell, cand in pending:
                    run_candidate(run, wav_dir, cell, cand, use_ear)
                    progressed = True
                if not progressed:
                    break
            # Announcer accent: the best candidates read ACCENT_TEXT until one is heard as England.
            if use_ear:
                for ann in [c for c in plan if c["cell"] == "announcer"]:
                    for row in announcer_ranking(audition_rows(run, [ann]).get(("announcer", "male"), [])):
                        check = run_announcer_accent(run, wav_dir, row["voice"], use_ear)
                        if judge_sample(check, "british", "male")[0]:
                            break
            # Second takes of the best British voices: the "same voice twice" controls.
            interim = propose(run, catalog, plan, out)
            for gender in ("female", "male"):
                best = (interim["file"]["pools"].get("british", {}).get(gender) or [None])[0]
                cell = next((c for c in plan if c["cell"] == "british" and c["gender"] == gender), None)
                if best and cell:
                    cand = next(c for c in cell["candidates"] if c["voice"] == best["id"])
                    run_candidate(run, wav_dir, cell, cand, False, take=2)
        except BudgetStop as stop:
            say(f"STOPPED: {stop}")
            unfinished = [f"{c['cell']} {c['gender']} ({approved_count(run, c)} approved, "
                          f"{sum(1 for x in c['candidates'] if not run.entry(sample_id(c['cell'], c['gender'], x['voice'])))} untried)"
                          for c in plan if c["cell"] != "announcer" and approved_count(run, c) < POOL_MINIMUM[c["cell"]]]
            run.manifest["runs"][-1]["stopped"] = str(stop)
            run.manifest["runs"][-1]["unfinished_cells"] = unfinished
            say("cells below their minimum: " + (", ".join(unfinished) or "none"))
        run.manifest["runs"][-1]["finished"] = dt.datetime.now().isoformat(timespec="seconds")
        run.save()
    result = propose(run, catalog, plan, out)
    write_audition_sheet(run, catalog, plan, result)
    for (cell, gender), s in result["summary"].items():
        say(f"  {cell:<12} {gender:<6} approved {len(s['approved'])}/{s['tried']} (min {s['need']}): "
            f"{', '.join(s['approved'])}" + (f"; unconfirmed {', '.join(s['unconfirmed'])}" if s["unconfirmed"] else ""))
    say(f"proposal: {out / 'default_voices.proposed.json'}; table: {folder / 'proposal.md'}; "
        f"spent in folder {usd(int(run.manifest.get('spent_usd', 0) * 1e6))}")


# ---------------------------------------------------------------- audition sheet (sheet 2)

AUDITION_QUESTIONS = {
    "sample": [
        ("authenticity", "scale", "Đúng giọng vùng ghi ở đầu mục không? (1 = hoàn toàn không, 5 = rất chuẩn)"),
        ("intelligibility", "scale", "Học sinh Việt Nam B1–C1 nghe rõ không? (1–5)"),
        ("naturalness", "scale", "Tự nhiên không? (1 = rất máy, 5 = như người thật)"),
        ("gender", "choice:Nữ|Nam|Không rõ", "Giới tính nghe được"),
        ("age", "choice:20s|30s|40s|50+", "Độ tuổi nghe được"),
        ("usable", "choice:Có|Không", "Dùng được trong đề thi?"),
    ],
    "pair_same": [
        ("same", "choice:Cùng một người|Hai người khác nhau|Không chắc", "Hai mẫu X và Y là cùng một người không?"),
    ],
}
AUDITION_QUESTIONS["announcer"] = AUDITION_QUESTIONS["sample"] + [
    ("announcer_fit", "scale", "Hợp làm giọng đọc hướng dẫn của đề thi không? (1–5)")]
ACCENT_LABELS_VI = {"british": "Anh (England)", "american": "Mỹ", "australian": "Úc", "canadian": "Canada",
                    "newzealand": "New Zealand", "irish": "Ireland", "scottish": "Scotland",
                    "southafrican": "Nam Phi", "indian": "Ấn Độ", "announcer": "Anh (England), người đọc hướng dẫn"}
AUDITION_SECTIONS = {
    "core": {"title": "Phần 1 — giọng chính: Anh, Mỹ, Úc, Canada, New Zealand và người đọc hướng dẫn",
             "intro": "Mỗi mục là một giọng đọc cùng một đoạn văn ngắn. Vùng giọng cần chấm ghi ở đầu mục; giới tính "
                      "thì không ghi — hãy ghi giới tính bạn nghe được."},
    "pairs": {"title": "Phần 2 — cùng một người hay hai người?",
              "intro": "Mỗi mục có hai mẫu giọng Anh X và Y. Chỉ trả lời: hai mẫu là cùng một người hay hai người khác "
                       "nhau. Nghe kỹ âm sắc, không cần chấm điểm."},
    "new": {"title": "Phần 3 — giọng mới: Ireland, Scotland, Nam Phi, Ấn Độ",
            "intro": "Có thể làm phần này trong một buổi khác: câu trả lời đã lưu vẫn còn."},
}


def write_audition_sheet(run: Run, catalog: list[dict], plan: list[dict], result: dict) -> None:
    rows = result["rows"]
    seed = int(run.manifest.get("sheet_seed") or random.SystemRandom().randrange(1, 10**6))
    run.manifest["sheet_seed"] = seed
    rng = random.Random(seed)
    sheet_dir = run.folder / "sheet2"
    if sheet_dir.exists():
        shutil.rmtree(sheet_dir)
    sheet_dir.mkdir(parents=True)
    core, new = [], []
    for (cell, gender), rs in rows.items():
        for r in rs:
            entry = r["entry"]
            if not entry.get("wav") or entry.get("prescreen"):
                continue
            item = {"wav": entry["wav"], "voice": r["voice"], "cell": cell, "gender": gender, "control": r.get("control"),
                    "kind": "announcer" if cell == "announcer" else "sample", "request": entry["id"]}
            (new if cell in NEW_ACCENTS else core).append(item)
    # Controls: the best British female again (a second take when there is one), and a
    # classic General American voice presented as British.
    def take2(cell, gender, voice):
        e = run.entry(sample_id(cell, gender, voice, 2))
        return e if e and e.get("wav") else None
    brit_f = result["file"]["pools"].get("british", {}).get("female") or []
    if brit_f:
        voice = brit_f[0]["id"]
        first = run.entry(sample_id("british", "female", voice))
        again = take2("british", "female", voice) or first
        if again and again.get("wav"):
            core.append({"wav": again["wav"], "voice": voice, "cell": "british", "gender": "female",
                         "control": "repeat", "kind": "sample", "request": again["id"]})
    classic = next((r for r in rows.get(("american", "female"), []) + rows.get(("american", "male"), [])
                    if r["entry"].get("wav") and "general american" in str(
                        next((v.get("accent") for v in catalog if v.get("id") == r["voice"]), "")).lower()), None)
    if classic:
        core.append({"wav": classic["entry"]["wav"], "voice": classic["voice"], "cell": "british",
                     "gender": classic["entry"].get("gender"), "control": "classic-in-british", "kind": "sample",
                     "request": classic["entry"]["id"]})
    # Distinctness pairs: the first four British voices per gender, 6 pairs each, plus
    # one same-voice pair (second take when there is one) as a control.
    pairs = []
    for gender in ("female", "male"):
        ids = [e["id"] for e in result["file"]["pools"].get("british", {}).get(gender) or []]
        if len(ids) < 4:
            extra = sorted((r for r in rows.get(("british", gender), []) if r["entry"].get("wav") and r["voice"] not in ids),
                           key=lambda r: _rank_key(r, r["fit"]))
            ids += [r["voice"] for r in extra][: 4 - len(ids)]
        ids = ids[:4]
        entries = {v: run.entry(sample_id("british", gender, v)) for v in ids}
        for i in range(len(ids)):
            for j in range(i + 1, len(ids)):
                pairs.append({"files": [entries[ids[i]]["wav"], entries[ids[j]]["wav"]], "voices": [ids[i], ids[j]],
                              "requests": [entries[ids[i]]["id"], entries[ids[j]]["id"]], "gender": gender})
        if ids:
            again = take2("british", gender, ids[0]) or entries[ids[0]]
            pairs.append({"files": [entries[ids[0]]["wav"], again["wav"]], "voices": [ids[0], ids[0]],
                          "requests": [entries[ids[0]]["id"], again["id"]], "gender": gender, "control": "same-voice"})
    rng.shuffle(core)
    rng.shuffle(pairs)
    rng.shuffle(new)
    n_files = len(core) + len(new) + 2 * len(pairs)
    tokens = list(range(1, n_files + 1))
    rng.shuffle(tokens)
    items, key = [], {"seed": seed, "sheet": "audition", "items": []}

    def token_copy(src: str) -> str:
        name = f"s{tokens.pop():03d}.wav"
        shutil.copyfile(run.folder / src, sheet_dir / name)
        return name

    for section, group in (("core", core), ("pairs", pairs), ("new", new)):
        for g in group:
            qid = f"q{len(items) + 1:02d}"
            if section == "pairs":
                order = [0, 1]
                rng.shuffle(order)
                files = [token_copy(g["files"][k]) for k in order]
                items.append({"id": qid, "kind": "pair_same", "files": files, "section": section,
                              "label": "giọng Anh (England), " + ("nữ" if g["gender"] == "female" else "nam")})
                key["items"].append({"id": qid, "kind": "pair_same", "files": files, "section": section,
                                     "voices": [g["voices"][k] for k in order],
                                     "requests": [g["requests"][k] for k in order], "gender": g["gender"],
                                     "control": g.get("control")})
            else:
                name = token_copy(g["wav"])
                items.append({"id": qid, "kind": g["kind"], "files": [name], "section": section,
                              "label": "giọng " + ACCENT_LABELS_VI.get(g["cell"], g["cell"])})
                key["items"].append({"id": qid, "kind": g["kind"], "file": name, "section": section,
                                     "voice": g["voice"], "cell": g["cell"], "gender": g["gender"],
                                     "control": g.get("control"), "request": g["request"]})
    write_json(run.folder / "key.json", key)
    template = {"sheet": "audition", "seed": seed,
                "how": ("Mẫu câu trả lời. Phiếu sheet.html tự xuất file này khi bấm 'Tải kết quả (JSON)'. Nếu điền tay: "
                        "thang 1-5 ghi \"1\"..\"5\"; gender: \"Nữ\" | \"Nam\" | \"Không rõ\"; age: \"20s\" | \"30s\" | "
                        "\"40s\" | \"50+\"; usable: \"Có\" | \"Không\"; same: \"Cùng một người\" | \"Hai người khác nhau\" "
                        "| \"Không chắc\". Chấm xong: python tools/voice_lab.py score <file>."),
                "answers": {}}
    for item in items:
        fields = [q[0] for q in AUDITION_QUESTIONS[item["kind"]]] + ["comment"]
        template["answers"][item["id"]] = {f: "" for f in fields}
    write_json(sheet_dir / "answers-template.json", template)
    intro = ("Phiếu nghe 2 — tuyển giọng cho đề thi. Mỗi mẫu là một giọng đọc tiếng Anh; tên file là mã ngẫu nhiên nên bạn "
             "không biết đó là giọng nào. Phiếu có ba phần; có thể làm Phần 1–2 trong một buổi và Phần 3 trong buổi sau, vì "
             "câu trả lời được lưu tự động trong trình duyệt này. Nên nghe bằng tai nghe. Chấm 1–5 (5 là tốt nhất). Vài mục "
             "là mục kiểm tra (một giọng xuất hiện hai lần, hoặc giọng không đúng vùng) — cứ chấm bình thường. Xong thì bấm "
             "“Tải kết quả (JSON)” và gửi file đó lại.")
    page = sheet_html("Phiếu nghe 2 — tuyển giọng", intro, items, f"voice-lab-sheet2-{seed}", seed, "audition",
                      AUDITION_QUESTIONS, AUDITION_SECTIONS)
    write_text(sheet_dir / "sheet.html", page)
    run.save()
    say(f"audition sheet: {sheet_dir / 'sheet.html'} ({len(items)} items: {len(core)} core, {len(pairs)} pairs, "
        f"{len(new)} new; key in {run.folder / 'key.json'})")


# ---------------------------------------------------------------- score

def cmd_score(args) -> None:
    """The PO's sheet-2 answers + the blind key -> pools.json (scores) and a v2
    default_voices file ordered by PO score."""
    answers = json.loads(Path(args.answers).read_text(encoding="utf-8"))
    out = Path(args.out)
    key_path = Path(args.key) if args.key else out / "audition" / "key.json"
    key = json.loads(key_path.read_text(encoding="utf-8"))
    if str(answers.get("seed")) != str(key.get("seed")):
        raise SystemExit(f"answers are for seed {answers.get('seed')}, the key is for {key.get('seed')}")
    catalog_path = out / "catalog" / "catalog.json"
    by_id = {v.get("id"): v for v in load_catalog(out)} if catalog_path.is_file() else {}
    given = answers.get("answers") or {}
    per_voice: dict[str, dict] = {}
    controls, same_pairs = [], []
    unanswered = 0
    for item in key["items"]:
        a = given.get(item["id"]) or {}
        if not any(str(v).strip() for k, v in a.items() if k != "comment"):
            unanswered += 1
            continue
        if item.get("kind") == "pair_same":
            verdict = a.get("same")
            if item.get("control"):
                controls.append(f"same-voice pair {item['voices'][0]} ({item.get('gender')}): {verdict}")
            elif verdict == "Cùng một người":
                same_pairs.append(tuple(item["voices"]))
            continue
        if item.get("control") in ("classic-in-british", "classic-announcer"):
            controls.append(f"classic {item['voice']} shown as British ({item['cell']}): authenticity "
                            f"{a.get('authenticity')}" + (f", announcer fit {a.get('announcer_fit')}"
                                                          if item["cell"] == "announcer" else ""))
            continue
        rec = per_voice.setdefault(item["voice"], {"cell": item["cell"], "gender": item["gender"], "ratings": []})
        rec["ratings"].append(a)
        if item.get("control") == "repeat":
            controls.append(f"repeat of {item['voice']}")
    gender_vi = {"female": "Nữ", "male": "Nam"}
    pools: dict[str, dict[str, list]] = {}
    scores = {}
    for voice, rec in per_voice.items():
        def mean(field):
            vals = [float(r[field]) for r in rec["ratings"] if str(r.get(field, "")).strip()]
            return round(sum(vals) / len(vals), 2) if vals else None
        s = {"cell": rec["cell"], "gender": rec["gender"], "authenticity": mean("authenticity"),
             "intelligibility": mean("intelligibility"), "naturalness": mean("naturalness"),
             "gender_ok": all(r.get("gender") == gender_vi.get(rec["gender"]) for r in rec["ratings"]),
             "usable": all(r.get("usable") == "Có" for r in rec["ratings"])}
        if rec["cell"] == "announcer":
            s["announcer_fit"] = mean("announcer_fit")
        if len(rec["ratings"]) > 1:
            s["repeat_spread"] = {f: [r.get(f) for r in rec["ratings"]] for f in ("authenticity", "naturalness")}
        s["accepted"] = bool(s["authenticity"] and s["authenticity"] >= 4 and s["intelligibility"]
                             and s["intelligibility"] >= 4 and s["gender_ok"] and s["usable"])
        scores[voice] = s
        if s["accepted"] and rec["cell"] != "announcer":
            pools.setdefault(rec["cell"], {}).setdefault(rec["gender"], []).append(voice)

    def total(v):
        s = scores[v]
        return (s["authenticity"] or 0) + (s["naturalness"] or 0) + (s["intelligibility"] or 0)
    for cell in pools:
        for gender in pools[cell]:
            ranked = sorted(pools[cell][gender], key=lambda v: (-total(v), v))
            # Two voices the PO heard as one person never both sit in the first three.
            for a, b in same_pairs:
                if a in ranked[:3] and b in ranked[:3] and len(ranked) > 3:
                    lower = max((a, b), key=ranked.index)
                    ranked.remove(lower)
                    ranked.insert(3, lower)
            pools[cell][gender] = ranked
    announcers = sorted((v for v, s in scores.items() if s["cell"] == "announcer" and s["accepted"]),
                        key=lambda v: (-(scores[v].get("announcer_fit") or 0), -total(v), v))
    dropped = [a for a in NEW_ACCENTS if a in pools and not (pools[a].get("female") and pools[a].get("male"))]
    for a in dropped:
        del pools[a]

    def entry(v, cell, gender):
        return pool_entry(by_id[v], cell, gender) if v in by_id else {"id": v, "name": v, "description": ""}
    v2 = {"version": 2,
          "comment": f"Scored by tools/voice_lab.py score on {dt.date.today().isoformat()} from the PO's sheet-2 answers "
                     f"(seed {key.get('seed')}): accepted = authenticity >= 4, intelligibility >= 4, gender heard right, "
                     f"usable; ordered by PO score." + (f" Dropped: {', '.join(dropped)}." if dropped else "")}
    if announcers:
        v2["announcer"] = entry(announcers[0], "british", "male")
    v2["pools"] = {a: {g: [entry(v, a, g) for v in pools[a][g]] for g in ("male", "female") if g in pools[a]}
                   for a in CORE_ACCENTS + NEW_ACCENTS if a in pools}
    result = {"version": 2, "pools": pools, "announcer": announcers[0] if announcers else None, "scores": scores,
              "same_pairs": same_pairs, "controls": controls}
    write_json(out / "audition" / "pools.json", result)
    write_json(out / "default_voices.scored.json", v2)
    lines = ["# Audition scores (PO, sheet 2)", "", f"Unanswered items: {unanswered}.", "",
             "| cell | gender | accepted | needed | voices |", "|---|---|---|---|---|"]
    for cell in CORE_ACCENTS + NEW_ACCENTS:
        for gender in ("female", "male"):
            got = pools.get(cell, {}).get(gender, [])
            need = POOL_MINIMUM[cell]
            lines.append(f"| {cell} | {gender} | {len(got)} | {need}{' MISSING' if len(got) < need else ''} | "
                         f"{', '.join(got)} |")
    lines += ["", f"Announcer: {announcers[0] if announcers else 'none accepted'}", "",
              "Pairs heard as the same person: " + (", ".join(f"{a} = {b}" for a, b in same_pairs) or "none"), "",
              "Controls: " + ("; ".join(controls) or "none"), "",
              "An accent new in 0.8 without an accepted voice for each gender is not offered (dropped: "
              + (", ".join(dropped) or "none") + ")."]
    lines += ["", "| voice | cell | gender | authenticity | intelligibility | naturalness | gender ok | usable | accepted |",
              "|---|---|---|---|---|---|---|---|---|"]
    for v, s in sorted(scores.items(), key=lambda kv: (kv[1]["cell"], kv[1]["gender"], kv[0])):
        lines.append(f"| {v} | {s['cell']} | {s['gender']} | {s['authenticity']} | {s['intelligibility']} | "
                     f"{s['naturalness']} | {s['gender_ok']} | {s['usable']} | {s['accepted']} |")
    write_text(out / "audition" / "scores.md", "\n".join(lines) + "\n")
    say(f"pools.json and scores.md in {out / 'audition'}; v2 file {out / 'default_voices.scored.json'}")


# ---------------------------------------------------------------- report

SPEECH_CHUNK = re.compile(r"speech chunk")
FIELD = re.compile(r'(\w+)=("([^"]*)"|\S+)')


def cmd_report(args) -> None:
    if args.ear and args.max_usd is None:
        raise SystemExit("--ear makes paid requests: pass --max-usd (e.g. --max-usd 0.05).")
    out = Path(args.out)
    rows = []
    if args.wav:
        samples, rate = read_wav(Path(args.wav))
        report = f0_report(samples, rate, args.expect)
        rows.append({"file": str(args.wav), "f0": report})
        say(f"{args.wav}: {f0_line(report)}")
        for flag in report["flags"]:
            say(f"  FLAG {flag['start']:.2f}-{flag['end']:.2f} s median {flag['median_hz']} Hz "
                f"({int(flag['below_150'] * 100)}% of voiced frames < 150 Hz)")
    elif args.data_dir:
        cache = Path(args.data_dir) / "audio" / "cache"
        log = Path(args.log) if args.log else None
        voices_of: dict[str, list[str]] = {}
        if log and log.is_file():
            for line in log.read_text(encoding="utf-8", errors="replace").splitlines():
                if not SPEECH_CHUNK.search(line):
                    continue
                fields = {m.group(1): (m.group(3) if m.group(3) is not None else m.group(2)) for m in FIELD.finditer(line)}
                if "key" in fields and "voices" in fields:
                    voices_of[fields["key"].strip('"')] = [p.split("=", 1)[1] for p in fields["voices"].split(",") if "=" in p]
        catalog = {v.get("id"): v for v in load_catalog(out)} if (out / "catalog" / "catalog.json").is_file() else {}
        per_voice: dict[str, list[float]] = {}
        for wav in sorted(cache.glob("*.wav")):
            voices = voices_of.get(wav.stem, [])
            genders = sorted({gender_of(catalog.get(v, {})) for v in voices if catalog.get(v)})
            expect = (genders[0] + ("-pair" if len(voices) == 2 else "")) if len(genders) == 1 else (
                "mixed" if len(genders) == 2 else args.expect)
            samples, rate = read_wav(wav)
            report = f0_report(samples, rate, expect)
            rows.append({"file": wav.name, "voices": voices, "f0": report})
            if len(voices) == 1 and report["median_hz"]:
                per_voice.setdefault(voices[0], []).append(report["median_hz"])
            say(f"{wav.name[:12]} {','.join(voices) or '?'} ({expect}): {f0_line(report)}")
        for voice, medians in per_voice.items():
            spread = (max(medians) - min(medians)) / min(medians)
            say(f"  {voice}: medians {medians}, spread {spread:.1%}")
    else:
        raise SystemExit("report needs --wav F.wav or --data-dir DIR")
    if args.ear:
        key, source = find_key()
        say(f"key from {source}")
        run = Run(out / "report", Api(key), args.max_usd, "report")
        for row in rows:
            path = Path(args.wav) if args.wav else Path(args.data_dir) / "audio" / "cache" / row["file"]
            try:
                row["ear"] = run_ear(run, f"report.{Path(row['file']).stem[:16]}", path, "lean")
            except BudgetStop as stop:
                say(f"STOPPED: {stop}")
                break
    write_json(out / "report" / f"f0-{dt.datetime.now().strftime('%H%M%S')}.json", rows)


# ---------------------------------------------------------------- main

def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--out", default=str(default_out()), help="output folder (default TESTING_DUMP/voice-lab/<date>)")
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("catalog", help="dump every voice (free)")
    p = sub.add_parser("probe", help="experiments E1-E10 (paid)")
    p.add_argument("--max-usd", type=float)
    p.add_argument("--only", help="comma-separated experiments or request ids, e.g. E1,E2a,E4")
    p.add_argument("--design", action="store_true", help="also run E9: create ONE designed voice (kept)")
    p.add_argument("--no-ear", action="store_true", help="skip the AI ear")
    p.add_argument("--skip-done", action="store_true", help="skip requests already answered in this folder")
    p.add_argument("--reanalyse", action="store_true",
                   help="no synthesis: redo F0 on saved WAVs and run the ear where it has not answered")
    p = sub.add_parser("audition", help="candidate pools per accent x gender (paid)")
    p.add_argument("--max-usd", type=float)
    p.add_argument("--accents", help=f"comma-separated, from {', '.join(CELLS)}")
    p.add_argument("--per-cell", type=int, default=AUDITION_MAX_PER_CELL,
                   help=f"candidates ranked per accent x gender (at most {AUDITION_MAX_PER_CELL}); round 1 tries the "
                        "pool minimum, later rounds one more while a cell lacks a spare approved voice")
    p.add_argument("--dry-run", action="store_true", help="list the candidates only (free)")
    p.add_argument("--offline", action="store_true",
                   help="no requests: rebuild the proposal, proposal.md and sheet 2 from the folder's manifest")
    p.add_argument("--no-ear", action="store_true")
    p = sub.add_parser("score", help="PO answers + blind key -> pools.json, scores.md (free)")
    p.add_argument("answers")
    p.add_argument("--key", help="key.json (default <out>/audition/key.json)")
    p = sub.add_parser("report", help="F0 report on a WAV or an app data folder; optional AI ear")
    p.add_argument("--wav")
    p.add_argument("--expect", choices=["female", "male", "mixed", "female-pair", "male-pair"])
    p.add_argument("--data-dir")
    p.add_argument("--log")
    p.add_argument("--ear", action="store_true")
    p.add_argument("--max-usd", type=float)
    args = parser.parse_args()
    {"catalog": cmd_catalog, "probe": cmd_probe, "audition": cmd_audition, "score": cmd_score,
     "report": cmd_report}[args.command](args)


if __name__ == "__main__":
    main()
