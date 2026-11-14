// Replay schedules use the model's actions. Guard timeout is checked separately
// against the unchanged real 60-second deadline, one lock per invocation.
module replay {
  import sandbox_reopen(PLANT = "none") as M from "./sandbox_reopen"
  action step = {
    nondet c = Set("boot", "mount", "list", "status").oneOf()
    any { M::observe(c), M::acquire(c), M::check(c), M::effect(c), M::admit(c), M::finish(c) }
  }
  action stepOrDone = any { step, M::stutter }
}
