# Sandbox reopen binding report

The independent binding matches model `fb7611789` against product code `095194ae6` on 128 seed-42 traces: **3,712 state comparisons pass**. Ten negative compiled plants turn replay red. Three model controls remain green because the daemon folder flock preserves `OneDaemon`. The fifth model review accepted this pin. A later AllEnd plant is not part of this report.

The binder wrote neither the product implementation nor the model. Production source remains unchanged by the binding. Each arm builds real first-party SDK, process backend, and daemon source with test-only checkpoints and explicit mutations. Every arm retains byte diffs, consumed-source hashes, build logs, worker/daemon hashes, commands, exit codes, and traces. `final-plant-table.json` supplies the per-arm artifact hashes.

## Final plant table

Each negative row runs the same unmutated model against a mutated compiled artifact. All negative builds succeed. Each exits 1 with `State invariant failed`; no compiler failure or tool failure counts as sensitivity. TLC separately finds each negative model witness.

| Compiled plant | Model property | Built-artifact result | Receipt directory |
| --- | --- | --- | --- |
| `no_kept` | NoOverlayLoss | RED, exit 1 | `followup-matrix-no_kept` |
| `no_guard` | StatusWaitsOutRestart | RED, exit 1 | `followup-matrix-no_guard` |
| `leak_guard` | GuardReleased | RED, exit 1 | `followup-matrix-leak_guard` |
| `boot_reclaims` | BootNeverDeletes | RED, exit 1 | `followup-matrix-boot_reclaims` |
| `no_lock` | OneDaemon | RED, exit 1 | `followup-matrix-no_lock` |
| `no_holder_name` | OrphanNamed | RED, exit 1 | `followup-matrix-no_holder_name` |
| `no_revived_report` | RevivedReported | RED, exit 1 | `followup-matrix-no_revived_report` |
| `status_no_wait` | StatusWaitsOutRestart | RED, exit 1 | `followup-matrix-status_no_wait` |
| `no_stale_check` | StatusRefusesUnusable | RED, exit 1 | `followup-matrix-no_stale_check` |
| `count_unattached` | ReopenedWhenDone | RED, exit 1 | `followup-matrix-count_unattached` |
| `no_running_check` | OneDaemon | GREEN control, exit 0 | `followup-probe-no-running` |
| `no_revive_check` | OneDaemon | GREEN control, exit 0; 32 traces, 928 comparisons | `followup-probe-revive-only` |
| `keep_failed_daemon` | OneDaemon | GREEN control, exit 0 | `followup-probe-keep-failed` |

The two physical positive controls establish `OneDaemon`, not complete mutant-state equivalence. `no_running_check` removes both running checks. `keep_failed_daemon` removes the cleanup fixed by `2f254db3c`. The daemon folder flock still rejects the second admission and names its holder.

`no_guard` removes both project and Sandbox flocks. `no_stale_check` removes the shared Hello instance check; the separate physical status probe establishes its status-specific effect. `count_unattached` restores the removed implicit branch-head lookup only in the mutant source copy. The observer reads the extra FUSE view's saved bytes.

The extra `lost_exit_diagnostic` regression plant reverts the fast-child-exit diagnostic. `followup-probe-fast-diagnostic-mutant` exits 1: the refusal fails to name holder PID 20. The unchanged artifact passes `followup-probe-fast`. This removes R4-B1 as a boundary.

## Commands and exit codes

Replace `RESOURCES` with the registered private resources file and `OUT` with a new directory. Run from the repository root. Full argument arrays, timing, and exits are retained in each directory's `commands.json`.

```sh
python3 quint/sandbox-reopen/run-replay.py --resources RESOURCES --out OUT --seed 42 --samples 128
python3 quint/sandbox-reopen/run-replay.py --resources RESOURCES --out OUT --plant NAME --seed 42 --samples 128
python3 quint/sandbox-reopen/run-replay.py --resources RESOURCES --out OUT --witness NAME
```

| Invocation | Exit | Result |
| --- | --- | --- |
| Baseline, `followup-fb-42-128` | 0 | 128 traces; 3,712 comparisons |
| Baseline after checkpoint lint fix, `final-fb-lint-base` | 0 | Same 128 traces; 3,712 comparisons |
| Each negative plant above | 1 | Intended model/code divergence |
| `no_stale_check` after checkpoint lint fix, `final-fb-lint-stale` | 1 | Intended divergence remains |
| Physical positive controls above | 0 | Intended OneDaemon control |
| `--witness fast-holder` | 0 | Rejected child exits before identity read; holder named |
| `--plant lost_exit_diagnostic --witness fast-holder` | 1 | Missing holder diagnostic |
| `--witness folder-lock-release` | 0 | Named orphan prevents second admission; later call serves overlay |
| `--witness fallback-failure` | 0 | Outer mount refusal carries `revived` |
| `--witness stale` | 0 | Real stale-instance refusal |
| `--fixture rootless` | 0 | Real UID-501 Prepare refusal settles |
| `--liveness` | 0 | Both locks release normally and after holder exit |
| `--witness kill-deadline` | 0 | Stop settles in 65.054562167 s; holder named |
| Worker Rust 1.85.1 Clippy, `-D warnings` | 0 | Final worker and included SDK copies |
| Driver stable Rust 1.93.0 Clippy, `-D warnings` | 0 | Driver; formatting also passes |
| Python `py_compile` | 0 | Four binding scripts |
| `check_fuser_integrity.py` | 0 | Authorized dependency integrity |
| Product-boundary base/current | 1 / 1 | Exact output bytes match; comparison exits 0 |

Compilation runs separately before every replay. Worker and daemon locked builds use Rust 1.85.1. The driver locked build uses exact Rust 1.99.1. Replay uses `timeout --kill-after=5s 95 ...`. No test invocation exceeds 120 seconds.

`final-checks.json` records final lint commands. The first worker Clippy run failed on a block in a generated match condition. The checkpoint insertion now uses a match-arm block. Its affected baseline and stale plant were rerun; no product logic changed. Earlier source versions remain in each arm's `binding-inputs.zip`.

### TLC

`followup-tlc/rows.json` records 24 independent `quint verify --backend tlc` invocations, each bounded by `timeout --kill-after=5s 90`. Eleven unplanted properties pass with exit 0. Ten negative plants return the expected counterexample with exit 1. Three positive controls pass with exit 0. Every row matches its expected verdict. No tool errors occurred in this final batch.

Earlier `final-tlc` history includes an aggregate 110-second timeout and one Apalache `AddressAlreadyInUse` on port 8822. The affected row succeeded on a distinct private port. Those errors remain history; they are not counterexamples. The final batch uses a distinct kernel-selected port per invocation. The coordinator's intermittent two-of-five `check.sh` tool errors were not reproduced here.

## Liveness checks

| Waiting state | Check and result | Receipt |
| --- | --- | --- |
| Project flock held | Waiter blocked, then enters within two seconds after release | `followup-probe-lock-release` |
| Sandbox flock held | Same check | `followup-probe-lock-release` |
| Project holder dies | Waiter enters within two seconds | `followup-probe-lock-release` |
| Sandbox holder dies | Same check | `followup-probe-lock-release` |
| Project lock deadline | Unchanged real 60 seconds returns Busy; successor enters after release | `deadline-project` |
| Sandbox lock deadline | Same check | `deadline-sandbox-clean-v2` |
| Status during restart | Real guard wait, reread, and completion occur in bounded replay | Baseline corpus; earlier physical status receipt |
| Child before admission | Readiness is bounded at three seconds; real admission is then released | Baseline corpus |
| Spawned/restarting | Real handshake, listener failure, or cleanup must settle under 95-second outer bound | Baseline corpus and failure probes |
| Failed stop/recovery | Eight before/after-publication stop/save combinations complete | Baseline corpus |
| Rootless refusal | Real Prepare EPERM settles and project guard releases | `followup-probe-rootless` |
| Orphan folder holder | Second daemon refused with holder PID; holder exits; distinct later call serves overlay | `followup-probe-orphan-release` |
| Rejected child exits early | Real backend returns holder diagnostic | `followup-probe-fast` |
| Post-KILL wait, old code | `d08e3b656`: exit 124 at 95.011267 s; RED hang | `post-kill-d08-red` |
| Post-KILL wait, first fix | `779d0447e`: exit 0; 65.048793 s; holder named | `post-kill-779-green` |
| Post-KILL wait, final code | `095194ae6`: exit 0; 65.054562167 s; holder named | `followup-probe-post-kill` |

The two 60-second lock checks run once per lock on their retained historical builds. Their lock source is unchanged; later fixture additions do not invalidate them. `deadline-sandbox` remains seal-rejected because its old seal hashed an unused model input. `deadline-sandbox-clean` failed before execution. The corrected consumed-input seal produced `deadline-sandbox-clean-v2`; neither failed receipt was relabeled.

Every checkpoint has a 95-second outer bound. Admission file waits have a 30-second bound. These checks establish completion for exercised waits; they do not prove arbitrary scheduler fairness.

## Model/code findings and traces

| Finding | Retained trace or probe | Resolution |
| --- | --- | --- |
| Overlay-less boot kept its folder; initial model deleted it | Early baseline seed traces in first archive | Model corrected to `ended`; reclaim alone deletes |
| List observed before guard; initial model always held it | Early list/reclaim replay | Model corrected |
| Reclaim after concurrent removal failed opening removed lock | Early reclaim replay | PR 19 fix, included in pinned code |
| Failed handshake left an unrecorded daemon | `physical-orphan-baseline` | PR 20 cleanup fix |
| `after_record` model retained daemon; code stopped it | Seed-42 history before model 73 | Model `73c1b5ca7` corrected |
| Failed stop saved new PID with old instance; status refused StaleSandbox | `model-73-baseline-42` | Revised model records unusable instance |
| Status after reclaim ended immediately; model added a guarded read | `model-73-absent-status-9` | Revised absent-record status path |
| Stop plus recovery-save failure left orphan; second revive admitted two daemons | `physical-failed-duplicate-mutant`, d08 failure probes | Daemon folder flock rejects second admission |
| Replacement start refusal lost failed-revive report | d08 replacement/fallback failure probe | Mount refusal now carries `revived` |
| Post-KILL stop waited without a bound | `post-kill-d08-red` | Real 60-second post-KILL bound |
| Fast-exiting rejected child lost holder diagnostic | Final fast-holder pair | Code `095194ae6` passes; reverted artifact fails |
| Post-publication stop/recovery failure needed separate outcomes | `followup-fb-42-128` | Model `fb7611789`; all eight combinations exercised |

No unresolved baseline divergence remains in the final retained corpus. This does not claim equivalence for all executions.

Harness failures remain separate: pre-exec child observation, fallback-runtime list aliasing, root-observer ownership mismatch, and an early FUSE mount-count check. The final observer waits for actual exec readiness, filters the original target, uses the same UID, and reads saved bytes through FUSE. Earlier failures and source versions remain archived.

## Boundaries and projections

- Scheduler phases stop at real entry, observation, guard, decision, spawn, admission, and return checkpoints.
- Kernel probes establish real flocks. No scheduling-only phase invents ownership.
- `children` represents raw processes; `live` represents admitted daemons. Actual PID/start and folder-flock fdinfo remain evidence.
- Incarnation numbers map observed process identities. They do not stand in for liveness checks.
- `overlays` and `eligible` select fixture records. Served views require the saved payload at the saved path.
- `unusable` uses an extra read-only Hello connection to compare the daemon instance with its saved record.
- Target outcome is captured at actual revive return, separately from the enclosing mount result.
- Target observation excludes a replacement runtime. Failed-target mount still executes real fallback start, mount, and response/refusal construction.
- Successful ordinary mount stops after target placement. New Workspace allocation is outside the model comparison.
- Status exercises `registry::sandbox`; later Workspace status is outside the comparison.
- Listing membership, every Workspace reason, and full revived-item fields are outside the comparison.
- The valid replay adapter omits model timeout transitions. Separate real lock-deadline probes cover primitive Busy, not endpoint-specific Busy mapping.
- Physical positive controls establish OneDaemon only. They do not claim full mutant-state equivalence.
- Bounded trace replay is sampled. TLC establishes the bounded model, not unbounded native execution.
- The later AllEnd plant is unrun because it is outside accepted model pin `fb7611789`.

## Evidence custody and source size

`d08-73-history.tar.gz` retains the first 85 result directories. `final-history.tar.gz` retains 65 later directories, including rejected intermediate runs and the final table. The generated indexes record hashes; every archived result file was read back and hash-checked. Extract an archive into a temporary directory to read the named receipts.

No live Sandbox, rescue path, or deployment was used. Runs use fresh private Docker containers and temporary volumes. The supervising process cleans its own daemons and mounts. Durable Store execution is NOT_RUN: disabled by owner.

The coordinator approved the separate test-only driver toolchain. `quint-connect` 0.1.2 uses let chains in `value/display.rs:85` despite its README. Exact Rust 1.85.1 failures are retained. Delete the separate pin when product Rust reaches at least 1.88. Product Rust remains 1.85.1.

Final binding commit production LOC: **110426 -> 110426 (delta +0)**. Scope: core 97,921; SDK 3,774; dsh 8,731. Counter: unchanged `tools/production_loc.py --json`, exact first-parent versus staged tree. Test crates, generated copies, tools, and receipts are excluded. Reference absent (retired R9c; last counted 65,417 at tree `498dd1917812`). Merge comparisons are recorded in the commit messages and `loc-*-staged.json` receipts.
