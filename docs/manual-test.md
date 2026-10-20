# devsync Manual E2E Test

Automated tests drive the agent's stdin/stdout directly and never touch SSH.
Everything specific to the real transport — binary payload integrity through
`ssh.exe`, remote shell banner pollution, filesystem encoding on the remote —
can only be verified here.

Last executed: **2026-08-06**, between two real Windows machines over OpenSSH.
Results are recorded at the bottom.

## Phase 0: Remote prerequisites

Install and start the OpenSSH server on the remote machine:

```powershell
Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0
Start-Service sshd
Set-Service -Name sshd -StartupType Automatic
```

**Check the default shell.** The agent writes protocol frames — and nothing else —
to stdout. If the remote login shell is PowerShell, a profile banner or any
startup output is prepended to the stream and corrupts the 4-byte frame length,
failing the handshake immediately. Leave the default (`cmd.exe`):

```powershell
Get-ItemProperty "HKLM:\SOFTWARE\OpenSSH" -Name DefaultShell -ErrorAction SilentlyContinue
```

No output means no override is set, which is what you want.

## Phase 1: Connection sanity

From the local machine:

```bash
ssh <host> "echo ok"
```

This must print exactly `ok` with no password prompt and no banner. **A single
stray character here will break the protocol** — fix it before continuing.

## Phase 2: Deploy the agent

Local and remote must run the same build: `PROTOCOL_VERSION` is checked during
the handshake and a mismatch is rejected by design.

Follow the README's installation steps, which put the agent in
`%LOCALAPPDATA%\Programs\devsync\` and on the user `PATH`. Then confirm the
remote resolves it in a non-interactive session — this is what devsync itself
gets, and it is not the same environment as an interactive login:

```bash
ssh <host> "where devsync.exe"
ssh <host> "devsync.exe --help"
```

With the agent on `PATH`, `connection.agent_path` can be omitted from
`.devsync/config.toml`. Verifying that omission works is part of the check: it is the
configuration the README tells users to write.

## Phase 3: Test project

Create a local project. The contents matter — each item exists to exercise a
specific failure mode:

```text
.devsync/config.toml created by devsync init, edited for your hosts
src/hello.txt         plain text, for the incremental-diff check
src/テスト.txt        non-ASCII filename, for the path encoding check
assets/blob.bin       binary, for the payload integrity check (see below)
build.ps1 run.ps1 test.ps1 lint.ps1
.git/                 run `git init` — verifies the forced exclude
```

Generate `assets/blob.bin` so it contains every byte value, including `0x00`,
`0x0A`, `0x0D`, and `0x1A`:

```powershell
$b = New-Object byte[] 262144
for ($i = 0; $i -lt $b.Length; $i++) { $b[$i] = $i % 256 }
[IO.File]::WriteAllBytes("assets\blob.bin", $b)
(Get-FileHash "assets\blob.bin" -Algorithm SHA256).Hash
```

Make `run.ps1` take a few seconds (`1..5 | ForEach-Object { Write-Host "tick $_"; Start-Sleep 1 }`)
so the output-buffering behavior below is observable.

## Phase 4: Core checks

1. `devsync status` — every file listed as upload.
2. Confirm `.devsync/config.toml`, legacy `devsync.toml`, and `.git` are **not** listed.
3. `devsync sync` — uploads them.
4. `ssh <host> "dir /s /b <remote_dir>"` — source files present, config and `.git` absent; `.devsync/state` is a remote-local cache.
5. `devsync status` again — `upload: 0`, everything `skipped`. This also proves
   the non-ASCII filename round-tripped: a corrupted name would not match the
   remote manifest and would reappear as an upload.
6. Edit `src/hello.txt`, run `devsync status` — exactly one file listed.
7. `devsync build` — syncs, then runs remotely; output shows `cwd` = `remote_dir`.
8. `devsync test`, `devsync exec lint` — an arbitrary configured name works.
9. `devsync exec nosuchname` — rejected locally, before any connection.
10. `devsync run --no-sync` — no `uploaded:` line; executes against the current copy.

**Check output timing.** Output streams as the command produces it. With a
`run.ps1` that prints a line per second, the lines must appear about a second
apart, not all at once when the command exits. Timestamping each line makes this
unambiguous:

```bash
devsync run 2>&1 | while IFS= read -r line; do echo "$(date +%H:%M:%S.%3N)  $line"; done
```

Output arriving in one burst at the end means the agent is buffering — a
regression of the streaming path in [src/agent.rs](../src/agent.rs).

## Phase 5: Checks the core list does not cover

These target the failure modes that only appear on real hardware.

**1. Binary payload integrity (highest priority).** The protocol writes raw bytes
after the JSON frame, straight through `ssh.exe`'s pipes. Text files cannot
detect corruption here. Compare hashes on both sides:

```bash
ssh <host> "certutil -hashfile <remote_dir>\assets\blob.bin SHA256"
```

It must equal the local SHA256 from Phase 3. A mismatch makes the tool unusable
regardless of what else works.

**2. `--delete` must not destroy remote build output.** Create excluded
directories remotely and a stale non-excluded file:

```bash
ssh <host> "mkdir <remote_dir>\bin <remote_dir>\artifacts & echo x > <remote_dir>\bin\app.exe & echo x > <remote_dir>\artifacts\out.log & echo x > <remote_dir>\stale.txt"
```

`devsync status` must list only `stale.txt` as a delete. After
`devsync sync --delete`, `bin\app.exe` and `artifacts\out.log` must survive.

**3. Exit code propagation.** Add a command running a script that ends with
`exit 3`. `devsync exec <name>; echo $?` must report **3**, not 1. PowerShell's
`-Command` collapses native exit codes to 1, so the agent appends an explicit
`exit $LASTEXITCODE`; this check guards that.

**4. Error handling.** Each must fail fast with a readable message, never hang:

- `connection.agent_path` pointing at a nonexistent file
- `connection.host` that does not resolve

Both should name `agent_path` and the ssh connection as things to check.

**5. Protocol version mismatch.** Build a local binary with `PROTOCOL_VERSION`
bumped and run it against the deployed agent. Expect
`unsupported protocol version: local devsync speaks N, remote agent speaks M; upgrade the ... side`. Revert afterwards.

**6. Hash cache.** After a `sync`, both `<local_dir>/.devsync/state` and
`<remote_dir>/.devsync/state` must exist. Then check all three of these:

- `devsync status` reports `upload: 0` / `delete: 0` — the cache files must not
  show up as files to sync or delete.
- `devsync sync --delete` leaves `<remote_dir>/.devsync/state` in place.
- Editing a file after the cache is warm still shows it as an upload. A cache
  that hides an edit is the one failure mode that loses work silently.

Deleting `.devsync/` forces a full rehash; that is the escape hatch if a cache
is ever suspected of being wrong.

**7. Non-ASCII command output.** Add a script that prints text in your language
and run it through `devsync exec`. The text must arrive intact — replacement
characters (`?`) mean the console code page was misdecoded.

Save the script as **UTF-8 with BOM**. PowerShell 5.1 reads a BOM-less `.ps1`
as ANSI, so a UTF-8 file without one is garbled at parse time, before devsync is
involved. Getting this wrong produces output that looks *almost* right and hides
whether the transport is actually correct.

## Linux remote (Docker)

[docker/linux-remote](../docker/linux-remote/Dockerfile) stands in for a Linux
machine. Its sshd is real, so disconnect behavior matches a real host; keep
`remote_dir` inside the container, since a bind-mounted Windows folder would
bring NTFS case and permission rules back.

```powershell
docker build -f docker/linux-remote/Dockerfile -t devsync-linux-remote .
ssh-keygen -t ed25519 -N '""' -f $HOME\.ssh\devsync-linux
docker run -d --init --name devsync-linux -p 2222:22 -e AUTHORIZED_KEY="$(Get-Content $HOME\.ssh\devsync-linux.pub)" devsync-linux-remote
```

Add a host alias to `~/.ssh/config` (devsync passes only `-p` and `user@host`):

```text
Host devsync-linux
  HostName localhost
  IdentityFile ~/.ssh/devsync-linux
  StrictHostKeyChecking no
  UserKnownHostsFile NUL
```

and point a test project at it with `host = "devsync-linux"`, `user = "dev"`,
`port = 2222`, `agent_path = "devsync"`, `remote_dir = "/home/dev/proj"`.
Rebuild the image after every agent change; the local binary must match.

| Check | Expected |
| --- | --- |
| `status`, `sync` | Same as on Windows. |
| `exec` of `echo before; exit 7`, a missing command, `printf 'こんにちは\n'` | Exit 7, exit 127, Japanese text intact. |
| `./build.sh` with an LF `#!/bin/sh` script | Runs; `ls -l` shows `rwxr-xr-x`. |
| Same script with CRLF | `./build.sh: not found` (exit 127). |
| `exec` of `echo start; sleep 300`, then Ctrl+C | `docker exec devsync-linux ps -u dev` is empty within seconds. |
| Rename local `Src/Foo.txt` to `src/foo.txt`, then `status` and `sync` | `~ Src -> src` and `~ src/Foo.txt -> src/foo.txt`, `uploaded: 0`; remote holds `src/foo.txt` only. |
| Create a remote `src/FOO.txt` beside it, then `sync --delete` | `deleted: 1`; a second `status` plans nothing. |

All of these passed on 2026-10-20.

## 2026-08-06 results

All Phase 4 and Phase 5 checks passed, including binary integrity (SHA256 match
on 256 KB covering all byte values), forced excludes, non-ASCII filenames,
`--delete` exclusion protection, and version-mismatch rejection.

Two defects were found and fixed:

- **Exit codes collapsed to 1.** A script exiting 3 was reported as 1, and the
  same applied to any native command (`cargo build` failing with 101 would also
  report 1). Fixed by appending `exit $LASTEXITCODE` in
  [src/agent.rs](../src/agent.rs); regression test
  `e2e_exec_propagates_exact_nonzero_exit_code`.
- **Unhelpful handshake failure.** A missing remote agent and an unresolvable
  host both produced only `failed to fill whole buffer`. Fixed in
  [src/client.rs](../src/client.rs); regression test
  `perform_handshake_reports_actionable_error_when_agent_never_replies`.

Output buffering was confirmed as a third limitation and then fixed in the same
session: the agent now spawns the command and streams `Output` frames as they
are produced. Re-verified on the same hardware — a script printing once a second
arrives a line per second instead of in one burst after 6 seconds.

A fourth defect surfaced from that work. The remote reports
`[Console]::OutputEncoding` as `shift_jis / cp932`, while the agent decoded
output as UTF-8, so Japanese build errors arrived as solid U+FFFD — unreadable
and unrecoverable. Every check up to this point had used ASCII-only scripts and
missed it entirely. Fixed by decoding in the console code page; verified with
Japanese text surviving the round trip intact.

## 2026-10-20 results (Windows remote, protocol 4)

Re-run against the same kind of real Windows remote after the Linux work, with
the new agent installed beside the old one (`agent_path` pointing at it) so
existing projects kept working. Everything passed:

- `status` / `sync` into a fresh `remote_dir`.
- `exec`: exit 7 from `cmd /c exit 7`, exit 1 from a failing `Get-Item`,
  Japanese output intact, and a 4-second silent command finishing normally
  while `Keepalive` frames arrived.
- Case-only rename `Src/Foo.txt` → `src/foo.txt`: `status` listed both renames,
  `sync --delete` renamed without uploading or deleting, the remote showed the
  new spelling, and the next `status` was clean. A rename combined with a
  content change renamed and uploaded in one sync.
- Ctrl+C (local `devsync.exe` and `ssh.exe` killed) during a silent
  `ping -n 300`: the remote agent, PowerShell, and `PING.EXE` were gone within
  6 seconds; unrelated PowerShell processes were untouched.
- An old (protocol 3) client against the new agent:
  `unsupported protocol version: 3 (agent supports 4)`.
