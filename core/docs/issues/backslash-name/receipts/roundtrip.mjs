// Reads a project's Store through the SDK of the mounted bundle and prints one
// JSON line: the init answer (when a source is given), the branch list, the
// commits of `main`, and every path on `main` with the sha256 of its read
// answer. Reading every path decodes every directory page and file.
// usage: STORES=<dir> node roundtrip.mjs <project> [<source-dir>]
// With a source it inits the project and forks Branch `w` from `main`;
// either way it then reads `w`.
import { createHash } from "node:crypto";
import { mkdirSync } from "node:fs";
const [project, source] = process.argv.slice(2);
const { LayerFS } = await import("/sdk/src/index.js");
mkdirSync(`${process.env.STORES}/${project}`, { recursive: true });
const l = new LayerFS(project, { runtime: "process", storePath: `${process.env.STORES}/${project}/global/store.sqlite` });
const out = { project };
const items = (v) => (Array.isArray(v) ? v : v?.items ?? v?.paths ?? v?.entries ?? []);
try {
  if (source) {
    out.init = await l.project.init(source);
    out.fork = await l.branch.fork("w", "main");
  }
  out.branches = await l.project.list_branches();
  out.commits = await l.branch.list_commits("w", 20, 0);
  const paths = [];
  const size = 500;
  for (let pos = 0; ; pos += size) {
    const page = await l.fs.glob("w", "**", "/", size, pos);
    if (pos === 0) out.globShape = Object.keys(page ?? {});
    const got = items(page).map((i) => (typeof i === "string" ? i : i.path));
    paths.push(...got);
    if (got.length < size) break;
  }
  out.pathCount = paths.length;
  const files = {};
  let read = 0, refused = 0;
  for (const p of paths.sort()) {
    try {
      const answer = await l.fs.read("w", p);
      files[p] = createHash("sha256").update(JSON.stringify(answer)).digest("hex");
      read++;
    } catch (e) {
      files[p] = `refused ${e.code}`;
      refused++;
    }
  }
  out.read = read;
  out.refused = refused;
  out.filesDigest = createHash("sha256").update(JSON.stringify(files)).digest("hex");
  out.files = files;
} catch (e) {
  out.error = { code: e.code ?? null, message: String(e.message) };
} finally {
  await l.close();
}
console.log(JSON.stringify(out));
