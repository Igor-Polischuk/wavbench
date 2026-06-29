mod args;
mod audio_buffer;
mod band_energy;
mod commands;
mod metrics;
mod prepare;
mod wav;

use std::error::Error;

use clap::Parser;

use args::{Cli, Command};

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    match cli.command {
        Command::Bec(args) => commands::bec::run(args),
    }
}
