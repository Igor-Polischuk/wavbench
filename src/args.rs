use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(version, about = "WAV comparison and analysis tools")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Compare band energy between two WAV files.
    Bec(BecArgs),

    /// Generate WAV test signals.
    Gen(GenArgs),
}

#[derive(Args, Debug)]
pub struct BecArgs {
    /// Reference WAV file.
    pub target_wav_path: PathBuf,

    /// Candidate WAV file to compare against the reference.
    pub candidate_wav_path: PathBuf,
}

#[derive(Args, Debug)]
pub struct GenArgs {
    /// Signal group or exact preset to generate: all, sine, sweep, noise, or a preset name like sine_1000_-36.
    #[arg(default_value = "all")]
    pub target: String,

    /// Directory where generated WAV files will be written.
    #[arg(default_value = ".")]
    pub output_dir: PathBuf,
}
