#!/bin/sh
# lfs-proxy-apply.sh <workbench> --vault <vault> [--shim <IPv4>:<port>]
# Points a running workbench's agents at the Mac av-proxy, from one shim address:
#   1. ~/.lfs/proxy.env of the agent user (lfs-proxy-env.sh writes it; no token),
#   2. PROXY_IP and PROXY_PORT in /etc/lfs-vm/boot.env, the one proxy address the agent firewall
#      allows (vm-boot.sh apply; any earlier proxy address, such as the old broker, goes),
#   3. vm-boot.sh apply, which re-reads boot.env and rebuilds the OUTPUT chain.
# lfs-vm.sh runs it for a new workbench; the cutover runs it for an existing one, so both get the
# same rule. Default shim: 0.250.250.254:14390 (OrbStack's gateway to the Mac loopback).
set -eu
usage() { echo "usage: lfs-proxy-apply.sh <workbench> --vault <vault> [--shim <IPv4>:<port>]" >&2; exit 64; }
[ "$#" -ge 3 ] || usage
workbench=$1
shift
vault= shim=0.250.250.254:14390
while [ "$#" -gt 0 ]; do
  [ "$#" -ge 2 ] || usage
  case $1 in
    --vault) vault=$2 ;;
    --shim) shim=$2 ;;
    *) usage ;;
  esac
  shift 2
done
ip=${shim%:*} port=${shim##*:}
# The firewall allows an address, so the shim host is an IPv4 address here.
echo "$ip" | grep -Eq '^([0-9]{1,3}\.){3}[0-9]{1,3}$' && case $port in ''|*[!0-9]*) false ;; esac \
  || { echo "lfs-proxy-apply: --shim must be <IPv4>:<port>: $shim" >&2; exit 64; }
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)

"$here/lfs-proxy-env.sh" "$workbench" --vault "$vault" --shim "$shim" \
  | docker exec -i -u 501:20 "$workbench" sh -c 'umask 077; mkdir -p /home/agent/.lfs; cat > /home/agent/.lfs/proxy.env'
docker exec -i -e PROXY_IP="$ip" -e PROXY_PORT="$port" "$workbench" sh -c '
  set -eu
  f=/etc/lfs-vm/boot.env
  [ -f "$f" ] || { echo "lfs-proxy-apply: no $f in this workbench" >&2; exit 66; }
  { grep -v -e "^PROXY_IP=" -e "^PROXY_PORT=" "$f" || true; printf "PROXY_IP=%s\nPROXY_PORT=%s\n" "$PROXY_IP" "$PROXY_PORT"; } > "$f.new"
  mv "$f.new" "$f"
  /opt/layerfs/tools/vm-boot.sh apply'
echo "lfs-proxy-apply: $workbench agents use $shim (vault $vault)"
