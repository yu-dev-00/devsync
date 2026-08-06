# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`devsync` is a Windows-first Rust CLI that keeps a local project as the source of truth and syncs it to a remote Windows *execution copy* over SSH. One executable plays two roles: the local client, and the remote agent started as `devsync.exe agent --stdio` through `ssh.exe`.

The authoritative design is [docs/superpowers/specs/2026-06-05-devsync-agent-design.md](docs/superpowers/specs/2026-06-05-devsync-agent-design.md) — consult it before changing protocol, config, or command semantics. `docs/old/` is the superseded pre-agent (SFTP-era) design, kept for history only; do not implement from it.

## Commands

```bash
cargo build
cargo test
cargo test --test agent_stdio_tests          # one integration test file
cargo test agent_writes_file_payload         # one test by name
cargo run -- --help
cargo run -- status                          # needs ./devsync.toml (see devsync.toml.example)
```

Tests spawn PowerShell and the built binary, so the suite is Windows-only in practice. No SSH or remote machine is required — cross-process E2E tests drive the agent's stdin/stdout directly. Real SSH behavior is verified by hand via [docs/manual-test.md](docs/manual-test.md).

## Architecture

Local side (`main.rs` → `sync.rs` → `client.rs`) owns config, diffing, and orchestration. Remote side (`agent.rs`) owns filesystem writes and command execution. The agent never reads `devsync.toml`; the local client ships the settings it needs (`remote_dir`, `commands`, `exclude`) in a `Config` frame right after the handshake.

| Module | Responsibility |
| --- | --- |
| [src/main.rs](src/main.rs) | clap CLI, dispatch; loads config for every subcommand except `agent` |
| [src/config.rs](src/config.rs) | `devsync.toml` parse, defaults, required-field validation, `commands.<name>` lookup |
| [src/sync.rs](src/sync.rs) | `status` / `sync` / `exec` / `run_command` flows |
| [src/client.rs](src/client.rs) | spawns `ssh -p <port> user@host "<agent_path> agent --stdio"`, handshake, framed I/O |
| [src/agent.rs](src/agent.rs) | remote message loop (config, manifest, file apply, delete, exec) |
| [src/protocol.rs](src/protocol.rs) | `Message` enum + frame encode/decode, `PROTOCOL_VERSION` |
| [src/manifest.rs](src/manifest.rs) | walk a root, BLAKE3-hash every non-excluded file, sorted slash paths |
| [src/hash_cache.rs](src/hash_cache.rs) | `<root>/.devsync/state` — reuse hashes for files unchanged since the last walk |
| [src/diff.rs](src/diff.rs) | upload / delete / skip plan from two manifests |
| [src/exclude.rs](src/exclude.rs) | configured + forced excludes (`devsync.toml`, `.devsync`, `.git`) |
| [src/path_safety.rs](src/path_safety.rs) | rejects absolute paths, drive letters, `..`, empty segments |

`src/lib.rs` re-exports every module: integration tests consume the public API (`devsync::…`) rather than duplicating logic, and `main.rs` is a thin shell over the lib.

### Invariants worth knowing before editing

- **Frame format:** 4-byte big-endian JSON length, then the JSON message, then — *only* for `File` — exactly `size` raw payload bytes. Both sides must consume that payload or the stream desyncs. `agent.rs` deliberately reads the bytes *before* any validation or error return for that reason, and `sync::build_file_message` derives `size`/`hash` from the bytes it just read so the header can never disagree with the payload.
- **Handshake:** the client sends `Hello`, and requires a matching `HelloAck` (a plain `Hello` echo is rejected — that's how an old agent is caught). Bump `PROTOCOL_VERSION` whenever the `Message` enum or framing changes.
- **Excludes apply on both sides.** The agent builds its manifest with the same exclude list, so remote-only build output (`bin`, `obj`, `dist`, …) is invisible to the diff and `sync --delete` cannot remove it. Changing where excludes are applied changes what `--delete` destroys.
- **Diff is content-hash based**, never mtime — intentional, to dodge Windows timestamp precision/timezone issues. The hash cache does compare mtime, but only against the timestamp *the same machine* recorded when it last hashed *that same file*; a local timestamp is never compared to a remote one. Do not "simplify" it into an mtime-based diff.
- **Both sides cache hashes** in `<root>/.devsync/state`. `.devsync` is a forced exclude, so the cache is never uploaded and `sync --delete` never removes it — check that still holds if you touch the exclude list. A missing or corrupt cache must degrade to hashing everything, never fail the walk.
- **Only named commands are executable.** `Exec { name }` resolves against the `commands` map the client sent; there is no arbitrary-command path, and a `shell` subcommand is deliberately out of scope.
- **Execution syncs first.** `exec`/`build`/`run`/`test` all call `sync` unless `--no-sync`; `build`/`run`/`test` are pure aliases for `exec <name>`. Command names in `[commands]` are arbitrary and may collide with subcommand names (`devsync exec sync` runs `commands.sync`).
- **`sync` waits for the `SyncComplete` ack** before returning, because `RemoteClient::Drop` kills the ssh child — dropping early would truncate pending writes. The exec arms in `main.rs` call `std::process::exit(code)` (skipping `Drop`) only after the `Exit` frame has arrived.
- **Exec streams output.** The agent `.spawn()`s the command and drains stdout *and* stderr on two reader threads — draining only one deadlocks when the child fills the other. The threads forward raw bytes to the main thread, which owns the writer; frames written from two threads would interleave mid-frame. Child stdin is `Stdio::null()`: `.output()` did that implicitly, but an inherited handle would let the child consume the agent's own protocol stream. Chunk boundaries split UTF-8 sequences, so `take_decodable` emits only the complete prefix and carries the remainder — decoding each chunk independently corrupts multi-byte characters.
- **Command output is decoded in the console code page**, not UTF-8. Windows PowerShell encodes redirected stdout with the console output code page (CP932 on a Japanese install), so `from_utf8_lossy` destroyed every non-ASCII character. `console_encoding()` asks Windows (`GetConsoleOutputCP`, falling back to `GetOEMCP` because the agent has no console under sshd) and `encoding_rs` decodes. Note this is separate from *script file* encoding: PowerShell 5.1 reads a BOM-less `.ps1` as ANSI, which garbles literals before they are ever printed.
- **Exec appends `; exit $LASTEXITCODE`** to the configured command. PowerShell's `-Command` collapses every non-zero native exit code to 1, so without it `cargo build` failing with 101 is reported as 1. `exit $null` (no native process ran) yields 0, so success is unaffected.

### Known v1 limitations (documented in-code; don't "fix" accidentally)

- File uploads are fire-and-forget; a per-file `Error` surfaces later, when the `SyncComplete` read bails.
- The agent validates a `Hello` when it gets one but does not *require* a handshake before operational frames.

## Conventions

- Errors use `anyhow` with `context`; the agent answers recoverable problems with an `Error` frame and keeps serving rather than terminating the connection.
- Paths on the wire are always slash-normalized relative strings; convert with `MAIN_SEPARATOR_STR` only at the filesystem boundary.
- Work is spec- and plan-driven: [docs/superpowers/plans/2026-06-05-devsync-agent.md](docs/superpowers/plans/2026-06-05-devsync-agent.md) tracks tasks. Commits are conventional-style (`feat:`, `fix:`, `docs:`, `test:`, `harden:`) and scoped to one change.
- Out of scope for this version: SFTP/rsync, daemon or TCP/HTTP server modes, Linux remotes, bidirectional sync, Git object storage, `logs`/`clean`/`shell` subcommands, permission/symlink preservation.
