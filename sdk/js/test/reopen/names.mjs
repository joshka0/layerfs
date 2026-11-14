// The names phase (see main.mjs): names with a backslash, written by the
// independent test author from the operator's contract sentence of
// 2026-11-14 (ADR docs/adr/0006-backslash-in-names.md), before reading the
// fix or its tests:
//   N1. A lookup or stat of a missing `a\b` answers EINVAL, not ENOENT.
//   N2. mkdir, create or rename to a name with a backslash is refused, or the
//       name does not read back byte-exact after commit and a new mount.
//   N3. A name that is not UTF-8 (`a\xff`) is accepted instead of EINVAL.
// Every call goes through the real FUSE mount: as root from this process, and
// as the agent (uid 501) through workspace.exec.
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, readdirSync, renameSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { BASE, check, init, open, sh, text } from "./harness.mjs";

const BS = "\\";
const errno = (body) => {
  try { body(); return "ok"; } catch (error) { return error.code; }
};
// A path whose last name is raw bytes, for names that are not UTF-8.
const rawPath = (dir, bytes) => Buffer.concat([Buffer.from(`${dir}/`), Buffer.from(bytes)]);

export const namesPhases = {
  async names() {
    const l = open("rnames");
    await init(l, BASE);
    await l.branch.fork("n");
    const ws = await l.workspace.mount("n");
    const at = (name) => join(ws.path, name);

    await check("N1", "stat and open of a missing a\\b answer ENOENT", () => {
      assert.equal(errno(() => statSync(at(`a${BS}b`))), "ENOENT");
      assert.equal(errno(() => readFileSync(at(`a${BS}b`))), "ENOENT");
      assert.equal(errno(() => statSync(at(`dir${BS}x/y`))), "ENOENT");
    });
    await check("N1", "the agent's shell: stat of a missing a\\b says No such file", async () => {
      const r = await l.workspace.exec(ws, "stat 'a\\b' 2>&1; echo rc=$?");
      assert.match(r.stdout, /No such file or directory/, r.stdout);
      assert.match(r.stdout, /rc=1/, r.stdout);
    });

    await check("N2", "mkdir, create and rename to names with a backslash work through the mount", () => {
      mkdirSync(at(`d${BS}1`));
      writeFileSync(join(at(`d${BS}1`), `f${BS}2.txt`), "inside\n");
      writeFileSync(at(`old${BS}name`), "renamed\n");
      renameSync(at(`old${BS}name`), at(`new${BS}name`));
      writeFileSync(at("plain.txt"), "to-backslash\n");
      renameSync(at("plain.txt"), at(`x${BS}y`));
      assert.ok(statSync(at(`d${BS}1`)).isDirectory());
      assert.equal(readFileSync(at(`new${BS}name`), "utf8"), "renamed\n");
      assert.equal(errno(() => statSync(at(`old${BS}name`))), "ENOENT", "rename left the old name");
    });
    await check("N2", "the agent's shell creates and lists a name with a backslash", async () => {
      await sh(l, ws, "mkdir 'sh\\dir' && printf agent > 'sh\\dir/a\\b' && ls 'sh\\dir'");
      assert.equal(await sh(l, ws, "cat 'sh\\dir/a\\b'"), "agent");
    });
    const expectedTop = [`d${BS}1`, `new${BS}name`, `sh${BS}dir`, `x${BS}y`];
    await check("N2", "readdir lists each backslash name byte-exact", () => {
      const names = readdirSync(ws.path);
      for (const name of expectedTop) assert.ok(names.includes(name), `${JSON.stringify(name)} missing from ${JSON.stringify(names)}`);
    });

    await check("N3", "a name that is not UTF-8 (a\\xff) is refused EINVAL for mkdir, create and rename", () => {
      const bad = [0x61, 0xff];
      const badBs = [0x61, 0x5c, 0xff];
      assert.equal(errno(() => mkdirSync(rawPath(ws.path, bad))), "EINVAL");
      assert.equal(errno(() => writeFileSync(rawPath(ws.path, badBs), "x")), "EINVAL");
      writeFileSync(at("src.txt"), "s\n");
      assert.equal(errno(() => renameSync(at("src.txt"), rawPath(ws.path, bad))), "EINVAL");
      assert.equal(errno(() => statSync(rawPath(ws.path, bad))), "EINVAL");
      assert.ok(!readdirSync(ws.path, { encoding: "buffer" }).some((n) => n.includes(0xff)), "a non-UTF-8 name exists");
    });

    let committed = false;
    await check("N2", "commit publishes the backslash names", async () => {
      const r = await l.workspace.commit(ws, "backslash names");
      assert.equal(r.kind, "Committed", JSON.stringify(r));
      committed = true;
      assert.equal(text(await l.fs.read("n", `/new${BS}name`)), "renamed\n");
      assert.equal(text(await l.fs.read("n", `/d${BS}1/f${BS}2.txt`)), "inside\n");
    });
    if (!committed) return;
    await l.workspace.unmount(ws, true).catch(() => {});
    const again = await l.workspace.mount("n");
    await check("N2", "a new mount reads every backslash name back byte-exact", () => {
      const names = readdirSync(again.path);
      for (const name of expectedTop) assert.ok(names.includes(name), `${JSON.stringify(name)} missing after remount: ${JSON.stringify(names)}`);
      assert.equal(readFileSync(join(again.path, `d${BS}1`, `f${BS}2.txt`), "utf8"), "inside\n");
      assert.equal(readFileSync(join(again.path, `x${BS}y`), "utf8"), "to-backslash\n");
      assert.equal(readFileSync(join(again.path, `sh${BS}dir`, `a${BS}b`), "utf8"), "agent");
      assert.equal(errno(() => statSync(join(again.path, `old${BS}name`))), "ENOENT");
    });
  },
};
