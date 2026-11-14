#!/bin/sh
# Inside node:24. Each run has its own SDK home and Store root.
#   old-p:  the main build inits p from /src, forks w, reads w
#   old-p2: the same again in a fresh home (is Init deterministic?)
#   new-p:  the branch build does the same in a fresh home
#   new-reads-old: the branch build reads old-p's Store in place
#   old-bs, new-bs: each build inits the backslash source
set -u
run() { bundle=$1 home=$2; shift 2; mkdir -p -m 700 /h/$home /s/$home; chmod 700 /h /h/$home; LAYERFS_HOME=/h/$home STORES=/s/$home LAYERFS_NATIVE=/$bundle/layerfs.node LAYERFS_DAEMON=/$bundle/layerfs-daemon node /rt/roundtrip.mjs "$@"; }
run bundle-main old p /src > /out/old-p.json
run bundle-main old2 p /src > /out/old-p2.json
run bundle-bs new p /src > /out/new-p.json
run bundle-bs old p > /out/new-reads-old.json
run bundle-main oldbs bs /src-bs > /out/old-bs.json
run bundle-bs newbs bs /src-bs > /out/new-bs.json
echo done
