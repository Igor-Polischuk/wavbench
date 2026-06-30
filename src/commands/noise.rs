use std::error::Error;

use crate::{
    args::NoiseArgs,
    noise_analysis::{
        NoiseAnalysisReport, NoiseSpectrumBand, SpectralTiltBand, analyze_noise_pair,
    },
    wav::load_wav,
};

pub fn run(args: NoiseArgs) -> Result<(), Box<dyn Error>> {
    let input_wav = load_wav(&args.input)?;
    let output_wav = load_wav(&args.output)?;
    let report = analyze_noise_pair(&input_wav, &output_wav)?;

    print_report(&report);

    Ok(())
}

fn print_report(report: &NoiseAnalysisReport) {
    println!("{}", "=".repeat(52));
    println!("NOISE ANALYSIS");
    println!("{}", "=".repeat(52));

    println!();
    println!("Input");
    println!("{}", "-".repeat(35));
    println!("RMS:           {:>8.2} dBFS", report.input.rms_dbfs);
    println!("Peak:          {:>8.2} dBFS", report.input.peak_dbfs);
    println!("Crest factor:  {:>8.2} dB", report.input.crest_factor_db);

    println!();
    println!("Output");
    println!("{}", "-".repeat(35));
    println!("RMS:           {:>8.2} dBFS", report.output.rms_dbfs);
    println!("Peak:          {:>8.2} dBFS", report.output.peak_dbfs);
    println!("Crest factor:  {:>8.2} dB", report.output.crest_factor_db);
    if let Some(dc_offset) = report.output.dc_offset {
        println!("DC offset:     {:>8.5}", dc_offset);
    }

    println!();
    println!("Gain");
    println!("{}", "-".repeat(35));
    println!("RMS gain:      {:>+8.2} dB", report.rms_gain_db);

    println!();
    println!("Average Spectrum");
    println!("{}", "-".repeat(35));
    for band in &report.average_spectrum {
        print_spectrum_band(band);
    }

    println!();
    println!("Spectral Tilt");
    println!("{}", "-".repeat(35));
    for band in &report.spectral_tilt {
        print_tilt_band(band);
    }
}

fn print_spectrum_band(band: &NoiseSpectrumBand) {
    println!(
        "{:<10} {:>+8.1} dB",
        format_frequency_label(band.frequency_hz),
        band.response_db
    );
}

fn print_tilt_band(band: &SpectralTiltBand) {
    println!(
        "{:<10} {:>8}: {:>+7.1} dB",
        band.name,
        format_frequency_range(band.start_hz, band.end_hz),
        band.response_db
    );
}

fn format_frequency_range(start_hz: f32, end_hz: f32) -> String {
    format!(
        "{}-{}",
        format_frequency_label(start_hz),
        format_frequency_label(end_hz)
    )
}

fn format_frequency_label(frequency_hz: f32) -> String {
    if frequency_hz >= 1000.0 {
        let khz = frequency_hz / 1000.0;
        return format!("{} kHz", trim_float(khz));
    }

    format!("{} Hz", trim_float(frequency_hz))
}

fn trim_float(value: f32) -> String {
    if (value - value.round()).abs() < 0.001 {
        return format!("{:.0}", value);
    }

    let formatted = format!("{:.2}", value);
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}
