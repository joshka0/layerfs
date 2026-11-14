// One process Sandbox folder after a restart: the daemon its record names
// has ended, and the folder may hold saved Workspace overlays. Three
// callers race on it, each its own process: the boot reopen (lfs reopen),
// a mount (placement) and a listing (operator.sandbox.list).
//
// Each caller observes the record outside any guard, takes the project
// guard (one lock: every mutating path takes it first), then reads the
// record and the running daemon again and acts: use the daemon, restart it
// and reopen the Workspaces, or reclaim (delete) the folder. Source:
// sdk/js/native/src/discovery/revive.rs, sandbox.rs reclaim, workspace.rs.
//
// PLANT selects one deliberate bug, for witness traces:
//   "none"          the code as written
//   "no_kept"       reclaim deletes a folder that holds overlays
//   "stale_restart" revive restarts on the observation made outside the
//                   guard, without reading the record again
//   "no_guard"      act without holding the guard
//   "leak_guard"    a caller ends without releasing the guard
//   "boot_reclaims" a revive with no saved overlay deletes the folder (the
//                   model before F1; the code answers "ended" instead)
module sandbox_reopen {
  const PLANT: str

  val CALLERS = Set("boot", "mount", "list")

  // The Sandbox folder exists.
  var folder: bool
  // Saved Workspace overlay files in it (0 or 1 is enough to show loss).
  var overlays: int
  // Running daemon incarnations on the folder.
  var live: Set[int]
  // The incarnation the Sandbox record names; 0 is the ended one.
  var recorded: int
  var nextId: int
  // Holder of the project guard, or "".
  var guard: str
  // idle -> observed -> held -> checked -> done
  var phase: str -> str
  // The record each caller saw outside the guard.
  var seen: str -> int
  // What each caller chose under the guard.
  var plan: str -> str
  // Workspaces mounted after a reopen.
  var mounted: int
  // The caller that deleted the folder, or "".
  var deleter: str

  // ---- the pure decision (revive_rules::next) ----
  pure def decide(running: bool, saved: int): str =
    if (running) "use" else if (saved == 0) "reclaim" else "restart"

  action init = all {
    folder' = true,
    nondet o = Set(0, 1).oneOf()
    overlays' = o,
    live' = Set(),
    recorded' = 0,
    nextId' = 1,
    guard' = "",
    phase' = CALLERS.mapBy(_ => "idle"),
    seen' = CALLERS.mapBy(_ => 0),
    plan' = CALLERS.mapBy(_ => "none"),
    mounted' = 0,
    deleter' = "",
  }

  action unchanged_shell = all {
    folder' = folder, overlays' = overlays, live' = live,
    recorded' = recorded, nextId' = nextId, mounted' = mounted,
    deleter' = deleter,
  }

  // Outside any guard: read the record (and so the daemon it names).
  action observe(c: str): bool = all {
    phase.get(c) == "idle",
    seen' = seen.set(c, recorded),
    plan' = plan,
    phase' = phase.set(c, "observed"),
    guard' = guard,
    unchanged_shell,
  }

  action acquire(c: str): bool = all {
    phase.get(c) == "observed",
    guard == "" or PLANT == "no_guard",
    guard' = if (PLANT == "no_guard") guard else c,
    phase' = phase.set(c, "held"),
    seen' = seen,
    plan' = plan,
    unchanged_shell,
  }

  // Under the guard: read the record and the running daemon again and
  // choose what to do. The effect is a later step (a spawn or a delete
  // takes time); only the guard keeps another caller out in between.
  action check(c: str): bool = all {
    phase.get(c) == "held",
    guard == c or PLANT == "no_guard",
    val running = if (PLANT == "stale_restart") live.contains(seen.get(c))
                  else live.contains(recorded)
    // A revive never deletes: with no saved overlay it answers "ended" and
    // keeps the folder (revive.rs, Next::Reclaim, after F1). Only reclaim
    // (a mount or listing, past its Busy and Kept checks) deletes.
    val decided = decide(running, overlays)
    val revived = if (not(folder) or (decided == "reclaim" and PLANT != "boot_reclaims")) "none"
                  else decided
    val seenRunning = live.contains(seen.get(c))
    val chosen =
      if (c == "boot") revived
      else if (seenRunning or not(folder)) "none"
      // reclaim refuses Busy (started again meanwhile) and Kept (overlays)
      else if (recorded != seen.get(c)) "none"
      else if (overlays == 0 or PLANT == "no_kept") "reclaim"
      else if (c == "mount") revived
      else "none"
    all {
      plan' = plan.set(c, chosen),
      phase' = phase.set(c, "checked"),
      guard' = guard, seen' = seen,
      unchanged_shell,
    },
  }

  action effect(c: str): bool = all {
    phase.get(c) == "checked",
    val p = plan.get(c)
    if (p == "restart") all {
      live' = live.union(Set(nextId)),
      recorded' = nextId,
      nextId' = nextId + 1,
      mounted' = overlays,
      folder' = folder, overlays' = overlays,
      deleter' = deleter,
    }
    else if (p == "reclaim") all {
      folder' = false,
      // the overlay files go with the folder; the count stays to show the loss
      overlays' = overlays,
      live' = live, recorded' = recorded, nextId' = nextId, mounted' = mounted,
      deleter' = c,
    }
    else if (p == "use") all {
      mounted' = overlays,
      folder' = folder, overlays' = overlays, live' = live,
      recorded' = recorded, nextId' = nextId, deleter' = deleter,
    }
    else unchanged_shell,
    guard' = if (guard == c and PLANT != "leak_guard") "" else guard,
    phase' = phase.set(c, "done"),
    seen' = seen,
    plan' = plan,
  }

  action step = {
    nondet c = CALLERS.oneOf()
    any { observe(c), acquire(c), check(c), effect(c) }
  }

  action stutter = all {
    CALLERS.forall(c => phase.get(c) == "done"),
    folder' = folder, overlays' = overlays, live' = live, recorded' = recorded,
    nextId' = nextId, guard' = guard, phase' = phase, seen' = seen, plan' = plan,
    mounted' = mounted, deleter' = deleter,
  }

  action stepOrDone = any { step, stutter }

  // ---- properties ----
  // Saved overlays are never deleted.
  val NoOverlayLoss = overlays > 0 implies folder
  // At most one daemon serves the folder.
  val OneDaemon = live.size() <= 1
  // A folder with saved overlays ends with them mounted once every caller is done.
  val ReopenedWhenDone =
    (CALLERS.forall(c => phase.get(c) == "done") and overlays > 0) implies mounted == overlays
  // The boot reopen never deletes a Sandbox folder (REOPEN.md: "ended").
  val BootNeverDeletes = deleter != "boot"
  val Safety = NoOverlayLoss and OneDaemon and ReopenedWhenDone and BootNeverDeletes

  // Liveness of the only wait (the project guard): with every enabled step
  // taken eventually, every caller ends.
  temporal AllEnd =
    weakFair(step, (folder, overlays, live, recorded, nextId, guard, phase, seen, plan, mounted, deleter))
      implies eventually(CALLERS.forall(c => phase.get(c) == "done"))
}
