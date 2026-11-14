# Reopen gate

The gate for reopen after restart: `core/docs/issues/373/REOPEN.md`, failure
list F1-F40, plus the cases F41-F47, R4-B1 and N1-N3 that the operator
added on 2026-11-14. An independent test author wrote these tests from the
failure list and the public interfaces (SDK, operator, `lfs`, `vm-boot.sh`,
daemon control), not from the implementation.

## Run the gate

1. Build a bundle (`layerfs-daemon` and `layerfs.node`) of the commit under
   test. `tools/orbstack/process-e2e.sh <work-dir>` writes one to
   `<work-dir>/bundle`.
2. Get the bundle from before reopen existed, and the `sdk/js` tree of that
   time: the F32 cases seed Workspace records without `root` with them. The
   bundle of 2026-10-22 is `~/.local/share/lfs-vm/bundle-backup-2026-10-22`
   (daemon sha256 `3ab3e2f3…`); the SDK is `git archive 49f27c532 sdk/js`.
3. Run:

   ```
   tools/orbstack/reopen-e2e.sh <work-dir> <bundle-dir> <legacy-bundle-dir> <legacy-sdk-js-dir>
   ```

4. Read `<work-dir>/report/summary.txt`. The script exits 0 only when no
   check has `FAIL`.

To run only phases that stand alone, set `REOPEN_E2E_PHASES`, for example
`REOPEN_E2E_PHASES="races lock holder"`. To name the commit of an exported
tree (`git archive`), set `REOPEN_E2E_PRODUCT`.

The script runs each phase in its own container on two named volumes,
laid out as in lfs-orb (`/var/lib/layerfs-data`, `/var/lib/layerfs`), and
removes the container between phases. That removal is the restart: every
daemon and FUSE mount ends, every file stays. The volumes are removed at
the end. The tests refuse to run outside such a container. Never point them
at a live workbench.

The script refuses a legacy bundle that is the bundle under test: a
redeployed lfs-orb bundle seeds records with `root`, and F32 then fails
falsely.

## Phases

| Phase | File | What it covers |
| --- | --- | --- |
| `seed-legacy` | `seed.mjs` | Records without `root`, from the old SDK (F32) |
| `seed` | `seed.mjs` | Saved Workspaces for every later check |
| `tamper` | `seed.mjs` | Overlay files and branches changed while no daemon runs (F1-F5, F31) |
| `check` | `check.mjs` | Discovery, reopen, use, commit and unmount of each project (F1-F38) |
| `boot-unmounted` | `boot.mjs` | `vm-boot.sh apply` without the Sandbox volume (F40) |
| `boot` | `boot.mjs` | `vm-boot.sh apply` after a second restart (F32, F39) |
| `live` | `live.mjs` | SIGKILL in one container, dead FUSE mounts, running daemons, races (F13, F21-F25, F30, F34, F35) |
| `races` | `races.mjs` | Listings and status during a restart, failed restarts, failed revives (F41-F44) |
| `names` | `names.mjs` | Backslash names and non-UTF-8 names (N1-N3, ADR 0006) |
| `lock` | `lock.mjs` | One daemon per Sandbox folder, stop, failed mount (F45-F47) |
| `holder` | `holder.mjs` | 1000 revives against a held folder (R4-B1) |

`harness.mjs` holds the shared helpers; `main.mjs` selects a phase.

## Daemon and unit tests

Build and run them in `rust:1.85.1-bookworm`, with a timeout of 120 s:

```
cargo test --manifest-path core/Cargo.toml --offline --locked -p layerfs-daemon --test reopen_control
cargo test --manifest-path core/Cargo.toml --offline --locked -p layerfs-sandbox --test stop_bound
```

- `core/crates/layerfs-daemon/tests/reopen_control.rs`: the `Reopen`
  control request (F1, F11-F16).
- `core/crates/layerfs-sandbox/tests/stop_bound.rs`: the 60 s bound of a
  daemon stop on an injected clock (F46).

## Known failures and gaps

- `baseline.tsv` lists failures the gate accepts: a check name, a regex its
  reason must match, and the issue. A match prints `BASELINE`; any other
  failure of that check stays `FAIL`. The entries cover the unmount `Join`
  fault (`core/docs/issues/unmount-join/README.md`). Remove an entry when its
  fault is fixed; the report lists unused entries as `BASELINE-UNUSED`.
- F29 (a Docker Sandbox found gone) is not covered: the gate has no Docker
  Sandbox.
- F46 end to end covers only the reachable stop path. The 60 s bound is
  covered by `stop_bound.rs`.
- `NOTE outside-list` lines report a fault outside the failure list and do
  not gate: `core/docs/issues/sdk-stale-daemon/README.md`.
- The F41 restart window and R4-B1 depend on timing. F41 holds the window
  open with a daemon that delays its listen line; R4-B1 runs 1000 rounds.
  Both were red on every run before their fixes.

## Receipts

`receipts/` holds the summary of each gate run: the product commit, the
bundle hashes, and every `PASS`, `FAIL`, `BASELINE` and `NOTE` line.
