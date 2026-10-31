# 0004: OrbStack is the only workbench runtime

- Date: 2026-10-31
- Status: accepted
- Topic: layerfs-container-port
- Replaces: the choice of ADR 0002 (two runtimes)

## Context

ADR 0002 added the OrbStack workbench `lfs-orb` beside the Apple `container`
workbench `lfs-vm`, and chose to support both.

On 2026-10-31 the operator retired Apple `container` on the Mac. OrbStack is
the only container runtime there. SPACE microVMs run only on the dev box.
Agents use `lfs-orb`. `lfs-vm` is retired, and its Apple images are
deleted.

The Apple path cost code: a runtime switch in `lfs-vm.sh`, Docker Engine
inside the container, IP forwarding in `vm-boot.sh`, and the Apple disk paths
in `lfs-space.sh`. `process-e2e.sh` built the bundle in Apple `container`.
`KVM-KERNEL.md` described a kernel only for SPACE in an Apple container.

## Options

1. Keep both runtimes.
2. Remove the Apple `container` path. OrbStack is the only runtime.

## Choice

Option 2. Nothing else on the Mac needs the Apple path, so keeping it adds
code that nobody runs.

- `tools/apple-container` is now `tools/orbstack`.
- `lfs-vm.sh` creates `lfs-orb` and has no `LFS_VM_RUNTIME`. `vm-boot.sh`
  has no Docker Engine branch.
- `process-e2e.sh` builds and tests with `docker`.
- `lfs-space.sh` measures `lfs-orb`: the `lfs-data` volume, the container's
  writable layer and the Sandbox volume.
- `services.compose.yml` stays: `services.orb.yml` builds on it.
- `KVM-KERNEL.md` is deleted.

## Consequences

- One workbench exists, with one SSH port (`127.0.0.1:2223`) and one Herdr
  machine (`lfs orb`).
- The workbench mounts `tools/orbstack` at `/opt/layerfs/tools`. A container
  made before the rename mounts the old folder. Recreate it with
  `lfs-vm.sh` after the rename reaches the checkout it was made from.
- Other repos that name `tools/apple-container/lfs.mjs` or `lfs-vm.sh` must
  use the new path.
- ADR 0003 turned off OrbStack's network bridge to protect Apple `container`
  bridges. With Apple `container` gone from the Mac, that reason no longer
  applies. This ADR does not change the OrbStack setting.
- `docker run` in `lfs-vm.sh` still adds `CAP_SYS_PTRACE`. The old comment
  says only the removed Docker Engine needed it. Remove it after a test shows that `lfs-orb`
  works without it.
