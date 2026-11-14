# Reopen a process Sandbox after a restart

> **Status:** Design and failure list, 2026-11-14. Written before the code.
> The operator chose this feature (option A) on 2026-11-14.

## Problem

A restart of the host (or of the workbench container) ends every daemon
process. Each process Sandbox keeps its folder
`/var/lib/layerfs/sandboxes/<name>/`: its config, its daemon log, one overlay
file `overlay/<namespace>.sqlite` for each mounted Workspace, and the mount
home `workspaces/`. The SDK keeps `sandbox.record` and one
`workspace-<id>.record` for each Workspace.

Before this change, the next `workspace.mount` or `operator.sandbox.list`
found the daemon gone and reclaimed the Sandbox: `remove_dir_all` deleted
every overlay, with its uncommitted changes.

Measured on lfs-orb after the reboot of 2026-11-14: 34 overlay files, 26 GB,
all schema 39, all in generation 1 only, with no orphan rows, no cleanup
tasks and no commit records. No branch of a saved Workspace had a commit after
that Workspace's mount.

## What a reopen restores

A Workspace binding has three parts:

1. The overlay file. It holds every row. It survives.
2. The base: the committed root that the overlay changes. Before this change
   only daemon memory held it.
3. The overlay engine state (`WorkspaceState`): active generation, install
   floor, revision, dirty counts. Only daemon memory held it.

A reopen gets the base from the record and the engine state from the rows.

### Base

- A new `workspace-<id>.record` holds `root`: the effective root of the
  binding at mount.
- A record from before this change has no `root`, and does not reopen: its
  Workspace shows `mounted: false` with the reason `its record has no saved
  base root (written before reopen existed)`. (The `legacyHead` option that
  reopened such records on the branch head served the overlays of
  2026-11-14; it was removed once they were reopened.)
- The daemon binds the reopened Workspace only when the branch's current
  effective root equals the base the SDK sends. It never rebases.

### Engine state

The overlay rows decide the engine state (pure rule
`layerfs_overlay::resume_rules`):

- Every row of `fs_inode`, `fs_entry`, `fs_data`, `fs_truncate` and
  `fs_data_ref` is in one generation `G`, and `G` is 1.
- No row is in the orphan domain (generation -1).
- `cleanup_task` and `commit_record` are empty.
- Then: active generation 1, install floor 0, no capture, revision equal to
  the largest `fs_inode.sequence` (0 when there is none), dirty inodes equal
  to the `fs_inode` rows, dirty names equal to the `fs_entry` rows.
- Any other shape is refused with the fact that failed.

`G` > 1 means the Workspace committed before the restart, and its base moved
at each install. Neither the record nor the overlay holds that base. This
change refuses that shape (parked decision P1).

## Steps

1. `process::Sandbox::restart` starts a daemon again on an existing Sandbox
   folder. The saved daemon must not run. It detaches any dead mount in the
   mount home, keeps the old log as `daemon.previous.log`, and spawns the
   daemon on the saved config.
2. The daemon's overlay roster finds the overlay files of the folder at start.
   It replaces the stale template, and it numbers new Workspaces above every
   existing file.
3. Control request `Reopen { workspace, branch, namespace, root }` binds the
   saved Workspace on its existing overlay file and number. `Attach` then
   mounts it at `workspaces/<namespace>`, its saved path.
4. SDK discovery (`workspace.mount` placement, `operator.sandbox.list`) does
   not reclaim a dead process Sandbox that has a saved Workspace with an
   overlay file. `operator.sandbox.reopen` restarts every such Sandbox of the
   project and reopens its Workspaces. When `workspace.mount` placement finds
   such a Sandbox, it revives it, and uses it if it has a free slot. If that
   revive fails or ends the Sandbox, the mount places the new Workspace in
   another Sandbox or starts a new one. The old Sandbox, its records and its
   overlay files are kept. The mount answer reports the failed revive in
   `revived` (Interfaces).
5. `lfs reopen <project>` calls it. `vm-boot.sh apply` runs it for each
   project at boot.

## Interfaces

`operator.sandbox.reopen()` (JS `layerfs.operator.sandbox.reopen()`) answers:

```
{ kind: "ok",
  sandboxes: [
    { id: "1", name: "layerfs-memoryd-1",
      state: "running" | "restarted" | "failed" | "ended",
      reason: null | "<why the restart failed>",
      workspaces: [
        { path: "/var/lib/layerfs/sandboxes/layerfs-memoryd-1/workspaces/6",
          workspace: "<64 hex>", namespace: 6, branch: "ziglead-2-mwb14ppm",
          mounted: true | false,
          reopened: true | false,
          reason: null | "already mounted" | "<why it is not mounted>" } ] } ] }
```

- It lists each process Sandbox of the project that has a saved Workspace
  record. `restarted`: this call started its daemon. `running`: its daemon
  ran already; nothing was started. `failed`: the daemon could not be
  started or reached, and every Workspace shows `mounted: false` with that
  reason. `ended`: its daemon ended and no saved Workspace has an overlay
  file; nothing was started.
- A saved Workspace record whose overlay file is gone is listed with
  `mounted: false`, `reopened: false` and reason `no saved overlay file`.
- On a `restarted` or `running` Sandbox, each saved Workspace that the daemon
  does not serve is reopened (a reopen can stop after the restart). One it
  serves shows `mounted: true`, `reopened: false`, reason `already mounted`.
- `mounted` is true when the Workspace is mounted after the call.
  `reopened` is true when this call reopened it. `reason` is `null` exactly
  when `reopened` is `true`.

`workspace.mount(branch)` (JS `layerfs.workspace.mount`, `lfs mount`)
answers its usual fields and `revived`:

```
{ sandbox: "2", path: "...", image: null,
  revived: [ <one operator.sandbox.reopen sandboxes item, state "failed"> ] }
```

- `revived` is empty unless placement revived a Sandbox with saved overlays
  and that revive failed. Then it holds that Sandbox's item exactly as
  `operator.sandbox.reopen` gives it: `id`, `name`, `state: "failed"`,
  `reason`, and each saved Workspace by `path` with `mounted: false` and a
  non-empty `reason`. The new Workspace is placed in another Sandbox; the
  failed Sandbox, its records and its overlay files are kept.
- If the mount itself then fails, its refusal carries the same `revived`
  in its `detail` (operator, 2026-11-14: a revive failure is never hidden).

One daemon serves a Sandbox folder. At start, before it binds or starts
anything, the daemon takes an exclusive flock on
`<folder>/config/daemon.setup.lock` and writes its pid there; it holds the
lock until it exits. A daemon that finds the lock held exits at once
(status 1) with one stderr line:
`layerfs-daemon: the Sandbox folder is held by daemon pid <pid>`. A revive
whose restart meets it answers `failed`, and its reason holds that line. A
daemon left running by a failed restart (its stop failed, and its record
could not be saved) therefore blocks every later daemon on the folder, and
each revive answer names it, until it ends.

A stop of a daemon sends SIGTERM, waits, then SIGKILL, and waits at most
60 s more (`layerfs-sandbox` `backend/stop_rules.rs`); then it fails
`TimedOut`.

A Sandbox is `restarting` while another call holds the project's guard
and the Sandbox's saved daemon has ended: `operator.sandbox.reopen` holds
that guard while it revives each Sandbox.

- `operator.sandbox.list` lists a `restarting` Sandbox.
- A Workspace call (`workspace.status`, `commit`, `exec`, `unmount`, process
  calls) that finds its saved daemon ended waits for the project's guard, at
  most 60 s, then reads the Sandbox record again: it answers normally from a
  restarted daemon, and refuses `NoSuchSandbox` for a Sandbox that is still
  gone. If the guard is still held after 60 s, it refuses
  `Busy` with message `Sandbox <id> is restarting`.

`lfs reopen <project>` prints that answer as one JSON line. It exits 0 when
every listed Workspace has `mounted: true`, 1 otherwise, and 2 on a usage
error.

`vm-boot.sh apply` runs `lfs reopen` for each project folder under
`$LAYERFS_HOME` that has a `sandboxes` folder, only when
`mountpoint -q /var/lib/layerfs-data` and `mountpoint -q /var/lib/layerfs`
both succeed. Otherwise it prints `vm-boot: Store volume not mounted, no
reopen` to stderr and reopens nothing.

## Failure list

This list is the contract for the gating tests. An independent author writes
the tests from it. Each item names a way the feature can fail.

### Overlay open-existing and engine state

- F1. Reopen creates a new file when the overlay file is missing, instead of
  a refusal.
- F2. Reopen truncates, rewrites or copies the template over an existing
  overlay file, so rows are lost.
- F3. Reopen accepts a file whose schema version is not 39 or whose
  application id is not the overlay id.
- F4. Reopen accepts a file with rows in more than one generation, in a
  generation other than 1, or in the orphan domain (-1).
- F5. Reopen accepts a file with a `cleanup_task` row or a `commit_record`
  row.
- F6. The rebuilt revision is lower than the largest `fs_inode.sequence`, so
  a later write reuses a sequence and a stale `fs_digest` reads as current.
- F7. The rebuilt dirty counts differ from the active generation's row
  counts, so status reports a clean Workspace that has changes, or Commit
  answers UpToDate with uncommitted changes.
- F8. A refused reopen changes the file: any row, the page count, or the
  journal mode.
- F9. A reopened Workspace does not show the files, bytes, modes, symlinks
  and deletions that the overlay held before the restart.

### Daemon roster and control

- F10. A daemon restarted on an existing folder fails at start because the
  template `overlay/overlay.sqlite` exists.
- F11. A new mount on a restarted daemon gets a namespace whose overlay file
  already exists, so it fails or overwrites a saved Workspace.
- F12. `Reopen` of a namespace that a live Workspace of the daemon holds
  succeeds, so two bindings share one file.
- F13. `Reopen` of the same Workspace twice succeeds.
- F14. `Reopen` binds when the branch's current root differs from the sent
  root (a commit happened after the mount), so the overlay applies to the
  wrong base.
- F15. `Reopen` of a missing branch, or with a root of the zero value,
  succeeds.
- F16. A failed `Reopen` deletes or changes the overlay file. A refused
  reopen must leave the file for a later attempt.
- F17. After `Reopen` and `Attach`, the mount path differs from
  `workspaces/<namespace>`, the saved path.
- F18. A reopened Workspace takes new inode serials that collide with serials
  already in its overlay rows.
- F19. Commit of a reopened Workspace does not publish its overlay changes, or
  publishes changes against a base other than the saved one.
- F20. Unmount with discard of a reopened Workspace leaves its overlay file;
  unmount without discard is refused while it has uncommitted changes.

### Process runtime restart

- F21. `restart` runs while the saved daemon (same pid and start time) still
  runs, so two daemons serve one folder.
- F22. `restart` creates a missing Sandbox folder or config, instead of a
  refusal.
- F23. `restart` reads the old daemon's listen line from the old log and
  dials a dead endpoint.
- F24. `restart` leaves a dead FUSE mount (ENOTCONN) at `workspaces/<n>`, so
  the attach fails.
- F25. `restart` detaches a mount that is not in the Sandbox's own mount home.
- F26. `restart` deletes any overlay file.

### SDK discovery and records

- F27. `workspace.mount` or `operator.sandbox.list` reclaims a dead process
  Sandbox that has a saved Workspace with an overlay file.
- F28. A dead Sandbox with no saved Workspace overlay is no longer reclaimed
  (the idle-close path regresses).
- F29. A Docker Sandbox found gone takes the reopen path.
- F30. `operator.sandbox.reopen` does not rewrite `sandbox.record` with the
  new pid, start time and daemon instance, so the next call dials a dead
  daemon or fails the instance check.
- F31. A record with `root` reopens on a branch whose root moved.
- F32. A record without `root` reopens.
- F33. One Workspace that fails to reopen stops the reopen of the other
  Workspaces of the Sandbox, or deletes its record or overlay.
- F34. `operator.sandbox.reopen` restarts a Sandbox whose daemon runs.
- F35. Two concurrent callers (boot reopen and an `lfs mount`, or two
  reopens) start two daemons on one folder, or one reclaims what the other
  reopens. Every check and action runs under the project guard and the
  Sandbox guard.
- F36. A new mount does not save `root` in its record.
- F37. The reopen answer does not name, for each saved Workspace, its path and
  whether it reopened, with the reason when it did not.

### Tools

- F38. `lfs reopen <project>` exits 0 when a Workspace did not reopen.
- F39. `vm-boot.sh apply` stops the boot (firewall, sshd, services) when a
  reopen fails.
- F40. `vm-boot.sh apply` reopens before the Store volume is mounted.

## Parked decisions

- P1. Reopen of a Workspace that committed before the restart (generation >
  1). Recommended: keep the binding (branch, base root, generation) in the
  overlay file, written in the same transaction as each install. This is an
  overlay schema change (39 to 40).
