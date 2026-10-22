#!/bin/zsh
# Runs one agent harness in `lfs-vm`, as uid 501, in a LayerFS Workspace,
# through this shell's Agent Vault proxy.
#
# usage: tools/apple-container/lfs-agent.sh <workspace-path> <harness> [args...]
#   for example: lfs-agent.sh /var/lib/layerfs/sandboxes/layerfs-sum-1/workspaces/1 claude
#
# The proxy URL carries the agent's vault credential: it goes to an agent-only
# file in the VM through stdin, never into a command line or a log.
set -eu
workspace=${1:?usage: lfs-agent.sh <workspace-path> <harness> [args...]}
shift
proxy=${HTTPS_PROXY:?the Agent Vault proxy (HTTPS_PROXY) is not set}
proxy_host=${${proxy#*@}%%:*}
unset HTTP_PROXY HTTPS_PROXY http_proxy https_proxy NO_PROXY no_proxy ALL_PROXY all_proxy

printf '%s\n' "$proxy" | container exec -i -u 501:20 lfs-vm sh -c "
  read -r p
  umask 077
  cat > /home/agent/.agent-vault.env <<ENV
export HTTPS_PROXY='\$p' HTTP_PROXY='\$p' https_proxy='\$p' http_proxy='\$p'
export NO_PROXY='localhost,127.0.0.1,$proxy_host' no_proxy='localhost,127.0.0.1,$proxy_host'
export SSL_CERT_FILE=/opt/ca-bundle.pem CURL_CA_BUNDLE=/opt/ca-bundle.pem GIT_SSL_CAINFO=/opt/ca-bundle.pem
export REQUESTS_CA_BUNDLE=/opt/ca-bundle.pem NODE_EXTRA_CA_CERTS=/opt/agent-vault-ca.pem
ENV"
tty=()
[ -t 0 ] && tty=(-t)
exec container exec -i $tty -u 501:20 -e HOME=/home/agent -w "$workspace" lfs-vm \
  bash -c '. /home/agent/.agent-vault.env; exec "$@"' agent "$@"
