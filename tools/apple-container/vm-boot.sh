#!/bin/bash
# The command of the lfs-vm container, and the step that applies its boot
# state after setup. A container restart is a VM boot: iptables rules and
# sshd do not survive it, so this script brings them back.
#   vm-boot.sh apply   apply the agent firewall and start sshd (when installed)
#   vm-boot.sh run     apply, then wait forever (the container command)
# The container runs with --init, which reaps the processes sshd leaves.
set -eu
config=/etc/lfs-vm/boot.env

apply() {
  [ -f "$config" ] || return 0
  . "$config"
  if command -v iptables >/dev/null; then
    # Agents (uid 501) may connect only to the proxy and the bridge.
    for tool in iptables ip6tables; do
      $tool -F OUTPUT
      $tool -A OUTPUT -m owner --uid-owner 501 -o lo -j ACCEPT
      $tool -A OUTPUT -m owner --uid-owner 501 -m conntrack --ctstate ESTABLISHED -j ACCEPT
    done
    iptables -A OUTPUT -m owner --uid-owner 501 -d "$PROXY_IP" -p tcp --dport "$PROXY_PORT" -j ACCEPT
    iptables -A OUTPUT -m owner --uid-owner 501 -d "$GATEWAY" -p tcp --dport "$BRIDGE_PORT" -j ACCEPT
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
  run) apply; exec sleep infinity ;;
  *) echo "usage: $0 apply|run" >&2; exit 2 ;;
esac
