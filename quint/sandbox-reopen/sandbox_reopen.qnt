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
//   record first and ends NoSuchSandbox when another call reclaimed it
//   (sandbox/reclaim.rs), then refuses Busy and Kept.
//
// The guard wait is bounded (discovery/lock.rs: 60 s, then Busy): a caller
// that cannot get the guard ends refused. A check under the guard and its
// effect are separate steps. A restart is two steps: the spawn (refused by
// process.rs while the saved daemon runs), then the handshake and record
// save, which can fail (revive.rs restart_daemon); a failure stops the
// spawned daemon before its record is saved (fix-revive-failed-restart).
//
// PLANT selects one deliberate bug, for witness traces:
//   "none"             the code as written
//   "no_kept"          reclaim deletes a folder that holds overlays
//   "no_running_check" both running checks are removed: revive's
//                      (process::Sandbox::reopen) and restart's own
//                      (process.rs, "the saved daemon still runs")
//   "no_revive_check"  only revive's running check is removed; restart's own
//                      check still refuses (expected to hold: two defenses)
//   "no_guard"         callers act without holding the guard
//   "leak_guard"       a caller ends without releasing the guard
//   "boot_reclaims"    a revive with no saved overlay deletes the folder
//                      (the model before F1; the code answers "ended")
//   "keep_failed_daemon" a failed restart leaves its daemon running with the
//                      record unchanged (the code before 2f254db3c)
module sandbox_reopen {
  const PLANT: str

  val CALLERS = Set("boot", "mount", "list")

  // The Sandbox folder and its record exist.
  var folder: bool
  // Saved Workspace overlay files in it (0 or 1 is enough to show loss).
  var overlays: int
  // The saved records can reattach: each holds its base root, or the
  // caller passed legacyHead (revive_rules::base). Fixed per run.
  var eligible: bool
  // Running daemon incarnations on the folder.
  var live: Set[int]
  // The incarnation the Sandbox record names; 0 is the ended one.
  var recorded: int
  var nextId: int
  // Holder of the project guard, or "".
  var guard: str
  // idle -> [observed ->] held -> checked -> [spawned ->] done
  var phase: str -> str
  // The record list saw outside the guard.
  var seen: str -> int
  // What each caller chose under the guard.
  var plan: str -> str
  // The daemon a caller spawned and has not yet recorded.
  var spawned: str -> int
  // How each caller ended: "", "ok", "busy" (guard wait expired), "failed".
  var result: str -> str
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
    nondet e = Set(true, false).oneOf()
    eligible' = e,
    live' = Set(),
    recorded' = 0,
    nextId' = 1,
    guard' = "",
    phase' = CALLERS.mapBy(_ => "idle"),
    seen' = CALLERS.mapBy(_ => 0),
    plan' = CALLERS.mapBy(_ => "none"),
    spawned' = CALLERS.mapBy(_ => 0),
    result' = CALLERS.mapBy(_ => ""),
    mounted' = 0,
    deleter' = "",
  }

  action unchanged_shell = all {
    folder' = folder, overlays' = overlays, eligible' = eligible, live' = live,
    recorded' = recorded, nextId' = nextId, mounted' = mounted,
    deleter' = deleter,
  }
  action unchanged_callers = all {
    seen' = seen, plan' = plan, spawned' = spawned,
  }

  def released(c: str): str = if (guard == c and PLANT != "leak_guard") "" else guard

  // list only, with no guard: read the record and whether its daemon runs.
  // A running daemon or an absent record ends the call.
  action observe(c: str): bool = all {
    c == "list",
    phase.get(c) == "idle",
    val ends = not(folder) or live.contains(recorded)
    all {
      seen' = seen.set(c, recorded),
      phase' = phase.set(c, if (ends) "done" else "observed"),
      result' = if (ends) result.set(c, "ok") else result,
    },
    plan' = plan, spawned' = spawned,
    guard' = guard,
    unchanged_shell,
  }

  def waiting(c: str): bool =
    if (c == "list") phase.get(c) == "observed" else phase.get(c) == "idle"

  // boot and mount take the guard first; list only after it saw an ended daemon.
  action acquire(c: str): bool = all {
    waiting(c),
    guard == "" or PLANT == "no_guard",
    guard' = if (PLANT == "no_guard") guard else c,
    phase' = phase.set(c, "held"),
    result' = result,
    unchanged_callers,
    unchanged_shell,
  }

  // The bounded wait expires: the caller ends refused Busy.
  action timeout(c: str): bool = all {
    waiting(c),
    PLANT != "no_guard",
    guard != "" and guard != c,
    phase' = phase.set(c, "done"),
    result' = result.set(c, "busy"),
    guard' = guard,
    unchanged_callers,
    unchanged_shell,
  }

  // What a revive does with the Sandbox as it is now.
  def revived: str = {
    val running = live.contains(recorded)
      and PLANT != "no_running_check" and PLANT != "no_revive_check"
    val decided = decide(running, overlays)
    // "ended": a revive never deletes (revive.rs Next::Reclaim, after F1)
    if (decided == "reclaim" and PLANT != "boot_reclaims") "none" else decided
  }

  // Under the guard: read the Sandbox again and choose. A caller that finds
  // no record has no Sandbox to visit: it ends here (for list, reclaim's
  // NoSuchSandbox, which the listing takes as gone).
  action check(c: str): bool = all {
    phase.get(c) == "held",
    guard == c or PLANT == "no_guard",
    if (not(folder)) all {
      plan' = plan.set(c, "none"),
      phase' = phase.set(c, "done"),
      result' = result.set(c, "ok"),
      guard' = released(c),
    } else {
      val chosen =
        if (c == "boot") revived
        else if (c == "mount")
          if (live.contains(recorded)) "none"
          // reclaim, unless Kept; then the mount revives it
          else if (overlays == 0 or PLANT == "no_kept") "reclaim"
          else revived
        // list: reclaim refuses Busy (started again meanwhile) and Kept
        else if (recorded != seen.get(c)) "none"
        else if (overlays == 0 or PLANT == "no_kept") "reclaim"
        else "none"
      all {
        plan' = plan.set(c, chosen),
        phase' = phase.set(c, "checked"),
        result' = result,
        guard' = guard,
      }
    },
    seen' = seen, spawned' = spawned,
    unchanged_shell,
  }

  // Mount the saved Workspaces on a daemon that serves the folder; records
  // that cannot reattach stay unmounted.
  def reattached: int = if (eligible) overlays else mounted

  action effect(c: str): bool = all {
    phase.get(c) == "checked",
    val p = plan.get(c)
    if (p == "restart")
      // process.rs restart: refused while the saved daemon still runs
      if (live.contains(recorded) and PLANT != "no_running_check") all {
        phase' = phase.set(c, "done"),
        result' = result.set(c, "failed"),
        guard' = released(c),
        spawned' = spawned,
        unchanged_shell,
      } else all {
        live' = live.union(Set(nextId)),
        spawned' = spawned.set(c, nextId),
        nextId' = nextId + 1,
        phase' = phase.set(c, "spawned"),
        result' = result,
        guard' = guard,
        folder' = folder, overlays' = overlays, eligible' = eligible,
        recorded' = recorded, mounted' = mounted, deleter' = deleter,
      }
    else all {
      if (p == "reclaim") all {
        folder' = false,
        // the overlay files go with the folder; the count stays to show the loss
        overlays' = overlays,
        eligible' = eligible, live' = live, recorded' = recorded, nextId' = nextId,
        mounted' = mounted,
        deleter' = c,
      }
      // a revive of a running daemon reopens the Workspaces it lacks
      else if (p == "use") all {
        mounted' = reattached,
        folder' = folder, overlays' = overlays, eligible' = eligible, live' = live,
        recorded' = recorded, nextId' = nextId, deleter' = deleter,
      }
      else unchanged_shell,
      phase' = phase.set(c, "done"),
      result' = result.set(c, "ok"),
      guard' = released(c),
      spawned' = spawned,
    },
    seen' = seen, plan' = plan,
  }

  // After the spawn: the handshake and record save succeed (the record names
  // the new daemon, the Workspaces reattach), or one fails, and the spawned
  // daemon is stopped; the record is unchanged.
  action finish(c: str): bool = all {
    phase.get(c) == "spawned",
    val id = spawned.get(c)
    nondet ok = Set(true, false).oneOf()
    if (ok) all {
      recorded' = id,
      mounted' = reattached,
      result' = result.set(c, "ok"),
      live' = live,
    } else all {
      recorded' = recorded,
      mounted' = mounted,
      result' = result.set(c, "failed"),
      live' = if (PLANT == "keep_failed_daemon") live else live.exclude(Set(id)),
    },
    folder' = folder, overlays' = overlays, eligible' = eligible,
    nextId' = nextId, deleter' = deleter,
    phase' = phase.set(c, "done"),
    guard' = released(c),
    spawned' = spawned.set(c, 0),
    seen' = seen, plan' = plan,
  }

  action step = {
    nondet c = CALLERS.oneOf()
    any { observe(c), acquire(c), timeout(c), check(c), effect(c), finish(c) }
  }

  action stutter = all {
    CALLERS.forall(c => phase.get(c) == "done"),
    guard' = guard, phase' = phase, result' = result,
    unchanged_callers,
    unchanged_shell,
  }

  action stepOrDone = any { step, stutter }

  // ---- properties ----
  // Saved overlays are never deleted.
  val NoOverlayLoss = overlays > 0 implies folder
  // At most one daemon serves the folder.
  val OneDaemon = live.size() <= 1
  // Every running daemon is the one the record names, once no restart is
  // between its spawn and its record save.
  val NoOrphanDaemon =
    CALLERS.forall(c => phase.get(c) != "spawned") implies live.subseteq(Set(recorded))
  // When every caller ended and each succeeded, a folder with saved overlays
  // that can reattach has them mounted. (A failed or Busy caller, or a
  // record without a base, is outside this claim; see eligible.)
  val ReopenedWhenDone =
    (CALLERS.forall(c => phase.get(c) == "done" and result.get(c) == "ok")
      and overlays > 0 and eligible) implies mounted == overlays
  // The boot reopen never deletes a Sandbox folder (REOPEN.md: "ended").
  val BootNeverDeletes = deleter != "boot"
  val Safety = NoOverlayLoss and OneDaemon and NoOrphanDaemon and ReopenedWhenDone
    and BootNeverDeletes

  val vars = (folder, overlays, eligible, live, recorded, nextId, guard, phase, seen, plan,
    spawned, result, mounted, deleter)

  // Every caller ends (it may end refused Busy: the wait is bounded).
  temporal AllEnd =
    weakFair(step, vars) implies eventually(CALLERS.forall(c => phase.get(c) == "done"))
  // The guard is not held forever: completion is not enough when the
  // others only end because their wait expired.
  temporal GuardReleased =
    weakFair(step, vars) implies eventually(always(guard == ""))
}
