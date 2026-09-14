# devsync

`devsync` keeps a local Windows project as the source of truth and syncs it to a remote Windows execution copy through an SSH-launched stdio agent.

## Installation

Codex and Claude Code can both use devsync. The CLI embeds one shared skill;
choose its installation target with `--skill-target codex`, `claude`, or `both`.

The same executable is both the local client and the remote agent, so it has to
be installed on **both** machines.

### 0. Optional: hand Claude Code the skill first

The fiddly parts below — the login shell that must stay `cmd.exe`, the `PATH`
edit that `setx` gets wrong, keeping both machines on the same build — are
exactly what the bundled skill knows. It is normally installed by step 4, which
is after you have already done them by hand. Copy it out of this checkout first
and Claude Code can drive the rest of this section instead:

```powershell
$dst = "$env:USERPROFILE\.claude\skills\devsync"
New-Item -ItemType Directory -Force $dst | Out-Null
Copy-Item skills\devsync\SKILL.md $dst
```

This copy is a bootstrap, not a second distribution channel. Step 4 replaces it
with the copy embedded in the binary you are about to build — and since both
come from this same checkout, there is nothing to reconcile.

### 1. Remote: enable OpenSSH

On the remote Windows machine, as administrator:

```powershell
Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0
Start-Service sshd
Set-Service -Name sshd -StartupType Automatic
```

Leave the default login shell alone. The agent writes protocol frames — and
nothing else — to stdout, so if the shell is switched to PowerShell, its profile
banner is prepended to the stream and corrupts the frame length, which shows up
as a handshake failure. No output from this means nothing is overridden:

```powershell
Get-ItemProperty "HKLM:\SOFTWARE\OpenSSH" -Name DefaultShell -ErrorAction SilentlyContinue
```

### 2. Set up key authentication

From the local machine, confirm this prints exactly `ok`, with no password
prompt and no banner:

```bash
ssh <user>@<host> "echo ok"
```

A single stray character here will break the protocol. If you are prompted for a
password, copy your public key to `C:\Users\<user>\.ssh\authorized_keys` on the
remote. For an administrator account the file is
`C:\ProgramData\ssh\administrators_authorized_keys` instead, and its ACL must be
restricted or sshd ignores it.

### 3. Build and install

```bash
cargo build --release
```

Install `target\release\devsync.exe` into `%LOCALAPPDATA%\Programs\devsync\` on
**both** machines, and add that directory to each machine's user `PATH`. This is
the standard per-user location on Windows, so no administrator rights are
needed.

To add it to `PATH`, edit the user-scoped value directly rather than using
`setx PATH "%PATH%;..."` — that one expands to the *merged* system and user
`PATH`, writes the whole thing back into the user `PATH`, and silently truncates
it at 1024 characters:

```powershell
$dir = "$env:LOCALAPPDATA\Programs\devsync"
$user = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($user -notlike "*$dir*") {
    [Environment]::SetEnvironmentVariable('Path', "$user;$dir", 'User')
}
```

Open a new session and confirm the remote resolves it:

```bash
ssh <host> "where devsync.exe"
```

**Both sides must run the same build.** The handshake compares
`PROTOCOL_VERSION` and refuses to continue on a mismatch, so after upgrading,
reinstall the remote copy as well. The error names both versions when you
forget.

### 4. Set up a project

From the project root:

```bash
devsync init --host <host> --user <user> --remote-dir "C:\work\project" --install-skill
```

This writes `.devsync/config.toml`, adds `.devsync/` to `.gitignore` when the directory
is a git repository, and installs the Claude Code skill (see below). It refuses
to overwrite an existing config unless you pass `--force`. The three connection
flags are optional — omit them and edit the generated file instead.

`connection.agent_path` is left out of the generated config on purpose: it
defaults to `devsync.exe`, which the remote resolves through `PATH`. Set it only
if the agent lives somewhere off `PATH`.

The project layout is:

```text
.devsync/
  config.toml   # environment-specific connection, commands, and exclusions
  state         # machine-local hash cache
```

The whole `.devsync/` directory is excluded from Git and synchronization.
Run devsync from the project root. In `.devsync/config.toml`, relative
`local_dir` values are based on that root; `local_dir = "."` does not select
`.devsync/`. An absolute `local_dir` is used unchanged.

Existing projects with root `devsync.toml` keep working. The hidden config takes
priority if both exist; an invalid hidden config produces an error rather than
falling back. `init` also uses the existing legacy config when the hidden one
is absent, so refreshing a skill does not create a competing config. To migrate,
move the legacy config to `.devsync/config.toml` and add `.devsync/` to
`.gitignore`. Relative paths in legacy and other custom config files retain
working-directory-relative behavior.

Fill in `[commands]` with whatever this project needs building and running with,
then:

```bash
devsync status
```

This transfers nothing; it prints what a sync *would* upload and delete. Once it
looks right, run `devsync sync`.

### The Codex / Claude Code skill

Choose the host when initializing a project:

```powershell
devsync init --host <host> --user <user> --remote-dir "C:\work\project" --install-skill --skill-target codex
```

| Selection | User-wide installation directory |
| --- | --- |
| `--skill-target codex` | `~/.agents/skills/devsync/` |
| `--skill-target claude` | `~/.claude/skills/devsync/` |
| `--skill-target both` | Both directories |

`--skill-target` requires `--install-skill`. Omitting the target keeps the
existing Claude Code default. The shared skill teaches either agent to sync
current local code before executing remote builds. In Codex, invoke `$devsync`
explicitly or let the agent select it for a matching request. Restart Codex if
the newly installed skill does not appear.

The binary is the skill's source of truth: `include_str!` embeds it at build
time, so an installed skill cannot describe a flag its own `devsync.exe` does
not have. That is why step 0 is a copy out of the checkout you build from rather
than a separate download, and why it ends by being overwritten.

It installs once per user rather than per project, because it describes how
devsync works, not what any one project does. A per-project copy would go stale
as devsync changes and keep advising behavior that no longer exists.

Re-run `devsync init --install-skill --skill-target codex` after upgrading to
refresh the Codex skill; use `claude` or `both` for the other targets. That is safe
in a project that is already set up: with `--install-skill`, an existing
config is reported and left alone rather than treated as an error, so
refreshing the skill never costs you your connection details or `[commands]`.

`docs/manual-test.md` has a fuller checklist for verifying an installation,
including the failure modes that are easy to miss.

## Commands

```text
devsync init                 # scaffold .devsync/config.toml; --install-skill adds the skill
devsync status
devsync sync
devsync sync --delete
devsync exec <name>          # run any named command from [commands], syncing first
devsync exec <name> --no-sync  # skip sync and execute against the current remote copy
devsync build                # alias for: devsync exec build
devsync run                  # alias for: devsync exec run
devsync test                 # alias for: devsync exec test
```

`build`, `run`, and `test` are convenience aliases for `exec <name>`. All execution commands (`exec`, `build`, `run`, `test`) sync first by default. Pass `--no-sync` to skip the sync step and execute against the current remote copy.

`--config <path>` selects an explicit config path without fallback, and `-v` /
`--verbose` prints progress and protocol diagnostics. Both work on either side
of the subcommand, so `devsync build -v` is fine.

## Diagnosing with `--verbose`

Verbose output goes to **stderr**, tagged `[devsync]`, so it never mixes into a
remote build log you piped somewhere:

```text
[devsync] local manifest: 10 file(s) in 5.1968ms from .
[devsync] spawning: ssh -p 22 user@remote-pc "devsync.exe" agent --stdio
[devsync] handshake: agent acknowledged version 3
[devsync] plan: 1 upload, 0 delete, 9 skip
[devsync] uploading src/hello.txt (21 bytes)
[devsync] agent acked: wrote 1 file(s), deleted 0
```

Reach for the `spawning:` line first when a connection misbehaves — running that
exact command by hand separates an ssh problem from a devsync one. The manifest
timing tells a cold walk apart from one served by the hash cache, and the
`uploading` lines show which files the diff actually picked, which is how you
catch an exclude pattern that is too broad or too narrow.

> **Breaking change:** `devsync run` previously executed without syncing. It now
> syncs first like every other execution command; use `devsync run --no-sync`
> for the old behavior.

Command names in `[commands]` are arbitrary. A name that matches a devsync
subcommand is fine: `devsync exec sync` runs `commands.sync`, not `devsync sync`.

Commands run with `remote_dir` as the working directory, and their output
streams back as it is produced. The remote command's own exit code becomes
devsync's exit code. A failing PowerShell cmdlet or command lookup returns a
nonzero code even when no native program supplied an exit code.

See `devsync.toml.example` for the full set of configuration options.

## Hash cache

Both sides record file hashes in `<root>/.devsync/state` and reuse them for
files whose size and modification time are unchanged since the last run, so an
unchanged tree is not re-read on every command. Uploads are still decided by
comparing content hashes, never timestamps.

`.devsync/` is a forced exclude: the cache is never uploaded and never deleted
by `sync --delete`. Delete the directory to force a full rehash.

## `sync --delete` and excludes

`devsync sync --delete` deletes any file under `remote_dir` that is not part of the
local manifest. Your `[sync].exclude` patterns are applied on **both** sides: the
local upload scan and the remote agent's manifest. Excluded paths such as `bin`,
`obj`, `build`, `dist`, and `artifacts` are therefore invisible to the diff on the
remote side too, so `--delete` will **not** remove them — build outputs produced on
the remote machine are preserved.

Path comparisons and exclude matching ignore case using Windows ordinal rules,
so `DEVSYNC.TOML` is also excluded and a case-only rename cannot cause the
uploaded file to be deleted. Existing remote spelling may be retained.

Forced excludes (`devsync.toml`, `.git/`, `.devsync/`) are always protected. Plain
`devsync sync` (without `--delete`) never deletes existing files; use `--delete` when you
want the remote copy to mirror the local, non-excluded file set exactly.

When a file becomes a directory or a directory becomes a file, `--delete`
removes the conflicting tracked files before uploading the replacement. Empty
directories at the replacement path are removed as needed. Excluded files and
directories are preserved; if they block a replacement, sync fails.
