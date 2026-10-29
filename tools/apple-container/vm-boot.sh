#!/bin/bash
# The command of the workbench container (lfs-vm or lfs-orb), and the step
# that applies its boot state after setup. A container restart is a VM boot: iptables rules and
# sshd do not survive it, so this script brings them back.
#   vm-boot.sh apply   apply the agent firewall, start sshd and the shared
#                      services (when installed)
#   vm-boot.sh run     apply, then trim the disks hourly (the container command)
# The container runs with --init, which reaps the processes sshd leaves.
set -eu
config=/etc/lfs-vm/boot.env

apply() {
  [ -f "$config" ] || return 0
  . "$config"
  if command -v iptables >/dev/null; then
    # Agents (uid 501) may use DNS, HTTPS (port 443) to any host, the proxy,
    # the bridge, and SSH (port 22) of the Git hosts and the operator's servers. Direct HTTPS is there for
    # Socket Firewall: its npm wrapper connects to registries itself. Each Git
    # host is pinned in /etc/hosts and allowed by address. The Agent Vault
    # proxy expects TLS in every tunnel, so it cannot carry SSH.
    for tool in iptables ip6tables; do
      $tool -F OUTPUT
      $tool -A OUTPUT -m owner --uid-owner 501 -o lo -j ACCEPT
      $tool -A OUTPUT -m owner --uid-owner 501 -m conntrack --ctstate ESTABLISHED -j ACCEPT
    done
    iptables -A OUTPUT -m owner --uid-owner 501 -d "$PROXY_IP" -p tcp --dport "$PROXY_PORT" -j ACCEPT
    iptables -A OUTPUT -m owner --uid-owner 501 -d "$GATEWAY" -p tcp --dport "$BRIDGE_PORT" -j ACCEPT
    for proto in udp tcp; do
      iptables -A OUTPUT -m owner --uid-owner 501 -p "$proto" --dport 53 -j ACCEPT
    done
    iptables -A OUTPUT -m owner --uid-owner 501 -p tcp --dport 443 -j ACCEPT
    for host in ${GIT_SSH_HOSTS:-}; do
      # /etc/hosts is a bind mount: rewrite it in place, never replace it.
      grep -v " $host\$" /etc/hosts > /etc/hosts.lfs || true
      cat /etc/hosts.lfs > /etc/hosts && rm -f /etc/hosts.lfs
      ip=$(getent ahostsv4 "$host" | awk 'NR == 1 { print $1 }')
      [ -n "$ip" ] || continue
      echo "$ip $host" >> /etc/hosts
      iptables -A OUTPUT -m owner --uid-owner 501 -d "$ip" -p tcp --dport 22 -j ACCEPT
    done
    # The operator's servers (dev box, VPS) by address, from lfs-bridge install-vm.
    for ip in ${SSH_SERVER_IPS:-}; do
      iptables -A OUTPUT -m owner --uid-owner 501 -d "$ip" -p tcp --dport 22 -j ACCEPT
    done
    iptables -A OUTPUT -m owner --uid-owner 501 -j REJECT
    ip6tables -A OUTPUT -m owner --uid-owner 501 -j REJECT
  fi
  if [ -x /usr/sbin/sshd ] && ! pgrep -x sshd >/dev/null; then
    mkdir -p /run/sshd
    /usr/sbin/sshd
  fi
  # The VM must keep running without its services.
  services || echo "vm-boot: shared services did not start" >&2
}

# The shared services (services.compose.yml). In lfs-orb they are sibling
# containers: forward each to loopback (SERVICE_FORWARDS). In lfs-vm they run
# in Docker Engine, which needs IP forwarding; the container mounts /proc/sys
# read-only.
services() {
  if [ -n "${SERVICE_FORWARDS:-}" ]; then
    for forward in $SERVICE_FORWARDS; do
      port=${forward%%:*}
      pgrep -f "socat TCP-LISTEN:$port," >/dev/null && continue
      socat "TCP-LISTEN:$port,bind=127.0.0.1,fork,reuseaddr" "TCP:${forward#*:}" >/dev/null 2>&1 &
    done
    return 0
  fi
  command -v dockerd >/dev/null || return 0
  if ! pgrep -x dockerd >/dev/null; then
    mount -o remount,rw /proc/sys
    echo 1 > /proc/sys/net/ipv4/ip_forward
    dockerd > /var/log/dockerd.log 2>&1 &
  fi
  for _ in $(seq 30); do docker info >/dev/null 2>&1 && break; sleep 1; done
  docker compose -f /opt/layerfs/tools/services.compose.yml up -d --quiet-pull
}

# At `container stop`. Docker's cgroups are threaded, and the kernel cannot
# kill a threaded cgroup: the stop fails with errno 95 while they exist. A
# clean dockerd exit also shuts the services down cleanly.
shutdown() {
  if pgrep -x dockerd >/dev/null; then
    pkill -TERM -x dockerd
    for _ in $(seq 40); do pgrep -x dockerd >/dev/null || break; sleep 1; done
  fi
  [ -d /sys/fs/cgroup/docker ] && find /sys/fs/cgroup/docker -depth -type d -exec rmdir {} \;
  exit 0
}

case "${1:-}" in
  apply) apply ;;
  # The Mac gets space back from the disk images only after a trim: deleted
  # files (a discarded Workspace's Overlay) stay allocated until then.
  # The sleep runs in the background, so the TERM trap runs at once.
  run)
    trap shutdown TERM
    apply
    while :; do sleep 3600 & wait $!; fstrim -a >/dev/null 2>&1 || true; done ;;
  *) echo "usage: $0 apply|run" >&2; exit 2 ;;
esac
