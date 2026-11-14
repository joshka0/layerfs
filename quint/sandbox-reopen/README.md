# Sandbox reopen race model

> **Status:** Model checked with TLC on 2026-11-14, against the code at
> `5f106ea97` (after F1). Not yet bound to the code
> by trace replay, and not yet reviewed by another model family (see Open).

`sandbox_reopen.qnt` models one process Sandbox folder after a restart: the
daemon its record names has ended, and the folder may hold saved Workspace
overlays. Three callers race on it, each its own process:

- the boot reopen (`lfs reopen`, `operator.sandbox.reopen`),
- a mount (`workspace.mount` placement),
- a listing (`operator.sandbox.list`).

The boot reopen and a mount take the project guard first; a boot visits the
Sandbox only while its record exists. A listing reads the record and the
daemon with no guard, ends there when the daemon runs or the record is gone,
and takes the guard only to reclaim, which reads the record again (Busy) and
refuses saved overlays (Kept). Under the guard a caller chooses (use,
restart and reopen, reclaim, or nothing: a revive never deletes), and acts
in a later step. The source is
`sdk/js/native/src/discovery/revive.rs`, `sandbox.rs` (`reclaim`) and
`workspace.rs`; the choice is `revive_rules::next`. See
[REOPEN.md](../../core/docs/issues/373/REOPEN.md).

## Properties

- `NoOverlayLoss`: a folder with saved overlays is never deleted.
- `OneDaemon`: at most one daemon serves the folder.
- `ReopenedWhenDone`: when every caller has ended, the saved Workspaces are
  mounted.
- `BootNeverDeletes`: the boot reopen never deletes a folder; with no saved
  overlay it answers `ended` (post-F1, `5f106ea97`). Only a reclaim deletes.
- `AllEnd` (temporal, weak fairness): every caller ends; the guard is the
  only wait.

## Plants

Each plant is one deliberate bug, set by the constant `PLANT`. Each must
break the property it names; that trace is the witness that the property can
fail.

| Plant | Bug | Breaks |
| --- | --- | --- |
| `no_kept` | reclaim deletes a folder with overlays | `NoOverlayLoss` |
| `no_running_check` | revive restarts without checking that the saved daemon runs | `OneDaemon` |
| `no_guard` | callers act without the guard | `OneDaemon` |
| `leak_guard` | a caller ends without releasing the guard | `AllEnd` |
| `boot_reclaims` | a revive with no saved overlay deletes the folder | `BootNeverDeletes` |

## Run

From this folder:

```sh
sfw npm ci --no-audit --no-fund
./check.sh
```

`check.sh` exits 0 only when the instance without a plant holds every
property and each plant breaks its property. It uses Quint 0.33.1 (pinned in
`package.json`) and the Java on `PATH` (TLC; Java 25 tested).

## Open

- Trace replay through the real guard and reclaim/revive code
  (`quint-connect` 0.1.2, a Rust crate this repo does not use yet), with code
  plants that the replay must catch. That replay is a test that gates this
  change, so an author other than the implementer writes it.
- A review of this model against the intent and the code steps by a model of
  another family, before the model is frozen.
