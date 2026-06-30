use std::error::Error;

use crate::{
    args::TwotoneArgs,
    twotone_analysis::{
        HarmonicAnalysis, IntermodulationProduct, ToneAnalysis, TwotoneAnalysisReport,
        analyze_twotone_pair,
    },
    wav::load_wav,
};

pub fn run(args: TwotoneArgs) -> Result<(), Box<dyn Error>> {
    let input_wav = load_wav(&args.input)?;
    let output_wav = load_wav(&args.output)?;
    let report = analyze_twotone_pair(&input_wav, &output_wav)?;

    print_report(&report);

    Ok(())
}

fn print_report(report: &TwotoneAnalysisReport) {
    println!("{}", "=".repeat(52));
    println!("TWO-TONE ANALYSIS");
    println!("{}", "=".repeat(52));

    println!();
    println!("Input");
    println!("{}", "-".repeat(35));
    println!(
        "{}",
        format_frequency_label(report.input.first_frequency_hz)
    );
    println!(
        "{}",
        format_frequency_label(report.input.second_frequency_hz)
    );

    println!();
    println!("Output");
    println!("{}", "-".repeat(35));
    println!("Peak:       {:>8.2} dBFS", report.output.peak_dbfs);
    println!("RMS:        {:>8.2} dBFS", report.output.rms_dbfs);
    println!("Gain RMS:   {:>+8.2} dB", report.output.rms_gain_db);

    println!();
    println!("Fundamentals");
    println!("{}", "-".repeat(35));
    for fundamental in &report.fundamentals {
        print_fundamental(fundamental);
    }

    println!();
    println!("Harmonics");
    println!("{}", "-".repeat(35));
    for group in &report.harmonics {
        println!("{}", format_frequency_label(group.fundamental_hz));
        for harmonic in &group.harmonics {
            print_harmonic(harmonic);
        }
    }

    println!();
    println!("Intermodulation");
    println!("{}", "-".repeat(35));
    for product in &report.intermodulation {
        print_intermodulation_product(product);
    }

    println!();
    println!("IMD");
    println!("{}", "-".repeat(35));
    println!(
        "{:.2} % ({:.1} dB)",
        report.imd.percent, report.imd.relative_db
    );
}

fn print_fundamental(fundamental: &ToneAnalysis) {
    println!(
        "{:<10} {:>8.1} dBFS",
        format_frequency_label(fundamental.frequency_hz),
        fundamental.level_dbfs
    );
}

fn print_harmonic(harmonic: &HarmonicAnalysis) {
    println!(
        "H{} {:<10} {:>8.1} dBc ({:>8.1} dBFS)",
        harmonic.number,
        format_frequency_label(harmonic.frequency_hz),
        harmonic.relative_db,
        harmonic.level_dbfs
    );
}

fn print_intermodulation_product(product: &IntermodulationProduct) {
    println!(
        "{:<10} {:>8.1} dBc ({:>8.1} dBFS)",
        format_frequency_label(product.frequency_hz),
        product.relative_db,
        product.level_dbfs
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
