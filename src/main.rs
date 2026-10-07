//! Builds, verifies and benchmarks the ChaCha20-Poly1305 implementations under `impls/`, and
//! maintains the results the website is built from.

mod machine;
mod preflight;
mod results;
mod run;
mod schema;
mod ui;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// Benchmark Rust ChaCha20-Poly1305 implementations the way WireGuard uses them.
///
/// Without a subcommand, runs the benchmarks: builds every implementation under impls/, checks
/// it against test vectors, measures sealing and opening 1280-byte messages and saves the
/// results to results/<timestamp>-<machine>.json, ready to be submitted in a pull request.
#[derive(Parser, Debug)]
#[command(bin_name = "cargo run --", args_conflicts_with_subcommands = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[command(flatten)]
    run: run::RunArgs,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Benchmark and save the results (the default)
    Run(run::RunArgs),

    /// Combine all result files into the JSON file the website loads
    Merge {
        /// Directory of result files
        #[arg(long, value_name = "DIR", default_value = "results")]
        results: PathBuf,

        /// Where to write the combined file
        #[arg(long, value_name = "FILE", default_value = "site/results.json")]
        out: PathBuf,
    },

    /// Check result files (or every file in a directory) against the schema
    Validate {
        /// Files or directories [default: results]
        paths: Vec<PathBuf>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Paths are relative to the repository, wherever `cargo run` was started from.
    if let Err(e) = std::env::set_current_dir(env!("CARGO_MANIFEST_DIR")) {
        eprintln!("cannot enter the repository directory: {e}");
        return ExitCode::FAILURE;
    }

    match cli.command {
        None => run::run(cli.run),
        Some(Command::Run(args)) => run::run(args),
        Some(Command::Merge { results, out }) => results::merge(&results, &out),
        Some(Command::Validate { paths }) => results::validate(&paths),
    }
}
