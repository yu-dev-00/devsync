mod config;

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
    let _cfg = cfg;

    match cli.command {
        Command::Status => {
            println!("status is not implemented yet");
        }
        Command::Sync(args) => {
            println!("sync is not implemented yet; delete={}", args.delete);
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
            println!("agent stdio is not implemented yet");
        }
    }

    Ok(())
}
