// Inits project wt6 from /wt6/src, forks w, mounts a Workspace of w and
// prints its path. The process-runtime daemon outlives this process.
import { mkdirSync } from "node:fs";
const { LayerFS } = await import("/sdk/src/index.js");
mkdirSync("/s/wt6", { recursive: true });
const l = new LayerFS("wt6", { runtime: "process", storePath: "/s/wt6/global/store.sqlite" });
try {
  await l.project.init("/wt6/src");
  await l.branch.fork("w", "main");
  const ws = await l.workspace.mount("w");
  console.log(ws.path);
} finally {
  await l.close();
}
