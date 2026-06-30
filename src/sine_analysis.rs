use std::{
    error::Error,
    io::{self, ErrorKind},
};

use crate::{audio_buffer::AudioBuffer, metrics};

const MAX_HARMONIC: usize = 8;
const ANALYSIS_TRIM_SECS: f32 = 0.01;
const MIN_FUNDAMENTAL_AMPLITUDE: f32 = 1e-9;
const MIN_HARMONIC_POWER_FOR_PERCENT: f32 = 1e-10;
const MIN_HARMONIC_RATIO_FOR_SEARCH: f32 = 3.162_277_6e-5; // -90 dBc

#[derive(Debug)]
pub struct SineAnalysisReport {
    pub input: SignalStats,
    pub output: SignalStats,
    pub peak_gain_db: f32,
    pub rms_gain_db: f32,
    pub fundamental: FundamentalAnalysis,
    pub dc_offset: f32,
    pub noise_floor_dbfs: f32,
    pub harmonics: Vec<HarmonicAnalysis>,
    pub distortion: DistortionAnalysis,
    pub relative_harmonics: RelativeHarmonics,
}

#[derive(Clone, Copy, Debug)]
pub struct SignalStats {
    pub frequency_hz: Option<f32>,
    pub duration_secs: f32,
    pub peak_dbfs: f32,
    pub rms_dbfs: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct FundamentalAnalysis {
    pub frequency_hz: f32,
    pub level_dbfs: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct HarmonicAnalysis {
    pub number: usize,
    pub frequency_hz: f32,
    pub level_dbfs: f32,
    pub relative_db: f32,
    pub amplitude_ratio: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct DistortionAnalysis {
    pub thd_percent: f32,
    pub thdn_percent: f32,
    pub sinad_db: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct RelativeHarmonics {
    pub h2_percent: f32,
    pub h3_percent: f32,
    pub h4_percent: f32,
    pub h5_percent: f32,
    pub higher_percent: f32,
}

pub fn analyze_sine_pair(
    input_wav: &AudioBuffer,
    output_wav: &AudioBuffer,
) -> Result<SineAnalysisReport, Box<dyn Error>> {
    if input_wav.sample_rate != output_wav.sample_rate {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "Input and output sample rates differ: {}Hz vs {}Hz",
                input_wav.sample_rate, output_wav.sample_rate
            ),
        )
        .into());
    }

    let input = input_wav.to_mono_left();
    let output = output_wav.to_mono_left();
    validate_samples("input", &input)?;
    validate_samples("output", &output)?;

    let sample_rate = input_wav.sample_rate;
    let parsed_frequency_hz = parse_sine_frequency_from_name(&input_wav.id);
    let input_frequency_hz =
        parsed_frequency_hz.or_else(|| estimate_frequency_from_zero_crossings(&input, sample_rate));

    let nominal_frequency_hz = input_frequency_hz.ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidInput,
            "Could not determine sine frequency from input filename or samples",
        )
    })?;

    let input_stats = signal_stats(&input, sample_rate, Some(nominal_frequency_hz));
    let output_stats = signal_stats(&output, sample_rate, None);
    let output_steady = steady_section(&output, sample_rate)?;
    let output_dc = mean(output_steady);
    let output_without_dc = output_steady
        .iter()
        .map(|sample| sample - output_dc)
        .collect::<Vec<_>>();

    let fundamental_frequency_hz = if parsed_frequency_hz.is_some() {
        nominal_frequency_hz
    } else {
        estimate_frequency_around(&output_without_dc, sample_rate, nominal_frequency_hz)
    };
    let fundamental_fit = tone_fit(&output_without_dc, sample_rate, fundamental_frequency_hz);
    if fundamental_fit.amplitude < MIN_FUNDAMENTAL_AMPLITUDE {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "Output fundamental is too low to analyze harmonics",
        )
        .into());
    }

    let harmonics = harmonic_analyses(&output_without_dc, sample_rate, &fundamental_fit);
    let distortion = distortion_analysis(
        &output_without_dc,
        sample_rate,
        &fundamental_fit,
        &harmonics,
    );
    let noise_floor_dbfs = residual_noise_floor_dbfs(
        &output_without_dc,
        sample_rate,
        &fundamental_fit,
        &harmonics,
    );
    let relative_harmonics = relative_harmonics(&harmonics);
    let peak_gain_db = output_stats.peak_dbfs - input_stats.peak_dbfs;
    let rms_gain_db = output_stats.rms_dbfs - input_stats.rms_dbfs;

    Ok(SineAnalysisReport {
        input: input_stats,
        output: output_stats,
        peak_gain_db,
        rms_gain_db,
        fundamental: FundamentalAnalysis {
            frequency_hz: fundamental_fit.frequency_hz,
            level_dbfs: metrics::db(fundamental_fit.amplitude),
        },
        dc_offset: output_dc,
        noise_floor_dbfs,
        harmonics,
        distortion,
        relative_harmonics,
    })
}

fn validate_samples(label: &str, samples: &[f32]) -> io::Result<()> {
    if samples.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("{label} WAV does not contain samples"),
        ));
    }

    if samples.iter().any(|sample| !sample.is_finite()) {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("{label} WAV contains non-finite samples"),
        ));
    }

    Ok(())
}

fn signal_stats(samples: &[f32], sample_rate: u32, frequency_hz: Option<f32>) -> SignalStats {
    SignalStats {
        frequency_hz,
        duration_secs: samples.len() as f32 / sample_rate as f32,
        peak_dbfs: metrics::db(peak(samples)),
        rms_dbfs: metrics::db(metrics::rms(samples)),
    }
}

fn steady_section(samples: &[f32], sample_rate: u32) -> io::Result<&[f32]> {
    let trim_samples = (ANALYSIS_TRIM_SECS * sample_rate as f32).round() as usize;
    if samples.len() <= trim_samples * 2 {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!(
                "WAV is too short for sine analysis after trimming {:.0}ms at both ends",
                ANALYSIS_TRIM_SECS * 1000.0
            ),
        ));
    }

    Ok(&samples[trim_samples..samples.len() - trim_samples])
}

fn parse_sine_frequency_from_name(file_name: &str) -> Option<f32> {
    let stem = file_name.strip_suffix(".wav").unwrap_or(file_name);
    let mut parts = stem.split('_');
    if parts.next()? != "sine" {
        return None;
    }

    parse_frequency_token(parts.next()?)
}

fn parse_frequency_token(token: &str) -> Option<f32> {
    if let Some(khz) = token.strip_suffix('k') {
        return khz.parse::<f32>().ok().map(|value| value * 1000.0);
    }

    token.parse::<f32>().ok()
}

fn estimate_frequency_from_zero_crossings(samples: &[f32], sample_rate: u32) -> Option<f32> {
    let mut crossings = Vec::new();

    for (index, pair) in samples.windows(2).enumerate() {
        let previous = pair[0];
        let current = pair[1];
        if previous <= 0.0 && current > 0.0 {
            let fraction = -previous / (current - previous);
            crossings.push(index as f32 + fraction);
        }
    }

    if crossings.len() < 2 {
        return None;
    }

    let total_period_samples = crossings.last()? - crossings.first()?;
    let period_count = (crossings.len() - 1) as f32;
    if total_period_samples <= 0.0 {
        return None;
    }

    Some(sample_rate as f32 * period_count / total_period_samples)
}

fn estimate_frequency_around(samples: &[f32], sample_rate: u32, center_hz: f32) -> f32 {
    let span_hz = (center_hz * 0.005).max(2.0);
    let step_hz = (span_hz / 100.0).max(0.05);
    let max_frequency_hz = sample_rate as f32 / 2.0 - 1.0;
    let steps_each_side = (span_hz / step_hz).ceil() as i32;

    let mut frequencies = Vec::new();
    let mut magnitudes = Vec::new();
    for offset in -steps_each_side..=steps_each_side {
        let frequency_hz = (center_hz + offset as f32 * step_hz).clamp(1.0, max_frequency_hz);
        let fit = tone_fit(samples, sample_rate, frequency_hz);
        frequencies.push(frequency_hz);
        magnitudes.push(fit.amplitude);
    }

    let peak_index = magnitudes
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| left.total_cmp(right))
        .map(|(index, _)| index)
        .unwrap_or(0);
    let center_index = steps_each_side as usize;

    if peak_index == center_index {
        return center_hz;
    }

    if peak_index == 0 || peak_index + 1 >= magnitudes.len() {
        return frequencies[peak_index];
    }

    let left = magnitudes[peak_index - 1];
    let center = magnitudes[peak_index];
    let right = magnitudes[peak_index + 1];
    let denominator = left - 2.0 * center + right;
    if denominator.abs() < f32::EPSILON {
        return frequencies[peak_index];
    }

    let offset_steps = 0.5 * (left - right) / denominator;
    frequencies[peak_index] + offset_steps.clamp(-1.0, 1.0) * step_hz
}

fn harmonic_analyses(
    samples: &[f32],
    sample_rate: u32,
    fundamental_fit: &ToneFit,
) -> Vec<HarmonicAnalysis> {
    (2..=MAX_HARMONIC)
        .filter_map(|number| {
            let expected_frequency_hz = fundamental_fit.frequency_hz * number as f32;
            if expected_frequency_hz >= sample_rate as f32 / 2.0 {
                return None;
            }

            let expected_fit = tone_fit(samples, sample_rate, expected_frequency_hz);
            let expected_ratio = expected_fit.amplitude / fundamental_fit.amplitude;
            let fit = if expected_ratio >= MIN_HARMONIC_RATIO_FOR_SEARCH {
                let frequency_hz =
                    estimate_frequency_around(samples, sample_rate, expected_frequency_hz);
                tone_fit(samples, sample_rate, frequency_hz)
            } else {
                expected_fit
            };
            let amplitude_ratio = fit.amplitude / fundamental_fit.amplitude;

            Some(HarmonicAnalysis {
                number,
                frequency_hz: fit.frequency_hz,
                level_dbfs: metrics::db(fit.amplitude),
                relative_db: metrics::db(amplitude_ratio),
                amplitude_ratio,
            })
        })
        .collect()
}

fn distortion_analysis(
    samples: &[f32],
    sample_rate: u32,
    fundamental_fit: &ToneFit,
    harmonics: &[HarmonicAnalysis],
) -> DistortionAnalysis {
    let fundamental_rms = fundamental_fit.amplitude / 2.0_f32.sqrt();
    let harmonic_rms = harmonics
        .iter()
        .map(|harmonic| {
            let harmonic_rms =
                harmonic.amplitude_ratio * fundamental_fit.amplitude / 2.0_f32.sqrt();
            harmonic_rms * harmonic_rms
        })
        .sum::<f32>()
        .sqrt();

    let residual_without_fundamental =
        residual_without_components(samples, sample_rate, &[*fundamental_fit]);
    let thdn_rms = metrics::rms(&residual_without_fundamental);

    DistortionAnalysis {
        thd_percent: harmonic_rms / fundamental_rms * 100.0,
        thdn_percent: thdn_rms / fundamental_rms * 100.0,
        sinad_db: metrics::db(fundamental_rms / thdn_rms.max(1e-12)),
    }
}

fn residual_noise_floor_dbfs(
    samples: &[f32],
    sample_rate: u32,
    fundamental_fit: &ToneFit,
    harmonics: &[HarmonicAnalysis],
) -> f32 {
    let mut fits = Vec::with_capacity(harmonics.len() + 1);
    fits.push(*fundamental_fit);
    for harmonic in harmonics {
        fits.push(tone_fit(samples, sample_rate, harmonic.frequency_hz));
    }

    let residual = residual_without_components(samples, sample_rate, &fits);
    metrics::db(metrics::rms(&residual))
}

fn relative_harmonics(harmonics: &[HarmonicAnalysis]) -> RelativeHarmonics {
    let total_power = harmonics
        .iter()
        .map(|harmonic| harmonic.amplitude_ratio * harmonic.amplitude_ratio)
        .sum::<f32>();

    let percent = |number: usize| -> f32 {
        if total_power < MIN_HARMONIC_POWER_FOR_PERCENT {
            return 0.0;
        }

        harmonics
            .iter()
            .find(|harmonic| harmonic.number == number)
            .map(|harmonic| {
                harmonic.amplitude_ratio * harmonic.amplitude_ratio / total_power * 100.0
            })
            .unwrap_or(0.0)
    };

    let higher_percent = if total_power < MIN_HARMONIC_POWER_FOR_PERCENT {
        0.0
    } else {
        harmonics
            .iter()
            .filter(|harmonic| harmonic.number >= 6)
            .map(|harmonic| harmonic.amplitude_ratio * harmonic.amplitude_ratio)
            .sum::<f32>()
            / total_power
            * 100.0
    };

    RelativeHarmonics {
        h2_percent: percent(2),
        h3_percent: percent(3),
        h4_percent: percent(4),
        h5_percent: percent(5),
        higher_percent,
    }
}

#[derive(Clone, Copy, Debug)]
struct ToneFit {
    frequency_hz: f32,
    amplitude: f32,
    cos_coeff: f64,
    sin_coeff: f64,
}

fn tone_fit(samples: &[f32], sample_rate: u32, frequency_hz: f32) -> ToneFit {
    let projection = tone_projection(samples, sample_rate, frequency_hz);
    let determinant =
        projection.cos_cos_sum * projection.sin_sin_sum - projection.cos_sin_sum.powi(2);

    let (cos_coeff, sin_coeff) = if determinant.abs() < f64::EPSILON {
        (0.0, 0.0)
    } else {
        (
            (projection.sample_cos_sum * projection.sin_sin_sum
                - projection.sample_sin_sum * projection.cos_sin_sum)
                / determinant,
            (projection.sample_sin_sum * projection.cos_cos_sum
                - projection.sample_cos_sum * projection.cos_sin_sum)
                / determinant,
        )
    };

    ToneFit {
        frequency_hz,
        amplitude: (cos_coeff * cos_coeff + sin_coeff * sin_coeff).sqrt() as f32,
        cos_coeff,
        sin_coeff,
    }
}

struct ToneProjection {
    sample_cos_sum: f64,
    sample_sin_sum: f64,
    cos_cos_sum: f64,
    sin_sin_sum: f64,
    cos_sin_sum: f64,
}

fn tone_projection(samples: &[f32], sample_rate: u32, frequency_hz: f32) -> ToneProjection {
    let omega = 2.0 * std::f64::consts::PI * frequency_hz as f64 / sample_rate as f64;
    let mut sample_cos_sum = 0.0_f64;
    let mut sample_sin_sum = 0.0_f64;
    let mut cos_cos_sum = 0.0_f64;
    let mut sin_sin_sum = 0.0_f64;
    let mut cos_sin_sum = 0.0_f64;

    for (index, &sample) in samples.iter().enumerate() {
        let phase = omega * index as f64;
        let cos_value = phase.cos();
        let sin_value = phase.sin();

        sample_cos_sum += sample as f64 * cos_value;
        sample_sin_sum += sample as f64 * sin_value;
        cos_cos_sum += cos_value * cos_value;
        sin_sin_sum += sin_value * sin_value;
        cos_sin_sum += cos_value * sin_value;
    }

    ToneProjection {
        sample_cos_sum,
        sample_sin_sum,
        cos_cos_sum,
        sin_sin_sum,
        cos_sin_sum,
    }
}

fn residual_without_components(
    samples: &[f32],
    sample_rate: u32,
    components: &[ToneFit],
) -> Vec<f32> {
    let mut residual = samples.to_vec();

    for component in components {
        subtract_tone(&mut residual, sample_rate, component);
    }

    residual
}

fn subtract_tone(samples: &mut [f32], sample_rate: u32, fit: &ToneFit) {
    let omega = 2.0 * std::f64::consts::PI * fit.frequency_hz as f64 / sample_rate as f64;

    for (index, sample) in samples.iter_mut().enumerate() {
        let phase = omega * index as f64;
        let cos_value = phase.cos();
        let sin_value = phase.sin();

        *sample -= (fit.cos_coeff * cos_value + fit.sin_coeff * sin_value) as f32;
    }
}

fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
}

fn mean(samples: &[f32]) -> f32 {
    samples.iter().sum::<f32>() / samples.len() as f32
}

#[cfg(test)]
mod tests {
    use crate::audio_buffer::AudioBuffer;

    use super::{analyze_sine_pair, parse_sine_frequency_from_name};

    const SAMPLE_RATE: u32 = 44_100;

    #[test]
    fn parses_frequency_from_generated_sine_name() {
        assert_eq!(
            parse_sine_frequency_from_name("sine_1000_-24.wav"),
            Some(1000.0)
        );
        assert_eq!(
            parse_sine_frequency_from_name("sine_5k_-24.wav"),
            Some(5000.0)
        );
        assert_eq!(parse_sine_frequency_from_name("white_noise.wav"), None);
    }

    #[test]
    fn reports_harmonics_relative_to_fundamental() {
        let fundamental = sine(1000.0, -24.0, 3.0);
        let second_harmonic = sine(2000.0, -64.0, 3.0);
        let output = fundamental
            .iter()
            .zip(second_harmonic.iter())
            .map(|(fundamental, harmonic)| fundamental + harmonic)
            .collect::<Vec<_>>();

        let report = analyze_sine_pair(
            &buffer("sine_1000_-24.wav", fundamental),
            &buffer("sine_1000_-24_amp.wav", output),
        )
        .expect("analysis should succeed");

        let h2 = report
            .harmonics
            .iter()
            .find(|harmonic| harmonic.number == 2)
            .expect("H2 should be reported");

        assert!((h2.relative_db + 40.0).abs() < 0.5);
        assert!((h2.level_dbfs + 64.0).abs() < 0.5);
        assert!((report.distortion.thd_percent - 1.0).abs() < 0.1);
    }

    #[test]
    fn reports_fundamental_level_in_dbfs() {
        let input = sine(1000.0, -24.0, 3.0);
        let report = analyze_sine_pair(
            &buffer("sine_1000_-24.wav", input.clone()),
            &buffer("sine_1000_-24_amp.wav", input),
        )
        .expect("analysis should succeed");

        assert!((report.fundamental.frequency_hz - 1000.0).abs() < 0.1);
        assert!((report.fundamental.level_dbfs + 24.0).abs() < 0.1);
    }

    #[test]
    fn suppresses_relative_harmonic_percentages_below_noise_threshold() {
        let input = sine(1000.0, -24.0, 3.0);
        let report = analyze_sine_pair(
            &buffer("sine_1000_-24.wav", input.clone()),
            &buffer("sine_1000_-24_amp.wav", input),
        )
        .expect("analysis should succeed");

        assert_eq!(report.relative_harmonics.h2_percent, 0.0);
        assert_eq!(report.relative_harmonics.h3_percent, 0.0);
        assert_eq!(report.relative_harmonics.h4_percent, 0.0);
        assert_eq!(report.relative_harmonics.h5_percent, 0.0);
        assert_eq!(report.relative_harmonics.higher_percent, 0.0);
    }

    #[test]
    fn reports_clean_passthrough_as_low_distortion() {
        let input = sine(1000.0, -18.0, 3.0);
        let report = analyze_sine_pair(
            &buffer("sine_1000_-18.wav", input.clone()),
            &buffer("sine_1000_-18_amp.wav", input),
        )
        .expect("analysis should succeed");

        assert!(
            report
                .harmonics
                .iter()
                .all(|harmonic| harmonic.relative_db < -100.0)
        );
        assert!(report.distortion.thd_percent < 0.001);
        assert!(
            report.distortion.sinad_db > 90.0,
            "SINAD was {:.2} dB",
            report.distortion.sinad_db
        );
    }

    #[test]
    fn reports_symmetric_saturation_as_odd_harmonic_dominant() {
        let input = sine(1000.0, -6.0, 3.0);
        let output = input
            .iter()
            .map(|sample| (sample * 3.0).tanh())
            .collect::<Vec<_>>();

        let report = analyze_sine_pair(
            &buffer("sine_1000_-6.wav", input),
            &buffer("sine_1000_-6_amp.wav", output),
        )
        .expect("analysis should succeed");

        let harmonic = |number| {
            report
                .harmonics
                .iter()
                .find(|harmonic| harmonic.number == number)
                .expect("harmonic should be reported")
        };

        assert!(harmonic(3).relative_db > harmonic(5).relative_db);
        assert!(harmonic(5).relative_db > harmonic(7).relative_db);
        assert!(harmonic(2).relative_db < -80.0);
        assert!(harmonic(4).relative_db < -80.0);
    }

    fn sine(frequency_hz: f32, peak_dbfs: f32, duration_secs: f32) -> Vec<f32> {
        let sample_count = (duration_secs * SAMPLE_RATE as f32).round() as usize;
        let amplitude = 10.0_f32.powf(peak_dbfs / 20.0);

        (0..sample_count)
            .map(|index| {
                let phase = 2.0_f64 * std::f64::consts::PI * frequency_hz as f64 * index as f64
                    / SAMPLE_RATE as f64;
                amplitude * phase.sin() as f32
            })
            .collect()
    }

    fn buffer(id: &str, samples: Vec<f32>) -> AudioBuffer {
        AudioBuffer {
            frames: samples.len() as u32,
            samples,
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 32,
            id: id.to_string(),
        }
    }
}
