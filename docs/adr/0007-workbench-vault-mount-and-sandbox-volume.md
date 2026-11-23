# 0007: The workbench writes the memory vault, and a recreate can keep its Sandboxes

- Date: 2026-11-23
- Status: accepted
- Topic: layerfs-container-port

## Context

memoryd runs in `lfs-orb` (memoryd ADR 0069). Its Obsidian mirror writes
`topics`, `sessions`, `scopes`, `repos` and the index note of the vault
`~/vaults/fleet-memory`, and it refuses a vault path that holds a symbolic
link. `lfs-orb` mounted only parts of the vault: `summaries` and `features`
read-only at their Mac paths, and four folders under `/vaults`.

A mount is a `docker run` flag, so a new mount needs a new container.
`tools/orbstack/lfs-vm.sh` gave each container an anonymous volume at
`/var/lib/layerfs`. On 2026-11-23 that volume of `lfs-orb` held 46 overlay
files (18 GB): the uncommitted state of 26 mounted Workspaces. A recreate
deleted them. A restart keeps them, and `vm-boot.sh` reopens the Workspaces
(`core/docs/issues/373/REOPEN.md`).

## Options

Vault:

1. Mount the whole vault read-write at its Mac path, with read-only mounts
   on top of the folders the mirror does not write.
2. The mirror writes a staging folder, and a Mac job copies it into the
   vault.
3. Keep the mirror on the Mac.

Sandboxes at a recreate:

1. `LFS_SANDBOX_VOLUME` names the volume of the replaced container; the
   default stays an anonymous volume.
2. Start fresh Sandboxes and lose the uncommitted changes.
3. Close each task before the recreate.

## Choice

The operator chose option 1 for both on 2026-11-23 ("Allow RW vault mount",
"Carry it over", "Yes to both").

- `lfs-vm.sh` mounts `$LFS_VAULT` read-write at its Mac path in `lfs-orb`.
- Read-only on top: `summaries` and `features` (the memoryd mounts), and
  `router` and `.obsidian`.
- `LFS_SANDBOX_VOLUME=<volume>` mounts that volume at `/var/lib/layerfs`.
  `lfs-vm.sh` stops with exit 66 when the volume does not exist.

## Consequences

- The mirror runs as uid 501, as every agent does. Each agent in `lfs-orb`
  can now read the generated memory pages and write the vault, except the
  four read-only folders.
- The mounts under `/vaults` do not change: agents write feature notes,
  diagrams and this year's summaries there.
- A recreate with `LFS_SANDBOX_VOLUME` equals a restart for LayerFS: the
  limits of REOPEN.md apply (a Workspace that committed after its last
  reopen does not reopen; its overlay stays).
- `docker rm -v` does not remove a volume that a container mounts by name.
  `docker volume prune` removes it when no container uses it: keep one
  container on it.
- Only `lfs-orb` gets the vault mount. `LFS_SANDBOX_VOLUME` works for each
  workbench.
