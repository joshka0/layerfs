# Store round-trip, before and after the backslash change (2026-11-14)

Bundles (rust:1.85.1-bookworm, release, separate cargo targets):
- before: main 34d57a297: layerfs-daemon 584c69c7..., layerfs.node 2c15bae8...
- after: fix-backslash-name 63d0342a0: layerfs-daemon 3d2829d2..., layerfs.node 20ce2dc2...
Source: core/ of main 34d57a297 (4105 files). SDK: sdk/js of the branch. One node:24-bookworm
container; each run has its own SDK home and Store root. Script and runner: roundtrip.mjs, run.sh here.

| run | build | init root | paths | read | refused | digest of all read answers |
| --- | --- | --- | --- | --- | --- | --- |
| old-p: init, fork w, read w | before | 230beff61124e381... | 1956 | 1674 | 282 | 4cc6328a49fa68a3... |
| old-p2: the same, fresh home | before | 1cb5c9ce2f8b6cdf... | 1956 | 1674 | 282 | 95db9f6c95a17293... |
| new-p: the same, fresh home | after | b9bd2da189428d5f... | 1956 | 1674 | 282 | 700eeadfce0ef4e3... |
| new-reads-old: read old-p Store | after | (reads only) | 1956 | 1674 | 282 | 4cc6328a49fa68a3... |

- The after build reads the before build's Store with identical answers for all 1956 paths
  (per-path answers equal: True), and the same branches, base layer and fork point
  (equal: True; base layer 32a20d3b309dbd44cc700a0a5119ed28670d3521425b5d1348bda3cda358dcac13).
- Init roots differ between two runs of the same before build (old-p, old-p2): Init draws a random
  scope seed, so Init roots are no before/after comparison. Same-tree-same-root is checked by the
  sealed reference parity tests of layerfs-content (tests/filesystem_reference.rs), which pass on the after build.

Backslash source (files `a\b`, `dir/q"b\é`, `plain.txt`):
- before: Init refused, error Project(Content(InvalidPath))
- after: Init root 03af53e4024e11534118f4c032e47021ce2db2b2b5a66959da3a77bcd275399b; read 3 files, paths ['/a\\b', '/dir', '/dir/q"b\\é', '/plain.txt']
