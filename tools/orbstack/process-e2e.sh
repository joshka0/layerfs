#!/bin/zsh
# Builds the Linux daemon and SDK addon in OrbStack Docker, then runs
# sdk/js/test/process_runtime.mjs (the process Sandbox runtime) in one
# container. No network is needed inside the containers. The Store and the
# Sandboxes sit on anonymous volumes, because SQLite locks and commits need
# a real filesystem, not OrbStack's overlay root (as in lfs-vm.sh). The bundle in
# <work-dir>/bundle is what lfs-vm.sh takes.
#
# usage: tools/orbstack/process-e2e.sh <work-dir>
#
# Before the first run, on the Mac:
#   cargo fetch --locked --manifest-path core/Cargo.toml
#   docker pull rust:1.85.1-bookworm
#   docker pull node:24-bookworm
set -eu
repo=${0:A:h:h:h}
work=${1:?usage: process-e2e.sh <work-dir>}
mkdir -p "$work/target" "$work/bundle"

docker run --rm --cpus 4 --memory 8G \
  -v "$repo":/work:ro -v "$HOME/.cargo/registry":/usr/local/cargo/registry:ro \
  -v "$work/target":/target -e CARGO_TARGET_DIR=/target -w /work rust:1.85.1-bookworm \
  cargo build --manifest-path core/Cargo.toml --offline --locked --release -j 4 \
  -p layerfs-node --lib -p layerfs-daemon --bins
cp "$work/target/release/layerfs-daemon" "$work/bundle/"
cp "$work/target/release/liblayerfs_node.so" "$work/bundle/layerfs.node"
shasum -a 256 "$work/bundle/layerfs-daemon" "$work/bundle/layerfs.node"

docker run --rm --cpus 4 --memory 4G --cap-add SYS_ADMIN --device /dev/fuse \
  -v /lfs-store -v /var/lib/layerfs \
  -v "$repo":/work:ro -v "$work/bundle":/bundle:ro \
  -e LAYERFS_RUNTIME=process -e LAYERFS_HOME=/root/lfs-home \
  -e LAYERFS_DAEMON=/bundle/layerfs-daemon -e LAYERFS_NATIVE=/bundle/layerfs.node \
  -e LAYERFS_STORE_PATH=/lfs-store/global/store.sqlite \
  -w /work node:24-bookworm node sdk/js/test/process_runtime.mjs
