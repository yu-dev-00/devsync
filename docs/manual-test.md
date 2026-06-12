# devsync Manual Test

## Prerequisites

- Windows local machine.
- Windows remote machine.
- `ssh user@host` works from the local machine.
- `devsync.exe` is copied to the remote path configured as `connection.agent_path`.

## Test Project

Create a local test project with:

```text
devsync.toml
src/hello.txt
build.ps1
run.ps1
test.ps1
```

Use `devsync.toml.example` as the starting config.

## Checks

1. Run `devsync status`.
2. Confirm `src/hello.txt` is listed as upload.
3. Run `devsync sync`.
4. Confirm the remote `remote_dir` contains `src/hello.txt`.
5. Confirm the remote `remote_dir` does not contain `devsync.toml`, `.git`, or `.devsync`.
6. Edit `src/hello.txt`.
7. Run `devsync status`.
8. Confirm only `src/hello.txt` is listed as upload.
9. Run `devsync build`.
10. Confirm build output streams locally.
11. Run `devsync run`.
12. Confirm run output streams locally.
13. Run `devsync test`.
14. Confirm test output streams locally.
15. Run `devsync exec <name>` for a custom command and confirm it syncs then executes.
16. Run `devsync run --no-sync` and confirm it executes without syncing.
