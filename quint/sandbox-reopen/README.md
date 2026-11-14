# Sandbox reopen race model

> **Status:** Model checked with TLC on 2026-11-14 against `main` ff5631b7a
> plus the fixes `fix-reclaim-gone-sandbox` (93beafa3f) and
> `fix-revive-failed-restart` (2f254db3c). Reviewed by another model family
> (4 blocking findings, all addressed here). Replay binding: see Open.

`sandbox_reopen.qnt` models one process Sandbox folder after a restart: the
daemon its record names has ended, and the folder may hold saved Workspace
overlays. Three callers race on it, each its own process:

- the boot reopen (`operator.sandbox.reopen`, `revive.rs reopen_all`),
- a mount (`workspace.rs place`),
- a listing (`workspace.rs list`).

The boot reopen and a mount take the project guard first. A listing reads
the record and the daemon with no guard, ends there when the daemon runs or
the record is gone, and takes the guard only to reclaim. Under the guard a
caller that finds no record ends at once (for a listing, reclaim's
`NoSuchSandbox`). The guard wait is bounded: a caller that cannot get it
ends refused `Busy`. A restart is two steps: the spawn, which `process.rs`
refuses while the saved daemon runs, then the handshake and record save,
which can fail; a failure stops the spawned daemon.

## Properties

- `NoOverlayLoss`: a folder with saved overlays is never deleted.
- `OneDaemon`: at most one daemon serves the folder.
- `NoOrphanDaemon`: outside a restart in progress, every running daemon is
  the one the record names.
- `ReopenedWhenDone`: when every caller ended with success, saved overlays
  whose records can reattach are mounted. Records without a base root and
  without `legacyHead` (`eligible` false), and runs with a failed or `Busy`
  caller, are outside this claim.
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

## Findings

- A listing that raced a reclaim failed with ENOENT: reclaim took the
  Sandbox lock inside the removed folder. Fixed in the code
  (`fix-reclaim-gone-sandbox`): reclaim reads the record first; the listing
  takes `NoSuchSandbox` as gone.
- A restart whose handshake or record save failed left its daemon running,
  and the next revive started a second one (TLC: boot spawns 1, fails; mount
  spawns 2). Fixed in the code (`fix-revive-failed-restart`).

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

- Trace replay through the real code with `quint-connect`, with code plants
  that the replay must catch (the binder's branch).
