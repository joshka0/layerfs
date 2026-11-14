//! F46 (core/docs/issues/373/REOPEN.md), written by the independent test
//! author from one contract line before reading the unit's body: a stop whose
//! "still running" probe never reports the process gone returns a timeout
//! error once the 60 s deadline passes, on an injected clock, and never waits
//! past it. Only the public signature of `stop_rules::wait_gone` is used.
//!
//! Every test caps its pauses, so a wait without a deadline fails here
//! instead of hanging.
use layerfs_sandbox::backend::stop_rules::{wait_gone, StillRunning, KILL_WAIT};
use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

const CAP: usize = 100_000;

/// An injected clock that moves only when the wait pauses.
struct Clock {
    start: Instant,
    at: Cell<Duration>,
    step: Duration,
    pauses_at: RefCell<Vec<Duration>>,
}

impl Clock {
    fn new(step: Duration) -> Self {
        Self { start: Instant::now(), at: Cell::new(Duration::ZERO), step, pauses_at: RefCell::new(Vec::new()) }
    }
    fn now(&self) -> Instant {
        self.start + self.at.get()
    }
    fn pause(&self) {
        let mut pauses = self.pauses_at.borrow_mut();
        assert!(pauses.len() < CAP, "the wait paused {CAP} times: it has no deadline");
        pauses.push(self.at.get());
        self.at.set(self.at.get() + self.step);
    }
}

#[test]
fn the_bound_is_60_seconds() {
    assert_eq!(KILL_WAIT, Duration::from_secs(60));
}

#[test]
fn a_process_that_never_ends_times_out_at_the_deadline_and_not_before() {
    for step in [Duration::from_millis(100), Duration::from_secs(1), Duration::from_secs(7), Duration::from_secs(59)] {
        let clock = Clock::new(step);
        let deadline = clock.now() + KILL_WAIT;
        let probes = Cell::new(0usize);
        let result = wait_gone(
            || {
                probes.set(probes.get() + 1);
                true
            },
            || clock.now(),
            || clock.pause(),
            deadline,
        );
        assert_eq!(result, Err(StillRunning), "step {step:?}");
        assert!(clock.now() >= deadline, "step {step:?}: gave up at {:?}, before the 60 s deadline", clock.at.get());
        let pauses = clock.pauses_at.borrow();
        assert!(
            pauses.iter().all(|at| *at < KILL_WAIT),
            "step {step:?}: paused at or after the deadline: {:?}",
            pauses.iter().filter(|at| **at >= KILL_WAIT).collect::<Vec<_>>()
        );
        assert!(probes.get() >= 1, "step {step:?}: never probed");
    }
}

#[test]
fn a_deadline_already_passed_with_the_process_running_times_out_without_a_pause() {
    let clock = Clock::new(Duration::from_secs(1));
    let deadline = clock.now();
    clock.at.set(Duration::from_secs(5));
    let result = wait_gone(|| true, || clock.now(), || clock.pause(), deadline);
    assert_eq!(result, Err(StillRunning));
    assert!(clock.pauses_at.borrow().is_empty(), "paused after the deadline");
}

#[test]
fn a_process_that_ends_is_ok_and_the_wait_stops_at_once() {
    for gone_after in [0usize, 1, 30] {
        let clock = Clock::new(Duration::from_secs(1));
        let deadline = clock.now() + KILL_WAIT;
        let probes = Cell::new(0usize);
        let result = wait_gone(
            || {
                probes.set(probes.get() + 1);
                probes.get() <= gone_after
            },
            || clock.now(),
            || clock.pause(),
            deadline,
        );
        assert_eq!(result, Ok(()), "gone after {gone_after} running probes");
        assert_eq!(probes.get(), gone_after + 1, "probed again after the process was gone");
        assert_eq!(clock.pauses_at.borrow().len(), gone_after, "paused after the process was gone");
    }
}

#[test]
fn a_process_gone_at_the_last_probe_before_the_deadline_is_ok() {
    // The probe reports gone only once the clock has reached 59 s.
    let clock = Clock::new(Duration::from_secs(1));
    let deadline = clock.now() + KILL_WAIT;
    let result = wait_gone(|| clock.at.get() < Duration::from_secs(59), || clock.now(), || clock.pause(), deadline);
    assert_eq!(result, Ok(()));
}
