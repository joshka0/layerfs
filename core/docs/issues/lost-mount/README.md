# A Workspace whose kernel mount is gone still reads as mounted

> **Status:** Open, failure list written before the code, 2026-11-17 (task
> jos-layerfs-lost-mount-chn). The operator chose option 1: LayerFS reads the
> kernel mount, finishes an unmount whose mount is gone, and logs mount events.
> Related: [unmount-join](../unmount-join/README.md).

## What happened

On lfs-orb, on 2026-11-17, two task Workspaces lost their mounts:
fleet-protocol `workspaces/1` and effect-folio `workspaces/37`. Their
space-vm closes then failed. In both, the close committed the Workspace to
its branch (`close <task>`) and then called `lfs unmount`. That unmount
detached the kernel mount. The drain then never ended, because no FUSE
receive loop exited. This is the [unmount-join](../unmount-join/README.md)
fault. The daemon kept the Workspace as Retained, with activity `Closing`.

After that:

- What was left was the empty mount directory, root-owned with mode 0700.
- `lfs reopen` answered `mounted: true, reason: "already mounted"`.
- `lfs unmount ... --discard` was refused by the retained custody.
- `daemon.log` recorded nothing with a time.

Measured on a throwaway Workspace (herdr-lessons, rowan ran the root steps):

- An outside `umount -l` while a process held a file gave the same state.
  The detached connection kept serving that file.
- The daemon still answered `already mounted` with no reference left
  (`loops_exited: 0`).
- `lfs unmount --discard` stopped at `plain detach: EINVAL`.
- Stopping the daemon and running `lfs reopen` remounted the Workspace with
  its uncommitted change. A Workspace whose branch had moved since its mount
  (a close commit) was refused with "the Branch moved".

## Rules this changes

The owner approved each change, in this task, with its record:

- `layerfs-daemon/src/control/kernel.rs`: status is "maintained observations
  only". It stays that way. The kernel check goes into the SDK, which reads
  `/proc/self/mountinfo`.
- `layerfs-fuse/src/session/force.rs` and `control/detach.rs`: "EBUSY or any
  other errno stops at Detach". `EINVAL` when the kernel table no longer holds
  this connection's mount means the mount is already gone. The teardown goes
  on from the drain.

## Failures

Each item names a failure and the expected behavior. An independent author
turns each item into a check, run in a Boat through `boat-run`, never as root
next to live daemons on lfs-orb.

L1. `workspace.status`, `operator.sandbox.reopen` or the reopen pass of
    `workspace.mount` reports a Workspace as mounted when the kernel mount
    table has no entry for its path, or has an entry of another connection.
    Expected: `mounted: false` with the reason "kernel mount gone".
L2. These operations report a Workspace in activity `Closing`, or with
    Retained custody, as mounted or as `already mounted`. Expected: not
    mounted, with the custody stage.
L3. `lfs reopen` answers `kind: ok` and exits 0 while a listed Workspace is
    not mounted in the kernel. Expected: exit 1, with that Workspace listed
    as not reopened and the reason.
L4. `lfs unmount` of a Workspace whose kernel mount is gone, after an outside
    detach or a drain that stopped, stops at `plain detach: EINVAL`, or stays
    Retained after its connection has ended. Expected: it ends the
    connection, finishes the drain and Close, removes the mount directory,
    and keeps the overlay unless `--discard`.
L5. L4 runs when the kernel still holds the mount (`EBUSY`, or the mount is
    there). Expected: the existing reversible Busy answer, with no effect.
L6. After L4 without `--discard`, `lfs reopen` does not mount the saved
    overlay again at its path. If the branch moved since the mount, it gives
    no reason that names the moved branch.
L7. The daemon mounts, detaches, unmounts, finds a mount gone, or keeps a
    Workspace as Retained, and `daemon.log` has no line for it. Expected: one
    line for each event. Each line has an ISO 8601 UTC time, the Workspace
    path, its id, its namespace, the event, the stage, and the errno or cause.
L8. A `daemon.log` line has no time: the listen line or a `ConnectionFailure`
    line, for example.
L9. A normal unmount stops at `Join` while another mount namespace holds a
    private copy of the Workspace mount. A rootless `unshare --mount` makes
    its copies private by default, and they share the superblock. The plain
    detach in the daemon's namespace then cannot end the FUSE connection, so
    no receive loop exits. Measured 2026-11-17 on lfs-orb with two throwaway
    Workspaces (herdr-lessons):
    - With `unshare -r --mount sleep` alive (copy without a propagation tag),
      the close stopped at `Join`, `detached: true`, `loops_exited: 0`, and
      left the bare root 0700 folder.
    - With `--propagation slave` (copy `master:306`), the close finished.

    An agent's gate runner did this on lfs-orb during both failed closes
    (likely; matched by time only). Expected: the unmount neither stops at
    `Join` nor loses the mount. Before the detach, the daemon looks for a
    copy of this connection's mount in another mount namespace
    (`/proc/*/mountinfo`, matched by the device). If it finds one, it answers
    the existing reversible Busy and names the holder's pid, and the
    Workspace stays mounted and usable. The [unmount-join](../unmount-join/README.md)
    case (30 s, in a container) may have the same cause; this is not
    measured.
L10. A fix for L1 to L9 changes a Workspace that is mounted and healthy: its
     status, its service, or its unmount.

## Not in scope

- What called `umount -l` from outside. The 2026-11-17 losses came from
  LayerFS's own unmount (L9), not from an outside detach.
