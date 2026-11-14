// Reopen after restart E2E: the gating tests for core/docs/issues/373/REOPEN.md
// (failure list F1-F40). tools/orbstack/reopen-e2e.sh runs each phase in its
// own throwaway container, on two named volumes laid out as in lfs-orb:
//   /var/lib/layerfs-data   LAYERFS_HOME (home/) and the Stores (stores/)
//   /var/lib/layerfs        the process Sandboxes (sandboxes/<name>/)
// A real restart is the removal of one container and the start of the next on
// the same volumes: every daemon dies, every FUSE mount goes, the files stay.
//
//   node sdk/js/test/reopen/main.mjs <phase>
//   phases, in order: seed-legacy seed tamper check boot-unmounted boot live races names lock
//
// Each check prints `PASS <F ids> <what>` or `FAIL <F ids> <what>: <why>`;
// the runner collects them into the report. The phase exits 1 when a check failed.
import { check, failures } from "./harness.mjs";
import { seedPhases } from "./seed.mjs";
import { checkPhases } from "./check.mjs";
import { livePhases } from "./live.mjs";
import { bootPhases } from "./boot.mjs";
import { racesPhases } from "./races.mjs";
import { namesPhases } from "./names.mjs";
import { lockPhases } from "./lock.mjs";

const phases = { ...seedPhases, ...checkPhases, ...livePhases, ...bootPhases, ...racesPhases, ...namesPhases, ...lockPhases };
const phase = process.argv[2];
if (!phases[phase]) {
  console.error(`usage: main.mjs <${Object.keys(phases).join("|")}>`);
  process.exit(2);
}
await check(`setup ${phase}`, "the phase ran to its end", () => phases[phase]());
console.log(`PHASE ${phase} ${failures() ? `FAILED ${failures()}` : "ok"}`);
process.exit(failures() ? 1 : 0);
