mod args;
mod audio_buffer;
mod band_energy;
mod commands;
mod generator;
mod metrics;
mod noise_analysis;
mod prepare;
mod sine_analysis;
mod sweep_analysis;
mod twotone_analysis;
mod wav;
mod wav_info;

use std::error::Error;

use clap::Parser;

use args::{Cli, Command};

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    match cli.command {
        Command::Bec(args) => commands::bec::run(args),
        Command::Energy(args) => commands::energy::run(args),
        Command::Gen(args) => commands::generate::run(args),
        Command::Info(args) => commands::info::run(args),
        Command::Noise(args) => commands::noise::run(args),
        Command::Sine(args) => commands::sine::run(args),
        Command::Sweep(args) => commands::sweep::run(args),
        Command::Twotone(args) => commands::twotone::run(args),
    }
}
