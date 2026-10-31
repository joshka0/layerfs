# 0005: Rootless Docker for agents in the workbench

- Date: 2026-10-31
- Status: accepted
- Topic: layerfs-container-port

## Context

Agents work in the OrbStack workbench `lfs-orb`. Some repos need Docker: the
praze stack, and effect-folio's E2E gates, which call `docker compose`
themselves. Their compose files publish ports on `127.0.0.1` and bind-mount
repo files (`build: .`, `./Caddyfile`).

`lfs-orb` has no Docker. OrbStack's Docker cannot see LayerFS mounts inside
the workbench, and its socket controls every container on the Mac.

## Options

1. A filtering proxy in front of OrbStack's Docker socket. Workspace files and
   the agent's `localhost` stay out of reach, and the proxy must filter
   request bodies to refuse privileged runs and host mounts.
2. Rootless Docker inside `lfs-orb`, run as the agent user.

## Choice

Option 2. dockerd runs as uid 501 in its own user namespace. Its containers
see the Workspaces, publish ports on the workbench's loopback, and root in
them is uid 501 outside.

A probe container with the workbench's image and capabilities proved it on
2026-10-31: a compose stack with a published port and a file bind mount ran;
a container could not read root's files or write `/usr` of the outer
container; `--privileged` containers do not start.

## Consequences

- `lfs-orb` runs with `--device /dev/net/tun` (rootlesskit's network),
  `--security-opt systempaths=unconfined` (rootlesskit sets
  `net.ipv4.ip_forward` in its own namespace) and the seccomp profile
  `tools/orbstack/seccomp.json`.
- `seccomp.json` is Docker's default profile plus `keyctl`, `add_key`,
  `request_key` (runc and crun create a session keyring) and `pivot_root` for
  holders of `CAP_SYS_ADMIN`. `seccomp.sh` writes it from the upstream file,
  pinned by digest.
- Root in `lfs-orb` can now write kernel settings of the OrbStack VM through
  `/proc/sys`. Agents (uid 501) cannot. `lfs-orb` already had
  `CAP_SYS_ADMIN`.
- Image layers live in the agent home on `lfs-data` and count against its
  disk.
- The agent firewall of `vm-boot.sh` applies to dockerd and its containers.
- `--privileged` containers stay unsupported.
