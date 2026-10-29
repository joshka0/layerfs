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
