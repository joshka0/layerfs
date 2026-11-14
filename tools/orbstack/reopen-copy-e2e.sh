#!/bin/zsh
# Reopen E2E on reflink copies of real saved overlays (core/docs/issues/373/REOPEN.md).
# Run on the Mac. It never touches the live Sandboxes or the rescue snapshot
# except to reflink-copy them, and it runs the reopen in a separate container
# that sees only the copies, at the same absolute paths as on lfs-orb.
#
# usage: tools/orbstack/reopen-copy-e2e.sh <bundle-dir> <project> <sandbox-name> <namespace>
#   e.g. reopen-copy-e2e.sh $W/bundle memoryd layerfs-memoryd-1 6
# Writes its report to <bundle-dir>/../reopen-e2e-<project>.log.
#
# Steps:
#   1. In lfs-orb: reflink-copy the project's Store, its SDK home (from the
#      rescue snapshot) and its Sandbox folder (from the rescue snapshot)
#      under /var/lib/layerfs-data/reopen-e2e/.
#   2. In a new container on volume subpaths of those copies: lfs reopen
#      --legacy-head, then check the Workspace (files, git), commit it,
#      read the commit back, and unmount with discard.
#   3. Remove the copies.
set -eu
bundle=${1:?usage: reopen-copy-e2e.sh <bundle-dir> <project> <sandbox-name> <namespace>}
project=${2:?project}
sandbox=${3:?sandbox name}
namespace=${4:?namespace}
repo=${0:A:h:h:h}
report=${bundle:A:h}/reopen-e2e-$project.log
e2e=/var/lib/layerfs-data/reopen-e2e
rescue=/var/lib/layerfs-data/rescue

docker exec -u root lfs-orb rm -rf "$e2e/data" "$e2e/layerfs"
docker exec -u root lfs-orb sh -c "set -e
  mkdir -p $e2e/data/stores/$project/global $e2e/data/home $e2e/layerfs/sandboxes
  cp -a --reflink=always /var/lib/layerfs-data/stores/$project/global/. $e2e/data/stores/$project/global/
  rm -f $e2e/data/stores/$project/global/store.sqlite-shm
  cp -a --reflink=always $rescue/home-2026-11-14-reboot/$project $e2e/data/home/
  # the SDK refuses a home that is not private, as on lfs-orb
  chmod 700 $e2e/data/home
  cp -a --reflink=always $rescue/sandboxes-2026-11-14-reboot/$sandbox $e2e/layerfs/sandboxes/"

volume=lfs-data
mount_data="type=volume,src=$volume,dst=/var/lib/layerfs-data,volume-subpath=reopen-e2e/data"
mount_sandboxes="type=volume,src=$volume,dst=/var/lib/layerfs,volume-subpath=reopen-e2e/layerfs"
ws=/var/lib/layerfs/sandboxes/$sandbox/workspaces/$namespace

docker run --rm --name lfs-reopen-e2e --cpus 4 --memory 8G --device /dev/fuse --cap-add SYS_ADMIN \
  --security-opt apparmor=unconfined \
  --mount "$mount_data" --mount "$mount_sandboxes" \
  -v "${bundle:A}":/opt/layerfs/bundle:ro -v "$repo/sdk/js":/opt/layerfs/sdk:ro \
  -v "$repo/tools/orbstack":/opt/layerfs/tools:ro \
  -e LAYERFS_RUNTIME=process -e LAYERFS_HOME=/var/lib/layerfs-data/home \
  -e LAYERFS_DAEMON=/opt/layerfs/bundle/layerfs-daemon -e LAYERFS_NATIVE=/opt/layerfs/bundle/layerfs.node \
  -e LAYERFS_STORE_ROOT=/var/lib/layerfs-data/stores -e LAYERFS_CONTAINER_PREFIX=layerfs- \
  -e PROJECT="$project" -e WS="$ws" \
  node:24-bookworm bash -c '
set -u
lfs() { node /opt/layerfs/tools/lfs.mjs "$@"; }
step() { echo "== $*"; }
step "overlay files before"; ls -la /var/lib/layerfs/sandboxes/*/overlay/
step "lfs reopen without --legacy-head (old records must refuse)"
lfs reopen "$PROJECT"; echo "exit $?"
step "lfs reopen --legacy-head"
t0=$(date +%s%N); lfs reopen "$PROJECT" --legacy-head; echo "exit $? ms $(( ($(date +%s%N) - t0) / 1000000 ))"
step "lfs reopen again (daemon runs: nothing restarted)"
lfs reopen "$PROJECT"; echo "exit $?"
step "mounts"; grep " /var/lib/layerfs/sandboxes/" /proc/self/mountinfo | cut -d" " -f5,9
step "workspace listing"; ls -la "$WS" | head -20; echo "entries: $(ls -A "$WS" | wc -l)"
step "file count and bytes under the Workspace"; find "$WS" -xdev -type f | wc -l; du -sb --apparent-size "$WS" | cut -f1
cd "$WS"
step "git"
git config --global --add safe.directory "$WS"
git log --oneline -3; git status --porcelain | wc -l | sed "s/^/changed paths: /"
git status --porcelain | head -10
git fsck --connectivity-only --no-progress 2>&1 | tail -3; echo "fsck exit ${PIPESTATUS[0]}"
step "write a probe file, commit"
echo "reopen e2e $(date -u +%FT%TZ)" > reopen-e2e-probe.txt
sb=$(ls /var/lib/layerfs-data/home/$PROJECT/sandboxes | head -1)
lfs commit "$PROJECT" "$sb" "$WS" "reopen e2e commit"; echo "exit $?"
branch=$(node -e "
const fs=require(\"fs\");const d=\"/var/lib/layerfs-data/home/$PROJECT/sandboxes/$sb\";
for (const f of fs.readdirSync(d)) if (f.startsWith(\"workspace-\")) { const b=fs.readFileSync(d+\"/\"+f); if (b.includes(Buffer.from(process.env.WS))) { const i=b.indexOf(Buffer.from(\"branch\"))+6+5; const n=b.readUInt32BE(i-4); console.log(b.slice(i,i+n).toString()); } }")
step "read the committed probe from branch $branch"
lfs read "$PROJECT" "$branch" /reopen-e2e-probe.txt; echo "exit $?"
cmp <(lfs read "$PROJECT" "$branch" /reopen-e2e-probe.txt | node -e "let s=\"\";process.stdin.on(\"data\",d=>s+=d).on(\"end\",()=>{const v=JSON.parse(s);process.stdout.write(typeof v===\"string\"?v:(v.content??v.text))})") reopen-e2e-probe.txt && echo "probe matches"
lfs log "$PROJECT" "$branch" | head -c 400; echo
cd /
step "unmount every reopened Workspace with discard"
for p in $(grep " /var/lib/layerfs/sandboxes/" /proc/self/mountinfo | cut -d" " -f5); do
  lfs unmount "$PROJECT" "$sb" "$p" --discard >/dev/null; echo "unmount --discard $p exit $?"
done
step "mounts after"; grep -c " /var/lib/layerfs/sandboxes/" /proc/self/mountinfo
step "overlay files after"; ls -la /var/lib/layerfs/sandboxes/*/overlay/
' 2>&1 | tee "$report"

docker exec -u root lfs-orb rm -rf "$e2e/data" "$e2e/layerfs"
echo "report: $report"
