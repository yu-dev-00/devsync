use anyhow::Result;
use devsync::{agent, config, sync};
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
        // Note: these arms end with std::process::exit(code), which skips destructors
        // (RemoteClient::Drop). That is safe here — exec() returns only after the
        // remote Exit frame is received, so the protocol exchange is already complete;
        // the OS reaps the ssh subprocess on process exit.
        Command::Build => {
            let cfg = cfg.as_ref().expect("config loaded for local commands");
            let code = sync::sync_then_exec(cfg, "build")?;
            std::process::exit(code);
        }
        Command::Run => {
            let cfg = cfg.as_ref().expect("config loaded for local commands");
            let code = sync::exec(cfg, "run")?;
            std::process::exit(code);
        }
        Command::Test => {
            let cfg = cfg.as_ref().expect("config loaded for local commands");
            let code = sync::sync_then_exec(cfg, "test")?;
            std::process::exit(code);
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
