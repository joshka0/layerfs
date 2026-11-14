// The state of the Sandbox reopen model (sandbox_reopen.qnt): the plant
// selector, its variables, the pure revive decision, the initial state, the
// frame helpers that keep groups of variables unchanged, and the derived
// decisions the actions share.
module sandbox_state {
  // The deliberate bug of a plant instance (sandbox_reopen.qnt lists them).
  const PLANT: str

  val CALLERS = Set("boot", "mount", "list", "status")

  // The Sandbox folder and its record exist.
  var folder: bool
  // Saved Workspace overlay files in it.
  var overlays: int
  // How many of them can reattach: each record holds its base root and the
  // daemon accepts its overlay. Fixed.
  var eligible: int
  // Admitted daemons: running incarnations that hold the folder's flock
  // (application/folder.rs) and serve it.
  var live: Set[int]
  // Attempted daemons: spawned processes before their flock check. One
  // that finds the lock held exits (`admit`).
  var children: Set[int]
  // The incarnation the Sandbox record names; 0 is the ended one.
  var recorded: int
  var nextId: int
  // Holder of the project guard, or "".
  var guard: str
  // idle -> [observed ->] held -> checked -> [spawned -> admitted ->] done
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
  // A status call answered from a daemon that is unusable.
  var answeredUnusable: bool
  // Callers whose restart was refused by the folder lock, and those whose
  // answer named the holder's pid.
  var heldRefused: Set[str]
  var namedHolder: Set[str]
  // Running daemons that answer no usable Hello: a restart spawned them,
  // failed, and could not stop them (finish "stop_failed").
  var unusable: Set[int]
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
    children' = Set(),
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
    answeredUnusable' = false,
    heldRefused' = Set(),
    namedHolder' = Set(),
    unusable' = Set(),
    mounted' = 0,
    deleter' = "",
  }

  action unchanged_shell = all {
    folder' = folder, overlays' = overlays, eligible' = eligible, live' = live,
    recorded' = recorded, nextId' = nextId, mounted' = mounted,
    deleter' = deleter, unusable' = unusable, children' = children,
  }
  action unchanged_callers = all {
    seen' = seen, plan' = plan, spawned' = spawned,
  }
  action unchanged_reports = all {
    target' = target, reported' = reported, goneMidRestart' = goneMidRestart,
    answeredUnusable' = answeredUnusable,
    heldRefused' = heldRefused, namedHolder' = namedHolder,
  }

  def released(c: str): str = if (guard == c and PLANT != "leak_guard") "" else guard

  // A restart of this folder is between its check and its finish.
  def restarting: bool = CALLERS.exists(c =>
    plan.get(c) == "restart" and Set("checked", "spawned", "admitted").contains(phase.get(c)))

  def waiting(c: str): bool =
    if (Set("list", "status").contains(c)) phase.get(c) == "observed" else phase.get(c) == "idle"

  // What a revive does with the Sandbox as it is now.
  def revived: str = {
    val running = live.contains(recorded)
      and PLANT != "no_running_check" and PLANT != "no_revive_check"
    val decided = decide(running, overlays)
    // "ended": a revive never deletes (revive.rs Next::Reclaim, after F1)
    if (decided == "reclaim" and PLANT != "boot_reclaims") "none" else decided
  }

  // Each saved Workspace that can reattach is mounted on a daemon that
  // serves the folder; the others stay unmounted, each with its reason.
  def reattached: int = if (PLANT == "count_unattached") overlays else eligible

  // The end of a revive that failed: boot answers the Sandbox "failed";
  // mount places its Workspace elsewhere, which succeeds (`placed`) or not;
  // its answer or its refusal carries `revived` (779d0447e).
  def failedResult(c: str, placed: bool): str =
    if (c == "mount" and placed) "ok" else "failed"
  def reportedAfter(c: str): bool =
    reported or (c == "mount" and PLANT != "no_revived_report")
}
