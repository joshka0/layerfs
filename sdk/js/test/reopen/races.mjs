// The races phase (see main.mjs): two failure cases proposed beyond F1-F40,
// written from two sentences of the parent agent on 2026-11-14, before the
// fix commits were read:
//   F41. A listing that races the end of a Sandbox (another caller reclaims
//        or restarts it) reports `Failed`, not `NoSuchSandbox`, for a Sandbox
//        that is gone.
//   F42. A restart whose daemon handshake or record save fails leaves the
//        daemon it spawned running, so it holds the Sandbox folder that a
//        later reopen needs.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { existsSync, readdirSync } from "node:fs";
import { join } from "node:path";
import {
  BASE, HOME, OPERATOR, SDK, STORES, byPath, check, daemons, init, installLfs, killDaemons, lfsCli, open, overlayOf,
  sandboxRoot, sh, sha, until,
} from "./harness.mjs";

// One SDK or operator call in a new node process; prints {ok, code}.
function callInChild(project, call) {
  const script = `
    const { LayerFS } = await import(${JSON.stringify(SDK)});
    const { LayerFSOperator } = await import(${JSON.stringify(OPERATOR)});
    const l = new LayerFS(${JSON.stringify(project)}, { runtime: "process", storePath: ${JSON.stringify(join(STORES, project, "global/store.sqlite"))} });
    const op = new LayerFSOperator(l);
    try { await (${call})(l, op); console.log(JSON.stringify({ ok: true })); }
    catch (e) { console.log(JSON.stringify({ ok: false, code: e.code ?? null, message: String(e.message).split("\\n")[0] })); }
    process.exit(0);`;
  return new Promise((resolve) => {
    const child = spawn("node", ["--input-type=module", "-e", script]);
    let out = "";
    child.stdout.on("data", (d) => { out += d; });
    child.stderr.on("data", (d) => { out += d; });
    const timer = setTimeout(() => child.kill("SIGKILL"), 60000);
    child.on("close", () => {
      clearTimeout(timer);
      try { resolve(JSON.parse(out.trim().split("\n").pop())); } catch { resolve({ ok: false, code: "unparsed", message: out.trim() }); }
    });
  });
}
// A listing that meets a gone Sandbox answers or refuses NoSuchSandbox. A
// Workspace call may also refuse NotMounted while its Workspace is not yet
// mounted again. `Failed` is never right for either.
const ALLOWED = { list: ["NoSuchSandbox"], status: ["NoSuchSandbox", "NotMounted"], reopen: [] };
const notGoneAsFailed = (results) => {
  const bad = results.filter((r) => !r.ok && !ALLOWED[r.call].includes(r.code));
  assert.deepEqual(bad, [], `racing calls refused with a wrong code: ${JSON.stringify(bad)}`);
};
const labelled = (call, promise) => promise.then((r) => ({ call, ...r }));

export const racesPhases = {
  async races() {
    installLfs();

    // F41a: a dead Sandbox with no saved Workspace, met by six listings at
    // once: one reclaims it; the others see it gone.
    for (const project of ["rrace1", "rrace2", "rrace3"]) {
      const l = open(project);
      await init(l, BASE);
      await l.branch.fork("w");
      const ws = await l.workspace.mount("w");
      await l.workspace.unmount(ws, true);
      killDaemons();
      await until("the daemons to die", () => daemons().length === 0);
      const results = await Promise.all(Array.from({ length: 6 }, () => labelled("list", callInChild(project, "(l, op) => op.sandbox.list()"))));
      await check("F41 F28", `${project}: six racing listings of a dead idle Sandbox: success or NoSuchSandbox, never Failed`, () => {
        notGoneAsFailed(results);
        assert.ok(!existsSync(sandboxRoot(ws.path)), "the dead idle Sandbox was not reclaimed");
      });
    }

    // F41b: listings and Workspace calls race a reopen of a dead Sandbox with
    // a saved Workspace. The race is timing-dependent: three rounds per run.
    for (const project of ["rrace4", "rrace4b", "rrace4c"]) {
      const l = open(project);
      await init(l, BASE);
      await l.branch.fork("w");
      const ws = await l.workspace.mount("w");
      await sh(l, ws, "echo saved > s.txt");
      killDaemons();
      await until("the daemons to die", () => daemons().length === 0);
      const wsArg = JSON.stringify({ sandbox: ws.sandbox, path: ws.path });
      const results = await Promise.all([
        labelled("reopen", callInChild(project, "(l, op) => op.sandbox.reopen()")),
        ...Array.from({ length: 4 }, () => labelled("list", callInChild(project, "(l, op) => op.sandbox.list()"))),
        ...Array.from({ length: 2 }, () => labelled("status", callInChild(project, `(l) => l.workspace.status(${wsArg})`))),
      ]);
      await check("F41 F35", `${project}: listings and status racing a reopen: never Failed`, () => {
        notGoneAsFailed(results);
      });
      await check("F41 F35", `${project}: after the race the saved Workspace is mounted and intact`, async () => {
        const answer = lfsCli("reopen", project);
        assert.equal(byPath(answer.json, ws.path)?.mounted, true, JSON.stringify(answer.json));
        assert.equal(spawnSync("cat", [join(ws.path, "s.txt")], { encoding: "utf8" }).stdout, "saved\n");
      });
    }

    // F42: the restart's record save fails (the Sandbox records folder is
    // immutable), so the reopen must fail and stop the daemon it spawned.
    {
      const project = "rrace5";
      const l = open(project);
      await init(l, BASE);
      await l.branch.fork("w");
      const ws = await l.workspace.mount("w");
      await sh(l, ws, "echo kept > k.txt");
      const folder = sandboxRoot(ws.path).split("/").pop();
      killDaemons();
      await until("the daemons to die", () => daemons().length === 0);
      const records = join(HOME, project, "sandboxes", ws.sandbox);
      const before = sha(overlayOf(ws.path));
      // Only the record: its lock file and folder stay writable, so the
      // restart gets as far as spawning the daemon.
      const record = join(records, "sandbox.record");
      const lock = spawnSync("chattr", ["+i", record], { encoding: "utf8" });
      if (lock.status !== 0) {
        console.log(`SKIP F42 chattr +i refused (${lock.stderr.trim()}): the runner must give CAP_LINUX_IMMUTABLE on a filesystem that has the flag`);
        return;
      }
      let answer;
      try {
        answer = lfsCli("reopen", project);
      } finally {
        spawnSync("chattr", ["-i", record]);
      }
      await check("F42", "rrace5: a restart whose record save fails reports the Sandbox failed", () => {
        assert.equal(answer.status, 1, `exit ${answer.status}: ${answer.stdout}${answer.stderr}`);
        const box = answer.json?.sandboxes?.find((b) => b.name === folder);
        assert.equal(box?.state, "failed", `${answer.stdout}${answer.stderr}`);
      });
      await check("F42", "rrace5: the daemon spawned by the failed restart is stopped", async () => {
        await until("the spawned daemon to stop", () => !daemons().some((d) => d.folders.includes(folder)), 5000)
          .catch(() => assert.fail(`daemons still on ${folder}: ${JSON.stringify(daemons())}`));
      });
      await check("F42 F26", "rrace5: the failed restart leaves the overlay; a later reopen mounts it", () => {
        assert.equal(sha(overlayOf(ws.path)), before, "overlay changed");
        const again = lfsCli("reopen", project);
        assert.equal(again.status, 0, `${again.stdout}${again.stderr}`);
        assert.equal(spawnSync("cat", [join(ws.path, "k.txt")], { encoding: "utf8" }).stdout, "kept\n");
        assert.ok(readdirSync(records).length > 0);
      });
    }
  },
};
