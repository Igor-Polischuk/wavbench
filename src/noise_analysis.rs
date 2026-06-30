use std::{
    error::Error,
    io::{self, ErrorKind},
};

use realfft::RealFftPlanner;

use crate::{audio_buffer::AudioBuffer, metrics};

const STEADY_START_SECS: f32 = 0.5;
const STEADY_END_SECS: f32 = 9.5;
const EDGE_TRIM_SECS: f32 = 0.5;
const FFT_SIZE: usize = 8192;
const HOP_SIZE: usize = FFT_SIZE / 2;
const MIN_POWER: f32 = 1e-24;

const RESPONSE_FREQUENCIES_HZ: [f32; 28] = [
    20.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0, 400.0, 500.0,
    630.0, 800.0, 1000.0, 1250.0, 1600.0, 2000.0, 2500.0, 3150.0, 4000.0, 5000.0, 6300.0, 8000.0,
    10_000.0, 16_000.0,
];

const TILT_BANDS: [TiltBandSpec; 4] = [
    TiltBandSpec {
        name: "Low band",
        start_hz: 80.0,
        end_hz: 250.0,
    },
    TiltBandSpec {
        name: "Mid band",
        start_hz: 250.0,
        end_hz: 1000.0,
    },
    TiltBandSpec {
        name: "High band",
        start_hz: 1000.0,
        end_hz: 5000.0,
    },
    TiltBandSpec {
        name: "Fizz band",
        start_hz: 5000.0,
        end_hz: 10_000.0,
    },
];

#[derive(Debug)]
pub struct NoiseAnalysisReport {
    pub input: NoiseSignalStats,
    pub output: NoiseSignalStats,
    pub rms_gain_db: f32,
    pub average_spectrum: Vec<NoiseSpectrumBand>,
    pub spectral_tilt: Vec<SpectralTiltBand>,
}

#[derive(Clone, Copy, Debug)]
pub struct NoiseSignalStats {
    pub rms_dbfs: f32,
    pub peak_dbfs: f32,
    pub crest_factor_db: f32,
    pub dc_offset: Option<f32>,
}

#[derive(Clone, Copy, Debug)]
pub struct NoiseSpectrumBand {
    pub frequency_hz: f32,
    pub response_db: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct SpectralTiltBand {
    pub name: &'static str,
    pub start_hz: f32,
    pub end_hz: f32,
    pub response_db: f32,
}

#[derive(Clone, Copy)]
struct TiltBandSpec {
    name: &'static str,
    start_hz: f32,
    end_hz: f32,
}

pub fn analyze_noise_pair(
    input_wav: &AudioBuffer,
    output_wav: &AudioBuffer,
) -> Result<NoiseAnalysisReport, Box<dyn Error>> {
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

    let input_steady = steady_section(&input, input_wav.sample_rate)?;
    let output_steady = steady_section(&output, output_wav.sample_rate)?;
    let input_stats = signal_stats(input_steady, None);
    let output_dc = mean(output_steady);
    let output_without_dc = output_steady
        .iter()
        .map(|sample| sample - output_dc)
        .collect::<Vec<_>>();
    let output_stats = signal_stats(output_steady, Some(output_dc));
    let rms_gain_db = output_stats.rms_dbfs - input_stats.rms_dbfs;

    let input_spectrum = AverageSpectrum::from_samples(input_steady)?;
    let output_spectrum = AverageSpectrum::from_samples(&output_without_dc)?;
    let average_spectrum = response_bands(&input_spectrum, &output_spectrum, input_wav.sample_rate);
    let spectral_tilt = tilt_bands(&input_spectrum, &output_spectrum, input_wav.sample_rate);

    Ok(NoiseAnalysisReport {
        input: input_stats,
        output: output_stats,
        rms_gain_db,
        average_spectrum,
        spectral_tilt,
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

fn steady_section(samples: &[f32], sample_rate: u32) -> io::Result<&[f32]> {
    let duration_secs = samples.len() as f32 / sample_rate as f32;
    let (start_secs, end_secs) = if duration_secs >= STEADY_END_SECS {
        (STEADY_START_SECS, STEADY_END_SECS)
    } else if duration_secs > EDGE_TRIM_SECS * 2.0 {
        (EDGE_TRIM_SECS, duration_secs - EDGE_TRIM_SECS)
    } else {
        (0.0, duration_secs)
    };

    let start = (start_secs * sample_rate as f32).round() as usize;
    let end = (end_secs * sample_rate as f32).round() as usize;
    if end <= start || end - start < FFT_SIZE {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!(
                "Not enough steady noise samples for analysis: need at least {} samples after trimming",
                FFT_SIZE
            ),
        ));
    }

    Ok(&samples[start..end.min(samples.len())])
}

fn signal_stats(samples: &[f32], dc_offset: Option<f32>) -> NoiseSignalStats {
    let rms = metrics::rms(samples);
    let peak = peak(samples);
    let rms_dbfs = metrics::db(rms);
    let peak_dbfs = metrics::db(peak);

    NoiseSignalStats {
        rms_dbfs,
        peak_dbfs,
        crest_factor_db: peak_dbfs - rms_dbfs,
        dc_offset,
    }
}

struct AverageSpectrum {
    power: Vec<f32>,
    fft_size: usize,
}

impl AverageSpectrum {
    fn from_samples(samples: &[f32]) -> Result<Self, Box<dyn Error>> {
        if samples.len() < FFT_SIZE {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!(
                    "Not enough samples for noise FFT analysis: got {}, need at least {}",
                    samples.len(),
                    FFT_SIZE
                ),
            )
            .into());
        }

        let mut planner = RealFftPlanner::<f32>::new();
        let r2c = planner.plan_fft_forward(FFT_SIZE);
        let mut average_power = vec![0.0_f32; FFT_SIZE / 2 + 1];
        let window = hann_window();
        let window_power = window.iter().map(|value| value * value).sum::<f32>();
        let mut block_count = 0_usize;
        let mut start = 0;

        while start + FFT_SIZE <= samples.len() {
            let block = &samples[start..start + FFT_SIZE];
            let mut time_buffer = r2c.make_input_vec();
            for index in 0..FFT_SIZE {
                time_buffer[index] = block[index] * window[index];
            }

            let mut spectrum = r2c.make_output_vec();
            r2c.process(&mut time_buffer, &mut spectrum)?;

            for (index, bin) in spectrum.iter().enumerate() {
                average_power[index] += (bin.re * bin.re + bin.im * bin.im) / window_power;
            }

            block_count += 1;
            start += HOP_SIZE;
        }

        if block_count == 0 {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "No FFT blocks were available for noise analysis",
            )
            .into());
        }

        for power in &mut average_power {
            *power /= block_count as f32;
        }

        Ok(Self {
            power: average_power,
            fft_size: FFT_SIZE,
        })
    }

    fn bin_frequency_hz(&self, bin: usize, sample_rate: u32) -> f32 {
        bin as f32 * sample_rate as f32 / self.fft_size as f32
    }

    fn band_power(&self, sample_rate: u32, start_hz: f32, end_hz: f32) -> Option<f32> {
        let mut power_sum = 0.0_f32;
        let mut bin_count = 0_usize;

        for (bin, &power) in self.power.iter().enumerate() {
            let frequency_hz = self.bin_frequency_hz(bin, sample_rate);
            if frequency_hz >= start_hz && frequency_hz < end_hz {
                power_sum += power;
                bin_count += 1;
            }
        }

        (bin_count > 0).then_some(power_sum)
    }
}

fn response_bands(
    input_spectrum: &AverageSpectrum,
    output_spectrum: &AverageSpectrum,
    sample_rate: u32,
) -> Vec<NoiseSpectrumBand> {
    let nyquist = sample_rate as f32 / 2.0;
    RESPONSE_FREQUENCIES_HZ
        .iter()
        .filter_map(|&frequency_hz| {
            if frequency_hz >= nyquist {
                return None;
            }

            let (start_hz, end_hz) = third_octave_bounds(frequency_hz);
            let input_power = input_spectrum.band_power(sample_rate, start_hz, end_hz)?;
            let output_power = output_spectrum.band_power(sample_rate, start_hz, end_hz)?;
            let input_db = power_db(input_power);
            let output_db = power_db(output_power);

            Some(NoiseSpectrumBand {
                frequency_hz,
                response_db: output_db - input_db,
            })
        })
        .collect()
}

fn tilt_bands(
    input_spectrum: &AverageSpectrum,
    output_spectrum: &AverageSpectrum,
    sample_rate: u32,
) -> Vec<SpectralTiltBand> {
    TILT_BANDS
        .iter()
        .filter_map(|band| {
            let input_power = input_spectrum.band_power(sample_rate, band.start_hz, band.end_hz)?;
            let output_power =
                output_spectrum.band_power(sample_rate, band.start_hz, band.end_hz)?;

            Some(SpectralTiltBand {
                name: band.name,
                start_hz: band.start_hz,
                end_hz: band.end_hz,
                response_db: power_db(output_power) - power_db(input_power),
            })
        })
        .collect()
}

fn third_octave_bounds(center_hz: f32) -> (f32, f32) {
    let factor = 2.0_f32.powf(1.0 / 6.0);
    (center_hz / factor, center_hz * factor)
}

fn hann_window() -> Vec<f32> {
    (0..FFT_SIZE)
        .map(|index| {
            0.5 - 0.5 * (2.0 * std::f32::consts::PI * index as f32 / (FFT_SIZE - 1) as f32).cos()
        })
        .collect()
}

fn power_db(power: f32) -> f32 {
    10.0 * power.max(MIN_POWER).log10()
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

    use super::analyze_noise_pair;

    const SAMPLE_RATE: u32 = 44_100;

    #[test]
    fn reports_flat_transfer_response_for_gain() {
        let input = white_noise(-24.0, 3.0);
        let output = input.iter().map(|sample| sample * 2.0).collect::<Vec<_>>();

        let report = analyze_noise_pair(
            &buffer("pink_noise_-24.wav", input),
            &buffer("pink_noise_amp.wav", output),
        )
        .expect("noise analysis should succeed");

        assert!((report.rms_gain_db - 6.02).abs() < 0.05);
        assert!((report.output.crest_factor_db - report.input.crest_factor_db).abs() < 0.05);

        for frequency_hz in [100.0, 1000.0, 5000.0, 10_000.0] {
            let band = spectrum_band(&report, frequency_hz);
            assert!(
                (band.response_db - 6.02).abs() < 0.05,
                "{}Hz response was {:.2}dB",
                frequency_hz,
                band.response_db
            );
        }
    }

    #[test]
    fn reports_lowpass_noise_shape() {
        let input = white_noise(-18.0, 3.0);
        let output = moving_average(&input, 8);

        let report = analyze_noise_pair(
            &buffer("pink_noise_-18.wav", input),
            &buffer("pink_noise_amp.wav", output),
        )
        .expect("noise analysis should succeed");

        let low = tilt_band(&report, "Low band").response_db;
        let high = tilt_band(&report, "High band").response_db;
        let fizz = tilt_band(&report, "Fizz band").response_db;

        assert!(low > -0.5, "low band response was {low:.2}dB");
        assert!(high < -2.0, "high band response was {high:.2}dB");
        assert!(
            fizz < high,
            "fizz {fizz:.2}dB should be below high {high:.2}dB"
        );
    }

    #[test]
    fn reports_output_dc_offset() {
        let input = white_noise(-24.0, 3.0);
        let output = input
            .iter()
            .map(|sample| sample * 0.5 + 0.01)
            .collect::<Vec<_>>();

        let report = analyze_noise_pair(
            &buffer("pink_noise_-24.wav", input),
            &buffer("pink_noise_amp.wav", output),
        )
        .expect("noise analysis should succeed");

        let dc_offset = report
            .output
            .dc_offset
            .expect("output should report DC offset");
        assert!((dc_offset - 0.01).abs() < 0.001);
    }

    fn white_noise(peak_dbfs: f32, duration_secs: f32) -> Vec<f32> {
        let sample_count = (duration_secs * SAMPLE_RATE as f32).round() as usize;
        let amplitude = 10.0_f32.powf(peak_dbfs / 20.0);
        let mut state = 0x1234_abcd_u32;

        (0..sample_count)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let normalized = state as f32 / u32::MAX as f32;
                (normalized * 2.0 - 1.0) * amplitude
            })
            .collect()
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

    fn spectrum_band(
        report: &super::NoiseAnalysisReport,
        frequency_hz: f32,
    ) -> super::NoiseSpectrumBand {
        report
            .average_spectrum
            .iter()
            .find(|band| (band.frequency_hz - frequency_hz).abs() < f32::EPSILON)
            .copied()
            .expect("spectrum band should exist")
    }

    fn tilt_band(report: &super::NoiseAnalysisReport, name: &str) -> super::SpectralTiltBand {
        report
            .spectral_tilt
            .iter()
            .find(|band| band.name == name)
            .copied()
            .expect("tilt band should exist")
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
