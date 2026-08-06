# devsync

`devsync` keeps a local Windows project as the source of truth and syncs it to a remote Windows execution copy through an SSH-launched stdio agent.

## Installation

The same executable is both the local client and the remote agent, so it has to
be installed on **both** machines.

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

### 4. Configure the project

Copy `devsync.toml.example` to `devsync.toml` in your project root and set
`connection.host`, `connection.user`, and `paths.remote_dir`.

`connection.agent_path` can be left out: it defaults to `devsync.exe`, which the
remote resolves through `PATH`. Set it only if the agent lives somewhere off
`PATH`. Then:

```bash
devsync status
```

This transfers nothing; it prints what a sync *would* upload and delete. Once it
looks right, run `devsync sync`.

`docs/manual-test.md` has a fuller checklist for verifying an installation,
including the failure modes that are easy to miss.

## Commands

```text
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

> **Breaking change:** `devsync run` previously executed without syncing. It now
> syncs first like every other execution command; use `devsync run --no-sync`
> for the old behavior.

Command names in `[commands]` are arbitrary. A name that matches a devsync
subcommand is fine: `devsync exec sync` runs `commands.sync`, not `devsync sync`.

Commands run with `remote_dir` as the working directory, and their output
streams back as it is produced. The remote command's own exit code becomes
devsync's exit code.

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

Forced excludes (`devsync.toml`, `.git/`, `.devsync/`) are always protected. Plain
`devsync sync` (without `--delete`) never deletes anything; use `--delete` when you
want the remote copy to mirror the local, non-excluded file set exactly.
