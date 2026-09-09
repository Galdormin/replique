use std::{path::PathBuf, process::ExitCode};

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::check::check;

mod check;
mod discover;

pub const EXTENSIONS: &[&str] = &["rep", "replique"];

#[derive(Parser)]
#[command(
    name = "replique",
    version,
    about = "Tools for writing Replique Dialogues"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check all given dialogue files
    Check(CheckArgs),
}

#[derive(Args)]
struct CheckArgs {
    /// Files or folders to check
    paths: Vec<PathBuf>,

    /// Display format
    #[arg(short, long, value_enum, default_value = "pretty")]
    format: Format,

    /// Handle warnings as errors
    #[arg(short = 'W', long)]
    warnings_as_errors: bool,

    /// Don't display diagnostics
    #[arg(short, long)]
    quiet: bool,

    /// Recursive for folder
    #[arg(short, long)]
    recursive: bool,
}

#[derive(Clone, ValueEnum)]
enum Format {
    Pretty,
    Short,
}

pub enum Outcome {
    Clean,
    Errors,
}

fn main() -> ExitCode {
    let args = Cli::parse();

    let outcome = match args.command {
        Command::Check(check_args) => check(&check_args),
    };

    match outcome {
        Ok(Outcome::Clean) => ExitCode::SUCCESS,
        Ok(Outcome::Errors) => ExitCode::from(1),
        Err(e) => {
            eprintln!("replique: {e}");
            ExitCode::from(2)
        }
    }
}
