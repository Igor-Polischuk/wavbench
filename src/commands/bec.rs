use std::error::Error;

use crate::{
    args::BecArgs,
    audio_buffer::AudioBuffer,
    band_energy::{BANDS_1_3_OCTAVE, BandEnergy},
    metrics::{correlation, db, null_test},
    prepare::PreparedComparison,
    wav::load_wav,
};

pub fn run(args: BecArgs) -> Result<(), Box<dyn Error>> {
    let target_wav = load_wav(&args.target_wav_path)?;
    let candidate_wav = load_wav(&args.candidate_wav_path)?;

    println!("Loaded WAV files:");
    print_wav_info("first", &target_wav);
    print_wav_info("second", &candidate_wav);

    let prepared = PreparedComparison::prepare(&target_wav, &candidate_wav)?;
    print_rms_info(&target_wav.id, &candidate_wav.id, &prepared);

    let null_test_result = null_test(&prepared.target, &prepared.candidate_matched);
    println!("Null RMS: {:.2} dBFS", db(null_test_result));

    let corr = correlation(&prepared.candidate_matched, &prepared.target);
    println!("Correlation: {:.4}", corr);

    let target_energy = BandEnergy::from_samples(&prepared.target, target_wav.sample_rate)?;
    let candidate_energy =
        BandEnergy::from_samples(&prepared.candidate_matched, candidate_wav.sample_rate)?;

    print_band_energy_diff_table(&target_energy, &candidate_energy);

    Ok(())
}

fn print_wav_info(label: &str, wav: &AudioBuffer) {
    println!(
        "{label}: channels={} sample_rate={}Hz bits={}  samples={} frames={}",
        wav.channels,
        wav.sample_rate,
        wav.bits_per_sample,
        wav.samples.len(),
        wav.frames,
    );
}

fn print_rms_info(target_id: &str, candidate_id: &str, prepared: &PreparedComparison) {
    println!("{}", String::from("-").repeat(40));
    println!(
        "Target ({}) RMS:  {:.2} dBFS",
        target_id,
        db(prepared.target_rms)
    );
    println!(
        "Candidate ({}) RMS: {:.2} dBFS",
        candidate_id,
        db(prepared.candidate_rms)
    );
    println!("{}", String::from("-").repeat(40));
    println!("Candidate needs gain: {:.2} dB", prepared.gain_db);
}

fn print_band_energy_diff_table(target_energy: &BandEnergy, candidate_energy: &BandEnergy) {
    println!(
        "{:<15} | {:<10} | {:<10} | {}",
        "Freq (Hz)", "Target", "Candidate", "Diff"
    );
    println!("{}", "-".repeat(55));

    for (i, &freq) in BANDS_1_3_OCTAVE.iter().enumerate() {
        let target_power = target_energy.bands[i];
        let candidate_power = candidate_energy.bands[i];

        let target_db = db(target_power);
        let candidate_db = db(candidate_power);
        let diff_db = target_db - candidate_db;

        let freq_label = if freq >= 1000.0 {
            format!("{:.1} kHz", freq / 1000.0)
        } else {
            format!("{:.1} Hz", freq)
        };

        println!(
            "{:<15} | {:>7.2} dB | {:>7.2} dB | {:>+7.2} dB",
            freq_label, target_db, candidate_db, diff_db
        );
    }
}
