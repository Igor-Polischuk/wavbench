use std::{
    error::Error,
    io::{self, ErrorKind},
    ops::Range,
};

use realfft::RealFftPlanner;

use crate::{audio_buffer::AudioBuffer, metrics};

const SILENCE_TRIM_DB: f32 = -80.0;
const EDGE_TRIM_SECS: f32 = 0.01;
const IMPULSE_PRE_SECS: f32 = 0.005;
const IMPULSE_POST_SECS: f32 = 1.0;
const MAX_DELAY_SECS: f32 = 2.0;
const MIN_IMPULSE_PEAK: f32 = 1e-12;
const MIN_SPECTRUM_POWER: f32 = 1e-20;

const RESPONSE_FREQUENCIES_HZ: [f32; 27] = [
    20.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0, 400.0, 500.0,
    630.0, 800.0, 1000.0, 1250.0, 1600.0, 2000.0, 2500.0, 3150.0, 4000.0, 5000.0, 6300.0, 8000.0,
    10_000.0,
];

#[derive(Debug)]
pub struct SweepAnalysisReport {
    pub input: SweepInputStats,
    pub output: SweepOutputStats,
    pub delay_samples: isize,
    pub frequency_response: Vec<FrequencyResponsePoint>,
}

#[derive(Clone, Copy, Debug)]
pub struct SweepInputStats {
    pub start_frequency_hz: f32,
    pub end_frequency_hz: f32,
    pub duration_secs: f32,
    pub rms_dbfs: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct SweepOutputStats {
    pub peak_dbfs: f32,
    pub rms_dbfs: f32,
    pub rms_gain_db: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct FrequencyResponsePoint {
    pub frequency_hz: f32,
    pub magnitude_db: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct SweepParams {
    start_hz: f32,
    end_hz: f32,
}

pub fn analyze_sweep_pair(
    input_wav: &AudioBuffer,
    output_wav: &AudioBuffer,
) -> Result<SweepAnalysisReport, Box<dyn Error>> {
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

    let params = parse_sweep_params_from_name(&input_wav.id).ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "Could not determine sweep range from input filename '{}'. Expected sweep_<start>_<end>_<level>.wav",
                input_wav.id
            ),
        )
    })?;
    validate_sweep_params(params, input_wav.sample_rate)?;

    let input_range = analysis_range(&input, input_wav.sample_rate);
    let inverse = inverse_log_sweep(params, input_wav.sample_rate, input.len());
    let input_impulse = fft_convolve(&input, &inverse)?;
    let output_impulse = fft_convolve(&output, &inverse)?;

    let input_peak = peak_index(&input_impulse).ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidData,
            "Could not locate input sweep impulse peak",
        )
    })?;
    let output_peak = peak_index_near(
        &output_impulse,
        input_peak.index,
        max_delay_samples(input_wav.sample_rate),
    )
    .ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidData,
            "Could not locate output sweep impulse peak near the input peak",
        )
    })?;

    if input_peak.value < MIN_IMPULSE_PEAK || output_peak.value < MIN_IMPULSE_PEAK {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "Sweep deconvolution peak is too low to analyze",
        )
        .into());
    }

    let delay_samples = output_peak.index as isize - input_peak.index as isize;
    let (input_stats_samples, output_stats_samples) =
        aligned_stat_slices(&input, &output, delay_samples, input_range)?;
    let input_rms = metrics::rms(input_stats_samples);
    let output_rms = metrics::rms(output_stats_samples);

    let input_window = impulse_window(
        &input_impulse,
        input_peak.index,
        input_wav.sample_rate,
        input.len(),
    );
    let output_window = impulse_window(
        &output_impulse,
        output_peak.index,
        input_wav.sample_rate,
        input.len(),
    );
    let frequency_response =
        frequency_response(&input_window, &output_window, input_wav.sample_rate, params)?;

    Ok(SweepAnalysisReport {
        input: SweepInputStats {
            start_frequency_hz: params.start_hz,
            end_frequency_hz: params.end_hz,
            duration_secs: input.len() as f32 / input_wav.sample_rate as f32,
            rms_dbfs: metrics::db(input_rms),
        },
        output: SweepOutputStats {
            peak_dbfs: metrics::db(peak(output_stats_samples)),
            rms_dbfs: metrics::db(output_rms),
            rms_gain_db: metrics::db(output_rms / input_rms.max(1e-12)),
        },
        delay_samples,
        frequency_response,
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

fn validate_sweep_params(params: SweepParams, sample_rate: u32) -> io::Result<()> {
    if params.start_hz <= 0.0 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "Sweep start frequency must be greater than 0 Hz",
        ));
    }

    if params.end_hz <= params.start_hz {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "Sweep end frequency must be greater than start frequency",
        ));
    }

    let nyquist = sample_rate as f32 / 2.0;
    if params.end_hz >= nyquist {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "Sweep end frequency {:.1}Hz must be below Nyquist {:.1}Hz",
                params.end_hz, nyquist
            ),
        ));
    }

    Ok(())
}

fn parse_sweep_params_from_name(file_name: &str) -> Option<SweepParams> {
    let stem = file_name.strip_suffix(".wav").unwrap_or(file_name);
    let mut parts = stem.split('_');
    if parts.next()? != "sweep" {
        return None;
    }

    Some(SweepParams {
        start_hz: parse_frequency_token(parts.next()?)?,
        end_hz: parse_frequency_token(parts.next()?)?,
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

fn analysis_range(samples: &[f32], sample_rate: u32) -> Range<usize> {
    let active = active_range(samples);
    let edge_trim = (EDGE_TRIM_SECS * sample_rate as f32).round() as usize;

    if active.len() > edge_trim * 2 {
        active.start + edge_trim..active.end - edge_trim
    } else {
        active
    }
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

fn aligned_stat_slices<'a>(
    input: &'a [f32],
    output: &'a [f32],
    delay_samples: isize,
    input_range: Range<usize>,
) -> Result<(&'a [f32], &'a [f32]), Box<dyn Error>> {
    let mut input_start = input_range.start as isize;
    let mut input_end = input_range.end as isize;

    let output_start = input_start + delay_samples;
    if output_start < 0 {
        input_start -= output_start;
    }

    let output_end = input_end + delay_samples;
    if output_end > output.len() as isize {
        input_end -= output_end - output.len() as isize;
    }

    input_start = input_start.max(0);
    input_end = input_end.min(input.len() as isize);

    if input_end <= input_start {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "Input and output do not overlap after delay compensation",
        )
        .into());
    }

    let output_start = input_start + delay_samples;
    let output_end = input_end + delay_samples;
    if output_start < 0 || output_end > output.len() as isize || output_end <= output_start {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "Output range is invalid after delay compensation",
        )
        .into());
    }

    Ok((
        &input[input_start as usize..input_end as usize],
        &output[output_start as usize..output_end as usize],
    ))
}

fn inverse_log_sweep(params: SweepParams, sample_rate: u32, sample_count: usize) -> Vec<f32> {
    let duration_secs = sample_count as f64 / sample_rate as f64;
    let sweep = render_log_sweep(params, sample_rate, sample_count);
    let log_ratio = (params.end_hz as f64 / params.start_hz as f64).ln();

    (0..sample_count)
        .map(|index| {
            let time_secs = index as f64 / sample_rate as f64;
            let envelope = (-time_secs * log_ratio / duration_secs).exp() as f32;
            sweep[sample_count - 1 - index] * envelope
        })
        .collect()
}

fn render_log_sweep(params: SweepParams, sample_rate: u32, sample_count: usize) -> Vec<f32> {
    let start_hz = params.start_hz as f64;
    let end_hz = params.end_hz as f64;
    let duration_secs = sample_count as f64 / sample_rate as f64;
    let ratio = end_hz / start_hz;
    let exponent = ratio.ln() / duration_secs;

    (0..sample_count)
        .map(|sample_index| {
            let time_secs = sample_index as f64 / sample_rate as f64;
            let phase =
                2.0 * std::f64::consts::PI * start_hz * ((exponent * time_secs).exp() - 1.0)
                    / exponent;
            phase.sin() as f32
        })
        .collect()
}

fn fft_convolve(signal: &[f32], kernel: &[f32]) -> Result<Vec<f32>, Box<dyn Error>> {
    if signal.is_empty() || kernel.is_empty() {
        return Err(io::Error::new(ErrorKind::InvalidData, "Cannot convolve empty signals").into());
    }

    let convolution_len = signal.len() + kernel.len() - 1;
    let fft_size = convolution_len.next_power_of_two();
    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(fft_size);
    let c2r = planner.plan_fft_inverse(fft_size);

    let mut signal_time = r2c.make_input_vec();
    signal_time[..signal.len()].copy_from_slice(signal);
    let mut signal_spectrum = r2c.make_output_vec();
    r2c.process(&mut signal_time, &mut signal_spectrum)?;

    let mut kernel_time = r2c.make_input_vec();
    kernel_time[..kernel.len()].copy_from_slice(kernel);
    let mut kernel_spectrum = r2c.make_output_vec();
    r2c.process(&mut kernel_time, &mut kernel_spectrum)?;

    for (signal_bin, kernel_bin) in signal_spectrum.iter_mut().zip(kernel_spectrum.iter()) {
        let re = signal_bin.re * kernel_bin.re - signal_bin.im * kernel_bin.im;
        let im = signal_bin.re * kernel_bin.im + signal_bin.im * kernel_bin.re;
        signal_bin.re = re;
        signal_bin.im = im;
    }

    let mut output = c2r.make_output_vec();
    c2r.process(&mut signal_spectrum, &mut output)?;

    let scale = 1.0 / fft_size as f32;
    output.truncate(convolution_len);
    for sample in &mut output {
        *sample *= scale;
    }

    Ok(output)
}

fn impulse_window(
    impulse: &[f32],
    peak_index: usize,
    sample_rate: u32,
    input_sample_count: usize,
) -> Vec<f32> {
    let pre_samples = (IMPULSE_PRE_SECS * sample_rate as f32).round() as usize;
    let post_samples = ((IMPULSE_POST_SECS * sample_rate as f32).round() as usize)
        .min(input_sample_count)
        .max(2048);
    let window_len = pre_samples + post_samples;
    let window_start = peak_index as isize - pre_samples as isize;
    let mut window = Vec::with_capacity(window_len);

    for index in 0..window_len {
        let source_index = window_start + index as isize;
        let sample = if source_index >= 0 && source_index < impulse.len() as isize {
            impulse[source_index as usize]
        } else {
            0.0
        };
        window.push(sample);
    }

    apply_impulse_window(&mut window, pre_samples);
    window
}

fn apply_impulse_window(samples: &mut [f32], pre_samples: usize) {
    let fade_in = pre_samples.min(samples.len());
    if fade_in > 1 {
        for (index, sample) in samples.iter_mut().take(fade_in).enumerate() {
            let phase = std::f32::consts::FRAC_PI_2 * index as f32 / fade_in as f32;
            *sample *= phase.sin().powi(2);
        }
    }

    let fade_out = (samples.len() / 10).max(1);
    for index in 0..fade_out {
        let sample_index = samples.len() - fade_out + index;
        let phase = std::f32::consts::FRAC_PI_2 * (index + 1) as f32 / fade_out as f32;
        samples[sample_index] *= phase.cos().powi(2);
    }
}

fn frequency_response(
    input_impulse: &[f32],
    output_impulse: &[f32],
    sample_rate: u32,
    params: SweepParams,
) -> Result<Vec<FrequencyResponsePoint>, Box<dyn Error>> {
    let fft_size = input_impulse
        .len()
        .max(output_impulse.len())
        .next_power_of_two()
        .max(4096);
    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(fft_size);

    let mut input_time = r2c.make_input_vec();
    input_time[..input_impulse.len()].copy_from_slice(input_impulse);
    let mut input_spectrum = r2c.make_output_vec();
    r2c.process(&mut input_time, &mut input_spectrum)?;

    let mut output_time = r2c.make_input_vec();
    output_time[..output_impulse.len()].copy_from_slice(output_impulse);
    let mut output_spectrum = r2c.make_output_vec();
    r2c.process(&mut output_time, &mut output_spectrum)?;

    let nyquist = sample_rate as f32 / 2.0;
    let mut response = Vec::new();

    for &frequency_hz in &RESPONSE_FREQUENCIES_HZ {
        if frequency_hz < params.start_hz || frequency_hz > params.end_hz || frequency_hz >= nyquist
        {
            continue;
        }

        let bin = (frequency_hz * fft_size as f32 / sample_rate as f32).round() as usize;
        if bin == 0 || bin >= input_spectrum.len() {
            continue;
        }

        let input_bin = input_spectrum[bin];
        let output_bin = output_spectrum[bin];
        let input_power = input_bin.re * input_bin.re + input_bin.im * input_bin.im;
        if input_power < MIN_SPECTRUM_POWER {
            continue;
        }

        let response_re =
            (output_bin.re * input_bin.re + output_bin.im * input_bin.im) / input_power;
        let response_im =
            (output_bin.im * input_bin.re - output_bin.re * input_bin.im) / input_power;
        let magnitude = (response_re * response_re + response_im * response_im).sqrt();

        response.push(FrequencyResponsePoint {
            frequency_hz,
            magnitude_db: metrics::db(magnitude),
        });
    }

    if response.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "No frequency response points were available inside the sweep range",
        )
        .into());
    }

    Ok(response)
}

#[derive(Clone, Copy, Debug)]
struct Peak {
    index: usize,
    value: f32,
}

fn peak_index(samples: &[f32]) -> Option<Peak> {
    samples
        .iter()
        .enumerate()
        .map(|(index, sample)| Peak {
            index,
            value: sample.abs(),
        })
        .max_by(|left, right| left.value.total_cmp(&right.value))
}

fn peak_index_near(samples: &[f32], center: usize, max_offset: usize) -> Option<Peak> {
    if samples.is_empty() {
        return None;
    }

    let start = center.saturating_sub(max_offset);
    let end = (center + max_offset + 1).min(samples.len());

    peak_index(&samples[start..end]).map(|peak| Peak {
        index: start + peak.index,
        value: peak.value,
    })
}

fn max_delay_samples(sample_rate: u32) -> usize {
    (MAX_DELAY_SECS * sample_rate as f32).round() as usize
}

fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
}

#[cfg(test)]
mod tests {
    use crate::audio_buffer::AudioBuffer;

    use super::{SweepParams, analyze_sweep_pair, parse_sweep_params_from_name, render_log_sweep};

    const SAMPLE_RATE: u32 = 44_100;

    #[test]
    fn parses_generated_sweep_name() {
        assert_eq!(
            parse_sweep_params_from_name("sweep_20_20k_-24.wav"),
            Some(SweepParams {
                start_hz: 20.0,
                end_hz: 20_000.0,
            })
        );
        assert_eq!(
            parse_sweep_params_from_name("sweep_20_20000_-24.wav"),
            Some(SweepParams {
                start_hz: 20.0,
                end_hz: 20_000.0,
            })
        );
        assert_eq!(parse_sweep_params_from_name("sine_1000_-24.wav"), None);
    }

    #[test]
    fn reports_flat_gain_with_latency_compensation() {
        let input = test_sweep(100.0, 8000.0, -24.0, 2.0);
        let delay_samples = 321;
        let gain = 2.0;
        let output = delayed_gain(&input, delay_samples, gain);

        let report = analyze_sweep_pair(
            &buffer("sweep_100_8000_-24.wav", input),
            &buffer("sweep_100_8000_amp.wav", output),
        )
        .expect("sweep analysis should succeed");

        assert_eq!(report.delay_samples, delay_samples as isize);
        assert!((report.output.rms_gain_db - 6.02).abs() < 0.05);

        for frequency_hz in [125.0, 1000.0, 4000.0, 8000.0] {
            let point = response_point(&report, frequency_hz);
            assert!(
                (point.magnitude_db - 6.02).abs() < 0.35,
                "{}Hz response was {:.2}dB",
                frequency_hz,
                point.magnitude_db
            );
        }
    }

    #[test]
    fn reports_flat_gain_when_latency_truncates_output_tail() {
        let input = test_sweep(100.0, 8000.0, -24.0, 2.0);
        let delay_samples = 256;
        let mut output = delayed_gain(&input, delay_samples, 0.5);
        output.truncate(input.len());

        let report = analyze_sweep_pair(
            &buffer("sweep_100_8000_-24.wav", input),
            &buffer("sweep_100_8000_amp.wav", output),
        )
        .expect("sweep analysis should succeed");

        assert_eq!(report.delay_samples, delay_samples as isize);
        assert!((report.output.rms_gain_db + 6.02).abs() < 0.05);

        for frequency_hz in [125.0, 1000.0, 4000.0] {
            let point = response_point(&report, frequency_hz);
            assert!(
                (point.magnitude_db + 6.02).abs() < 0.4,
                "{}Hz response was {:.2}dB",
                frequency_hz,
                point.magnitude_db
            );
        }
    }

    #[test]
    fn reports_known_lowpass_shape() {
        let input = test_sweep(100.0, 10_000.0, -18.0, 2.0);
        let delay_samples = 128;
        let filtered = moving_average(&input, 4);
        let output = delayed_gain(&filtered, delay_samples, 1.0);

        let report = analyze_sweep_pair(
            &buffer("sweep_100_10000_-18.wav", input),
            &buffer("sweep_100_10000_amp.wav", output),
        )
        .expect("sweep analysis should succeed");

        let low = response_point(&report, 125.0).magnitude_db;
        let high = response_point(&report, 8000.0).magnitude_db;

        assert!(low.abs() < 0.4, "125Hz response was {low:.2}dB");
        assert!(high < -5.0, "8000Hz response was {high:.2}dB");
    }

    fn test_sweep(start_hz: f32, end_hz: f32, peak_dbfs: f32, duration_secs: f32) -> Vec<f32> {
        let sample_count = (duration_secs * SAMPLE_RATE as f32).round() as usize;
        let params = SweepParams { start_hz, end_hz };
        let amplitude = 10.0_f32.powf(peak_dbfs / 20.0);

        render_log_sweep(params, SAMPLE_RATE, sample_count)
            .into_iter()
            .map(|sample| sample * amplitude)
            .collect()
    }

    fn delayed_gain(input: &[f32], delay_samples: usize, gain: f32) -> Vec<f32> {
        let mut output = vec![0.0; delay_samples];
        output.extend(input.iter().map(|sample| sample * gain));
        output
    }

    fn moving_average(input: &[f32], len: usize) -> Vec<f32> {
        let mut output = vec![0.0; input.len()];
        for (index, sample) in output.iter_mut().enumerate() {
            let start = index.saturating_sub(len - 1);
            let end = index + 1;
            *sample = input[start..end].iter().sum::<f32>() / (end - start) as f32;
        }
        output
    }

    fn response_point(
        report: &super::SweepAnalysisReport,
        frequency_hz: f32,
    ) -> super::FrequencyResponsePoint {
        report
            .frequency_response
            .iter()
            .find(|point| (point.frequency_hz - frequency_hz).abs() < f32::EPSILON)
            .copied()
            .expect("frequency response point should exist")
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
