#!/usr/bin/env bash
# Diagnostic reproduction of core/docs/issues/unmount-join (not a gate): mount
# Workspace A, mount and unmount Workspace B at once, wait, then unmount A.
# Before the late unmount it lists every process whose mount namespace holds a
# copy of A's device, with that copy's propagation fields. Run as root in a
# fresh Linux VM with /dev/fuse (a Boat through boat-run):
#   bash tools/orbstack/unmount-join-repro.sh [wait seconds, default 40]
set -Eeuo pipefail
exec 2>&1
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
trap 'printf "ERROR line %d: %s (exit %d)\n" "$LINENO" "$BASH_COMMAND" "$?"' ERR
if [[ $(id -u) != 0 ]]; then
  exec sudo -n env "PATH=$PATH" "HOME=$HOME" \
    "RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup}" "CARGO_HOME=${CARGO_HOME:-$HOME/.cargo}" \
    bash "$repo/tools/orbstack/unmount-join-repro.sh" "$@"
fi
wait_s=${1:-40}
uname -m; uname -r; id
work=$(mktemp -d /var/tmp/unmount-join.XXXXXXXX)

# Toolchain and Node 24, as tools/orbstack/lost-mount-e2e.sh does.
if ! rustup toolchain list | awk '{print $1}' | grep -Eq '^1\.85\.1(-|$)'; then
  timeout 240s rustup toolchain install 1.85.1 --profile minimal >"$work/build.log" 2>&1
fi
if ! command -v node >/dev/null || [[ $(node -p 'process.versions.node.split(".")[0]') != 24 ]]; then
  case $(uname -m) in x86_64) arch=x64 ;; aarch64) arch=arm64 ;; *) echo "FAIL arch"; exit 1 ;; esac
  curl -fsS --max-time 60 https://nodejs.org/dist/latest-v24.x/SHASUMS256.txt >"$work/SHASUMS256.txt"
  tarball=$(awk -v a="$arch" '$2 ~ ("^node-v24\\.[0-9]+\\.[0-9]+-linux-" a "\\.tar\\.xz$") {print $2}' "$work/SHASUMS256.txt")
  curl -fsS --max-time 120 "https://nodejs.org/dist/latest-v24.x/$tarball" >"$work/$tarball"
  (cd "$work"; awk -v n="$tarball" '$2 == n' SHASUMS256.txt | sha256sum --check --status)
  mkdir "$work/node" && tar -xJf "$work/$tarball" --strip-components=1 -C "$work/node"
  export PATH="$work/node/bin:$PATH"
fi
cd "$repo"
unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS
export CARGO_TARGET_DIR="$work/target"
python3 -B core/tools/check_fuser_integrity.py >>"$work/build.log" 2>&1
timeout 1100s cargo +1.85.1 build --manifest-path core/Cargo.toml --locked --release -j 4 \
  -p layerfs-node --lib -p layerfs-daemon --bins >>"$work/build.log" 2>&1 || { tail -n 20 "$work/build.log"; exit 1; }
mkdir "$work/bundle"
cp "$work/target/release/layerfs-daemon" "$work/bundle/"
cp "$work/target/release/liblayerfs_node.so" "$work/bundle/layerfs.node"
mountpoint -q /sys/fs/fuse/connections || mount -t fusectl none /sys/fs/fuse/connections

export LAYERFS_RUNTIME=process LAYERFS_HOME="$work/home"
export LAYERFS_DAEMON="$work/bundle/layerfs-daemon" LAYERFS_NATIVE="$work/bundle/layerfs.node"
export LAYERFS_STORE_ROOT="$work/stores" LAYERFS_SDK="$repo/sdk/js/src/index.js"
export LAYERFS_SDK_OPERATOR="$repo/sdk/js/operator/index.js" LAYERFS_CONSTRUCTION_WORKERS=1
lfs() { timeout --kill-after=2s 30s node "$repo/tools/orbstack/lfs.mjs" "$@"; }
field() { python3 -c 'import json,sys; print(json.load(sys.stdin)[sys.argv[1]])' "$1"; }
project="join-$$"
mkdir -p "$work/source" && echo "unmount-join repro" >"$work/source/README.md"
lfs init "$project" "$work/source" >/dev/null
lfs fork "$project" work >/dev/null

a=$(lfs mount "$project" work)
a_sandbox=$(field sandbox <<<"$a"); a_path=$(field path <<<"$a")
b=$(lfs mount "$project" work)
b_sandbox=$(field sandbox <<<"$b"); b_path=$(field path <<<"$b")
rc=0; out=$(lfs unmount "$project" "$b_sandbox" "$b_path" --discard 2>&1) || rc=$?
echo "B immediate unmount: rc=$rc $(tail -n 1 <<<"$out" | cut -c1-200)"

echo "waiting ${wait_s} s with A mounted at $a_path"
sleep "$wait_s"
device=$(awk -v p="$a_path" '$5 == p {print $3}' /proc/self/mountinfo | tail -n 1)
echo "A device $device; mount row here: $(awk -v p="$a_path" '$5 == p {print $7, $8}' /proc/self/mountinfo)"
echo "copies of $device in other mount namespaces:"
self_ns=$(readlink /proc/self/ns/mnt)
for d in /proc/[0-9]*; do
  ns=$(readlink "$d/ns/mnt" 2>/dev/null) || continue
  [[ $ns == "$self_ns" ]] && continue
  row=$(awk -v dev="$device" '$3 == dev {print $5, $7, $8}' "$d/mountinfo" 2>/dev/null | head -n 1)
  [[ -n $row ]] && echo "  pid ${d#/proc/} ns $ns comm $(cat "$d/comm" 2>/dev/null): $row"
done
echo "end of copies"

rc=0; out=$(lfs unmount "$project" "$a_sandbox" "$a_path" --discard 2>&1) || rc=$?
echo "A late unmount: rc=$rc"
grep -E 'kind|code|phase|stage|detail|Retained|Held|namespace' <<<"$out" | head -n 8 | cut -c1-300
echo "daemon.log events:"
find "$LAYERFS_HOME" "$work" -name daemon.log -exec grep -h -E ' (mounted|detached|busy|held|gone|abort|retained|unmounted) ' {} + 2>/dev/null | cut -c1-200 | tail -n 12
echo "RESULT A late unmount rc=$rc"
