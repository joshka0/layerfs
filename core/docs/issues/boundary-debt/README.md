# The 19 product boundary violations on main

> **Status:** Open, failure list written before the code, 2026-11-18 (task
> jos-layerfs-boundary-ufv). Assigned by ash-coordinator.

## What the guard reports

On main (`85af44ba7`), `python3 -B core/tools/check_product_boundary.py`
reports 19 violations:

- 13 lines in `core/crates/layerfs-sandbox/src/backend/mod.rs`: "implementation
  construct in declaration/delegation entry file". The entry file holds the
  `Runtime` and `Failure` types and their `impl` blocks.
- 6 in `sdk/js/native/src`: "test-only attribute in product source". These are
  an inline `#[cfg(test)] mod tests` in `discovery/lock.rs` (lines 97 and 125),
  and `#[cfg(test)] #[path = "../tests/unit/..."]` includes in `pool.rs`,
  `workspace.rs`, `sandbox/reclaim.rs` and `discovery/revive.rs`.

The guard has no baseline: its target is zero, and `b40da4616` cleared 55
violations the same way this list asks. `core/AGENTS.md` ("Product source and
external verification") forbids inline tests, test-only cfg and test-only
public APIs in product source, and forbids redirecting target paths into test
directories. `layerfs-node` is a `cdylib`, so no Rust test outside its source
can reach it. `b40da4616` left these items "for a separate decision".

## Failures

B1. `backend/mod.rs` still holds an implementation construct. Expected: it
    declares modules and re-exports only. `Runtime`, `Failure` and their
    `impl` blocks move unchanged into `backend/runtime.rs`.
B2. B1 changes a caller's path or behavior: `layerfs_sandbox::backend::Runtime`
    or `backend::Failure` no longer resolve, or a body changes. Expected: the
    same paths through a re-export; every workspace crate and the SDK compile;
    the `layerfs-sandbox` tests pass as before.
B3. An SDK test-only attribute stays in product source. Expected: each test
    moves to where the rules allow it. That is a JS test through the SDK
    (`sdk/js/test`) for behavior a consumer sees, or a package `tests/` file
    of a real library crate for a pure rule that moves there as product API
    (as `layerfs_bridge::mount_rules` did in lost-mount). This needs a
    decision for each test; see "Decision" below.
B4. A move for B3 drops a check: an assertion that held before has no
    counterpart after. Expected: every moved test keeps its cases; a case
    that cannot move is named in the commit, not dropped in silence.
B5. The guard's count rises, or a new kind of violation appears. Expected:
    the count falls to 6 after B1, and to 0 after B3.

## Decision (B3)

Decided 2026-11-18 by ash-coordinator, under the house rule that a gate red on
main gets a baseline file the gate diffs against, plus a task for the debt.
`check_product_boundary.py` diffs against a baseline of exactly the six SDK
items. The port of the 14 SDK tests to `sdk/js/test` is its own task.

B6. A violation that the baseline does not list passes. Expected: the guard
    fails and prints it, as before.
B7. A baseline entry that no longer occurs passes in silence. Expected: the
    guard fails and names the stale entry, so the baseline only shrinks: a fix
    removes its entry in the same change.
B8. The baseline keys on line numbers, so an edit above a listed item breaks
    the guard; or it keys so loosely that a second violation of the same kind
    in the same file is absorbed. Expected: an entry is the file, the reason
    and the count of that pair; more than the count fails (B6), fewer fails
    (B7).
B9. The guard's self-tests (`core/tools/test_check_product_boundary.py`) do
    not cover B6-B8, or the baseline file is missing or malformed and the
    guard passes. Expected: a missing or malformed baseline fails with its
    path.
