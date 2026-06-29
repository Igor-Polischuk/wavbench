mod args;
mod audio_buffer;
mod wav;

use std::error::Error;

use args::parse_args;
use wav::load_wav;

use audio_buffer::AudioBuffer;

fn main() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;

    let first_wav = load_wav(&args.first_wav_path)?;
    let second_wav = load_wav(&args.second_wav_path)?;

    println!("Loaded WAV files:");
    print_wav_info("first", &first_wav);
    print_wav_info("second", &second_wav);

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
