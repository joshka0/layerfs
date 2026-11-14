# 0006: A backslash is an ordinary name byte

- Date: 2026-11-14
- Status: accepted
- Topic: layerfs-name-grammar

## Context

The canonical name grammar (`validate_name`,
`core/crates/layerfs-content/src/filesystem/path.rs`) refused a name that
holds `\` and a name that is not UTF-8. It came with `07f0fe8eb`
(2026-09-17) to match the v0.1.6 reference grammar; that reference was
retired in R9c.

Linux allows every byte in a name except `/` and NUL. In a mounted
Workspace, `stat` and `mkdir` of a name with a backslash answered `EINVAL`.
memoryd's test WT6 makes the folder `q"b\é/x` and failed inside a Workspace
(core/docs/issues/backslash-name).

Every check of a name goes through `PathName::from_bytes`: the FUSE name
check, the API path check, Init (`import/scan.rs`, `import/namespace.rs`),
the directory page decoders (`sorted/format.rs`), the path reader
(`read.rs`) and the reference cursor (`references/indexed_wire.rs`). Init
refused such names, so no Store holds one.

## Options

1. Accept a backslash; keep refusing names that are not UTF-8.
2. Also accept names that are not UTF-8. Names are used as text in 49 places
   across merge, the store service, the API and the SDK, so this is a format
   and API change.
3. Keep the grammar; answer `ENOENT` for a lookup of a refused name. Creating
   such a file still fails.

## Choice

Option 1 (operator, 2026-11-14). `validate_name` no longer refuses `\`. The
encoder, the decoders and Init change together, because they share the one
check.

## Consequences

- A Workspace, Init and the API accept names with a backslash, as Linux
  does. Names that are not UTF-8 still answer `EINVAL` (`InvalidUtf8`).
- No encoded object changes: a name's bytes and its byte order are the same.
  Existing Stores hold no backslash name, so their roots and digests stay
  the same. Evidence: a Store built before and after the change from the
  same source yields the same root (see the PR receipt).
- A Store with a backslash name cannot be read by a build from before this
  change: its decoder refuses the page.
- Faithful raw Unix names (non-UTF-8) remain open as a separate decision.
