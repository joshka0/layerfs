#!/bin/bash
# The command of the lfs-vm container, and the step that applies its boot
# state after setup. A container restart is a VM boot: iptables rules and
# sshd do not survive it, so this script brings them back.
#   vm-boot.sh apply   apply the agent firewall and start sshd (when installed)
#   vm-boot.sh run     apply, then trim the disks hourly (the container command)
# The container runs with --init, which reaps the processes sshd leaves.
set -eu
config=/etc/lfs-vm/boot.env

apply() {
  [ -f "$config" ] || return 0
  . "$config"
  if command -v iptables >/dev/null; then
    # Agents (uid 501) may use DNS, HTTPS (port 443) to any host, the proxy,
    # the bridge, and SSH (port 22) of the Git hosts. Direct HTTPS is there for
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
    iptables -A OUTPUT -m owner --uid-owner 501 -j REJECT
    ip6tables -A OUTPUT -m owner --uid-owner 501 -j REJECT
  fi
  if [ -x /usr/sbin/sshd ] && ! pgrep -x sshd >/dev/null; then
    mkdir -p /run/sshd
    /usr/sbin/sshd
  fi
}

case "${1:-}" in
  apply) apply ;;
  # The Mac gets space back from the disk images only after a trim: deleted
  # files (a discarded Workspace's Overlay) stay allocated until then.
  run) apply; while :; do sleep 3600; fstrim -a >/dev/null 2>&1 || true; done ;;
  *) echo "usage: $0 apply|run" >&2; exit 2 ;;
esac
