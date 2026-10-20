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
`target
elease\devsync.exe` into `%LOCALAPPDATA%\Programs\devsync\`, and put
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
(`%USERPROFILE%\.devsync
emotes.toml`). It is an optional, user-maintained
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
- **Chosen entry:** use its values for `init` below. `remote_dir` is
  `remote_root` joined with the project's directory name, unless the user
  wants otherwise. Add `# remote: <name>` as the first line of the resulting
  config so the source machine is recorded. Copy `agent_path` when present.
- **Make sure `remote_dir` is not another project's copy.** Two projects with
  the same directory name land in the same folder, and `sync --delete` would
  then wipe the other one. Run `devsync status` first: a long list of planned
  deletes for a new project means the folder is taken, so pick another name.
- **Before trusting an entry**, run the `echo ok` check and compare versions;
  the catalog is a hint, not proof the machine is reachable.
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
devsync init --host <host> --user <user> --remote-dir "C:\work\project" --install-skill --skill-target codex
```

### Linux remote

**1. Any OpenSSH server.** Shell startup files (`~/.bashrc` and friends) must
print nothing for a non-interactive session, or the `echo ok` check above fails
for the same reason a PowerShell banner does.

**2. Build the agent on the remote**, from the same version as the local build,
and install it:

```bash
cargo build --release
install -D target/release/devsync ~/.local/bin/devsync
```

Confirm from the local machine: `ssh <host> "~/.local/bin/devsync --version"`
matches `devsync --version`.

**3. Configure the project**, then fix two things `init` writes for Windows:

```bash
devsync init --host <host> --user <user> --remote-dir "/home/<user>/work/project" --install-skill --skill-target codex
```

- Add `agent_path = "/home/<user>/.local/bin/devsync"` under `[connection]`. It
  is required: sshd's non-interactive shell often lacks `~/.local/bin` on
  `PATH`, and the default is the Windows name `devsync.exe`.
- Replace the PowerShell examples in `[commands]` with `sh` command lines.

### The skill and the first sync

Choose `--skill-target codex` for Codex, `claude` for Claude Code, or `both`
when the user uses both. Omitting `--skill-target` preserves the original Claude
Code default. Install only the requested host(s). The same embedded `SKILL.md`
is installed to `~/.agents/skills/devsync/` for Codex and
`~/.claude/skills/devsync/` for Claude Code. No per-project skill copy is needed.

`init` writes `.devsync/config.toml`, adds `.devsync/` to `.gitignore` when the
directory is a git repository, and installs this skill. Fill in `[commands]`,
then run `devsync status` — it transfers nothing and shows what a sync would
do — before the first `devsync sync`.

**`--install-skill` overwrites this file, and finishing setup with it is the
point.** Refresh Codex with `devsync init --install-skill --skill-target codex`
(or use `both` for both hosts). An existing config is preserved unless `--force`
is supplied. In Codex the user can explicitly invoke `$devsync`; if discovery
does not refresh, restart Codex. The skill embedded in the binary is the only copy guaranteed to match
the binary. A copy placed by hand — out of a git checkout, so this guidance
exists before devsync does — is a bootstrap, and the binary's copy supersedes it.

## What differs between a Windows and a Linux remote

The remote OS is the agent's OS; nothing in the config names it. A POSIX
`remote_dir` (`/home/dev/project`) means a Linux remote.

| | Windows remote | Linux remote |
| --- | --- | --- |
| ssh login shell | Keep `cmd.exe`; a PowerShell profile banner breaks the handshake. | Startup files must print nothing for non-interactive sessions. |
| `agent_path` | Optional; `devsync.exe` is found on `PATH`. | Required, as an absolute path. |
| Command shell | PowerShell `-Command`. A failing cmdlet or command lookup still exits nonzero. | `sh -c`; the exit status passes through unchanged. |
| Script files | PowerShell 5.1 reads a BOM-less `.ps1` as ANSI, so save scripts with non-ASCII text as UTF-8 *with* BOM. | Bytes are copied verbatim; a CRLF script fails. Keep it LF via `.gitattributes` (`*.sh text eol=lf`). |
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
build = "powershell -NoProfile -ExecutionPolicy Bypass -File .\\build.ps1"
test  = "cargo test"

[sync]
exclude = ["target", "node_modules", "bin", "obj"]
```

Commands run with `remote_dir` as the working directory. Paths in a command are
relative to it, not to anything local.

## Commands

```bash
devsync status                  # list planned uploads/deletes; may refresh hash caches
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
more files than expected.

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
| `commands.<name> is not defined` | The name is missing from `[commands]`. Add it. Caught locally, before connecting. |
| `unsupported protocol version: N (agent supports M)` | The two machines are running different builds. Reinstall the remote binary from the same build as the local one. |
| `no response from the remote agent: ...` | ssh could not reach the host, or the agent is not where the config expects. Check `ssh <user>@<host> "echo ok"` first; ssh's own error usually prints just above. |
| Non-ASCII output is mangled | The remote agent is likely an old build. Reinstall it. |
| `./build.sh: not found`, `$'
': command not found`, `set: Illegal option -` | Linux remote, CRLF script. Fix line endings (`.gitattributes`: `*.sh text eol=lf`), not the command. |
| `Permission denied` running a script on Linux | The file has no `#!` line, so it was not marked executable. Add one, or run it as `sh ./script.sh`. |
| A file you edited did not upload | Check whether an exclude pattern matches it — `status` shows what is in scope. |

For a suspected bad hash cache, delete `.devsync/` on the affected side to force
a full rehash. It is only a cache; removing it costs time, never correctness.

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
