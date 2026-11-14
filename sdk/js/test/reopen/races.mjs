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
//   F44. A mount that meets a dead Sandbox whose revive fails does not
//        succeed elsewhere, or its answer has no `revived` entry naming that
//        Sandbox and each saved Workspace with its reason, or the old
//        Sandbox's records or overlays are lost (operator decision relayed by
//        rowan-joshka-main, 2026-11-14).
// F41 status (same decision): a status that meets a Sandbox mid-restart
// waits, bounded by 60 s, and answers the restarted daemon's real status;
// only after the bound may it refuse Busy. NoSuchSandbox and Failed are wrong.
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
// not refuse at all, and neither may a Workspace status inside the 60 s
// bound: it answers the restarted daemon's status. `Failed` is never right.
const GONE = { list: ["NoSuchSandbox"] };
const RESTARTING = { list: [], status: [], reopen: [] };
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
      await check("F41 F35", `${project}: listings racing a restart list the Sandbox; status waits and answers the real status`, () => {
        const folder = sandboxRoot(ws.path).split("/").pop();
        const bad = wrongCodes(RESTARTING, results);
        const missing = results.filter((r) => r.call === "list" && r.ok && !lists(r.value, ws.sandbox, folder));
        const notReal = results.filter((r) => r.call === "status" && r.ok && (r.value?.kind !== "ok" || r.value?.branch !== "w"));
        const found = { wrongCodes: bad, listingsWithoutTheSandbox: missing, statusNotFromTheRestartedDaemon: notReal };
        const none = { wrongCodes: [], listingsWithoutTheSandbox: [], statusNotFromTheRestartedDaemon: [] };
        assert.deepEqual(found, none, JSON.stringify(found));
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

    // F44: the dead Sandbox's config is gone, so its revive fails; a mount
    // from a new process must still succeed in another Sandbox and name the
    // failed revive in `revived`.
    {
      const project = "rrace7";
      const l = open(project);
      await init(l, BASE);
      await l.branch.fork("w");
      await l.branch.fork("w2");
      const ws = await l.workspace.mount("w");
      await sh(l, ws, "echo old > o.txt");
      const root = sandboxRoot(ws.path);
      const folder = root.split("/").pop();
      killDaemons();
      await until("the daemons to die", () => daemons().length === 0);
      spawnSync("rm", ["-rf", join(root, "config")]);
      const before = sha(overlayOf(ws.path));
      const recordsBefore = readdirSync(join(HOME, project, "sandboxes", ws.sandbox)).sort();
      const mounted = await callInChild(project, `(l) => l.workspace.mount("w2")`);
      await check("F44", "rrace7: a mount after a failed revive succeeds in another Sandbox", () => {
        assert.equal(mounted.ok, true, JSON.stringify(mounted));
        assert.ok(!mounted.value.path.startsWith(`${root}/`), `mounted in the Sandbox whose revive failed: ${mounted.value.path}`);
        assert.equal(spawnSync("cat", [join(mounted.value.path, "README.md")], { encoding: "utf8" }).stdout, "hello\n");
      });
      await check("F44", "rrace7: the mount answer's `revived` names the failed Sandbox and its Workspace with a reason", () => {
        const revived = mounted.value?.revived;
        assert.ok(Array.isArray(revived), `no revived list: ${JSON.stringify(mounted.value)}`);
        const box = revived.find((b) => b.id === ws.sandbox || b.name === folder);
        assert.ok(box, `revived does not name Sandbox ${ws.sandbox}: ${JSON.stringify(revived)}`);
        const item = (box.workspaces ?? []).find((w) => w.path === ws.path);
        assert.ok(typeof item?.reason === "string" && item.reason.length > 0, `no reason for ${ws.path}: ${JSON.stringify(box)}`);
      });
      await check("F44 F26 F33", "rrace7: the old Sandbox's records and overlay stay", () => {
        assert.equal(sha(overlayOf(ws.path)), before, "overlay changed");
        assert.deepEqual(readdirSync(join(HOME, project, "sandboxes", ws.sandbox)).sort(), recordsBefore, "records changed");
      });
    }
  },
};
