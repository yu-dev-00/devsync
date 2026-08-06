---
name: devsync
description: Build, run, and test a project on a remote Windows machine using the devsync CLI, which syncs the local source of truth to a remote execution copy over SSH and runs named commands there. Use this whenever the working directory contains a devsync.toml — including when the user just says "build it", "run the tests", or "does it compile" in such a project, since the build belongs on the remote machine rather than locally. Also use it when the user mentions building or running on another PC, a build machine, a GPU box, or "the remote", and when a devsync command fails and its error needs interpreting.
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

## Start by reading devsync.toml

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
devsync status                  # what a sync would upload and delete; changes nothing
devsync sync                    # upload changed files; never deletes
devsync sync --delete           # also delete remote files absent locally
devsync exec <name>             # sync, then run [commands].<name> remotely
devsync exec <name> --no-sync   # run against whatever is on the remote now
devsync build                   # alias for: devsync exec build
devsync run                     # alias for: devsync exec run
devsync test                    # alias for: devsync exec test
```

`--config <path>` selects a config other than `./devsync.toml`.

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

The right move is to add the entry to `[commands]` in `devsync.toml`:

```toml
[commands]
lint = "cargo clippy -- -D warnings"
```

Then `devsync exec lint`. Since `devsync.toml` is itself never synced, adding a
command takes effect immediately with no redeploy.

Command names are arbitrary and may collide with subcommands: `devsync exec sync`
runs `commands.sync`, not the built-in sync.

## Excludes decide what --delete destroys

`[sync].exclude` applies on **both** sides. Excluded paths are invisible to the
diff on the remote too, which is what keeps `sync --delete` from wiping remote
build output like `target/` or `obj/`. `devsync.toml`, `.git/`, and `.devsync/`
are always excluded.

`sync --delete` is the one command here that destroys data. Plain `sync` never
deletes, so prefer it, and confirm with the user before using `--delete` unless
they asked for it. Run `status` first — it lists exactly what would be deleted.

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

No `devsync.toml` means the project does not build remotely; build it locally as
usual. devsync targets Windows on both ends and syncs one way only, so it is not
the tool for fetching artifacts or logs back from the remote — use `scp` for
that.
