#!/bin/zsh
# Prints one JSON line of disk use for lfs-vm, for the LayerFS space trial.
#
# usage: tools/apple-container/lfs-space.sh [label]
#
#   image     bytes the Mac allocates for lfs-vm (the real cost): the lfs-data
#             volume (Stores, agent home) plus the container's root disk
#             (system packages and the Sandboxes' Overlays)
#   volume, rootfs   the two parts of image
#   stores    bytes of each project's Store
#   overlays  bytes of each Sandbox's Overlay (writes not yet committed)
#   logical   apparent bytes of each mounted Workspace (what a checkout of it would hold)
#
# Append it to a log to sample over time:
#   tools/apple-container/lfs-space.sh daily >> ~/.local/share/lfs-vm/space.jsonl
set -eu
label=${1:-}
base="$HOME/Library/Application Support/com.apple.container"
volume=$(( $(du -k "$base/volumes/lfs-data/volume.img" | cut -f1) * 1024 ))
rootfs=$(( $(du -k "$base/containers/lfs-vm/rootfs.ext4" | cut -f1) * 1024 ))
unset HTTP_PROXY HTTPS_PROXY http_proxy https_proxy NO_PROXY no_proxy ALL_PROXY all_proxy

container exec -i lfs-vm node - "$label" "$volume" "$rootfs" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" <<'JS'
const { execFileSync } = require("child_process");
const fs = require("fs");
const [label, volume, rootfs, at] = process.argv.slice(2);
// du exits 1 when files move while it counts (an install in progress); its
// total still holds.
const bytes = (path, apparent) => {
  const args = ["-sb", ...(apparent ? ["--apparent-size"] : []), path];
  let out;
  try {
    out = execFileSync("du", args, { stdio: ["ignore", "pipe", "ignore"] });
  } catch (error) {
    out = error.stdout;
  }
  return Number(String(out).split("\t")[0]) || 0;
};
const list = (dir) => (fs.existsSync(dir) ? fs.readdirSync(dir) : []);
const stores = Object.fromEntries(list("/var/lib/layerfs-data/stores").map((p) => [p, bytes(`/var/lib/layerfs-data/stores/${p}`)]));
const overlays = {};
const logical = {};
for (const sandbox of list("/var/lib/layerfs/sandboxes")) {
  const root = `/var/lib/layerfs/sandboxes/${sandbox}`;
  if (fs.existsSync(`${root}/overlay`)) overlays[sandbox] = bytes(`${root}/overlay`);
  for (const w of list(`${root}/workspaces`)) logical[`${sandbox}/${w}`] = bytes(`${root}/workspaces/${w}`, true);
}
console.log(JSON.stringify({
  at, label, image: Number(volume) + Number(rootfs), volume: Number(volume), rootfs: Number(rootfs), stores, overlays, logical,
}));
JS
