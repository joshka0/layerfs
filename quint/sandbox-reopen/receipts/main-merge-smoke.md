# Main merge smoke

The binding branch merges `origin/main` at `13df7f874` (model PR 26, `68ced0c90`). The local merge completes without conflicts. Main's model, model README, `plant_stuck_stop.qnt`, and `check.sh` remain byte-identical. Binding source remains unchanged.

| Check | Exit | Result |
| --- | --- | --- |
| Seed 42, first trace, merged model | 0 | One trace; 29 state comparisons pass |
| Seed 42, first trace, compiled `no_guard` | 1 | Intended `State invariant failed` at trace step 2 (`acquire mount`); three comparisons include init |
| Binding diff whitespace check against main | 0 | No binding whitespace errors |
| First-parent staged whitespace check | 2 | Existing trailing whitespace in six main receipt files; bytes preserved |

The private caches were already removed at handoff. Dependency preparation therefore runs explicitly before fresh locked Rust builds. Compilation completes before each replay. Each replay retains its unchanged 95-second outer deadline plus five seconds for cancellation. No live Sandbox or rescue data is used.

`main-merge-smoke-commands.json` records the exact replay commands and exits. `main-merge-smoke-inputs.json` records merge pins and main-file equality. The hash-checked `main-merge-smoke.tar.gz` preserves both runs: build logs, worker/daemon hashes, consumed inputs, byte diffs, ITF traces, and raw comparison output. `main-merge-diff-check.json` records the inherited whitespace baseline.

The full plant table remains scoped to accepted model `fb7611789`. This short smoke confirms compatibility with the merged model. It does not claim a new full corpus or compiled `stuck_stop` plant run. The historical unbounded-stop RED and bounded-stop GREEN receipts remain unchanged.

Production LOC: 110426 -> 110426 (delta +0), exact first-parent/staged snapshots using unchanged `tools/production_loc.py --json`. Core 97,921; SDK 3,774; dsh 8,731. Tests, docs, tools, and fixtures are excluded. Reference absent (retired R9c; last counted 65,417 at tree `498dd1917812`). `main-merge-loc.json` records both snapshots.
