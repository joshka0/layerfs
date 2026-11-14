// One process Sandbox folder after a restart: the daemon its record names
// has ended, and the folder may hold saved Workspace overlays. Three
// callers race on it, each its own process:
//
// - boot (operator.sandbox.reopen, revive.rs reopen_all): takes the project
//   guard first, then visits the Sandbox only while its record exists, and
//   revives it: use a running daemon, restart an ended one that holds
//   saved overlays, or answer "ended" (no saved overlay; the folder stays).
// - mount (workspace.rs place): takes the project guard first, then observes
//   the daemon; a running one is used; an ended one is reclaimed unless
//   reclaim refuses it Kept (saved overlays), and then it is revived.
// - list (workspace.rs list): observes the record and the daemon with no
//   guard. A running daemon or an absent record ends the call there. An
//   ended daemon is reclaimed under the project guard; reclaim reads the
//   record again (Busy) and refuses saved overlays (Kept).
//
// A check under the guard and its effect (a spawn, a delete) are separate
// steps: only the guard keeps another caller out in between.
//
// PLANT selects one deliberate bug, for witness traces:
//   "none"             the code as written
//   "no_kept"          reclaim deletes a folder that holds overlays
//   "no_running_check" revive restarts without checking that the saved
//                      daemon runs
//   "no_guard"         callers act without holding the guard
//   "leak_guard"       a caller ends without releasing the guard
//   "boot_reclaims"    a revive with no saved overlay deletes the folder
//                      (the model before F1; the code answers "ended")
module sandbox_reopen {
  const PLANT: str

  val CALLERS = Set("boot", "mount", "list")

  // The Sandbox folder and its record exist.
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
  // idle -> [observed ->] held -> checked -> done
  var phase: str -> str
  // The record list saw outside the guard.
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

  // list only, with no guard: read the record and whether its daemon runs.
  // A running daemon or an absent record ends the call.
  action observe(c: str): bool = all {
    c == "list",
    phase.get(c) == "idle",
    val ends = not(folder) or live.contains(recorded)
    all {
      seen' = seen.set(c, recorded),
      phase' = phase.set(c, if (ends) "done" else "observed"),
    },
    plan' = plan,
    guard' = guard,
    unchanged_shell,
  }

  // boot and mount take the guard first; list only after it saw an ended daemon.
  action acquire(c: str): bool = all {
    if (c == "list") phase.get(c) == "observed" else phase.get(c) == "idle",
    guard == "" or PLANT == "no_guard",
    guard' = if (PLANT == "no_guard") guard else c,
    phase' = phase.set(c, "held"),
    seen' = seen,
    plan' = plan,
    unchanged_shell,
  }

  // What a revive does with the Sandbox as it is now.
  def revived: str = {
    val running = live.contains(recorded) and PLANT != "no_running_check"
    val decided = decide(running, overlays)
    // "ended": a revive never deletes (revive.rs Next::Reclaim, after F1)
    if (decided == "reclaim" and PLANT != "boot_reclaims") "none" else decided
  }

  // Under the guard: read the Sandbox again and choose. A boot or mount
  // that finds no record has no Sandbox to visit: it ends here.
  action check(c: str): bool = all {
    phase.get(c) == "held",
    guard == c or PLANT == "no_guard",
    if (not(folder) and c != "list") all {
      plan' = plan.set(c, "none"),
      phase' = phase.set(c, "done"),
      guard' = if (guard == c and PLANT != "leak_guard") "" else guard,
    } else {
      val chosen =
        if (c == "boot") revived
        else if (c == "mount")
          if (live.contains(recorded)) "none"
          // reclaim, unless Kept; then the mount revives it
          else if (overlays == 0 or PLANT == "no_kept") "reclaim"
          else revived
        // list: reclaim reads the record again (Busy) and refuses Kept
        else if (not(folder) or recorded != seen.get(c)) "none"
        else if (overlays == 0 or PLANT == "no_kept") "reclaim"
        else "none"
      all {
        plan' = plan.set(c, chosen),
        phase' = phase.set(c, "checked"),
        guard' = guard,
      }
    },
    seen' = seen,
    unchanged_shell,
  }

  action effect(c: str): bool = all {
    phase.get(c) == "checked",
    val p = plan.get(c)
    if (p == "restart") all {
      live' = live.union(Set(nextId)),
      recorded' = nextId,
      nextId' = nextId + 1,
      mounted' = overlays,
      folder' = folder, overlays' = overlays, deleter' = deleter,
    }
    else if (p == "reclaim") all {
      folder' = false,
      // the overlay files go with the folder; the count stays to show the loss
      overlays' = overlays,
      live' = live, recorded' = recorded, nextId' = nextId, mounted' = mounted,
      deleter' = c,
    }
    // a revive of a running daemon reopens the Workspaces it lacks
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
