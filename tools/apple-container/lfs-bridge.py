#!/usr/bin/env python3
"""lfs-bridge: runs a fixed set of Mac commands for agents in `lfs-vm`.

It listens on the VM gateway address only (never the LAN), checks a token
that only the VM's `agent` user can read, and runs no shell: a request is an
argv that must match one of the shapes in `allowed`. Anything else is refused.

usage: lfs-bridge.py <listen-ip> <port> <token-file> <mem-folder>

Request: POST / with {"argv": [...]} and header `Authorization: Bearer <token>`.
Answer: {"code": n, "stdout": "...", "stderr": "..."}, or 403 with a reason.
"""
import hmac
import json
import os
import re
import subprocess
import sys
import tempfile
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PANE = re.compile(r"^w[A-Za-z0-9]+:p[0-9]+$")
WORKSPACE = re.compile(r"^w[A-Za-z0-9]+$")
WORD = re.compile(r"^[A-Za-z0-9][A-Za-z0-9 _.:+@/-]{0,200}$")
REPOS = os.path.expanduser("~/repos") + "/"
TIMEOUT = 120
WORKSPACE_PATH = re.compile(r"^/var/lib/layerfs/sandboxes/[A-Za-z0-9._-]+/workspaces/[0-9]+$")
XCODE_TIMEOUT = 1800
LOG_LINES = 200


# Alchemy plans per LayerFS project: its Infisical folder and the child token
# variable that becomes CLOUDFLARE_API_TOKEN. Test configuration for the
# stage 6 proof; remove when the bridge reads a config file.
ALCHEMY = {"infra-kit": ("/repos/personal/infra-kit", "CLOUDFLARE_R2_PLAN_API_TOKEN")}
ALCHEMY_TIMEOUT = 900


def export(path, target, exclude=()):
    """Copies a Workspace out of lfs-vm into `target`; True when it worked."""
    os.mkdir(target)
    tar = ["container", "exec", "lfs-vm", "tar", "-C", path]
    tar += [f"--exclude=./{name}" for name in exclude] + ["-cf", "-", "."]
    out = subprocess.Popen(tar, stdout=subprocess.PIPE)
    unpack = subprocess.run(["tar", "-xf", "-", "-C", target], stdin=out.stdout)
    out.stdout.close()
    return out.wait() == 0 and unpack.returncode == 0


def alchemy_plan(path, example):
    """`alchemy plan` of one example stack of a Workspace, on the Mac: macOS
    dependencies through sfw, the child token through gondr. The token never
    enters the VM. Returns (code, log tail)."""
    project = re.match(r"^/var/lib/layerfs/sandboxes/layerfs-(.+)-[0-9]+/", path)
    if not project or project.group(1) not in ALCHEMY:
        return 1, "no Alchemy configuration for this Workspace's project\n"
    folder, token = ALCHEMY[project.group(1)]
    with tempfile.TemporaryDirectory(prefix="lfs-alchemy-") as work:
        source = os.path.join(work, "source")
        if not export(path, source, exclude=("node_modules",)):
            return 1, f"copy of {path} out of lfs-vm failed\n"
        stack = os.path.join(source, "examples", example)
        if not os.path.isfile(os.path.join(stack, "alchemy.run.ts")):
            return 1, f"no examples/{example}/alchemy.run.ts\n"
        steps = [
            (["sfw", "bun", "install", "--frozen-lockfile"], source),
            (["gondr", "run", "--path", folder, "--", "sh", "-c",
              f'CLOUDFLARE_API_TOKEN="${token}" exec ../../node_modules/.bin/alchemy plan'], stack),
        ]
        log = ""
        for argv, cwd in steps:
            run = subprocess.run(argv, cwd=cwd, capture_output=True, text=True, timeout=ALCHEMY_TIMEOUT)
            log += (run.stdout + run.stderr).replace(source, path)
            if run.returncode:
                break
        return run.returncode, "\n".join(log.splitlines()[-LOG_LINES:]) + "\n"


def xcode(path, scheme, action):
    """Copies a Workspace out of lfs-vm, runs xcodebuild on it for macOS,
    and removes the copy. Returns (code, log tail)."""
    with tempfile.TemporaryDirectory(prefix="lfs-xcode-") as work:
        source = os.path.join(work, "source")
        os.mkdir(source)
        export = subprocess.Popen(
            ["container", "exec", "lfs-vm", "tar", "-C", path, "-cf", "-", "."],
            stdout=subprocess.PIPE,
        )
        unpack = subprocess.run(["tar", "-xf", "-", "-C", source], stdin=export.stdout)
        export.stdout.close()
        if export.wait() or unpack.returncode:
            return 1, f"copy of {path} out of lfs-vm failed"
        run = subprocess.run(
            ["xcodebuild", "-scheme", scheme, "-destination", "platform=macOS",
             "-derivedDataPath", os.path.join(work, "derived"), action],
            cwd=source, capture_output=True, text=True, timeout=XCODE_TIMEOUT,
        )
        # error locations name the Workspace path in the VM, not this copy
        log = (run.stdout + run.stderr).replace(source, path)
        tail = log.splitlines()[-LOG_LINES:]
        return run.returncode, "\n".join(tail) + "\n"


def pairs(rest, flags):
    """`--flag value` pairs, each flag known and each value a plain word."""
    if len(rest) % 2:
        return False
    return all(rest[i] in flags and WORD.match(rest[i + 1]) for i in range(0, len(rest), 2))


def mem_args(rest):
    """Read-only mem calls; --scope only for a folder under ~/repos."""
    words = []
    i = 0
    while i < len(rest):
        if rest[i] == "--scope":
            if i + 1 >= len(rest):
                return False
            scope = os.path.realpath(rest[i + 1])
            if not scope.startswith(REPOS):
                return False
            i += 2
        elif rest[i] in ("--like", "--only", "--since", "--page"):
            if i + 1 >= len(rest) or not WORD.match(rest[i + 1]):
                return False
            i += 2
        elif rest[i] in ("--tree", "--all"):
            i += 1
        elif WORD.match(rest[i]):
            words.append(rest[i])
            i += 1
        else:
            return False
    return True


def allowed(argv):
    """The reason argv is refused, or None when it may run."""
    match argv:
        case ["herdr", "pane", "list", "--workspace", ws] if WORKSPACE.match(ws):
            return None
        case ["herdr", "pane", "read", pane, *rest] if PANE.match(pane) and pairs(
            rest, {"--source", "--lines"}
        ):
            return None
        case ["herdr", "pane", "run", pane, text] if PANE.match(pane):
            # pane messages from agents start with their label (fleet rule)
            if text.startswith("[agent ") and len(text) <= 4000 and "\x1b" not in text:
                return None
            return "a pane message must start with '[agent <label>]' and hold no escape codes"
        case ["xcode", path, scheme, ("build" | "test")] if WORKSPACE_PATH.match(path) and WORD.match(
            scheme
        ):
            return None
        case ["alchemy-plan", path, example] if WORKSPACE_PATH.match(path) and re.match(
            r"^[a-z0-9-]+$", example
        ):
            return None
        case ["mem", ("view" | "topics" | "topic" | "find" | "zoom" | "date"), *rest] if mem_args(rest):
            return None
    return "not an allowed command shape"


class Bridge(BaseHTTPRequestHandler):
    token = b""
    mem_folder = ""

    def answer(self, status, body):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        given = self.headers.get("Authorization", "").removeprefix("Bearer ").encode()
        if not hmac.compare_digest(given, self.token):
            return self.answer(401, {"error": "bad token"})
        try:
            length = int(self.headers.get("Content-Length", "0"))
            argv = json.loads(self.rfile.read(min(length, 65536)))["argv"]
            if not (isinstance(argv, list) and all(isinstance(a, str) for a in argv)):
                raise ValueError
        except (ValueError, KeyError, TypeError):
            return self.answer(400, {"error": "body must be {\"argv\": [strings]}"})
        reason = allowed(argv)
        self.log_message("%s %s", "ALLOW" if reason is None else "DENY", json.dumps(argv[:3]))
        if reason is not None:
            return self.answer(403, {"error": reason})
        if argv[0] == "alchemy-plan":
            try:
                code, log = alchemy_plan(*argv[1:])
            except subprocess.TimeoutExpired:
                return self.answer(504, {"error": f"alchemy plan timed out after {ALCHEMY_TIMEOUT}s"})
            return self.answer(200, {"code": code, "stdout": log, "stderr": ""})
        if argv[0] == "xcode":
            try:
                code, log = xcode(*argv[1:])
            except subprocess.TimeoutExpired:
                return self.answer(504, {"error": f"xcodebuild timed out after {XCODE_TIMEOUT}s"})
            return self.answer(200, {"code": code, "stdout": log, "stderr": ""})
        cwd = self.mem_folder if argv[0] == "mem" else None
        try:
            run = subprocess.run(argv, cwd=cwd, capture_output=True, text=True, timeout=TIMEOUT)
        except subprocess.TimeoutExpired:
            return self.answer(504, {"error": f"timed out after {TIMEOUT}s"})
        self.answer(200, {"code": run.returncode, "stdout": run.stdout, "stderr": run.stderr})


def main():
    if len(sys.argv) != 5:
        sys.exit(__doc__)
    listen, port, token_file, mem_folder = sys.argv[1:]
    if os.stat(token_file).st_mode & 0o077:
        sys.exit("token file must be private (0600)")
    with open(token_file, "rb") as f:
        Bridge.token = f.read().strip()
    Bridge.mem_folder = mem_folder
    server = ThreadingHTTPServer((listen, int(port)), Bridge)
    print(f"LFS_BRIDGE_LISTEN {listen}:{port}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
