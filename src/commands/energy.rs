use std::error::Error;

use crate::{
    args::EnergyArgs,
    audio_buffer::AudioBuffer,
    band_energy::{BANDS_1_3_OCTAVE, BandEnergy},
    metrics::{db, rms},
    wav::load_wav,
};

pub fn run(args: EnergyArgs) -> Result<(), Box<dyn Error>> {
    let wav = load_wav(&args.wav_path)?;
    let samples = wav.to_mono_left();
    let energy = BandEnergy::from_samples(&samples, wav.sample_rate)?;

    print_report(&wav, &samples, &energy);

    Ok(())
}

fn print_report(wav: &AudioBuffer, samples: &[f32], energy: &BandEnergy) {
    println!("{}", "=".repeat(52));
    println!("BAND ENERGY");
    println!("{}", "=".repeat(52));

    println!();
    println!("File");
    println!("{}", "-".repeat(35));
    println!("Name:        {}", wav.id);
    println!("Channels:    {}", wav.channels);
    println!("Sample rate: {} Hz", wav.sample_rate);
    println!("Frames:      {}", wav.frames);
    println!("RMS:         {:.2} dBFS", db(rms(samples)));

    println!();
    println!("Energy");
    println!("{}", "-".repeat(35));
    println!("{:<15} | {:<10}", "Freq (Hz)", "Energy");
    println!("{}", "-".repeat(30));

    for (index, &freq) in BANDS_1_3_OCTAVE.iter().enumerate() {
        println!(
            "{:<15} | {:>7.2} dB",
            format_frequency_label(freq),
            db(energy.bands[index])
        );
    }
}

fn format_frequency_label(freq: f32) -> String {
    if freq >= 1000.0 {
        format!("{} kHz", trim_float(freq / 1000.0))
    } else {
        format!("{} Hz", trim_float(freq))
    }
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
