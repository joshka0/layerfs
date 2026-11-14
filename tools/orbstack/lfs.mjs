// lfs: the root-side LayerFS control for agents in the VM. Each call is its
// own process; the SDK reopens Sandboxes from LAYERFS_HOME records.
//
//   lfs init <project> <source-dir>
//   lfs fork <project> <branch> [from]
//   lfs mount <project> <branch>            prints {"sandbox","path","revived"}
//   lfs commit <project> <sandbox> <path> <message>
//   lfs unmount <project> <sandbox> <path> [--discard]
//   lfs read <project> <ref> <file>
//   lfs log <project> <branch>
//   lfs write <project> <branch> <file> <message>   content from stdin
//   lfs merge <project> <a> <b> <name> [message]
//   lfs reopen <project> [--legacy-head]   after a restart: start each Sandbox
//                                           that holds saved Workspaces again and
//                                           reopen them at their saved paths;
//                                           exit 1 if one did not reopen
//
// Environment: LAYERFS_RUNTIME=process, LAYERFS_HOME, LAYERFS_DAEMON,
// LAYERFS_STORE_ROOT (each project's Store is <root>/<project>/global/store.sqlite).
import { mkdirSync } from "node:fs";
import { join } from "node:path";

const [command, project, ...rest] = process.argv.slice(2);
const COMMANDS = ["init", "fork", "mount", "commit", "unmount", "read", "log", "write", "merge", "reopen"];
if (!COMMANDS.includes(command) || !project) {
  console.error(`usage: lfs <${COMMANDS.join("|")}> <project> ...`);
  process.exit(2);
}
const volume = join(process.env.LAYERFS_STORE_ROOT ?? "/var/lib/layerfs/stores", project);
const { LayerFS } = await import(process.env.LAYERFS_SDK ?? "/opt/layerfs/sdk/src/index.js");
// The constructor checks the project name, so a refused name makes no folder.
const layerfs = new LayerFS(project, { runtime: "process", storePath: join(volume, "global/store.sqlite") });
mkdirSync(volume, { recursive: true });
const print = (value) => console.log(JSON.stringify(value));
// Every listed Workspace is mounted after the call.
const reopened = (answer) => answer.sandboxes.every((s) => s.workspaces.every((w) => w.mounted));
try {
  switch (command) {
    case "init":
      print(await layerfs.project.init(rest[0]));
      break;
    case "fork":
      print(await layerfs.branch.fork(rest[0], rest[1] ?? "main"));
      break;
    case "mount": {
      const ws = await layerfs.workspace.mount(rest[0]);
      print({ sandbox: ws.sandbox, path: ws.path, revived: ws.revived });
      break;
    }
    case "commit":
      print(await layerfs.workspace.commit({ sandbox: rest[0], path: rest[1] }, rest[2]));
      break;
    case "unmount":
      print(await layerfs.workspace.unmount({ sandbox: rest[0], path: rest[1] }, rest[2] === "--discard"));
      break;
    case "read":
      print(await layerfs.fs.read(rest[0], rest[1]));
      break;
    case "write": {
      const chunks = [];
      for await (const chunk of process.stdin) chunks.push(chunk);
      print(await layerfs.fs.write(rest[0], rest[1], Buffer.concat(chunks).toString("utf8"), rest[2]));
      break;
    }
    case "merge":
      print(await layerfs.branch.merge(rest[0], rest[1], rest[2], rest[3]));
      break;
    case "reopen": {
      // --legacy-head: a record saved before roots were recorded reopens on its
      // Branch head. Stopgap (operator, 2026-11-14): remove it once the overlays
      // of 2026-11-14 are reopened. vm-boot.sh never passes it.
      const { LayerFSOperator } = await import(process.env.LAYERFS_SDK_OPERATOR ?? "/opt/layerfs/sdk/operator/index.js");
      const answer = await new LayerFSOperator(layerfs).sandbox.reopen({ legacyHead: rest.includes("--legacy-head") });
      print(answer);
      if (!reopened(answer)) process.exitCode = 1;
      break;
    }
    case "log":
      print(await layerfs.branch.list_commits(rest[0], 20, 0));
      break;
    default:
      throw new Error(`unknown command ${command}`);
  }
} finally {
  await layerfs.close();
}
