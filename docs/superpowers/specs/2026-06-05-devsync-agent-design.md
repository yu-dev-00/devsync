# devsync Agent Protocol Design

## 1. Purpose

`devsync` is a Windows-first development helper for keeping a local project as the source of truth while using a remote Windows machine as the build and execution copy.

The tool replaces the earlier SFTP-centered design with an SSH-transported agent protocol:

- Local code is edited only on the local machine.
- The remote directory is treated as a disposable execution copy.
- SSH is used for authentication, encryption, and process startup.
- File synchronization and command execution semantics are handled by `devsync` on both sides.

Initial scope is Windows local to Windows remote. Linux, POSIX shells, SFTP sync, daemon mode, and Git-style history management are outside the first implementation.

## 2. Architecture

The system has two roles in one executable:

```text
Local devsync.exe
  - reads devsync.toml
  - scans the local project
  - launches ssh.exe
  - starts the remote agent through SSH
  - compares manifests
  - sends changed files
  - requests named remote commands

Remote devsync.exe agent --stdio
  - communicates over stdin/stdout
  - receives runtime settings from local devsync
  - scans remote_dir
  - applies file changes under remote_dir
  - runs named commands in remote_dir
  - streams command output and exit status
```

The local process starts the remote agent with Windows OpenSSH:

```text
ssh -p <port> <user>@<host> "<agent_path> agent --stdio"
```

The remote agent does not read `devsync.toml`. The local configuration is the source of truth and is sent over the protocol after connection setup.

## 3. Commands

Initial commands:

```text
devsync status
```

Compare local and remote manifests and print planned uploads and deletes. It never transfers or deletes files.

```text
devsync sync
```

Synchronize local files to the remote copy. It uploads changed or missing files. It does not delete remote-only files by default.

```text
devsync sync --delete
```

Synchronize local files and delete remote-only files that are not excluded. Delete is explicit to avoid accidental data loss.

```text
devsync exec <name>
```

Run `sync`, then ask the remote agent to execute the command registered as
`commands.<name>` in `devsync.toml`. Any configured name can be executed;
only configured names can be executed.

```text
devsync build
devsync run
devsync test
```

Aliases for `devsync exec build`, `devsync exec run`, and `devsync exec test`.
They behave identically to `exec`, including the sync-first default.

Every execution command (`exec`, `build`, `run`, `test`) synchronizes first by
default, because the remote copy exists to run the latest local code. Running
stale remote code is the exception and must be requested explicitly:

```text
--no-sync    Skip the sync step and execute against the current remote copy.
```

Initial global options:

```text
--config <path>  Use a config file other than ./devsync.toml.
-v, --verbose    Print detailed local progress and protocol diagnostics.
```

Deferred commands and options:

- `logs`
- `clean`
- `shell` (deliberately excluded: arbitrary remote execution would break the
  named-command-only security model; register commands in `devsync.toml` instead)
- `watch` / `dev` auto-sync loop (requires a long-lived connection; revisit
  after the initial scope is proven)
- daemon or TCP server mode
- checksum mode flag, because the initial diff model always uses content hashes

## 4. Configuration

Default config file:

```text
./devsync.toml
```

Example:

```toml
[connection]
host = "remote-pc"
user = "user"
port = 22
agent_path = "C:\\tools\\devsync.exe"

[paths]
local_dir = "."
remote_dir = "C:\\work\\project"

[commands]
build = "powershell -NoProfile -ExecutionPolicy Bypass -File .\\build.ps1"
run = "powershell -NoProfile -ExecutionPolicy Bypass -File .\\run.ps1"
test = "powershell -NoProfile -ExecutionPolicy Bypass -File .\\test.ps1"
lint = "powershell -NoProfile -ExecutionPolicy Bypass -File .\\lint.ps1"

[sync]
exclude = [
  ".git",
  ".devsync",
  "devsync.toml",
  "bin",
  "obj",
  "build",
  "dist",
  "logs",
  "artifacts",
  "node_modules",
  ".vs",
  ".vscode"
]
```

Required fields:

- `connection.host`
- `connection.user`
- `paths.remote_dir`

Defaults:

- `connection.port = 22`
- `connection.agent_path = "devsync.exe"`
- `paths.local_dir = "."`
- `sync.exclude = []`

`[commands]` is a map of arbitrary command names to command strings. A command
entry is required only when that name is invoked. For example, `commands.build`
is required for `devsync build` (alias of `devsync exec build`), but not for
`devsync status`. `devsync exec lint` requires `commands.lint`.

Forced excludes are always applied, even if the user omits them:

- `devsync.toml`
- `.devsync/`
- `.git/`

## 5. Sync Model

Synchronization is one-way:

```text
local_dir -> remote_dir
```

The local side is authoritative. The remote side is a build and run copy.

The sync flow:

1. Local walks `local_dir` and applies forced and configured excludes.
2. Local builds a manifest.
3. Local starts `devsync.exe agent --stdio` over SSH.
4. Local sends runtime settings to the agent.
5. Remote agent walks `remote_dir` and builds a remote manifest.
6. Local compares the manifests.
7. Local sends only missing or changed files.
8. Remote agent writes files under `remote_dir`.
9. If `--delete` is set, remote agent deletes remote-only files under `remote_dir`.

Manifest entry:

```text
path  Slash-normalized relative path.
size  File size in bytes.
hash  BLAKE3 content hash.
```

Modification time is not used for initial diffing. Hash-based comparison is slower than mtime comparison, but avoids Windows timestamp precision and timezone issues.

Future optimization may add `.devsync/state` cache files to avoid rehashing unchanged files.

## 6. Protocol

The protocol runs over the SSH child process stdin/stdout streams.

Frame format:

```text
[4-byte big-endian JSON length][JSON message][optional binary payload]
```

JSON messages are UTF-8. Binary file content follows only messages that declare a payload size.

Initial message types:

```json
{"type":"hello","version":1}
{"type":"config","remote_dir":"C:\\work\\project","commands":{"build":"...","run":"...","test":"..."}}
{"type":"manifest_request"}
{"type":"manifest","files":[{"path":"src/main.cs","size":1234,"hash":"..."}]}
{"type":"sync_plan","upload":["src/main.cs"],"delete":[]}
{"type":"file","path":"src/main.cs","size":1234,"hash":"..."}
{"type":"sync_complete","uploaded":1,"deleted":0,"skipped":10}
{"type":"exec","name":"build"}
{"type":"output","stream":"stdout","data":"..."}
{"type":"exit","code":0}
{"type":"error","message":"commands.build is not defined"}
```

Protocol rules:

- The agent writes only protocol frames to stdout.
- Agent diagnostics go to stderr or are sent as `output` frames.
- Both sides reject unsupported protocol versions.
- The local side owns diff calculation.
- The remote side owns filesystem application and command execution.

## 7. Remote Execution

Remote commands are named commands from `devsync.toml`.

The local side sends:

```json
{"type":"exec","name":"build"}
```

The remote agent resolves the command from the config sent by the local side, sets the working directory to `remote_dir`, runs it, and streams output:

```json
{"type":"output","stream":"stdout","data":"..."}
{"type":"output","stream":"stderr","data":"..."}
{"type":"exit","code":0}
```

Initial command examples use PowerShell:

```text
powershell -NoProfile -ExecutionPolicy Bypass -File .\build.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\run.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\test.ps1
```

The protocol does not expose arbitrary command execution. Only configured names can be executed.

## 8. Error Handling

The CLI should report actionable errors for:

- `ssh.exe` not found.
- SSH connection failure.
- Remote `agent_path` not found.
- Remote agent exits before protocol handshake.
- Protocol version mismatch.
- `devsync.toml` parse or validation failure.
- Missing `commands.<name>` when that name is invoked.
- `remote_dir` creation, scan, write, or delete failure.
- Rejected unsafe paths.
- Transfer interruption.
- Remote command non-zero exit code.

Remote command failures return the remote command exit code when possible. Transport, protocol, and configuration failures use dedicated CLI exit codes.

## 9. Security

Initial security model:

- SSH provides authentication and encryption.
- No custom TCP listener or daemon is started.
- The agent is stdio-only.
- The agent never writes outside `remote_dir`.
- Absolute paths in file messages are rejected.
- Relative paths containing `..` are rejected.
- `devsync.toml`, `.devsync/`, and `.git/` are forced excludes.
- Only named commands are executable.
- The remote agent does not trust paths or command names sent by local until validated.

## 10. Testing Strategy

Unit tests:

- Config parsing and defaults.
- Required field validation.
- Forced and configured exclude matching.
- Manifest creation.
- Diff calculation.
- Path normalization and safety validation.
- Protocol frame encode/decode.

Integration tests:

- Sync engine against two temporary local directories.
- Agent stdio protocol using a child process without SSH.
- Remote command execution against temporary scripts.

Manual E2E tests:

- Install `devsync.exe` on both Windows machines.
- Confirm normal `ssh user@host` works.
- Run `devsync status`.
- Run `devsync sync`.
- Modify a file and confirm only that file uploads.
- Run `devsync build`, `devsync test`, and `devsync run`.
- Confirm `devsync.toml`, `.git/`, and `.devsync/` are never uploaded.

## 11. Initial Scope

In scope:

- Rust single executable.
- Windows local to Windows remote.
- Config file `devsync.toml`.
- `ssh.exe` child process transport.
- Remote `agent --stdio`.
- Hash-based manifest diff.
- One-way local-to-remote sync.
- `status`, `sync`, `sync --delete`, `exec <name>`, `build`, `run`, `test`, `--no-sync`.
- PowerShell command examples.

Out of scope:

- SFTP.
- rsync.
- Git object storage, history, branches, merge, or bidirectional sync.
- Linux/POSIX remote support.
- Long-running daemon.
- Custom TCP, HTTP, WebSocket, or QUIC server.
- Logs, clean, and shell commands.
- File permission and symlink preservation.

## 12. Milestones

M1: CLI and config

- Create Rust project structure.
- Parse `devsync.toml`.
- Implement command routing and validation.

M2: Protocol and agent stdio

- Implement frame encode/decode.
- Implement `agent --stdio`.
- Add handshake and version validation.

M3: Manifest and diff

- Walk local and remote directories.
- Hash files with BLAKE3.
- Compare manifests.
- Print `status`.

M4: Sync

- Upload changed files over the protocol.
- Write files safely under `remote_dir`.
- Support explicit `--delete`.

M5: Remote execution

- Implement named command execution.
- Stream stdout/stderr.
- Propagate exit codes.

M6: Manual E2E hardening

- Verify Windows OpenSSH flow.
- Improve error messages.
- Document installation and example config.
