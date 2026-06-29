use std::f32::consts::PI;

use crate::generator::presets::{FADE_MS, SAMPLE_RATE, SignalPreset, SignalSpec};

pub fn render_preset(preset: &SignalPreset) -> Vec<f32> {
    let sample_count = (preset.duration_secs * SAMPLE_RATE as f32).round() as usize;
    let amplitude = dbfs_to_amplitude(preset.peak_dbfs);
    let faded_samples = render_raw_samples(preset, sample_count)
        .into_iter()
        .enumerate()
        .map(|(index, sample)| sample * fade_gain(index, sample_count))
        .collect::<Vec<_>>();
    let peak = faded_samples
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
        .max(f32::EPSILON);

    faded_samples
        .into_iter()
        .map(|sample| sample / peak * amplitude)
        .collect()
}

fn render_raw_samples(preset: &SignalPreset, sample_count: usize) -> Vec<f32> {
    match preset.spec {
        SignalSpec::WhiteNoise => render_white_noise(sample_count),
        SignalSpec::PinkNoise => render_pink_noise(sample_count),
        SignalSpec::Sine { frequency_hz } => (0..sample_count)
            .map(|index| sine_sample(frequency_hz, index as f32 / SAMPLE_RATE as f32))
            .collect(),
        SignalSpec::LogSweep { start_hz, end_hz } => (0..sample_count)
            .map(|index| {
                log_sweep_sample(
                    start_hz,
                    end_hz,
                    preset.duration_secs,
                    index as f32 / SAMPLE_RATE as f32,
                )
            })
            .collect(),
        SignalSpec::TwoTone {
            first_hz,
            second_hz,
        } => (0..sample_count)
            .map(|index| {
                let time_secs = index as f32 / SAMPLE_RATE as f32;
                sine_sample(first_hz, time_secs) + sine_sample(second_hz, time_secs)
            })
            .collect(),
    }
}

fn sine_sample(frequency_hz: f32, time_secs: f32) -> f32 {
    (2.0 * PI * frequency_hz * time_secs).sin()
}

fn log_sweep_sample(start_hz: f32, end_hz: f32, duration_secs: f32, time_secs: f32) -> f32 {
    let ratio = end_hz / start_hz;
    let exponent = ratio.ln() / duration_secs;
    let phase = 2.0 * PI * start_hz * ((exponent * time_secs).exp() - 1.0) / exponent;

    phase.sin()
}

fn render_white_noise(sample_count: usize) -> Vec<f32> {
    let mut noise = Vec::with_capacity(sample_count);
    let mut state = 0x1234_abcd_u32;

    for _ in 0..sample_count {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let normalized = state as f32 / u32::MAX as f32;
        noise.push(normalized * 2.0 - 1.0);
    }

    noise
}

fn render_pink_noise(sample_count: usize) -> Vec<f32> {
    let white = render_white_noise(sample_count);
    let mut b0 = 0.0_f32;
    let mut b1 = 0.0_f32;
    let mut b2 = 0.0_f32;
    let mut b3 = 0.0_f32;
    let mut b4 = 0.0_f32;
    let mut b5 = 0.0_f32;
    let mut b6 = 0.0_f32;

    white
        .into_iter()
        .map(|sample| {
            b0 = 0.99886 * b0 + sample * 0.0555179;
            b1 = 0.99332 * b1 + sample * 0.0750759;
            b2 = 0.96900 * b2 + sample * 0.1538520;
            b3 = 0.86650 * b3 + sample * 0.3104856;
            b4 = 0.55000 * b4 + sample * 0.5329522;
            b5 = -0.7616 * b5 - sample * 0.0168980;

            let pink = b0 + b1 + b2 + b3 + b4 + b5 + b6 + sample * 0.5362;
            b6 = sample * 0.115926;

            pink
        })
        .collect()
}

fn dbfs_to_amplitude(dbfs: f32) -> f32 {
    10.0_f32.powf(dbfs / 20.0)
}

fn fade_gain(index: usize, sample_count: usize) -> f32 {
    let fade_samples = ((SAMPLE_RATE as f32 * FADE_MS) / 1000.0).round() as usize;
    if fade_samples == 0 {
        return 1.0;
    }

    let fade_in = (index + 1) as f32 / fade_samples as f32;
    let fade_out = (sample_count - index) as f32 / fade_samples as f32;

    fade_in.min(fade_out).min(1.0)
}

#[cfg(test)]
mod tests {
    use crate::generator::{
        presets::{PRESETS, SAMPLE_RATE, SignalSpec},
        signal::render_preset,
    };

    #[test]
    fn renders_expected_duration() {
        let samples = render_preset(&PRESETS[0]);

        assert_eq!(samples.len(), 3 * SAMPLE_RATE as usize);
    }

    #[test]
    fn renders_sweep_duration() {
        let preset = PRESETS
            .iter()
            .find(|preset| matches!(preset.spec, SignalSpec::LogSweep { .. }))
            .expect("minimal set should include sweep");

        let samples = render_preset(preset);

        assert_eq!(samples.len(), 10 * SAMPLE_RATE as usize);
    }

    #[test]
    fn generated_samples_do_not_clip() {
        for preset in PRESETS {
            let peak = render_preset(preset)
                .iter()
                .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));

            assert!(peak <= 1.0);
        }
    }

    #[test]
    fn generated_peak_matches_preset_dbfs() {
        for preset in PRESETS {
            let expected_peak = 10.0_f32.powf(preset.peak_dbfs / 20.0);
            let actual_peak = render_preset(preset)
                .iter()
                .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));

            assert!(
                (actual_peak - expected_peak).abs() < 0.0001,
                "{} peak {} did not match expected {}",
                preset.file_name,
                actual_peak,
                expected_peak
            );
        }
    }
}
