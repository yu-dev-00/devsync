# devsync

`devsync` keeps a local Windows project as the source of truth and syncs it to a remote Windows or Linux execution copy through an SSH-launched stdio agent.

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
with the copy embedded in the binary you are about to install — and since both
come from the same version, there is nothing to reconcile.

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
password, create a key (once) and append it to `authorized_keys` on the remote.
Use `scp`, not a PowerShell pipe: PowerShell 5.1 re-encodes piped text and the
key line arrives corrupted.

```powershell
ssh-keygen -t ed25519
scp $env:USERPROFILE\.ssh\id_ed25519.pub <user>@<host>:devsync-key.pub
ssh <user>@<host> "if not exist .ssh mkdir .ssh & type devsync-key.pub >> .ssh\authorized_keys & del devsync-key.pub"
```

For an account in the remote's Administrators group, sshd reads
`C:\ProgramData\ssh\administrators_authorized_keys` instead, and ignores it
unless its ACL is restricted. As administrator on the remote:

```powershell
icacls C:\ProgramData\ssh\administrators_authorized_keys /inheritance:r /grant "Administrators:F" /grant "SYSTEM:F"
```

devsync runs `ssh -p <port> <user>@<host>` and nothing else, so a non-default
key goes in an `~/.ssh/config` `Host` entry (with `IdentityFile`), and a
non-default port in the project config's `port`.

### 3. Download (or build) and install

Download `devsync.exe` from the
[latest release](https://github.com/yu-dev-00/devsync/releases/latest) and check
it against the `devsync.exe.sha256` published beside it:

```powershell
(Get-FileHash devsync.exe -Algorithm SHA256).Hash
```

The binary is unsigned, so SmartScreen may warn on first run. To build it
yourself instead, run `cargo build --release` and use
`target\release\devsync.exe`.

Install `devsync.exe` into `%LOCALAPPDATA%\Programs\devsync\` on
**both** machines, and add that directory to each machine's user `PATH`. This is
the standard per-user location on Windows, so no administrator rights are
needed. To put the same file on the remote from the local machine:

```powershell
ssh <user>@<host> "mkdir %LOCALAPPDATA%\Programs\devsync"
scp devsync.exe <user>@<host>:AppData/Local/Programs/devsync/devsync.exe
```

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
ssh <user>@<host> "where devsync.exe"
```

**Both sides must run the same build.** devsync refuses to connect when the two
speak different protocol versions, so after upgrading one machine, upgrade the
other too; the error says which side is older. Compare `devsync --version` with
`ssh <user>@<host> "devsync.exe --version"` to check up front.

### 4. Set up a project

From the project root:

```powershell
devsync init --host <host> --user <user> --remote-dir "C:\work\project"
```

This writes `.devsync/config.toml` and adds `.devsync/` to `.gitignore` when the
directory is a git repository. It refuses to overwrite an existing config unless
you pass `--force`. The three connection flags are optional — omit them and edit
the generated file instead. Add `--install-skill` to also install the AI agent
skill (see below).

**Give each project its own `remote_dir`.** devsync creates the folder; pick one
that does not exist yet. Two projects sharing a folder means `sync --delete` for
one deletes the other's files. For a new project, `devsync status` must plan no
deletes — even one means the folder is already someone else's.

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

The generated `[commands]` and `[sync].exclude` are placeholders. Replace the
commands with the project's real ones (`build = "dotnet build"`,
`test = "cargo test"`, `build = ".\\build.ps1"`, …) and add to `exclude` whatever
the toolchain generates or the remote creates (`.venv`, `__pycache__`, `target`,
datasets, model weights) — see [`sync --delete` and excludes](#sync---delete-and-excludes)
for the pattern rules. Then:

```powershell
devsync status   # transfers nothing; lists what a sync would rename, upload, and delete
devsync sync
devsync build    # syncs, runs commands.build in remote_dir, exits with its exit code
```

### The Codex / Claude Code skill

Choose the host when initializing a project:

```powershell
devsync init --host <host> --user <user> --remote-dir "C:\work\project" --install-skill --skill-target <claude|codex|both>
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

Re-run `devsync init --install-skill --skill-target <same target>` after
upgrading to refresh the skill. That is safe
in a project that is already set up: with `--install-skill`, an existing
config is reported and left alone rather than treated as an error, so
refreshing the skill never costs you your connection details or `[commands]`.

`docs/manual-test.md` has a fuller checklist for verifying an installation,
including the failure modes that are easy to miss.

### Linux remote

The local side stays Windows and is installed exactly as in step 3. Only the
remote half of steps 1–3 changes:

1. Any OpenSSH server works. As on Windows, `ssh <user>@<host> "echo ok"` must
   print exactly `ok`: a shell startup file that prints anything corrupts the
   protocol stream.
2. On the remote, install `devsync-x86_64-linux` from the same release as the
   local `devsync.exe` (statically linked, so any x86_64 distribution works):

   ```bash
   curl -fLO https://github.com/yu-dev-00/devsync/releases/download/v<version>/devsync-x86_64-linux
   curl -fLO https://github.com/yu-dev-00/devsync/releases/download/v<version>/devsync-x86_64-linux.sha256
   sha256sum -c devsync-x86_64-linux.sha256
   install -D -m 755 devsync-x86_64-linux ~/.local/bin/devsync
   ~/.local/bin/devsync --version   # must match the local devsync --version
   ```

   For another architecture, or to build it yourself: `git clone --branch
   v<version> https://github.com/yu-dev-00/devsync.git`, `cargo build --release`,
   and install `target/release/devsync` the same way.
3. Set `agent_path` to that absolute path. sshd runs the agent from a
   non-interactive shell, which often does not put `~/.local/bin` on `PATH`, and
   the default (`devsync.exe`) is the Windows name anyway. `init` cannot set it,
   so add it to the generated config, and replace the placeholder PowerShell
   `[commands]` with `sh` command lines.

Run `init` from PowerShell. In Git Bash, `--remote-dir /home/...` is rewritten
to `C:/Program Files/Git/home/...` before devsync sees it, unless you prefix the
command with `MSYS_NO_PATHCONV=1`.

```toml
[connection]
host = "linux-box"
user = "dev"
agent_path = "/home/dev/.local/bin/devsync"

[paths]
local_dir = "."
remote_dir = "/home/dev/work/project"

[commands]
build = "./build.sh"
test = "cargo test"
```

What differs from a Windows remote:

- Commands run under `sh -c` in `remote_dir`, and their exit status is passed
  through unchanged.
- An uploaded file that starts with `#!` is made executable, because Windows has
  no executable bit to send. Other permissions are not synchronized.
- Files are copied byte for byte, so a CRLF checkout breaks shell scripts
  (`./build.sh: not found` from a `#!/bin/sh\r` line, or
  `set: Illegal option -`). Keep remotely run scripts LF in git with
  `.gitattributes`, for example `*.sh text eol=lf` and `gradlew text eol=lf`.
- Ctrl+C stops the remote command and everything it started. A command that
  calls `setsid` itself escapes this.
- The local spelling of a name wins: a case-only rename is applied remotely as a
  rename, and on a case-sensitive remote a stale second spelling is removed by
  `sync --delete`.

`docker/linux-remote/Dockerfile` builds a disposable Linux remote for trying
this out; `docs/manual-test.md` walks through it.

### Remote catalog (optional)

Keep the machines you build on in `~/.devsync/remotes.toml`. devsync does not
read this file; it is where you, or an AI agent using the skill, look when a
project has no config yet, so a new project starts from a known machine instead
of from memory:

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

`os`, `remote_root` (the parent of project copies), and `note` are what ssh's
own config cannot hold. Setting up a project copies the values into its
`.devsync/config.toml`, with a `# remote: <name>` comment recording the source;
later catalog edits do not change existing projects. Do not put passwords or
keys in it.

A short root such as `C:\work` keeps build paths well under Windows' 260-character
limit and is easy to exempt from antivirus scanning. Folders created directly
under `C:\` let other local users modify them by default, so on a shared machine
restrict its ACL or use a folder under your profile instead.

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
[devsync] handshake: agent acknowledged version 4
[devsync] plan: 0 rename, 1 upload, 0 delete, 9 skip
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
devsync's exit code. On a Windows remote each command runs as
`powershell -NoProfile -ExecutionPolicy Bypass -Command <command>` (Windows
PowerShell 5.1, so no `&&`), which means a script path such as `.\build.ps1` is
a complete command; a failing cmdlet or command lookup returns a nonzero code
even when no native program supplied one. On a Linux remote, commands run under
`sh -c`.

See `devsync.toml.example` for the full set of configuration options.

## Hash cache

Both sides record file hashes in `<root>/.devsync/state` and reuse them for
files whose size and modification time are unchanged since the last run, so an
unchanged tree is not re-read on every command. Uploads are still decided by
comparing content hashes, never timestamps.

`.devsync/` is a forced exclude: the cache is never uploaded and never deleted
by `sync --delete`. Delete `.devsync/state` to force a full rehash — not the
whole `.devsync/` directory, which locally also holds `config.toml`.

## `sync --delete` and excludes

`devsync sync --delete` deletes any file under `remote_dir` that is not part of the
local manifest. Your `[sync].exclude` patterns are applied on **both** sides: the
local upload scan and the remote agent's manifest. Excluded paths such as `bin`,
`obj`, `build`, `dist`, and `artifacts` are therefore invisible to the diff on the
remote side too, so `--delete` will **not** remove them — build outputs produced on
the remote machine are preserved.

Each exclude entry is either a file/directory name, matched at any depth (so
`build` also excludes `src/build/`), or a relative path prefix such as
`data/raw`. Matching ignores case. Wildcards are **not** supported: `*.log`
matches nothing. `status` shows exactly what is in scope, so check it after
changing excludes. `status` also lists remote-only files under
`delete (only with sync --delete)`; plain `sync` keeps them.

Path comparisons and exclude matching ignore case using Windows ordinal rules,
so `DEVSYNC.TOML` is also excluded and a case-only rename cannot cause the
uploaded file to be deleted. The local spelling wins: a remote file or
directory spelled differently is renamed in place (`status` lists these as
`~ old -> new`) rather than uploaded again.

Forced excludes (`devsync.toml`, `.git/`, `.devsync/`) are always protected. Plain
`devsync sync` (without `--delete`) never deletes existing files; use `--delete` when you
want the remote copy to mirror the local, non-excluded file set exactly.

When a file becomes a directory or a directory becomes a file, `--delete`
removes the conflicting tracked files before uploading the replacement. Empty
directories at the replacement path are removed as needed. Excluded files and
directories are preserved; if they block a replacement, sync fails.

## License

[MIT](LICENSE)
