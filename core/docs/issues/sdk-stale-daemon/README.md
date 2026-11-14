# A long-lived SDK process dials a daemon that another process replaced

> **Status:** Open, filed 2026-11-14 by the reopen test author. GitHub issues are disabled on the fork, so the issue lives here. Not part of the reopen failure list (core/docs/issues/373/REOPEN.md F1-F40); the reopen gate prints it as a `NOTE` and does not gate on it.

## Summary

A process that used a Workspace before its daemon was killed keeps the old daemon's connection after another process restarted the Sandbox with `lfs reopen`. Its next Workspace call fails with `Failed: Process(Custom { kind: NotFound, error: "daemon process exited" })`. A new process makes the same call successfully, so the Sandbox record names the new daemon. A new `LayerFS` object in the old process fails the same way, so the stale state is process-wide in the native binding, not per object.

## Reproduction

Environment: OrbStack on macOS, the process runtime (`LAYERFS_RUNTIME=process`) in a `node:24-bookworm-slim` container with `--device /dev/fuse --cap-add SYS_ADMIN`, product 5f106ea97 (bundle `layerfs-daemon` sha256 `584c69c7…`, `layerfs.node` `2c15bae8…`).

1. Process P: `new LayerFS(project)`, `workspace.mount("w2")` gives `ws`; `workspace.exec(ws, "echo race > race.txt")`.
2. Kill the Sandbox daemon with SIGKILL.
3. From other processes: `lfs reopen <project>`. The answer shows `ws` with `reopened: true` (or `already mounted`), and one daemon runs on the Sandbox folder.
4. A new node process: `workspace.exec(ws, "cat race.txt")` prints `race`.
5. Process P: `workspace.exec(ws, "cat race.txt")` fails with `daemon process exited`. So does the same call on a new `LayerFS(project)` made inside P.

The reopen gate runs these steps in its `live` phase:

```
tools/orbstack/reopen-e2e.sh <work-dir> <bundle-dir> ~/.local/share/lfs-vm/bundle <sdk/js of main>
grep '^NOTE' <work-dir>/report/summary.txt
```

Receipt: `sdk/js/test/reopen/receipts/2026-11-14-5f106ea97.txt`.

## Impact

A process that holds the SDK across a Sandbox restart (a dsh Live worker, a long-running harness) cannot use its Workspaces again until it restarts. The `lfs` command starts a new process per call, so lfs-orb agents that only use `lfs` do not see this.

## Not tested

- Whether the old process recovers after the 2-minute idle close of its cached state, if any.
- A Docker-runtime Sandbox.
