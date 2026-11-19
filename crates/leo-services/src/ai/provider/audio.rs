use std::path::PathBuf;

pub fn expand_tilde(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => dirs::home_dir()
            .map(|h| h.join(rest))
            .unwrap_or_else(|| PathBuf::from(p)),
        None => PathBuf::from(p),
    }
}

pub fn models_dir() -> PathBuf {
    match std::env::var_os("LEO_HOME").filter(|v| !v.is_empty()) {
        Some(home) => PathBuf::from(home).join(leo_core::paths::own::MODELS),
        None => expand_tilde("~/.leo/models"),
    }
}

pub fn read_wav(bytes: &[u8]) -> Result<Vec<f32>, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a WAV file".to_string());
    }
    let mut at = 12;
    let mut format: Option<(u16, u16, u32, u16)> = None;
    let mut data: Option<&[u8]> = None;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]])
            as usize;
        let start = at + 8;
        let end = start.saturating_add(size).min(bytes.len());
        if id == b"fmt " && end - start >= 16 {
            let f = &bytes[start..end];
            format = Some((
                u16::from_le_bytes([f[0], f[1]]),
                u16::from_le_bytes([f[2], f[3]]),
                u32::from_le_bytes([f[4], f[5], f[6], f[7]]),
                u16::from_le_bytes([f[14], f[15]]),
            ));
        } else if id == b"data" {
            data = Some(&bytes[start..end]);
            break;
        }
        at = start.saturating_add(size).saturating_add(size & 1);
    }
    let (encoding, channels, rate, bits) = format.ok_or("the WAV file has no format")?;
    let data = data.ok_or("the WAV file has no audio")?;
    if encoding != 1 || bits != 16 || channels == 0 || rate == 0 {
        return Err(format!(
            "unsupported WAV: encoding {encoding}, {bits}-bit, {channels} channels"
        ));
    }
    let channels = channels as usize;
    let mono: Vec<f32> = data
        .as_chunks::<2>()
        .0
        .chunks_exact(channels)
        .map(|frame| {
            frame
                .iter()
                .map(|b| i16::from_le_bytes(*b) as f32 / 32768.0)
                .sum::<f32>()
                / channels as f32
        })
        .collect();
    Ok(resample(&mono, rate, 16_000))
}

fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || samples.is_empty() {
        return samples.to_vec();
    }
    let out_len = (samples.len() as u64 * to as u64 / from as u64) as usize;
    let step = from as f64 / to as f64;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * step;
            let left = pos.floor() as usize;
            let right = (left + 1).min(samples.len() - 1);
            let frac = (pos - left as f64) as f32;
            samples[left] * (1.0 - frac) + samples[right] * frac
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(channels: u16, rate: u32, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + 12 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"LIST");
        out.extend_from_slice(&4u32.to_le_bytes());
        out.extend_from_slice(b"INFO");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2 * channels as u32).to_le_bytes());
        out.extend_from_slice(&(2 * channels).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    #[test]
    fn a_leo_segment_reads_back_as_samples_between_minus_one_and_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        crate::session::wav::write(&path, &[0, 16384, -32768]).unwrap();
        let samples = read_wav(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(samples, vec![0.0, 0.5, -1.0]);
    }

    #[test]
    fn stereo_is_mixed_down_and_other_rates_are_resampled_after_skipping_other_chunks() {
        let stereo = wav(2, 16_000, &[1000, 3000, -2000, -4000]);
        assert_eq!(
            read_wav(&stereo).unwrap(),
            vec![2000.0 / 32768.0, -3000.0 / 32768.0]
        );
        let fast = wav(1, 32_000, &[0; 3200]);
        assert_eq!(read_wav(&fast).unwrap().len(), 1600);
    }

    #[test]
    fn a_file_that_is_not_pcm_wav_is_refused_plainly() {
        assert!(read_wav(b"hello").is_err());
        let mut float = wav(1, 16_000, &[0; 4]);
        float[20 + 12] = 3;
        assert!(read_wav(&float).unwrap_err().contains("unsupported"));
    }

    #[test]
    fn models_follow_leo_home() {
        assert!(models_dir().ends_with("models"));
    }
}
