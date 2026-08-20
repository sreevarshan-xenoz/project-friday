//! 7E scheduler + skill consultation — the controlled recurrence seam.
//!
//! The scheduler is deliberately boring: it only decides *when* a bounded
//! `AutonomousTick` may start. It owns no cognition — no planning, memory, skills,
//! verification, or tools. It answers one question: "is a tick allowed to start now?"
//! and, if so, hands a [TickTrigger] to the caller, who runs the tick.
//!
//! Safety boundaries (budget, concurrency, cooldown, permissions, runtime/cost/action
//! limits) are defined as a configurable [SchedulerPolicy] seam — no hardcoded
//! production numbers. Scheduler failures are isolated from tick failures, and every
//! scheduled tick gets its own `trace_id` (the tick creates it), so episode boundaries
//! stay intact.

use crate::tick::{TickOutcome, TickTrigger};
use std::sync::{Arc, Mutex};

/// A clock the scheduler reads. Injectable so tests are deterministic (no real sleep).
pub trait Clock: Send + Sync {
    fn now(&self) -> u64;
}

/// Deterministic fake clock advanced explicitly by tests. `Clone` so the test driver
/// and the scheduler can share the same clock.
#[derive(Clone)]
pub struct FakeClock {
    t: Arc<Mutex<u64>>,
}

impl FakeClock {
    pub fn new() -> Self {
        Self {
            t: Arc::new(Mutex::new(0)),
        }
    }
    pub fn advance(&self, ms: u64) {
        let mut t = self.t.lock().expect("clock poisoned");
        *t += ms;
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for FakeClock {
    fn now(&self) -> u64 {
        *self.t.lock().expect("clock poisoned")
    }
}

/// Permission policy seam. The scheduler consults it before allowing a tick; it knows
/// nothing about skills, lessons, or tools.
pub trait PermissionPolicy: Send + Sync {
    fn allowed(&self, trigger: &TickTrigger) -> bool;
}

/// Allows every trigger (default for hermetic tests).
pub struct AllowAllPolicy;
impl PermissionPolicy for AllowAllPolicy {
    fn allowed(&self, _trigger: &TickTrigger) -> bool {
        true
    }
}

/// Denies every trigger.
pub struct DenyAllPolicy;
impl PermissionPolicy for DenyAllPolicy {
    fn allowed(&self, _trigger: &TickTrigger) -> bool {
        false
    }
}

/// The decision a scheduler makes about the next tick.
#[derive(Debug, Clone, PartialEq)]
pub enum SchedulerDecision {
    /// A tick is permitted to start; carry this trigger into the tick.
    Allowed(TickTrigger),
    /// Not now — try later (budget/concurrency/cooldown/interval).
    Deferred(String),
    /// Not at all (disabled / permission denied).
    Stopped(String),
}

/// Configurable safety policy. `0` means "unbounded" for the limit fields, so tests
/// set explicit values rather than relying on hardcoded production numbers.
pub struct SchedulerPolicy {
    pub enabled: bool,
    /// Minimum gap between ticks (cadence), in ms. `0` = no interval.
    pub interval_ms: u64,
    /// Max ticks permitted within `window_ms`.
    pub max_ticks_window: u64,
    /// Window length for the tick budget, in ms.
    pub window_ms: u64,
    /// Max ticks in flight concurrently.
    pub max_concurrent: u64,
    /// Per-tick runtime limit, in ms (`0` = unbounded).
    pub max_runtime_ms: u64,
    /// Per-window cost limit (`0` = unbounded).
    pub max_cost_window: u64,
    /// Max actions a single tick may take (`0` = unbounded).
    pub max_actions_per_tick: u64,
    /// Cooldown after a failed tick, in ms.
    pub cooldown_ms: u64,
    pub permissions: Box<dyn PermissionPolicy>,
}

impl SchedulerPolicy {
    pub fn new(permissions: Box<dyn PermissionPolicy>) -> Self {
        Self {
            enabled: true,
            interval_ms: 0,
            max_ticks_window: u64::MAX,
            window_ms: u64::MAX,
            max_concurrent: u64::MAX,
            max_runtime_ms: 0,
            max_cost_window: 0,
            max_actions_per_tick: 0,
            cooldown_ms: 0,
            permissions,
        }
    }

    /// Policy that permits everything (used by hermetic tests).
    pub fn allow_all() -> Self {
        Self::new(Box::new(AllowAllPolicy))
    }

    /// Admission seam for the per-tick runtime/cost/action limits. The executor calls
    /// this with measured values; `0` policy values mean "no limit".
    pub fn within_limits(&self, runtime_ms: u64, cost: u64, actions: u64) -> bool {
        (self.max_runtime_ms == 0 || runtime_ms <= self.max_runtime_ms)
            && (self.max_cost_window == 0 || cost <= self.max_cost_window)
            && (self.max_actions_per_tick == 0 || actions <= self.max_actions_per_tick)
    }
}

/// The scheduler. Holds only operational state (budgets, concurrency, cooldown) and
/// decides when a bounded tick may start. It never runs a tick itself.
pub struct Scheduler {
    policy: SchedulerPolicy,
    clock: Box<dyn Clock>,
    ticks_in_window: Vec<u64>,
    in_flight: u64,
    last_tick_at: Option<u64>,
    last_failure_at: Option<u64>,
}

impl Scheduler {
    pub fn new(policy: SchedulerPolicy, clock: Box<dyn Clock>) -> Self {
        Self {
            policy,
            clock,
            ticks_in_window: Vec::new(),
            in_flight: 0,
            last_tick_at: None,
            last_failure_at: None,
        }
    }

    fn prune_window(&mut self, now: u64) {
        self.ticks_in_window
            .retain(|t| now.saturating_sub(*t) <= self.policy.window_ms);
    }

    /// Decide whether a tick may start at `now`. Scheduler-only policy; no cognition.
    pub fn decide(&mut self, now: u64) -> SchedulerDecision {
        if !self.policy.enabled {
            return SchedulerDecision::Stopped("scheduler disabled".into());
        }
        let trigger = TickTrigger::Scheduled;
        if !self.policy.permissions.allowed(&trigger) {
            return SchedulerDecision::Stopped("permission denied".into());
        }

        self.prune_window(now);

        if self.ticks_in_window.len() as u64 >= self.policy.max_ticks_window {
            return SchedulerDecision::Deferred("budget exhausted".into());
        }
        if self.in_flight >= self.policy.max_concurrent {
            return SchedulerDecision::Deferred("concurrency limit".into());
        }
        if let Some(last) = self.last_failure_at {
            if self.policy.cooldown_ms > 0 && now.saturating_sub(last) < self.policy.cooldown_ms {
                return SchedulerDecision::Deferred("cooldown active".into());
            }
        }
        if let Some(last) = self.last_tick_at {
            if self.policy.interval_ms > 0 && now.saturating_sub(last) < self.policy.interval_ms {
                return SchedulerDecision::Deferred("interval not elapsed".into());
            }
        }

        // Record the schedule time now — the driver will call `mark_started` before
        // running, but cadence/interval is measured from this decision.
        self.last_tick_at = Some(now);
        SchedulerDecision::Allowed(trigger)
    }

    /// Record that a permitted tick has started (call before running it).
    pub fn mark_started(&mut self, now: u64) {
        self.in_flight += 1;
        self.last_tick_at = Some(now);
        self.ticks_in_window.push(now);
    }

    /// Record a finished tick. A failed tick starts the cooldown; the scheduler itself
    /// always survives (failure isolation).
    pub fn mark_finished(&mut self, outcome: TickOutcome, _now: u64) {
        self.in_flight = self.in_flight.saturating_sub(1);
        if matches!(outcome, TickOutcome::Failure) {
            self.last_failure_at = Some(self.clock.now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_cadence_respects_interval() {
        let mut p = SchedulerPolicy::allow_all();
        p.interval_ms = 10_000;
        let clock = FakeClock::new();
        let mut sched = Scheduler::new(p, Box::new(clock.clone()));

        assert!(matches!(sched.decide(clock.now()), SchedulerDecision::Allowed(_)));
        // Too soon — interval not elapsed.
        clock.advance(5_000);
        assert!(matches!(sched.decide(clock.now()), SchedulerDecision::Deferred(_)));
        // Interval elapsed.
        clock.advance(5_000);
        assert!(matches!(sched.decide(clock.now()), SchedulerDecision::Allowed(_)));
    }

    #[test]
    fn concurrency_limit_defers() {
        let mut p = SchedulerPolicy::allow_all();
        p.max_concurrent = 1;
        let clock = FakeClock::new();
        let mut sched = Scheduler::new(p, Box::new(clock.clone()));

        assert!(matches!(sched.decide(clock.now()), SchedulerDecision::Allowed(_)));
        sched.mark_started(clock.now());
        // A second concurrent tick is deferred.
        assert!(matches!(sched.decide(clock.now()), SchedulerDecision::Deferred(_)));
        sched.mark_finished(TickOutcome::Success, clock.now());
        // After it finishes, allowed again.
        assert!(matches!(sched.decide(clock.now()), SchedulerDecision::Allowed(_)));
    }

    #[test]
    fn budget_exhaustion_defers() {
        let mut p = SchedulerPolicy::allow_all();
        p.max_ticks_window = 2;
        p.window_ms = u64::MAX;
        let clock = FakeClock::new();
        let mut sched = Scheduler::new(p, Box::new(clock.clone()));

        sched.mark_started(clock.now());
        sched.mark_finished(TickOutcome::Success, clock.now());
        sched.mark_started(clock.now());
        sched.mark_finished(TickOutcome::Success, clock.now());
        // Budget (2) reached.
        assert!(matches!(sched.decide(clock.now()), SchedulerDecision::Deferred(_)));
    }

    #[test]
    fn failure_triggers_cooldown_then_recovers() {
        let mut p = SchedulerPolicy::allow_all();
        p.cooldown_ms = 10_000;
        let clock = FakeClock::new();
        let mut sched = Scheduler::new(p, Box::new(clock.clone()));

        sched.mark_started(clock.now());
        sched.mark_finished(TickOutcome::Failure, clock.now());
        assert!(matches!(sched.decide(clock.now()), SchedulerDecision::Deferred(_)));
        clock.advance(9_999);
        assert!(matches!(sched.decide(clock.now()), SchedulerDecision::Deferred(_)));
        clock.advance(1);
        assert!(matches!(sched.decide(clock.now()), SchedulerDecision::Allowed(_)));
    }

    #[test]
    fn disabled_scheduler_stops() {
        let mut p = SchedulerPolicy::allow_all();
        p.enabled = false;
        let clock = FakeClock::new();
        let mut sched = Scheduler::new(p, Box::new(clock.clone()));
        assert!(matches!(sched.decide(clock.now()), SchedulerDecision::Stopped(_)));
    }

    #[test]
    fn permission_denied_stops() {
        let p = SchedulerPolicy::new(Box::new(DenyAllPolicy));
        let clock = FakeClock::new();
        let mut sched = Scheduler::new(p, Box::new(clock.clone()));
        assert!(matches!(sched.decide(clock.now()), SchedulerDecision::Stopped(_)));
    }

    #[test]
    fn within_limits_seam() {
        let mut p = SchedulerPolicy::allow_all();
        p.max_runtime_ms = 1_000;
        p.max_cost_window = 5;
        p.max_actions_per_tick = 3;
        // Within all limits.
        assert!(p.within_limits(500, 2, 1));
        // Exceeds runtime.
        assert!(!p.within_limits(1_500, 2, 1));
        // Exceeds cost.
        assert!(!p.within_limits(500, 9, 1));
        // Exceeds actions.
        assert!(!p.within_limits(500, 2, 9));
    }

    #[test]
    fn scheduler_survives_tick_failure_isolation() {
        let mut p = SchedulerPolicy::allow_all();
        p.cooldown_ms = 0;
        let clock = FakeClock::new();
        let mut sched = Scheduler::new(p, Box::new(clock.clone()));
        // A tick fails; scheduler must remain usable, not die.
        sched.mark_started(clock.now());
        sched.mark_finished(TickOutcome::Failure, clock.now());
        clock.advance(1);
        assert!(matches!(sched.decide(clock.now()), SchedulerDecision::Allowed(_)));
    }
}
