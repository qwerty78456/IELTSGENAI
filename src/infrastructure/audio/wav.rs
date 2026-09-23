//! Minimal 16-bit mono PCM buffer with WAV (RIFF) encoding and decoding.
//! No external crate: the formats involved are fixed by Gemini (24 kHz, mono).

/// Sample rate of Gemini TTS output and of every rendered recording.
pub const SAMPLE_RATE: u32 = 24_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pcm16 {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
}

impl Pcm16 {
    pub fn from_le_bytes(bytes: &[u8], sample_rate: u32) -> Self {
        let samples = bytes
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        Self {
            samples,
            sample_rate,
        }
    }

    pub fn silence(ms: u32, sample_rate: u32) -> Self {
        Self {
            samples: vec![0; samples_for(ms, sample_rate)],
            sample_rate,
        }
    }

    /// A sine tone with a short fade so it does not click.
    pub fn tone(hz: f32, ms: u32, amplitude: f32, sample_rate: u32) -> Self {
        let count = samples_for(ms, sample_rate);
        let fade = (sample_rate / 100) as usize; // 10 ms
        let samples = (0..count)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                let env = if i < fade {
                    i as f32 / fade as f32
                } else if i + fade > count {
                    (count - i) as f32 / fade as f32
                } else {
                    1.0
                };
                (amplitude * env * (2.0 * std::f32::consts::PI * hz * t).sin() * i16::MAX as f32)
                    as i16
            })
            .collect();
        Self {
            samples,
            sample_rate,
        }
    }

    pub fn duration_ms(&self) -> u32 {
        (self.samples.len() as u64 * 1000 / u64::from(self.sample_rate.max(1))) as u32
    }

    /// Appends `other`; both buffers must share a sample rate.
    pub fn append(&mut self, other: &Pcm16) {
        debug_assert_eq!(self.sample_rate, other.sample_rate, "sample rates differ");
        self.samples.extend_from_slice(&other.samples);
    }

    pub fn to_wav(&self) -> Vec<u8> {
        let data_len = (self.samples.len() * 2) as u32;
        let mut wav = Vec::with_capacity(44 + data_len as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&self.sample_rate.to_le_bytes());
        wav.extend_from_slice(&(self.sample_rate * 2).to_le_bytes()); // byte rate
        wav.extend_from_slice(&2u16.to_le_bytes()); // block align
        wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        for sample in &self.samples {
            wav.extend_from_slice(&sample.to_le_bytes());
        }
        wav
    }

    /// Decodes a 16-bit PCM mono WAV. Anything else is rejected with a message.
    pub fn from_wav(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return Err("not a RIFF/WAVE file".into());
        }
        let mut pos = 12;
        let mut sample_rate = None;
        while pos + 8 <= bytes.len() {
            let id = &bytes[pos..pos + 4];
            let size = u32::from_le_bytes([
                bytes[pos + 4],
                bytes[pos + 5],
                bytes[pos + 6],
                bytes[pos + 7],
            ]) as usize;
            let body = pos + 8;
            let end = body
                .checked_add(size)
                .filter(|end| *end <= bytes.len())
                .ok_or("truncated WAV chunk")?;
            match id {
                b"fmt " => {
                    if size < 16 {
                        return Err("fmt chunk too short".into());
                    }
                    let format = u16::from_le_bytes([bytes[body], bytes[body + 1]]);
                    let channels = u16::from_le_bytes([bytes[body + 2], bytes[body + 3]]);
                    let rate = u32::from_le_bytes([
                        bytes[body + 4],
                        bytes[body + 5],
                        bytes[body + 6],
                        bytes[body + 7],
                    ]);
                    let bits = u16::from_le_bytes([bytes[body + 14], bytes[body + 15]]);
                    if format != 1 || channels != 1 || bits != 16 {
                        return Err(format!(
                            "unsupported WAV: format {format}, {channels} channel(s), {bits} bits; need PCM mono 16-bit"
                        ));
                    }
                    sample_rate = Some(rate);
                }
                b"data" => {
                    let rate = sample_rate.ok_or("data chunk before fmt chunk")?;
                    if size % 2 != 0 {
                        return Err("incomplete 16-bit WAV sample".into());
                    }
                    return Ok(Self::from_le_bytes(&bytes[body..end], rate));
                }
                _ => {}
            }
            pos = body + size + (size & 1);
        }
        Err("no data chunk".into())
    }
}

fn samples_for(ms: u32, sample_rate: u32) -> usize {
    (u64::from(ms) * u64::from(sample_rate) / 1000) as usize
}

/// Size of the header `to_wav` writes.
const WAV_HEADER_LEN: u64 = 44;

/// Duration of a recording written by `to_wav`, from the file's length in
/// bytes; lets the server describe a WAV on disk without reading it.
pub fn duration_ms_for_len(wav_len: u64) -> u32 {
    let data_len = wav_len.saturating_sub(WAV_HEADER_LEN);
    (data_len * 1000 / (u64::from(SAMPLE_RATE) * 2)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_for_len_matches_pcm() {
        let wav = Pcm16::silence(1_500, SAMPLE_RATE).to_wav();
        assert_eq!(duration_ms_for_len(wav.len() as u64), 1_500);
        assert_eq!(duration_ms_for_len(0), 0);
    }

    #[test]
    fn silence_and_tone_have_the_right_length() {
        assert_eq!(Pcm16::silence(500, SAMPLE_RATE).samples.len(), 12_000);
        let tone = Pcm16::tone(1000.0, 250, 0.5, SAMPLE_RATE);
        assert_eq!(tone.duration_ms(), 250);
        assert!(tone.samples.iter().any(|s| *s != 0));
    }

    #[test]
    fn wav_round_trips() {
        let mut pcm = Pcm16::tone(440.0, 20, 0.3, SAMPLE_RATE);
        pcm.append(&Pcm16::silence(10, SAMPLE_RATE));
        let decoded = Pcm16::from_wav(&pcm.to_wav()).unwrap();
        assert_eq!(decoded, pcm);
    }

    #[test]
    fn rejects_stereo() {
        let mut wav = Pcm16::silence(1, SAMPLE_RATE).to_wav();
        wav[22] = 2; // channels
        assert!(Pcm16::from_wav(&wav).is_err());
    }

    #[test]
    fn rejects_truncated_chunks_without_panicking() {
        let wav = Pcm16::silence(1, SAMPLE_RATE).to_wav();
        for length in 0..wav.len() {
            assert!(Pcm16::from_wav(&wav[..length]).is_err(), "length {length}");
        }
    }
}
