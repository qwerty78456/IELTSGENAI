# Voices: how they were chosen and what was measured

This is the record behind the 0.8 voice work: why recordings sounded like one American woman,
what was tested against the real Gemini 3.8 TTS API, which decisions follow, and how the
built-in voice pools were chosen. Re-run the tools below when Google changes the catalogue,
when an accent is added, or before changing a decision.

Tools: `tools/voice_lab.py` (catalog, probe, audition, score, report; stdlib + numpy, reads the
key like the app and sends it only in the header) and the ignored Rust test `voice_live_probe`
(`src/infrastructure/tts/voices.rs`). Raw data, WAVs and listening sheets are kept under
`TESTING_DUMP/voice-lab/<date>/` (not in git).

## What was wrong in 0.7.1 (2026-10-05)

Evidence: the teacher's HSG Part 1 recording (three speakers, all set to Female, British
English), its six cache chunks, the log and the transcript.

- **Accent never reached the model.** Gender + accent picked one of the 30 classic voices
  (`voices.json`). Google's catalogue (`GET /v1beta/voices`) lists all 30 as `en-US`,
  "General American". The style was a fixed sentence with no accent, and Google says accent
  belongs to the voice anyway, not to the style.
- **One voice per gender and accent.** All three speakers were sent as `Zephyr` in all six
  requests: recomputing the SHA-256 cache keys matches only that combination.
- **Two speakers on one voice make Gemini improvise.** In 2 of the 4 two-speaker chunks the
  second speaker came out as a deep male voice (F0 about 95 Hz against 170-195 Hz).
- The AI ear (below) heard the three speakers of the 0.7.1 file as one female American voice.

The request JSON itself was right: it matches Google's documented shape, and different voice
names do produce different voices.

## Decisions (gates)

Measured on 2026-10-05 with about $1.3 of probes (`probe`, `audition`, the controls, and two
full re-reads of the HSG passage). n is small; the PO's listening sheets decide what the
measurements leave open.

| Gate | Result | Decision in code |
|---|---|---|
| G1 library voices in a two-speaker request | Accepted (`en-gb-*` pairs, library + classic, cross-region); the ear hears both speakers with the configured genders | Keep two-speaker chunks (`PER_TURN_ALL = false`). |
| G2 cause of the gender flip | Reproduced with the exact 0.7.1 requests: Zephyr + Zephyr flipped the second speaker in 3 of 5 takes; two distinct `en-gb` voices in 0 of 5 (one-sided Fisher p = 0.08; with earlier takes 5/12 vs 0/8, p = 0.05) | A part never shares a voice between speakers (`assign_voices`, `voice_conflicts`, refused at job start). |
| G3 inline tags | Never read aloud and performed in most takes: `<sigh>` (7/7), `<cough>` (5/5), `<chuckle>` (3/5; missed as the first word), `<laugh>` (7/10; in a two-speaker turn opening it came out as a breath). Read aloud at least once: `<long pause>`, `<whispers>`. `<short pause>` adds 0.7-1.2 s mid-phrase but the ear rarely notices it | Allowed tags: `<sigh>`, `<cough>`, `<laugh>`, `<chuckle>`, mid-sentence. |
| G4 designed voices in a two-speaker request | Accepted (against Google's docs) | Still read one turn per request (`reads_alone`), as documented; one constant flips it. |
| G5 a different style on each line | The style changes delivery but also moves the voice (F0 -18 % on a "calm" line; +20 % on a "worried" turn) | One short style per speaker for every turn, from the role; per-line emotion goes through the allowed tags only (`PER_TURN_STYLE`). |
| G6 `speech_config[].language` | Accepted; no reliable effect measured | Not sent. |
| G7 `generation_config.seed` | Accepted but ignored: two takes with seed 42 differ | A new take skips the speech cache instead. |
| G8 drift across requests | One voice in three requests: F0 spread 11-14 %, heard as one speaker | Nothing to change. |
| G9 `gemini-3.8-flash-lite-tts` | Library voices accepted; the same female voice is 20-24 % higher than on Flash TTS | Works; voices sound different on lite. |

Other measured facts:
- Every voice in a request bills its reference audio as input tokens: classics 200-260,
  library voices 740-1,970, designed voices 490-1,220 (at most about $0.001 a request).
- Output is 32 audio tokens per second on both TTS models.
- A designed voice can be created on the Gemini API with this key (`POST /v1beta/voices`,
  `store: true`, no `voice.model`): 21 s, the response is the Voice with a 20 s sample.
- `GET /v1beta/voices/{id}` returns 404 for library voices; only designed voices carry a sample.

## The AI ear and its limits

`voice_lab.py` asks `gemini-3.8-flash` to listen to a WAV (audio input, JSON answer). Use it as
a screen, never as the judge:
- Gender: matched the catalogue on 64 of 66 samples (the two misses are now left out of the pools).
- American vs not American: reliable.
- Other accents: it hears most non-rhotic English as "England" (South African 6/6, Irish 4/6,
  New Zealand 3/8, Australian 2/8), and asked twice about the same clip it changed its answer in
  2 of 4 cases.
- Same or different speaker: unreliable. It called a British woman at 188 Hz and an Australian
  woman at 149 Hz "the same speaker" with high confidence.

## How the pools were chosen

`voice_lab.py audition` synthesised one accent-heavy sentence per candidate voice (up to 6 per
accent and gender, chosen from the catalogue by persona and pitch), ran the ear and F0, and
approved voices heard with the right gender, the cell's accent (or, where the ear cannot tell,
kept on the catalogue's word), and clarity 4-5. Within a pool the order maximises contrast: the
best-rated voice first, then each next voice as far as possible (timbre MFCCs plus F0) from the
ones before it, so the first speakers of a part get the most different voices.

| Accent | Female | Male | Confirmed by the ear |
|---|---|---|---|
| British (`en-GB`, Winchester) | 5 | 4 | yes |
| American (`en-US`) | 4 | 4 | yes (includes the classics `despina`, `rasalgethi`) |
| Australian (`en-AU`, Sydney) | 4 | 3 | female yes; male 2 of 3 |
| Canadian (`en-CA`) | 3 | 3 | no: heard as American, kept on the catalogue's word |
| New Zealand (`en-NZ`, Auckland) | 3 | 3 | male yes; female 1 of 3 |
| Irish (`en-IE`, Dublin) | 2 | 2 | 1 of 2 per gender |
| Scottish (`en-GB`, Glasgow) | 3 | 3 | yes |
| South African (`en-ZA`, Cape Town) | 2 | 2 | no: heard as England, kept on the catalogue's word |
| Indian (`en-IN`) | 2 | 2 | yes |

Announcer: `en-gb-tutor-9` (male, Winchester). The pools file is
`src/infrastructure/tts/default_voices.json`; its `comment` lists the voices kept on the
catalogue's word. They stay until the PO has listened to the audition sheet; replace them
there, or override a pool in `voices.json` version 2.

## The HSG Part 1 case, before and after

Same passage (701 words, three speakers set to Female, British English), read again through
`plan_passage` with the new pools (`voice_live_probe` with `PROBE_PASSAGE`):

| | 0.7.1 | 0.8 |
|---|---|---|
| Voices sent | `Zephyr` for A, B and C | three different `en-gb` voices |
| AI ear on one stretch per speaker | female, American | female, British |
| Turns that flipped to a male voice (F0 < 120 Hz) | 2 of 10 | 0 |
| Requests / cost | 6 / $0.093 | 6 / $0.106 |

## Re-running

```bash
python tools/voice_lab.py catalog
python tools/voice_lab.py probe --max-usd 0.30
python tools/voice_lab.py audition --max-usd 0.45
python tools/voice_lab.py report --wav recording.wav --expect female
cargo test --features server --no-default-features voice_live_probe -- --ignored --nocapture
```

`voice_live_probe` checks for free that every pooled voice still exists with its gender and
language, then spends about $0.003 on a three-speaker passage. With `PROBE_PASSAGE` (a
"Speaker A: ..." file), `PROBE_SPEAKERS` (`female:british:host,...`) and `PROBE_OUT` it also
reads a whole passage the way the app does (about $0.10 for 700 words).
