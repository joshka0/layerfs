// Process runtime E2E: a Sandbox is a layerfs-daemon child process, no Docker.
// Run as root on Linux with /dev/fuse and CAP_SYS_ADMIN, with this env set:
//   LAYERFS_RUNTIME=process LAYERFS_HOME=... LAYERFS_DAEMON=... \
//   LAYERFS_STORE_PATH=... LAYERFS_NATIVE=...
//   node sdk/js/test/process_runtime.mjs
// LAYERFS_DOCKER, LAYERFS_IMAGE and LAYERFS_VOLUME stay unset on purpose.
import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

for (const name of ["LAYERFS_RUNTIME", "LAYERFS_HOME", "LAYERFS_DAEMON", "LAYERFS_STORE_PATH"]) {
  assert.ok(process.env[name], `${name} must be set`);
}
for (const name of ["DOCKER", "IMAGE", "VOLUME"]) {
  delete process.env[`LAYERFS_${name}`];
  delete process.env[`DSH_LAYERFS_${name}`];
}
const ok = (step) => console.log(`OK ${step}`);
// README line 3 and 150 do not give fs.read's result shape: accept a string or
// an object with the text in `content` or `text`.
const text = (r) => (typeof r === "string" ? r : (r.content ?? r.text));
// README line 17 and 150: mount returns { sandbox, path }; `path` is the
// Workspace directory. With the process runtime it is a host path.
const mounted = (dir) => readFileSync("/proc/self/mountinfo", "utf8").split("\n").some((l) => l.split(" ")[4] === dir);

const work = mkdtempSync(join(tmpdir(), "layerfs-process-"));
const source = join(work, "src-tree");
mkdirSync(join(source, "src"), { recursive: true });
writeFileSync(join(source, "README.md"), "hello\n");
writeFileSync(join(source, "src", "a.txt"), "one\n");
// Make only the volume folder; init's install makes the private `global/` (0700).
mkdirSync(dirname(dirname(process.env.LAYERFS_STORE_PATH)), { recursive: true });
const { LayerFS } = await import("@layerfs/sdk");
const layerfs = new LayerFS(`proc-${process.pid}-${Date.now().toString(36)}`, { runtime: "process" });
const { workspace: w } = layerfs;
const mountedWs = [];
const mount = async () => { const ws = await w.mount("work"); mountedWs.push(ws); return ws; };

try {
  await layerfs.project.init(source);
  ok("project.init");

  assert.equal(text(await layerfs.fs.read("main", "/README.md")), "hello\n", "fs.read main README.md");
  ok("fs.read main");
  await layerfs.branch.fork("work"); // main changes only through publish (README line 155)
  ok("branch.fork work");

  const ws = await mount();
  assert.ok(ws.path && existsSync(ws.path), `mount directory ${ws.path} readable by the script`);
  assert.ok(readdirSync(ws.path).includes("README.md"), "mount directory lists README.md on the host");
  let r = await w.exec(ws, "id -u; id -g; cat README.md");
  assert.equal(r.exitCode, 0, `exec id: exit ${r.exitCode} stderr ${r.stderr}`);
  assert.equal(r.stdout, "501\n20\nhello\n", "exec runs as uid 501 gid 20 in the Workspace");
  ok("mount and exec as uid 501");

  r = await w.exec(ws, "echo two >> src/a.txt; mkdir -p new; echo b > new/b.txt");
  assert.equal(r.exitCode, 0, `exec write: ${r.stderr}`);
  r = await w.exec(ws, "exit 7");
  assert.equal(r.exitCode, 7, "exit code passes through");
  r = await w.exec(ws, "echo oops >&2");
  assert.equal(r.exitCode, 0);
  assert.equal(r.stderr, "oops\n", "stderr is returned");
  ok("exec write, exit code, stderr");

  await w.commit(ws, "process runtime e2e");
  ok("workspace.commit");

  assert.equal(text(await layerfs.fs.read("work", "/src/a.txt")), "one\ntwo\n", "committed a.txt on work");
  assert.equal(text(await layerfs.fs.read("work", "/new/b.txt")), "b\n", "committed new/b.txt on work");
  const page = await layerfs.branch.list_commits("work", 20, 0);
  assert.ok(JSON.stringify(page.items).includes("process runtime e2e"), "list_commits has the commit message");
  ok("committed state visible to Store calls");

  await w.unmount(ws);
  mountedWs.length = 0;
  assert.ok(!mounted(ws.path), `${ws.path} is no longer a mount`);
  assert.ok(!existsSync(ws.path) || readdirSync(ws.path).length === 0, `${ws.path} is gone or empty`);
  ok("unmount removes the mount");

  const second = await mount();
  r = await w.exec(second, "cat src/a.txt new/b.txt");
  assert.equal(r.stdout, "one\ntwo\nb\n", "a new Workspace sees the committed state");
  await w.unmount(second);
  mountedWs.length = 0;
  ok("second workspace sees commit");

  const a = await mount();
  const b = await mount();
  assert.notEqual(a.path, b.path, "two Workspaces have distinct mount directories");
  r = await w.exec(a, "echo only-in-a > iso.txt");
  assert.equal(r.exitCode, 0, `exec iso write: ${r.stderr}`);
  r = await w.exec(b, "test ! -e iso.txt");
  assert.equal(r.exitCode, 0, "an uncommitted write in one Workspace is not visible in the other");
  await w.unmount(a, true);
  await w.unmount(b);
  mountedWs.length = 0;
  ok("two workspaces are isolated before commit");
} finally {
  for (const ws of mountedWs) await w.unmount(ws, true).catch(() => {});
  await layerfs.close();
  rmSync(work, { recursive: true, force: true });
}
ok("close");
console.log("PROCESS_RUNTIME_E2E ok");
