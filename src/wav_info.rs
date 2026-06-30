use crate::audio_buffer::AudioBuffer;

#[derive(Debug)]
pub struct WavInfo {
    pub path: String,
    pub sample_rate: u32,
    pub duration_secs: f32,
    pub frames: u32,
    pub total_samples: usize,
    pub channels: usize,
    pub bits_per_sample: u16,
}

#[derive(Debug)]
pub struct WavInfoChecks {
    pub same_sample_rate: bool,
    pub same_sample_count: bool,
}

impl WavInfo {
    pub fn from_audio_buffer(path: String, wav: &AudioBuffer) -> Self {
        Self {
            path,
            sample_rate: wav.sample_rate,
            duration_secs: wav.frames as f32 / wav.sample_rate as f32,
            frames: wav.frames,
            total_samples: wav.samples.len(),
            channels: wav.channels,
            bits_per_sample: wav.bits_per_sample,
        }
    }
}

pub fn compare_wav_info(infos: &[WavInfo]) -> WavInfoChecks {
    WavInfoChecks {
        same_sample_rate: all_same(infos, |info| info.sample_rate),
        same_sample_count: all_same(infos, |info| info.frames),
    }
}

fn all_same<T>(infos: &[WavInfo], value: impl Fn(&WavInfo) -> T) -> bool
where
    T: Eq,
{
    infos
        .split_first()
        .is_none_or(|(first, rest)| rest.iter().all(|info| value(info) == value(first)))
}

#[cfg(test)]
mod tests {
    use crate::audio_buffer::AudioBuffer;

    use super::{WavInfo, compare_wav_info};

    const SAMPLE_RATE: u32 = 44_100;

    #[test]
    fn compares_matching_files() {
        let first = WavInfo::from_audio_buffer("first.wav".to_string(), &buffer(vec![0.1; 1000]));
        let second = WavInfo::from_audio_buffer("second.wav".to_string(), &buffer(vec![0.2; 1000]));

        let checks = compare_wav_info(&[first, second]);

        assert!(checks.same_sample_rate);
        assert!(checks.same_sample_count);
    }

    #[test]
    fn detects_sample_count_mismatch() {
        let first = WavInfo::from_audio_buffer("first.wav".to_string(), &buffer(vec![0.1; 1000]));
        let second = WavInfo::from_audio_buffer("second.wav".to_string(), &buffer(vec![0.1; 900]));

        let checks = compare_wav_info(&[first, second]);

        assert!(!checks.same_sample_count);
    }

    fn buffer(samples: Vec<f32>) -> AudioBuffer {
        AudioBuffer {
            frames: samples.len() as u32,
            samples,
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 32,
            id: "test.wav".to_string(),
        }
    }
}
