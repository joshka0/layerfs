#!/usr/bin/env python3
"""Build the isolated worker with the registered private Rust 1.85.1 resources."""
import argparse
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT/'tools'))
from worktree_resources import load, lease, docker_build, execute, operation

p=argparse.ArgumentParser()
p.add_argument('--resources', required=True)
p.add_argument('--resolve', action='store_true')
p.add_argument('--clippy', action='store_true')
a=p.parse_args()
r=load(a.resources)
assert Path(r['worktree']).resolve()==ROOT, 'resources belong to another worktree'
with lease(r,'linux'):
    out=operation(r,'reopen-worker-build')
    extra=['--mount','type=bind,src='+str(ROOT/'quint/sandbox-reopen/generated/src/core_process.rs')+',dst=/work/core/crates/layerfs-sandbox/src/backend/process.rs,readonly']
    for copied, target in [('daemon_cli.rs','cli.rs'),('daemon_failure.rs','failure.rs')]:
        extra += ['--mount','type=bind,src='+str(ROOT/'quint/sandbox-reopen/generated/src'/copied)+',dst=/work/core/crates/layerfs-daemon/src/application/'+target+',readonly']
    if a.resolve:
        command=['cargo','+1.85.1','check','--manifest-path','quint/sandbox-reopen/worker/Cargo.toml','--offline','-j','2']
        lock=ROOT/'quint/sandbox-reopen/worker/Cargo.lock'
        extra += ['--mount','type=bind,src='+str(lock)+',dst=/work/quint/sandbox-reopen/worker/Cargo.lock']
    elif a.clippy:
        command=['bash','-c','set -e\nrustup component add clippy\ncargo +1.85.1 clippy --manifest-path quint/sandbox-reopen/worker/Cargo.toml --locked --offline -j 2 -- -D warnings']
    else:
        command=['cargo','+1.85.1','build','--manifest-path','quint/sandbox-reopen/worker/Cargo.toml','--locked','--offline','-j','2']
    if not (a.resolve or a.clippy):
        daemon=['cargo','+1.85.1','build','--manifest-path','core/Cargo.toml','--bin','layerfs-daemon','--locked','--offline','-j','2']
        execute(docker_build(r,daemon,extra,network='none'),r['worktree'],log=out/'daemon.log')
    execute(docker_build(r,command,extra,network='default' if a.clippy else 'none'),r['worktree'],log=out/'output.log')
    print(out)
