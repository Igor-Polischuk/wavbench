use std::error::Error;

use crate::{
    args::InfoArgs,
    wav::load_wav,
    wav_info::{WavInfo, compare_wav_info},
};

pub fn run(args: InfoArgs) -> Result<(), Box<dyn Error>> {
    let infos = args
        .wav_paths
        .iter()
        .map(|path| {
            let wav = load_wav(path)?;
            Ok(WavInfo::from_audio_buffer(path.display().to_string(), &wav))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;

    print_infos(&infos);
    print_checks(&infos);

    Ok(())
}

fn print_infos(infos: &[WavInfo]) {
    println!("{}", "=".repeat(56));
    println!("WAV INFO");
    println!("{}", "=".repeat(56));

    for info in infos {
        println!();
        println!("{}", info.path);
        println!("{}", "-".repeat(40));
        println!("Sample Rate:       {} Hz", info.sample_rate);
        println!("Duration:          {:.3} s", info.duration_secs);
        println!("Number of samples: {}", info.frames);
        println!("Total samples:     {}", info.total_samples);
        println!("Channels:          {}", info.channels);
        println!("Bit depth:         {}", info.bits_per_sample);
    }
}

fn print_checks(infos: &[WavInfo]) {
    let checks = compare_wav_info(infos);

    println!();
    println!("Checks");
    println!("{}", "-".repeat(40));
    println!("same sample rate:  {}", yes_no(checks.same_sample_rate));
    println!("same sample count: {}", yes_no(checks.same_sample_count));
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
