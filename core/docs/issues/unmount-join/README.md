# Unmount fails at Join for a Workspace mounted 30 s or more

> **Status:** Open, narrowed 2026-11-18 (see "Update 2026-11-18"). Filed 2026-11-14. GitHub issues are disabled on the fork, so the issue lives here.

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

## Update 2026-11-18

One cause of this state is proven and fixed in
[lost-mount](../lost-mount/README.md) L9. Another mount namespace that holds a
private copy of the Workspace mount keeps the FUSE connection alive after the
plain detach, so no receive loop exits and the drain stops at `Join`. A
rootless `unshare --mount` makes such a copy, because its default propagation
is private. Measured on lfs-orb on 2026-11-17 with throwaway Workspaces:
with a private copy alive, the close stopped at `Join`; with
`--propagation slave`, it finished. Since `53523b31` the unmount looks for
such a copy before the detach. If it finds one, it answers the reversible
Busy (`unmount:namespace`) and names the holder's pid. If the kernel mount is
already gone, it ends the connection through its fusectl abort (L4).

This issue's own steps, run on a Boat at `4c73c28de` (main `53523b31` plus
the diagnostic script `tools/orbstack/unmount-join-repro.sh`): an x86_64
Linux VM, kernel 6.8.0-117-generic, systemd, Workspaces on the VM's disk.
- B, unmounted at once: `Unmounted`.
- A, unmounted after 40 s: `Unmounted`, no stall.
- Before the late unmount, 11 processes in other mount namespaces (systemd
  services such as NetworkManager, systemd-logind and polkitd) held copies of
  A's mount. All were slaves of A's peer group (`master:707`), so the detach
  reached them.

Not known: whether the original repro needed a private copy. That repro ran
in an OrbStack Docker container (`node:24-bookworm`, `--device /dev/fuse
--cap-add SYS_ADMIN`, Docker volumes). The original check through `/proc`
found no process with a mount namespace in the mounts. So that container may
stall for another reason, and the Boat cannot reproduce it. The open part of
this issue is that container case. Run
`tools/orbstack/unmount-join-repro.sh` in it: an `unmount:namespace` Busy that
names a pid means the cause is the same, and a stop at `Join` with no copy
listed means the cause is still unknown. Close this issue when that run
unmounts cleanly or names its cause.
