//! Argument parsing and command dispatch.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "orvena",
    version,
    about = "A customizable, config-first coding agent — the runnable reference for AI-native software engineering."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scaffold config into ./.orvena and choose a model provider.
    Init,
    /// Run a coding task through one bounded loop.
    Run {
        /// The task instruction.
        task: String,
        /// Relative paths the task may modify (everything else is read-only).
        #[arg(short = 'w', long = "write")]
        write: Vec<String>,
        /// Override the configured provider for this run only (does not change
        /// your config). Use `--provider offline` to see the loop run with no
        /// API key or network.
        #[arg(short = 'p', long = "provider")]
        provider: Option<String>,
    },
    /// Preflight: provider readiness, config validity.
    Doctor,
    /// Show the current config: provider, tier, roles, gates, budgets, skills.
    Status,
}

/// Run the CLI, returning a process exit code.
pub async fn run() -> i32 {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Init => commands::init::run(),
        Command::Run { task, write, provider } => commands::run::run(task, write, provider).await,
        Command::Doctor => commands::doctor::run(),
        Command::Status => commands::status::run(),
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

use crate::commands;
