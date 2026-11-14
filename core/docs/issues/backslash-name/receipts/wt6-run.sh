#!/bin/sh
# usage: run.sh <bundle-dir>. Runs memoryd's worktree_test (all 9 tests,
# WT6 among them) from inside a mounted LayerFS Workspace.
set -u
mkdir -m 700 -p /h /s
export LAYERFS_HOME=/h LAYERFS_NATIVE=$1/layerfs.node LAYERFS_DAEMON=$1/layerfs-daemon LAYERFS_RUNTIME=process
ws=$(node /wt6/mount.mjs) || { echo "mount failed: $ws"; exit 1; }
echo "workspace: $ws"
stat -f -c 'filesystem: %T' "$ws"
cd "$ws" || exit 1
# as the agent user of lfs-orb (uid 501), who owns the Workspace files
mkdir -p /tmp/h501 && chown 501:20 /tmp/h501
timeout 110 setpriv --reuid 501 --regid 20 --clear-groups env HOME=/tmp/h501 /b/m/zig/daemon-next/zig-out/bin/worktree_test --cache-dir=./.zig-cache > /tmp/wt.log 2>&1
code=$?
grep -E "^[0-9]+/9" /tmp/wt.log | sed -E 's/^([0-9]+\/9 worktree_test\.test\.WT[0-9]).*\.\.\.(.*)$/\1 ... \2/' | cut -c1-120
grep -E "passed|failed" /tmp/wt.log | tail -2
echo "worktree_test exit $code"
