// The check phase: discovery, reopen and use of every seeded project (see main.mjs).
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

export const checkPhases = {
  async check() {
    installLfs();
    const s = loadState();
    const all = [s.main.a, s.main.b, ...Object.values(s.mixed), s.moved, ...Object.values(s.disc), s.nocfg, ...Object.values(s.legacy ?? {})];

    // F27 first: discovery meets the dead Sandboxes before any reopen.
    const kept = (project) => {
      const w = s.disc[project];
      assert.ok(existsSync(overlayOf(w.path)), `saved overlay ${overlayOf(w.path)} deleted`);
      assert.ok(records(project).length >= s.recordCount[project], "saved Workspace record deleted");
    };
    await check("F27", "operator.sandbox.list keeps a dead Sandbox that has a saved Workspace overlay", async () => {
      await new LayerFSOperator(open("rdisc")).sandbox.list();
      kept("rdisc");
    });
    await check("F27", "workspace.mount places beside a dead Sandbox with a saved overlay, and keeps it", async () => {
      const fresh = await open("rdisc2").workspace.mount("w2");
      assert.notEqual(fresh.path, s.disc.rdisc2.path, "the new mount took the saved Workspace's path");
      kept("rdisc2");
    });
    await check("F27", "lfs mount from a fresh process keeps the dead Sandbox", async () => {
      const r = lfsCli("mount", "rdisc3", "w2");
      assert.equal(r.status, 0, r.stderr);
      assert.notEqual(r.json?.path, s.disc.rdisc3.path, "the new mount took the saved Workspace's path");
      kept("rdisc3");
    });
    for (const project of ["rdisc", "rdisc2", "rdisc3"]) {
      await check("F27 F9", `${project}: after the discovery call, reopen restores the saved Workspace`, async () => {
        const l = open(project);
        const w = byPath(await reopen(l), s.disc[project].path);
        assert.equal(w?.reopened, true, JSON.stringify(w));
        assert.equal(await sh(l, wsOf(s.disc[project]), "cat d.txt"), `${project}\n`);
      });
    }

    // F28: a dead Sandbox with no saved Workspace is still reclaimed.
    await check("F28", "operator.sandbox.list reclaims a dead Sandbox with no saved Workspace", async () => {
      const idle = open("ridle");
      await new LayerFSOperator(idle).sandbox.list();
      assert.ok(!existsSync(s.idle.root), `${s.idle.root} still exists`);
      assert.ok(!existsSync(join(HOME, "ridle", "sandboxes", "1")), "its records still exist");
    });

    // F26: nothing deleted an overlay file before the reopen.
    await check("F26", "every saved overlay file exists before the reopen", () => {
      const lost = all.filter((w) => w !== s.mixed.missing && !existsSync(overlayOf(w.path))).map((w) => w.path);
      assert.deepEqual(lost, []);
    });

    // rmain through the CLI.
    const main = open("rmain");
    let cli;
    await check("F38 F37", "lfs reopen rmain exits 0 and prints the answer when every Workspace reopened", () => {
      cli = lfsCli("reopen", "rmain");
      assert.equal(cli.status, 0, `exit ${cli.status}: ${cli.stderr}${cli.stdout}`);
      assertAnswerShape(cli.json);
      assert.equal(cli.json.sandboxes.length, 1);
      assert.equal(cli.json.sandboxes[0].state, "restarted");
    });
    for (const key of ["a", "b"]) {
      const w = s.main[key];
      await check("F17 F37", `rmain ${key}: reopened at its saved path ${w.path}`, () => {
        const got = byPath(cli?.json, w.path);
        assert.ok(got, `answer does not name ${w.path}: ${JSON.stringify(cli?.json)}`);
        assert.equal(got.reopened, true, got.reason);
        assert.ok(isMount(w.path), `${w.path} is not mounted`);
      });
      await check("F2 F9", `rmain ${key}: files, bytes, modes, symlinks and deletions match the pre-restart listing`, () => {
        assert.equal(manifest(w.path), w.manifest);
      });
      await check("F7", `rmain ${key}: status counts the same overlay changes as before the restart`, async () => {
        assert.deepEqual((await main.workspace.status(wsOf(w))).overlay, w.overlay);
      });
    }
    await check("F10 F11", "rmain: a new mount beside the reopened ones gets its own path and leaves them unchanged", async () => {
      const m = await main.workspace.mount("work3");
      assert.ok(![s.main.a.path, s.main.b.path].includes(m.path), `new mount reused ${m.path}`);
      await sh(main, m, "echo three > w3.txt && rm README.md");
      assert.equal(await sh(main, m, "cat w3.txt"), "three\n");
      assert.equal(manifest(s.main.a.path), s.main.a.manifest, "saved Workspace a changed");
      assert.equal(manifest(s.main.b.path), s.main.b.manifest, "saved Workspace b changed");
      saveState({ main3: { ...m, manifest: manifest(m.path) } });
    });
    await check("F9", "rmain a: the known edits read back through the mount", () => {
      const p = s.main.a.path;
      assert.equal(readFileSync(join(p, "src/a.txt"), "utf8"), "one\ntwo\n");
      assert.ok(!existsSync(join(p, "del.txt")), "deleted file is back");
      assert.equal(readFileSync(join(p, "new/deep/n.txt"), "utf8"), "n\n");
      assert.equal(statSync(join(p, "keep.sh")).mode & 0o777, 0o755);
      assert.equal(readlinkSync(join(p, "link")), "src/a.txt");
      assert.equal(statSync(join(p, "trunc.bin")).size, 5000);
      assert.equal(sha(join(p, "big.bin")), BIG_SHA);
    });
    await check("F18", "rmain a: new files after the reopen get inode numbers no other file has", async () => {
      await same(main, s.main.a, "work");
      await sh(main, wsOf(s.main.a), "touch n1 n2 n3 && mkdir n4 && echo x > n4/x");
      const dups = await sh(main, wsOf(s.main.a), "find . -xdev -printf '%i\\n' | sort | uniq -d");
      assert.equal(dups, "", `duplicate inode numbers: ${dups}`);
    });
    await check("F6", "rmain a: a file rewritten after the reopen commits its new bytes", async () => {
      await same(main, s.main.a, "work");
      await sh(main, wsOf(s.main.a), "printf 'one\\ntwo\\nthree\\n' > src/a.txt");
    });
    await check("F19 F7 F6", "rmain a: commit publishes the saved and the new changes on the saved base", async () => {
      await same(main, s.main.a, "work");
      const r = await main.workspace.commit(wsOf(s.main.a), "after reopen");
      assert.equal(r.kind, "Committed", `commit answered ${r.kind}`);
      assert.equal(text(await main.fs.read("work", "/src/a.txt")), "one\ntwo\nthree\n");
      assert.equal(text(await main.fs.read("work", "/README.md")), "hello\n", "base file lost");
      assert.equal(text(await main.fs.read("work", "/new/deep/n.txt")), "n\n");
      await assert.rejects(main.fs.read("work", "/del.txt"), "deleted file published");
      const stat = await main.fs.stat("work", "/trunc.bin");
      assert.ok(JSON.stringify(stat).includes("5000"), `trunc.bin stat ${JSON.stringify(stat)}`);
    });
    await check("F20", "rmain a: unmount without discard succeeds after the commit", async () => {
      await same(main, s.main.a, "work");
      await main.workspace.unmount(wsOf(s.main.a));
      assert.ok(!isMount(s.main.a.path));
    });
    await check("F20", "rmain b: unmount without discard is refused as Dirty while it has changes", async () => {
      await same(main, s.main.b, "work2");
      await assert.rejects(main.workspace.unmount(wsOf(s.main.b)), (e) => e.code === "Dirty" || /Dirty/.test(e.message));
      assert.ok(isMount(s.main.b.path), "refused unmount removed the mount");
    });
    await check("F20", "rmain b: unmount with discard deletes the reopened overlay file", async () => {
      await main.workspace.unmount(wsOf(s.main.b), true);
      assert.ok(!existsSync(overlayOf(s.main.b.path)), `${overlayOf(s.main.b.path)} remains`);
      assert.ok(!isMount(s.main.b.path));
    });

    // rmixed: one good Workspace and eight refusals in one Sandbox.
    const mixed = open("rmixed");
    const mcli = lfsCli("reopen", "rmixed");
    await check("F38", "lfs reopen rmixed exits 1 when a Workspace did not reopen", () => {
      assert.equal(mcli.status, 1, `exit ${mcli.status}: ${mcli.stderr}`);
      assertAnswerShape(mcli.json);
    });
    await check("F33", "rmixed good: reopens though its siblings fail", async () => {
      const w = byPath(mcli.json, s.mixed.good.path);
      assert.equal(w?.reopened, true, JSON.stringify(w ?? mcli.json));
      assert.equal(await sh(mixed, wsOf(s.mixed.good), "cat f.txt src/a.txt; test ! -e del.txt"), "good\none\nmore\n");
    });
    const refusal = { F1: "missing", F3: ["schema", "appid"], F4: ["gen2", "genmix", "orphan"], F5: ["cleanup", "commitrec"] };
    for (const [id, kinds] of Object.entries(refusal)) for (const kind of [kinds].flat()) {
      const w = s.mixed[kind];
      await check(`${id} F33`, `rmixed ${kind}: not reopened, with a reason`, () => {
        const got = byPath(mcli.json, w.path);
        assert.ok(got, `answer does not name ${w.path}`);
        assert.equal(got.reopened, false, `${kind} reopened`);
        assert.ok(!isMount(w.path), `${w.path} is mounted`);
      });
      if (kind === "missing") {
        await check("F1", "rmixed missing: no overlay file is created", () => assert.ok(!existsSync(overlayOf(w.path))));
      } else {
        await check(`F8 F16 ${id}`, `rmixed ${kind}: the refused overlay file is byte-identical`, () => {
          assert.equal(sha(overlayOf(w.path)), s.hashes[w.path]);
          assert.deepEqual(sidecars(overlayOf(w.path)), []);
        });
      }
    }
    await check("F33", "rmixed: every Workspace record survives the failed reopens", () => {
      assert.equal(records("rmixed").length, s.recordCount.rmixed);
    });

    // rmoved: its branch moved after the mount (F14, F31, F36).
    await check("F31 F14 F36", "rmoved: not reopened when the branch has a Commit after the mount", async () => {
      const answer = await reopen(open("rmoved"));
      const w = byPath(answer, s.moved.path);
      assert.equal(w?.reopened, false, JSON.stringify(w ?? answer));
      assert.ok(!isMount(s.moved.path));
      assert.equal(sha(overlayOf(s.moved.path)), s.hashes[s.moved.path], "overlay changed");
      assert.equal(records("rmoved").length, s.recordCount.rmoved, "record deleted");
    });

    // rlegacy: a record without `root`, from the SDK before the change (F32).
    if (s.legacy) {
      const w = s.legacy.rlegacy;
      const legacy = open("rlegacy");
      await check("F32", "rlegacy: a record without root does not reopen without legacyHead", async () => {
        const got = byPath(await reopen(legacy), w.path);
        assert.equal(got?.reopened, false, JSON.stringify(got));
        assert.ok(!isMount(w.path), `${w.path} mounted`);
        assert.equal(sha(overlayOf(w.path)), s.hashes[w.path], "overlay changed");
        assert.equal(records("rlegacy").length, s.recordCount.rlegacy, "record deleted");
      });
      await check("F32 F38", "rlegacy: lfs reopen --legacy-head reopens it on the branch head and exits 0", async () => {
        const r = lfsCli("reopen", "rlegacy", "--legacy-head");
        assert.equal(r.status, 0, `exit ${r.status}: ${r.stderr}${r.stdout}`);
        assertAnswerShape(r.json);
        assert.equal(byPath(r.json, w.path)?.reopened, true, JSON.stringify(r.json));
        assert.equal(await sh(legacy, wsOf(w), "cat legacy.txt src/a.txt"), "rlegacy\none\n");
      });
    } else {
      console.log("SKIP F32 no legacy bundle given to the runner");
    }

    // rnocfg: the Sandbox config is gone (F22).
    await check("F22 F26", "rnocfg: restart refuses a Sandbox without its config and creates none", async () => {
      const answer = await reopen(open("rnocfg"));
      const box = answer.sandboxes.find((b) => b.workspaces.some((w) => w.path === s.nocfg.path));
      assert.equal(box?.state, "failed", JSON.stringify(answer));
      assert.ok(!existsSync(join(sandboxRoot(s.nocfg.path), "config")), "config created");
      assert.equal(sha(overlayOf(s.nocfg.path)), s.hashes[s.nocfg.path], "overlay changed");
    });

    await check("F21 F35", "after all reopens: no two daemons hold one Sandbox folder", () => { assertOneDaemonPerFolder(); });
  }
};
