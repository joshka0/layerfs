// The races phase (see main.mjs): two failure cases proposed beyond F1-F40,
// written from two sentences of the parent agent on 2026-11-14, before the
// fix commits were read:
//   F41. A listing that races the end of a Sandbox (another caller reclaims
//        it) reports `Failed`, not `NoSuchSandbox`, for a Sandbox that is
//        gone. A listing that meets a Sandbox mid-restart fails, skips it, or
//        waits without a bound, instead of listing it as present (contract
//        decision of rowan-joshka-main, 2026-11-14; `NoSuchSandbox` is wrong
//        there, because the Sandbox exists).
//   F42. A restart whose daemon handshake or record save fails leaves the
//        daemon it spawned running, so it holds the Sandbox folder that a
//        later reopen needs.
//   F43. A restart whose daemon listener never becomes ready leaves the
//        daemon it spawned running (added 2026-11-14 from a model review).
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { existsSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  BASE, HOME, OPERATOR, SDK, STORES, byPath, check, daemons, init, installLfs, killDaemons, lfsCli, open, overlayOf,
  sandboxRoot, sh, sha, until,
} from "./harness.mjs";

// One SDK or operator call in a new node process; prints {ok, code}.
function callInChild(project, call, env = {}) {
  const script = `
    const { LayerFS } = await import(${JSON.stringify(SDK)});
    const { LayerFSOperator } = await import(${JSON.stringify(OPERATOR)});
    const l = new LayerFS(${JSON.stringify(project)}, { runtime: "process", storePath: ${JSON.stringify(join(STORES, project, "global/store.sqlite"))} });
    const op = new LayerFSOperator(l);
    try { const value = await (${call})(l, op); console.log(JSON.stringify({ ok: true, value: value ?? null })); }
    catch (e) { console.log(JSON.stringify({ ok: false, code: e.code ?? null, message: String(e.message).split("\\n")[0] })); }
    process.exit(0);`;
  return new Promise((resolve) => {
    const child = spawn("node", ["--input-type=module", "-e", script], { env: { ...process.env, ...env } });
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
// Refusal codes a racing call may give. A listing that meets a gone
// Sandbox may refuse NoSuchSandbox; one that meets a Sandbox mid-restart may
// not refuse at all. A Workspace call keeps its existing contract: before
// the restart it meets a dead daemon (NoSuchSandbox, README), then a
// Workspace not mounted yet (NotMounted). `Failed` is never right.
const GONE = { list: ["NoSuchSandbox"] };
const RESTARTING = { list: [], status: ["NoSuchSandbox", "NotMounted"], reopen: [] };
const wrongCodes = (allowed, results) => results.filter((r) => !r.ok && !allowed[r.call].includes(r.code));
const refusedOnlyWith = (allowed, results) => {
  const bad = wrongCodes(allowed, results);
  assert.deepEqual(bad, [], `racing calls refused with a wrong code: ${JSON.stringify(bad)}`);
};
// A listing item names its Sandbox by number or by folder name.
const lists = (value, id, folder) => (Array.isArray(value) ? value : []).some((item) =>
  item && (item.id === id || item.sandbox === id || item.name === folder));
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
        refusedOnlyWith(GONE, results);
        assert.ok(!existsSync(sandboxRoot(ws.path)), "the dead idle Sandbox was not reclaimed");
      });
    }

    // F41b: listings and Workspace calls meet a Sandbox mid-restart. The
    // restart's daemon delays its listen line by 2 s (its output passes a
    // pipe that opens after 2 s), so the restart window is open when the
    // other calls start 0.5 s later: no timing luck needed.
    const slow = "/usr/local/bin/slow-layerfs-daemon";
    writeFileSync(slow, '#!/bin/sh\n/opt/layerfs/bundle/layerfs-daemon "$@" 2>&1 | { sleep 2; cat; }\n', { mode: 0o755 });
    for (const project of ["rrace4", "rrace4b", "rrace4c"]) {
      const l = open(project);
      await init(l, BASE);
      await l.branch.fork("w");
      const ws = await l.workspace.mount("w");
      await sh(l, ws, "echo saved > s.txt");
      killDaemons();
      await until("the daemons to die", () => daemons().length === 0);
      const wsArg = JSON.stringify({ sandbox: ws.sandbox, path: ws.path });
      const restarting = labelled("reopen", callInChild(project, "(l, op) => op.sandbox.reopen()", { LAYERFS_DAEMON: slow }));
      await new Promise((r) => setTimeout(r, 500));
      const results = await Promise.all([
        restarting,
        ...Array.from({ length: 4 }, () => labelled("list", callInChild(project, "(l, op) => op.sandbox.list()"))),
        ...Array.from({ length: 2 }, () => labelled("status", callInChild(project, `(l) => l.workspace.status(${wsArg})`))),
      ]);
      await check("F41 F35", `${project}: listings racing a restart succeed and list the Sandbox; status refuses only NoSuchSandbox or NotMounted`, () => {
        const folder = sandboxRoot(ws.path).split("/").pop();
        const bad = wrongCodes(RESTARTING, results);
        const missing = results.filter((r) => r.call === "list" && r.ok && !lists(r.value, ws.sandbox, folder));
        const found = { wrongCodes: bad, listingsWithoutTheSandbox: missing };
        assert.deepEqual(found, { wrongCodes: [], listingsWithoutTheSandbox: [] }, JSON.stringify(found));
      });
      await check("F41", `${project}: precondition: the slow restart itself succeeded`, () => {
        assert.equal(results[0].ok, true, JSON.stringify(results[0]));
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

    // F43: the real daemon runs, but its output goes nowhere, so the restart
    // never reads a listen line: its listener never becomes ready.
    {
      const project = "rrace6";
      const l = open(project);
      await init(l, BASE);
      await l.branch.fork("w");
      const ws = await l.workspace.mount("w");
      await sh(l, ws, "echo quiet > q.txt");
      const folder = sandboxRoot(ws.path).split("/").pop();
      killDaemons();
      await until("the daemons to die", () => daemons().length === 0);
      const quiet = "/usr/local/bin/quiet-layerfs-daemon";
      writeFileSync(quiet, '#!/bin/sh\nexec /opt/layerfs/bundle/layerfs-daemon "$@" >/dev/null 2>&1\n', { mode: 0o755 });
      const r = spawnSync("node", ["/opt/layerfs/tools/lfs.mjs", "reopen", project], {
        encoding: "utf8", timeout: 90000,
        env: { ...process.env, LAYERFS_DAEMON: quiet, LAYERFS_CONTAINER_PREFIX: "lfs-" },
      });
      let answer = null;
      try { answer = JSON.parse(r.stdout.trim().split("\n").pop()); } catch {}
      await check("F43", "precondition: with no listen line the restart reports the Sandbox failed", () => {
        assert.notEqual(r.status, null, `lfs reopen hung: ${r.stderr}`);
        const box = answer?.sandboxes?.find((b) => b.name === folder);
        assert.equal(box?.state, "failed", `${r.stdout}${r.stderr}`);
      });
      await check("F43", "rrace6: the daemon spawned by a restart whose listener never became ready is stopped", async () => {
        await until("the spawned daemon to stop", () => !daemons().some((d) => d.folders.includes(folder)), 5000)
          .catch(() => assert.fail(`daemons still on ${folder}: ${JSON.stringify(daemons())}`));
      });
      await check("F43 F26", "rrace6: after the failed restart a normal reopen mounts the saved Workspace", () => {
        const again = lfsCli("reopen", project);
        assert.equal(again.status, 0, `${again.stdout}${again.stderr}`);
        assert.equal(spawnSync("cat", [join(ws.path, "q.txt")], { encoding: "utf8" }).stdout, "quiet\n");
      });
    }
  },
};
