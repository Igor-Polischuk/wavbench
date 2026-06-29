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
}

#[derive(Args, Debug)]
pub struct BecArgs {
    /// Reference WAV file.
    pub target_wav_path: PathBuf,

    /// Candidate WAV file to compare against the reference.
    pub candidate_wav_path: PathBuf,
}
