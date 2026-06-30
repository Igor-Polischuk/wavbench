use std::error::Error;

use crate::{
    args::SineArgs,
    sine_analysis::{HarmonicAnalysis, SineAnalysisReport, analyze_sine_pair},
    wav::load_wav,
};

pub fn run(args: SineArgs) -> Result<(), Box<dyn Error>> {
    let input_wav = load_wav(&args.input)?;
    let output_wav = load_wav(&args.output)?;
    let report = analyze_sine_pair(&input_wav, &output_wav)?;

    print_report(&report);

    Ok(())
}

fn print_report(report: &SineAnalysisReport) {
    println!("{}", "=".repeat(56));
    println!("SINE ANALYSIS");
    println!("{}", "=".repeat(56));

    println!();
    println!("Input");
    println!("{}", "-".repeat(40));
    if let Some(frequency_hz) = report.input.frequency_hz {
        println!("Frequency:       {:>8.1} Hz", frequency_hz);
    }
    println!("Duration:        {:>8.2} s", report.input.duration_secs);
    println!("Peak:            {:>8.2} dBFS", report.input.peak_dbfs);
    println!("RMS:             {:>8.2} dBFS", report.input.rms_dbfs);

    println!();
    println!("Output");
    println!("{}", "-".repeat(40));
    println!("Peak:            {:>8.2} dBFS", report.output.peak_dbfs);
    println!("RMS:             {:>8.2} dBFS", report.output.rms_dbfs);

    println!();
    println!("Gain");
    println!("{}", "-".repeat(40));
    println!("Peak gain:       {:>+8.2} dB", report.peak_gain_db);
    println!("RMS gain:        {:>+8.2} dB", report.rms_gain_db);

    println!();
    println!("Fundamental");
    println!("{}", "-".repeat(40));
    println!(
        "Frequency:       {:>8.2} Hz",
        report.fundamental.frequency_hz
    );
    println!(
        "Level:           {:>8.2} dBFS",
        report.fundamental.level_dbfs
    );

    println!();
    println!("DC Offset");
    println!("{}", "-".repeat(40));
    println!("{:.5}", report.dc_offset);

    println!();
    println!("Noise floor");
    println!("{}", "-".repeat(40));
    println!("{:.1} dBFS", report.noise_floor_dbfs);

    println!();
    println!("Harmonics");
    println!("{}", "-".repeat(40));
    for harmonic in &report.harmonics {
        print_harmonic(harmonic);
    }

    println!();
    println!("Distortion");
    println!("{}", "-".repeat(40));
    println!("THD:       {:.2} %", report.distortion.thd_percent);
    println!("THD+N:     {:.2} %", report.distortion.thdn_percent);
    println!("SINAD:     {:.1} dB", report.distortion.sinad_db);

    println!();
    println!("Harmonic Contribution");
    println!("{}", "-".repeat(40));
    println!("H2: {:.0}%", report.relative_harmonics.h2_percent);
    println!("H3: {:.0}%", report.relative_harmonics.h3_percent);
    println!("H4: {:.0}%", report.relative_harmonics.h4_percent);
    println!("H5: {:.0}%", report.relative_harmonics.h5_percent);
    println!("Higher: {:.0}%", report.relative_harmonics.higher_percent);

    println!();
    print_bar(report.fundamental.frequency_hz, 1.0);
    for harmonic in report
        .harmonics
        .iter()
        .filter(|harmonic| harmonic.number <= 5)
    {
        print_bar(harmonic.frequency_hz, harmonic.amplitude_ratio);
    }
}

fn print_harmonic(harmonic: &HarmonicAnalysis) {
    println!(
        "H{} ({:.0} Hz):    {:>7.1} dBc ({:>7.1} dBFS)",
        harmonic.number, harmonic.frequency_hz, harmonic.relative_db, harmonic.level_dbfs
    );
}

fn print_bar(frequency_hz: f32, amplitude_ratio: f32) {
    let bar_width = (amplitude_ratio.clamp(0.0, 1.0) * 24.0).round() as usize;
    let label = if frequency_hz >= 1000.0 {
        format!("{:.0} Hz", frequency_hz)
    } else {
        format!("{:.1} Hz", frequency_hz)
    };

    println!("{:<10} {}", label, "#".repeat(bar_width.max(1)));
}
