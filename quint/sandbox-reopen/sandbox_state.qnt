// The state of the Sandbox reopen model (sandbox_reopen.qnt): its variables,
// the pure revive decision, the initial state, and the frame helpers that
// keep groups of variables unchanged. No behavior depends on PLANT here.
module sandbox_state {
  val CALLERS = Set("boot", "mount", "list", "status")

  // The Sandbox folder and its record exist.
  var folder: bool
  // Saved Workspace overlay files in it.
  var overlays: int
  // How many of them can reattach: each record holds its base root, or the
  // caller passed legacyHead, and the daemon accepts its overlay. Fixed.
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
}
