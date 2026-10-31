#!/bin/zsh
# Creates the workbench `lfs-orb`: one long-running Linux container in
# OrbStack (Docker) that holds the LayerFS Stores, their daemons and the
# agents. Agents run as uid 501 (`agent`); the LayerFS control (`lfs`) and
# the daemons run as root.
#
# usage: tools/orbstack/lfs-vm.sh <bundle-dir>
#   <bundle-dir> holds layerfs-daemon and layerfs.node (process-e2e.sh makes it).
#   Run it in a proxied shell: `agent-vault-run shared -- tools/orbstack/lfs-vm.sh ...`.
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
# Parts of the fleet-memory Obsidian vault ($LFS_VAULT, default
# ~/vaults/fleet-memory), at /vaults/fleet-memory and, for agents, at
# ~/vaults as on the Mac: features, diagrams and this year's summaries
# read-write, router read-only. The generated memory pages stay out (agents
# read them with `mac mem`), and so do summaries/foxway and the rest of
# summaries: Foxway data must not reach the devin harness in the VM.
# Each year's summaries folder is its own mount: at a new year, add it and
# recreate. Removal condition: Foxway summaries move out of summaries/.
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
# pilotprospector, praze-landing). ripgrep, jq and fd-find (as `fd`) are
# the search tools agents expect.
#
# The shared services of services.compose.yml that every agent uses on
# 127.0.0.1: Postgres, Redis and RustFS. OrbStack runs them as sibling
# containers on the Docker network `lfs-orb` (services.orb.yml), and socat
# forwards each port to the workbench's loopback. No Docker runs inside
# `lfs-orb`.
#
# sshd takes key logins for `agent` only, on 127.0.0.1:$LFS_VM_SSH_PORT
# (default 2223) of the Mac. Put public keys in /home/agent/.ssh/authorized_keys;
# the host keys live on the volume, so they survive a recreate.
#
# The Herdr plugin joshka.lfs-bridge (herdr-plugins) makes this container the
# Herdr machine `lfs orb` and installs the agent harnesses.
set -eu
repo=${0:A:h:h:h}
bundle=${1:?usage: lfs-vm.sh <bundle-dir>}
sfw_package=$(dirname "$(dirname "$(readlink -f "$(whence -p sfw)")")")
name=lfs-orb
ssh_port=${LFS_VM_SSH_PORT:-2223}
# OrbStack's VM has 16 GB unless its memory_mib setting says otherwise.
memory=${LFS_VM_MEMORY:-12G}

# The docker commands must not inherit an agent's proxy settings. Keep only
# the proxy's host and port.
proxy_url=${HTTPS_PROXY:?the Agent Vault proxy (HTTPS_PROXY) is not set}
proxy=${proxy_url#*@}
proxy_host=${proxy%%:*}
proxy_port=${${proxy#*:}%%/*}
unset HTTP_PROXY HTTPS_PROXY http_proxy https_proxy NO_PROXY no_proxy ALL_PROXY all_proxy
proxy_ip=$(dscacheutil -q host -a name "$proxy_host" | awk '/ip_address/ {print $2; exit}')
bridge_port=${LFS_BRIDGE_PORT:-47111}
share=${LFS_SHARE:-$HOME/lfs-share}
mkdir -p "$share"
vault=${LFS_VAULT:-$HOME/vaults/fleet-memory}
year=$(date +%Y)
mkdir -p "$vault/summaries/$year"

docker volume inspect lfs-data >/dev/null 2>&1 || docker volume create lfs-data >/dev/null
docker network inspect lfs-orb >/dev/null 2>&1 || docker network create lfs-orb >/dev/null
# Where the workbench reaches the Mac (the bridge): OrbStack's
# host.docker.internal, the Mac's loopback.
gateway=0.250.250.254
# The proxy's tailnet address, in the hosts file Docker keeps across restarts.
# LayerFS Sandboxes need a real filesystem: on OrbStack's overlay root, a
# commit finds no changes. An anonymous volume holds them; `docker rm -v`
# removes it, so a recreate starts them fresh.
flags=(--device /dev/fuse --network lfs-orb --restart unless-stopped --add-host "$proxy_host:$proxy_ip"
  -v /var/lib/layerfs)

docker run -d --init --name $name --cpus 10 --memory $memory --cap-add CAP_SYS_ADMIN --cap-add CAP_NET_ADMIN --cap-add CAP_SYS_PTRACE \
  $flags -p "127.0.0.1:$ssh_port:22" -v lfs-data:/var/lib/layerfs-data \
  -v "$bundle":/opt/layerfs/bundle:ro -v "$repo/sdk/js":/opt/layerfs/sdk:ro \
  -v "$repo/tools/orbstack":/opt/layerfs/tools:ro -v "$share":/share \
  -v "$vault/features":/vaults/fleet-memory/features -v "$vault/diagrams":/vaults/fleet-memory/diagrams \
  -v "$vault/summaries/$year":"/vaults/fleet-memory/summaries/$year" \
  -v "$vault/router":/vaults/fleet-memory/router:ro \
  node:24-bookworm /opt/layerfs/tools/vm-boot.sh run

# The Agent Vault proxy: its CA and its tailnet address (the VM's DNS has no
# MagicDNS). The proxy URL itself goes to each agent (lfs-bridge agent).
docker exec -i $name sh -c 'cat > /opt/agent-vault-ca.pem && chmod 644 /opt/agent-vault-ca.pem' \
  < "${CURL_CA_BUNDLE:?the Agent Vault CA (CURL_CA_BUNDLE) is not set}"
# sfw for agents' package installs (it caches its firewall binary beside itself).
docker exec $name mkdir -p /opt/sfw/dist /opt/sfw/.sfw-cache
docker exec -i $name sh -c 'cat > /opt/sfw/dist/sfw.mjs' < "$sfw_package/dist/sfw.mjs"
docker exec -i $name sh -c 'cat > /opt/sfw/package.json' < "$sfw_package/package.json"

docker exec -i -e PROXY_IP="$proxy_ip" -e PROXY_PORT="$proxy_port" \
  -e GATEWAY="$gateway" -e BRIDGE_PORT="$bridge_port" $name bash -s <<'SETUP'
set -eu
# OrbStack's docker exec starts with umask 0000.
umask 022
data=/var/lib/layerfs-data
mkdir -p "$data/home" "$data/stores" "$data/agent-home" "$data/ssh-host-keys" /etc/lfs-vm
chmod 700 "$data/ssh-host-keys"
printf 'PROXY_IP=%s\nPROXY_PORT=%s\nGATEWAY=%s\nBRIDGE_PORT=%s\nGIT_SSH_HOSTS="github.com gitlab.com"\n' \
  "$PROXY_IP" "$PROXY_PORT" "$GATEWAY" "$BRIDGE_PORT" > /etc/lfs-vm/boot.env
# The sibling services, forwarded to loopback (<port>:<host>:<port>).
echo 'SERVICE_FORWARDS="5432:postgres:5432 6379:redis:6379 9000:rustfs:9000"' >> /etc/lfs-vm/boot.env
chmod 700 "$data/home"
ln -sfn "$data/agent-home" /home/agent
# The Mac's ~/vaults/wiki/summaries is a link to the fleet-memory summaries.
mkdir -p /vaults/wiki
ln -sfn /vaults/fleet-memory/summaries /vaults/wiki/summaries
ln -sfn /vaults /home/agent/vaults
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
DEBIAN_FRONTEND=noninteractive apt-get -qq install -y --no-install-recommends socat >/dev/null
DEBIAN_FRONTEND=noninteractive apt-get -qq install -y --no-install-recommends ripgrep jq fd-find >/dev/null
ln -sfn /usr/bin/fdfind /usr/local/bin/fd
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

# OrbStack runs the shared services next to the workbench.
docker compose -f "$repo/tools/orbstack/services.compose.yml" \
  -f "$repo/tools/orbstack/services.orb.yml" up -d --quiet-pull

# The proxy for every agent shell: ~/.lfs/profile.sh loads it (and the
# bridge session the Herdr plugin writes). Through stdin only.
{
  printf "export HTTPS_PROXY='%s' HTTP_PROXY='%s' https_proxy='%s' http_proxy='%s'\n" \
    "$proxy_url" "$proxy_url" "$proxy_url" "$proxy_url"
  # The bridge on the gateway is direct: the proxy cannot reach it. Package
  # installs are direct too (the firewall allows port 443): the registry and
  # Socket Firewall need no key, and one install opens more connections than
  # the shared proxy's rate limit allows every agent together.
  direct="localhost,127.0.0.1,$proxy_host,$gateway,registry.npmjs.org,.socket.dev"
  printf "export NO_PROXY='%s' no_proxy='%s'\n" "$direct" "$direct"
  printf "export SSL_CERT_FILE=/opt/ca-bundle.pem CURL_CA_BUNDLE=/opt/ca-bundle.pem GIT_SSL_CAINFO=/opt/ca-bundle.pem\n"
  printf "export REQUESTS_CA_BUNDLE=/opt/ca-bundle.pem NODE_EXTRA_CA_CERTS=/opt/agent-vault-ca.pem\n"
  # gh needs a token to start; the proxy replaces this placeholder with the
  # vault's GitHub key.
  printf "export GH_TOKEN=agent-vault\n"
  # Node's fetch ignores the proxy variables without this; agents have no DNS.
  printf "export NODE_USE_ENV_PROXY=1\n"
  # The Mac's time zone, so summaries and notes carry the operator's local time.
  printf "export TZ='%s'\n" "$(readlink /etc/localtime | sed 's|.*/zoneinfo/||')"
} | docker exec -i -u 501:20 $name sh -c 'umask 077; mkdir -p /home/agent/.lfs; cat > /home/agent/.lfs/proxy.env'
docker exec -i -u 501:20 $name sh -s <<'PROFILE'
umask 022
cat > /home/agent/.lfs/profile.sh <<'SH'
# mise shims give each project the tool versions its .tool-versions pins.
export PATH="$HOME/.local/share/mise/shims:$HOME/.local/bin:$PATH"
export LANG=C.UTF-8 LC_ALL=C.UTF-8
# A missing file must not fail the profile: callers run with `set -e`.
for f in "$HOME/.lfs/proxy.env" "$HOME/.lfs/bridge.env"; do [ ! -f "$f" ] || . "$f"; done
SH
for rc in /home/agent/.bashrc /home/agent/.profile; do
  grep -qs '.lfs/profile.sh' "$rc" || echo '. "$HOME/.lfs/profile.sh"' >> "$rc"
done
PROFILE
