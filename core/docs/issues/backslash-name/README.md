# A name with a backslash answers EINVAL instead of ENOENT

> **Status:** Open, filed 2026-11-14. GitHub issues are disabled on the fork,
> so the issue lives here. Reported by memoryd: work item
> https://gitlab.com/joshka0/memoryd/-/work_items/4 and
> `zig/daemon-next/tests/BASELINE.md` (WT6).

## Summary

In a mounted Workspace (file system `fuseblk`), `stat` and `mkdir` of any
name that holds a backslash (`\`) fail with `EINVAL`, also when the name does
not exist. A plain Linux file system answers `ENOENT` for a missing name and
creates the folder for `mkdir`. A backslash is a valid byte in a Linux file
name. Other bytes behave: `"` and `é` answer `ENOENT` as expected.

## Reproduction

In a mounted Workspace on lfs-orb (measured by the memoryd agent,
2026-11-14):

```sh
cd <Workspace path>
python3 -c 'import os; os.stat("a\\b")'    # OSError errno 22 (EINVAL)
cd /tmp
python3 -c 'import os; os.stat("a\\b")'    # FileNotFoundError errno 2 (ENOENT)
```

Same evidence at a larger scale: the same compiled memoryd `worktree_test`
binary fails test WT6 when run from a Workspace (8 passed, 1 failed) and
passes all 9 from `/tmp`. WT6 makes the folder `q"b\é/x` under the
Workspace's `.zig-cache/tmp`.

## Likely cause

The name grammar refuses the byte. `validate_name` in
`core/crates/layerfs-content/src/filesystem/path.rs:169-188` returns
`ContentError::InvalidPath` for a name that contains `\` (line 183), and
`InvalidUtf8` for a name that is not UTF-8. The kernel request path then
answers `EINVAL`. The rule came with commit `07f0fe8eb` (2026-09-17, "build,
update and read filesystem trees against the reference grammar"). This cause
comes from reading the code; it is not yet confirmed by a test.

## What a fix decides

The grammar is part of the canonical tree format, so a fix is a format
decision, not a local change:

1. Accept `\` in a name (Linux allows every byte except `/` and NUL). Check
   whether stored roots, digests or a reference (Windows) reader depend on the
   refusal.
2. Decide the same for names that are not UTF-8: a Linux file system accepts
   them, and the same refusal would answer `EINVAL` for them.
3. Until a fix, a refused name in a lookup should answer `ENOENT`, not
   `EINVAL`: it cannot exist in the Workspace. `mkdir` of such a name still
   cannot succeed.

## Impact

Any tool that probes or creates a path with a backslash under a Workspace
fails: memoryd's WT6 today, and real transcripts whose start folder holds a
backslash.
