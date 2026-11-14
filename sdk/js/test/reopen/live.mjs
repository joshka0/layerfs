// The live phase: SIGKILL in one container, running daemons, races (see main.mjs).
import assert from "node:assert/strict";
import { execFileSync, spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  existsSync, mkdirSync, readFileSync, readdirSync, readlinkSync, statSync, unlinkSync, utimesSync, writeFileSync,
  rmSync,
} from "node:fs";
import { DatabaseSync } from "node:sqlite";
import { basename, dirname, join } from "node:path";
import {
  BASE, BIG, BIG_SHA, execInFreshProcess, DATA, HOME, LayerFS, LayerFSOperator, MAIN_EDIT, MIXED, OPERATOR, SANDBOXES, SDK, STATE, STORES, answerWorkspaces, assertAnswerShape, assertOneDaemonPerFolder, byPath, check, daemons, failures, init, installLfs, isMount, killDaemons, lfsCli, loadState, manifest, mountTargets, open, overlayOf, records, reopen, same, sandboxRoot, saveState, sh, sha, sidecars, text, until, wsOf,
} from "./harness.mjs";

export const livePhases = {
  // One container, daemons killed with SIGKILL: dead FUSE mounts stay (F24),
  // and reopen meets running daemons and concurrent callers.
  async live() {
    installLfs();
    const l = open("rlive");
    await init(l, BASE);
    await l.branch.fork("w");
    await l.branch.fork("w2");
    const ws = await l.workspace.mount("w");
    await sh(l, ws, "echo live > live.txt");
    const root = sandboxRoot(ws.path);
    const name = basename(root);

    await check("F34 F21 F13", "reopen while the daemon runs starts nothing and reports the served Workspace", async () => {
      const before = daemons().map((d) => d.pid).sort();
      const answer = await reopen(l);
      assert.equal(answer.sandboxes.find((b) => b.name === name)?.state, "running", JSON.stringify(answer));
      const w = byPath(answer, ws.path);
      assert.deepEqual([w?.mounted, w?.reopened, w?.reason], [true, false, "already mounted"], JSON.stringify(w));
      assert.deepEqual(daemons().map((d) => d.pid).sort(), before);
      assert.equal(await sh(l, ws, "cat live.txt"), "live\n");
    });
    await check("F38", "lfs reopen exits 0 when the Sandbox runs", () => {
      const r = lfsCli("reopen", "rlive");
      assert.equal(r.status, 0, `exit ${r.status}: ${r.stderr}`);
    });

    // Mounts that are not this Sandbox's Workspaces (F25): a sibling folder
    // whose name starts with this Sandbox's name, and one outside.
    const probes = [join(SANDBOXES, `${name}0`, "workspaces", basename(ws.path)), "/tmp/reopen-f25"];
    for (const p of probes) {
      mkdirSync(p, { recursive: true });
      execFileSync("mount", ["-t", "tmpfs", "tmpfs", p]);
    }
    const oldLog = readFileSync(join(root, "daemon.log"), "utf8");
    killDaemons();
    await until("the daemon to die", () => daemons().length === 0);
    await check("F24", "precondition: the killed daemon left a dead FUSE mount", () => {
      assert.ok(isMount(ws.path), "no mount left");
      assert.throws(() => readdirSync(ws.path), (e) => ["ENOTCONN", "ECONNABORTED"].includes(e.code));
    });
    const killedAt = Date.now();
    let answer;
    await check("F24 F23 F17", "reopen after SIGKILL replaces the dead mount at the saved path", async () => {
      answer = await reopen(l);
      const w = byPath(answer, ws.path);
      assert.equal(w?.reopened, true, JSON.stringify(w ?? answer));
      assert.equal(readFileSync(join(ws.path, "live.txt"), "utf8"), "live\n");
    });
    await check("F25", "reopen leaves mounts outside its own mount home", () => {
      for (const p of probes) assert.ok(isMount(p), `${p} was detached`);
    });
    await check("F23", "the old daemon log is kept as daemon.previous.log", () => {
      const prev = join(root, "daemon.previous.log");
      assert.ok(existsSync(prev), `${prev} missing`);
      assert.ok(readFileSync(prev, "utf8").startsWith(oldLog.slice(0, 200)), "previous log is not the old log");
    });
    await check("F30", "a fresh lfs process uses the reopened Sandbox (record names the new daemon)", async () => {
      const r = lfsCli("commit", "rlive", ws.sandbox, ws.path, "commit from a fresh process");
      assert.equal(r.status, 0, `${r.stderr}${r.stdout}`);
      assert.equal(text(await l.fs.read("w", "/live.txt")), "live\n");
    });
    await check("F13 F34", "a second reopen starts no daemon", async () => {
      const before = daemons().map((d) => d.pid).sort();
      const again = await reopen(l);
      assert.equal(again.sandboxes.find((b) => b.name === name)?.state, "running", JSON.stringify(again));
      assert.equal(byPath(again, ws.path)?.reason, "already mounted", JSON.stringify(again));
      assert.deepEqual(daemons().map((d) => d.pid).sort(), before);
      assertOneDaemonPerFolder();
    });

    // F35: two reopens and an lfs mount race on a dead Sandbox. ws committed
    // (generation 2, parked P1), so the race uses a new Workspace.
    let ws2;
    await check("F35", "setup: a new saved Workspace, then SIGKILL", async () => {
      await l.workspace.unmount(ws);
      ws2 = await l.workspace.mount("w2");
      await sh(l, ws2, "echo race > race.txt");
    });
    if (!ws2) return;
    killDaemons();
    await until("the daemons to die", () => daemons().length === 0);
    const run = (args) => new Promise((resolve) => {
      const child = spawn("/usr/local/bin/lfs", args, { env: { PATH: process.env.PATH } });
      let out = "";
      child.stdout.on("data", (d) => { out += d; });
      child.stderr.on("data", (d) => { out += d; });
      const timer = setTimeout(() => child.kill("SIGKILL"), 60000);
      child.on("close", (code) => { clearTimeout(timer); resolve({ code, out }); });
    });
    const results = await Promise.all([run(["reopen", "rlive"]), run(["reopen", "rlive"]), run(["mount", "rlive", "w"])]);
    await check("F35", "concurrent reopens and a mount: one daemon per Sandbox folder, the saved Workspace intact", async () => {
      for (const r of results) assert.notEqual(r.code, null, `a caller hung: ${r.out}`);
      assertOneDaemonPerFolder();
      assert.ok(existsSync(overlayOf(ws2.path)), "saved overlay deleted in the race");
      assert.ok(results.slice(0, 2).some((r) => {
        try { return byPath(JSON.parse(r.out.trim().split("\n").pop()), ws2.path)?.reopened; } catch { return false; }
      }), `no reopen reopened ${ws2.path}: ${results.map((r) => r.out).join(" || ")}`);
      assert.ok(isMount(ws2.path), `${ws2.path} not mounted after the race`);
      const evidence = () => `daemons ${JSON.stringify(daemons())}; callers ${results.map((r) => r.out.trim()).join(" || ")}`;
      try {
        // From a separate process: this process's SDK still holds the daemon
        // that was killed (reported apart; not an F35 fault).
        const r = execInFreshProcess("rlive", ws2, "cat race.txt");
        assert.equal(r.status, 0, r.stderr);
        assert.equal(r.stdout, "race\n");
      } catch (error) {
        throw new Error(`${error.message}; ${evidence()}`);
      }
    });
    // Outside F1-F40, so it does not gate: does a client that lived through the
    // restart reach the daemon another process started?
    try {
      await sh(l, ws2, "cat race.txt");
      console.log("NOTE outside-list: a long-lived SDK client reaches the daemon another process restarted");
    } catch (error) {
      console.log(`NOTE outside-list: a long-lived SDK client still dials the killed daemon: ${error.message.split("\n")[0]}`);
    }
    for (const p of probes) spawnSync("umount", [p]);
    console.log(`LIVE done ${Date.now() - killedAt} ms after the first kill`);
  }
};
