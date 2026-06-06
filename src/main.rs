mod agent;
mod client;
mod config;
mod diff;
mod exclude;
mod manifest;
mod path_safety;
mod protocol;
mod sync;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "devsync")]
#[command(about = "Sync local projects to a remote Windows execution copy")]
struct Cli {
    #[arg(long, default_value = "devsync.toml")]
    config: PathBuf,

    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Status,
    Sync(SyncArgs),
    Build,
    Run,
    Test,
    Agent(AgentArgs),
}

#[derive(Debug, Args)]
struct SyncArgs {
    #[arg(long)]
    delete: bool,
}

#[derive(Debug, Args)]
struct AgentArgs {
    #[arg(long)]
    stdio: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let cfg = if matches!(&cli.command, Command::Agent(_)) {
        None
    } else {
        Some(config::Config::load(&cli.config)?)
    };
    match cli.command {
        Command::Status => {
            let cfg = cfg.as_ref().expect("config loaded for local commands");
            sync::status(cfg)?;
        }
        Command::Sync(args) => {
            let cfg = cfg.as_ref().expect("config loaded for local commands");
            sync::sync(cfg, args.delete)?;
        }
        Command::Build => {
            println!("build is not implemented yet");
        }
        Command::Run => {
            println!("run is not implemented yet");
        }
        Command::Test => {
            println!("test is not implemented yet");
        }
        Command::Agent(args) => {
            if !args.stdio {
                anyhow::bail!("agent requires --stdio");
            }
            agent::run_stdio_agent()?;
        }
    }

    Ok(())
}
