use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

pub const SAMPLE_RATE: u32 = 44_100;
pub const CHANNELS: u16 = 1;
pub const BITS_PER_SAMPLE: u16 = 32;
pub const FADE_MS: f32 = 5.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignalGroup {
    Sine,
    Sweep,
    TwoTone,
    Noise,
}

#[derive(Clone, Copy, Debug)]
pub enum SignalSpec {
    Sine { frequency_hz: f32 },
    LogSweep { start_hz: f32, end_hz: f32 },
    TwoTone { first_hz: f32, second_hz: f32 },
    WhiteNoise,
    PinkNoise,
}

#[derive(Clone, Copy, Debug)]
pub struct SignalPreset {
    pub file_name: &'static str,
    pub group: SignalGroup,
    pub spec: SignalSpec,
    pub peak_dbfs: f32,
    pub duration_secs: f32,
}

const SINE_DURATION_SECS: f32 = 3.0;
const SWEEP_DURATION_SECS: f32 = 10.0;
const NOISE_DURATION_SECS: f32 = 3.0;
const DEFAULT_PEAK_DBFS: f32 = -24.0;

pub const PRESETS: &[SignalPreset] = &[
    SignalPreset {
        file_name: "sine_1000_-36.wav",
        group: SignalGroup::Sine,
        spec: SignalSpec::Sine {
            frequency_hz: 1000.0,
        },
        peak_dbfs: -36.0,
        duration_secs: SINE_DURATION_SECS,
    },
    SignalPreset {
        file_name: "sine_1000_-30.wav",
        group: SignalGroup::Sine,
        spec: SignalSpec::Sine {
            frequency_hz: 1000.0,
        },
        peak_dbfs: -30.0,
        duration_secs: SINE_DURATION_SECS,
    },
    SignalPreset {
        file_name: "sine_1000_-24.wav",
        group: SignalGroup::Sine,
        spec: SignalSpec::Sine {
            frequency_hz: 1000.0,
        },
        peak_dbfs: -24.0,
        duration_secs: SINE_DURATION_SECS,
    },
    SignalPreset {
        file_name: "sine_1000_-18.wav",
        group: SignalGroup::Sine,
        spec: SignalSpec::Sine {
            frequency_hz: 1000.0,
        },
        peak_dbfs: -18.0,
        duration_secs: SINE_DURATION_SECS,
    },
    SignalPreset {
        file_name: "sine_1000_-12.wav",
        group: SignalGroup::Sine,
        spec: SignalSpec::Sine {
            frequency_hz: 1000.0,
        },
        peak_dbfs: -12.0,
        duration_secs: SINE_DURATION_SECS,
    },
    SignalPreset {
        file_name: "sine_1000_-6.wav",
        group: SignalGroup::Sine,
        spec: SignalSpec::Sine {
            frequency_hz: 1000.0,
        },
        peak_dbfs: -6.0,
        duration_secs: SINE_DURATION_SECS,
    },
    SignalPreset {
        file_name: "sine_100_-24.wav",
        group: SignalGroup::Sine,
        spec: SignalSpec::Sine {
            frequency_hz: 100.0,
        },
        peak_dbfs: -24.0,
        duration_secs: SINE_DURATION_SECS,
    },
    SignalPreset {
        file_name: "sine_100_-12.wav",
        group: SignalGroup::Sine,
        spec: SignalSpec::Sine {
            frequency_hz: 100.0,
        },
        peak_dbfs: -12.0,
        duration_secs: SINE_DURATION_SECS,
    },
    SignalPreset {
        file_name: "sine_5000_-24.wav",
        group: SignalGroup::Sine,
        spec: SignalSpec::Sine {
            frequency_hz: 5000.0,
        },
        peak_dbfs: -24.0,
        duration_secs: SINE_DURATION_SECS,
    },
    SignalPreset {
        file_name: "sweep_20_20k_-24.wav",
        group: SignalGroup::Sweep,
        spec: SignalSpec::LogSweep {
            start_hz: 20.0,
            end_hz: 20_000.0,
        },
        peak_dbfs: -24.0,
        duration_secs: SWEEP_DURATION_SECS,
    },
    SignalPreset {
        file_name: "sweep_20_20k_-6.wav",
        group: SignalGroup::Sweep,
        spec: SignalSpec::LogSweep {
            start_hz: 20.0,
            end_hz: 20_000.0,
        },
        peak_dbfs: -6.0,
        duration_secs: SWEEP_DURATION_SECS,
    },
    SignalPreset {
        file_name: "two_tone_700_1900.wav",
        group: SignalGroup::TwoTone,
        spec: SignalSpec::TwoTone {
            first_hz: 700.0,
            second_hz: 1900.0,
        },
        peak_dbfs: DEFAULT_PEAK_DBFS,
        duration_secs: SINE_DURATION_SECS,
    },
    SignalPreset {
        file_name: "pink_noise.wav",
        group: SignalGroup::Noise,
        spec: SignalSpec::PinkNoise,
        peak_dbfs: DEFAULT_PEAK_DBFS,
        duration_secs: NOISE_DURATION_SECS,
    },
    SignalPreset {
        file_name: "white_noise.wav",
        group: SignalGroup::Noise,
        spec: SignalSpec::WhiteNoise,
        peak_dbfs: DEFAULT_PEAK_DBFS,
        duration_secs: NOISE_DURATION_SECS,
    },
];

#[derive(Debug)]
pub struct UnknownPreset {
    target: String,
}

impl Display for UnknownPreset {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        writeln!(f, "Unknown generator target: {}", self.target)?;
        write!(f, "Expected all, sine, sweep, or one of:")?;
        for preset in PRESETS {
            write!(f, " {}", preset_name(preset))?;
        }
        Ok(())
    }
}

impl Error for UnknownPreset {}

pub fn select_presets(target: &str) -> Result<Vec<&'static SignalPreset>, UnknownPreset> {
    let normalized = target.trim_end_matches(".wav");

    match normalized {
        "all" => Ok(PRESETS.iter().collect()),
        "sine" => Ok(PRESETS
            .iter()
            .filter(|preset| preset.group == SignalGroup::Sine)
            .collect()),
        "sweep" => Ok(PRESETS
            .iter()
            .filter(|preset| preset.group == SignalGroup::Sweep)
            .collect()),
        "two-tone" | "two_tone" => Ok(PRESETS
            .iter()
            .filter(|preset| preset.group == SignalGroup::TwoTone)
            .collect()),
        "noise" => Ok(PRESETS
            .iter()
            .filter(|preset| preset.group == SignalGroup::Noise)
            .collect()),
        target => PRESETS
            .iter()
            .find(|preset| preset_name(preset) == target)
            .map(|preset| vec![preset])
            .ok_or_else(|| UnknownPreset {
                target: target.to_string(),
            }),
    }
}

pub fn preset_name(preset: &SignalPreset) -> &str {
    preset
        .file_name
        .strip_suffix(".wav")
        .unwrap_or(preset.file_name)
}

#[cfg(test)]
mod tests {
    use super::{PRESETS, SignalGroup, select_presets};

    const MINIMAL_SET: &[&str] = &[
        "sine_1000_-36.wav",
        "sine_1000_-30.wav",
        "sine_1000_-24.wav",
        "sine_1000_-18.wav",
        "sine_1000_-12.wav",
        "sine_1000_-6.wav",
        "sine_100_-24.wav",
        "sine_100_-12.wav",
        "sine_5000_-24.wav",
        "sweep_20_20k_-24.wav",
        "sweep_20_20k_-6.wav",
        "two_tone_700_1900.wav",
        "pink_noise.wav",
        "white_noise.wav",
    ];

    #[test]
    fn all_presets_match_minimal_set() {
        let files = PRESETS
            .iter()
            .map(|preset| preset.file_name)
            .collect::<Vec<_>>();

        assert_eq!(files, MINIMAL_SET);
    }

    #[test]
    fn selects_sine_group() {
        let presets = select_presets("sine").expect("sine target should be valid");

        assert_eq!(presets.len(), 9);
        assert!(
            presets
                .iter()
                .all(|preset| preset.group == SignalGroup::Sine)
        );
    }

    #[test]
    fn selects_exact_preset_with_or_without_extension() {
        let without_extension =
            select_presets("sine_1000_-36").expect("preset target should be valid");
        let with_extension =
            select_presets("sine_1000_-36.wav").expect("preset target should be valid");

        assert_eq!(without_extension.len(), 1);
        assert_eq!(without_extension[0].file_name, "sine_1000_-36.wav");
        assert_eq!(with_extension[0].file_name, "sine_1000_-36.wav");
    }

    #[test]
    fn selects_noise_group() {
        let presets = select_presets("noise").expect("noise target should be valid");

        assert_eq!(presets.len(), 2);
        assert!(
            presets
                .iter()
                .all(|preset| preset.group == SignalGroup::Noise)
        );
    }
}
