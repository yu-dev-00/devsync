# devsync Manual E2E Test

Automated tests drive the agent's stdin/stdout directly and never touch SSH.
Everything specific to the real transport — binary payload integrity through
`ssh.exe`, remote shell banner pollution, filesystem encoding on the remote —
can only be verified here.

Last executed: **2026-08-06** (local `sch-0066` → remote `pc-0141`). Results are
recorded at the bottom.

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

```bash
cargo build --release
scp target/release/devsync.exe '<host>:C:\tools\devsync.exe'
ssh <host> "C:\tools\devsync.exe --help"
```

## Phase 3: Test project

Create a local project. The contents matter — each item exists to exercise a
specific failure mode:

```text
devsync.toml          copied from devsync.toml.example, edited for your hosts
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
2. Confirm `devsync.toml` and `.git` are **not** listed.
3. `devsync sync` — uploads them.
4. `ssh <host> "dir /s /b <remote_dir>"` — files present, `devsync.toml` / `.git` / `.devsync` absent.
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
`unsupported protocol version: N (agent supports M)`. Revert afterwards.

**6. Non-ASCII command output.** Add a script that prints text in your language
and run it through `devsync exec`. The text must arrive intact — replacement
characters (`?`) mean the console code page was misdecoded.

Save the script as **UTF-8 with BOM**. PowerShell 5.1 reads a BOM-less `.ps1`
as ANSI, so a UTF-8 file without one is garbled at parse time, before devsync is
involved. Getting this wrong produces output that looks *almost* right and hides
whether the transport is actually correct.

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
