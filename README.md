# devsync

`devsync` keeps a local Windows project as the source of truth and syncs it to a remote Windows execution copy through an SSH-launched stdio agent.

Initial commands:

```text
devsync status
devsync sync
devsync sync --delete
devsync build
devsync run
devsync test
```

See `devsync.toml.example` for configuration and `docs/manual-test.md` for the Windows OpenSSH E2E checklist.

## `sync --delete` and remote-only files

`devsync sync --delete` deletes any file under `remote_dir` that is not part of the
local manifest. In this version the remote agent enumerates `remote_dir` **without
applying your `[sync].exclude` patterns** — only the forced excludes
(`devsync.toml`, `.git/`, `.devsync/`) are protected remotely.

Because excluded paths such as `bin`, `obj`, `build`, `dist`, and `artifacts` are
not uploaded, they appear "remote-only" and **`--delete` will remove them**, including
build outputs produced on the remote machine. Plain `devsync sync` (without `--delete`)
never deletes anything and is safe. Prefer it unless you specifically want the remote
copy to mirror the local file set exactly.
