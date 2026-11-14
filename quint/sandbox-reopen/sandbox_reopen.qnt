// One process Sandbox folder after a restart: the daemon its record names
// has ended, and the folder may hold saved Workspace overlays. Four
// callers race on it, each its own process:
//
// - boot (operator.sandbox.reopen, revive.rs reopen_all): takes the project
//   guard first, then visits the Sandbox only while its record exists, and
//   revives it: use a running daemon, restart an ended one that holds
//   saved overlays, or answer "ended" (no saved overlay; the folder stays).
// - mount (workspace.rs place): takes the project guard first, then observes
//   the daemon; a running one is used; an ended one is reclaimed unless
//   reclaim refuses it Kept (saved overlays), and then it is revived. If
//   that revive fails, the mount places its Workspace in another Sandbox
//   (outside this folder) and reports the failed revive in `revived`.
// - list (workspace.rs list): observes the record and the daemon with no
//   guard. A running daemon or an absent record ends the call there. An
//   ended daemon is reclaimed under the project guard; reclaim reads the
//   record first and ends NoSuchSandbox when another call reclaimed it
//   (sandbox/reclaim.rs), then refuses Busy and Kept. A guard wait that
//   expires lists the Sandbox (it may be restarting).
// - status (a Workspace call, registry.rs sandbox): observes the daemon
//   with no guard. A running daemon answers. An ended one waits for the
//   project guard, then reads the record again: a running daemon answers,
//   else the Sandbox is gone (NoSuchSandbox). A wait that expires refuses
//   Busy "restarting".
//
// The guard wait is bounded (discovery/lock.rs: 60 s, then Busy). A check
// under the guard and its effect are separate steps. A restart is two
// steps: the spawn (refused by process.rs while the saved daemon runs),
// then the handshake, record save and the steps after it (revive.rs
// restart_daemon), which end in one of four outcomes; see finish.
//
// Each saved Workspace reattaches or not on its own (a moved branch root,
// an overlay shape the daemon refuses): `eligible` of the `overlays` can.
// A call's own result is separate from the reattach outcome of each
// Workspace and, for mount, from the outcome of its target revive.
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
//   "no_revived_report" a mount whose target revive failed answers without
//                      `revived` (the code before d08e3b656)
//   "status_no_wait"   status calls an ended daemon gone at once, without
//                      the guard (the code before d08e3b656)
//   "no_fallback"      a mount whose target revive failed fails too
//   "count_unattached" a Workspace that cannot reattach counts as mounted
module sandbox_reopen {
  const PLANT: str

  val CALLERS = Set("boot", "mount", "list", "status")

  // The Sandbox folder and its record exist.
  var folder: bool
  // Saved Workspace overlay files in it.
  var overlays: int
  // How many of them can reattach: each record holds its base root, or the
  // caller passed legacyHead, and the daemon accepts its overlay. Fixed.
  var eligible: int
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
  // The daemon a caller spawned and has not yet finished.
  var spawned: str -> int
  // How each call ended: "", "ok", "busy" (guard wait expired), "failed",
  // "gone" (status: NoSuchSandbox).
  var result: str -> str
  // The outcome of the revive a caller ran on this folder: "", "ok",
  // "failed". For mount it differs from its result: a failed target
  // revive still ends the mount "ok" in another Sandbox.
  var target: str -> str
  // The mount answer's `revived` names this Sandbox.
  var reported: bool
  // A status call answered "gone" while a restart of the Sandbox was under
  // way (between its check and its finish).
  var goneMidRestart: bool
  // Workspaces mounted after a reopen.
  var mounted: int
  // The caller that deleted the folder, or "".
  var deleter: str

  // ---- the pure decision (revive_rules::next) ----
  pure def decide(running: bool, saved: int): str =
    if (running) "use" else if (saved == 0) "reclaim" else "restart"

  action init = all {
    folder' = true,
    nondet o = Set(0, 1, 2).oneOf()
    nondet e = Set(0, 1, 2).filter(n => n <= o).oneOf()
    all { overlays' = o, eligible' = e },
    live' = Set(),
    recorded' = 0,
    nextId' = 1,
    guard' = "",
    phase' = CALLERS.mapBy(_ => "idle"),
    seen' = CALLERS.mapBy(_ => 0),
    plan' = CALLERS.mapBy(_ => "none"),
    spawned' = CALLERS.mapBy(_ => 0),
    result' = CALLERS.mapBy(_ => ""),
    target' = CALLERS.mapBy(_ => ""),
    reported' = false,
    goneMidRestart' = false,
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
  action unchanged_reports = all {
    target' = target, reported' = reported, goneMidRestart' = goneMidRestart,
  }

  def released(c: str): str = if (guard == c and PLANT != "leak_guard") "" else guard

  // A restart of this folder is between its check and its finish.
  def restarting: bool = CALLERS.exists(c =>
    plan.get(c) == "restart" and (phase.get(c) == "checked" or phase.get(c) == "spawned"))

  // list and status, with no guard: read the record and whether its daemon
  // runs. list ends on a running daemon or an absent record; status ends on
  // a running daemon (it answers), and with status_no_wait also on an ended
  // one (gone).
  action observe(c: str): bool = all {
    Set("list", "status").contains(c),
    phase.get(c) == "idle",
    val running = folder and live.contains(recorded)
    val gone = c == "status" and not(running) and PLANT == "status_no_wait"
    val ends = running or gone or (c == "list" and not(folder))
    all {
      seen' = seen.set(c, recorded),
      phase' = phase.set(c, if (ends) "done" else "observed"),
      result' = if (gone) result.set(c, "gone") else if (ends) result.set(c, "ok") else result,
      goneMidRestart' = (goneMidRestart or (gone and restarting)),
    },
    plan' = plan, spawned' = spawned,
    guard' = guard,
    target' = target, reported' = reported,
    unchanged_shell,
  }

  def waiting(c: str): bool =
    if (Set("list", "status").contains(c)) phase.get(c) == "observed" else phase.get(c) == "idle"

  // boot and mount take the guard first; list and status only after they
  // saw an ended daemon.
  action acquire(c: str): bool = all {
    waiting(c),
    guard == "" or PLANT == "no_guard",
    guard' = if (PLANT == "no_guard") guard else c,
    phase' = phase.set(c, "held"),
    result' = result,
    unchanged_callers,
    unchanged_reports,
    unchanged_shell,
  }

  // The bounded wait expires. list lists the Sandbox; every other caller
  // ends refused Busy (status: "Sandbox <id> is restarting").
  action timeout(c: str): bool = all {
    waiting(c),
    PLANT != "no_guard",
    guard != "" and guard != c,
    phase' = phase.set(c, "done"),
    result' = result.set(c, if (c == "list") "ok" else "busy"),
    guard' = guard,
    unchanged_callers,
    unchanged_reports,
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
  // NoSuchSandbox, which the listing takes as gone; for status, gone).
  action check(c: str): bool = all {
    phase.get(c) == "held",
    guard == c or PLANT == "no_guard",
    if (not(folder) or c == "status") {
      val answers = c == "status" and folder and live.contains(recorded)
      val gone = c == "status" and not(answers)
      all {
        plan' = plan.set(c, "none"),
        phase' = phase.set(c, "done"),
        result' = result.set(c, if (gone) "gone" else "ok"),
        guard' = released(c),
        goneMidRestart' = (goneMidRestart or (gone and restarting)),
      }
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
        goneMidRestart' = goneMidRestart,
      }
    },
    seen' = seen, spawned' = spawned,
    target' = target, reported' = reported,
    unchanged_shell,
  }

  // Each saved Workspace that can reattach is mounted on a daemon that
  // serves the folder; the others stay unmounted, each with its reason.
  def reattached: int = if (PLANT == "count_unattached") overlays else eligible

  // The end of a revive that failed: boot answers the Sandbox "failed";
  // mount places its Workspace elsewhere and reports the failed revive.
  def failedResult(c: str): str =
    if (c == "mount" and PLANT != "no_fallback") "ok" else "failed"
  def reportedAfter(c: str): bool =
    reported or (c == "mount" and PLANT != "no_revived_report")

  action effect(c: str): bool = all {
    phase.get(c) == "checked",
    val p = plan.get(c)
    if (p == "restart")
      // process.rs restart: refused while the saved daemon still runs
      if (live.contains(recorded) and PLANT != "no_running_check") all {
        phase' = phase.set(c, "done"),
        result' = result.set(c, failedResult(c)),
        target' = target.set(c, "failed"),
        reported' = reportedAfter(c),
        guard' = released(c),
        spawned' = spawned,
        unchanged_shell,
      } else all {
        live' = live.union(Set(nextId)),
        spawned' = spawned.set(c, nextId),
        nextId' = nextId + 1,
        phase' = phase.set(c, "spawned"),
        result' = result,
        target' = target, reported' = reported,
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
      target' = if (p == "use") target.set(c, "ok") else target,
      reported' = reported,
      guard' = released(c),
      spawned' = spawned,
    },
    seen' = seen, plan' = plan,
    goneMidRestart' = goneMidRestart,
  }

  // After the spawn, revive.rs restart_daemon ends in one of four outcomes.
  // Every failure stops the spawned daemon (stop_spawned); where the record
  // stands depends on when the failure came:
  // - "ok": handshake and record save succeed; the record names the new
  //   daemon and the Workspaces reattach;
  // - "stopped": the handshake fails or never sees the listen line; the
  //   daemon is stopped and the record is unchanged;
  // - "after_record": the record save succeeded and a later step (the
  //   container file rename, the registry) fails; the daemon is stopped
  //   and the record names it (replay of d08e3b656, binder seed 42);
  // - "stop_failed": a failure whose stop fails; the record is made to name
  //   the still-running daemon (3dd211bd3).
  action finish(c: str): bool = all {
    phase.get(c) == "spawned",
    val id = spawned.get(c)
    nondet outcome = Set("ok", "stopped", "stop_failed", "after_record").oneOf()
    if (outcome == "ok") all {
      recorded' = id,
      mounted' = reattached,
      result' = result.set(c, "ok"),
      target' = target.set(c, "ok"),
      reported' = reported,
      live' = live,
    } else all {
      recorded' = if (outcome == "stopped") recorded else id,
      mounted' = mounted,
      result' = result.set(c, failedResult(c)),
      target' = target.set(c, "failed"),
      reported' = reportedAfter(c),
      live' =
        if (outcome == "stop_failed" or PLANT == "keep_failed_daemon") live
        else live.exclude(Set(id)),
    },
    folder' = folder, overlays' = overlays, eligible' = eligible,
    nextId' = nextId, deleter' = deleter,
    phase' = phase.set(c, "done"),
    guard' = released(c),
    spawned' = spawned.set(c, 0),
    seen' = seen, plan' = plan,
    goneMidRestart' = goneMidRestart,
  }

  action step = {
    nondet c = CALLERS.oneOf()
    any { observe(c), acquire(c), timeout(c), check(c), effect(c), finish(c) }
  }

  action stutter = all {
    CALLERS.forall(c => phase.get(c) == "done"),
    guard' = guard, phase' = phase, result' = result,
    unchanged_callers,
    unchanged_reports,
    unchanged_shell,
  }

  action stepOrDone = any { step, stutter }

  // ---- properties ----
  // Saved overlays are never deleted.
  val NoOverlayLoss = overlays > 0 implies folder
  // At most one daemon serves the folder.
  val OneDaemon = live.size() <= 1
  // Every running daemon is the one the record names, once no restart is
  // between its spawn and its finish.
  val NoOrphanDaemon =
    CALLERS.forall(c => phase.get(c) != "spawned") implies live.subseteq(Set(recorded))
  // When every caller ended, boot and mount succeeded, and no revive of
  // this folder failed, each saved Workspace that can reattach is mounted.
  // Those that cannot stay unmounted although the call succeeds.
  val ReopenedWhenDone =
    (CALLERS.forall(c => phase.get(c) == "done" and target.get(c) != "failed")
      and Set("boot", "mount").forall(c => result.get(c) == "ok"))
      implies mounted == eligible
  // The boot reopen never deletes a Sandbox folder (REOPEN.md: "ended").
  val BootNeverDeletes = deleter != "boot"
  // A mount whose target revive failed names it in `revived`.
  val RevivedReported = target.get("mount") == "failed" implies reported
  // A mount ends with success whatever its target revive did (fallback).
  val MountFallsBack =
    (phase.get("mount") == "done" and target.get("mount") == "failed")
      implies result.get("mount") == "ok"
  // A status call never calls a restarting Sandbox gone.
  val StatusWaitsOutRestart = not(goneMidRestart)
  val Safety = NoOverlayLoss and OneDaemon and NoOrphanDaemon and ReopenedWhenDone
    and BootNeverDeletes and RevivedReported and MountFallsBack
    and StatusWaitsOutRestart

  val vars = (folder, overlays, eligible, live, recorded, nextId, guard, phase, seen, plan,
    spawned, result, target, reported, goneMidRestart, mounted, deleter)

  // Every caller ends (it may end refused Busy: the wait is bounded).
  temporal AllEnd =
    weakFair(step, vars) implies eventually(CALLERS.forall(c => phase.get(c) == "done"))
  // The guard is not held forever: completion is not enough when the
  // others only end because their wait expired.
  temporal GuardReleased =
    weakFair(step, vars) implies eventually(always(guard == ""))
}
