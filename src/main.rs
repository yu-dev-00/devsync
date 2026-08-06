use anyhow::Result;
use devsync::{agent, config, init, sync, verbose};
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "devsync")]
#[command(about = "Sync local projects to a remote Windows execution copy")]
struct Cli {
    // `global` so these work on either side of the subcommand. `devsync build -v`
    // is what people actually type; without it clap rejects the flag there.
    #[arg(long, default_value = "devsync.toml", global = true)]
    config: PathBuf,

    /// Print local progress and protocol diagnostics to stderr
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create devsync.toml in this project (and optionally install the skill)
    Init(InitArgs),
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
struct InitArgs {
    /// Remote host to write into connection.host
    #[arg(long)]
    host: Option<String>,
    /// Remote user to write into connection.user
    #[arg(long)]
    user: Option<String>,
    /// Remote execution directory to write into paths.remote_dir
    #[arg(long)]
    remote_dir: Option<String>,
    /// Overwrite an existing config
    #[arg(long)]
    force: bool,
    /// Also install the Claude Code skill into ~/.claude/skills/devsync
    #[arg(long)]
    install_skill: bool,
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
    verbose::set_enabled(cli.verbose);

    // `init` is what creates the config, so requiring one would make it unusable
    // in exactly the situation it exists for.
    let cfg = if matches!(&cli.command, Command::Agent(_) | Command::Init(_)) {
        None
    } else {
        Some(config::Config::load(&cli.config)?)
    };
    match cli.command {
        Command::Init(args) => {
            init::run(
                &cli.config,
                &init::InitOptions {
                    host: args.host,
                    user: args.user,
                    remote_dir: args.remote_dir,
                    force: args.force,
                    install_skill: args.install_skill,
                },
            )?;
        }
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
