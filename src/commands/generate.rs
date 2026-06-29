use std::{error::Error, fs};

use crate::{
    args::GenArgs,
    generator::{
        presets::{BITS_PER_SAMPLE, CHANNELS, FADE_MS, SAMPLE_RATE, select_presets},
        signal::render_preset,
        wav_file::write_float_wav,
    },
};

pub fn run(args: GenArgs) -> Result<(), Box<dyn Error>> {
    let presets = select_presets(&args.target)?;
    fs::create_dir_all(&args.output_dir)?;

    println!(
        "Generating {} file(s): sample_rate={}Hz bits={} float channels={} fade={:.0}ms",
        presets.len(),
        SAMPLE_RATE,
        BITS_PER_SAMPLE,
        CHANNELS,
        FADE_MS,
    );

    for preset in presets {
        let samples = render_preset(preset);
        let path = args.output_dir.join(preset.file_name);

        write_float_wav(&path, &samples)?;
        println!("Wrote {}", path.display());
    }

    Ok(())
}
