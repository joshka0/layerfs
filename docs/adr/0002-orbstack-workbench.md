# 0002: OrbStack workbench beside the Apple container one

- Date: 2026-10-29
- Status: accepted on branch `lfs-orb`; Option 3 replaced by ADR 0004
- Topic: layerfs-container-port

## Context

The operator moves all Herdr agent work into a Linux workbench with LayerFS
Workspaces. Keys, Touch ID approvals and iOS builds (Xcode) stay on the Mac,
reached through `lfs-bridge`.

On 2026-10-29, Defender on the Mac scanned every file open: Python read
20,000 small files in 335 s on the Mac disk, 0.05 to 0.2 s on a VM's own
disk, and about 248 s through a mount of a Mac folder. Both OrbStack and
Apple `container` skip Defender for files on the VM disk.

`lfs-vm` (an Apple container) passed its agent stages on 2026-10-22, but its
network failed four times (pf rules, gateway DNS, builder DNS, VPN), and it
runs Docker Engine inside to host the shared services.

## Options

1. Move the workbench to OrbStack and drop Apple `container`.
2. Keep Apple `container` only.
3. Support both: one launcher with a runtime choice, one bridge that serves
   each workbench that exists.

## Choice

Option 3 (operator's choice: "container should still be supported
generally"). `LFS_VM_RUNTIME=orb tools/orbstack/lfs-vm.sh` creates
`lfs-orb`:

- A Docker container in OrbStack, SSH on `127.0.0.1:2223`, Herdr machine
  `lfs orb`.
- The shared services run as sibling containers on the Docker network
  `lfs-orb` (`services.orb.yml`). socat forwards each to the workbench's
  loopback, so agents use the same addresses as in `lfs-vm`.
- The bridge is at `host.docker.internal` (`0.250.250.254`), the Mac's
  loopback.
- `/var/lib/layerfs` is an anonymous volume. On OrbStack's overlay root
  filesystem, a LayerFS commit found no changes (`UpToDate`); on the btrfs
  volume it commits.

## Consequences

- Two workbenches can exist at once; each has its own Stores on its own
  `lfs-data` volume.
- `lfs-orb` shares OrbStack's VM: 10 CPUs and 16 GB unless OrbStack's
  `memory_mib` setting is raised. It defaults to 12 GB.
- Defender does not scan the workbench. The operator tells IT (decided
  2026-10-28).
- Future work: the `lfs` CLI and the bridge name repos and machines per
  workbench; agents must say which workbench a Workspace is in.
