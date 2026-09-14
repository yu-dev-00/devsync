---
name: devsync
description: Build, run, and test Windows projects remotely over SSH using devsync. Use for projects with .devsync/config.toml or legacy devsync.toml, for requested remote build setup, or for troubleshooting devsync sync and execution failures. Supports Codex and Claude Code.
---

# devsync

devsync keeps the local project as the source of truth and mirrors it to a
remote Windows machine that does the building and running. You edit locally; the
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

The same executable is both the local client and the remote agent, so it goes on
**both** machines, and both must run the same build — the handshake compares
protocol versions and refuses a mismatch.

**1. Remote: OpenSSH server, login shell left alone.** As administrator on the
remote machine:

```powershell
Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0
Start-Service sshd
Set-Service -Name sshd -StartupType Automatic
```

Leave the default shell as `cmd.exe`. The agent writes protocol frames and
nothing else to stdout, so a PowerShell profile banner prepended to the stream
corrupts the 4-byte frame length and surfaces as a handshake failure. No output
from this means nothing is overridden:

```powershell
Get-ItemProperty "HKLM:\SOFTWARE\OpenSSH" -Name DefaultShell -ErrorAction SilentlyContinue
```

**2. Key authentication, then prove it.** This must print exactly `ok` — no
password prompt, no banner, not one stray character:

```bash
ssh <user>@<host> "echo ok"
```

**3. Build, and install on both machines.**

```bash
cargo build --release
```

Copy `target\release\devsync.exe` into `%LOCALAPPDATA%\Programs\devsync\` on each
machine and put that directory on the user `PATH`. Do not reach for
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

In a new session, confirm the remote resolves it:

```bash
ssh <host> "where devsync.exe"
```

**4. Configure the project.** From the project root:

```bash
devsync init --host <host> --user <user> --remote-dir "C:\work\project" --install-skill --skill-target codex
```

Choose `--skill-target codex` for Codex, `claude` for Claude Code, or `both`
when the user uses both. Omitting `--skill-target` preserves the original Claude
Code default. Install only the requested host(s). The same embedded `SKILL.md`
is installed to `~/.agents/skills/devsync/` for Codex and
`~/.claude/skills/devsync/` for Claude Code. No per-project skill copy is needed.

That writes `.devsync/config.toml`, adds `.devsync/` to `.gitignore` when the directory
is a git repository, and installs this skill. Fill in `[commands]`, then run
`devsync status` — it transfers nothing and shows what a sync would do — before
the first `devsync sync`.

**`--install-skill` overwrites this file, and finishing setup with it is the
point.** Refresh Codex with `devsync init --install-skill --skill-target codex`
(or use `both` for both hosts). An existing config is preserved unless `--force`
is supplied. In Codex the user can explicitly invoke `$devsync`; if discovery
does not refresh, restart Codex. The skill embedded in the binary is the only copy guaranteed to match
the binary. A copy placed by hand — out of a git checkout, so this guidance
exists before devsync does — is a bootstrap, and the binary's copy supersedes it.

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
# agent_path is optional; it defaults to devsync.exe, found via the remote PATH

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
above. If they did not ask for that, build locally as usual; devsync is not
something to introduce uninvited.

devsync targets Windows on both ends and syncs one way only, so it is not
the tool for fetching artifacts or logs back from the remote — use `scp` for
that.
