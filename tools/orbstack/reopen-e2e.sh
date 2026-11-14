#!/bin/zsh
# Runs sdk/js/test/reopen/main.mjs, the gating tests of
# core/docs/issues/373/REOPEN.md, across real container restarts. Each phase
# runs in its own throwaway container on two named volumes laid out as in
# lfs-orb (/var/lib/layerfs-data and /var/lib/layerfs). Removing a container
# kills its daemons and FUSE mounts; the next container starts on the same
# files. The volumes and containers are removed at the end.
#
# usage: tools/orbstack/reopen-e2e.sh <work-dir> <bundle-dir> [<legacy-bundle-dir> <legacy-sdk-dir>]
#   <bundle-dir>         layerfs-daemon and layerfs.node built from this tree
#                        (tools/orbstack/process-e2e.sh writes one)
#   <legacy-bundle-dir>  the bundle from before the change (lfs-orb's), and
#   <legacy-sdk-dir>     its sdk/js tree: they seed records without `root` (F32)
#
# The report is <work-dir>/report/: one log per phase and summary.txt with
# every PASS and FAIL line and the command that reproduces it.
set -u
repo=${0:A:h:h:h}
work=${1:?usage: reopen-e2e.sh <work-dir> <bundle-dir> [<legacy-bundle-dir> <legacy-sdk-dir>]}
bundle=${2:?usage: reopen-e2e.sh <work-dir> <bundle-dir> [<legacy-bundle-dir> <legacy-sdk-dir>]}
legacy_bundle=${3:-}
legacy_sdk=${4:-}
work=${work:A} bundle=${bundle:A}
report=$work/report
mkdir -p "$report"
run_id=reopen-e2e-$$
data=$run_id-data sbx=$run_id-sbx
# lfs-orb's base image plus procps: vm-boot.sh needs pgrep. Built once and
# kept; remove it with `docker image rm reopen-e2e-node24`.
image=reopen-e2e-node24
docker image inspect $image >/dev/null 2>&1 || printf '%s\n' 'FROM node:24-bookworm-slim' \
  'RUN apt-get -qq update && apt-get -qq install -y --no-install-recommends procps && rm -rf /var/lib/apt/lists/*' |
  docker build -q -t $image - >/dev/null || exit 1
docker volume create $data >/dev/null
docker volume create $sbx >/dev/null
cleanup() {
  docker rm -f $run_id >/dev/null 2>&1
  docker volume rm $data $sbx >/dev/null 2>&1
}
trap cleanup EXIT

{
  echo "command: $0 $*"
  echo "repo: $repo $(git -C "$repo" rev-parse HEAD) dirty=$(git -C "$repo" status --porcelain | wc -l | tr -d ' ')"
  shasum -a 256 "$bundle"/layerfs-daemon "$bundle"/layerfs.node
  [[ -n $legacy_bundle ]] && shasum -a 256 "$legacy_bundle"/layerfs-daemon "$legacy_bundle"/layerfs.node
  date -u +%Y-%m-%dT%H:%M:%SZ
} > "$report/run.txt"

# phase <name> <sdk-dir> <bundle-dir> [sandbox volume: yes|no]
phase() {
  local name=$1 sdk=$2 bun=$3 with_sbx=${4:-yes}
  local vols=(-v $data:/var/lib/layerfs-data)
  [[ $with_sbx == yes ]] && vols+=(-v $sbx:/var/lib/layerfs)
  docker rm -f $run_id >/dev/null 2>&1
  # Each phase is one test command: at most 120 s (AGENTS.md).
  timeout 120 docker run --rm --name $run_id --init --cap-add SYS_ADMIN --device /dev/fuse \
    --security-opt apparmor=unconfined $vols \
    -v "$sdk":/opt/layerfs/sdk:ro -v "$bun":/opt/layerfs/bundle:ro \
    -v "$repo/tools/orbstack":/opt/layerfs/tools:ro -v "$repo/sdk/js/test/reopen":/opt/reopen-test:ro \
    -e REOPEN_E2E_CONTAINER=1 -w /root $image \
    node /opt/reopen-test/main.mjs $name > "$report/$name.log" 2>&1
  local rc=$?
  echo "phase $name exit $rc" >> "$report/run.txt"
  return 0
}

[[ -n $legacy_bundle && -n $legacy_sdk ]] && phase seed-legacy "${legacy_sdk:A}" "${legacy_bundle:A}"
phase seed "$repo/sdk/js" "$bundle"
phase tamper "$repo/sdk/js" "$bundle"
phase check "$repo/sdk/js" "$bundle"
phase boot-unmounted "$repo/sdk/js" "$bundle" no
phase boot "$repo/sdk/js" "$bundle"
phase live "$repo/sdk/js" "$bundle"

{
  cat "$report/run.txt"
  echo
  grep -h -E '^(PASS|FAIL|SKIP|PHASE) ' "$report"/*.log
} > "$report/summary.txt"
grep -c '^PASS ' "$report/summary.txt" | sed 's/^/passed: /'
grep -c '^FAIL ' "$report/summary.txt" | sed 's/^/failed: /'
echo "report: $report/summary.txt"
! grep -q '^FAIL ' "$report/summary.txt"
