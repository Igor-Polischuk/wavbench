mod args;
mod audio_buffer;
mod band_energy;
mod rms;
mod wav;

use std::error::Error;

use args::parse_args;
use wav::load_wav;

use audio_buffer::AudioBuffer;

use crate::{band_energy::get_band_energy, rms::compare_rms};

fn main() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;

    let target_wav = load_wav(&args.target_wav_path)?;
    let candidate_wav = load_wav(&args.candidate_wav_path)?;

    println!("Loaded WAV files:");
    print_wav_info("first", &target_wav);
    print_wav_info("second", &candidate_wav);

    compare_rms(&target_wav, &candidate_wav);

    let target_energy = get_band_energy(&target_wav);
    let candidate_energy = get_band_energy(&candidate_wav);

    println!("{:?}", target_energy);
    println!("{:?}", candidate_energy);

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
