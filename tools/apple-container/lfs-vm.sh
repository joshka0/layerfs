#!/bin/zsh
# Creates `lfs-vm`: one long-running Apple container that holds the LayerFS
# Stores, their daemons and the agents. Agents run as uid 501 (`agent`); the
# LayerFS control (`lfs`) and the daemons run as root.
#
# usage: tools/apple-container/lfs-vm.sh <bundle-dir>
#   <bundle-dir> holds layerfs-daemon and layerfs.node (process-e2e.sh makes it).
#
# Lasting state lives on the named volume `lfs-data` (made once), mounted at
# /var/lib/layerfs-data: the LayerFS home and Stores, and the agent home with
# its agent harnesses and logins. Recreating the container keeps them;
# Sandboxes (daemons, Overlays, mounts) start fresh and are reclaimed.
#
# A shared folder, read-write both ways: $LFS_SHARE (default ~/lfs-share) on
# the Mac is /share in the VM. Agents can write there, so treat its files as
# untrusted on the Mac.
#
# Agents may use DNS and HTTPS to any host, the Agent Vault proxy, the Mac
# bridge, and SSH of github.com and gitlab.com (iptables rules for uid 501). Root is not limited. vm-boot.sh applies the
# rule and starts sshd at each container start.
#
# iproute2 gives `ss`, which Herdr UI uses to list the ports agents listen on.
# rsync and zstd let memoryd on the Mac pull agent transcripts over SSH.
# clang (14 or newer) lets Bend build native binaries (`bend <file> -o <out>`).
# libsecret-1-0 is loaded by npm postinstall steps (uniwind-pro in praze).
# The second apt line holds what the active repos' builds and tests need:
# procps and build-essential (praze, yada, Rust crates), sqlite3
# (herdr-blackboard), python3-venv (dreamd, herdr-loom), postgresql-client
# (turtlr, infra-kit; Debian's 15), ffmpeg (dearday, praze-landing), and
# chromium, fonts-noto and xvfb for browser tests (jev-hand, effect-folio,
# pilotprospector, praze-landing).
#
# Docker Engine (root only) runs the shared services of services.compose.yml
# that every agent uses: Postgres, Redis and RustFS. Its data root is on the
# volume. CAP_SYS_PTRACE lets dockerd open the namespaces of an image that
# runs as a non-root user (RustFS).
#
# sshd takes key logins for `agent` only, on 127.0.0.1:$LFS_VM_SSH_PORT
# (default 2222) of the Mac. Put public keys in /home/agent/.ssh/authorized_keys;
# the host keys live on the volume, so they survive a recreate.
#
# The Herdr plugin joshka.lfs-bridge (herdr-plugins) makes this VM the Herdr
# machine `lfs vm` and installs the agent harnesses.
set -eu
repo=${0:A:h:h:h}
bundle=${1:?usage: lfs-vm.sh <bundle-dir>}
sfw_package=$(dirname "$(dirname "$(readlink -f "$(whence -p sfw)")")")

# `container` must not inherit an agent's proxy settings: it records its
# environment in launchd plists. Keep only the proxy's host and port.
proxy_url=${HTTPS_PROXY:?the Agent Vault proxy (HTTPS_PROXY) is not set}
proxy=${proxy_url#*@}
proxy_host=${proxy%%:*}
proxy_port=${${proxy#*:}%%/*}
unset HTTP_PROXY HTTPS_PROXY http_proxy https_proxy NO_PROXY no_proxy ALL_PROXY all_proxy
proxy_ip=$(dscacheutil -q host -a name "$proxy_host" | awk '/ip_address/ {print $2; exit}')
gateway=$(container network ls | awk '$1 == "default" { split($2, a, "/"); split(a[1], o, ".");
  print o[1] "." o[2] "." o[3] ".1" }')
bridge_port=${LFS_BRIDGE_PORT:-47111}
ssh_port=${LFS_VM_SSH_PORT:-2222}
share=${LFS_SHARE:-$HOME/lfs-share}
mkdir -p "$share"

container volume ls | awk 'NR > 1 {print $1}' | grep -qx lfs-data || container volume create lfs-data >/dev/null

container run -d --init --name lfs-vm --cpus 10 --memory 24G --cap-add CAP_SYS_ADMIN --cap-add CAP_NET_ADMIN --cap-add CAP_SYS_PTRACE \
  --dns 1.1.1.1 -p "127.0.0.1:$ssh_port:22" -v lfs-data:/var/lib/layerfs-data \
  -v "$bundle":/opt/layerfs/bundle:ro -v "$repo/sdk/js":/opt/layerfs/sdk:ro \
  -v "$repo/tools/apple-container":/opt/layerfs/tools:ro -v "$share":/share \
  node:24-bookworm /opt/layerfs/tools/vm-boot.sh run

# The Agent Vault proxy: its CA and its tailnet address (the VM's DNS has no
# MagicDNS). The proxy URL itself goes to each agent (lfs-bridge agent).
container exec -i lfs-vm sh -c 'cat > /opt/agent-vault-ca.pem && chmod 644 /opt/agent-vault-ca.pem' \
  < "${CURL_CA_BUNDLE:?the Agent Vault CA (CURL_CA_BUNDLE) is not set}"
container exec lfs-vm sh -c "echo '$proxy_ip $proxy_host' >> /etc/hosts"
# sfw for agents' package installs (it caches its firewall binary beside itself).
container exec lfs-vm mkdir -p /opt/sfw/dist /opt/sfw/.sfw-cache
container exec -i lfs-vm sh -c 'cat > /opt/sfw/dist/sfw.mjs' < "$sfw_package/dist/sfw.mjs"
container exec -i lfs-vm sh -c 'cat > /opt/sfw/package.json' < "$sfw_package/package.json"

container exec -i -e PROXY_IP="$proxy_ip" -e PROXY_PORT="$proxy_port" \
  -e GATEWAY="$gateway" -e BRIDGE_PORT="$bridge_port" lfs-vm bash -s <<'SETUP'
set -eu
data=/var/lib/layerfs-data
mkdir -p "$data/home" "$data/stores" "$data/agent-home" "$data/ssh-host-keys" /etc/lfs-vm
chmod 700 "$data/ssh-host-keys"
printf 'PROXY_IP=%s\nPROXY_PORT=%s\nGATEWAY=%s\nBRIDGE_PORT=%s\nGIT_SSH_HOSTS="github.com gitlab.com"\n' \
  "$PROXY_IP" "$PROXY_PORT" "$GATEWAY" "$BRIDGE_PORT" > /etc/lfs-vm/boot.env
chmod 700 "$data/home"
ln -sfn "$data/agent-home" /home/agent
id agent >/dev/null 2>&1 || useradd -u 501 -g 20 -M -d /home/agent -s /bin/bash agent
chown 501:20 "$data/agent-home" /opt/sfw/.sfw-cache
# System CAs plus the Agent Vault CA: proxied hosts are re-signed by it,
# passed-through ones keep their public certificates.
cat /etc/ssl/certs/ca-certificates.crt /opt/agent-vault-ca.pem > /opt/ca-bundle.pem
printf '#!/bin/sh\nexec node /opt/sfw/dist/sfw.mjs "$@"\n' > /usr/local/bin/sfw
cat > /usr/local/bin/lfs <<'LFS'
#!/bin/sh
export LAYERFS_RUNTIME=process LAYERFS_HOME=/var/lib/layerfs-data/home \
  LAYERFS_DAEMON=/opt/layerfs/bundle/layerfs-daemon LAYERFS_NATIVE=/opt/layerfs/bundle/layerfs.node \
  LAYERFS_STORE_ROOT=/var/lib/layerfs-data/stores
exec node /opt/layerfs/tools/lfs.mjs "$@"
LFS
chmod 755 /usr/local/bin/sfw /usr/local/bin/lfs
DEBIAN_FRONTEND=noninteractive apt-get -qq update >/dev/null
DEBIAN_FRONTEND=noninteractive apt-get -qq install -y iptables openssh-server iproute2 rsync zstd clang libsecret-1-0 >/dev/null
DEBIAN_FRONTEND=noninteractive apt-get -qq install -y --no-install-recommends procps build-essential \
  sqlite3 python3-venv postgresql-client ffmpeg chromium fonts-noto xvfb >/dev/null
install -m 0755 -d /etc/apt/keyrings /etc/docker
curl -fsSL https://download.docker.com/linux/debian/gpg -o /etc/apt/keyrings/docker.asc
echo "deb [arch=arm64 signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/debian bookworm stable" \
  > /etc/apt/sources.list.d/docker.list
DEBIAN_FRONTEND=noninteractive apt-get -qq update >/dev/null
DEBIAN_FRONTEND=noninteractive apt-get -qq install -y --no-install-recommends docker-ce docker-ce-cli \
  containerd.io docker-compose-plugin >/dev/null
printf '{"data-root": "%s/docker"}\n' "$data" > /etc/docker/daemon.json
# Host keys from the volume, or keep the new ones there.
if ls "$data"/ssh-host-keys/ssh_host_* >/dev/null 2>&1; then
  cp "$data"/ssh-host-keys/ssh_host_* /etc/ssh/
else
  cp /etc/ssh/ssh_host_* "$data/ssh-host-keys/"
fi
cat > /etc/ssh/sshd_config.d/lfs-vm.conf <<'SSHD'
PasswordAuthentication no
KbdInteractiveAuthentication no
PermitRootLogin no
AllowUsers agent
SSHD
usermod -p '*' agent
pkill -x sshd || true
/opt/layerfs/tools/vm-boot.sh apply
echo LFS_VM_READY
SETUP

# The proxy for every agent shell: ~/.lfs/profile.sh loads it (and the
# bridge session the Herdr plugin writes). Through stdin only.
{
  printf "export HTTPS_PROXY='%s' HTTP_PROXY='%s' https_proxy='%s' http_proxy='%s'\n" \
    "$proxy_url" "$proxy_url" "$proxy_url" "$proxy_url"
  # The bridge on the gateway is direct: the proxy cannot reach it.
  printf "export NO_PROXY='localhost,127.0.0.1,%s,%s' no_proxy='localhost,127.0.0.1,%s,%s'\n" \
    "$proxy_host" "$gateway" "$proxy_host" "$gateway"
  printf "export SSL_CERT_FILE=/opt/ca-bundle.pem CURL_CA_BUNDLE=/opt/ca-bundle.pem GIT_SSL_CAINFO=/opt/ca-bundle.pem\n"
  printf "export REQUESTS_CA_BUNDLE=/opt/ca-bundle.pem NODE_EXTRA_CA_CERTS=/opt/agent-vault-ca.pem\n"
  # Node's fetch ignores the proxy variables without this; agents have no DNS.
  printf "export NODE_USE_ENV_PROXY=1\n"
} | container exec -i -u 501:20 lfs-vm sh -c 'umask 077; mkdir -p /home/agent/.lfs; cat > /home/agent/.lfs/proxy.env'
container exec -i -u 501:20 lfs-vm sh -s <<'PROFILE'
cat > /home/agent/.lfs/profile.sh <<'SH'
# mise shims give each project the tool versions its .tool-versions pins.
export PATH="$HOME/.local/share/mise/shims:$HOME/.local/bin:$PATH"
export LANG=C.UTF-8 LC_ALL=C.UTF-8
for f in "$HOME/.lfs/proxy.env" "$HOME/.lfs/bridge.env"; do [ -f "$f" ] && . "$f"; done
SH
for rc in /home/agent/.bashrc /home/agent/.profile; do
  grep -qs '.lfs/profile.sh' "$rc" || echo '. "$HOME/.lfs/profile.sh"' >> "$rc"
done
PROFILE
