# quint-connect 0.1.2

> Status: First-use research, 2026-11-14. Test dependency only.

## Sources

- [Published crate](https://crates.io/crates/quint-connect/0.1.2): Apache-2.0; version 0.1.2, published 2026-05-25.
- [Versioned API](https://docs.rs/quint-connect/0.1.2/quint_connect/): `Driver::step` runs each selected action.
- [Driver source](https://docs.rs/quint-connect/0.1.2/src/quint_connect/driver/mod.rs.html): `Config` selects state and nondeterministic picks.
- [State source](https://docs.rs/quint-connect/0.1.2/src/quint_connect/driver/state.rs.html): `State` requires `DeserializeOwned`, `PartialEq`, and `Debug`.
- [Published source archive](https://crates.io/api/v1/crates/quint-connect/0.1.2/download): read `Cargo.toml`, `src/runner/mod.rs`, `src/driver/step.rs`, and `src/trace/generator/`.
- [Quint model-based testing](https://quint.sh/docs/model-based-testing): `--mbt` records actions and nondeterministic choices in ITF traces.

Exa found the versioned docs and crate page. Perplexity returned the same primary sources.
The published source archive resolves the API details below.

## Replay API

`quint_run` generates traces with `quint run --mbt`. `quint_test` runs named model tests.
`runner::run_test` takes a `Driver` and `RunConfig` or `TestConfig`.
The runner derives each `Step`, calls the driver, then compares the extracted states.
A failed action, invalid trace, deserialization failure, or state mismatch returns an error.
A driver with `State = ()` disables state checking. This binding must compare concrete observations.

`Step::action_taken` names the selected action. `nondet_picks.get(name)` holds its chosen value.
`switch!` deserializes those values. The names must match the model's nondeterministic variables.
Quint action parameters alone are not a process scheduler.

The trace runner does not pause threads, intercept `flock`, or infer implementation state.
The binding must coordinate workers at real function boundaries and read actual files and process identities.
It must preserve the production lock acquisition and protected recheck.

## Limits

The crate uses Rust edition 2024. Its README's Rust 1.70 claim is stale for this archive.
The archive also uses let chains in `src/value/display.rs:85`, which require Rust 1.88.
A locked native test build on Rust 1.85.1 exits 101 before compilation:
`darling` 0.24.1 and `serde_with` 3.24.0 require Rust 1.88.
The exact error is retained in `quint/sandbox-reopen/receipts/rust-1.85.1-dependency-error.txt`.

An unchanged published `src/value/display.rs` was also passed directly to Rust 1.85.1.
Rust reported `E0658` at lines 85 and 86: `` `let` expressions in this position are unstable ``.
That standalone check also lacks the crate's `itf` import; it is not a complete crate build.
Its exact command and errors are retained in `rust-1.85.1-published-display-command.json`
and `rust-1.85.1-published-display-error.txt` in the same receipts directory.

The coordinator approved a separate test crate on an exact newer toolchain.
The driver pins Rust 1.99.1, [released 2026-10-15](https://blog.rust-lang.org/releases/).
The worker and product remain on Rust 1.85.1. No third-party source is patched.
Remove the separate toolchain when the product toolchain reaches Rust 1.88 or newer.
The crate launches `quint` from PATH. It provides no command timeout.
Each replay invocation therefore needs an outer wall timeout of at most 120 seconds.

Random replay supplements TLC. It does not prove all schedules or process failures.
The binding must retain seeds, traces, source identities, and separate plant results.
A timeout, unavailable environment, or unmatched model action cannot count as a product PASS.
