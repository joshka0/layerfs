# Reopen trace replay

> Status: Independent binding of model `fb7611789` to code `095194ae6`. Historical receipts keep their original identities.

## Real execution

The driver uses `quint-connect` 0.1.2. The Linux worker builds with Rust 1.85.1.
Each boot, mount, list, and status caller runs in a separate process.
The worker calls the SDK functions. Linux supplies the project and Sandbox flocks.
A separate open description probes the project flock. Concrete liveness probes exercise both lock scopes.

`instrument.py` copies first-party source and inserts checkpoints. Every run saves the full byte diff and both source hashes.
File mounts supply the copied process backend and daemon CLI during compilation. Instrumentation does not edit product source.
The worker crate is outside the product workspace. Product builds do not need generated replay files.
The separate driver toolchain and its removal condition are recorded in [the dependency reference](../../core/docs/references/quint-connect.md).

Each fixture creates a Disposable Store through real project Init, forks a Branch, starts the real daemon, and mounts real Workspaces.
It writes uncommitted bytes through FUSE, saves the records, and kills the known seed daemon.
The replay must serve those same bytes from the saved overlays at the saved paths.
An entry in `mountinfo` alone does not count: an ended FUSE mount returns `ENOTCONN`.

## Model and code boundaries

These projections define the binding's scope. They do not change the model or product decisions.

- The caller phases describe scheduling at real entry, guard, decision, spawn, daemon admission, and return checkpoints.
- `children` counts raw child processes before admission. `live` counts those past the real daemon admission check. Raw identities remain in the receipt.
- File checkpoints pause the daemon before and after its real folder-flock call. Kernel `fdinfo` confirms ownership separately; no flock is invented.
- The pre-flock readiness wait is bounded at three seconds. It avoids reading the child before its `exec` replaces the command line.
- A stale plan includes the real protected Hello lookup. The failed lookup is paused before the revive returns.
- `unusable` compares an observed Hello instance with the saved record after an actual failed stop/recovery. The observer uses an extra read-only Hello connection.
- `overlays` and `eligible` are fixture inputs. Each ineligible record lacks its saved base and receives no implicit legacy-head permission.
- `plan` comes from the real revive or reclaim decision. A `none` decision may end target placement without a filesystem mutation.
- Incarnation numbers map observed PID/start identities. They do not replace a process-liveness test.
- The target observer counts daemons and served saved Workspaces in the original runtime folder. It excludes a replacement runtime.
- Successful ordinary mount stops after target placement. New Workspace allocation on that successful path lies outside the model.
- After a target revive fails, mount executes the real replacement Sandbox start, Workspace mount, and `revived` response construction.
- The target revive outcome is captured at its real return. It is distinct from the enclosing mount result.
- Status drives `registry::sandbox`, the guarded Sandbox lookup shared by Workspace calls. It does not drive the later Workspace status request.
- `reported` observes whether a successful response or refusal detail includes a failed revive item. The model does not check every item field or reason.
- The list result does not model membership. Independent E2E evidence must establish which Sandboxes are listed.
- Replay schedules omit the model's timeout transition. Separate probes test the unchanged real 60-second deadline, once per lock.

No-op transitions schedule a caller. They never assert ownership of a flock that the kernel probe did not find.
All model state fields are deserialized with unknown fields denied. The driver compares each state through `quint-connect`.

## Failure inputs and plants

A handshake failure uses a wrong Noise controller key. A listener failure writes a malformed listen line before the real listener check.
A post-record failure marks the target `container` inode immutable. The real rename fails after `sandbox.record` is saved.
A stop failure uses a process-local seccomp filter: the real `kill` syscall returns `EPERM`.
A failed recovery save marks only `sandbox.record` immutable. For a post-publication failure, this happens after the real publication.
The fault flags are removed at the failure return. The written record, daemon state, and returned refusal remain as the product left them.
This limits the injected failure to that operation; replacement placement remains an independent model input.
Preparation refusal uses a temporary mount and a real `umount2` syscall refused with `EPERM`. A separate fixture runs as UID 501.
The supervising process remains able to clean up its own fixture.
The startup and lock deadlines stay unchanged.

Each negative plant changes only the compiled first-party test copy. Its full diff and artifact hash are retained.
`no_guard` disables both project and Sandbox flock acquisition, as required by the model's compound mutation.
`no_running_check` removes both running checks. The daemon folder flock now makes this a positive `OneDaemon` control.
Removing only the revive check and retaining a failed daemon are also positive `OneDaemon` controls.
`no_lock` removes the daemon admission flock; `no_holder_name` removes its holder diagnostic.
`no_stale_check` removes the Hello instance check in the shared connection shell. Its status-specific effect has a separate physical probe.
`keep_failed_daemon` removes failure cleanup and restores the orphan risk fixed by `2f254db3c`.
`count_unattached` restores an implicit branch-head fallback in the compiled test copy. A record that should refuse actually reattaches.
The removed head-lookup helper returns only as part of this deliberate test mutation; product source has no compatibility path.
The observer then sees an extra served FUSE view; it does not trust an invented mounted count.

Physical probes continue beyond an early model decision mismatch to prove deletion, duplicate daemons, and held flocks.
A tool error, compile error, timeout, or unrelated model mismatch is not plant sensitivity evidence.

## Commands and receipts

From the repository root, use registered private worktree resources:

```sh
python3 quint/sandbox-reopen/instrument.py
python3 quint/sandbox-reopen/build-worker.py --resources /absolute/path/resources.json --resolve
python3 quint/sandbox-reopen/run-replay.py --resources /absolute/path/resources.json --out /absolute/path/new-result --seed 3
```

The output directory must be new. `commands.json` records commands and exit codes.
`identity.json`, binary hashes, source manifests, and ITF traces bind observations to each build.
`binding-inputs.zip` preserves the consumed binding/model files. Generated source hashes are checked before and after execution.
The seal hashes only inputs the selected probe consumes. Non-model probes do not consume model files.
Compilation runs first. Each test has a 95-second deadline and five seconds for cancellation.
Each run mounts a private target volume and a new disposable `/tmp` volume.
It mounts no live workbench Sandbox or rescue data.

Use `--plant NAME` for a compiled plant, `--witness NAME` for a concrete effect, and `--liveness` for lock release/crash probes.
Use `--busy project` and `--busy sandbox` separately for the unchanged deadlines.
Use `--fixture missing-base`, `--fixture handshake-failure`, or `--fixture rootless` for concrete failure fixtures.
Use `--witness fast-holder` to force the rejected child to exit before the backend reads its identity.
The extra `lost_exit_diagnostic` regression control must make that probe red.

## Liveness

The concrete probes check both project and Sandbox flocks:

- A waiter stays blocked while the holder owns the lock.
- The waiter enters within two seconds after normal release.
- The waiter enters within two seconds after the holder process exits.
- The unchanged 60-second deadline returns `Busy` while the holder stays alive.
- A successor enters after the holder releases the lock following that deadline.

Every scheduled checkpoint has the invocation's outer deadline. Spawned states are released into real handshake or failure cleanup.
The status race probe observes the paused restart, releases it, and requires status to settle after its guarded reread.
Failure probes require the call to settle, release the project guard, and preserve the measured daemon custody.
The orphan-holder probe starts a distinct call after the holder exits and requires the saved overlay to serve again.
The post-KILL probe acknowledges TERM and KILL without delivering them. The real five-second TERM wait and 60-second KILL wait run unchanged.
On `d08e3b656`, it hit the 95-second ceiling. The bounded implementation returns after about 65 seconds and names the live holder.
No automatic operation retry is added.

The deadline receipts are `deadline-project` and `deadline-sandbox-clean-v2`.
They precede later fixture extensions; their unchanged guard inputs can be compared through the manifests.
`deadline-sandbox` stays rejected: its old seal included an unused model hash that changed during execution.
`deadline-sandbox-clean` failed to build before execution. Neither failed receipt is relabeled as passing.

## Findings

The receipts preserve these model/code boundaries and their traces:

- Boot on overlay-less records kept the folder and answered `ended`; the original model deleted it. The model was corrected.
- Read-only list paths took no guard; the original model acquired one. The model was corrected.
- Reclaim after a concurrent deletion failed at the removed Sandbox lock file. PR 19 repaired the real code.
- Failed handshake cleanup left an unrecorded live daemon. PR 20 repaired the real code.
- At `fae7d3929`, `after_record` kept a daemon that real failure cleanup stopped. Seed 42 reproduced it; `73c1b5ca7` corrected that model state.
- At `73c1b5ca7`, a failed stop saved the new PID but retained the old daemon instance. A later status returned `StaleSandbox`; the model expected success.
- At `73c1b5ca7`, status after reclamation returned `NoSuchSandbox` immediately. The model scheduled an additional guarded observation. Seed 9 reproduced it.

The report must distinguish matched traces, model findings, and physical plant effects. A partial trace corpus is not a global model/code PASS.

The double-failure probe reproduced an unrecorded live daemon when both stopping and recovery save failed.
The replacement-failure probe refused `execve` after target cleanup; the outer mount refused without `revived` on `d08e3b656`.
The folder-flock, refusal-reporting, and fast-exit fixes are bound at `095194ae6`.
Model `fb7611789` also represents post-publication stop failure with either recovery-save result. All eight failure/stop/save combinations occur in the retained corpus.

## Remaining scope

The bounded replay is not exhaustive execution. TLC checks the bounded model; native replay checks the retained schedules and injected failures.
Timeout actions are not sampled. Endpoint-specific `Busy` response mapping is outside the isolated lock-deadline probes.
Listing membership, complete Workspace reasons, new allocation after successful placement, and the later Workspace status request remain outside the state comparison.
Positive `OneDaemon` physical controls check that property; they do not assert complete mutant-state equivalence.
The fast-exit probe covers the repaired pre-identity branch without the model admission pause.
The requested later AllEnd mutation is outside the `fb7611789` pin and has no replay receipt here.
