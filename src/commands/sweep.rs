use std::error::Error;

use crate::{
    args::SweepArgs,
    sweep_analysis::{FrequencyResponsePoint, SweepAnalysisReport, analyze_sweep_pair},
    wav::load_wav,
};

pub fn run(args: SweepArgs) -> Result<(), Box<dyn Error>> {
    let input_wav = load_wav(&args.input)?;
    let output_wav = load_wav(&args.output)?;
    let report = analyze_sweep_pair(&input_wav, &output_wav)?;

    print_report(&report);

    Ok(())
}

fn print_report(report: &SweepAnalysisReport) {
    println!("{}", "=".repeat(56));
    println!("SWEEP ANALYSIS");
    println!("{}", "=".repeat(56));

    println!();
    println!("Input");
    println!("{}", "-".repeat(40));
    println!(
        "Start freq:      {:>8}",
        format_frequency_label(report.input.start_frequency_hz)
    );
    println!(
        "End freq:        {:>8}",
        format_frequency_label(report.input.end_frequency_hz)
    );
    println!("Duration:        {:>8.2} s", report.input.duration_secs);
    println!("RMS:             {:>8.2} dBFS", report.input.rms_dbfs);

    println!();
    println!("Output");
    println!("{}", "-".repeat(40));
    println!("Peak:            {:>8.2} dBFS", report.output.peak_dbfs);
    println!("RMS:             {:>8.2} dBFS", report.output.rms_dbfs);
    println!("Gain RMS:        {:>+8.2} dB", report.output.rms_gain_db);
    println!("Delay:           {:>8} samples", report.delay_samples);

    println!();
    println!("Frequency Response");
    println!("{}", "-".repeat(40));
    for point in &report.frequency_response {
        print_frequency_response_point(point);
    }
}

fn print_frequency_response_point(point: &FrequencyResponsePoint) {
    println!(
        "{:<12} {:>+8.1} dB",
        format_frequency_label(point.frequency_hz),
        point.magnitude_db
    );
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
