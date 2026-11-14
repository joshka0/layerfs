#!/bin/sh
# lfs-proxy-env.sh <workbench> --vault <vault> [--shim <host>:<port>] [--out <path>]
# Writes a workbench's ~/.lfs/proxy.env: the proxy and CA variables every agent shell loads
# (~/.lfs/profile.sh). The proxy is the Mac av-proxy, reached through the OrbStack gateway; the URL
# names the vault and holds no token (contract: skills agent-vault/scripts/av-proxy.md, "proxy.env writer").
# It reads no Keychain, no token file and no proxy variable of its caller.
# The vault is an input (lfs-vm.sh passes its shell's AGENT_VAULT_VAULT). Defaults: shim
# 0.250.250.254:14390; out `-` (standard output, which lfs-vm.sh pipes into the workbench).
set -eu
usage() { echo "usage: lfs-proxy-env.sh <workbench> --vault <vault> [--shim <host>:<port>] [--out <path>]" >&2; exit 64; }
[ "$#" -ge 1 ] || usage
workbench=$1
shift
vault= shim=0.250.250.254:14390 out=-
while [ "$#" -gt 0 ]; do
  [ "$#" -ge 2 ] || usage
  case $1 in
    --vault) vault=$2 ;;
    --shim) shim=$2 ;;
    --out) out=$2 ;;
    *) usage ;;
  esac
  shift 2
done
[ -n "$vault" ] || { echo "lfs-proxy-env: --vault is required" >&2; exit 64; }
case $workbench in ''|-*) usage ;; esac
case $vault in
  ''|-*|*[!a-z0-9-]*) echo "lfs-proxy-env: vault name must match [a-z0-9][a-z0-9-]{0,62}: $vault" >&2; exit 64 ;;
esac
[ "${#vault}" -le 63 ] || { echo "lfs-proxy-env: vault name longer than 63 characters" >&2; exit 64; }
host=${shim%:*} port=${shim##*:}
case $shim in *:*) ;; *) host= ;; esac
case $port in ''|*[!0-9]*) host= ;; esac
case $host in ''|*[!A-Za-z0-9.-]*) echo "lfs-proxy-env: --shim must be <host>:<port>: $shim" >&2; exit 64 ;; esac

gateway=0.250.250.254
url=http://av:$vault@$host:$port
tz=$(readlink /etc/localtime 2>/dev/null | sed 's|.*/zoneinfo/||')
write() {
  printf "export HTTPS_PROXY='%s' HTTP_PROXY='%s' https_proxy='%s' http_proxy='%s'\n" "$url" "$url" "$url" "$url"
  # The bridge on the gateway is direct. Package installs are direct too (the firewall allows port
  # 443): the registry and Socket Firewall need no key, and one install opens more connections
  # than the broker's rate limit allows every agent together.
  direct="localhost,127.0.0.1,$gateway,registry.npmjs.org,.socket.dev"
  printf "export NO_PROXY='%s' no_proxy='%s'\n" "$direct" "$direct"
  printf "export SSL_CERT_FILE=/opt/ca-bundle.pem CURL_CA_BUNDLE=/opt/ca-bundle.pem GIT_SSL_CAINFO=/opt/ca-bundle.pem\n"
  printf "export REQUESTS_CA_BUNDLE=/opt/ca-bundle.pem NODE_EXTRA_CA_CERTS=/opt/agent-vault-ca.pem\n"
  # gh and glab need a token to start; the broker replaces these placeholders with the vault's
  # GitHub and GitLab keys; other workbenches' vaults have neither.
  [ "$workbench" != lfs-orb ] || printf "export GH_TOKEN=agent-vault GITLAB_TOKEN=agent-vault\n"
  # Node's fetch ignores the proxy variables without this; agents have no DNS.
  printf "export NODE_USE_ENV_PROXY=1\n"
  # The Mac's time zone, so summaries and notes carry the operator's local time.
  printf "export TZ='%s'\n" "$tz"
}
if [ "$out" = - ]; then
  write
else
  dir=$(dirname -- "$out")
  tmp=$(umask 077; mktemp "$dir/.proxy.env.XXXXXX")
  write > "$tmp"
  chmod 600 "$tmp"
  mv "$tmp" "$out"
fi
