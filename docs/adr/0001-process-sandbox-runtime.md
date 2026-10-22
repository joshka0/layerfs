# 0001: Process Sandbox runtime

- Date: 2026-10-22
- Status: accepted on branch `feat/process-backend` (fork joshka0/layerfs)
- Topic: layerfs-container-port

## Context

A LayerFS Sandbox was always a Docker container. The container gives the
daemon `/dev/fuse` and `CAP_SYS_ADMIN`, and gives commands uid 501, a seccomp
profile and their own namespaces. On macOS without Docker, Apple `container`
runs each container in its own VM. Two Apple containers cannot share one
read-write volume, so they cannot share one Store.

An experiment on 2026-10-22 ran Init, the daemon, a FUSE mount, a Commit and
an unmount in one Apple container, with no code change
(`core/crates/layerfs-api/examples/process_kernel_mount.rs`). The only
Docker-bound part is the runtime layer: container create and upload, exec,
log reads and inspect-based reopen.

## Options

1. A Docker Engine API shim in front of Apple `container` (socktainer). It
   has no shared read-write volume and no `--device`.
2. Docker inside one Apple VM. It adds a second container engine inside the
   VM.
3. A `process` runtime: the daemon runs as a child process on the same Linux
   host as the SDK, and commands run through `setpriv`.

## Choice

Option 3. `layerfs-sandbox` has `backend::process` and a `backend::Runtime`
enum over Docker and process Sandboxes. `layerfs-api` keeps one startup path:
the runtime start differs, then connect, authenticate and Hello are shared.
The SDK selects the runtime with `LAYERFS_RUNTIME` or the `runtime` option.
`project.init` records the Store's locator: the container mount point for
Docker, the client's `storePath` for a process runtime.

## Consequences

- A process Sandbox gives commands no separate namespaces and no seccomp
  profile. Commands run as uid 501 with no capabilities and
  `no_new_privs`, in the host's mount, process and network namespaces.
- The client runs as root, because it starts the daemon, which mounts FUSE.
- A project keeps the runtime it was initialized with: its locator decides
  where daemons open the Store.
- `WorkspaceApi::exec` (the Docker streaming launcher) panics on a process
  Sandbox. Both runtimes use `internal::run`.
- Future work: give each process Sandbox its own mount and PID namespaces.
