// The boot phases: vm-boot.sh apply after a restart (see main.mjs).
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

export const bootPhases = {
  // F40: /var/lib/layerfs is not a mount: vm-boot must reopen nothing and
  // touch no record, and still finish its other steps.
  async "boot-unmounted"() {
    installLfs();
    const before = Object.fromEntries(["rmain", "rmixed"].map((p) => [p, records(p).length]));
    const r = bootApply();
    await check("F40", "vm-boot apply without the Sandbox volume reopens nothing and says why", () => {
      assert.equal(r.status, 0, `exit ${r.status}: ${r.stderr}`);
      assert.match(r.stderr, /vm-boot: Store volume not mounted, no reopen/);
      assert.deepEqual(daemons(), []);
      assert.ok(!existsSync(SANDBOXES) || readdirSync(SANDBOXES).length === 0, "Sandbox folders appeared");
      for (const [p, n] of Object.entries(before)) assert.equal(records(p).length, n, `${p} records changed`);
    });
    await check("F39 F40", "vm-boot apply still starts sshd", () => assert.ok(existsSync("/run/sshd-started")));
  },

  // A second restart: vm-boot apply reopens every project; a project whose
  // reopen fails must not stop the boot (F39).
  async boot() {
    installLfs();
    const s = loadState();
    const r = bootApply();
    await check("F39", "vm-boot apply exits 0 and starts sshd though rmixed fails to reopen", () => {
      assert.equal(r.status, 0, `exit ${r.status}: ${r.stderr}`);
      assert.ok(existsSync("/run/sshd-started"), "sshd not started");
    });
    await check("F39 F9", "vm-boot apply reopened rmain's saved Workspace", () => {
      assert.ok(isMount(s.main3.path), `${s.main3.path} not mounted: ${r.stderr}`);
      assert.equal(manifest(s.main3.path), s.main3.manifest);
    });
    await check("F39", "vm-boot apply reopened rmixed's good Workspace despite its failing siblings", () => {
      assert.ok(isMount(s.mixed.good.path), `${s.mixed.good.path} not mounted`);
    });
    if (s.legacy) {
      await check("F32", "vm-boot apply does not reopen a record without root", () => {
        const w = s.legacy.rlegboot;
        assert.ok(!isMount(w.path), `${w.path} mounted`);
        assert.equal(sha(overlayOf(w.path)), s.hashes[w.path], "overlay changed");
        assert.equal(records("rlegboot").length, s.recordCount.rlegboot, "record deleted");
      });
    }
  }
};

// vm-boot.sh apply as the container command runs it: no LAYERFS_* in its
// environment, a boot.env, and an sshd stub that leaves a marker.
function bootApply() {
  mkdirSync("/etc/lfs-vm", { recursive: true });
  writeFileSync("/etc/lfs-vm/boot.env", 'SERVICE_FORWARDS=""\nGIT_SSH_HOSTS=""\nTCP_EGRESS=""\n');
  writeFileSync("/usr/sbin/sshd", "#!/bin/sh\ntouch /run/sshd-started\n", { mode: 0o755 });
  return spawnSync("/opt/layerfs/tools/vm-boot.sh", ["apply"], {
    encoding: "utf8", timeout: 100000, env: { PATH: "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin", HOME: "/root" },
  });
}
