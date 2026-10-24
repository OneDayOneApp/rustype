//! Small, dependency-free DSP helpers: resampling, silence trimming and WAV.

/// Root-mean-square level of a block of samples.
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|s| s * s).sum();
    (sum / samples.len() as f32).sqrt()
}

/// Linear-interpolation resampler. Adequate for 48k -> 16k speech and keeps the
/// crate free of large DSP dependencies.
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() || from == 0 || to == 0 {
        return input.to_vec();
    }
    let ratio = to as f64 / from as f64;
    let out_len = ((input.len() as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    let last = input.len() - 1;
    for i in 0..out_len {
        let src = (i as f64) / ratio;
        let idx = src.floor() as usize;
        let frac = (src - idx as f64) as f32;
        let a = input[idx.min(last)];
        let b = input[(idx + 1).min(last)];
        out.push(a + (b - a) * frac);
    }
    out
}

/// Downmix interleaved channels to mono.
pub fn downmix(interleaved: &[f32], channels: u16) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    if ch == 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

/// Trim leading/trailing silence, keeping `pad_ms` of padding on each side.
pub fn trim_silence(samples: &[f32], sample_rate: u32, threshold: f32, pad_ms: u32) -> Vec<f32> {
    let Some(first) = samples.iter().position(|s| s.abs() > threshold) else {
        return Vec::new();
    };
    let last = samples
        .iter()
        .rposition(|s| s.abs() > threshold)
        .unwrap_or(first);
    let pad = (sample_rate as u64 * pad_ms as u64 / 1000) as usize;
    let start = first.saturating_sub(pad);
    let end = (last + 1 + pad).min(samples.len());
    samples[start..end].to_vec()
}

/// Convert to 16-bit PCM with saturation.
pub fn to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect()
}

/// Encode 16-bit PCM as a RIFF/WAVE file in memory.
pub fn encode_wav(samples: &[i16], sample_rate: u32, channels: u16) -> Vec<u8> {
    let channels = channels.max(1);
    let bits = 16u16;
    let block_align = channels * bits / 8;
    let byte_rate = sample_rate * block_align as u32;
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);

    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // PCM chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM format
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Total milliseconds of frames whose RMS reaches `threshold`.
///
/// Frame-level voicing is far more robust than a whole-recording RMS check:
/// quiet microphones still produce loud short frames for speech, while steady
/// background noise stays below the threshold.
pub fn voiced_ms(samples: &[f32], sample_rate: u32, threshold: f32, frame_ms: u32) -> u64 {
    let frame = (sample_rate as u64 * frame_ms as u64 / 1000) as usize;
    if frame == 0 || sample_rate == 0 {
        return 0;
    }
    let mut voiced = 0u64;
    for chunk in samples.chunks(frame) {
        if chunk.len() < frame / 2 {
            break;
        }
        if rms(chunk) >= threshold {
            voiced += chunk.len() as u64 * 1000 / sample_rate as u64;
        }
    }
    voiced
}

pub fn duration_ms(samples: usize, sample_rate: u32) -> u64 {
    if sample_rate == 0 {
        return 0;
    }
    samples as u64 * 1000 / sample_rate as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_is_valid() {
        let pcm = vec![0i16; 160];
        let wav = encode_wav(&pcm, 16_000, 1);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(wav[4..8].try_into().unwrap()), 36 + 320);
        assert_eq!(u16::from_le_bytes(wav[20..22].try_into().unwrap()), 1);
        assert_eq!(u16::from_le_bytes(wav[22..24].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
        assert_eq!(wav.len(), 44 + 320);
    }

    #[test]
    fn resample_changes_length_proportionally() {
        let input: Vec<f32> = (0..480).map(|i| (i as f32 * 0.01).sin()).collect();
        let out = resample(&input, 48_000, 16_000);
        assert_eq!(out.len(), 160);
    }

    #[test]
    fn trims_silence_but_keeps_padding() {
        let mut s = vec![0.0f32; 1000];
        s[400..600].fill(0.5);
        let trimmed = trim_silence(&s, 16_000, 0.01, 10);
        // 400 - 160 samples of padding .. 600 + 160
        assert_eq!(trimmed.len(), 520);
    }

    #[test]
    fn all_silence_trims_to_empty() {
        assert!(trim_silence(&vec![0.0f32; 100], 16_000, 0.01, 5).is_empty());
    }

    #[test]
    fn voiced_ms_counts_speech_frames() {
        let rate = 16_000;
        let mut samples = vec![0.0f32; rate as usize]; // 1s silence
        // 0.3s tone at 300 Hz, amplitude 0.05
        for i in 0..(rate as usize * 3 / 10) {
            samples[i] = 0.05 * (i as f32 * 2.0 * std::f32::consts::PI * 300.0 / rate as f32).sin();
        }
        let voiced = voiced_ms(&samples, rate, 0.004, 20);
        assert!(voiced >= 250 && voiced <= 320, "voiced={voiced}");
    }

    #[test]
    fn downmix_averages_channels() {
        let out = downmix(&[1.0, 0.0, 0.5, 0.5], 2);
        assert_eq!(out, vec![0.5, 0.5]);
    }
}

/// Parse a RIFF/WAVE file into mono f32 samples.
///
/// Supports PCM 16-bit and IEEE float 32-bit, which covers the files the CLI is
/// expected to transcribe.
pub fn parse_wav(bytes: &[u8]) -> crate::error::Result<(Vec<f32>, u32, u16)> {
    use crate::error::Error;

    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(Error::Audio("not a RIFF/WAVE file".into()));
    }
    let mut pos = 12usize;
    let mut format = None;
    let mut channels = 0u16;
    let mut rate = 0u32;
    let mut bits = 0u16;
    let mut data: Option<&[u8]> = None;

    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let len = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body_start = pos + 8;
        let body_end = body_start.saturating_add(len).min(bytes.len());
        let body = &bytes[body_start..body_end];
        match id {
            b"fmt " => {
                if body.len() < 16 {
                    return Err(Error::Audio("truncated fmt chunk".into()));
                }
                format = Some(u16::from_le_bytes(body[0..2].try_into().unwrap()));
                channels = u16::from_le_bytes(body[2..4].try_into().unwrap());
                rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
                bits = u16::from_le_bytes(body[14..16].try_into().unwrap());
            }
            b"data" => data = Some(body),
            _ => {}
        }
        pos = body_start + len + (len & 1);
    }

    let format = format.ok_or_else(|| Error::Audio("missing fmt chunk".into()))?;
    let data = data.ok_or_else(|| Error::Audio("missing data chunk".into()))?;
    if channels == 0 || rate == 0 {
        return Err(Error::Audio("invalid fmt values".into()));
    }
    let samples: Vec<f32> = match (format, bits) {
        (1, 16) => data
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
            .collect(),
        (3, 32) => data
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect(),
        _ => {
            return Err(Error::Audio(format!(
                "unsupported wav encoding (format {format}, {bits} bits)"
            )))
        }
    };
    Ok((samples, rate, channels))
}

#[cfg(test)]
mod wav_tests {
    use super::*;

    #[test]
    fn parses_own_encoder_output() {
        let pcm: Vec<i16> = (0..160).map(|i| (i * 100) as i16).collect();
        let wav = encode_wav(&pcm, 16_000, 1);
        let (samples, rate, ch) = parse_wav(&wav).unwrap();
        assert_eq!(rate, 16_000);
        assert_eq!(ch, 1);
        assert_eq!(samples.len(), 160);
        assert!((samples[1] - (100.0 / 32768.0)).abs() < 1e-6);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_wav(b"not a wav").is_err());
    }
}
