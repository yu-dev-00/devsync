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
