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
    /// Execute a named command from [commands], syncing first by default
    Exec(ExecArgs),
    /// Alias for `exec build`; syncs first by default
    Build(RunFlags),
    /// Alias for `exec run`; syncs first by default
    Run(RunFlags),
    /// Alias for `exec test`; syncs first by default
    Test(RunFlags),
    Agent(AgentArgs),
}

#[derive(Debug, Args)]
struct SyncArgs {
    #[arg(long)]
    delete: bool,
}

#[derive(Debug, Args)]
struct ExecArgs {
    /// Name of the command in [commands] to execute
    name: String,
    #[command(flatten)]
    flags: RunFlags,
}

#[derive(Debug, Args)]
struct RunFlags {
    /// Skip the sync step and execute against the current remote copy
    #[arg(long)]
    no_sync: bool,
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
        Command::Exec(args) => {
            let cfg = cfg.as_ref().expect("config loaded for local commands");
            let code = sync::run_command(cfg, &args.name, args.flags.no_sync)?;
            std::process::exit(code);
        }
        Command::Build(flags) => {
            let cfg = cfg.as_ref().expect("config loaded for local commands");
            let code = sync::run_command(cfg, "build", flags.no_sync)?;
            std::process::exit(code);
        }
        Command::Run(flags) => {
            let cfg = cfg.as_ref().expect("config loaded for local commands");
            let code = sync::run_command(cfg, "run", flags.no_sync)?;
            std::process::exit(code);
        }
        Command::Test(flags) => {
            let cfg = cfg.as_ref().expect("config loaded for local commands");
            let code = sync::run_command(cfg, "test", flags.no_sync)?;
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
