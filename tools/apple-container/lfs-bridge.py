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
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PANE = re.compile(r"^w[A-Za-z0-9]+:p[0-9]+$")
WORKSPACE = re.compile(r"^w[A-Za-z0-9]+$")
WORD = re.compile(r"^[A-Za-z0-9][A-Za-z0-9 _.:+@/-]{0,200}$")
REPOS = os.path.expanduser("~/repos") + "/"
TIMEOUT = 120


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
