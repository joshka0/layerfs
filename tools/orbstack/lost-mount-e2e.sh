#!/usr/bin/env bash
# Run as root in a fresh Linux Boat VM: bash tools/orbstack/lost-mount-e2e.sh
# No Docker or Mac environment. Node 24 is fetched from nodejs.org with its
# published SHA256. The SDK's init/open routes select Disposable explicitly.
# Builds have a separate budget; the mounted checks together have 115 seconds.
set -Eeuo pipefail
exec 2>&1
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)

# GNU timeout owns the complete process group, including install/build children.
if [[ ${1:-} != --bounded && ${1:-} != --cases ]]; then
  exec timeout --signal=TERM --kill-after=10s 1750s bash "$repo/tools/orbstack/lost-mount-e2e.sh" --bounded
fi
if [[ $1 == --bounded ]]; then
  uname -m
  ls -l /dev/fuse
  id
  rustup toolchain list
fi
work=${LOST_MOUNT_WORK:-}
holder=
ws_path=
passed=0
failed=0
summary() { printf 'SUMMARY passed=%d failed=%d\n' "$passed" "$failed"; }
pass() { printf 'PASS %s\n' "$1"; passed=$((passed + 1)); }
fail() { printf 'FAIL %s\n' "$1"; failed=$((failed + 1)); }
if [[ $1 == --bounded ]]; then
  work=$(mktemp -d /var/tmp/layerfs-lost-mount.XXXXXXXX)
  export LOST_MOUNT_WORK=$work
  finish() {
    local rc=$?
    if (( rc != 0 )); then
      printf 'FAIL VM setup/build/cases (exit %d)\n' "$rc"
      tail -n 12 "$work/build.log" 2>/dev/null || true
      printf 'SUMMARY run=FAIL\n'
    fi
    # The case runner stops its holders, mounts and daemons first. Keep evidence
    # on failure within this disposable VM; stdout is the Boat return channel.
    if (( rc == 0 )); then rm -rf -- "${work:?}"; fi
  }
  trap finish EXIT
  [[ $(id -u) == 0 && $(uname -s) == Linux && -c /dev/fuse ]]
  for tool in curl tar xz sha256sum python3 unshare mountpoint umount cc pkg-config; do
    command -v "$tool" >/dev/null || { echo "FAIL missing prerequisite: $tool"; exit 1; }
  done
  if ! rustup toolchain list | awk '{print $1}' | grep -Eq '^1\.85\.1(-|$)'; then
    timeout --kill-after=5s 240s rustup toolchain install 1.85.1 --profile minimal >"$work/build.log" 2>&1
  fi
  if ! command -v node >/dev/null || [[ $(node -p 'process.versions.node.split(".")[0]') != 24 ]]; then
    case $(uname -m) in
      x86_64) node_arch=x64 ;;
      aarch64) node_arch=arm64 ;;
      *) echo 'FAIL Node 24 bootstrap supports x86_64 and aarch64'; exit 1 ;;
    esac
    echo 'Installing Node 24: official latest-v24.x Linux tarball, verified with SHASUMS256.txt'
    curl --fail --silent --show-error --max-time 60 https://nodejs.org/dist/latest-v24.x/SHASUMS256.txt >"$work/SHASUMS256.txt"
    node_tar=$(awk -v arch="$node_arch" '$2 ~ ("^node-v24\\.[0-9]+\\.[0-9]+-linux-" arch "\\.tar\\.xz$") {print $2}' "$work/SHASUMS256.txt")
    [[ -n $node_tar && $node_tar != *$'\n'* ]]
    curl --fail --silent --show-error --max-time 120 "https://nodejs.org/dist/latest-v24.x/$node_tar" >"$work/$node_tar"
    (cd "$work"; awk -v name="$node_tar" '$2 == name' SHASUMS256.txt | sha256sum --check --status)
    mkdir "$work/node"
    tar -xJf "$work/$node_tar" --strip-components=1 -C "$work/node"
    export PATH="$work/node/bin:$PATH"
  fi
  node --version
  cd "$repo"
  # Root Cargo config supplies the required ARM AEAD flags. Do not inherit an
  # override from boat-run. Build only the daemon and addon, with locked inputs.
  unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS
  export CARGO_TARGET_DIR="$work/target"
  python3 -B core/tools/check_fuser_integrity.py >"$work/build.log" 2>&1
  timeout --kill-after=10s 1100s cargo +1.85.1 build --manifest-path core/Cargo.toml \
    --locked --release -j 4 -p layerfs-node --lib -p layerfs-daemon --bins >>"$work/build.log" 2>&1
  mkdir "$work/bundle"
  cp "$work/target/release/layerfs-daemon" "$work/bundle/"
  cp "$work/target/release/liblayerfs_node.so" "$work/bundle/layerfs.node"
  sha256sum "$work/bundle/layerfs-daemon" "$work/bundle/layerfs.node"
  # Compile separately, then apply the 120 s ceiling to the literal-row tests.
  timeout --kill-after=5s 120s cargo +1.85.1 test --manifest-path core/Cargo.toml \
    --locked --release -j 4 -p layerfs-fuse --test namespace_copies --no-run >>"$work/build.log" 2>&1
  timeout --kill-after=5s 115s cargo +1.85.1 test --manifest-path core/Cargo.toml \
    --locked --release -p layerfs-fuse --test namespace_copies >"$work/rows.log" 2>&1 || {
      tail -n 16 "$work/rows.log"; exit 1;
    }
  echo 'PASS literal mountinfo cases (L9 L11 L13 malformed-row L14)'
  # No test invocation exceeds 120 s; the complete run stays below 30 minutes.
  timeout --signal=TERM --kill-after=5s 115s bash "$repo/tools/orbstack/lost-mount-e2e.sh" --cases
  exit
fi

[[ -n $work && -d $work/bundle ]]
export LAYERFS_RUNTIME=process LAYERFS_HOME="$work/home"
export LAYERFS_DAEMON="$work/bundle/layerfs-daemon"
export LAYERFS_NATIVE="$work/bundle/layerfs.node"
export LAYERFS_STORE_ROOT="$work/stores" LAYERFS_SDK="$repo/sdk/js/src/index.js"
export LAYERFS_CONSTRUCTION_WORKERS=1
project="lost-mount-$$"
lfs() { timeout --kill-after=2s 15s node "$repo/tools/orbstack/lfs.mjs" "$@"; }
stop_holder() {
  if [[ -n $holder ]]; then
    kill "$holder" 2>/dev/null || true
    wait "$holder" 2>/dev/null || true
    holder=
  fi
}
cleanup() {
  local rc=$?
  trap - EXIT
  stop_holder
  # External plain umount is cleanup only, never a second product attempt.
  if [[ -n $ws_path ]] && mountpoint -q "$ws_path"; then
    timeout --kill-after=1s 3s umount "$ws_path" || rc=1
  fi
  # Identify only this run's executable, rather than killing by process name.
  while read -r daemon_pid; do
    kill "$daemon_pid" 2>/dev/null || true
  done < <(python3 - "$LAYERFS_DAEMON" <<'PY'
import os, sys
for name in os.listdir('/proc'):
    if not name.isdecimal():
        continue
    try:
        if os.readlink('/proc/' + name + '/exe') == sys.argv[1]:
            print(name)
    except (FileNotFoundError, PermissionError, ProcessLookupError):
        continue
PY
  )
  if (( rc != 0 )); then fail "case runner stopped (exit $rc)"; fi
  if (( failed != 0 )); then
    for evidence in "$work"/*.err "$work/private.out" "$work/slave.out"; do
      if [[ -s $evidence ]]; then
        printf 'EVIDENCE %s\n' "${evidence##*/}"
        tail -n 6 "$evidence"
      fi
    done
  fi
  summary
  if (( failed == 0 && rc == 0 )); then exit 0; else exit 1; fi
}
trap cleanup EXIT
trap 'exit 1' TERM INT

# Readiness checks require the holder to be in a distinct namespace, with the
# same FUSE device and the requested propagation. No unmount is retried.
start_holder() {
  local propagation=$1
  unshare --mount --propagation "$propagation" sleep 300 &
  holder=$!
  timeout --kill-after=1s 5s python3 - "$holder" "$ws_path" "$propagation" <<'PY'
import os, sys, time
pid, path, mode = sys.argv[1:]
def mount_row(pid):
    for line in open('/proc/' + pid + '/mountinfo'):
        fields = line.split()
        if fields[4] == path:
            return fields
    raise AssertionError('Workspace mount missing')
own = mount_row('self')
shared = [f for f in own[6:own.index('-')] if f.startswith('shared:')]
assert len(shared) == 1, 'fixture requires our mount to be shared'
expected_master = 'master:' + shared[0].split(':')[1]
deadline = time.monotonic() + 4
while time.monotonic() < deadline:
    try:
        row = mount_row(pid)
        tags = row[6:row.index('-')]
        distinct = os.readlink('/proc/self/ns/mnt') != os.readlink('/proc/' + pid + '/ns/mnt')
        ready = not any(t.startswith(('shared:', 'master:')) for t in tags) if mode == 'private' else expected_master in tags
        if distinct and ready and row[2] == own[2] and open('/proc/' + pid + '/comm').read().strip() == 'sleep':
            break
    except (FileNotFoundError, ProcessLookupError):
        pass
    time.sleep(.02)
else:
    raise AssertionError('namespace copy did not become ready')
PY
}
mount_workspace() {
  lfs mount "$project" work >"$work/mount.json" 2>"$work/mount.err"
  sandbox=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["sandbox"])' "$work/mount.json")
  ws_path=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["path"])' "$work/mount.json")
  [[ -n $ws_path && -n $sandbox ]]
}
mkdir -p "$work/source"
printf 'namespace-copy sentinel\n' >"$work/source/sentinel"
lfs init "$project" "$work/source" >"$work/init.json" 2>"$work/init.err"
lfs fork "$project" work >"$work/fork.json" 2>"$work/fork.err"
mount_workspace
start_holder private
pass 'private namespace fixture has same device and no propagation (L9)'
# Keep the exact mount identity, including mount ID, across the Busy refusal.
awk -v path="$ws_path" '$5 == path' /proc/self/mountinfo >"$work/before.row"
set +e
lfs unmount "$project" "$sandbox" "$ws_path" >"$work/busy.out" 2>"$work/busy.err"
busy_rc=$?
set -e
if python3 - "$busy_rc" "$holder" "$work/busy.err" <<'PY'
import re, sys
rc, pid, filename = sys.argv[1:]
text = open(filename).read()
assert int(rc) not in (0, 124, 137), 'must refuse Busy, not succeed or time out'
assert re.search(r'\bBusy\b', text), text[-1500:]
assert 'unmount:namespace' in text, text[-1500:]
assert re.search(r'(?<![0-9])' + re.escape(pid) + r'(?![0-9])', text), text[-1500:]
PY
then pass 'Busy at unmount:namespace names holder pid (L9 L12)'
else fail 'Busy answer, namespace phase or holder pid (L9 L12)'; fi
awk -v path="$ws_path" '$5 == path' /proc/self/mountinfo >"$work/after.row"
if [[ -s $work/before.row ]] && cmp -s "$work/before.row" "$work/after.row" && \
   timeout --kill-after=1s 3s cmp -s "$work/source/sentinel" "$ws_path/sentinel"; then
  pass 'Busy preserves exact kernel mount and readable content (L9 L12)'
else fail 'Busy detached or lost service (L9 L12)'; fi
stop_holder
if lfs unmount "$project" "$sandbox" "$ws_path" >"$work/private.out" 2>"$work/private.err" && \
   python3 -c 'import json,sys; assert json.load(open(sys.argv[1])) == {"kind":"Unmounted"}' "$work/private.out" && \
   [[ ! -e $ws_path ]] && ! mountpoint -q "$ws_path"; then
  pass 'after holder exits: Unmounted and directory gone (L9 L12)'
else fail 'after holder exits: unmount or directory removal (L9 L12)'; exit 1; fi
ws_path=
mount_workspace
start_holder slave
pass 'slave namespace fixture follows our shared group (L11)'
if lfs unmount "$project" "$sandbox" "$ws_path" >"$work/slave.out" 2>"$work/slave.err" && \
   python3 -c 'import json,sys; assert json.load(open(sys.argv[1])) == {"kind":"Unmounted"}' "$work/slave.out" && \
   kill -0 "$holder" && [[ ! -e $ws_path ]] && ! mountpoint -q "$ws_path"; then
  pass 'live slave permits Unmounted and directory removal (L11)'
else fail 'live slave blocks unmount (L11)'; fi
stop_holder
(( failed == 0 ))
