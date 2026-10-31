#!/bin/zsh
# Prints one JSON line of disk use for lfs-orb, for the LayerFS space trial.
#
# usage: tools/orbstack/lfs-space.sh [label]
#
#   image     bytes lfs-orb holds in OrbStack (the real cost): the sum of
#             volume, rootfs and sandboxes
#   volume    the lfs-data volume (Stores, agent home)
#   rootfs    the container's writable layer (system packages)
#   sandboxes the volume at /var/lib/layerfs (the Sandboxes' Overlays)
#   stores    bytes of each project's Store
#   overlays  bytes of each Sandbox's Overlay (writes not yet committed)
#   logical   apparent bytes of each mounted Workspace (what a checkout of it would hold)
#
# Append it to a log to sample over time:
#   tools/orbstack/lfs-space.sh daily >> ~/.local/share/lfs-vm/space.jsonl
set -eu
label=${1:-}
rootfs=$(docker container inspect --size -f '{{.SizeRw}}' lfs-orb)
unset HTTP_PROXY HTTPS_PROXY http_proxy https_proxy NO_PROXY no_proxy ALL_PROXY all_proxy

docker exec -i lfs-orb node - "$label" "$rootfs" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" <<'JS'
const { execFileSync } = require("child_process");
const fs = require("fs");
const [label, rootfs, at] = process.argv.slice(2);
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
const volume = bytes("/var/lib/layerfs-data");
const sandboxes = bytes("/var/lib/layerfs");
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
  at, label, image: volume + Number(rootfs) + sandboxes, volume, rootfs: Number(rootfs), sandboxes, stores, overlays, logical,
}));
JS
