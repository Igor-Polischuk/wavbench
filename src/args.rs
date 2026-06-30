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

    /// Show WAV file metadata and comparison preflight checks.
    Info(InfoArgs),

    /// Analyze a processed sine test file.
    Sine(SineArgs),

    /// Analyze a processed log sweep test file.
    Sweep(SweepArgs),

    /// Analyze a processed two-tone intermodulation test file.
    #[command(name = "twotone")]
    Twotone(TwotoneArgs),
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

#[derive(Args, Debug)]
pub struct InfoArgs {
    /// WAV files to inspect. Pass multiple files to compare preflight properties.
    #[arg(required = true)]
    pub wav_paths: Vec<PathBuf>,
}

#[derive(Args, Debug)]
pub struct SineArgs {
    /// Input WAV file with the source sine test signal.
    #[arg(short, long)]
    pub input: PathBuf,

    /// Output WAV file after processing through the device or amp.
    #[arg(short, long)]
    pub output: PathBuf,
}

#[derive(Args, Debug)]
pub struct SweepArgs {
    /// Input WAV file with the source log sweep test signal.
    #[arg(short, long)]
    pub input: PathBuf,

    /// Output WAV file after processing through the device or amp.
    #[arg(short, long)]
    pub output: PathBuf,
}

#[derive(Args, Debug)]
pub struct TwotoneArgs {
    /// Input WAV file with the source two-tone test signal.
    #[arg(short, long)]
    pub input: PathBuf,

    /// Output WAV file after processing through the device or amp.
    #[arg(short, long)]
    pub output: PathBuf,
}
