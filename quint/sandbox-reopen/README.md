# Sandbox reopen race model

> **Status:** Model checked with TLC on 2026-11-14 against
> `fix-reopen-races` d08e3b656. The third review's R3-B3 and the binder's
> replay findings up to model 73 are addressed here. R3-B2 (a failed stop
> whose recovery save also fails) is open: see Open.

`sandbox_reopen.qnt` models one process Sandbox folder after a restart: the
daemon its record names has ended, and the folder may hold saved Workspace
overlays. Four callers race on it, each its own process:

- the boot reopen (`operator.sandbox.reopen`, `revive.rs reopen_all`),
- a mount (`workspace.rs place`),
- a listing (`workspace.rs list`),
- a Workspace call such as `workspace.status` (`registry.rs sandbox`).

The boot reopen and a mount take the project guard first. A listing reads
the record and the daemon with no guard, ends there when the daemon runs or
the record is gone, and takes the guard only to reclaim. Under the guard a
caller that finds no record ends at once (for a listing, reclaim's
`NoSuchSandbox`). The guard wait is bounded: a caller that cannot get it
ends refused `Busy`, except a listing, which then lists the Sandbox. A
Workspace call reads the record and the daemon with no guard: no record is
`NoSuchSandbox` at once; a running daemon answers, or refuses
`StaleSandbox` when it is unusable; on an ended daemon it waits for the
guard and reads the record again the same way, else the Sandbox is gone.
A daemon is unusable when a failed restart could not stop it: it runs, the
record names it, but it answers Hello with another reply.

A restart is two steps: the spawn, which `process.rs` refuses while the
saved daemon runs, then `finish`, with four outcomes: `ok` (the record
names the new daemon; the Workspaces reattach), `stopped` (the handshake
fails; the daemon is stopped, the record unchanged), `after_record` (a step
after the record save fails; the daemon is stopped, the record names it),
`stop_failed` (a failure whose stop fails; the record names the running
daemon).

A mount's call result and its target revive outcome are separate
variables (`result`, `target`): when the revive fails, the mount places its
Workspace in another Sandbox, outside this folder. That start can fail too;
when it succeeds, the mount ends `ok` and names the failed revive in
`revived` (`reported`). Each saved Workspace reattaches or
not on its own: `eligible` of `overlays` can (a moved branch root or an
overlay the daemon refuses cannot); a call can succeed with some
Workspaces unmounted.

## Properties

- `NoOverlayLoss`: a folder with saved overlays is never deleted.
- `OneDaemon`: at most one daemon serves the folder.
- `NoOrphanDaemon`: outside a restart in progress, every running daemon is
  the one the record names.
- `ReopenedWhenDone`: when every caller ended, the boot reopen and the
  mount succeeded, and no revive of the folder failed, exactly the saved
  Workspaces that can reattach (`eligible`) are mounted.
- `RevivedReported`: a mount that succeeds after its target revive failed
  names that revive in `revived`.
- `StatusRefusesUnusable`: a Workspace call never answers from an unusable
  daemon; it refuses `StaleSandbox`.
- `StatusWaitsOutRestart`: a Workspace call never calls the Sandbox gone
  while a restart of it is under way.
- `BootNeverDeletes`: the boot reopen never deletes a folder; with no saved
  overlay it answers `ended`. Only a reclaim deletes.
- `AllEnd` (temporal, weak fairness): every caller ends, possibly refused.
- `GuardReleased` (temporal, weak fairness): the guard is eventually free
  for good. `AllEnd` alone cannot see a leaked guard, because the waiters
  then end refused `Busy`.

## Plants

Each plant is one deliberate change, set by the constant `PLANT`.

| Plant | Change | Expected |
| --- | --- | --- |
| `no_kept` | reclaim deletes a folder with overlays | breaks `NoOverlayLoss` |
| `no_running_check` | both running checks removed: revive's and `process.rs` restart's | breaks `OneDaemon` |
| `no_revive_check` | only revive's running check removed | holds: restart's own check still refuses |
| `no_guard` | callers act without the guard | breaks `OneDaemon` |
| `leak_guard` | a caller ends without releasing the guard | breaks `GuardReleased` |
| `boot_reclaims` | a revive with no saved overlay deletes the folder | breaks `BootNeverDeletes` |
| `keep_failed_daemon` | a failed restart leaves its daemon running (the code before 2f254db3c) | breaks `OneDaemon` and `NoOrphanDaemon` |
| `no_revived_report` | a mount drops its failed target revive (the code before d08e3b656) | breaks `RevivedReported` |
| `status_no_wait` | a Workspace call calls an ended daemon gone without the guard (the code before d08e3b656) | breaks `StatusWaitsOutRestart` |
| `no_stale_check` | a Workspace call answers from an unusable daemon | breaks `StatusRefusesUnusable` |
| `count_unattached` | a Workspace that cannot reattach counts as mounted | breaks `ReopenedWhenDone` |

## Findings

- A listing that raced a reclaim failed with ENOENT: reclaim took the
  Sandbox lock inside the removed folder. Fixed in the code
  (`fix-reclaim-gone-sandbox`): reclaim reads the record first; the listing
  takes `NoSuchSandbox` as gone.
- A restart whose handshake or record save failed left its daemon running,
  and the next revive started a second one (TLC: boot spawns 1, fails; mount
  spawns 2). Fixed in the code (`fix-revive-failed-restart`).

- A listing or Workspace call during a restart failed with "no daemon
  listen line": the restart starts a new, empty log while the record still
  names the ended daemon (gate case F41). Fixed in the code (296987bd6):
  the saved pid is checked before the log. A Workspace call then called
  the restarting Sandbox gone; it now waits for the guard (d08e3b656,
  `status_no_wait`).

## Run

From this folder:

```sh
sfw npm ci --no-audit --no-fund
./check.sh
```

`check.sh` exits 0 only when every expectation holds. A result counts only
when the tool answers clearly: exit 0 with "No violation found", or exit 1
with "found a counterexample" and TLC's own violation line for the kind
checked. Any other result is an error and fails the check. Logs go to
`results/`. `QUINT` overrides the quint binary, to test this handling.
Quint 0.33.1 is pinned in `package.json`; TLC needs Java (25 tested).

## Not modelled

- The Sandbox's own lock (`ownership.lock`), taken inside the project
  guard: every path that takes it already holds the project guard.
- Changes inside a kept folder (an overwritten overlay file):
  `NoOverlayLoss` checks that the folder exists.

## Open

- R3-B2 (third review, reproduced in code on 2026-11-14 by fault
  injection): a restart whose handshake fails, whose stop fails (EPERM) and
  whose recovery save fails leaves its daemon running while the record names
  the ended one; the next revive starts a second daemon on the folder. The
  model records the daemon for every failed stop, so it does not show this.
  The fix is an operator decision; the model follows it.
- Not modelled: which Sandboxes a listing returns (a listing ends `ok`;
  its items are not tracked), and each Workspace's reason (the model counts
  reattachable Workspaces; the code gives every unmounted one a reason).
- `no_guard` removes the project guard. The Sandbox lock is not modelled,
  because every path that takes it holds the project guard, so the plant
  stands for removing both.

- Trace replay through the real code with `quint-connect`, with code plants
  that the replay must catch (the binder's branch).
