#!/usr/bin/env python3
"""Build one source-bound arm and run bounded Quint replay against its artifact."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import zipfile

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
p = argparse.ArgumentParser()
p.add_argument('--resources', required=True)
p.add_argument('--out', required=True)
p.add_argument('--plant', default='none')
p.add_argument('--seed', default='1')
p.add_argument('--samples', type=int, default=1)
p.add_argument('--liveness', action='store_true')
p.add_argument('--busy', choices=['project', 'sandbox'])
p.add_argument('--fixture', choices=['missing-base', 'handshake-failure', 'rootless'])
p.add_argument('--witness', choices=['overlay', 'boot', 'guard', 'release', 'running', 'orphan', 'failed-duplicate', 'report', 'fallback', 'unattached', 'status', 'listener', 'after-record', 'stop-failed', 'recovery-save', 'fallback-failure', 'folder-lock', 'kill-deadline', 'stale', 'folder-lock-release', 'fast-holder'])
a = p.parse_args()
out = Path(a.out).resolve()
out.mkdir(parents=True, exist_ok=False)
resources = json.loads(Path(a.resources).read_text())
commands = []


def source_identity():
    files = [*sorted((HERE/'worker').glob('*.rs')), HERE/'instrument.py', HERE/'build-worker.py', HERE/'container.py', HERE/'run-replay.py', *sorted((HERE/'replay/src').glob('*.rs')), ROOT/'.cargo/config.toml', ROOT/'core/Cargo.toml', ROOT/'core/Cargo.lock', HERE/'worker/Cargo.toml', HERE/'worker/rust-toolchain.toml', HERE/'worker/Cargo.lock', HERE/'replay/Cargo.toml', HERE/'replay/rust-toolchain.toml', HERE/'replay/Cargo.lock']
    files += sorted((ROOT/'sdk/js/native/src').rglob('*.rs'))
    files += sorted((ROOT/'core/crates').glob('*/Cargo.toml'))
    files += sorted((ROOT/'core/crates').glob('*/src/**/*.rs'))
    files += sorted((ROOT/'core/crates').glob('*/src/**/*.sql'))
    files += sorted((ROOT/'core/crates').glob('*/sql/**/*.sql'))
    if not (a.busy or a.liveness or a.fixture or a.witness): files += [HERE/'sandbox_reopen.qnt', HERE/'sandbox_state.qnt', HERE/'replay.qnt', HERE/'replay-control.qnt', HERE/'bin/quint']
    return {str(f.relative_to(ROOT)): hashlib.sha256(f.read_bytes()).hexdigest() for f in files}

sealed_inputs = source_identity()
# Product inputs are recoverable from the named Git head. Preserve consumed
# binding/model inputs too, including uncommitted test-only source versions.
with zipfile.ZipFile(out/'binding-inputs.zip', 'w', zipfile.ZIP_DEFLATED) as archive:
    for relative in sealed_inputs:
        if relative.startswith('quint/sandbox-reopen/'):
            archive.write(ROOT/relative, relative)


def run(command, name, env=None, timeout=None):
    started = time.monotonic()
    try:
        with (out / (name + '.log')).open('wb') as log:
            result = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=timeout)
            code = result.returncode
    except subprocess.TimeoutExpired:
        code = 124
    commands.append({'command': command, 'exit_code': code, 'seconds': time.monotonic()-started, 'log': name+'.log'})
    (out / 'commands.json').write_text(json.dumps(commands, indent=2)+'\n')
    return code

code = run(['python3', str(HERE/'instrument.py'), '--plant', a.plant], 'instrument')
if code: sys.exit(code)
compiled_inputs={str(f.relative_to(HERE/'generated/src')):hashlib.sha256(f.read_bytes()).hexdigest() for f in (HERE/'generated/src').rglob('*.rs')}
assert compiled_inputs==json.loads((HERE/'generated/manifest.json').read_text())['compiled_source'], 'generated source differs from its manifest'
for name in ['manifest.json', 'instrumentation.diff']:
    shutil.copyfile(HERE/'generated'/name, out/name)
code = run(['python3', str(HERE/'build-worker.py'), '--resources', a.resources], 'worker-build')
if code: sys.exit(code)
build_directory=Path((out/'worker-build.log').read_text().strip().splitlines()[-1])
for name in ['output.log','daemon.log']:
    if (build_directory/name).is_file(): shutil.copyfile(build_directory/name,out/('compiled-'+name))
state = Path(resources['state'])
env = {**os.environ, 'QUINT_VERBOSE': '1', 'CARGO_HOME': str(state/'cargo-home-replay'), 'CARGO_TARGET_DIR': str(state/'replay-target')}
if os.environ.get('NODE_EXTRA_CA_CERTS'): env['CARGO_HTTP_CAINFO'] = os.environ['NODE_EXTRA_CA_CERTS']
code = run(['cargo', '+1.99.1', 'build', '--manifest-path', 'quint/sandbox-reopen/replay/Cargo.toml', '--locked', '-j', '2'], 'driver-build', env)
if code: sys.exit(code)
env.update({'REOPEN_RESOURCES': str(Path(a.resources).resolve()), 'REOPEN_OUTPUT': str(out), 'REOPEN_SEED': a.seed, 'REOPEN_SAMPLES': str(a.samples), 'PATH': str(HERE/'bin')+os.pathsep+os.environ['PATH']})
if not (a.liveness or a.busy or a.fixture or a.witness): env['REOPEN_SCHEDULE_ADMISSION']='1'
if a.liveness: env['REOPEN_LIVENESS'] = '1'
if a.busy: env['REOPEN_BUSY'] = a.busy
if a.fixture: env['REOPEN_FIXTURE'] = a.fixture
if a.witness: env['REOPEN_WITNESS'] = a.witness
if a.witness=='fast-holder': env['REOPEN_FAST_REJECT']='1'
if a.plant=='no_revive_check': env['REOPEN_CONTROL']='no_revive_check'
# timeout kills the complete process group. container.py traps TERM and
# removes its exact container and disposable volume before KILL's ceiling.
code = run(['timeout', '--kill-after=5s', '95', str(state/'replay-target/debug/layerfs-reopen-replay')], 'replay', env)
assert source_identity() == sealed_inputs, 'inputs changed during build/replay; evidence ineligible'
assert compiled_inputs=={str(f.relative_to(HERE/'generated/src')):hashlib.sha256(f.read_bytes()).hexdigest() for f in (HERE/'generated/src').rglob('*.rs')}, 'compiled inputs changed during replay'
(out/'identity.json').write_text(json.dumps({'head': subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(), 'plant':a.plant,'seed':a.seed,'samples':a.samples,'resources':resources,
    'driver_sha256':hashlib.sha256((state/'replay-target/debug/layerfs-reopen-replay').read_bytes()).hexdigest(), 'inputs': sealed_inputs},indent=2)+'\n')
print(f'{a.plant} seed={a.seed} samples={a.samples} exit={code}: {out}')
sys.exit(code)
