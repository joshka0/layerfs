#!/usr/bin/env python3
"""Run the built 1.85.1 worker in a fresh container and disposable /tmp volume."""
import json
import os
from pathlib import Path
import subprocess
import signal
import sys
import uuid

resources = json.loads(Path(os.environ['REOPEN_RESOURCES']).read_text())
assert Path(resources['worktree']).resolve()==Path(__file__).resolve().parents[2], 'resources belong to another worktree'
identity = 'lfs-quint-replay-' + uuid.uuid4().hex
out = Path(os.environ['REOPEN_OUTPUT'])
out.mkdir(parents=True, exist_ok=True)
subprocess.run(['docker', 'volume', 'create', '--label', 'layerfs.quint-replay=' + identity, identity], check=True, stdout=subprocess.DEVNULL)
command = ['docker', 'run', '--rm', '--name', identity, '--cidfile', str(out / (identity + '.cid')), '--pull', 'never', '--network', 'none', '--privileged', '--device', '/dev/fuse',
           '--mount', 'type=volume,src=' + resources['linux_target_volume'] + ',dst=/artifacts,readonly',
           '--mount', 'type=volume,src=' + identity + ',dst=/tmp',
           '-e', 'REOPEN_DAEMON=/artifacts/debug/layerfs-daemon', '-i', resources['images']['build'], '/artifacts/debug/reopen-worker']
if os.environ.get('REOPEN_FIXTURE'):
    command[command.index('-i'):command.index('-i')] = ['-e', 'REOPEN_FIXTURE=' + os.environ['REOPEN_FIXTURE']]
(out / (identity + '.json')).write_text(json.dumps({'command': command, 'resources': resources}, indent=2) + '\n')
def stopped(number, frame):
    raise SystemExit(128 + number)
signal.signal(signal.SIGTERM, stopped)
signal.signal(signal.SIGINT, stopped)
try:
    result = subprocess.run(command)
finally:
    # These exact names belong to this invocation. No host paths or other
    # container's Store, Sandbox or rescue data is mounted.
    subprocess.run(['docker', 'rm', '-f', identity], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    subprocess.run(['docker', 'volume', 'rm', identity], check=True, stdout=subprocess.DEVNULL)
sys.exit(result.returncode)
