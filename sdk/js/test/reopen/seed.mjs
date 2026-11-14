// Phases before the reopen: seed-legacy, seed and tamper (see main.mjs).
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
  BASE, BIG, BIG_SHA, DATA, HOME, LayerFS, LayerFSOperator, MAIN_EDIT, MIXED, OPERATOR, SANDBOXES, SDK, STATE, STORES, answerWorkspaces, assertAnswerShape, assertOneDaemonPerFolder, byPath, check, daemons, failures, init, installLfs, isMount, killDaemons, lfsCli, loadState, manifest, mountTargets, open, overlayOf, records, reopen, same, sandboxRoot, saveState, sh, sha, sidecars, text, until, wsOf,
} from "./harness.mjs";

export const seedPhases = {
  // With the SDK and daemon from before the change (lfs-orb's bundle): its
  // Workspace records have no `root` (F32).
  // rlegacy is refused by reopen; rlegboot is left for vm-boot, which must
  // refuse it too.
  async "seed-legacy"() {
    const saved = {};
    for (const project of ["rlegacy", "rlegboot"]) {
      const l = open(project);
      await init(l, BASE);
      await l.branch.fork("lw");
      const ws = await l.workspace.mount("lw");
      await sh(l, ws, `echo ${project} > legacy.txt`);
      saved[project] = { ...ws };
    }
    saveState({ legacy: saved });
    console.log("SEEDED legacy");
  },

  async seed() {
    installLfs();
    const state = {};
    // rmain: two saved Workspaces in one Sandbox, one rich.
    {
      const l = open("rmain");
      await init(l, BASE);
      for (const b of ["work", "work2", "work3"]) await l.branch.fork(b);
      const a = await l.workspace.mount("work");
      await sh(l, a, MAIN_EDIT);
      const b = await l.workspace.mount("work2");
      await sh(l, b, "echo b2 > b2.txt");
      state.main = {
        a: { ...a, manifest: manifest(a.path), overlay: (await l.workspace.status(a)).overlay },
        b: { ...b, manifest: manifest(b.path), overlay: (await l.workspace.status(b)).overlay },
      };
      assert.equal(sandboxRoot(a.path), sandboxRoot(b.path), "both rmain Workspaces share one Sandbox");
    }
    // rmixed: one Sandbox, one Workspace per tamper kind (F1-F5, F8, F16, F33).
    {
      const l = open("rmixed");
      await init(l, BASE);
      state.mixed = {};
      for (const kind of MIXED) {
        await l.branch.fork(`m-${kind}`);
        const ws = await l.workspace.mount(`m-${kind}`);
        await sh(l, ws, `echo ${kind} > f.txt && printf 'one\\nmore\\n' > src/a.txt && rm del.txt`);
        state.mixed[kind] = { ...ws };
      }
    }
    // rmoved: its branch gets a Commit after the mount (F14, F31, F36).
    {
      const l = open("rmoved");
      await init(l, BASE);
      await l.branch.fork("mv");
      const ws = await l.workspace.mount("mv");
      await sh(l, ws, "echo moved > m.txt");
      state.moved = { ...ws };
    }
    // ridle: a Sandbox whose only Workspace was unmounted (F28).
    {
      const l = open("ridle");
      await init(l, BASE);
      await l.branch.fork("w");
      const ws = await l.workspace.mount("w");
      await l.workspace.unmount(ws, true);
      state.idle = { root: sandboxRoot(ws.path) };
    }
    // rdisc*: each discovery call meets a dead Sandbox before any reopen (F27).
    state.disc = {};
    for (const project of ["rdisc", "rdisc2", "rdisc3"]) {
      const l = open(project);
      await init(l, BASE);
      await l.branch.fork("w1");
      await l.branch.fork("w2");
      const ws = await l.workspace.mount("w1");
      await sh(l, ws, `echo ${project} > d.txt`);
      state.disc[project] = { ...ws };
    }
    // rnocfg: its config is gone after the restart (F22).
    {
      const l = open("rnocfg");
      await init(l, BASE);
      await l.branch.fork("w");
      const ws = await l.workspace.mount("w");
      await sh(l, ws, "echo cfg > c.txt");
      state.nocfg = { ...ws };
    }
    saveState(state);
    console.log("SEEDED; the runner now removes this container with its daemons and mounts");
  },

  // After the restart, with no daemon: change files and branches as a
  // crash, an old daemon or another process could have left them.
  async tamper() {
    const s = loadState();
    await check("restart", "no daemon and no Workspace mount survive the container restart", () => {
      assert.deepEqual(daemons(), []);
      assert.deepEqual(mountTargets().filter((t) => t.startsWith(SANDBOXES)), []);
    });
    const hashes = {};
    const edit = (path, sql) => {
      const db = new DatabaseSync(overlayOf(path));
      db.exec(sql);
      db.close();
      assert.deepEqual(sidecars(overlayOf(path)), [], "the tamper left no journal");
    };
    const T = {
      gen2: ["fs_inode", "fs_entry", "fs_data", "fs_truncate", "fs_data_ref"].map((t) => `UPDATE ${t} SET generation=2;`).join(""),
      genmix: "UPDATE fs_inode SET generation=2 WHERE inode=(SELECT max(inode) FROM fs_inode);",
      orphan: "UPDATE fs_inode SET generation=-1 WHERE inode=(SELECT max(inode) FROM fs_inode);",
      cleanup: "INSERT INTO cleanup_task(kind,resource,target) VALUES(1,0,1);",
      commitrec: "INSERT INTO commit_record VALUES(1,zeroblob(8),0,zeroblob(32),x'');",
      schema: "PRAGMA user_version=38;",
      appid: "PRAGMA application_id=0;",
    };
    for (const [kind, sql] of Object.entries(T)) edit(s.mixed[kind].path, sql);
    // Precondition of F4: the tampered rows really exist.
    {
      const db = new DatabaseSync(overlayOf(s.mixed.genmix.path), { readOnly: true });
      const gens = db.prepare("SELECT DISTINCT generation FROM fs_inode ORDER BY 1").all().map((r) => r.generation);
      db.close();
      assert.deepEqual(gens, [1, 2], "genmix has rows in generations 1 and 2");
    }
    unlinkSync(overlayOf(s.mixed.missing.path));
    for (const kind of MIXED.filter((k) => k !== "missing")) hashes[s.mixed[kind].path] = sha(overlayOf(s.mixed[kind].path));

    // rmoved: a Commit on its branch after the mount; the record looks newer
    // than that Commit, so only a saved `root` can tell (F31 with F36).
    const moved = open("rmoved");
    await moved.fs.write("mv", "/after.txt", "after mount\n", "commit after the mount");
    const future = new Date(Date.now() + 3600_000);
    for (const r of records("rmoved")) utimesSync(r, future, future);
    hashes[s.moved.path] = sha(overlayOf(s.moved.path));

    for (const w of Object.values(s.legacy ?? {})) hashes[w.path] = sha(overlayOf(w.path));

    // rnocfg: the Sandbox config is gone (F22).
    rmSync(join(sandboxRoot(s.nocfg.path), "config"), { recursive: true, force: true });
    hashes[s.nocfg.path] = sha(overlayOf(s.nocfg.path));

    saveState({
      hashes,
      recordCount: Object.fromEntries(["rmain", "rmixed", "rmoved", "rlegacy", "rlegboot", "rdisc", "rdisc2", "rdisc3", "rnocfg"].map((p) => [p, records(p).length])),
    });
    console.log("TAMPERED");
  }
};
