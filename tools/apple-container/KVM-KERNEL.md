# A KVM kernel for Apple container

Apple `container`'s default kernel (Kata `vmlinux-6.18.15-186`) has no
`CONFIG_KVM`. A container started with `--virtualization` runs at EL2 on an
M3 or newer Mac, but it has no `/dev/kvm`. A kernel built from
apple/containerization's `kernel/config-arm64` has KVM, and Firecracker
microVMs boot inside such a container (2026-10-29, Firecracker v1.17.0,
Ubuntu 24.04 guest).

## Use it

Pass the kernel to one container. Never make it the default
(`container system kernel set`): that changes every container.

```sh
env -u HTTPS_PROXY -u HTTP_PROXY container run --rm --virtualization \
  -k ~/.local/share/lfs-vm/kernels/vmlinux-6.18.5-kvm-arm64 alpine ls -l /dev/kvm
```

The kernel: `~/.local/share/lfs-vm/kernels/vmlinux-6.18.5-kvm-arm64`
(sha256 `6195d96f05018d0a8021e300797c2c0ddbec82d7de9e34460db56980de11137d`),
its resolved config beside it as `config-6.18.5-kvm-arm64`.

## Build it again

On the dev box (x86_64 Ubuntu), cross-compile; it took 9 min 23 s at `-j8`.

1. Install the tools: `sudo apt-get install -y gcc-aarch64-linux-gnu flex bison bc libssl-dev libelf-dev`.
2. Get apple/containerization at `baf212e5a8e432e539beee325050aa26443a154d`
   and the Linux source its `kernel/Makefile` names (`KSOURCE`, 6.18.5 from
   cdn.kernel.org).
3. In the Linux source, as `kernel/build.sh` does but without its container:

   ```sh
   cp <containerization>/kernel/config-arm64 .config
   make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- olddefconfig
   make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- -j8 LOCALVERSION=-cz-baf212e5a8e4
   ```

4. Copy the uncompressed `arch/arm64/boot/Image` to the Mac as the kernel
   file, and check that the `.config` has `CONFIG_KVM=y`.

## SPACE kernel

SPACE needs more than `CONFIG_KVM`. A second kernel,
`~/.local/share/lfs-vm/kernels/vmlinux-6.18.5-kvm-space-arm64` (sha256
`566530ec9644f1effb1b198531b5a7e4dc044b782808fad4398dee6f07711973`, config
`config-6.18.5-kvm-space-arm64` beside it), adds these as `=y`: `BTRFS_FS`,
`BLK_DEV_DM`, `DM_CRYPT`, `DM_SNAPSHOT`, `DM_THIN_PROVISIONING`, `DM_VERITY`,
`FS_VERITY`, `XFS_FS`, `NETFILTER_XTABLES_LEGACY`, `IP_NF_IPTABLES_LEGACY`,
`IP_NF_NAT`, `IP_NF_TARGET_MASQUERADE`, `IP_NF_FILTER`, `IP_NF_MANGLE`,
`IP6_NF_IPTABLES`, `IP6_NF_NAT`, `NF_NAT`, `NETFILTER_XT_MATCH_ADDRTYPE`,
`NETFILTER_XT_MATCH_CONNTRACK`, `NETFILTER_XT_MARK`,
`NETFILTER_XT_MATCH_COMMENT` and `NETFILTER_XT_MATCH_MULTIPORT`. Set their
dependencies `MD`, `NETFILTER_ADVANCED`, `NF_CONNTRACK`, `BRIDGE_NETFILTER` and
`NETFILTER_XTABLES` first, or `olddefconfig` drops them. The dev box build took
9 min 4 s at `-j8`.

## Run SPACE's Kind stack in one container

With the SPACE kernel, a 6-node Kind cluster reached Ready (2026-10-29,
`container` 1.6.1, 6 CPUs, 12 GB). It needs all of these:

1. Create the container with `--virtualization -k <SPACE kernel>
   --dns 1.1.1.1 --cap-add ALL`. The gateway DNS (`192.168.64.1`) does not
   answer. Without `CAP_SYS_RESOURCE`, runc in the Kind node fails on
   `oom_score_adj`.
2. Before `dockerd` starts, move every process out of the root cgroup:
   `mkdir /sys/fs/cgroup/init`, write each pid of the root `cgroup.procs` to
   `/sys/fs/cgroup/init/cgroup.procs`, then write
   `+cpuset +cpu +pids +memory +io` to the root `cgroup.subtree_control`.
   Otherwise the root becomes threaded and systemd in the Kind node exits 255.
3. Set `fs.inotify.max_user_watches=1048576` and
   `fs.inotify.max_user_instances=8192`, or Tilt cannot watch files.
4. In SPACE's `docker/Dockerfile.rust-build-common`, use the aarch64
   `sccache` and `wild` downloads with their own hashes, and rename the
   `CARGO_TARGET_X86_64_*` variables to `AARCH64`.
5. Remove the container with `container rm -f`. `container stop` fails with
   errno 95 while Docker's threaded cgroups exist.

No sandbox booted: Tilt compiles SPACE's Rust workspace once per service
image, which takes hours on arm64. The network stayed up for 57 min under
that load, after OrbStack's `network_bridge` was turned off (ADR 0003).
