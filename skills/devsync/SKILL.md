---
name: devsync
description: Build, run, and test projects on a remote Windows or Linux machine over SSH using devsync. Use for projects with .devsync/config.toml or legacy devsync.toml, for requested remote build setup, or for troubleshooting devsync sync and execution failures. Supports Codex and Claude Code.
---

# devsync

devsync keeps the local project as the source of truth and mirrors it to a
remote Windows or Linux machine that does the building and running. You edit locally; the
remote is a disposable execution copy.

## The two mistakes worth avoiding

Both come from forgetting that the remote copy is downstream of the local one.

**Do not ssh to the remote and run the build there directly.** It will appear to
work and will compile whatever was synced last, so you get a confident result
about stale code. Every devsync execution command syncs first for exactly this
reason. Go through devsync and the code you just edited is the code that runs.

**Do not edit files on the remote.** They are overwritten on the next sync, and
`sync --delete` removes anything not in the local tree. If something needs
fixing, fix it locally and sync.

## Installing devsync

Skip this whenever `devsync --help` works and the project has `.devsync/config.toml` or legacy `devsync.toml`.

The local machine is always Windows; the remote is Windows or Linux. The same
executable is both the local client and the remote agent, so it goes on
**both** machines, and both must run the same build — the handshake compares
protocol versions and refuses a mismatch. Compare `devsync --version` on each
side.

### Both remotes: the local install and the ssh check

Build with `cargo build --release` (or use a release download), copy
`target\release\devsync.exe` into `%LOCALAPPDATA%\Programs\devsync\`, and put
that directory on the user `PATH`. Do not reach for
`setx PATH "%PATH%;..."`: `%PATH%` expands to the *merged* system and user value,
which then gets written back into the user `PATH` and silently truncated at 1024
characters. Edit the user-scoped value directly:

```powershell
$dir = "$env:LOCALAPPDATA\Programs\devsync"
$user = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($user -notlike "*$dir*") {
    [Environment]::SetEnvironmentVariable('Path', "$user;$dir", 'User')
}
```

Then set up key authentication and prove it. This must print exactly `ok` — no
password prompt, no banner, not one stray character, because the agent's stdout
*is* the protocol stream:

```bash
ssh <user>@<host> "echo ok"
```

### Picking the remote: `~/.devsync/remotes.toml`

Before asking the user for host details, read `~/.devsync/remotes.toml`
(`%USERPROFILE%\.devsync\remotes.toml`). It is an optional, user-maintained
catalog of remote machines; devsync never reads it, so it may be absent or
out of date. An entry looks like:

```toml
[remotes.build-win]
host = "build-win"        # an ~/.ssh/config alias or an address
user = "dev"
os = "windows"
remote_root = 'C:\work'
note = "Windows build machine. VS 2022, .NET 8"

[remotes.gpu-linux]
host = "gpu-linux"
user = "dev"
port = 2222
os = "linux"
agent_path = "/home/dev/.local/bin/devsync"
remote_root = "/home/dev/work"
note = "CUDA 12, RTX 4090"
```

- **Entries exist:** propose the ones that fit the project, using `os` and
  `note` (a CUDA project wants the GPU box). The user picks; do not choose
  silently when more than one fits.
- **Chosen entry:** pass `host`, `user`, and `remote_dir` to `init` below.
  `remote_dir` is `remote_root` joined with the project's directory name,
  unless the user wants otherwise or the `note` suggests another root (offer
  it). `init` has no flags for the rest, so afterwards edit the config: add
  `# remote: <name>` as its first line, and copy `port` and `agent_path` into
  `[connection]` when the entry has them.
- **Make sure `remote_dir` is not another project's copy.** Two projects with
  the same directory name land in the same folder, and `sync --delete` would
  then wipe the other one. Before `init`, check the folder is free
  (`ssh -p <port> <user>@<host> "if exist C:\work\<name> echo TAKEN"`, or
  `test -e` on Linux). If it is taken, ask whether it is this project's own
  earlier copy before picking another name. After `init`, `devsync status` must
  plan no deletes for a project that has never been synced; even one means the
  folder is taken.
- **Before trusting an entry**, run `ssh -p <port> <user>@<host> "echo ok"` and
  compare `devsync --version` with the remote's
  (`ssh <host> "devsync.exe --version"`, or `<agent_path> --version` on Linux).
  The catalog is a hint, not proof the machine is reachable.
- **No catalog or no fitting entry:** ask the user. After the new machine works,
  offer to add it to the catalog (create the file if needed). Never store
  passwords or keys there.

### Windows remote

**1. OpenSSH server, login shell left alone.** As administrator on the remote:

```powershell
Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0
Start-Service sshd
Set-Service -Name sshd -StartupType Automatic
```

Leave the default shell as `cmd.exe`: a PowerShell profile banner prepended to
the stream corrupts the 4-byte frame length and surfaces as a handshake failure.
No output from this means nothing is overridden:

```powershell
Get-ItemProperty "HKLM:\SOFTWARE\OpenSSH" -Name DefaultShell -ErrorAction SilentlyContinue
```

**2. Install the agent** exactly as locally — same directory, same `PATH` edit —
and confirm in a new session: `ssh <host> "where devsync.exe"`.

**3. Configure the project** from its root:

```bash
devsync init --host <host> --user <user> --remote-dir "C:\work\project"
```

### Linux remote

**1. Any OpenSSH server.** Shell startup files (`~/.bashrc` and friends) must
print nothing for a non-interactive session, or the `echo ok` check above fails
for the same reason a PowerShell banner does.

**2. Install the agent on the remote**, from the same release as the local
`devsync.exe`: the release's `devsync-x86_64-linux` is statically linked, so it
runs on any x86_64 distribution.

```bash
curl -fLO https://github.com/yu-dev-00/devsync/releases/download/v<version>/devsync-x86_64-linux
install -D -m 755 devsync-x86_64-linux ~/.local/bin/devsync
```

For another architecture, build from the matching tag with
`cargo build --release` and install `target/release/devsync` the same way.

Confirm from the local machine: `ssh <host> "~/.local/bin/devsync --version"`
matches `devsync --version`.

**3. Configure the project**, then fix two things `init` writes for Windows:

```bash
devsync init --host <host> --user <user> --remote-dir "/home/<user>/work/project"
```

Run it from PowerShell, or from Git Bash with `MSYS_NO_PATHCONV=1` in front:
Git Bash otherwise rewrites `/home/...` into `C:/Program Files/Git/home/...`
before devsync sees it.

- Add `agent_path = "/home/<user>/.local/bin/devsync"` under `[connection]`. It
  is required: sshd's non-interactive shell often lacks `~/.local/bin` on
  `PATH`, and the default is the Windows name `devsync.exe`.
- Replace the PowerShell examples in `[commands]` with `sh` command lines.

### Finishing the config and the first sync

`init` writes `.devsync/config.toml` and adds `.devsync/` to `.gitignore` when
the directory is a git repository. Its `[commands]` and `[sync].exclude` are
placeholders for a .NET/Node project; replace both before the first sync:

- `[commands]`: the project's real build/run/test command lines (see the shell
  rows in the table below).
- `[sync].exclude`: everything the toolchain generates or the remote creates —
  `.venv`, `__pycache__`, `target`, `node_modules`, datasets, model weights.
  This matters twice: excluded paths are not uploaded, and `sync --delete` never
  removes them remotely (a remote `.venv` built by `uv sync` would otherwise be
  wiped). Entries are file/directory names matched at any depth, or relative
  path prefixes, case-insensitive. Wildcards are **not** supported: `*.pyc`
  matches nothing.

Then run `devsync status` (it transfers nothing), then `devsync sync`, then the
first `devsync build` (or whichever command).

### Installing or refreshing this skill

`devsync init --install-skill --skill-target <claude|codex|both>` installs the
skill embedded in the binary to `~/.claude/skills/devsync/` (Claude Code),
`~/.agents/skills/devsync/` (Codex), or both. Pick what the user uses; omitting
the target means Claude Code. Re-run it with the same target after upgrading
devsync; an existing config is left alone. The embedded copy is the only one
guaranteed to match the binary — one copied out of a git checkout is a
bootstrap that this replaces. In Codex, restart if the skill does not appear.

## What differs between a Windows and a Linux remote

The remote OS is the agent's OS; nothing in the config names it. A POSIX
`remote_dir` (`/home/dev/project`) means a Linux remote.

| | Windows remote | Linux remote |
| --- | --- | --- |
| ssh login shell | Keep `cmd.exe`; a PowerShell profile banner breaks the handshake. | Startup files must print nothing for non-interactive sessions. |
| `agent_path` | Optional; `devsync.exe` is found on `PATH`. | Required, as an absolute path. |
| Command shell | Each command runs as `powershell -NoProfile -ExecutionPolicy Bypass -Command <command>` (Windows PowerShell 5.1: no `&&`). `.\build.ps1` is enough; wrapping it in another `powershell -File` works but starts a second PowerShell. A failing cmdlet or command lookup still exits nonzero. | `sh -c <command>`; the exit status passes through unchanged. |
| Script files | PowerShell 5.1 reads a BOM-less `.ps1` as ANSI, so save scripts with non-ASCII text as UTF-8 *with* BOM. | Bytes are copied verbatim; a CRLF script fails. Keep it LF via `.gitattributes` (`*.sh text eol=lf`), then delete the working copy and `git checkout -- <file>` — `.gitattributes` alone does not rewrite the working copy, so nothing re-uploads. |
| Executable bit | Not applicable. | Files starting with `#!` arrive executable; other modes are not synced. Without a shebang, run `sh ./script.sh`. |
| Name case | Case-insensitive. A local case-only rename is applied as a rename. | Same, and a stale second spelling beside the current one is removed by `sync --delete`. |
| Ctrl+C | sshd tears down the remote command tree. | The agent kills the command's process group; a command that calls `setsid` escapes. |
| Output text | Decoded per line as UTF-8 or the console code page. | UTF-8. |

## Start by reading .devsync/config.toml

Use `.devsync/config.toml` when present, falling back to root `devsync.toml`
for legacy projects. An explicit `--config` overrides selection. Run commands
from the project root. Relative `local_dir` in `.devsync/config.toml` is resolved
against the project root, not `.devsync/`; legacy/custom configs retain their
working-directory-relative paths. `.devsync/` contains environment-specific
config and hash cache, so exclude the whole directory from Git and sync.
`init` preserves existing legacy setups rather than creating a second config.

The config tells you what can be run and where things go. `[commands]` is the
important part — it is the complete set of things devsync is allowed to execute
on the remote.

```toml
[connection]
host = "remote-pc"
user = "user"
# agent_path is optional on Windows (defaults to devsync.exe on PATH), required on Linux

[paths]
local_dir = "."
remote_dir = "C:\\work\\project"

[commands]
build = ".\\build.ps1"
test  = "cargo test"

[sync]
exclude = ["target", "node_modules", "bin", "obj"]
```

Commands run with `remote_dir` as the working directory. Paths in a command are
relative to it, not to anything local.

## Commands

```bash
devsync status                  # list planned renames/uploads/deletes; may refresh hash caches
devsync sync                    # upload changed files; preserve existing files
devsync sync --delete           # also delete remote files absent locally
devsync exec <name>             # sync, then run [commands].<name> remotely
devsync exec <name> --no-sync   # run against whatever is on the remote now
devsync build                   # alias for: devsync exec build
devsync run                     # alias for: devsync exec run
devsync test                    # alias for: devsync exec test
```

`--config <path>` selects an explicit config without fallback.

After editing code, `devsync build` is all you need — it syncs first. Running
`devsync sync && devsync build` works but does the sync twice.

Reach for `status` when you want to understand the situation without changing
anything: before a first sync against an unfamiliar remote, or when a sync moved
more files than expected. It lists `rename` (case-only respelling to match the
local name; every sync applies it and no content changes), `upload`,
`delete (only with sync --delete)` (plain `sync` keeps these), and `skipped`.

`-v` prints diagnostics to stderr, including the exact `spawning: ssh ...` line —
the first thing to try by hand when a connection fails.

## Output and exit codes are real

Remote output streams back as it is produced, and the remote command's exit code
becomes devsync's exit code. A failing build fails the devsync invocation with
the compiler's own code, so treat it exactly like a local build: read the error,
fix the source, re-run.

## Running something that isn't configured yet

There is no arbitrary-command path — `exec` resolves names against `[commands]`
and nothing else. That is a deliberate security boundary, not a gap to work
around, so do not reach for `ssh` when a command is missing.

The right move is to add the entry to `[commands]` in the selected config:

```toml
[commands]
lint = "cargo clippy -- -D warnings"
```

Then `devsync exec lint`. Since the config is excluded from sync, adding a
command takes effect immediately with no redeploy.

Command names are arbitrary and may collide with subcommands: `devsync exec sync`
runs `commands.sync`, not the built-in sync.

## Excludes decide what --delete destroys

`[sync].exclude` applies on **both** sides. Excluded paths are invisible to the
diff on the remote too, which is what keeps `sync --delete` from wiping remote
build output like `target/` or `obj/`. `devsync.toml`, `.git/`, and `.devsync/`
are always excluded.

`sync --delete` is the one command here that destroys data. Plain `sync` retains remote-only files and updates matching files; use `--delete` only within the user's
authorized scope. File/directory replacements may remove empty directories,
but retained files and excluded directories block replacement. Run `status` first — it lists exactly what would be deleted.

If a remote-only file keeps getting deleted that shouldn't be, the fix is an
exclude entry, not avoiding `--delete`.

## When something fails

| Message | What it means |
| --- | --- |
| `no devsync config at ...` | Not in a project root, or not set up. `cd` to the root, or set it up (see *Installing devsync*). |
| `unknown field ...` (when loading the config) | A misspelled key or section in the config, at the line shown. Fix the spelling. |
| `commands.<name> is not defined; defined: ...` | The name is missing from `[commands]`; the message lists what exists. Add it or fix the typo. Caught locally, before connecting. |
| `unsupported protocol version: ...` | The two machines run different builds. N (or "local devsync speaks") is the local side, M (or "remote agent speaks") the remote. Upgrade the **older** side to match the newer one; never downgrade a remote that other projects may use. Newer agents name the older side in the message. |
| `unexpected handshake response (remote agent may be an incompatible older version)` | The remote agent predates the handshake. Upgrade the remote. |
| `ssh could not connect to ...` | The network or ssh side: host, port, key, or sshd. ssh's own error is printed just above; try the suggested `echo ok` command. |
| `ssh connected, but the remote agent did not answer ...` | ssh works, but `agent_path` does not start devsync, or the login shell printed something (PowerShell banner, `.bashrc` output). On Linux this usually means `agent_path` is unset. Run with `-v` and try the printed ssh command by hand. |
| `the remote agent closed the connection ...` | The agent stopped mid-session; its own error, if any, is printed just above. |
| `cannot write <path> on the remote: is it open in a running program ...` | A program on the remote (often the app from a previous `run`, or an IDE) holds the file. Stop it — through a configured command if there is one — and re-run. Other files in the batch were still written. |
| `remote_dir ... does not exist on the remote; run devsync sync first` | `--no-sync` against a never-synced remote. Drop `--no-sync`. |
| `client disconnected during exec; command stopped` | The local side went away (Ctrl+C, network, sleep) and the remote command was killed. Re-run it. |
| `cannot rename A -> B: B already exists` | Linux remote only: a stale `B` with no synced files (empty, or excluded output only) sits beside `A`, and the case-only rename cannot proceed. Ask the user before removing `B` on the remote. |
| Non-ASCII output is mangled | Either a BOM-less `.ps1` with non-ASCII text on a Windows remote (see *Script files* above), or an old remote agent. |
| `./build.sh: not found`, `$'\r': command not found`, `set: Illegal option -` | Linux remote, CRLF script. Fix line endings (`.gitattributes`: `*.sh text eol=lf`), not the command. |
| `Permission denied` running a script on Linux | The file has no `#!` line, so it was not marked executable. Add one, or run it as `sh ./script.sh`. |
| A file you edited did not upload | Check whether an exclude pattern matches it — `status` shows what is in scope. |

For a suspected bad hash cache, delete `.devsync/state` on the affected side to
force a full rehash. It is only a cache; removing it costs time, never
correctness. Never delete the whole local `.devsync/`: it also holds
`config.toml`, which is not in git and cannot be recovered.

## Verifying the remote actually changed

Diffing is content-hash based, so a second `status` right after a sync should
report `upload: 0`. If it still lists files, something is rewriting them — a
build step writing into a non-excluded source directory, for instance.

## When this does not apply

A project with neither `.devsync/config.toml` nor legacy `devsync.toml` is not
set up for remote builds yet. If the user
wants it built or run on another machine, set it up — see *Installing devsync*
above, starting with the remote catalog. If they did not ask for that, build locally as usual; devsync is not
something to introduce uninvited.

devsync runs from Windows to a Windows or Linux remote and syncs one way only, so it is not
the tool for fetching artifacts or logs back from the remote — use `scp` for
that.
