#!/bin/zsh
# Starts `lfs-vm`: one long-running Apple container that holds the LayerFS
# Store, its daemons and the agents. Agents run as uid 501 (`agent`); the
# LayerFS control (`lfs`) runs as root.
#
# usage: tools/apple-container/lfs-vm.sh <bundle-dir>
#   <bundle-dir> holds layerfs-daemon and layerfs.node (process-e2e.sh makes it).
#
# Mounts: the bundle and this repo's SDK read-only; pi's managed release
# read-only; ~/.pi/agent read-write, so pi in the VM shares the Mac's
# auth.json (a token refresh on either side stays valid for both).
#
# Then, for example:
#   container exec -it -u 501:20 -w <workspace> lfs-vm pi
set -eu
repo=${0:A:h:h:h}
bundle=${1:?usage: lfs-vm.sh <bundle-dir>}
pi_agent=$HOME/.pi/agent
pi_release=$pi_agent/install/releases/$(<"$pi_agent/install/current-version")

# `container` must not inherit an agent's proxy settings: it records its
# environment in launchd plists.
env -u HTTP_PROXY -u HTTPS_PROXY -u http_proxy -u https_proxy -u NO_PROXY -u no_proxy \
  container run -d --name lfs-vm --cpus 4 --memory 8G --cap-add CAP_SYS_ADMIN --dns 1.1.1.1 \
  -v "$bundle":/opt/layerfs/bundle:ro -v "$repo/sdk/js":/opt/layerfs/sdk:ro \
  -v "$repo/tools/apple-container":/opt/layerfs/tools:ro \
  -v "$pi_release":/opt/pi:ro -v "$pi_agent":/opt/pi-host \
  node:24-bookworm sleep infinity

container exec -i lfs-vm bash -s <<'SETUP'
set -eu
id agent >/dev/null 2>&1 || useradd -u 501 -g 20 -m -d /home/agent -s /bin/bash agent
# pi in the VM: Mac settings without its Mac-path extensions and packages,
# and the Mac's own auth.json.
d=/home/agent/.pi-vm
mkdir -p "$d"
node -e '
  const fs = require("fs");
  const s = JSON.parse(fs.readFileSync("/opt/pi-host/settings.json", "utf8"));
  delete s.packages;
  fs.writeFileSync(process.argv[1] + "/settings.json", JSON.stringify(s, null, 2));
' "$d"
cp /opt/pi-host/models.json "$d/"
ln -sfn /opt/pi-host/auth.json "$d/auth.json"
chown -R 501:20 "$d"
cat > /usr/local/bin/pi <<'PI'
#!/bin/sh
export PI_CODING_AGENT_DIR=/home/agent/.pi-vm PI_SKIP_VERSION_CHECK=1
exec /opt/pi/node_modules/.bin/pi "$@"
PI
cat > /usr/local/bin/lfs <<'LFS'
#!/bin/sh
export LAYERFS_RUNTIME=process LAYERFS_HOME=/root/lfs-home LAYERFS_DAEMON=/opt/layerfs/bundle/layerfs-daemon \
  LAYERFS_NATIVE=/opt/layerfs/bundle/layerfs.node LAYERFS_STORE_ROOT=/var/lib/layerfs/stores
exec node /opt/layerfs/tools/lfs.mjs "$@"
LFS
chmod 755 /usr/local/bin/pi /usr/local/bin/lfs
echo LFS_VM_READY
SETUP
