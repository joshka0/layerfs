# Unmount fails at Join for a Workspace mounted 30 s or more

> **Status:** Open, filed 2026-11-14. GitHub issues are disabled on the fork, so the issue lives here.

## Summary

A Workspace that stays mounted for about 30 seconds or more cannot be unmounted. `unmount --discard` answers `Failed` with `Retained(TeardownCustody { stage: Join, detached: true, ... })`. The kernel mount is detached, but no FUSE receive loop exits (`loops_exited: 0`), so the drain stops at `Join` and the overlay file stays. An unmount right after the mount works.

This fault predates the Sandbox reopen work: it reproduces with the deployed bundle of 2026-10-22 and the SDK on `main`.

## Reproduction

Environment: OrbStack on macOS, a `node:24-bookworm` container with `--device /dev/fuse --cap-add SYS_ADMIN --security-opt apparmor=unconfined`, process runtime (`LAYERFS_RUNTIME=process`), the Store and Sandbox folders on a Docker volume (btrfs).

1. `lfs mount <project> base` (Workspace A)
2. `lfs mount <project> base` (Workspace B), then at once `lfs unmount <project> <sandbox> <B path> --discard` gives `{"kind":"Unmounted"}`.
3. Wait 40 s. Do nothing in A.
4. `lfs unmount <project> <sandbox> <A path> --discard` gives:

```
cause: Retained(TeardownCustody { ..., stage: Join, detached: true,
  work: Some(MountWork { loops_configured: 2, loops_entered: 2, loops_exited: 0, loops_joined: 0, active: 0, ... }),
  detail: "connection drain stopped at Join: loops Some(SessionSnapshot { phase: Serving, revision: 6, configured: 2, created: 2, entered: 2, exited: 0, joined: 0 }), work Some(MountWork { active: 0, retained: 0, requests: 0, completed: 0, terminal: true })", forced: None })
```

## Measured

- Bundle 2026-10-22 with the `main` SDK: fresh mount, 40 s, unmount: `Join`. Immediate unmount: `Unmounted`.
- Reopen branch bundle: unmount at 0 s `Unmounted`; at 35 s `Join`; the same with no Commit in between, so a Commit is not the trigger.
- `mount --make-rprivate /` in the container before the mounts changes nothing (the volume mounts were already `master`, not shared).
- The daemon is idle while it waits (CPU ticks flat); no process holds a file, a working directory or a mount namespace in the mounts (checked through `/proc`).

## Not tested

- On the lfs-orb workbench itself (no test mounts are allowed there).
- On a Linux host outside OrbStack.

## Impact

Every agent Workspace lives longer than 30 s, so a terminal unmount may fail this way in normal use. The reopen gate (F20 cases that keep a mount for 30 s or more) records these cases in a baseline that cites this issue.
