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
    ///
    /// With no flags on an interactive terminal this prompts. Passing
    /// `--provider` sets the provider outright and never prompts, which is what
    /// scripts and provisioning jobs should use.
    Init {
        /// Set the provider without prompting:
        /// anthropic|openai|openrouter|ollama|openai_compat|offline.
        #[arg(long = "provider")]
        provider: Option<String>,
        /// Model id to write into the config (used with `--provider`).
        #[arg(long = "model")]
        model: Option<String>,
        /// Endpoint override — required for `openai_compat`.
        #[arg(long = "base-url")]
        base_url: Option<String>,
        /// Name of the environment variable holding the API key. Omit for a
        /// provider that needs no key.
        #[arg(long = "api-key-env")]
        api_key_env: Option<String>,
        /// Scaffold and print the next steps without prompting, even on a
        /// terminal. Implied whenever we cannot safely read stdin.
        #[arg(long = "non-interactive")]
        non_interactive: bool,
    },
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
    /// Run the benchmark task set and report a completion rate.
    Bench {
        /// Override the configured provider for this benchmark only.
        #[arg(short = 'p', long = "provider")]
        provider: Option<String>,
        /// Path to a task set (YAML). Omit to use the built-in default set.
        #[arg(long = "tasks")]
        tasks: Option<std::path::PathBuf>,
        /// Also write the JSON report to this path (e.g. to publish/commit it).
        #[arg(long = "out")]
        out: Option<std::path::PathBuf>,
        /// Run each task N times and report a pass rate (de-noises a stochastic
        /// model). Default 1 = single pass.
        #[arg(long = "repeat", default_value_t = 1)]
        repeat: u32,
        /// Governance posture(s) to measure, comma-separated: off|light|engineering
        /// (e.g. `--governance off,engineering` for the differential matrix).
        /// `off` is a bench-only ungoverned baseline — it is not a product tier.
        /// Default: light (the previous behavior).
        #[arg(long = "governance")]
        governance: Option<String>,
        /// Which agent to measure: `native` (Orvena's own bounded loop, default)
        /// or a wrapped third-party CLI agent — `aider`. A wrapped agent is
        /// confined by the OS sandbox to the task's declared paths; Orvena
        /// supplies the scope, the gate, and the evidence, the agent supplies
        /// the loop.
        #[arg(long = "agent", default_value = "native")]
        agent: String,
        /// Ignore the task set's `frozen:` selection and run every task on
        /// file, alternates included (calibration / recalibration runs).
        /// Numbers from such a run are NOT the set's official reading.
        #[arg(long = "all-tasks", default_value_t = false)]
        all_tasks: bool,
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
        Command::Init { provider, model, base_url, api_key_env, non_interactive } => {
            commands::init::run(
                commands::init::ProviderArgs { kind: provider, model, base_url, api_key_env },
                non_interactive,
            )
        }
        Command::Run { task, write, provider } => commands::run::run(task, write, provider).await,
        Command::Bench { provider, tasks, out, repeat, governance, agent, all_tasks } => {
            commands::bench::run(provider, tasks, out, repeat, governance, agent, all_tasks).await
        }
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
