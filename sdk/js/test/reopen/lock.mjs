// The lock phase (see main.mjs): failure cases written from contract lines
// that rowan-joshka-main relayed on 2026-11-14 for PR 21 (operator chose a
// daemon folder lock), before reading the fix:
//   F45. Two daemons run on one Sandbox folder: a second daemon started on a
//        held folder keeps running, or a revive that meets a held folder does
//        not answer failed with the holder's pid.
//   F46. Stopping a spawned daemon hangs instead of returning an error after
//        a bounded wait (60 s). Not gated end to end: the bound covers only
//        the wait after SIGKILL, which only a process in uninterruptible
//        sleep outlives, and no public setup makes one reliably (implementer,
//        2026-11-14). The check here guards the reachable part: a daemon that
//        ignores SIGTERM is still stopped and the reopen ends.
//   F47. A mount that fails after its old Sandbox's revive failed refuses
//        without the `revived` report (failed revive, per-Workspace reasons).
//   R4-B1 (PR 24). A lock-refused daemon exits fast; a revive that races
//        its exit loses its first output line and does not name the holder.
//        Checked over 10 rounds.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { openSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  BASE, HOME, STORES, SDK, check, daemons, init, installLfs, killDaemons, lfsCli, open, overlayOf, sandboxRoot, sh,
  sha, until,
} from "./harness.mjs";

const DAEMON = "/opt/layerfs/bundle/layerfs-daemon";
const lfsWith = (env, ...args) => {
  const started = Date.now();
  const r = spawnSync("node", ["/opt/layerfs/tools/lfs.mjs", ...args], {
    encoding: "utf8", timeout: 100000, env: { ...process.env, LAYERFS_CONTAINER_PREFIX: "lfs-", ...env },
  });
  let json = null;
  try { json = JSON.parse(r.stdout.trim().split("\n").pop()); } catch {}
  return { ...r, json, ms: Date.now() - started };
};
// A dead Sandbox with one saved Workspace; returns it and its folder.
async function deadSandbox(project) {
  const l = open(project);
  await init(l, BASE);
  await l.branch.fork("w");
  await l.branch.fork("w2");
  const ws = await l.workspace.mount("w");
  await sh(l, ws, `echo ${project} > s.txt`);
  const root = sandboxRoot(ws.path);
  killDaemons();
  await until("the daemons to die", () => daemons().length === 0);
  return { l, ws, root, folder: root.split("/").pop(), config: join(root, "config", "daemon.setup") };
}
// A daemon this test starts by hand on a Sandbox folder, as the SDK does.
function startDaemon(config, log) {
  const out = openSync(log, "a");
  const child = spawn(DAEMON, ["--config", config], { stdio: ["ignore", out, out], detached: true });
  child.unref();
  return child;
}

export const lockPhases = {
  async lock() {
    installLfs();

    // F45a: a second daemon on a held folder exits.
    {
      const { root, folder, config } = await deadSandbox("rlock1");
      const holder = startDaemon(config, "/tmp/rlock1-holder.log");
      await until("the holder daemon to run", () => daemons().some((d) => d.pid === holder.pid), 5000);
      await check("F45", "rlock1: a second daemon on a held folder exits 1 naming the holder pid", () => {
        const second = spawnSync(DAEMON, ["--config", config], { encoding: "utf8", timeout: 10000 });
        assert.notEqual(second.status, null, `second daemon still ran after 10 s: ${JSON.stringify(daemons())}`);
        assert.equal(second.status, 1, `exit ${second.status}: ${second.stderr}`);
        assert.equal(second.stderr, `layerfs-daemon: the Sandbox folder is held by daemon pid ${holder.pid}\n`);
        assert.deepEqual(daemons().filter((d) => d.folders.includes(folder)).map((d) => d.pid), [holder.pid]);
      });

      // F45b, R4-B1: the recorded daemon is dead, an unrecorded one holds the
      // folder. Each revive's daemon is refused by the lock and exits fast.
      await check("F45 R4-B1", "rlock1: 10 revives that meet a held folder each answer failed naming the holder pid", () => {
        const misses = [];
        for (let round = 1; round <= 10; round++) {
          const r = lfsCli("reopen", "rlock1");
          const box = r.json?.sandboxes?.find((b) => b.name === folder);
          if (box?.state !== "failed" || !String(box.reason).includes(`held by daemon pid ${holder.pid}`)) {
            misses.push({ round, status: r.status, state: box?.state, reason: box?.reason ?? `${r.stdout}${r.stderr}`.slice(0, 300) });
          }
          const left = daemons().filter((d) => d.folders.includes(folder)).map((d) => d.pid);
          if (left.length !== 1 || left[0] !== holder.pid) misses.push({ round, daemons: left });
        }
        assert.deepEqual(misses, [], JSON.stringify(misses));
      });
      // R4-B1, sharper: a stand-in daemon gives exactly the real refusal
      // (the one stderr line, status 1) with no startup, so it exits as fast
      // as a refused daemon can.
      const refused = "/usr/local/bin/refused-layerfs-daemon";
      writeFileSync(refused, `#!/bin/sh\necho 'layerfs-daemon: the Sandbox folder is held by daemon pid ${holder.pid}' >&2\nexit 1\n`, { mode: 0o755 });
      await check("R4-B1", "rlock1: 20 revives whose daemon is refused at once each answer failed naming the holder pid", () => {
        const misses = [];
        for (let round = 1; round <= 20; round++) {
          const r = lfsWith({ LAYERFS_DAEMON: refused }, "reopen", "rlock1");
          const box = r.json?.sandboxes?.find((b) => b.name === folder);
          if (box?.state !== "failed" || !String(box.reason).includes(`held by daemon pid ${holder.pid}`)) {
            misses.push({ round, status: r.status, state: box?.state, reason: box?.reason ?? `${r.stdout}${r.stderr}`.slice(0, 300) });
          }
        }
        assert.deepEqual(misses, [], `${misses.length} of 20 rounds: ${JSON.stringify(misses.slice(0, 3))}`);
      });
      process.kill(holder.pid, "SIGKILL");
      await until("the holder to die", () => !daemons().some((d) => d.pid === holder.pid));
      await check("F45 F26", "rlock1: once the holder is gone, a reopen mounts the saved Workspace", () => {
        const r = lfsCli("reopen", "rlock1");
        assert.equal(r.status, 0, `${r.stdout}${r.stderr}`);
        assert.equal(spawnSync("cat", [join(root, "workspaces", "1", "s.txt")], { encoding: "utf8" }).stdout, "rlock1\n");
      });
    }

    // F46: the restart fails at its record save, so its cleanup stops the
    // daemon it spawned. The wrapper sets SIGTERM to ignored and execs the
    // daemon, so the spawned pid is the daemon itself, as in production.
    {
      const { ws, folder } = await deadSandbox("rlock2");
      const stubborn = "/usr/local/bin/stubborn-layerfs-daemon";
      writeFileSync(stubborn, `#!/bin/sh\ntrap '' TERM\nexec ${DAEMON} "$@"\n`, { mode: 0o755 });
      const record = join(HOME, "rlock2", "sandboxes", ws.sandbox, "sandbox.record");
      const lock = spawnSync("chattr", ["+i", record], { encoding: "utf8" });
      if (lock.status !== 0) {
        console.log(`SKIP F46 chattr +i refused: ${lock.stderr.trim()}`);
      } else {
        let r;
        try { r = lfsWith({ LAYERFS_DAEMON: stubborn }, "reopen", "rlock2"); } finally { spawnSync("chattr", ["-i", record]); }
        await check("F46", "rlock2: a restart that fails after its spawn stops a daemon that ignores SIGTERM and ends, failed", () => {
          assert.notEqual(r.status, null, `lfs reopen hung past ${r.ms} ms`);
          assert.ok(r.ms < 75000, `took ${r.ms} ms`);
          const box = r.json?.sandboxes?.find((b) => b.name === folder);
          assert.equal(box?.state, "failed", `${r.stdout}${r.stderr}`);
          assert.ok(!daemons().some((d) => d.folders.includes(folder)), `daemons still on ${folder}: ${JSON.stringify(daemons())}`);
        });
        console.log(`NOTE F46 rlock2 reopen took ${r.ms} ms; reason: ${r.json?.sandboxes?.[0]?.reason}`);
      }
      killDaemons();
    }

    // F47: the old Sandbox's revive fails (its config is gone), and the
    // replacement start is refused too (its daemon exits at once).
    {
      const { ws, root, folder } = await deadSandbox("rlock3");
      spawnSync("rm", ["-rf", join(root, "config")]);
      const before = sha(overlayOf(ws.path));
      const dead = "/usr/local/bin/exiting-layerfs-daemon";
      writeFileSync(dead, "#!/bin/sh\nexit 1\n", { mode: 0o755 });
      const script = `
        const { LayerFS } = await import(${JSON.stringify(SDK)});
        const l = new LayerFS("rlock3", { runtime: "process", storePath: ${JSON.stringify(join(STORES, "rlock3", "global/store.sqlite"))} });
        try { console.log(JSON.stringify({ ok: true, value: await l.workspace.mount("w2") })); }
        catch (e) { console.log(JSON.stringify({ ok: false, code: e.code ?? null, message: String(e.message).split("\\n")[0], detail: e.detail ?? null })); }
        process.exit(0);`;
      const r = spawnSync("node", ["--input-type=module", "-e", script], {
        encoding: "utf8", timeout: 90000, env: { ...process.env, LAYERFS_DAEMON: dead, LAYERFS_CONTAINER_PREFIX: "lfs-" },
      });
      let out = null;
      try { out = JSON.parse(r.stdout.trim().split("\n").pop()); } catch {}
      await check("F47", "rlock3: precondition: the mount is refused", () => {
        assert.equal(out?.ok, false, `${r.stdout}${r.stderr}`);
      });
      await check("F47", "rlock3: the refusal carries `revived` naming the failed Sandbox and its Workspace with a reason", () => {
        const revived = out?.detail?.revived;
        assert.ok(Array.isArray(revived), `no detail.revived: ${JSON.stringify(out)}`);
        const box = revived.find((b) => b.id === ws.sandbox || b.name === folder);
        assert.equal(box?.state, "failed", JSON.stringify(revived));
        const item = (box.workspaces ?? []).find((w) => w.path === ws.path);
        assert.equal(item?.mounted, false, JSON.stringify(box));
        assert.ok(typeof item?.reason === "string" && item.reason.length > 0, JSON.stringify(box));
      });
      await check("F47 F26", "rlock3: the old Sandbox's overlay stays", () => {
        assert.equal(sha(overlayOf(ws.path)), before);
      });
    }
  },
};
