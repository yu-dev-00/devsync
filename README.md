# devsync

`devsync` keeps a local Windows project as the source of truth and syncs it to a remote Windows execution copy through an SSH-launched stdio agent.

Initial commands:

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

See `devsync.toml.example` for configuration and `docs/manual-test.md` for the Windows OpenSSH E2E checklist.

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
