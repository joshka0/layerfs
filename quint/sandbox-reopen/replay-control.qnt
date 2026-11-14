// Positive control: restart's independent running check still refuses.
module replay_control {
  import sandbox_reopen(PLANT = "no_revive_check") as M from "./sandbox_reopen"
  action step = {
    nondet c = Set("boot", "mount", "list", "status").oneOf()
    any { M::observe(c), M::acquire(c), M::check(c), M::effect(c), M::admit(c), M::finish(c) }
  }
  action stepOrDone = any { step, M::stutter }
}
