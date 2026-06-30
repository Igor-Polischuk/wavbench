use std::{
    error::Error,
    io::{self, ErrorKind},
    ops::Range,
};

use crate::{audio_buffer::AudioBuffer, metrics};

const ANALYSIS_TRIM_SECS: f32 = 0.01;
const SILENCE_TRIM_DB: f32 = -80.0;
const MAX_HARMONIC: usize = 8;
const FREQUENCY_DEDUPE_TOLERANCE_HZ: f32 = 0.5;
const MIN_FUNDAMENTAL_AMPLITUDE: f32 = 1e-9;

#[derive(Debug)]
pub struct TwotoneAnalysisReport {
    pub input: TwotoneInputStats,
    pub output: TwotoneOutputStats,
    pub fundamentals: Vec<ToneAnalysis>,
    pub harmonics: Vec<HarmonicGroup>,
    pub intermodulation: Vec<IntermodulationProduct>,
    pub imd: ImdAnalysis,
}

#[derive(Clone, Copy, Debug)]
pub struct TwotoneInputStats {
    pub first_frequency_hz: f32,
    pub second_frequency_hz: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct TwotoneOutputStats {
    pub peak_dbfs: f32,
    pub rms_dbfs: f32,
    pub rms_gain_db: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct ToneAnalysis {
    pub frequency_hz: f32,
    pub level_dbfs: f32,
    amplitude: f32,
}

#[derive(Debug)]
pub struct HarmonicGroup {
    pub fundamental_hz: f32,
    pub harmonics: Vec<HarmonicAnalysis>,
}

#[derive(Clone, Copy, Debug)]
pub struct HarmonicAnalysis {
    pub number: usize,
    pub frequency_hz: f32,
    pub level_dbfs: f32,
    pub relative_db: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct IntermodulationProduct {
    pub frequency_hz: f32,
    pub level_dbfs: f32,
    pub relative_db: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct ImdAnalysis {
    pub percent: f32,
    pub relative_db: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TwotoneParams {
    first_hz: f32,
    second_hz: f32,
}

pub fn analyze_twotone_pair(
    input_wav: &AudioBuffer,
    output_wav: &AudioBuffer,
) -> Result<TwotoneAnalysisReport, Box<dyn Error>> {
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

    let params = parse_twotone_params_from_name(&input_wav.id).ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "Could not determine two-tone frequencies from input filename '{}'. Expected twotone_<f1>_<f2>_<level>.wav",
                input_wav.id
            ),
        )
    })?;
    validate_twotone_params(params, input_wav.sample_rate)?;

    let input_steady = steady_section(&input, input_wav.sample_rate)?;
    let output_steady = steady_section(&output, output_wav.sample_rate)?;
    let input_rms = metrics::rms(input_steady);
    let output_rms = metrics::rms(output_steady);

    let fundamental_frequencies = [params.first_hz, params.second_hz];
    let harmonic_frequencies =
        harmonic_frequencies(&fundamental_frequencies, output_wav.sample_rate);
    let intermodulation_frequencies = intermodulation_frequencies(params, output_wav.sample_rate);
    let analysis_frequencies = unique_frequencies(
        fundamental_frequencies
            .iter()
            .copied()
            .chain(harmonic_frequencies.iter().copied())
            .chain(intermodulation_frequencies.iter().copied()),
    );
    let fits = tone_fits(output_steady, output_wav.sample_rate, &analysis_frequencies)?;
    let amplitude_at = |frequency_hz| amplitude_for_frequency(&fits, frequency_hz).unwrap_or(0.0);

    let fundamentals = fundamental_frequencies
        .iter()
        .map(|&frequency_hz| {
            let amplitude = amplitude_at(frequency_hz);
            ToneAnalysis {
                frequency_hz,
                level_dbfs: metrics::db(amplitude),
                amplitude,
            }
        })
        .collect::<Vec<_>>();

    if fundamentals
        .iter()
        .any(|fundamental| fundamental.amplitude < MIN_FUNDAMENTAL_AMPLITUDE)
    {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "Output fundamentals are too low to analyze intermodulation",
        )
        .into());
    }

    let carrier_amplitude = carrier_amplitude(&fundamentals);
    let harmonics = harmonic_groups(
        output_wav.sample_rate,
        &fundamentals,
        carrier_amplitude,
        &amplitude_at,
    );
    let intermodulation = intermodulation_products(
        &intermodulation_frequencies,
        carrier_amplitude,
        &amplitude_at,
    );
    let imd = imd_analysis(&intermodulation, carrier_amplitude);

    Ok(TwotoneAnalysisReport {
        input: TwotoneInputStats {
            first_frequency_hz: params.first_hz,
            second_frequency_hz: params.second_hz,
        },
        output: TwotoneOutputStats {
            peak_dbfs: metrics::db(peak(output_steady)),
            rms_dbfs: metrics::db(output_rms),
            rms_gain_db: metrics::db(output_rms / input_rms.max(1e-12)),
        },
        fundamentals,
        harmonics,
        intermodulation,
        imd,
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

    if peak(samples) <= 0.0 {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("{label} WAV is silent"),
        ));
    }

    Ok(())
}

fn validate_twotone_params(params: TwotoneParams, sample_rate: u32) -> io::Result<()> {
    if params.first_hz <= 0.0 || params.second_hz <= 0.0 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "Two-tone frequencies must be greater than 0 Hz",
        ));
    }

    if (params.first_hz - params.second_hz).abs() < FREQUENCY_DEDUPE_TOLERANCE_HZ {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "Two-tone frequencies must be different",
        ));
    }

    let nyquist = sample_rate as f32 / 2.0;
    if params.first_hz >= nyquist || params.second_hz >= nyquist {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "Two-tone frequencies must be below Nyquist {:.1}Hz",
                nyquist
            ),
        ));
    }

    Ok(())
}

fn parse_twotone_params_from_name(file_name: &str) -> Option<TwotoneParams> {
    let stem = file_name.strip_suffix(".wav").unwrap_or(file_name);
    let parts = stem.split('_').collect::<Vec<_>>();

    let (first_token, second_token) = match parts.as_slice() {
        ["twotone", first, second, ..] => (*first, *second),
        ["two", "tone", first, second, ..] => (*first, *second),
        _ => return None,
    };

    let mut first_hz = parse_frequency_token(first_token)?;
    let mut second_hz = parse_frequency_token(second_token)?;
    if second_hz < first_hz {
        std::mem::swap(&mut first_hz, &mut second_hz);
    }

    Some(TwotoneParams {
        first_hz,
        second_hz,
    })
}

fn parse_frequency_token(token: &str) -> Option<f32> {
    let normalized = token.to_ascii_lowercase();
    let without_hz = normalized.strip_suffix("hz").unwrap_or(&normalized);

    if let Some(khz) = without_hz.strip_suffix('k') {
        return khz.parse::<f32>().ok().map(|value| value * 1000.0);
    }

    without_hz.parse::<f32>().ok()
}

fn steady_section(samples: &[f32], sample_rate: u32) -> io::Result<&[f32]> {
    let active = active_range(samples);
    let trim_samples = (ANALYSIS_TRIM_SECS * sample_rate as f32).round() as usize;

    if active.len() <= trim_samples * 2 {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!(
                "WAV is too short for two-tone analysis after trimming {:.0}ms at both ends",
                ANALYSIS_TRIM_SECS * 1000.0
            ),
        ));
    }

    Ok(&samples[active.start + trim_samples..active.end - trim_samples])
}

fn active_range(samples: &[f32]) -> Range<usize> {
    let peak = peak(samples);
    if peak <= 0.0 {
        return 0..samples.len();
    }

    let threshold = (peak * 10.0_f32.powf(SILENCE_TRIM_DB / 20.0)).max(1e-8);
    let start = samples
        .iter()
        .position(|sample| sample.abs() >= threshold)
        .unwrap_or(0);
    let end = samples
        .iter()
        .rposition(|sample| sample.abs() >= threshold)
        .map(|index| index + 1)
        .unwrap_or(samples.len());

    start..end
}

fn harmonic_groups(
    sample_rate: u32,
    fundamentals: &[ToneAnalysis],
    carrier_amplitude: f32,
    amplitude_at: &impl Fn(f32) -> f32,
) -> Vec<HarmonicGroup> {
    let nyquist = sample_rate as f32 / 2.0;

    fundamentals
        .iter()
        .map(|fundamental| {
            let harmonics = (2..=MAX_HARMONIC)
                .filter_map(|number| {
                    let frequency_hz = fundamental.frequency_hz * number as f32;
                    if frequency_hz >= nyquist {
                        return None;
                    }

                    let amplitude = amplitude_at(frequency_hz);
                    Some(HarmonicAnalysis {
                        number,
                        frequency_hz,
                        level_dbfs: metrics::db(amplitude),
                        relative_db: metrics::db(amplitude / carrier_amplitude),
                    })
                })
                .collect();

            HarmonicGroup {
                fundamental_hz: fundamental.frequency_hz,
                harmonics,
            }
        })
        .collect()
}

fn harmonic_frequencies(fundamental_frequencies: &[f32], sample_rate: u32) -> Vec<f32> {
    let nyquist = sample_rate as f32 / 2.0;
    unique_frequencies(fundamental_frequencies.iter().flat_map(|&fundamental_hz| {
        (2..=MAX_HARMONIC).filter_map(move |number| {
            let frequency_hz = fundamental_hz * number as f32;
            (frequency_hz < nyquist).then_some(frequency_hz)
        })
    }))
}

fn intermodulation_products(
    frequencies: &[f32],
    carrier_amplitude: f32,
    amplitude_at: &impl Fn(f32) -> f32,
) -> Vec<IntermodulationProduct> {
    frequencies
        .iter()
        .map(|&frequency_hz| {
            let amplitude = amplitude_at(frequency_hz);
            IntermodulationProduct {
                frequency_hz,
                level_dbfs: metrics::db(amplitude),
                relative_db: metrics::db(amplitude / carrier_amplitude),
            }
        })
        .collect()
}

fn intermodulation_frequencies(params: TwotoneParams, sample_rate: u32) -> Vec<f32> {
    let f1 = params.first_hz;
    let f2 = params.second_hz;
    let candidates = [
        (f2 - f1).abs(),
        (2.0 * f1 - f2).abs(),
        (2.0 * f2 - f1).abs(),
        f1 + f2,
        2.0 * f1 + f2,
        2.0 * f2 + f1,
        3.0 * f1 + f2,
        f1 + 3.0 * f2,
    ];
    let nyquist = sample_rate as f32 / 2.0;
    let mut frequencies = Vec::new();

    for frequency_hz in candidates {
        if frequency_hz <= 0.0 || frequency_hz >= nyquist {
            continue;
        }

        if is_same_frequency(frequency_hz, f1) || is_same_frequency(frequency_hz, f2) {
            continue;
        }

        if frequencies
            .iter()
            .any(|&existing| is_same_frequency(existing, frequency_hz))
        {
            continue;
        }

        frequencies.push(frequency_hz);
        if frequencies.len() == 6 {
            break;
        }
    }

    frequencies.sort_by(|left, right| left.total_cmp(right));
    frequencies
}

fn imd_analysis(products: &[IntermodulationProduct], carrier_amplitude: f32) -> ImdAnalysis {
    let product_amplitude = products
        .iter()
        .map(|product| {
            let amplitude = 10.0_f32.powf(product.level_dbfs / 20.0);
            amplitude * amplitude
        })
        .sum::<f32>()
        .sqrt();
    let ratio = product_amplitude / carrier_amplitude.max(1e-12);

    ImdAnalysis {
        percent: ratio * 100.0,
        relative_db: metrics::db(ratio),
    }
}

fn carrier_amplitude(fundamentals: &[ToneAnalysis]) -> f32 {
    fundamentals
        .iter()
        .map(|fundamental| fundamental.amplitude * fundamental.amplitude)
        .sum::<f32>()
        .sqrt()
}

fn is_same_frequency(left: f32, right: f32) -> bool {
    (left - right).abs() <= FREQUENCY_DEDUPE_TOLERANCE_HZ
}

fn unique_frequencies(frequencies: impl IntoIterator<Item = f32>) -> Vec<f32> {
    let mut unique = Vec::new();

    for frequency_hz in frequencies {
        if frequency_hz <= 0.0 {
            continue;
        }

        if unique
            .iter()
            .any(|&existing| is_same_frequency(existing, frequency_hz))
        {
            continue;
        }

        unique.push(frequency_hz);
    }

    unique.sort_by(|left, right| left.total_cmp(right));
    unique
}

#[derive(Clone, Copy, Debug)]
struct ToneFit {
    frequency_hz: f32,
    amplitude: f32,
}

fn tone_fits(
    samples: &[f32],
    sample_rate: u32,
    frequencies: &[f32],
) -> Result<Vec<ToneFit>, Box<dyn Error>> {
    let basis_count = frequencies.len() * 2;
    if basis_count == 0 {
        return Ok(Vec::new());
    }

    let mut normal = vec![vec![0.0_f64; basis_count]; basis_count];
    let mut rhs = vec![0.0_f64; basis_count];
    let omegas = frequencies
        .iter()
        .map(|&frequency_hz| 2.0 * std::f64::consts::PI * frequency_hz as f64 / sample_rate as f64)
        .collect::<Vec<_>>();
    let mut basis = vec![0.0_f64; basis_count];

    for (sample_index, &sample) in samples.iter().enumerate() {
        let sample = sample as f64;
        for (frequency_index, &omega) in omegas.iter().enumerate() {
            let phase = omega * sample_index as f64;
            basis[frequency_index * 2] = phase.cos();
            basis[frequency_index * 2 + 1] = phase.sin();
        }

        for row in 0..basis_count {
            rhs[row] += basis[row] * sample;
            for col in 0..=row {
                normal[row][col] += basis[row] * basis[col];
            }
        }
    }

    for row in 0..basis_count {
        for col in 0..row {
            normal[col][row] = normal[row][col];
        }
    }

    let coefficients = solve_linear_system(normal, rhs).ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidData,
            "Could not solve two-tone frequency fit",
        )
    })?;

    Ok(frequencies
        .iter()
        .enumerate()
        .map(|(index, &frequency_hz)| {
            let cos_coeff = coefficients[index * 2];
            let sin_coeff = coefficients[index * 2 + 1];
            ToneFit {
                frequency_hz,
                amplitude: (cos_coeff * cos_coeff + sin_coeff * sin_coeff).sqrt() as f32,
            }
        })
        .collect())
}

fn solve_linear_system(mut matrix: Vec<Vec<f64>>, mut rhs: Vec<f64>) -> Option<Vec<f64>> {
    let size = rhs.len();
    for column in 0..size {
        let pivot_row = (column..size).max_by(|&left, &right| {
            matrix[left][column]
                .abs()
                .total_cmp(&matrix[right][column].abs())
        })?;
        let pivot = matrix[pivot_row][column];
        if pivot.abs() < 1e-18 {
            return None;
        }

        if pivot_row != column {
            matrix.swap(column, pivot_row);
            rhs.swap(column, pivot_row);
        }

        for row in column + 1..size {
            let factor = matrix[row][column] / matrix[column][column];
            matrix[row][column] = 0.0;

            for col in column + 1..size {
                matrix[row][col] -= factor * matrix[column][col];
            }
            rhs[row] -= factor * rhs[column];
        }
    }

    let mut solution = vec![0.0_f64; size];
    for row in (0..size).rev() {
        let tail_sum = ((row + 1)..size)
            .map(|col| matrix[row][col] * solution[col])
            .sum::<f64>();
        let pivot = matrix[row][row];
        if pivot.abs() < 1e-18 {
            return None;
        }
        solution[row] = (rhs[row] - tail_sum) / pivot;
    }

    Some(solution)
}

fn amplitude_for_frequency(fits: &[ToneFit], frequency_hz: f32) -> Option<f32> {
    fits.iter()
        .find(|fit| is_same_frequency(fit.frequency_hz, frequency_hz))
        .map(|fit| fit.amplitude)
}

fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
}

#[cfg(test)]
mod tests {
    use crate::audio_buffer::AudioBuffer;

    use super::{
        TwotoneParams, analyze_twotone_pair, intermodulation_frequencies,
        parse_twotone_params_from_name,
    };

    const SAMPLE_RATE: u32 = 44_100;

    #[test]
    fn parses_twotone_frequencies_from_name() {
        assert_eq!(
            parse_twotone_params_from_name("twotone_440_660_-24.wav"),
            Some(TwotoneParams {
                first_hz: 440.0,
                second_hz: 660.0,
            })
        );
        assert_eq!(
            parse_twotone_params_from_name("two_tone_700_1900.wav"),
            Some(TwotoneParams {
                first_hz: 700.0,
                second_hz: 1900.0,
            })
        );
        assert_eq!(parse_twotone_params_from_name("sine_1000_-24.wav"), None);
    }

    #[test]
    fn reports_prioritized_intermodulation_frequencies() {
        let frequencies = intermodulation_frequencies(
            TwotoneParams {
                first_hz: 440.0,
                second_hz: 660.0,
            },
            SAMPLE_RATE,
        );

        assert_eq!(
            frequencies,
            vec![220.0, 880.0, 1100.0, 1540.0, 1760.0, 1980.0]
        );
    }

    #[test]
    fn reports_fundamentals_and_intermodulation() {
        let first = sine(440.0, -18.0, 3.0);
        let second = sine(660.0, -18.0, 3.0);
        let im = sine(220.0, -58.0, 3.0);
        let output = mix(&[&first, &second, &im]);
        let input = mix(&[&first, &second]);

        let report = analyze_twotone_pair(
            &buffer("twotone_440_660_-18.wav", input),
            &buffer("twotone_440_660_amp.wav", output),
        )
        .expect("two-tone analysis should succeed");

        assert!((fundamental_level(&report, 440.0) + 18.0).abs() < 0.1);
        assert!((fundamental_level(&report, 660.0) + 18.0).abs() < 0.1);

        let product = intermodulation_level(&report, 220.0);
        assert!((product.level_dbfs + 58.0).abs() < 0.2);
        assert!((product.relative_db + 43.0).abs() < 0.3);
        assert!((report.imd.relative_db + 43.0).abs() < 0.3);
    }

    #[test]
    fn reports_harmonics_for_each_fundamental() {
        let first = sine(440.0, -18.0, 3.0);
        let second = sine(660.0, -18.0, 3.0);
        let first_h2 = sine(880.0, -54.0, 3.0);
        let second_h2 = sine(1320.0, -60.0, 3.0);
        let output = mix(&[&first, &second, &first_h2, &second_h2]);
        let input = mix(&[&first, &second]);

        let report = analyze_twotone_pair(
            &buffer("twotone_440_660_-18.wav", input),
            &buffer("twotone_440_660_amp.wav", output),
        )
        .expect("two-tone analysis should succeed");

        let h2_440 = harmonic_level(&report, 440.0, 2);
        let h2_660 = harmonic_level(&report, 660.0, 2);

        assert!((h2_440.level_dbfs + 54.0).abs() < 0.2);
        assert!((h2_660.level_dbfs + 60.0).abs() < 0.2);
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

    fn mix(signals: &[&[f32]]) -> Vec<f32> {
        let sample_count = signals[0].len();
        (0..sample_count)
            .map(|index| signals.iter().map(|signal| signal[index]).sum())
            .collect()
    }

    fn fundamental_level(report: &super::TwotoneAnalysisReport, frequency_hz: f32) -> f32 {
        report
            .fundamentals
            .iter()
            .find(|fundamental| (fundamental.frequency_hz - frequency_hz).abs() < f32::EPSILON)
            .expect("fundamental should exist")
            .level_dbfs
    }

    fn intermodulation_level(
        report: &super::TwotoneAnalysisReport,
        frequency_hz: f32,
    ) -> super::IntermodulationProduct {
        report
            .intermodulation
            .iter()
            .find(|product| (product.frequency_hz - frequency_hz).abs() < f32::EPSILON)
            .copied()
            .expect("intermodulation product should exist")
    }

    fn harmonic_level(
        report: &super::TwotoneAnalysisReport,
        fundamental_hz: f32,
        number: usize,
    ) -> super::HarmonicAnalysis {
        report
            .harmonics
            .iter()
            .find(|group| (group.fundamental_hz - fundamental_hz).abs() < f32::EPSILON)
            .expect("harmonic group should exist")
            .harmonics
            .iter()
            .find(|harmonic| harmonic.number == number)
            .copied()
            .expect("harmonic should exist")
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
