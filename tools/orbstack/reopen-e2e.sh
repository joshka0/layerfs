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
# every PASS and FAIL line and the command that reproduces it. A FAIL that
# matches sdk/js/test/reopen/baseline.tsv is shown as BASELINE and does not
# fail the run.
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
# lfs-orb's base image plus procps (vm-boot.sh needs pgrep) and e2fsprogs
# (chattr, for F42). Built once and kept; remove it with
# `docker image rm reopen-e2e-node24:2`.
image=reopen-e2e-node24:2
docker image inspect $image >/dev/null 2>&1 || printf '%s\n' 'FROM node:24-bookworm-slim' \
  'RUN apt-get -qq update && apt-get -qq install -y --no-install-recommends procps e2fsprogs && rm -rf /var/lib/apt/lists/*' |
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
  timeout 120 docker run --rm --name $run_id --init --cap-add SYS_ADMIN --cap-add LINUX_IMMUTABLE --device /dev/fuse \
    --security-opt apparmor=unconfined $vols \
    -v "$sdk":/opt/layerfs/sdk:ro -v "$bun":/opt/layerfs/bundle:ro \
    -v "$repo/tools/orbstack":/opt/layerfs/tools:ro -v "$repo/sdk/js/test/reopen":/opt/reopen-test:ro \
    -e REOPEN_E2E_CONTAINER=1 -w /root $image \
    node /opt/reopen-test/main.mjs $name > "$report/$name.log" 2>&1
  local rc=$?
  echo "phase $name exit $rc" >> "$report/run.txt"
  return 0
}

# The legacy bundle must predate the change: a redeployed lfs-orb bundle
# would seed records with `root` and turn F32 into a false failure.
if [[ -n $legacy_bundle ]] && cmp -s "$legacy_bundle/layerfs-daemon" "$bundle/layerfs-daemon"; then
  echo "the legacy bundle $legacy_bundle is the bundle under test; give the pre-change bundle" >&2
  exit 2
fi
[[ -n $legacy_bundle && -n $legacy_sdk ]] && phase seed-legacy "${legacy_sdk:A}" "${legacy_bundle:A}"
phase seed "$repo/sdk/js" "$bundle"
phase tamper "$repo/sdk/js" "$bundle"
phase check "$repo/sdk/js" "$bundle"
phase boot-unmounted "$repo/sdk/js" "$bundle" no
phase boot "$repo/sdk/js" "$bundle"
phase live "$repo/sdk/js" "$bundle"
phase races "$repo/sdk/js" "$bundle"

{
  cat "$report/run.txt"
  echo
  grep -h -E '^(PASS|FAIL|SKIP|NOTE|PHASE) ' "$report"/*.log
} > "$report/summary.txt"
# Diff the failures against the baseline (sdk/js/test/reopen/baseline.tsv):
# a failure whose check and reason match an entry becomes BASELINE.
python3 -I - "$report/summary.txt" "$repo/sdk/js/test/reopen/baseline.tsv" <<'DIFF'
import re, sys
summary, baseline = sys.argv[1], sys.argv[2]
entries = []
for line in open(baseline):
    if line.strip() and not line.startswith("#"):
        check, reason, issue = line.rstrip("\n").split("\t")
        entries.append([check, re.compile(reason), issue, 0])
out, counts = [], {"PASS": 0, "FAIL": 0, "BASELINE": 0}
for line in open(summary).read().splitlines():
    # A check name may itself hold ": ", so match each entry's whole name.
    hit = next((e for e in entries if line.startswith(f"FAIL {e[0]}: ")
                and e[1].search(line[len(f"FAIL {e[0]}: "):])), None)
    if hit:
        hit[3] += 1
        line = f"BASELINE {line[5:]} [{hit[2]}]"
    word = line.split(" ", 1)[0]
    if word in counts:
        counts[word] += 1
    out.append(line)
out += [f"BASELINE-UNUSED {e[0]} [{e[2]}]" for e in entries if not e[3]]
open(summary, "w").write("\n".join(out) + "\n")
print(f"passed: {counts['PASS']}\nfailed: {counts['FAIL']}\nbaseline: {counts['BASELINE']}")
DIFF
echo "report: $report/summary.txt"
! grep -q '^FAIL ' "$report/summary.txt"
