// The holder phase (see main.mjs): R4-B1 of PR 24. A daemon refused by the
// folder lock exits at once; a revive that reads the child's identity after
// it already exited loses the refusal line and does not name the holder.
// The race is about 4% per restart one level down (implementer) and about
// 1.3% per revive here (4 of 300 on the pre-fix 1e066f3c0, 2026-11-14), so
// this phase runs 1000 revives: at 1.3% all 1000 pass with p of about 2e-6.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { openSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { BASE, SDK, OPERATOR, STORES, check, daemons, init, installLfs, killDaemons, open, sandboxRoot, sh, until } from "./harness.mjs";

const ROUNDS = 1000;

export const holderPhases = {
  async holder() {
    installLfs();
    const project = "rholder";
    const l = open(project);
    await init(l, BASE);
    await l.branch.fork("w");
    const ws = await l.workspace.mount("w");
    await sh(l, ws, "echo held > h.txt");
    const root = sandboxRoot(ws.path);
    const folder = root.split("/").pop();
    killDaemons();
    await until("the daemons to die", () => daemons().length === 0);
    // An unrecorded daemon holds the folder.
    const out = openSync("/tmp/rholder-holder.log", "a");
    const holder = spawn("/opt/layerfs/bundle/layerfs-daemon", ["--config", join(root, "config", "daemon.setup")],
      { stdio: ["ignore", out, out], detached: true });
    holder.unref();
    await until("the holder to run", () => daemons().some((d) => d.pid === holder.pid), 5000);
    // The stand-in gives exactly the real refusal: one stderr line, status 1.
    const refused = "/usr/local/bin/refused-layerfs-daemon";
    writeFileSync(refused, `#!/bin/sh\necho 'layerfs-daemon: the Sandbox folder is held by daemon pid ${holder.pid}' >&2\nexit 1\n`, { mode: 0o755 });
    const script = `
      const { LayerFS } = await import(${JSON.stringify(SDK)});
      const { LayerFSOperator } = await import(${JSON.stringify(OPERATOR)});
      const l = new LayerFS(${JSON.stringify(project)}, { runtime: "process", storePath: ${JSON.stringify(join(STORES, project, "global/store.sqlite"))} });
      const op = new LayerFSOperator(l);
      const misses = [];
      const started = Date.now();
      for (let round = 1; round <= ${ROUNDS}; round++) {
        let box;
        try { box = (await op.sandbox.reopen()).sandboxes.find((b) => b.name === ${JSON.stringify(folder)}); }
        catch (e) { misses.push({ round, error: String(e.message).slice(0, 200) }); continue; }
        if (box?.state !== "failed" || !String(box.reason).includes("held by daemon pid ${holder.pid}")) {
          misses.push({ round, state: box?.state, reason: String(box?.reason).slice(0, 200) });
        }
      }
      console.log(JSON.stringify({ misses, ms: Date.now() - started }));
      process.exit(0);`;
    const r = spawnSync("node", ["--input-type=module", "-e", script], {
      encoding: "utf8", timeout: 100000, env: { ...process.env, LAYERFS_DAEMON: refused, LAYERFS_CONTAINER_PREFIX: "lfs-" },
    });
    let result = null;
    try { result = JSON.parse(r.stdout.trim().split("\n").pop()); } catch {}
    await check("R4-B1 F45", `rholder: ${ROUNDS} revives whose daemon is refused at once each answer failed naming the holder pid`, () => {
      assert.ok(result, `the rounds did not finish: status ${r.status} ${r.stderr.slice(0, 300)}`);
      console.log(`NOTE R4-B1 ${result.misses.length} of ${ROUNDS} rounds missed, ${result.ms} ms`);
      assert.deepEqual(result.misses, [], `${result.misses.length} of ${ROUNDS} rounds: ${JSON.stringify(result.misses.slice(0, 3))}`);
      assert.deepEqual(daemons().filter((d) => d.folders.includes(folder)).map((d) => d.pid), [holder.pid]);
    });
    process.kill(holder.pid, "SIGKILL");
  },
};
