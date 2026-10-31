#!/bin/sh
# Writes seccomp.json, the seccomp profile of the workbench lfs-orb (lfs-vm.sh):
# Docker's default profile (moby/profiles seccomp v0.2.3, as Docker 29.8 ships it)
# plus what rootless Docker inside the workbench needs (ADR 0005): the
# keyring calls, which runc and crun use to start a container, and pivot_root
# for holders of CAP_SYS_ADMIN, which the workbench has.
#   tools/orbstack/seccomp.sh
set -eu
here=$(cd "$(dirname "$0")" && pwd)
url=https://raw.githubusercontent.com/moby/profiles/seccomp/v0.2.3/seccomp/default.json
want=536529b665dd0972c37bfb569f5d4ac8a53592e7b00752bc39ff063ca9864c74
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
curl -fsSL -o "$tmp" "$url"
got=$(shasum -a 256 "$tmp" | awk '{print $1}')
[ "$got" = "$want" ] || { echo "seccomp.sh: $url does not match its pinned digest" >&2; exit 1; }
jq '.syscalls += [
  {"names": ["add_key", "keyctl", "request_key"], "action": "SCMP_ACT_ALLOW"},
  {"names": ["pivot_root"], "action": "SCMP_ACT_ALLOW", "includes": {"caps": ["CAP_SYS_ADMIN"]}}
]' "$tmp" > "$here/seccomp.json"
