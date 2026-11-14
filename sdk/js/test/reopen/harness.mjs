// Shared harness of the reopen after restart E2E (see main.mjs): the hard
// rail, the check collector, container and SDK helpers, and the fixtures.
import assert from "node:assert/strict";
import { execFileSync, spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  existsSync, mkdirSync, readFileSync, readdirSync, readlinkSync, statSync, unlinkSync, utimesSync, writeFileSync,
  rmSync,
} from "node:fs";
import { DatabaseSync } from "node:sqlite";
import { basename, dirname, join } from "node:path";

export const DATA = "/var/lib/layerfs-data";
export const HOME = join(DATA, "home");
export const STORES = join(DATA, "stores");
export const SANDBOXES = "/var/lib/layerfs/sandboxes";
export const STATE = join(DATA, "reopen-e2e", "state.json");
export const SDK = process.env.LAYERFS_SDK ?? "/opt/layerfs/sdk/src/index.js";
export const OPERATOR = join(dirname(dirname(SDK)), "operator/index.js");

// Hard rail: never run on a live workbench. The runner sets the marker; the
// agent home exists only on lfs-orb and its siblings.
if (process.env.REOPEN_E2E_CONTAINER !== "1" || existsSync(join(DATA, "agent-home"))) {
  console.error("the reopen E2E runs only in a throwaway container made by tools/orbstack/reopen-e2e.sh");
  process.exit(2);
}
Object.assign(process.env, {
  LAYERFS_RUNTIME: "process", LAYERFS_HOME: HOME, LAYERFS_STORE_ROOT: STORES,
  LAYERFS_DAEMON: "/opt/layerfs/bundle/layerfs-daemon", LAYERFS_NATIVE: "/opt/layerfs/bundle/layerfs.node",
  LAYERFS_CONTAINER_PREFIX: "lfs-",
});
for (const name of ["DOCKER", "IMAGE", "VOLUME", "STORE_PATH"]) delete process.env[`LAYERFS_${name}`];

// --- checks -----------------------------------------------------------------
let failed = 0;
export const failures = () => failed;
export async function check(ids, what, body) {
  try {
    await body();
    console.log(`PASS ${ids} ${what}`);
  } catch (error) {
    failed += 1;
    const why = String(error?.message ?? error).split("\n").slice(0, 6).join(" | ");
    console.log(`FAIL ${ids} ${what}: ${why}`);
  }
}

// --- environment helpers ----------------------------------------------------
export const sha = (file) => createHash("sha256").update(readFileSync(file)).digest("hex");
export const sidecars = (file) => ["-journal", "-wal", "-shm"].filter((s) => existsSync(file + s));
export const loadState = () => (existsSync(STATE) ? JSON.parse(readFileSync(STATE, "utf8")) : {});
export function saveState(patch) {
  mkdirSync(dirname(STATE), { recursive: true });
  writeFileSync(STATE, JSON.stringify({ ...loadState(), ...patch }, null, 2));
}
export const mountTargets = () => readFileSync("/proc/self/mountinfo", "utf8").split("\n").map((l) => l.split(" ")[4]).filter(Boolean);
export const isMount = (dir) => mountTargets().includes(dir);
// The saved path is <sandbox root>/workspaces/<namespace>; its overlay is
// <sandbox root>/overlay/<namespace>.sqlite (REOPEN.md Problem, F17).
export const sandboxRoot = (wsPath) => dirname(dirname(wsPath));
export const overlayOf = (wsPath) => join(sandboxRoot(wsPath), "overlay", `${basename(wsPath)}.sqlite`);
export const records = (project) => {
  const dir = join(HOME, project, "sandboxes");
  if (!existsSync(dir)) return [];
  return readdirSync(dir, { recursive: true }).filter((f) => /workspace-[0-9a-f]+\.record$/.test(f)).map((f) => join(dir, f)).sort();
};
// Every running layerfs-daemon and the Sandbox folder of its --config. The
// daemon is not dumpable, so its exe and fds are unreadable; argv is not.
export function daemons() {
  const out = [];
  for (const pid of readdirSync("/proc").filter((p) => /^\d+$/.test(p))) {
    let argv;
    try { argv = readFileSync(`/proc/${pid}/cmdline`, "utf8").split("\0"); } catch { continue; }
    if (!argv[0]?.endsWith("/layerfs-daemon")) continue;
    if (/^Z/.test(readFileSync(`/proc/${pid}/stat`, "utf8").split(") ").pop())) continue;
    const config = argv[argv.indexOf("--config") + 1] ?? "";
    const m = config.match(/^\/var\/lib\/layerfs\/sandboxes\/([^/]+)\//);
    out.push({ pid: Number(pid), folders: m ? [m[1]] : [] });
  }
  return out;
}
export function assertOneDaemonPerFolder() {
  const seen = new Map();
  for (const d of daemons()) for (const f of d.folders) {
    assert.ok(!seen.has(f), `two daemons (${seen.get(f)}, ${d.pid}) hold files of Sandbox folder ${f}`);
    seen.set(f, d.pid);
  }
  return seen;
}
export const killDaemons = () => { for (const d of daemons()) process.kill(d.pid, "SIGKILL"); };
export async function until(what, ok, ms = 10000) {
  const end = Date.now() + ms;
  while (Date.now() < end) { if (ok()) return; await new Promise((r) => setTimeout(r, 50)); }
  throw new Error(`timed out after ${ms} ms waiting for ${what}`);
}

// The lfs command as lfs-vm.sh installs it, on this container's bundle.
export function installLfs() {
  writeFileSync("/usr/local/bin/lfs", `#!/bin/sh
export LAYERFS_RUNTIME=process LAYERFS_HOME=${HOME} \\
  LAYERFS_DAEMON=/opt/layerfs/bundle/layerfs-daemon LAYERFS_NATIVE=/opt/layerfs/bundle/layerfs.node \\
  LAYERFS_STORE_ROOT=${STORES} LAYERFS_CONTAINER_PREFIX=lfs-
exec node /opt/layerfs/tools/lfs.mjs "$@"
`, { mode: 0o755 });
}
export function lfsCli(...args) {
  const r = spawnSync("/usr/local/bin/lfs", args, { encoding: "utf8", timeout: 60000, env: { PATH: process.env.PATH } });
  let json = null;
  try { json = JSON.parse(r.stdout.trim().split("\n").pop()); } catch {}
  return { status: r.status, stdout: r.stdout, stderr: r.stderr, json };
}

// --- SDK helpers ------------------------------------------------------------
export const { LayerFS } = await import(SDK);
export const { LayerFSOperator } = await import(OPERATOR);
export function open(project) {
  mkdirSync(join(STORES, project), { recursive: true });
  return new LayerFS(project, { runtime: "process", storePath: join(STORES, project, "global/store.sqlite") });
}
export const text = (r) => (typeof r === "string" ? r : (r.content ?? r.text));
export async function sh(l, ws, command) {
  const r = await l.workspace.exec(ws, command);
  assert.equal(r.exitCode, 0, `exec ${JSON.stringify(command)} exit ${r.exitCode}: ${r.stderr}`);
  return r.stdout;
}
export async function init(l, files) {
  const src = join("/tmp", `src-${Date.now()}-${Math.random().toString(36).slice(2)}`);
  for (const [path, body] of Object.entries(files)) {
    mkdirSync(dirname(join(src, path)), { recursive: true });
    writeFileSync(join(src, path), body);
  }
  await l.project.init(src);
}
// A tree's full listing seen through the mount: path, type, mode, size,
// link target and content hash. Read with the host's own tools, as root.
export function manifest(dir) {
  const out = execFileSync("bash", ["-c",
    "cd \"$1\" && find . -printf '%p %y %m %s %l\\n' | LC_ALL=C sort && find . -type f -print0 | LC_ALL=C sort -z | xargs -0 -r sha256sum",
    "_", dir], { encoding: "utf8" });
  return out;
}
// One workspace.exec from a new node process, as an agent's next tool call
// would run it: no connection state of this process carries over.
export function execInFreshProcess(project, ws, command) {
  const script = `
    const { LayerFS } = await import(${JSON.stringify(SDK)});
    const l = new LayerFS(${JSON.stringify(project)}, { runtime: "process", storePath: ${JSON.stringify(join(STORES, project, "global/store.sqlite"))} });
    const r = await l.workspace.exec(${JSON.stringify({ sandbox: ws.sandbox, path: ws.path })}, ${JSON.stringify(command)});
    process.stdout.write(r.stdout);
    process.exit(r.exitCode);`;
  return spawnSync("node", ["--input-type=module", "-e", script], { encoding: "utf8", timeout: 60000 });
}
// The answer of operator.sandbox.reopen() / lfs reopen (REOPEN.md Interfaces).
export const answerWorkspaces = (answer) => (answer?.sandboxes ?? []).flatMap((s) => s.workspaces.map((w) => ({ ...w, sandbox: s })));
export const byPath = (answer, path) => answerWorkspaces(answer).find((w) => w.path === path);
export function assertAnswerShape(answer) {
  assert.equal(answer?.kind, "ok", `answer kind: ${JSON.stringify(answer)}`);
  assert.ok(Array.isArray(answer.sandboxes), "answer.sandboxes is a list");
  for (const s of answer.sandboxes) {
    assert.ok(typeof s.id === "string" && typeof s.name === "string", `sandbox id and name: ${JSON.stringify(s)}`);
    assert.ok(["running", "restarted", "failed", "ended"].includes(s.state), `sandbox state ${s.state}`);
    assert.equal(s.state === "failed", typeof s.reason === "string" && s.reason.length > 0, `sandbox reason iff failed: ${JSON.stringify(s)}`);
    for (const w of s.workspaces) {
      assert.ok(typeof w.path === "string" && /^[0-9a-f]{64}$/.test(w.workspace), `workspace path and id: ${JSON.stringify(w)}`);
      assert.equal(typeof w.reopened, "boolean", `reopened is a boolean: ${JSON.stringify(w)}`);
      assert.equal(typeof w.mounted, "boolean", `mounted is a boolean: ${JSON.stringify(w)}`);
      assert.equal(w.reason === null, w.reopened, `reason is null exactly when reopened: ${JSON.stringify(w)}`);
      if (!w.reopened) assert.ok(typeof w.reason === "string" && w.reason.length > 0, "a refusal names its reason");
      if (w.reopened) assert.equal(w.mounted, true, `reopened but not mounted: ${JSON.stringify(w)}`);
      assert.equal(w.mounted, isMount(w.path), `mounted ${w.mounted} but the mount table says ${isMount(w.path)}: ${w.path}`);
      if (s.state === "failed") assert.equal(w.mounted, false, "no Workspace is mounted in a failed Sandbox");
      if (s.state === "ended") assert.equal(w.reason, "no saved overlay file", `an ended Sandbox lists only Workspaces without overlay: ${JSON.stringify(w)}`);
      if (w.reason === "already mounted") assert.equal(w.mounted, true, "`already mounted` but not mounted");
    }
  }
}
export async function reopen(l) {
  const answer = await new LayerFSOperator(l).sandbox.reopen();
  assertAnswerShape(answer);
  return answer;
}
export const wsOf = (saved) => ({ sandbox: saved.sandbox, path: saved.path });
// A saved { sandbox, path } names the saved Workspace only while that
// Sandbox was not reclaimed: a reclaimed number is used again.
export async function same(l, saved, branch) {
  const st = await l.workspace.status(wsOf(saved));
  assert.equal(st.branch, branch, `${saved.path} now holds branch ${st.branch}, not the saved Workspace`);
}

// --- fixtures ---------------------------------------------------------------
// 200000 bytes of a known pattern: more than one 32768-byte slot of fs_data.
export const BIG = Buffer.from("abcdefgh\n".repeat(22223)).subarray(0, 200000);
export const BIG_SHA = createHash("sha256").update(BIG).digest("hex");
export const BASE = {
  "README.md": "hello\n",
  "src/a.txt": "one\n",
  "del.txt": "gone\n",
  "keep.sh": "#!/bin/sh\necho keep\n",
  "trunc.bin": "x".repeat(70000),
};
// The richest saved Workspace: changed bytes, a deletion, new folders, a mode,
// a symlink, a truncation and a multi-slot file (F9).
export const MAIN_EDIT = [
  "printf 'one\\ntwo\\n' > src/a.txt",
  "rm del.txt",
  "mkdir -p new/deep && echo n > new/deep/n.txt",
  "chmod 755 keep.sh",
  "ln -s src/a.txt link",
  "truncate -s 5000 trunc.bin",
  "yes abcdefgh | head -c 200000 > big.bin",
].join(" && ");
export const MIXED = ["good", "gen2", "genmix", "orphan", "cleanup", "commitrec", "schema", "appid", "missing"];
